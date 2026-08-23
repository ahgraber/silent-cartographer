# Design: semantic-chunking

## Context

- The corpus substrate (chunk-tiers, 2026-07-15) persists location/signature/interface/body content per symbol, and the retrieval layer (semantic-investigation, 2026-08-14) turned it into one render, one vector, and one lexical document per passage, fused by RRF.
  This change splits the vector side of that pairing only.

- The embedding model is static: `potion-code-16M-v2`, distilled from `nomic-ai/CodeRankEmbed` and contrastively fine-tuned on CornStack (query, document) pairs.
  It pools by averaging token rows, with no attention, no positions, and no mask.
  Its model card records "Max sequence length | 1,000,000 tokens (static, no limit in practice)" — the limit that matters is distributional, not architectural.

- Measured corpus shape across the five dogfood workspaces (13,444 passages): render tokens p50 79, p90 289, p99 1,823, max 332,199; 4.2% of passages hold 55% of all corpus text.
  Full measurements and sources in `discussion.md`.

- The build (internal-efficiency, 2026-08-20) holds one parsed document per source for the whole of `ingest` and asserts that it parses each document at most once.
  The splitter's syntax level therefore reads that prepared tree rather than parsing, and a re-parse would fail the existing parse-count assertion rather than merely cost time.

- The same change made a build over unchanged inputs skip analysis entirely, comparing sources, analyzer, and declared environment.
  The chunk parameters are a new input to that comparison; see `ParametersRecordedInIdentity`.

- Vocabulary: this change settles a three-level hierarchy, aligned with the information-retrieval convention rather than invented for the occasion.

  | Level | Term         | What it is                                                                                                                             |
  | ----- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
  | 1     | **document** | a source file — the baseline's existing meaning (`document_path`, "spans the module's whole document")                                 |
  | 2     | **passage**  | what one corpus-contributing symbol contributes: its own content if it is a leaf, its interface tier if it contains other contributors |
  | 3     | **chunk**    | the text actually handed to the embedding model — a passage's header plus a bounded run of its content — one vector each               |

  **corpus** keeps its ordinary meaning of a collection, so `semantic_corpus`, `CORPUS_DEFINITION_VERSION`, and the adjective "corpus-contributing" are unaffected; `SourceCorpus` and `PreparedCorpus` keep theirs as collections of documents.
  Renaming only the unit is what stops "corpus" competing with itself.

  The rename reaches two baseline scenario titles — `Module entry excludes member bodies` and `Every corpus entry is represented` — which become `Module passage excludes member bodies` and `Every passage is represented`.
  Their bodies are unchanged in substance, so nothing is lost, but a name-comparing check cannot tell a retitle from a deletion; each MODIFIED block therefore carries a `modified-removes` marker naming the old title, recording the retitle rather than claiming the evidence was dropped.

  One property does not carry over from the IR convention: **chunks tile their passage, but passages do not tile their document.**
  A symbol that contributes nothing yields no passage, and a container's passage excludes its members' bodies so no content is counted twice.

- The north-star principles that bind here: **Calibration over coverage** (a ranking derived from a contaminated vector is confidently wrong), **Safety by structure, not by caveat** (absence is typed, never implied — so content is split, never dropped), and **Precision is the moat**.

## Decisions

### Decision: PaddingDisabledAtLoad

**Chosen:** null the tokenizer's `padding` configuration when the model is constructed, beside the `truncation` removal already there.

**Rationale:** padding exists to make a batch into a rectangular tensor for a neural forward pass, and transformer stacks pair it with an attention mask so pad positions contribute nothing.
A static model has no forward pass, no tensor, and no mask; `model2vec` filters only the `[UNK]` id when pooling, so every pad row lands in the mean.
The `[PAD]` row is not neutral (norm 0.0072 against a median row norm of 8.96), so enough of them dominate.
Measured on Faker: peak heap in the embedding step falls 22,395 MiB → 122 MiB, wall 89.1 s → 1.1 s, and batched vectors become bit-identical to singly-embedded ones (0 of 5,794 differ).
Batching is unaffected — a static model pools a ragged batch, and the unpadded whole-corpus call is the fastest of every variant measured.

**Alternatives considered:**

- Tokenize in c10r and pool the ids ourselves: same vectors, but re-implements the model's own encode path and takes on its correctness.
- Restore the tokenizer's truncation: bounds the pads, keeps the contamination (1 real token + 511 pads still lands at cosine 0.9116 against its own text, measured), and silently drops content — refused by story `nothing-unrepresented`.
- Smaller batches: reduces peak memory, leaves every vector still a function of its batch.
- Patch `model2vec-rs` to skip pad ids: correct upstream, but this change cannot wait on it; worth reporting regardless.

### Decision: ChunkSize

**Chosen:** a chunk size of **512 tokens**, bounding the whole chunk — the passage header and the content run together — operator-configurable.

**Rationale:** the size must sit above the ordinary passage and below the dilution ceiling.
At 512, roughly 96% of passages are a single chunk, so the change is a no-op for the bulk of the corpus and its effects are attributable to the tail.
Published chunking evidence favours smaller chunks — precision and IoU roughly double moving 400 → 200 tokens at near-flat recall — but that evidence comes from prose corpora with windowed splitting and no contextualizing header, and the same study's weak-model repeat found overlap necessary for recall at small sizes. 512 keeps headroom for both the header (p50 29 tokens, p90 78) and an overlap the operator can add without re-tuning the size.

Bounding the whole chunk rather than the content alone is what makes the flag mean what it says: `--chunk-size 512` promises the model receives at most 512 tokens.
Bounding content alone would let a chunk reach 512 plus the header — up to 768 under `HeaderBudget` — which is a number no operator supplied.
The cost projections below are unaffected, because the measurement already charged the header against the chunk size.

**Alternatives considered:**

- 256 tokens: closest to Semble's 750-character target against this same model, and the precision-favouring choice; costs 1.7x vectors instead of 1.27x and leaves little room for header plus overlap.
- 1024 tokens: 1.1x vectors, but only 1.5% of passages would split, leaving most of the dilution problem unaddressed.
- A character size (as Semble uses): simpler to explain, but the model's actual limit is in tokens and characters-per-token varies 3.13–4.00 across our own corpora.
- Bound the content and let the header ride on top: keeps a full header on every chunk unconditionally, at the cost of a flag whose value is not the value that reaches the model.

### Decision: OverlapDefault

**Chosen:** an overlap default of **64 tokens**, operator-configurable — set by the boundary probe (below) run against 0, 64, and 128 before the change closed, under the rule pre-registered with the probe: a null result keeps no overlap, a material lift moves the default.

**Rationale:** overlap's published benefit is repair of spans severed by a boundary.
Unit-aligned splitting is designed to not sever them, so the default was not guessable in either direction: overlap did nothing for the strong model in the published evidence and moved weak-model recall 77.1 → 82.4, and ours is a 16M static model.
The probe answered: 64 lifted rank-1 on boundary-spanning dense queries 70.5% → 77.3% (top-5 91.5% → 94.8%), consistently across all five workspaces, at +2.6% vectors — far below the 1.34x projection, because packing absorbs carried units. 128 added +2.3pp rank-1 for +6.7% vectors: half the marginal gain at more than double the carried cost, so 64 is the knee.
The user ratified the move on the probe's result.

**Alternatives considered:**

- Ship no overlap: the pre-probe null hypothesis; refuted by the measured lift.
- Ship 128: takes the full measured lift, at diminishing returns per carried token.
- Omit the knob and revisit later: every revisit costs a full re-embed, and `eval-localization` would have to introduce the parameter before it could baseline it.

### Decision: UnitAlignedSplitting

**Chosen:** divide a passage's content into units and pack units into chunks up to the chunk size, by a separator hierarchy applied only when the level above overshoots:

1. syntax nodes from the tree the build already parses, descending into a node that alone exceeds the room available;
2. prose boundaries — paragraph, then sentence, then line — inside a unit the syntax oracle reports as one indivisible construct (a docstring, a doc-comment run, an interface tier);
3. a plain token window, as the terminal fallback for content offering no boundary at all.

Overlap is taken as whole trailing units of the preceding chunk, up to the overlap budget; a trailing unit that does not fit contributes nothing.
Within a run of windowed chunks (level 3) there are no units, so overlap there is plain token overlap.

**Rationale:** boundary-respecting chunks are what make a chunk a coherent excerpt rather than a fragment, and the recursive form is what the published comparison favours — the recursive splitter beat fixed-token splitting on both the strong and the weak model at every size at or below 400 tokens.
Levels 2 and 3 are not optional refinements: a 40-line docstring is a single string node with no children, and a minified line or a single enormous literal has no boundary at any level.
Our largest passages are data tables, where level 1 already yields the right units (list elements).

**Alternatives considered:**

- Line windows only: language-independent and trivial, but splits mid-expression and mid-sentence, which is the failure the unit alignment exists to prevent.
- Fixed token windows: simplest, and measurably the weaker splitter in the published comparison.
- Semantic (embedding-similarity) splitting: another embedding pass per passage at build time, for a gain the published comparison does not clearly show over recursive splitting.

### Decision: HeaderBudget

**Chosen:** the passage header carried on every chunk is itself bounded, to at most half the chunk size.
When it exceeds that, its documentation tail is trimmed to fit; the name words, kind, module-path words, and signature are trimmed only as the terminal fallback below.

When the identity head alone exceeds the budget — no documentation left to trim — the head itself is tail-trimmed to the budget: the hard size bound outranks header wholeness, and the sacrifice runs from the least identifying part inward (the signature's parameter run first; name words, kind, and module-path words last, at the head's front).
The size bound is the change's load-bearing promise and is never traded away; header wholeness is a means to contextualization, and the trimmed head keeps its most identifying part.

**Rationale:** the header is normally cheap (p50 29 tokens) but not always — p99 is 277 tokens and the worst observed is 7,011, which at a 512 chunk size would leave no room for content at all.
The identity-bearing head is what makes a mid-passage chunk recognizable as that symbol's content; the documentation tail is the expendable part because it also appears, in full, in the passage's own first chunk and in the lexical index.

**Alternatives considered:**

- Let the header push the chunk over the size: keeps every header whole, but reintroduces unbounded vectors through the back door for exactly the symbols with the most documentation.
- Trim the header from its front: destroys the identity signal, which is the reason the header exists.
- Emit a header-only chunk when the header alone exceeds the size: an extra vector representing no content.

### Decision: MultiVectorStorage

**Chosen:** `semantic_corpus` keeps exactly one row per symbol — one passage — carrying the render and feeding the lexical index unchanged.
The vector table gains one row per chunk, each carrying its passage's identity and its ordinal within that passage.

**Rationale:** the passage row is the symbol-keyed thing the graph, the lexical leg, and the clone keys all hang off; only the dense representation is plural, so only it changes shape.
Keeping the lexical document whole also preserves BM25's length normalization over the passage, and keeps a symbol-keyed retrieval leg that chunk counts cannot crowd.

**Alternatives considered:**

- One passage row per chunk: would multiply the symbol-keyed row that `search`, `similar`, clone keys and the corpus contract are all defined over, for no gain on the lexical side.
- Chunk vectors in a separate table joined at query time: an extra join for the same information the vector row can carry directly.

### Decision: BestChunkScoring

**Chosen:** a symbol's dense score is the maximum over its passage's chunks; for `similar`, the score is the maximum over pairs of the subject's chunks and the candidate's.
Deduplication to one row per symbol happens before fusion.

**Rationale:** maximum is the aggregation that makes splitting neutral for ranking — a symbol is as relevant as its most relevant part, and gains nothing from being long.
Any additive aggregation (sum, or count-weighted) rewards symbols for holding more chunks, which would re-introduce the length bias this change exists to remove, in a new form.
Maximum over pairs is also the aggregation a later late-interaction stage generalizes (MaxSim), so the query path does not have to be re-shaped for it.

**Alternatives considered:**

- Mean over chunks: re-averages what splitting just separated, restoring dilution at the ranking layer.
- Sum, or top-k sum: rewards length directly.
- Score only a passage's first chunk: cheap, and reintroduces "content late in a long symbol is unfindable".

### Decision: KeepSqliteVecKnn

**Chosen:** keep the sqlite-vec KNN query for the dense leg, request the candidate pool in chunks, and deduplicate to symbols by best chunk.
Record a revisit trigger rather than pre-building for scale.

**Rationale:** measured, the extension's KNN is the faster of the two paths at every corpus size we have (8.60 ms at 5,794 vectors, against 34.08 ms to load the same vectors into Rust plus 0.88 ms to score them) — the arithmetic is negligible and the cost is moving bytes out of SQLite, which the extension already does well.
The extension's `k` ceiling of 4,096 is a result-count limit on an exhaustive scan (`vec0Filter_knn_chunks_iter`), not an index budget, and after splitting only one dogfood workspace exceeds it — Faker, at 8,697 chunks, which already exceeds it today at 5,794 passages.
Chunk concentration is mild in ordinary repos (max chunks per passage: 10 in httpx2 and flask, 54 in ripgrep, 32 in fd) and severe only in generated data tables (681 in Faker, 16.6% of a full pool).

**One collision to keep straight:** `sqlite-vec` uses "chunk" for its own internal blocks of vectors — hence the shadow tables `semantic_vectors_chunks` and `semantic_vectors_vector_chunks00`, and the scan function named above.
Those are a storage detail of the extension and never our unit of text.
The store's schema carries a comment saying so, and our own `DEPENDENTS_FRONTIER_CHUNK` — a query-batching constant, a third unrelated sense — is renamed to `..._BATCH` so that within c10r's own code "chunk" has exactly one meaning.
The BM25 leg is symbol-keyed and unaffected, so a crowded dense pool degrades one signal of two rather than the answer.

**Alternatives considered:**

- Scan and score in Rust: measured 4x slower end-to-end at our scale; revisit only if per-symbol aggregation cannot be expressed against the pool.
- An ANN index: `sqlite-vec` 0.1.9 is brute-force only by design (its ANN tracking issue plans IVF then DiskANN, and sets HNSW aside as ill-suited to SQLite shadow tables); the brute-force wall is around 1M vectors, ~115x our largest corpus, and today's ~1 s query wall is dominated by model load.
  ANN would trade approximate recall — which the calibration principle would require disclosing — for latency we do not need.
- libSQL's native DiskANN index: a SQLite fork rather than an extension, so adopting it replaces the store engine wholesale.

**Revisit trigger:** a workspace that is not data-table-shaped exceeding the 4,096 ceiling, or `sqlite-vec` shipping a supported ANN index.

### Decision: ParametersRecordedInIdentity

**Chosen:** record the chunk size and the overlap with the build, as part of the semantic-index identity beside the embedding model identity and the corpus definition version.
The parameters are compared at both points where a build decides to reuse prior state: a build whose parameters differ from the recorded ones is not already current, and within a build that runs, a passage's prior vectors are carried forward only when its render **and** the parameters are unchanged.

**Rationale:** the build carries an unchanged passage's vector forward when its render is unchanged; under different parameters the same render yields different correct vectors, so carry-forward without a parameter comparison would persist vectors from a regime the store no longer describes.
The currency check is the same reasoning one level up, and is the one that bites first: it skips before any corpus is assembled, so without the parameters in its comparison a rebuild at a new chunk size would report the index already current and never reach carry-forward at all.
Recording the parameters also makes an experiment self-describing: two stores built for comparison state which regime each is.
A version bump cannot express this, because the values are the operator's, not the release's.

**Alternative rejected here:** rely on `--force` for a parameter change.
That makes the correct outcome depend on the operator remembering, and silently answers from the wrong regime when they do not — the failure mode the calibration principle exists to prevent.

**Alternatives considered:**

- Fold the parameters into the corpus definition version: cannot represent a user-chosen value.
- Disable carry-forward entirely: correct but wasteful, re-embedding an unchanged corpus on every build.

### Decision: ChunkParametersAreNotAgentReachable

**Chosen:** the MCP `build` tool does not expose the chunk parameters.
They are supplied on the command line or not at all, and an agent-initiated build therefore runs at whatever the compiled-in defaults are.

**Rationale:** the parameters decide what a stored vector represents, and the store records them as part of its identity precisely because that is a regime, not a preference.
An agent calling `build` has no basis for choosing a regime and no way to learn which one the operator chose, so a knob on that surface offers it nothing it can reason about while making the operator's decision reachable from a caller who is not accountable for it.
The baseline contract permits the omission: `Tool schemas match the installed surface` constrains tools to declare only real options, never to declare every option.

**Consequence, accepted:** an agent-initiated build over a workspace last built at non-default parameters re-derives the whole index at the defaults, and records them.
Nothing refuses and nothing discloses, because the currency check correctly observes a different regime and rebuilds into it.
This is a limitation of where the values come from, not of the comparison, and the durable fix is a project-scoped setting the agent inherits rather than overrides — proposed as the `c10r-settings` change.

**Alternatives considered:**

- Expose the parameters on the MCP tool: hands an agent a knob whose consequences it cannot evaluate, and makes the regime agent-changeable, which is the failure this decision exists to prevent.
- Fall back to the store's recorded parameters when none are supplied: fixes the reset, at the cost of freezing every existing workspace out of a recommended default that later moves — the release's decision would stop reaching the operators who never re-supplied a value.
- Restate the currency inputs in the MCP tool's description: the description omits the chunk parameters from the inputs it lists, but the user's judgment is that an operator who sets a parameter receives the resulting rebuild as a version change rather than as a broken promise, so the wording is not load-bearing.

### Decision: BoundaryProbeAsOverlapInstrument

**Chosen:** measure overlap with a purpose-built boundary-sensitivity probe, not with the recorded dogfood queries.
The probe is generated from the corpus: for each passage that splits under the chunk size, take distinctive content spanning a chunk boundary, query for it, and record whether the owning symbol returns at rank 1 (and its rank otherwise).
Run at overlap 0, 64, and 128 over all five workspaces, and report the rank distribution per setting.

**Rationale:** the 14 recorded dogfood queries cannot resolve this question.
Overlap changes vectors only for passages that split — 4.2% of the corpus — and only two of the fourteen recorded targets are such passages (`Flask.handle_exception` at 750 tokens, `WalkParallel::visit` at 900); every other target is 178–479 tokens and unaffected.
Three identical result sets would be the likely outcome and would be evidence of nothing.
The probe instead measures the mechanism overlap exists to repair, with 567 candidate passages instead of 2, and is generated rather than hand-labelled.

**Pre-registered expectation:** if unit-aligned splitting does what it is designed to do, overlap should move boundary-spanning retrieval little, and the null result is the outcome that justifies a no-overlap default.
A material lift at 64 or 128 is the outcome that changes the default, and was recorded as such before the probe ran.

**Outcome:** the lift was material — see `OverlapDefault` above and the probe results in `notes.md`; the default moved to 64.

**Alternatives considered:**

- The recorded dogfood queries alone: no resolution, as measured above; they are still run, to catch regression rather than to decide overlap.
- Waiting for `eval-localization`: leaves the default unmeasured for however long that takes, against a knob whose change costs a full re-embed.

### Decision: VersioningAndRebuild

**Chosen:** bump the store schema version (the vector table's shape changes), the corpus definition version (the text a vector derives from changes), and the surface version (`build` gains two flags).
Existing stores are refused and rebuilt wholesale; no vector migration is written.

**Rationale:** derived state is rebuilt from source on every build, so a migration would carry no information a rebuild cannot re-derive, and the every-vector-changes property makes a partial migration meaningless.
The rename of the corpus's unit to **passage** rides on this bump rather than earning one of its own: the store is being replaced regardless, so the vocabulary lands at no additional cost to anyone's index.

## Architecture

```text
build:
  passage (one per corpus-contributing symbol, unchanged in derivation)
    ├─► render ──► identifier-split words ──► FTS5 row            (one per passage, unchanged)
    └─► header + content
            │
            ├─ header + content ≤ chunk size ──► one chunk
            └─ otherwise ──► units ──► packed chunks (+ overlap of whole trailing units)
                   │  1. syntax nodes (descend when oversized)
                   │  2. prose: paragraph ▸ sentence ▸ line
                   │  3. token window (terminal fallback)
                   ▼
            chunk = bounded header + run of content, whole chunk ≤ chunk size
                   ▼
            static model (no padding) ──► vector row per chunk, ordinal within its passage

  recorded with the build: embedding model identity + corpus definition version + chunk size + overlap
  parameters differ from the recorded ones ──► the index is not current, the build runs
  parameters differ, within a build that runs ──► no vector carried forward

query:
  search <NL query> ──► dense leg: KNN over chunks ──► best chunk per symbol ─┐
                    └─► lexical leg: FTS5 BM25 over whole renders ────────────┴─► RRF ──► rows
  similar <ref|pos> ──► subject chunks × candidate chunks ──► best pair ──► same fusion
```

## Risks

- **Build memory holds every pending chunk before persistence.**
  The embed step accumulates all pending chunk texts and vectors before the insert loop runs; at the shipped defaults that is tens of megabytes (whole-build peaks 0.6–2.3 GiB, analyzer-dominated), and the minimum chunk size bounds how far tiny parameters can amplify the count.
  Revisit shape if a real workspace pushes the accumulation into contention: embed and persist in bounded batches inside the same build transaction.
- **`similar` cost is linear in the subject's chunk count.**
  Best-pair scoring runs one KNN query per subject chunk; measured on Faker's 782-chunk generated data table: 12.1 s wall against a ~1 s baseline, with the next-worst symbol (138 chunks) around 2.5 s and every ordinary symbol unaffected (max 10–54 chunks across the four real repos).
  Accepted as slow-but-correct: a subject-side cap would reintroduce the content-late-in-a-long-symbol failure on the query side, for a query shape only generated data tables produce.
  Revisit trigger: chunky subjects becoming common outside generated tables — the fix shape is batching the subject's scans into one pass over the pool, which preserves best-pair exactly, never a cap.
- **A data-table workspace crowds the dense candidate pool.**
  Measured: Faker's largest symbol would hold 681 chunks, 16.6% of a full 4,096-slot pool, and its corpus already exceeds the ceiling today.
  Mitigation is structural rather than added: the symbol-keyed BM25 leg is unaffected and fused, so coverage degrades in one signal, not in the answer; the revisit trigger is recorded above.
- **Header repetition inflates build-time embedding work.**
  Projected 3.16M corpus tokens become 3.51M embedded tokens at the default (about 11%), and more with overlap.
  Acceptable against an embedding step that now costs ~1 s per corpus.
- **Splitting widens a long symbol's match surface**, so it can appear for more queries than before.
  Maximum-over-chunks keeps it from being _ranked_ higher for that reason, but the effect on precision is real and is what the dogfood regression pass watches.
- **The prose fallback is heuristic.**
  Sentence detection in doc comments has no oracle, and a bad split is a chunk that reads as a fragment.
  Bounded by only firing inside a construct the syntax tree reports as indivisible and larger than the room available.
- **The vocabulary rename touches spec text this change does not otherwise alter.** `Semantic corpus` and `Semantic answers are structurally labeled` are copied wholesale to change one word, and `sdd-sync` replaces the baseline from the copy — so a stale copy silently reverts baseline text.
  The guard is the MODIFIED-completeness check, which compares each delta block against the baseline requirement it replaces and refuses a block that drops one; it caught two scenarios dropped from `Semantic answers are structurally labeled` during verification.
  A word-level sweep cannot serve here: the loss it must catch is usually a clause or a scenario, not a term.
- **The boundary probe measures a mechanism, not user-visible quality.**
  Its lift licenses the shipped default but says nothing about end-to-end retrieval; that remains `eval-localization`'s question, and this change does not claim otherwise.
- **Every store must be rebuilt.**
  Unavoidable given every vector changes; the refusal is a version check rather than a silent re-read.
