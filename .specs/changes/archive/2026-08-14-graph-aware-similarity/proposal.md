# Proposal: graph-aware-similarity

## Status: deferred, not executed (2026-08-14)

This change was built in full, measured against two real codebases, and reverted.
Its delta specs were never synced into the baseline; `similar` behaves as `semantic-investigation` left it.

The mechanism worked — the graph signal computed correctly and deterministically, every delta-spec scenario passed, and the suite was green — but the ranking it produced was worse than the ranking it replaced on four of six dogfood subjects.
Two structural causes, neither anticipated by the design and neither fixable inside it:

- **A symbol's own members flood its answer.**
  A method uses what its class uses and is called where its class is called, so it shares nearly the whole neighborhood and takes the signal's top ranks.
  Excluding `contains` from the neighborhood does not help: members inherit the container's neighborhood through their own edges.
- **The subject's own crate root outscores its real peers**, because it neighbors everything in the crate the subject lives in.

Damping cannot repair either, because reciprocal-rank fusion reads ranks and not scores: a hub-only overlap scoring 50× below a true sibling still lands near the top of a short list, and entering that list is worth about as much as a first-place content rank.
The pre-registered A/B confirmed the point from the other side — the union denominator changed no answer head anywhere, on either codebase.

The measurements are in `notes.md`; the reverted implementation is kept whole in `reverted-implementation.patch`.
Reviving this work means first deciding two things the design settled wrongly: whether the candidate set excludes the subject's containment closure, and whether the graph score scales its vote rather than only ordering the signal's own list.

## Intent

`similar` ranks candidates by content evidence alone: both of its signals — the embedding ranking and the word-overlap ranking — read the persisted render, so two symbols look similar only insofar as their text does.
But duplicated and parallel code is also recognizable by position — who calls it, what it uses: sibling functions invoked from the same call sites, parallel implementations wired into the same seams.
Content evidence ranks such siblings below incidental text matches (an error type's formatting impl, a same-vocabulary test) whenever their bodies read differently; a ranking built from dependency-neighborhood overlap surfaces them, because the codebase itself already treats them as a family.
Prototyping during the semantic-investigation change bore this out: of three candidate similarity signals evaluated against the shipped ranking, neighborhood overlap produced the largest improvement while preserving the top-ranked functional match (detail archived with that change's notes).
This change adds a graph-interaction signal to `similar`'s fusion, so structural position becomes similarity evidence alongside content.

## Sequencing

This change layers on the `similar` contract introduced by `semantic-investigation` and MODIFIES requirements that reach the baseline only when that change syncs.
It is applied after `semantic-investigation` completes sync; its delta specs are written against the post-sync baseline.

## User Stories

### Story: rank-by-position

As a coding agent or developer using `similar` to assess duplication, I want candidates that occupy the same structural position as the subject — shared callers, shared dependencies, the same wiring seams — ranked ahead of candidates that merely read alike, so that the consolidation candidates I act on are the ones the codebase itself treats as siblings.

Ladders to: north-star outcome 6 (find by intent — navigating by what code does) and outcome 3 (structural understanding); the graph is the product's existing moat, and this change makes the similarity ranking draw on it.

## Scope

**In scope:**

- code-navigation: the `similar` estimated ranking gains the subject's dependency-graph relationships as similarity evidence, fused with the existing content evidence; the ranking stays rank-only, the clone-certainty tier and the estimation labeling are unchanged.
- Ranking-version binding for `similar` continuation tokens: adding a signal changes the ordering without any persisted state changing, so a token issued under the old ranking must be refused rather than resumed against a differently-ordered sequence — the precedent set by the dependents ordering's ranking-model version, not the schema-bump precedent.
- Hub damping inside the graph signal: a widely-referenced symbol (a crate-root module, a ubiquitous type) shares neighbors with nearly everything, so undamped overlap leaks such symbols into answers where they are not similar — they merely touch everything.
- Task-grounded dogfood over the persistent clones, probing the three consolidation shapes the signal exists to serve: a co-called sibling family (several functions invoked from the same call sites), a same-container method family, and a cross-implementation twin pair (a sync/async or per-backend parallel implementation).

**Out of scope:**

- Signature-shape and token edit-distance similarity signals: prototyping found the first prone to flooding answers with same-shape-but-unrelated rows on generic signatures, and the second the most expensive to compute; both are revisited only after this signal's dogfood.
- Cross-module replicated helpers (the same function copy-pasted into several modules instead of shared): such copies occupy disjoint neighborhoods, so this signal deliberately says nothing about them — the clone-certainty tier and the content signals already carry that case, and a repo-wide replication survey remains its own future change.
- Any change to `search` — it has no subject, so it has no neighborhood to compare.
- Any schema or corpus change: the signal reads the dependency edges the store already persists.
- Relevance scores, similarity thresholds, or graph-distance disclosures in answers.
- An MCP surface.

## Approach

The graph signal enters `similar`'s existing reciprocal-rank fusion as a third rank list, computed at query time from the persisted dependency edges (`uses`, `imports`, `type_hierarchy`, both directions): candidates rank by neighborhood overlap with the subject, with overlap contributed by ubiquitous neighbors damped so hubs neither dominate their own answers nor leak into everyone else's.
The fusion combines ranks, so the new signal needs no calibration against the content signals, and a candidate absent from the graph (an isolated symbol) simply contributes no graph rank — the content signals still carry it.
The same consensus property protects cross-implementation twins: a sync/async pair shares few neighbors (each side is wired into its own half of the codebase), so the graph signal alone ranks the twin poorly, and the fusion must carry it on the content signals — a required behavior this change tests, not an accident.
Because the ordering can now change in a binary upgrade with no store change, the `similar` page identity binds a similarity-ranking version constant, mirroring how the dependents ordering binds its ranking-model version; the exact damping form and the version constant's granularity are design decisions.
Evaluation is task-grounded dogfood over the persistent clones against the three consolidation shapes named in scope, each asserted as a regression probe: the sibling family enters the answer's head, the twin stays at rank 1, and hub symbols stay out of heads they do not belong in.

## Open Questions

- The hub-damping form (neighbor-frequency weighting vs a degree cap vs excluding module-kind neighbors) — decided in design, targeting the two leak classes: crate/module hubs and ubiquitous types.
- Whether the similarity-ranking version shares the existing dependents ranking-model constant or carries its own — decided in design; they version independent models, which argues for its own.
