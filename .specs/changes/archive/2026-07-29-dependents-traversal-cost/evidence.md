# Evidence: dependents-traversal-cost

Captured 2026-07-28 against this repository's own index (`.c10r/index.db`, 1,700 symbols, 13,147 `uses` edges), release builds of the commit-state implementation ("before", recursive query) and the working-tree implementation ("after", breadth-first walk).
Both binaries ran the identical query against the identical index; every "after" answer was byte-compared against its "before" answer.

## Before/after timing

| query                                                                                       | before | after  | answer         |
| ------------------------------------------------------------------------------------------- | ------ | ------ | -------------- |
| `trace --relation dependents --depth 2 graph::store::GraphStore` (hub, 147 direct in-edges) | 9.29 s | 0.37 s | byte-identical |
| `trace --relation dependents --depth 2 silent-cartographer::graph::store` (module)          | 8.87 s | 0.07 s | byte-identical |
| `impact` over the working tree's own diff (6 seeds)                                         | 9.85 s | 0.46 s | byte-identical |

## Continuation tokens across the rewrite

A truncated `impact --limit 3` page was issued by the **before** binary; its cursor was then resumed by both binaries.
The two resumed pages are byte-identical, so tokens issued against the previous ordering semantics remain valid.

## Scale measured, and not

The largest walk exercised is the synthetic dense-graph scenario: ~43,000 dependency edges in the worst shape for this traversal (maximal fan-in, fully cyclic) — a larger edge count than any real index on hand (ripgrep 26k, httpx2 20k, this repo 13k).
The chunking scenario separately drives an 1,100-wide frontier past the 900-parameter chunk limit.
The walk's work is linear in edge count — each symbol enters the frontier at most twice, so each edge is scanned at most twice — so time is not the extreme-scale risk.
Untested: absolute scale beyond ~10⁵ edges, and memory — each round materializes every edge into the current frontier as owned strings before processing, so a monorepo-scale index (millions of edges) with a near-whole-graph frontier would spend transient memory proportional to that round's edge volume.
If indexes ever reach that size, stream rows per chunk instead of accumulating (or intern ids) before reaching for anything larger.

## The scaling gate in the test suite

The synthetic dense-graph scenario (`closed_reachable_set_does_not_pay_for_the_remaining_bound`: 120 mutually-connected nodes, all three dependency kinds, reach closed at distance 1) was run against the previous implementation before it was replaced: it took **46.8 s**, an order of magnitude past the test's generous 5-second bound.
The entire `code_graph` suite, that scenario included, now completes in under 2 seconds.
