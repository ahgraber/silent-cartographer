# Proposal: semantic-investigation

## Intent

Agents and developers can find code in c10r only by what it is named — exact resolution, substring `find` — but not by what it does.
When the name is unknown, they fall back to grep and burn context on unranked matches; when they are about to write a helper, they cannot cheaply ask whether similar code already exists.
This change adds meaning-based retrieval and similar-code lookup on top of the exact graph, using the chunk-tier substrate persisted for exactly this purpose, so recall becomes additive to the precision moat rather than a reason to leave the tool.

## User Stories

### Story: find-by-meaning

As a coding agent or developer, I want to find code by describing what it does in natural language, so that I can learn whether functionality already exists without knowing its name or exact keywords.

Ladders to: north-star outcome 6 (find by intent) and the success measure — an agent trusts c10r enough to stop grepping.

### Story: assess-similarity

As a coding agent or developer, I want the code most similar to a given symbol, ranked and retrievable at a chosen detail, so that I can assess duplication and consolidation opportunity before writing or refactoring code.

Ladders to: north-star outcome 6 (find by intent — navigating by what code does) and outcome 3 (structural understanding).

## Scope

**In scope:**

- code-graph: a per-build semantic corpus over non-nested symbol content (the SearchCorpusConstraint discharged as baseline contract); vector and lexical representations persisted at build time under the builds-wholly-supersede contract; clone-equivalence keys (formatting-insensitive and identifier-insensitive) per corpus symbol; semantic-index provenance recorded in the store; schema version bump.
- code-navigation: a `search` command — natural-language query in, ranked symbol rows out, per-row detail defaulting to signature; a `similar` command — exemplar symbol or source position in, ranked neighbor rows out, same detail axis; typed clone-certainty markers on `similar` rows; a structural heuristic marker and semantic-index provenance on every `search`/`similar` answer.
- Both commands inherit the existing envelope: bounded and resumable answers, calibrated output contract, exit taxonomy, human render.
- Task-grounded dogfood: the new verbs exercised through simplify/refactor/code-review-style workflows over the persistent dogfood clones.
- An optional spike: fusing several per-signal similarity rank lists (signature, keyword, vector, graph) by reciprocal rank fusion, to decide whether composite similarity ranking lands here or in a follow-up.

**Out of scope:**

- A repo-wide duplication survey or report verb — audits compose from targeted `similar` calls.
- Composite similarity ranking beyond the spike (unless the spike lands it).
- An MCP surface for the new verbs (no MCP surface exists yet; it arrives as its own change).
- Loading an external or user-supplied embedding model — introduced only if the fallback architecture below is ever adopted.
- Late-interaction (per-token multi-vector) retrieval — held as the fallback architecture should the static-embedding approach underperform in dogfood use — along with ANN indexes, GPU use, and any relevance-score exposure or similarity thresholds in answers.

## Approach

The retrieval architecture is a static-embedding hybrid, natively in Rust: a bundled static code-embedding model (`potion-code-16M-v2` via `model2vec-rs`, compiled into the binary — no network path) embeds a deterministic per-symbol render (split name, kind, module-path words, signature, own documentation, body text) at build time; embedding a static model is a token-lookup average, so build overhead is negligible and there is no token limit forcing sub-chunking.
Lexical retrieval rides SQLite FTS5 BM25 over the identifier-split render; `search` fuses the two signals with reciprocal rank fusion internally.
Vectors live in the index database through the sqlite-vec extension, statically compiled in at a pinned version; at repo scale (100–10k corpus entries) its brute-force scan is sub-millisecond, so no ANN machinery.
`similar` ranks by vector similarity over the same space, with deterministic normalized-token clone keys supplying the certainty tier above the heuristic ranking.
Answers are rank-only; the model and corpus-render identity fold into the schema version (a model change ships in a binary release and forces rebuild), and a semantic-index token binds continuation tokens and provenance.
An unchanged symbol's render hashes identically across builds, so re-embedding is skipped without observable effect.
Eval is task-grounded: run the verbs inside simplify/refactor/code-review workflows over the dogfood clones; if a ground-truth set proves wanted, independent subagents author queries in those workflow mindsets and a grading pass adjudicates answers against full source.

## Open Questions

- Whether composite similarity ranking (RRF over signature/keyword/vector/graph rank lists) lands in this change or a follow-up — decided by the spike's outcome.
  Resolved at apply time, in two steps: the first spike round showed the keyword signal lifts quality, so the two-signal hybrid (vector + BM25, RRF) became this change's `similar` floor; the second round showed the remaining signals (signature shape, graph interaction, edit distance) demonstrate further lift, and they land as a follow-up change carrying that evidence (see `notes.md`).
