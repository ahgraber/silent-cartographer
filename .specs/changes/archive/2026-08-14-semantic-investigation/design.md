# Design: semantic-investigation

## Context

- The chunk-tier substrate (chunk-tiers, 2026-07-15) persists location/signature/interface/body content per symbol at build time, explicitly as the corpus for this change; its design recorded the binding SearchCorpusConstraint: index non-nested content only — leaf bodies plus container interface tiers — never nested full bodies.
- North-star principle: precision is the moat, recall is additive — semantic search sits on top of the exact graph; the calibration principle forbids confidently-wrong answers, which shapes the rank-only and labeling decisions below.
- Project constraints: CPU-only default runtime, Rust implementation, single SQLite store, models cacheable locally, replayable derived artifacts.
- Scale assumption: one repo per store, roughly 100–10k corpus entries.
- The CLI envelope (flags, exit taxonomy, bounding/cursor, output discipline) and the calibrated output contract are written universally in the baseline specs; the new commands inherit them without delta.

## Decisions

### Decision: RetrievalArchitecture

**Chosen:** static-embedding hybrid — a static code-embedding model (`potion-code-16M-v2`, 256-dim, MIT) for the semantic signal, SQLite FTS5 BM25 for the lexical signal, fused with reciprocal rank fusion (RRF).

**Rationale:** a static model embeds by token-vector lookup plus averaging — no transformer forward pass — so build-time embedding of a 10k-entry corpus costs well under a second on CPU and queries are sub-millisecond; the official `model2vec-rs` crate is pure Rust with no C++ dependency.
Dense-only retrieval quality of static models is mediocre, but fused with BM25 the hybrid demonstrably reaches strong repo-scale NL-search quality (Semble reports NDCG@10 0.854 with exactly this pairing), and the lexical signal independently guarantees the spec's documentation-words and split-name-words matching scenarios.

**Alternatives considered:**

- Small code transformer via ONNX Runtime (e.g. gte-modernbert-base): substantially better dense retrieval, but adds a C++ runtime dependency, ~150 MB model cache, and much slower indexing; held as a quality step-up if hybrid quality disappoints.
- Late-interaction retrieval (per-token vectors, MaxSim scoring — e.g. LateOn-Code-edge): best quality per parameter, but multi-vector storage and scoring machinery is the heaviest option; **this is the named fallback architecture** if dogfood use shows the static hybrid underperforming.
- Lexical-only (FTS5 alone): no meaning-based matching; fails the find-by-meaning story.

### Decision: EmbeddingModelBundled

**Chosen:** the model is compiled into the binary (`model2vec-rs` supports loading from bytes); no network path exists at runtime.
The payload itself is not committed: a committed manifest pins the upstream repository, revision, and per-file SHA-256, and `build.rs` fetches missing files from the pinned revision and verifies every file against the manifest on every build.
The upstream MIT license text is committed beside the manifest, since the binary redistributes the weights.
The exported tokenizer serializes a 512-token truncation setting the model does not have (its card states a 1,000,000-token maximum); the loader removes it before constructing the tokenizer, guarded by a tail-sensitivity regression, so the no-token-limit contract holds in fact.

**Rationale:** hermetic at runtime — `search` works anywhere the binary runs; the model identity is pinned by the binary itself, which makes the versioning policy below trivially sound; replay is deterministic; and the repository carries ~1 KB of provenance instead of ~33 MB of weights, with the checksum gate making a drifted or corrupted payload a build failure rather than a silent model change.
The cost is that the first build needs network (or a pre-populated payload directory); the build error names the manifest and the manual recovery.

**Alternatives considered:**

- Download-and-cache from Hugging Face Hub: leaner binary, but first use needs network, adds a runtime supply-chain surface, and creates a "worked yesterday, fails offline today" trust papercut.
- Bundled default plus an external-model override path: deferred; introduced only together with the fallback architecture if it is ever adopted.

### Decision: CorpusRender

**Chosen:** each corpus entry's content is a deterministic per-symbol render: name split into words, symbol kind, module-path words, signature, own documentation, then the entry's source text (leaf body, or container interface content).

**Rationale:** a static model averages every token into one vector, so salient words must be present as words — `withBackoff` only matches "with backoff" if split; evidence from ColGREP shows a structured metadata preamble measurably improves code retrieval.
Every render field comes from data the graph already resolves, which is the advantage of graph-native chunking over tree-sitter-guess chunking.
The same render, identifier-split, feeds the FTS5 lexical index, so both signals see the same evidence.

**Alternatives considered:**

- Raw tier text only: simplest, but NL queries then connect only to words that happen to appear in code, and compound names are unfindable by their words.
- Render plus graph context (callee/caller names folded into the embedding text): no direct evidence in hand, entangles vectors with edge changes, and graph signals serve better as an explicit rank signal in the similarity spike; deferred.

### Decision: LeafContainerCorpusSplit

**Chosen:** leaves (symbols containing no other persisted symbol) contribute their own source; containers contribute their interface tier only.

**Rationale:** discharges the SearchCorpusConstraint — enclosure duplicates content at every level (a type's body contains its methods; a module's body is its whole document), and indexing nested bodies double-counts lexical statistics, returns duplicate hits, and doubles embedding cost.
Container interface tiers keep module and type documentation findable without their members' bodies riding along.
Name-only symbols are excluded outright: a symbol whose every tier degraded to its bare name token (scip-python persists function parameters this way) carries no content beyond its identity, yet its short render wins BM25 length normalization and displaces content-bearing candidates — dogfood on httpx2 showed parameter rows outranking the logic they parameterize.
The exclusion also keeps clone keys honest: a one-token "leaf" would otherwise share its keys with every same-named (formatting) or single-identifier (substitution) parameter in the workspace.
Containment counts contributing children only: a Python function encloses its persisted parameter symbols, and counting those would reclassify every parameterized function as a container and evict its body from the corpus — the exact content the SearchCorpusConstraint exists to index.

**Alternatives considered:**

- Exclusive text (span minus children's spans) for containers: admitted by the constraint, but interface tier is already persisted, cheaper to reuse, and carries the documentation that matters for retrieval.

### Decision: CloneKeyNormalization

**Chosen:** both equivalence keys are hashes over the leaf symbol's token sequence from the build's syntax tree, comments and whitespace excluded.
The formatting-insensitive key hashes the token sequence as spelled.
The substitution-insensitive key hashes the sequence with each distinct identifier and each distinct literal value replaced by its first-occurrence ordinal (de Bruijn-style canonicalization, one ordinal space per token class), which makes two sequences collide exactly when one is a consistent one-to-one substitution of the other's names and literal values — full Type-2 clone coverage in the literature's taxonomy.
Keys exist for leaves only; containers carry none.
The variant-clone marker this key backs asserts identical token structure, never behavioral equivalence — substituted literals change behavior — and the human rendering states the marker in those terms.

**Rationale:** canonicalization by first-occurrence ordinal is deterministic, language-neutral over the token stream, and gives the exactly-when equivalence the spec asserts — including refusing inconsistent substitutions (merged identifiers or merged literals change the ordinal pattern).
Extending the same mechanism from identifiers to literals costs one more token class and covers the changed-constant copy (`retry(3)` vs `retry(5)`) that identifier-only normalization misses.
Container "clones" over interface text would assert body equivalence the key never examined — a confidently-wrong fact, so containers are excluded.
Behavioral-equivalence certification (executing both and comparing) is out of reach by design: it requires running arbitrary repo code, and the north star excludes c10r from being a test runner; the certainty tier claims only what static token structure proves.

**Alternatives considered:**

- Identifier-only substitution (literals must match): stricter, but misses the classic changed-constant clone for no savings — the mechanism is identical.
- Type-collapsed literals (every number one placeholder): erases the one-to-one consistency guarantee; `f(3, 3)` would match `f(3, 5)`.
- Hashing normalized source text (regex comment-stripping): fragile per language; the syntax tree already tokenizes correctly.
- AST-shape hashing: catches more clone variants but loses the exactly-when guarantee that makes the markers presentable as fact.

### Decision: VectorStorageSqliteVec

**Chosen:** vectors are stored and searched through the `sqlite-vec` extension, statically compiled into the binary, at a pinned version; similarity queries run as SQL KNN over its virtual table, tie-broken by canonical identity for deterministic order.
The extension caps one KNN request at 4096 candidates, so on a corpus above that size the vector signal ranks the nearest 4096 while the lexical signal still ranks the whole corpus — an honest candidate pool under the candidates-not-completeness framing, discovered when ripgrep's corpus first crossed the cap and guarded by a store-level regression.

**Rationale:** the store stays single-file SQLite while vector search stays in SQL — no hand-rolled SIMD scan to write and maintain, KNN composes with ordinary SQL filtering, and the storage layer is already positioned if corpus scale ever grows past brute force.
At ≤10k × 256-dim entries its brute-force scan is sub-millisecond, which is all this scale needs.
The extension is pre-v1 with a warned-unstable on-disk format, but the exposure is low: vectors are derived state wholly rebuilt each build, so a format change is absorbed by the standard schema-bump-and-rebuild recovery, never a migration.

**Alternatives considered:**

- Plain BLOB columns with an in-process brute-force scan: fewest dependencies, but hand-rolls storage and scan code the extension provides, and gains nothing at any scale.
- HNSW/usearch/LanceDB: ANN machinery or a second storage engine, unjustified below ~100k vectors.

### Decision: HybridFusionRRF

**Chosen:** `search` runs the vector signal and the FTS5 BM25 signal separately and fuses them with reciprocal rank fusion (`score(d) = Σ 1/(k + rank_i(d))`, k = 60), internally; ties break by canonical identity.

**Rationale:** RRF fuses ranks, not scores, so the two signals' incomparable score scales never need calibration; it is the pattern every surveyed shipped system uses; exact-identifier queries are rescued by the lexical signal and paraphrase queries by the vector signal with no mode flag.

**Alternatives considered:**

- Score blending (min-max normalization): needs per-corpus calibration and breaks on model change.
- Vector-only or lexical-only: each fails a class of real queries the other covers.

### Decision: RankOnlyAnswers

**Chosen:** answers expose order only — no cosine, BM25, or RRF numbers, no bands, no gap fractions.

**Rationale:** none of the available numbers is calibrated (cosine scale shifts per model, BM25 is corpus-dependent, RRF is rank arithmetic); exposing them invites downstream thresholds that silently break on every model bump — the confidently-wrong failure mode at one remove.
The order is the claim; the estimation marker states its grade.

**Alternatives considered:**

- Raw scores labeled uncalibrated: maximum information, but the label does not stop threshold abuse.
- Coarse bands or top-hit-relative fractions: re-import the calibration problem as band edges the contract must then keep stable.

### Decision: SemanticVersioningPolicy

**Chosen:** the semantic-index identity (embedding model identity + corpus definition version) is recorded in store provenance and carried on answers; any change to model or corpus definition ships with a schema-version bump.
No continuation-token binding to the semantic identity is added.

**Rationale:** the model is bundled, so an identity change can only arrive in a new binary, and bumping the schema version makes every stale store refuse wholesale through the existing incompatible-store contract — a continuation token can never survive into a changed-identity world, unlike the ranked-ordering case (RANK_VERSION), where ordering could change without any persisted state changing.

**Alternatives considered:**

- A separate semantic-version compatibility gate scoped to `search`/`similar`: finer-grained, but adds a second refusal taxonomy for a situation the schema bump already covers completely.

### Decision: BuildTimeEmbeddingWithHashSkip

**Chosen:** representations are computed during the build and persisted with it; an entry whose render is byte-identical to the prior build's is carried forward instead of re-embedded.

**Rationale:** build-time persistence matches the chunk-tier philosophy and the replayability rule; the rebuild-idempotence contract in the delta spec makes the hash skip a pure, unobservable optimization — carried-forward and recomputed representations are identical by construction (the model is deterministic and pinned).

**Alternatives considered:**

- Lazy embedding on first search: faster builds, but makes first-query latency unpredictable and violates the persisted-at-build replay discipline — and a static model makes build-time cost negligible anyway.

### Decision: SignatureDefaultDetail

**Chosen:** `search` and `similar` rows default to signature detail, unlike `trace`, which defaults to location.

**Rationale:** a trace's relation is often the whole answer — a list of names suffices; a ranked semantic result exists to be triaged, and a bare ranked list of names forces the follow-up `get` calls the detail knob exists to avoid.
A signature is one line and usually decisive.

**Alternatives considered:**

- Location default for uniformity with `trace`: consistent but self-defeating for ranked candidates.
- Interface default: more informative, materially more tokens per row; opt-in instead.

### Decision: SimilarHybridFloorWithSignalSpike

**Chosen:** `similar` ranks by the same two-signal RRF hybrid `search` uses — the vector signal (cosine neighbors of the subject's vector) fused with a lexical signal (BM25 of the subject's own render words against the corpus) — beneath the deterministic clone-certainty tier.
A time-boxed spike evaluates whether the remaining per-signal rank lists — signature-shape rank, graph-interaction rank, and a length-normalized token edit-distance rank computed over the clone-normalized token sequences as a re-rank of the vector signal's top candidates (banded computation with early exit once the distance bound is exceeded, so far-apart pairs fail fast) — demonstrate further lift over the two-signal floor; RRF needs ranks only, so no cross-signal weight calibration.
Edit distance lives here in the ranked tier rather than as a third certainty marker: any "close enough" cutoff would be a threshold in the contract, exactly what the rank-only posture forbids.
The spike's outcome decides whether the additional signals land in this change or a follow-up.

**Rationale:** the first spike round (vector + keyword RRF on ripgrep subjects) showed fusion tightens the neighborhood toward same-domain functional neighbors, and the fusion machinery already exists for `search` — so the two-signal hybrid is the floor, not a follow-up.
Each remaining signal is real design work, and evaluating it against the hybrid floor (rather than vector-only) is what answers whether it earns its complexity.

**Alternatives considered:**

- Vector-only floor: the originally proposed v1; superseded once the fusion spike showed lift and the user set the hybrid as the requirement.
- Composite scorer with weights now: weights over uncalibrated signals are the threshold trap inside the ranker; rejected.

### Decision: TaskGroundedEval

**Chosen:** the primary evaluation runs the new verbs inside real workflows — simplify/refactor/code-review-style passes over the persistent dogfood clones (httpx2, Flask, ripgrep, fd) — judging whether the answers are useful in the task.
If a ground-truth set proves wanted, independent subagents author queries while operating in those workflow mindsets, and a separate grading pass adjudicates each query against full source and the answers; adjudicated pairs become the reusable set.

**Rationale:** the verbs exist to serve those workflows, so the workflows are the test; workflow-born queries also structurally avoid docstring leakage (queries authored by paraphrasing corpus docstrings would grade the model on its own answer sheet).
Numeric latency budgets are set at apply time; the dogfood judgment decides whether the fallback architecture is pursued.

**Alternatives considered:**

- A pre-registered hit@k benchmark authored by reading the repos: leakage-prone and optimizes for a lab metric rather than in-task usefulness.

## Architecture

```text
build:
  syntax pass ──► tier content ──► corpus assembly (leaf own-content | container interface)
                                        │
                                        ├─► render (name words, kind, path words, signature, docs, text)
                                        │       ├─► static model ──► vector row (sqlite-vec) ─┐
                                        │       └─► identifier-split text ──► FTS5 row        ─┤ persisted with build,
                                        └─► leaf token stream ──► clone keys (fmt, ident)     ─┘ wholly superseded

query:
  search <NL query> ──► vector signal (brute-force cosine) ─┐
                    └─► lexical signal (FTS5 BM25)          ─┴─► RRF ──► ranked rows ──► detail projection ──► envelope
  similar <ref|pos> ──► resolve subject ──► clone-key tier ▸ hybrid tier (vector + BM25 over the
                                            subject's render words, RRF) ──► ranked rows ──► detail projection ──► envelope
```

## Risks

- **Static-model retrieval quality disappoints in dogfood use**: the lexical signal carries exact-identifier and doc-word queries regardless; the fallback architecture (late interaction) is pre-planned with its own gate — no mid-change architecture churn.
- **Binary size growth from the bundled model (~16–32 MB)**: measured at apply time — the release binary grew from 6.2 MB to 44.3 MB (+38.1 MB: 32.5 MB f16 safetensors, 1.0 MB tokenizer, the rest tokenizer/ndarray/sqlite-vec code).
  The weights ship as f16, the only artifact upstream publishes; an i8 variant would be a locally derived re-quantization needing its own fidelity validation for a ~16 MB saving, so f16 is chosen and i8 revisited only if binary size becomes a complaint.
- **FTS5 tokenization mangles identifiers**: the lexical index is built over the pre-split render, not raw source, so the default unicode61 tokenizer sees plain words; verified by the split-name spec scenario.
- **Cosine ties produce nondeterministic order**: all orderings tie-break by canonical identity; the calibrated output contract's determinism scenario covers it.
- **sqlite-vec is pre-v1 and its on-disk format may change**: the version is pinned in Cargo.lock and vectors are derived state rebuilt wholly each build, so a format change is absorbed by schema-bump-and-rebuild, never a data migration.
- **Clone keys collide across unrelated code (hash collision)**: keys are content hashes; collision probability is negligible at repo scale, and markers are scoped to a single store built by a single-language backend.
- **`model2vec-rs` or model licensing surprises**: crate and model are MIT; the committed manifest pins the upstream revision and per-file checksums and the committed license file carries the MIT attribution, so upstream changes cannot drift in silently and redistribution stays properly credited.
