# Change notes: graph-aware-similarity

## Research digest: neighborhood-overlap similarity (2026-08-12)

Survey of the vertex-similarity / link-prediction literature and its code-domain applications, gathered while designing the graph signal.
Kept here because the DegreeDampedOverlap decision leans on it and the pre-registered A/B interprets against it.

**The local-index family and its benchmarks.**
For nodes x, y with one-hop neighborhoods Γ(x), Γ(y) and degree k:

- Common Neighbors: `|Γ(x) ∩ Γ(y)|` — no damping.
- Jaccard: `|∩| / |∪|`; Sørensen–Dice, Salton (cosine), Hub-Depressed `|∩|/max(k_x,k_y)` — endpoint-degree normalizations.
- Adamic–Adar: `Σ_{z∈∩} 1/log(k_z)`; Resource Allocation: `Σ_{z∈∩} 1/k_z` — per-shared-neighbor damping.
- Leicht–Holme–Newman: `|∩|/(k_x·k_z)` — harshest endpoint damping; Preferential Attachment: `k_x·k_y` — pure hub promotion.

Benchmark lineage (Liben-Nowell & Kleinberg 2003/2007; Zhou, Lü & Zhang 2009, EPJ B; Lü & Zhou 2011, Physica A): Resource Allocation was the best local index overall, Adamic–Adar close behind; the two are indistinguishable when shared-neighbor degrees are small and diverge on hub-heavy graphs, where Resource Allocation's linear penalty wins.
Endpoint-normalized indices (Jaccard, Sørensen, Salton, Hub-Depressed) sat mid-pack or below plain Common Neighbors; Leicht–Holme–Newman (over-damped) and Preferential Attachment were worst.
Headline: per-neighbor damping beats endpoint normalization _for predicting attachment_ — and hubs legitimately attach more, which is exactly where that objective diverges from a similarity contract that forbids rewarding ubiquity.

**Global measures (why not):** SimRank (Jeh & Widom 2002) and rooted PageRank buy multi-hop structural equivalence at whole-graph iterative cost and did not beat Adamic–Adar in the original benchmarks; node2vec-style embeddings add training, hyperparameters, and nondeterminism.

**Code-domain precedent:**

- Schwanke, Arch (ICSE 1991): shared-neighbor similarity over call/use graphs with inverse-frequency feature weighting, for clustering and misplaced-procedure detection — the signal itself, three decades ago.
- Robillard, Suade / "Topology Analysis of Software Dependencies" (ICSE 2007 / TOSEM 2008): ranks structurally related elements by one-hop neighborhood heuristics with explicit inverse-degree "specificity" damping — endpoint damping shipped in a code tool.
- Software clustering's "omnipresent modules" line (Rigi; Wen & Tzerpos, IWPC 2005; later refinements): code hubs treated as stop-words — detected and specially handled because they obscure structure.
- CLAN (ICSE 2012, API-usage app similarity) and Aroma (OOPSLA 2019, structural code recommendation) rank by feature-set overlap with frequency-based or unweighted scores — overlap ranking at other granularities.
- Díaz-Pace et al. (2018) apply the standard index battery directly to module dependency graphs for coupling prediction.

**Performance note:** skipping shared neighbors above a per-index degree cutoff loses almost no accuracy for already-damped indices while giving order-of-magnitude speedups (Sahu 2024) — empirical confirmation that hub neighbors carry near-zero signal, and an available optimization if the aggregation ever needs one.

**Consequence recorded in design:** numerator = Resource Allocation (best-evidenced per-neighbor damping); denominator = Jaccard union (serves the ubiquity-is-not-similarity clause the attachment-oriented benchmarks do not test); A/B against the bare Resource Allocation numerator pre-registered in dogfood to keep the denominator on evidence.

## Dogfood verdict: the signal makes `similar` worse on most subjects (2026-08-14)

Clones rebuilt from source with this binary: `httpx2` (Python, 119 files) and `ripgrep` (Rust, 13 crates).
Six subjects, each asked twice — once with the graph signal fused, once against a build whose graph rank list is always empty, which is the ranking that shipped before this change.
The question each answer is judged against is the one the story asks: _if I am assessing duplication around this symbol, which list is more useful?_

| subject                      | with the graph signal                                                             | before                                                              | verdict    |
| ---------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ---------- |
| `Client` (class)             | five of the top eight are `Client`'s own methods; `AsyncClient` drops to 2        | `AsyncClient` first, then httpx's and httpcore's sync/async pairs   | worse      |
| `Client.get`                 | the six sibling verbs (`post`, `put`, …); `AsyncClient.get` falls out of the head | `AsyncClient.get` first — the actual duplicate of this method       | worse      |
| `Client._init_transport`     | `AsyncClient._init_transport` first                                               | identical                                                           | no change  |
| `AfterContext` (flag struct) | `BeforeContext`, then `Color`, `Files`, `Follow`                                  | the context-related flags cluster first                             | worse      |
| `LowArgs`                    | the argument-handling family, top to bottom                                       | an unrelated `grep-regex::Config` at rank 1                         | **better** |
| `Searcher`                   | the crate root at rank 1, then `Sink`, `SinkFinish`, `Sunk`, `Sink::begin`        | `Sink`, `SearcherBuilder`, `SearchWorker` — things comparable to it | worse      |

One improvement, one no-change, four regressions.
`_init_transport` is unchanged because the clone tier already put the true twin at rank 1; the ranking underneath it never mattered there.

**Why the regressions happen.**
Two structural causes, both visible in more than one probe.

_A symbol's own members flood its answer._
A method uses whatever its class uses and is called from wherever its class is called, so it shares almost the whole neighborhood and takes the signal's top ranks.
`Client`'s answer is mostly `Client`'s own methods, which are parts of the subject, not code similar to it.
Excluding `contains` from the neighborhood does not prevent this: members inherit the container's neighborhood through their own edges.

_The subject's own crate root is a neighbor of everything in the crate._ `grep-searcher::crate#1` shares more of `Searcher`'s neighborhood than any real peer does and takes rank 1.
It is exactly the leak class the damping exists for, and the damping does not stop it.

**What the signal is actually good at.**
Both wins are the same shape: the subject is a data structure or a family member, and the graph knows which family it belongs to.
`LowArgs` improved because the content signal had matched a same-shaped `Config` struct in an unrelated crate, and no edge connects the two.
The sibling families (`Client`'s verbs, the flag structs) do surface as blocks — the change does what it set out to do — but a sibling family is by-design uniformity, not a consolidation finding, so surfacing it displaces the answers a duplication question wants.

## Pre-registered A/B: the union denominator changed nothing a reader sees (2026-08-14)

The shipped score against the bare Resource Allocation numerator (denominator forced to 1), on the same subjects.

The answer heads are **identical** under both forms on every subject tested.
The only differences are hundreds of positions down the tail: for `Searcher`, `ripgrep::crate#1` sits at 220 without the denominator and 519 with it, and `grep::crate#1` at 156 against 188; for `AfterContext` the two forms agree exactly.
So the denominator demotes crate roots deep in the tail, never promotes one, and never changes what a reader is shown.

It also did not prevent the failure it exists to prevent: `grep-searcher::crate#1` still reaches rank 1 for `Searcher` under the shipped form.
The denominator is therefore kept on the grounds that it is directionally right and costs nothing, not on the grounds that it works — the hub problem is not solved by damping the score, because the fusion reads only ranks.

## Why damping the score cannot fix ranking (2026-08-14)

Measured on the test fixture: a candidate whose only shared neighbor is a hub scores about 50× less than a true structural sibling (0.004 against 0.22), and still lands at graph rank 5 of 76, because only four candidates score above it.
Reciprocal-rank fusion reads that rank, not the score.
Both damping terms move a candidate's score a great deal and its fused contribution hardly at all: entering the graph list at rank 5 is worth about as much as a first-place content rank.

This is why the crate root reaches rank 1 for `Searcher` despite scoring far below the real peers, and it is the mechanism behind the regressions above.
Two changes would address it, neither of them in this change's design:

- exclude the subject's containment closure from the candidate set, so a class's own methods stop competing with code similar to the class;
- let the score influence the fusion rather than only the within-signal order, so a hub-only overlap contributes a fraction of a vote instead of a whole one.
