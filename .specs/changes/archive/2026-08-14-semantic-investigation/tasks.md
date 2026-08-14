# Tasks: semantic-investigation

## Dependencies and model vendoring

- [x] Add `model2vec-rs` and `sqlite-vec` to Cargo.toml at pinned versions; verify `cargo build` links the sqlite-vec extension statically.
- [x] Vendor the `potion-code-16M-v2` model files at a pinned upstream revision and compile them into the binary via `model2vec-rs` from-bytes loading.
- [x] Write a smoke test: embedding the same text twice yields byte-identical vectors, and the model loads with no filesystem or network access.
- [x] Record the measured binary-size growth; choose f16 vs i8 model weights by size and a fixture spot-check that retrieval answers are unchanged.

## Corpus and render (code-graph)

- [x] Implement corpus assembly over persisted symbols: a leaf contributes its own source content; a container contributes its interface tier; each contributor exactly once.
- [x] Implement the render function: split name words, kind, module-path words, signature, own documentation, then the entry's content text.
- [x] Write tests for the Semantic corpus requirement: leaf own-content entry, container interface-only entry, module entry excluding member bodies, and no entry deriving from a container's full body.
- [x] Write a determinism test: two corpus assemblies over identical inputs yield identical entries.

## Clone-equivalence keys (code-graph)

- [x] Implement token-sequence extraction for leaf symbols from the build's syntax tree, excluding comment tokens.
- [x] Implement the formatting-insensitive key: hash of the token sequence as spelled.
- [x] Implement the substitution-insensitive key: hash with identifiers and literal values each replaced by first-occurrence ordinals in separate token-class spaces.
- [x] Write tests for the Clone-equivalence keys requirement, one per partition scenario: formatting variants share the formatting key; a consistently renamed copy shares only the substitution key; a literal-substituted copy shares only the substitution key; an inconsistent substitution (merged identifiers) shares neither; unrelated code shares neither; a container carries no key.

## Persistence and build integration (code-graph)

- [x] Bump the schema version constant; extend the store schema with the sqlite-vec virtual table, the FTS5 table over identifier-split render text, the clone-key columns, and the semantic-index identity in provenance.
- [x] Wire corpus assembly, render, embedding, lexical indexing, and clone-key computation into the build, persisted in the build's transaction scope.
- [x] Implement the render-hash skip: an entry whose render matches the prior build's carries its representations forward.
- [x] Write tests for the Semantic representations requirement: every corpus entry carries both representations; a vanished symbol's representations do not linger; an edited symbol's representations derive from the new content.
- [x] Write the rebuild-idempotence test through the hash-skip path: rebuild over unchanged sources yields representations identical to the first build's, with the skip active.
- [x] Write the skip-bypass test: an entry whose render changed is re-embedded, not carried forward, in the same build where an unchanged sibling is carried forward.
- [x] Write the failed-build test: a build that fails mid-way leaves the prior build's representations intact and authoritative.
- [x] Write the provenance test for the Semantic-index identity requirement: after a build, the store's provenance carries the model identity and corpus definition version.

## Query engine (code-navigation)

- [x] Implement the vector signal: query embedding plus sqlite-vec KNN, tie-broken by canonical identity.
- [x] Implement the lexical signal: FTS5 BM25 over the render text, query identifier-split the same way.
- [x] Implement RRF fusion (k = 60) over the two signals with deterministic tie-breaking.
- [x] Implement similar-ranking: cosine neighbors of the subject's vector, subject excluded, clone-certainty classes ordered ahead of unmarked rows (exact before variant), estimated order within each class.

## CLI commands (code-navigation)

- [x] Add the `search` command: natural-language query argument, detail flag defaulting to signature, wired through the fused ranking into the shared answer envelope.
- [x] Add the `similar` command: subject by symbol reference or source position through the existing resolution path, same detail axis, wired through similar-ranking.
- [x] Attach the estimation marker, semantic-index provenance, and clone markers to both commands' machine answers and human renders, with the candidates-not-completeness framing in render and self-description.
- [x] Bump the surface version constant for the new commands and flags.
- [x] Write tests for the Search by meaning requirement: documentation words match without the name; split-name words match as natural language; rows default to signature detail; detail does not change the result set; a symbol appears at most once; empty corpus is typed absence.
- [x] Write tests for the Similar-code lookup requirement: neighbors ranked with subject excluded; position resolves the enclosing symbol; ambiguous reference yields the typed candidate set; no other corpus symbol is typed absence.
- [x] Write tests for the Clone certainty requirement: verbatim copy marked exact and first; renamed copy marked variant and ordered between; literal-substituted copy marked variant not exact; edited copy unmarked; keyless subject yields no markers.
- [x] Write tests for the Semantic answers labeling requirement: machine marker and provenance present; human render frames candidates; empty answer keeps marker and scoped-absence framing; marker composes with staleness; clone markers rendered as fact distinct from the estimate.
- [x] Write envelope tests exercising the inherited bounded-answers contract through the new commands: result limit and truncation disclosure on `search`, continuation token resume on `search`, and a mismatched-token rejection on `similar`.
- [x] Write exit-taxonomy tests for the new commands: success on results, success on typed-empty, usage error on an invalid detail value, no-index code with no store present.

## Amendments from dogfood review

- [x] Exclude name-only symbols from the corpus: a symbol whose every persisted tier is exactly its own name token contributes no entry and carries no clone keys; shared eligibility predicate for corpus assembly and the clone-key pass.
  (Implementing this surfaced a second defect fixed with it: containment now counts contributing children only, so a Python function enclosing its parameter symbols stays a leaf and its body is indexed.)
- [x] Write tests for the name-only exclusion: unit (a name-collapsed source is ineligible) and build-level (a parameter-like symbol yields no corpus entry and no clone keys, while real declarations still contribute).
- [x] Rank `similar` by the two-signal RRF hybrid (vector signal plus BM25 of the subject's render words), beneath the unchanged clone-certainty tier.
- [x] Write a test for the hybrid similar ranking: the answer's order must equal the RRF fusion recomputed from the store's two signals — evidence the ranking is fused, which a vector-only ranking fails.
- [x] Dogfood round 2: non-leading queries (vocabulary disjoint from the definitions, abstract descriptions) over the rebuilt clones; record hits and misses; keep round 1's leading queries as the regression floor.
- [x] Spike (time-boxed): signature-shape, graph-interaction, and banded token edit-distance rank lists RRF-fused against the two-signal floor; record whether any demonstrates further lift. (Verdict: real lift on the consolidation use case; the signals land in a follow-up change with this evidence.
  See `notes.md`.)

## Documentation

- [x] Document `search` and `similar` in the README: what each answers, the detail default, the estimation marker, and the clone-certainty markers.

## Dogfood and spike

- [x] Rebuild the persistent dogfood clones with the new schema and run task-grounded passes: simplify/refactor/code-review-style workflows using `search` and `similar`; record the usefulness judgment and any fallback-architecture signal in the change notes.
  (All four clones: the Python passes ran in a second round once `scip-python` was installed; see `notes.md`.)
- [x] Spike (time-boxed, optional): RRF-fuse per-signal similarity rank lists — signature shape, keyword overlap, vector, graph interaction, banded length-normalized token edit distance over top vector candidates — against vector-only on the dogfood clones; record whether composite ranking lands here or as a follow-up. (Keyword + vector signals spiked; verdict: composite ranking is a follow-up change.
  See `notes.md`.)

## Review remediation

- [x] Disable the exported tokenizer's serialized 512-token truncation at model load; add the tail-sensitivity regression (content past token 512 affects the vector).
- [x] Make corpus containment identity-aware at equal spans (module side wins) and stop deriving a `contains` parent from a whole-document module span; add the equal-span module regression.
- [x] Replace the clone-key ordinal's linear scan with a hash-map ordinal.
- [x] Stop committing the model payload: gitignore it, commit a revision/checksum manifest and the upstream MIT license text, and fetch-and-verify the payload in `build.rs`.
- [x] Credit the embedded model in the README and rewrite the README as user-facing documentation.
- [x] Resolve the estimation-marker machine/human divergence on ambiguous and absent semantic answers, per the user's ruling.
  (Ruling: the derivation carve-out — an ambiguous or unresolved subject carries neither the marker nor the semantic-index provenance; delta requirement amended with the clause and two scenarios, engine and tests aligned.)

## Verify remediation

- [x] Test the unresolved-position arm of `similar --at` through the command handler: typed absence with no marker and no provenance.
- [x] Test the no-corpus-entry subject arm: a resolved external subject over a non-empty corpus answers typed-empty with the marker and provenance, provably through the no-representation return.
- [x] Assert the estimation marker and semantic-index provenance survive pagination, on the truncated first page and the resumed page.
- [x] Test that token overlap is not a precondition for membership: a query sharing no token with any render still returns every corpus symbol.
- [x] Repoint the staleness-composition test at `similar`, matching its scenario.
- [x] Assert clone keys follow the build in the edited-symbol test (supersession evidence).
- [x] Add the unsampled scenarios to the code-graph delta: contributor exactly-once, corpus determinism, failed-build arm, clone-key supersession.
- [x] Document the 4096-candidate KNN ceiling in the storage decision.
- [ ] At commit time, include `models/potion-code-16M-v2.manifest.json` and `models/potion-code-16M-v2.LICENSE` in the feat commit — a tree without the manifest cannot build.

## Verify round-2 remediation

- [x] Add `similar`'s detail scenarios (signature default, detail invariance) to the delta with an engine-level invariance test and a process-level default-detail test.
- [x] Reword the identity requirement to "with every build it persists" — the identity rides build metadata, and an unbuilt store has no provenance of any kind.
- [x] Pin the variant-class internal order (the literal-substituted variant precedes the renamed one) with its scenario.
- [x] Document why the failed-build test simulates via the transaction guard (no public failure path exists after the clear), the unreachable `estimated(None)` arm, and the current command list in the page-identity doc.
- Declined with reasoning recorded in the session: failure injection through `ingest` (test-only machinery for an identical mechanism), a non-optional `estimated` parameter (unrepresents an already-unreachable state), and the token-overlap clause reword (the clause forbids lexically-gated membership — vacuous only against the current implementation, not the contract space).
