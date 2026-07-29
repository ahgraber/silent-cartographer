# Tasks: dependents-traversal-cost

Groups run in build-dependency order: the store's traversal first (the only place the walk lives), then the impact assembly that stops merging per-seed results now that the store returns the union, then the evidence that pins both the preserved answer and the removed cost.

## Store traversal

- [x] Replace the recursive query in `GraphStore::dependents` with a breadth-first walk over a visited set: seed the frontier from every seed at once, and in each round query the sources of `uses`/`imports`/`type_hierarchy` edges whose destination is in the frontier, drop anything already visited, and record the rest at the current distance.
- [x] Terminate the walk when the frontier empties or the horizon is reached, so a closed reachable set stops paying for the remaining depth bound.
- [x] Apply the connecting-kind tie-break deliberately: a dependent's reported kind is the lowest under the fixed `uses` < `imports` < `type_hierarchy` order among the edges reaching it at its shortest distance, and rows are ordered `(distance, kind order, identity)` exactly as today.
- [x] Chunk a frontier larger than the store's bound-parameter limit across several queries within the same round, merging their results before the visited-set filtering, so a hub symbol's frontier cannot fail the query outright.
- [x] Accept a seed set rather than a single seed, keeping the single-seed call as its one-element case, and exclude each seed from being reported as its own dependent.

## Impact assembly

- [x] Replace the impact assessment's per-seed traversal-and-merge with one combined call, so the union is computed by the walk rather than reassembled after N walks.
- [x] Confirm the horizon disclosure and beyond-bound aggregate still derive from the collapsed set unchanged, with no per-seed reconciliation left behind.

## Evidence

- [x] Test: the existing dependents scenarios — direct, transitive, imports, type-hierarchy, enclosure-never-propagates, shortest-distance-across-paths, and cycle termination — pass byte-identical against the new walk, with no scenario edited to accommodate it.
- [x] Test: a seed whose reachable set closes at some distance yields an identical answer — membership, distances, kinds, and order — when computed at that distance and again at a much larger bound.
- [x] Test: over a synthetic densely-connected graph whose reachable set closes far below the bound, the answer is produced within a generous wall-clock bound the current implementation would exceed by orders of magnitude, so the assertion pins the scaling without being a tight timing test.
- [x] Test: several seeds with heavily overlapping dependents produce each dependent once at its shortest distance from any seed, within a bounded time rather than one growing with the seed count.
- [x] Test: a symbol reachable at distance one from one seed and distance three from another is reported once, at distance one.
- [x] Test: a frontier driven past the bound-parameter limit still answers correctly, exercising the chunking path rather than only the single-query path.
- [x] Test: a seed whose only path to the other seeds passes through an intermediate that depends on both of them directly is reported once at distance two, so a saturated intermediate never swallows a seed's report.
- [x] Test: the impact assessment's row order and continuation-token behavior are unchanged, so tokens issued against the previous ordering semantics remain valid.
- [x] Capture the before/after timing for a hub symbol on this repository's own index in the change record, so the claimed improvement is evidenced rather than asserted.
