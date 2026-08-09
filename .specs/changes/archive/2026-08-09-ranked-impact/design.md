# Design: ranked-impact

## Context

The dependents walk already materializes everything the ordering needs: a multi-seed, one-pass, breadth-first traversal produces `DependentRow { id, depth, kind }`, currently sorted by `(depth, kind_order, id)` in `src/graph/store.rs`.
Bounding and continuation already bind an answer to its query identity (`PageIdentity` in `src/query/page.rs`: query parameters plus index content-hash and analyzer provenance), and `is-tested` (applied, unsynced) established the pattern of a structural heuristic marker carried alongside provenance and freshness.
Repository governance requires any retrieval-scoring change to carry a migration/test plan and a version bump of the affected artifact; the ranking model introduced here is such an artifact.
This change is authored against the post-`is-tested` baseline and must sync after `is-tested` syncs; its delta touches none of the requirements `is-tested` adds or modifies.
Adding the order selector changes the command surface, so SURFACE_VERSION bumps and the surface manifest snapshot is refreshed.

## Decisions

### Decision: Structural importance is global PageRank

**Chosen:** Classic (non-personalized) PageRank by power iteration over the projected dependency graph (next decision), with edges oriented from dependent to dependency, so mass flows toward what is depended upon and widely-used symbols accumulate rank transitively.
Uniform teleport, damping 0.85, a fixed iteration count of 40, uniform edge weights, dangling mass redistributed uniformly.
Damping 0.85 is the standard value from the PageRank literature; 40 iterations bounds the residual near 0.85⁴⁰ ≈ 1.5×10⁻³, far below the score separations that make one symbol observably more load-bearing than another.
Rank is a proxy for breakage cost, and every surface presents it as a structural-importance heuristic — never as measuring cost or risk.

**Rationale:** The ranking must differentiate within a distance layer, which is exactly where seed-personalized scores go flat: with deduplicated edges and a single seed, every direct dependent receives an equal mass share.
Global PageRank scores vary across the whole codebase, are independent of the query, and capture transitive load-bearing-ness — a symbol used by three hubs outranks one used by ten leaves.
A fixed iteration count (rather than a convergence threshold) keeps the computation deterministic and simple; near-equal scores order deterministically through the trailing comparator keys, and no score is ever published, so residual numerical noise is never presented as a meaningful distinction.

**Alternatives considered:**

- Seed-personalized PageRank: rejected for v1 — collapses to distance order for single-seed traces (the common `trace dependents` case); its genuine signal (breadth of contact with a multi-symbol diff) can return later as a fusion term under a rank-model version bump.
- Degree centrality (count direct dependents): no transitive weight; a symbol used by ten leaves would outrank one used by three hubs.
- Katz/eigenvector centrality: same family, fewer robustness properties, no advantage over PageRank here.
- Betweenness centrality: measures bridge-ness, not load-bearing-ness, and costs O(V·E) — wrong semantics at the wrong price.

### Decision: Rank graph projection

**Chosen:** The rank computation runs over the induced subgraph of in-workspace symbols only.
Edge kinds `uses`, `imports`, and `type_hierarchy` participate; `contains` is excluded — enclosure is structure, not dependency.
Edges are unique on `(kind, src, dst)`, so one symbol pair may appear under several kinds; such pairs collapse to a single unweighted edge, consistent with uniform weights.
Isolated in-workspace symbols remain in the universe and receive teleport mass; dangling nodes (no outgoing projected edges) redistribute their mass uniformly.
The universe is always the whole projected graph, never the query's reachable subgraph.

**Rationale:** External symbols (a `class` the store persists alongside in-workspace ones) never appear as dependents — they have no workspace definition — yet including them would drain and absorb rank mass, distorting the scores of the symbols that do appear in answers.
Collapsing multi-kind pairs keeps "depends on" a single fact regardless of how many relation kinds express it.
Ranking over the whole graph keeps a symbol's importance identical in every answer; a reachable-subgraph universe would re-introduce within-layer flatness for small radii and let the same symbol carry different ranks in different queries.

**Alternatives considered:**

- Including external symbols: distorts workspace scores for nodes that can never be rows; rejected.
- Parallel edges per kind (a two-kind pair counts double): an accidental weighting scheme decided by extraction detail rather than by design; rejected while weights are uniform.
- Ranking over the reachable subgraph only: cheaper, but semantically wrong per the story and unstable across queries.

### Decision: Rank is computed per query over the projected graph, not persisted

**Chosen:** Load the projected edge list and run the iteration at query time; no schema change, no persisted rank artifact, no in-process cache in v1.
Budget: on the four persistent dogfood clones, a `ranked` dependents/impact query completes within 100 ms of the same query `unranked`, measured on the largest clone's hub symbols.

**Rationale:** Per-query computation keeps rank trivially consistent with the index the query reads — no staleness coupling, no migration.
The measured dogfood indexes bound the cost: the largest (ripgrep) holds 5,092 in-workspace symbols and 26,006 non-`contains` edges, so 40 iterations is roughly one million edge-operations — sub-millisecond arithmetic plus a few milliseconds of edge-list load.
The budget is enforced by a benchmark task, not assumed.

**Alternatives considered:**

- Persist a rank column at index build: cheapest query path, but couples rank staleness to index staleness and adds a schema migration now to optimize a cost the measurements do not show; it remains the named fallback if the budget fails, as its own change with a schema/version plan.
- In-process cache keyed on index generation: helps only multi-query processes; the CLI is one query per process.

### Decision: Comparator and determinism

**Chosen:** The `ranked` ordering sorts by `(depth asc, rank desc, kind_order, id)`; the `unranked` ordering keeps the existing `(depth, kind_order, id)` comparator unchanged.
Rank values are f64 compared via total order; the iteration visits nodes and edges in stable canonical-identity order so floating-point summation order is fixed.

**Rationale:** IEEE arithmetic is deterministic given a fixed operation order, and the trailing `(kind_order, id)` keys make the sort total even under exact score ties, so identical inputs always produce identical output — satisfying the baseline calibrated-output determinism contract without restating it.

**Alternatives considered:**

- Quantizing scores or bucketing near-ties: introduces an arbitrary epsilon knob and manufactures the very ties it claims to resolve; unnecessary when no score is published and near-tie order is deterministic and disclosed as heuristic.

### Decision: Selector surface, rank-model version, and token binding

**Chosen:** A closed-vocabulary flag `--order <ranked|unranked>` on `trace` and `impact`, default `ranked`, accepted only where the ordering is defined — `trace` rejects it as a usage error for relations other than `dependents`, before any traversal, naming where the selector applies.
A `RANK_VERSION` constant identifies the ranking model — algorithm, projection, damping, iteration count, and weights together; any change to any of them bumps it, with a migration note, satisfying the retrieval-scoring governance rule.
The order mode and `RANK_VERSION` join `PageIdentity` as a single sealed ordering-identity value whose only production constructor stamps the current `RANK_VERSION` itself, so a site that binds the mode cannot fail to bind the version — a clause no runtime test can check, since the version is a compile-time constant within one binary.
A continuation token is therefore rejected across an ordering switch or a scoring change by the same path that rejects any parameter mismatch — never resumed against a reordered sequence with silently skipped or repeated rows.

**Rationale:** Reuses the established closed-flag and token-identity machinery; a selector silently ignored on unranked relations would be a lie of omission; `PageIdentity` today binds only query parameters and index identity, so without `RANK_VERSION` an old token would validate across a release that changed scoring and resume wrong.

**Alternatives considered:**

- A bare negation flag (`--no-rank`): saves nothing — the disclosure enum needs both tokens anyway — and departs from the surface's enum-with-nameable-default pattern (`--detail`).
- Accepting `--order` on all relations as a no-op: violates the rejections-name-valid-alternatives discipline and trains callers on a flag that does nothing.
- Binding only the order mode into the token: leaves cross-release scoring changes able to reorder a paginated sequence mid-flight; rejected.

### Decision: Disclosure shape

**Chosen:** The answer envelope carries a single structural `ordering` field with exactly the selector's two values, `ranked` or `unranked`; heuristic-ness is a documented property of the `ranked` value, stated by the human render's note line ("rows within a distance layer are ordered by a structural importance heuristic") and by the command's self-description, mirroring the `is-tested` marker line style.

**Rationale:** One enum cannot encode a contradictory state (a mode-plus-boolean pair could); a distinct field (not the `is-tested` convention-classification marker) keeps that requirement's prohibition true — that marker speaks to answer content, this one to ordering.
No numeric score is serialized anywhere: scores order rows within a layer and are discarded.

**Alternatives considered:**

- A `{mode, heuristic}` object: the boolean duplicates what the mode already implies and permits contradictory encodings; rejected.
- Reusing the `is-tested` heuristic-grade marker: conflates content trust with ordering trust and contradicts its "resolved relations carry no marker" clause.
- Exposing normalized scores: publishes a number the tool cannot stand behind as fact; rejected in proposal scope.

## Architecture

```text
                     ┌──────────────────────────────┐
                     │ SQLite index                 │
                     └──────┬───────────────┬───────┘
                            │               │
     projected edge list    │               │ seeds (subject | diff)
     (in-workspace; uses/   │               │
     imports/type_hierarchy;▼               ▼
     kinds collapsed) ┌────────────────┐  ┌──────────────────┐
                      │ global PageRank│  │ dependents walk  │
                      │ (fixed iters)  │  │ (existing, BFS)  │
                      └───────┬────────┘  └────────┬─────────┘
                              │ rank: id → f64     │ rows (id, depth, kind)
                              └─────────┬──────────┘
                                        ▼
                         sort by order mode:
                           ranked:   (depth, rank desc, kind, id)
                           unranked: (depth, kind, id)   [existing]
                                        ▼
                         bound / continuation
                         (order mode + RANK_VERSION in PageIdentity)
                                        ▼
                         envelope + ordering disclosure (single enum)
                                        ▼
                              JSON  /  human render
```

## Risks

- **Per-query rank cost regresses on larger graphs**: the 100 ms budget is gated by a benchmark task on the four persistent clones (httpx2, Flask, ripgrep, fd), including hub-symbol traces; if the budget fails, the fallback is a persisted rank artifact as a follow-up change with a schema/version plan.
- **Rank quality disappoints (import noise dominates layers)**: gated by a pre-registered dogfood check — for fixed sampled subjects on all four clones, the expected top-of-layer dependents are written down in the change's notes before running, and pass/fail is recorded against those predictions; edge-kind weighting is the designated follow-up knob if it fails.
- **Sync-order coupling with `is-tested`**: this change's baseline assumptions include the `tests` relation and heuristic-marker requirement; sync `ranked-impact` only after `is-tested` syncs.
- **Snapshot churn**: the default-order change and the new envelope field touch human-render and surface-manifest snapshots; each snapshot update must be a deliberate, reviewed diff, not a blind re-record.
