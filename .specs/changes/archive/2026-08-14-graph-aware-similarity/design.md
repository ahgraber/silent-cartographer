# Design: graph-aware-similarity

## Context

- `similar` (from semantic-investigation) ranks candidates by a reciprocal-rank fusion of two content signals — the subject's vector neighbors and BM25 over the subject's render words — beneath a deterministic clone-certainty tier; answers are rank-only and carry the estimation marker.
- The dependency edges the new signal reads (`uses`, `imports`, `type_hierarchy`) are already persisted and contracted; this change adds no schema, corpus, or build-time work — the signal is computed entirely at query time.
- Continuation tokens bind the query parameters and the recorded index identity; the dependents ordering additionally binds its ranking-model version because its ordering can change without any persisted state changing.
  This change puts `similar` in that same position for the first time: a ranking-composition change arrives in a binary upgrade with no store change, so no schema bump or index-hash change protects a stale token.
- Rank-only posture carries over: no overlap score, graph distance, or neighbor count appears in answers.

## Decisions

### Decision: GraphNeighborhoodSignal

**Chosen:** the graph signal ranks candidates by overlap between the subject's and the candidate's dependency neighborhoods — the symbols reachable over one `uses`/`imports`/`type_hierarchy` edge in either direction.
Candidates with zero overlap are absent from the signal's rank list; the fusion carries them on the content signals alone.

**Rationale:** one-hop neighborhood overlap is the structural-sibling test: co-called functions share callers, same-seam implementations share the types they are wired through.
The signal has long code-domain precedent — shared-neighbor similarity over call/use graphs dates to Schwanke's Arch (ICSE 1991), which used it to cluster procedures and flag misplaced ones — so this is an established measure applied to an existing substrate, not a novel heuristic.
It reads only persisted edges, needs no new state, and absence-from-the-list is RRF's native way to express "this signal has no opinion" — which is exactly right for isolated symbols and for cross-implementation twins wired into disjoint halves of the workspace (the spec's near-copy scenario requires the content signals to carry those).

**Alternatives considered:**

- Multi-hop or PageRank-style structural similarity: heavier, and blurs the sibling question ("shares this subject's neighbors") into global importance, which the dependents ordering already answers elsewhere.
- Folding graph context into the embedding at build time: entangles vectors with edge churn, forces re-embedding on edge changes, and was already rejected in semantic-investigation's render decision.

### Decision: DegreeDampedOverlap

**Chosen:** the overlap score composes two published measures: the Resource Allocation index as numerator — each shared neighbor contributes `1 / degree(neighbor)` (Zhou, Lü & Zhang 2009) — normalized by the Jaccard denominator, the size of the two neighborhoods' union: `score = Σ_{n ∈ A∩B} 1/deg(n) ÷ |A ∪ B|`, ranked descending, ties broken by canonical identity.
Scores are computed in Rust over rows fetched in a fixed order, never summed inside SQL aggregation, so the floating-point summation order is deterministic.

**Rationale:** hubs leak in two directions, and each component damps one.
A ubiquitous symbol appearing as a _shared neighbor_ certifies nothing — everything touches it — and per-neighbor inverse-degree weighting is the best-evidenced damping in the link-prediction literature: Resource Allocation was the top local index in the Zhou/Lü benchmark lineage, ahead of its softer `1/log(degree)` variant (Adamic-Adar) precisely on hub-heavy sparse graphs, which dependency graphs are.
A hub appearing as a _candidate_ overlaps everyone through sheer neighborhood size; the Resource Allocation index alone does not damp this (a candidate's own degree never enters it), so the Jaccard denominator carries the spec's ubiquity-is-not-similarity clause.
The same benchmark literature counsels against endpoint normalization — Jaccard underperforms there — but its objective is predicting attachment, where hubs legitimately attach more; this contract forbids rewarding attachment breadth, and the code-domain literature sides with damping it: dependency-graph element ranking has shipped inverse-degree "specificity" damping (Robillard's Suade/TOSEM topology analysis), and software clustering has excluded "omnipresent modules" as stop-words since Rigi.
Because the two objectives genuinely pull apart, the dogfood pre-registered an A/B: the chosen form against the undamped-endpoint Resource Allocation numerator alone, judged on the hub-subject and hub-candidate probes, so the denominator keeps its place on evidence.
The A/B ran over the rebuilt clones and kept the denominator, but on weak grounds: it changed no answer head on any subject tested, and it did not prevent a crate root from reaching rank 1 for its own crate's central type.
Damping the score cannot fix the ranking, because the fusion reads ranks and not scores; the dogfood verdict and the two changes that would address it are in the change notes.
Both dampings are smooth functions of persisted counts: no threshold, no cap, no excluded kind — consistent with the rank-only posture, and no contract constant to keep stable.

**Alternatives considered:**

- Plain Jaccard (unweighted common neighbors over the union): the evaluation that motivated this change used it and observed crate/module hubs and ubiquitous types leaking into answer heads; the benchmark literature also ranks it below the per-neighbor-weighted indices.
- Adamic-Adar weighting (`1/log(degree)`): indistinguishable from Resource Allocation when shared-neighbor degrees are small, weaker exactly where degrees are large — the case the damping exists for.
- A neighbor-degree cap or excluding module-kind neighbors: both are thresholds or kind lists the contract would then have to keep stable, and both discard information a smooth weight preserves. (A degree cutoff over shared neighbors is documented as a near-lossless _speedup_ for already-damped indices; it stays available as a performance option, never as the damping itself.)

### Decision: QueryTimeComputation

**Chosen:** the signal computes in a fixed number of set-based store queries, never per candidate: the subject's neighborhood; every symbol touching any of those neighbors, with the shared neighbors and their degrees (one aggregation); and the neighborhood sizes of the surviving candidates (one aggregation).
The candidate universe is therefore the symbols at graph distance two from the subject, intersected with the corpus.

**Rationale:** per-candidate edge queries would scale with the corpus; the set-based form scales with the subject's two-hop neighborhood, which repo-scale stores answer in milliseconds through the existing edge indexes.
A hub subject widens the candidate set toward the corpus size, which is the same bound the content signals already rank against.

**Alternatives considered:**

- Precomputing neighbor sets or degrees at build time: derived state with supersession obligations, bought to optimize a query that is already cheap; rejected under the no-schema-change scope.

### Decision: FusionCompositionUnchanged

**Chosen:** the graph signal enters the existing reciprocal-rank fusion as a third rank list, alongside the vector and lexical signals, under the same fusion constant; the clone-certainty tier, the subject exclusion, the detail projection, and the estimation labeling are untouched.

**Rationale:** RRF combines ranks, so a third list needs no calibration against the first two; every property the baseline contract asserts about `similar`'s answer shape survives verbatim, which is what keeps this change a MODIFIED ordering clause rather than a new surface.

**Alternatives considered:**

- Weighting the graph signal above or below the content signals: weights over uncalibrated signals are the threshold trap; if dogfood shows the signal needs more or less influence, that is a composition change and ships under a version bump like any other.

### Decision: SimilarityRankingVersion

**Chosen:** a new `SIMILARITY_RANK_VERSION` constant, starting at 1, sealed into the `similar` page identity through the same sealed-pair mechanism the dependents ordering uses — the production constructor stamps the constant itself, so no call site can bind the ranking without its version.
It is independent of the dependents ordering's ranking-model version.

**Rationale:** the two constants version independent models — a change to the dependents importance ranking must not invalidate `similar` tokens, nor the reverse — and the sealed-pair pattern already solved the untestable "bound the mode but not the version" gap, so it is reused rather than re-derived.
`search` gains no binding in this change: its composition is unchanged, and every ordering change it has faced so far arrives with a schema bump whose forced rebuild already invalidates tokens through the recorded index identity.
If a future change alters `search`'s composition without touching persisted state, that change adds the binding then, under this decision's pattern.

**Alternatives considered:**

- Sharing the existing dependents ranking-model constant: couples unrelated models; a bump for either surface would spuriously refuse the other's tokens.
- Binding nothing and accepting cross-version resume: silently serves page 2 of an ordering that no longer exists — the confidently-wrong failure mode the token identity exists to prevent.

## Architecture

```text
similar <ref|pos> ──► resolve subject ──► clone-key tier
                                          ▸ estimated tier: RRF over three rank lists
                                              vector signal   (subject's embedding neighbors)
                                              lexical signal  (BM25 over subject's render words)
                                              graph signal    (degree-damped neighborhood overlap,
                                                               distance-2 candidates, edges only)
                                          ──► ranked rows ──► detail projection ──► envelope
                                                              (token bound to SIMILARITY_RANK_VERSION)
```

## Risks

- **Test-code neighbors inflate sibling-ness**: test modules reference many symbols, so co-tested symbols share test neighbors; degree damping reduces this (busy test helpers have high degree) but may not eliminate it.
  Observe in dogfood; a test-aware weighting would be its own contract discussion.
- **Hub subjects widen the candidate set**: a subject that everything touches has a distance-2 set approaching the corpus.
  The set-based queries bound the cost, and the union denominator keeps the resulting ranks meaningful; dogfood includes a hub-subject probe.
- **Determinism under floating-point summation**: summation happens in Rust over rows in a fixed fetch order, and ranks tie-break on canonical identity, so identical stores yield identical orders; the calibrated-output determinism scenario covers it.
- **Sync-order coupling**: this change's MODIFIED block copies the `Similar-code lookup` requirement as semantic-investigation's sync will leave it in the baseline; if that change's spec text shifts before sync, this delta must be re-based on the final wording.
