# Proposal: ranked-impact

## Intent

Bounded impact answers currently order detail rows by distance, then edge kind, then identity — so within a distance layer, the order is arbitrary with respect to what matters.
On a change touching a widely-used symbol, the direct dependents most costly to break sit interleaved with trivial ones, and a bounded first page is an arbitrary slice of the layer.
This change keeps distance as the primary order and, within each distance layer, orders dependents by how load-bearing they are to the codebase as a whole — so the first bounded page shows the nearest, most load-bearing dependents first.
The answer set itself never changes — ranking is an ordering layer over the exact reverse-reachability closure, never a filter.

## User Stories

### Story: ranked-blast-radius

As a developer or agent whose change touches a widely-used symbol, I want the dependents within each distance layer ordered by how load-bearing each one is to the overall codebase, so that the first bounded page shows the nearest, most load-bearing dependents instead of an arbitrary slice of the layer.

> Ladders to north-star outcome 2 (Blast radius before change) and the search-inefficiency need: the bounded page an agent actually reads carries maximum signal.

### Story: unranked-order

As a consumer that wants ordering free of any ranking model, I want an order derived only from the answer's stable structural keys available on request, so that answers stay comparable across ranking-model changes and carry no heuristic component.

> Ladders to north-star outcome 5 (Calibrated trust); preserves an ordering derived from nothing but the graph's stable keys.

### Story: honest-ordering-label

As an agent consuming an impact answer, I want the answer to state which ordering is in effect and that relevance ordering is a heuristic, so that I treat rank as guidance and never mistake it for resolved semantic fact.

> Ladders to north-star outcome 5 (Calibrated trust): the ordering is disclosed, so trust in it can be calibrated.

## Scope

**In scope:**

- Deterministic ranked ordering of detailed dependent rows for `trace` over the `dependents` relation and for `impact`: distance remains the primary key; within each distance layer, rows order by the dependent's codebase-wide structural importance.
- Ranked as the default ordering; an order selector accepting `ranked` and `unranked`, where `unranked` reproduces the prior distance/kind/identity order exactly, and the selector is refused where its orderings are not defined.
- An ordering disclosure on every dependents/impact answer — machine answer and human render alike, empty answers included — naming the ordering in effect, with ranked ordering presented as heuristic.
- Continuation tokens bind the order selector and the ranking model's version as part of query identity and resume the selected ordering deterministically.
- Surface index update (SURFACE_VERSION bump) and surface manifest snapshot refresh for the new selector.

**Out of scope:**

- No numeric relevance score in any output — order only.
- No change to answer-set membership, depth bound, beyond-bound aggregates, or horizon disclosure.
- No ranking for other relations (`references`, `importers`, `implementers`, `contains`, `tests`).
- Focus-aware `find` ranking — deferred to its own change; this change builds the ranking machinery that would make it cheap later.
- No persisted or global rank artifact; ranking is computed per query from the index.

## Approach

Structural importance is global PageRank — uniform teleport, mass flowing along depends-on edges so widely-depended-upon symbols accumulate rank, transitively; a proxy for breakage cost, never presented as measuring it.
The rank graph is a defined projection of the index: in-workspace symbols only, dependency edge kinds only (enclosure excluded), multi-kind pairs collapsed to one edge, isolated symbols kept in the universe.
Seed-personalized PageRank was considered and rejected for v1: with deduplicated edges and a single seed, direct dependents score identically, so it collapses to distance order exactly where differentiation is needed most (rationale recorded in design.md).
Uniform edge weights in v1; edge-kind weighting (calls vs imports) is a dogfood-informed follow-up, not a v1 knob.
Determinism by construction: fixed damping factor, fixed iteration count, fixed summation order, and a total order (distance, then rank descending, then edge-kind order, then identity) so equal-score rows are stable.
Scores stay internal — they order rows within a layer and are then discarded.
The `unranked` selector reuses the existing comparator unchanged.
Continuation recomputes the ranking per page from the same query identity, which is deterministic, so resumption needs no persisted rank state; a rank-model version constant joins token identity so a token never resumes across a scoring change, satisfying the repository's retrieval-scoring versioning rule.
Rank computation is per query over the whole projected graph, inside a stated latency budget with a measured benchmark gate; persisting rank at index build is the named fallback if the budget fails.
This change is authored against the post-`is-tested` baseline and must sync after `is-tested` syncs.
