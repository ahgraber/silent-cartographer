# Tasks: ranked-impact

## Rank engine (foundation)

- [x] Add a projection loader in the graph store: in-workspace symbols only, edge kinds `uses`/`imports`/`type_hierarchy`, multi-kind pairs collapsed to one edge, returned in stable canonical-identity order.
- [x] Unit tests for the projection: an external symbol is absent, a `contains` edge is absent, a pair related under two kinds yields one edge, an isolated in-workspace symbol is present in the node universe.
- [x] Implement global PageRank by power iteration (damping 0.85, 40 iterations, uniform weights, uniform dangling redistribution) over the projected graph, visiting nodes and edges in canonical-identity order.
- [x] Unit tests for the rank: on a handcrafted graph a hub outranks a leaf, a symbol used by two hubs outranks one used by three leaves, and two runs over the same input produce bitwise-identical score vectors.

## Ordering and token identity

- [x] Add the `RANK_VERSION` constant (documented as covering algorithm, projection, damping, iterations, and weights; bumping requires a migration note per the retrieval-scoring governance rule).
- [x] Add the ranked comparator `(depth asc, rank desc via total order, kind_order, id)` alongside the existing unranked comparator, selected by order mode.
- [x] Extend `PageIdentity` with the order mode and `RANK_VERSION`; unit tests: identities differing only in order mode hash differently, and identities differing only in rank version hash differently.

## Command surface

- [x] Add `--order <ranked|unranked>` (clap value enum, default `ranked`) to `trace` and `impact`; reject it as a usage error, before any traversal, when `trace` is invoked with a relation other than `dependents`, naming where the selector applies.
- [x] Integration test: `trace --relation references --order ranked` exits through the usage-error code with a message naming the `dependents`/`impact` context, and an unknown order value enumerates the valid set.
- [x] Bump SURFACE_VERSION, refresh `tests/fixtures/surface_manifest.snapshot.json` as a reviewed diff, and update the surface-manifest test expectations.
- [x] Update `trace` and `impact` self-description text to present the ranked ordering as a structural-importance heuristic; test asserts the help/self-description carries it.

## Answer envelope and render

- [x] Add the single `ordering` field (`ranked` | `unranked`) to the dependents/impact answer envelope, populated on every answer including typed-empty ones.
- [x] Add the human-render note line for ranked answers ("rows within a distance layer are ordered by a structural importance heuristic"); no heuristic presentation for unranked answers.

## Behavior tests (integration, per contract)

- [x] Ranked ordering — `trace dependents`: fixture where two distance-1 dependents differ in codebase-wide importance; ranked order puts the widely-depended-upon one first (Rust and Python fixtures).
- [x] Ranked ordering — distance primacy: a more-important distance-2 dependent never precedes a distance-1 row.
- [x] Ranked ordering — `impact`: a multi-symbol diff whose same-distance dependents differ in importance orders each layer most-important-first (separate write-site from `trace`).
- [x] Set invariance: the same trace under `ranked` and `unranked` returns identical symbols, distances, kinds, aggregates, and horizon disclosure — only row order differs.
- [x] Truncation head: with a result limit smaller than the distance-1 layer, the detailed rows are the most important distance-1 dependents and truncation is disclosed with a continuation.
- [x] Continuation under ranked order: resuming with the token yields the next rows of the ranked sequence with no repeats or omissions (pagination write-site).
- [x] Token binding: a token issued under `ranked` presented with `--order unranked` is rejected as a usage error; a token whose identity was hashed under a different rank version is rejected (both write-sites of the identity check).
- [x] Unranked ordering: explicit `--order unranked` reproduces the pre-change `(depth, kind, id)` order byte-for-byte against existing fixtures.
- [x] Disclosure: JSON carries `ordering: ranked` by default and `ordering: unranked` on request; a typed-empty dependents answer still carries the field; human render carries the heuristic note only for ranked (render write-site distinct from JSON).
- [x] Impact identity-population write-site (emerged in review): `impact` binds the order selector into its own token identity, apart from `trace`'s — pinned by a process-level test that mints a ranked impact cursor, sees it refused under `--order unranked`, and sees the unswitched cursor resume; the test was verified to fail under mutation of the impact identity's order binding.
- [x] Rank-version binding sealed (emerged in review): binding the version alongside the mode is untestable at runtime (the version is a compile-time constant), so the pair is collapsed into a sealed ordering-identity value whose only production constructor stamps the current version — the `--order`-switch tests at both sites then cover the whole clause, and a site can no longer bind the mode without the version.
- [x] Recovery-recipe write-site (emerged in review): the approximate answer's re-run line reproduces the selected order, tested for both selector values.
- [x] Explicit-flag wiring write-site (emerged in review): through the built binary, `trace --relation references --order ranked` is refused while the same invocation without the flag succeeds — pinning the CLI's explicit-versus-default distinction, not only the handler's rejection.

## Performance and dogfood gates

- [x] Benchmark on the four persistent clones (httpx2, Flask, ripgrep, fd): `ranked` vs `unranked` on hub-symbol traces and a multi-symbol impact; record timings in the change notes; gate: ranked within 100 ms of unranked everywhere.
  If the gate fails, stop and surface — the fallback (persisted rank) is a separate change.
- [x] Pre-registered quality check: before running, write into the change notes the expected top-of-layer dependents for fixed sampled subjects on each clone; run, record pass/fail per prediction; a failing result feeds the edge-kind-weighting follow-up decision rather than silent retuning.

## Docs

- [x] README: document the ranked default, the `--order` selector, the ordering disclosure, and that ranking never changes the answer set.
