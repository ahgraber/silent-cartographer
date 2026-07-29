# Design: dependents-traversal-cost

## Context

- The traversal lives in one place, `GraphStore::dependents` (`src/graph/store.rs`), and is reached by two callers: the `dependents` relation behind `trace`, and the diff-seeded `impact` assessment, which calls it once per seed and merges the results.
- The current implementation is a single SQLite recursive common table expression whose recursive term deduplicates on the three projected columns `(id, depth, kind)`.
  Deduplicating on that triple is what makes the walk redundant: `depth` is part of the identity, so a symbol reachable at several depths is a distinct row at each one and is expanded again at each one.
  The module comment's claim that a node is "queued and expanded once rather than once per path" holds only within a single depth — the depth dimension is the leak.
- The collapse to one row per symbol, at its minimum depth with the lowest-ordered connecting kind, already happens in Rust after the query returns.
  Every observable property of the answer — membership, distance, kind, ordering, the beyond-bound aggregate, the horizon disclosure — is computed from that collapsed set, which is why the current answer is correct despite the redundant walk.
- `DEPENDENTS_HORIZON` is 20.
  The measured reachable set for a hub symbol in this repository's own index closes at depth 6, so fourteen levels of pure re-expansion follow.

## Decisions

### BreadthFirstWithAVisitedSet

The recursive query is replaced by a breadth-first walk driven from Rust, expanding each symbol exactly once at its shortest distance.

- **Chosen.**
  Maintain a visited set of every symbol already reached and a frontier of the symbols whose dependents are still to be found.
  Each round queries for the sources of dependency edges pointing at the current frontier, discards anything already visited, records the rest at the current distance, and makes them the next frontier.
  The walk ends when the frontier empties or the horizon is reached.
- **Rationale.**
  Shortest distance falls out of breadth-first order for free: the first time a symbol is reached is by definition via a shortest path, so the visited check is also the shortest-distance rule.
  Each symbol is expanded once instead of once per depth at which it is reachable, and — the larger win — the walk terminates when the answer is complete rather than at a fixed ceiling.
  On the measured graph that is 330 expansions instead of roughly 6,600.
- **Alternatives.**
  Keeping the recursive query and adding a "not already reached at a lower depth" condition to its recursive term — rejected: a recursive CTE cannot consult the rows it has already produced, which is exactly the state a visited set is.
  Adding `depth` to a materialized index or reformulating the CTE to carry a minimum — rejected: the same limitation, dressed differently.
  Leaving the walk alone and lowering the horizon — rejected: it would truncate genuinely deep reach to buy speed, trading a correctness property for a performance one.

### LevelAtATimeRatherThanOneQueryPerSymbol

Each round issues one query for the whole frontier, not one query per frontier member.

- **Rationale.**
  The walk's depth is bounded by the horizon, so a level-at-a-time walk issues at most that many queries regardless of how large the graph is, while a per-symbol walk would issue one per reachable symbol and reintroduce a per-node cost in round trips.
  The frontier is bound into a single `IN` list over the existing destination-keyed edge index.
- **Frontier chunking.**
  A frontier larger than the store's bound-parameter limit is split across several queries within the same round, and their results merged before the round's visited-set filtering.
  This is a correctness requirement, not an optimization: a hub symbol's frontier can exceed the limit on a large graph, and an unchunked query would fail outright.
- **Alternatives.**
  A temporary table holding the frontier instead of an `IN` list — rejected for now: it trades a bounded chunking loop for schema-adjacent state on a read-only query path.

### TheWalkIsSeededFromEverySeedAtOnce

The traversal takes a set of seeds and starts with all of them in the initial frontier.

- **Rationale.**
  The impact assessment's answer is the union of its seeds' dependents, each reported once at its shortest distance from any seed.
  Computing that as N independent walks does the work N times over and then throws most of it away in the merge — with heavily overlapping dependents, which is the normal case for declarations in one change, nearly all of it is redundant.
  One walk over a shared visited set produces the union directly, and each symbol is expanded once across the whole assessment rather than once per seed.
  The single-seed traversal becomes the one-element case of the same walk, so there is one implementation rather than two.
- **Seeds are excluded from their own answer.**
  A seed never appears as its own dependent, matching the existing single-seed behavior; with several seeds, a seed that genuinely depends on another seed is still reported, because that is a real dependency the caller asked about.
- **Mechanism: each node carries up to two distinct walk roots.**
  A plain shared visited set cannot honor both halves of the seed rule at once: it either never reports a seed (losing one that genuinely depends on another seed) or reports a seed reached only through its own dependency cycle.
  The walk therefore tracks, per node, the first two _distinct_ seed roots to reach it — for any single excluded root, at least one of two distinct roots differs from it, so a seed's shortest distance to a seed _other than itself_ is always answerable from the pair.
  A node re-enters the frontier when it gains a root, so each symbol is expanded at most twice across the whole assessment rather than once per seed or once per depth — the same asymptotic win, with exact seed-exclusion semantics.
- **Alternatives.**
  Keeping the per-seed walk and deduplicating afterwards — rejected: that is exactly the current cost.
  A combined recursive query seeded from a list — rejected for the same reason the single-seed one is being replaced.

### OrderingAndTieBreaksArePreservedExactly

The reported connecting edge kind, and the order rows appear in, are unchanged.

- **Rationale.**
  A dependent's reported kind is the lowest-ordered kind among the edges that reached it at its shortest distance, under the fixed order `uses` < `imports` < `type_hierarchy`, and rows are ordered by `(distance, kind order, identity)`.
  Breadth-first order supplies the shortest distance but says nothing about which of several same-distance edges to report, so the kind ordering has to be applied deliberately when a symbol is first reached and when further edges reach it at that same distance.
  This is the property most likely to drift silently in the rewrite, and the existing scenarios are the gate: they must pass byte-identical.
- **The horizon disclosure keeps its current meaning.**
  It is computed from the collapsed set, where the maximum distance is the true shortest distance of the furthest dependent, so a walk that stops at closure reports the same disclosure as one that ran to the ceiling.
  A walk that genuinely reaches the horizon with a non-empty frontier still discloses the cut.

## Architecture

```text
seeds ──► frontier ──┐
                     │  ┌──────────────────────────────────────────┐
                     └─►│ round at distance d                      │
                        │  query: sources of dependency edges      │
                        │         whose destination is in frontier │
                        │         (chunked if over the bind limit) │
                        │  drop: anything already visited          │
                        │  record: the rest at distance d,         │
                        │          kind by the fixed kind order    │
                        └──────────────┬───────────────────────────┘
                                       │ new symbols become the frontier
                        frontier empty │ or d == horizon
                                       ▼
                        collapsed rows ──► order by (distance, kind, identity)
```

- **Replaced**: the recursive query inside `GraphStore::dependents`.
- **Unchanged**: every caller's observable answer, the edge kinds that constitute dependence, the enclosure exclusion, the horizon value, and the query layer's split into detailed rows plus a beyond-bound aggregate.
- **Simplified**: the impact assembly's per-seed merge collapses into one call, since the store now returns the union.

## Risks

- **Silent ordering drift.**
  A row order that differs from today's would change every answer's presentation and invalidate in-flight continuation tokens, which bind to the answer's identity.
  Mitigation: the existing traversal scenarios are the regression gate and must pass unmodified; the impact assessment's ordering assertions guard the multi-seed path.
- **A performance scenario that is really a timing test.**
  A wall-clock assertion is flaky if its margin is thin.
  Mitigation: the scenarios are written against a synthetic graph whose reachable set closes far below the bound, where the current implementation is pathological and the intended one is not — so the gap is orders of magnitude and the threshold can be generous rather than tight.
- **Frontier exceeding the bind-parameter limit.**
  An unchunked frontier query fails outright on a large graph rather than degrading.
  Mitigation: chunking is specified above as a correctness requirement, with a test that drives a frontier past the limit.
- **Deep-but-narrow graphs.**
  A graph whose reach is a long chain issues one query per level, so a chain deeper than the horizon pays more round trips than the single recursive query did.
  Mitigation: the horizon bounds this at a small fixed number of queries, each trivial; the pathological case being removed is far larger.
