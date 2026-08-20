# Proposal: internal-efficiency

> **Status: PROPOSAL.**
> The delta spec and `design.md` are drafted alongside this document; `tasks.md` follows once the scope below is confirmed.
> The measurements cited here, and the investigation that produced them, are recorded in `discussion.md`.

## Intent

A build parses each source document many times over.
The guarded join parses every document once and discards the result.
Ingest then parses the same document again: once per persisted symbol, once per aligned definition occurrence, and once in each of its three per-document passes.
No input changes between those parses, and every one of them allocates a fresh syntax tree and a full copy of the document text.

| repository | documents | parses | parses per document |
| ---------- | --------- | ------ | ------------------- |
| httpx2     | 119       | 12,137 | 102x                |
| flask      | 83        | 7,987  | 96x                 |
| ripgrep    | 98        | 10,478 | 107x                |
| fd         | 24        | 1,526  | 64x                 |
| Faker      | 813       | 34,383 | 42x                 |

Indexing ripgrep's 1.7 MB of source parses 762 MB of text.

Repeated work consumes processor time, memory, elapsed time, and power, and it does so whether or not anyone measures it.
The objective of this change is that the build performs each piece of work once.
The figures above and below indicate the scale of the waste and give a way to observe progress; they are not the target.

### The waste recurs on every build

A build has no incremental path.
`run_build` re-runs the semantic indexer over the whole workspace, re-reads every source file, and `clear_derived` discards the previous build's rows before deriving them again.
Only embedding vectors whose text is unchanged survive a rebuild.

| build                     | Faker   | ripgrep |
| ------------------------- | ------- | ------- |
| 1, cold, no store         | 121.0 s | 46.8 s  |
| 2, sources unchanged      | 123.3 s | 44.3 s  |
| 3, one source file edited | 120.1 s | 44.5 s  |

Editing one file of 813 costs what indexing the repository from nothing costs.
The waste is therefore paid on every pass around the edit loop, not once during setup.

### A second repetition in the same phase

`enclosing_symbol` resolves a declaration to its symbol by iterating the entire definition map and comparing document paths and spans — 5,890 entries per lookup on httpx2 — for every reference occurrence and every definition.
The lookup it needs is by location, and the same phase already builds a location-keyed map for its own use.

## User Stories

### Story: rebuild-costs-what-changed

As a developer or agent editing a repository, I want a rebuild to cost in proportion to what changed rather than to the size of the workspace, so that keeping the index fresh is something I do continuously rather than something I avoid.

> Ladders to outcome 4 (**Honest under edit**) and outcome 5 (**Calibrated trust**).
> Both promise a labeled choice between a fresh answer and a stale-but-precise one, and that choice exists only while the fresh path is reachable.
> The shipped `dependents-traversal-cost` change excluded build latency from this laddering on the grounds that "a build runs once, out of band."
> Its reasoning holds and its premise does not: builds 2 and 3 above cost what build 1 cost, so build work sits in the edit loop, in the same position that made query latency ladder.
>
> The story is bounded by what a change to c10r can reach.
> The semantic indexer analyzes the whole workspace on every run and accepts no file list, so a Python rebuild keeps a floor of about a minute that only a resident analyzer can lower.
> Within that floor, the work c10r itself performs becomes proportional to what changed.

## Scope

**In scope, in build order:**

- Perform no work at all when the stored index already describes the workspace's sources, analyzer, and declared environment, with an explicit way to rebuild regardless (code-graph).
  Serves: rebuild-costs-what-changed.
- Parse each source document at most once per build, and derive symbol content, definition parents, test classification, type-hierarchy edges, and clone keys from that one parse (code-graph).
  Serves: rebuild-costs-what-changed.
- Resolve an enclosing declaration to its symbol by looking up its location, instead of scanning every persisted definition (code-graph).
  Serves: rebuild-costs-what-changed.

**Parked (2026-08-20, user decision at the one-pass measurement checkpoint):**

- Re-derive only the documents whose contribution to the semantic index changed, keeping the derived rows of documents that did not (code-graph).
  With one-pass derivation shipped, ingest is 4.5 s (Faker) / 3.3 s (ripgrep) of a rebuild whose floor is the whole-workspace indexer, so this item can recover at most a few seconds per edit — against a schema bump, a row-ownership model for rows with no single owning document, and modifications to five baseline "per build" requirements.
  Revisit only if the indexer's share of a rebuild shrinks (e.g. an incremental Python indexer, which the measurement suggests is where the remaining leverage lives).
  The blockers and the measurement are recorded in `discussion.md`.

**Out of scope:**

- Any change to query answers: their results, output shape, staleness labeling, edge derivation, or alignment accounting.
  The change is behavior-neutral over the derived graph, and the acceptance evidence below is what holds it to that.
  `build`'s own outcome report is the one deliberate exception: the first scope item requires a skipped build to be reported as such, so the build answer gains that distinction (a human line and a machine field), and the CLI and MCP surfaces gain the explicit-rebuild option.
- Reducing the semantic indexer's own work.
  `scip-python index` and `rust-analyzer scip` both analyze a whole project and accept no file list.
  `scip-python`'s `--target-only` limits analysis to a subtree, but the resulting index carries paths relative to that subtree and omits every reference from the rest of the workspace, so an index built that way under-reports dependents — a wrong answer, not a faster one.
  A resident analyzer that tracks its own dependencies is the only sound way to lower this floor, and it is Beyond v1 by the north star.
- The semantic corpus and its embeddings, which the `semantic-chunking` change owns.

## Approach

The join already builds one prepared document per document — the syntax tree, the line index, and the declared alias bindings — and drops the map when it returns.
The direction is to lift ownership of that map so that a build prepares each document once and the join and every downstream derivation read the same one.
Peak memory is unaffected in kind, because the join already holds every document's tree at once.

For the enclosing-declaration lookup, the definition map is keyed by identity and the derivations need the inverse.
The type-hierarchy pass already assembles a location-to-identity map for itself.

Where the prepared-document map is owned, and whether the two location maps become one, are `design.md` decisions.

## Acceptance evidence

Efficiency is asserted structurally rather than by stopwatch: a build parses each document at most once, which is observable in a test and does not decay with the machine it runs on.
A timing figure is recorded per dogfood repository as an indicator of progress, not as a threshold to pass.

Behavior neutrality is asserted by comparison: for each dogfood repository, a store built before the change and a store built after it agree row for row across symbols, occurrences, edges, discrepancies, and corpus renders, with identical alignment accounting.
Both sides of that comparison must be built from the same embedding code, because `semantic-chunking` changes stored vectors by design.
