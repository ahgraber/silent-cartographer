# Proposal: dependents-traversal-cost

> Status: PROPOSAL — delta spec, design.md, and tasks.md generated alongside this document.
> Depends on nothing; the `impact` change benefits from it but does not block it.

## Intent

The dependents traversal returns the right answer and takes far too long to do it.
Measured against this repository's own index (1,700 symbols, 13,147 `uses` edges), a single `trace --relation dependents` on a module symbol takes **19 seconds**, and a `impact` run seeded from ten changed declarations takes **over two minutes**.

The cost is not the graph's size — it is redundant work.
The traversal identifies each finding by _(symbol, depth, edge kind)_, so the same symbol reached at depth 7 and again at depth 8 counts as two findings, and each one is expanded again: every edge below it re-followed, everything beneath it re-discovered one level deeper.
The reachable set closes at depth 6; the walk runs to the internal horizon of 20, re-emitting all 330 reachable symbols at every level past closure.

| depth bound | rows emitted | distinct dependents | seconds |
| ----------- | ------------ | ------------------- | ------- |
| 2           | 118          | 115                 | 0.12    |
| 4           | 725          | 321                 | 0.94    |
| 6           | 1,381        | **330**             | 1.91    |
| 8           | 2,041        | **330**             | 2.97    |
| 10          | 2,701        | **330**             | 4.02    |

Roughly seventy percent of the work happens after the answer is already complete.

This is a performance change, and it carries no correctness defect: the answer, its shortest distances, its connecting edge kinds, and its horizon disclosure are all already correct, because the collapse to one row per symbol happens before any of them is computed.
What is at stake is whether the answer arrives soon enough to be used.

### A larger reproducer, measured while dogfooding `impact`

The figures above come from a 1,700-symbol index.
Against this repository's index once the `impact` change landed — 39,713 symbols — a single `impact` over the working tree's own 14-file, 1,393-line diff resolved 24 seeds and took **4 minutes 32 seconds**.

Two things that measurement pins down.

**The cost concentrates in a few hub symbols rather than spreading evenly.**
Most of those 24 seeds are cheap: `cli::Command` traces in 0.16 s, `commands::collect_dir` in 0.31 s, `semantic::probe::tests` and `main::run` in under 0.1 s.
One seed, `query::page::PageIdentity`, takes **47.9 s** on its own — 511 dependents whose reachable set closes at distance 6 while the walk continues to the horizon of 20.
A seed set is therefore priced by its worst member, not its average, which is why a diff that looks unremarkable can produce an unusable answer.

**Lowering the depth bound is not a mitigation.**
The same 24-seed assessment at `--depth 0` — no detailed rows at all, aggregate only — still spent 237 s of CPU.
The work is the walk itself, so a caller cannot buy responsiveness by asking for less detail, and the aggregate that makes the horizon disclosure honest is exactly what costs the most.

Both observations reinforce the combined-traversal item already in scope: sharing one visited set across the whole seed set is what stops a single hub symbol from being re-walked on behalf of every seed that reaches it.

## User Stories

### Story: responsive-blast-radius

As an agent or developer asking what a change could affect, I want the dependents answer back in something like the time I would have spent grepping, so that c10r's precision is actually reachable rather than abandoned mid-query.
Ladders to the north star's single measure of success — _an agent trusts c10r enough to stop grepping_ — and to outcome #2 (Blast radius before change).
A two-minute blast-radius query loses to a fifty-millisecond `grep` no matter how much more precise it is: an agent under a timeout gets no answer at all, which is strictly worse than an imprecise one, and a human stops reaching for the tool.
Query latency is therefore not the same concern as build latency, which the north star deliberately does not measure: a build runs once, out of band, while a query sits directly between the user and the answer they came for.

## Scope

**In scope:**

- The dependents traversal in the graph store: expand each reachable symbol once, at its shortest distance, and stop once nothing further is reachable rather than continuing to the internal horizon.
- A combined traversal over several seeds at once, sharing one visited set, so the multi-seed impact assessment stops paying the full cost per seed.
- Preserving the existing answer exactly: membership, shortest distances, connecting edge kinds, row ordering, the beyond-bound aggregate, and the horizon disclosure all stay byte-identical.

**Out of scope:**

- The answer's shape, bounding, or disclosure vocabulary — nothing observable about the answer changes except when it arrives.
- A deadline or cancellation on the query hot path.
  Bounding a query's wall-clock time is a separate concern from removing work that never needed doing, and adding one here would mask the cost rather than remove it.
- The `contains`/enclosure exclusion, the edge kinds that constitute dependence, and the internal horizon's value — all unchanged.
- Any other query's cost.
  `get`, `find`, and the non-dependents relations are single-step lookups and are not implicated.

## Approach

Replace the single recursive query with a breadth-first walk that keeps a visited set:

1. Seed the frontier with every seed symbol at once.
2. Query for everything whose dependency edges point at the current frontier.
3. Discard anything already visited; the remainder are new dependents at the current distance, and become the next frontier.
4. Stop when the frontier is empty, or when the internal horizon is reached.

Each symbol is expanded exactly once instead of once per depth at which it is reachable, and the walk ends when the answer is complete rather than at a fixed ceiling.
Seeding the frontier with the whole seed set is what collapses the impact assessment's N independent walks into one, since the visited set is then shared across seeds.

The tie-break that selects a dependent's reported edge kind — shortest distance first, then a fixed kind ordering, then identity — has to be applied deliberately among the edges arriving at the same distance, since a breadth-first walk gets shortest distance for free but not the kind ordering.
Row ordering is the sensitive part and the existing scenarios are the gate.

## Open Questions

- **Frontier size versus the store's query-parameter limit.**
  A level's frontier is bound into one query; a frontier larger than SQLite's variable limit needs chunking across several queries per level.
  The threshold and the chunking strategy are mechanism for `design.md`.
