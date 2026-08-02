# Tasks: ranked-impact

## Rank engine (foundation)

- [ ] Add a projection loader in the graph store: in-workspace symbols only, edge kinds `uses`/`imports`/`type_hierarchy`, multi-kind pairs collapsed to one edge, returned in stable canonical-identity order.
- [ ] Unit tests for the projection: an external symbol is absent, a `contains` edge is absent, a pair related under two kinds yields one edge, an isolated in-workspace symbol is present in the node universe.
- [ ] Implement global PageRank by power iteration (damping 0.85, 40 iterations, uniform weights, uniform dangling redistribution) over the projected graph, visiting nodes and edges in canonical-identity order.
- [ ] Unit tests for the rank: on a handcrafted graph a hub outranks a leaf, a symbol used by two hubs outranks one used by three leaves, and two runs over the same input produce bitwise-identical score vectors.

## Ordering and token identity

- [ ] Add the `RANK_VERSION` constant (documented as covering algorithm, projection, damping, iterations, and weights; bumping requires a migration note per the retrieval-scoring governance rule).
- [ ] Add the ranked comparator `(depth asc, rank desc via total order, kind_order, id)` alongside the existing unranked comparator, selected by order mode.
- [ ] Extend `PageIdentity` with the order mode and `RANK_VERSION`; unit tests: identities differing only in order mode hash differently, and identities differing only in rank version hash differently.

## Command surface

- [ ] Add `--order <ranked|unranked>` (clap value enum, default `ranked`) to `trace` and `impact`; reject it as a usage error, before any traversal, when `trace` is invoked with a relation other than `dependents`, naming where the selector applies.
- [ ] Integration test: `trace --relation references --order ranked` exits through the usage-error code with a message naming the `dependents`/`impact` context, and an unknown order value enumerates the valid set.
- [ ] Bump SURFACE_VERSION, refresh `tests/fixtures/surface_manifest.snapshot.json` as a reviewed diff, and update the surface-manifest test expectations.
- [ ] Update `trace` and `impact` self-description text to present the ranked ordering as a structural-importance heuristic; test asserts the help/self-description carries it.

## Answer envelope and render

- [ ] Add the single `ordering` field (`ranked` | `unranked`) to the dependents/impact answer envelope, populated on every answer including typed-empty ones.
- [ ] Add the human-render note line for ranked answers ("rows within a distance layer are ordered by a structural importance heuristic"); no heuristic presentation for unranked answers.

## Behavior tests (integration, per contract)

- [ ] Ranked ordering — `trace dependents`: fixture where two distance-1 dependents differ in codebase-wide importance; ranked order puts the widely-depended-upon one first (Rust and Python fixtures).
- [ ] Ranked ordering — distance primacy: a more-important distance-2 dependent never precedes a distance-1 row.
- [ ] Ranked ordering — `impact`: a multi-symbol diff whose same-distance dependents differ in importance orders each layer most-important-first (separate write-site from `trace`).
- [ ] Set invariance: the same trace under `ranked` and `unranked` returns identical symbols, distances, kinds, aggregates, and horizon disclosure — only row order differs.
- [ ] Truncation head: with a result limit smaller than the distance-1 layer, the detailed rows are the most important distance-1 dependents and truncation is disclosed with a continuation.
- [ ] Continuation under ranked order: resuming with the token yields the next rows of the ranked sequence with no repeats or omissions (pagination write-site).
- [ ] Token binding: a token issued under `ranked` presented with `--order unranked` is rejected as a usage error; a token whose identity was hashed under a different rank version is rejected (both write-sites of the identity check).
- [ ] Unranked ordering: explicit `--order unranked` reproduces the pre-change `(depth, kind, id)` order byte-for-byte against existing fixtures.
- [ ] Disclosure: JSON carries `ordering: ranked` by default and `ordering: unranked` on request; a typed-empty dependents answer still carries the field; human render carries the heuristic note only for ranked (render write-site distinct from JSON).

## Performance and dogfood gates

- [ ] Benchmark on the four persistent clones (httpx2, Flask, ripgrep, fd): `ranked` vs `unranked` on hub-symbol traces and a multi-symbol impact; record timings in the change notes; gate: ranked within 100 ms of unranked everywhere.
      If the gate fails, stop and surface — the fallback (persisted rank) is a separate change.
- [ ] Pre-registered quality check: before running, write into the change notes the expected top-of-layer dependents for fixed sampled subjects on each clone; run, record pass/fail per prediction; a failing result feeds the edge-kind-weighting follow-up decision rather than silent retuning.

## Docs

- [ ] README: document the ranked default, the `--order` selector, the ordering disclosure, and that ranking never changes the answer set.
