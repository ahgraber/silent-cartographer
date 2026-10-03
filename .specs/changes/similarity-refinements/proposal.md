# Proposal: similarity-refinements

## Intent

A dogfood audit ran `c10r similar <id> --limit 6 --json` over about 1,400 symbols of a Python repository and inspected the 84 pairs of symbols that were each other's first row.
About 19% of those pairs were real duplication.
About 35 pairs were parallel implementations of the same protocol or base class, about 12 were a caller and its callee or a facade and its delegate, and about 15 were a test whose body is one call to the subject.
`similar` gives a reader no way to tell these cases apart, so the reader must open both symbols for every pair.

The clone markers were more precise than the estimated ranking, but they missed two cases.
Two tests that were identical except for renamed identifiers and a docstring were not marked as clones, because a Python docstring counts as a literal in the token sequence, while a Rust doc comment does not.
Five lines shared inside a 56-line method were not found at all, because `similar` compares whole symbols and the clone keys match only whole symbols.

The audit needed about 1,400 process launches at about 0.42 s each, because `similar` takes one subject per invocation and no command lists clones or mutual nearest pairs across the corpus.

## User Stories

### Story: documentation-blind-clones

As a developer or agent looking for duplicated code, I want two functions that differ only in names, literal values, or documentation to carry a clone marker, so that a docstring edit does not hide copied code.

> Ladders to north-star outcome 3 (**Structural understanding**): copied code is a structural fact about the codebase.
> Ladders to the guiding principle **Calibration over coverage**: the clone marker is a deterministic claim, and it treats Rust and Python documentation the same way.

### Story: similar-rows-name-their-relation

As a developer or agent reading a `similar` answer, I want each row to state whether it uses the subject, is used by the subject, tests the subject, or belongs to the same type hierarchy as the subject, so that I can tell duplication from code that is similar because it is connected to the subject.

> Ladders to north-star outcome 3 (**Structural understanding**): the label states a graph fact beside an estimated ranking.
> Ladders to north-star outcome 5 (**Calibrated trust**): the label is resolved evidence, and it never changes the estimated order.

### Story: duplication-audit-in-one-run

As a developer or agent auditing a codebase for duplication, I want the corpus clone groups, the `similar` answers for many subjects, and the pairs of mutual nearest neighbors, each from one invocation, so that an audit costs one process instead of one process per symbol.

> Ladders to north-star outcome 6 (**Find by intent**): the audit finds code by what it does across the whole corpus.
> Ladders to the guiding principle **Agent-native, human-rendered**: structured, deterministic, bounded output for a high-volume consumer.

### Story: shared-fragments

As a developer or agent, I want to see the exact code that a symbol shares with part of another symbol, so that I find copied blocks that whole-symbol comparison misses.

> Ladders to north-star outcome 3 (**Structural understanding**) and to the guiding principle **Precision is the moat**: the reported span is an exact token match, not an estimate.

## Scope

**In scope** (two capabilities, `code-graph` then `code-navigation`, then the MCP server), in build order:

- Clone keys disregard documentation (`code-graph`).
  A Python docstring leaves the token sequence that the clone keys and the fragment evidence read, as Rust doc comments already do.
- The build persists shared-fragment evidence for every leaf symbol (`code-graph`).
- One schema version bump for both items above.
  The existing replace-or-refuse rule is the migration.
- Every `similar` row carries the relation labels that hold between it and the subject (`code-navigation`).
  The labels are `uses_subject`, `used_by_subject`, `tests_subject`, and `type_family`.
  They derive from the persisted `uses` and `type_hierarchy` edges and the test classification.
- `similar --exclude <label>` omits rows that carry a chosen label (`code-navigation`).
- `similar --subjects-from <path|->` answers many subjects in one invocation (`code-navigation`).
- `similar --mutual` lists the pairs of corpus symbols each of which is the first row of the other's `similar` answer (`code-navigation`).
- A new command `clones` (`code-navigation`).
  With no subject, it lists the corpus clone groups.
  With a subject, it lists the subject's whole-symbol clones and the token runs the subject shares with other leaf symbols.
- No similarity score appears in any answer.
  Answers carry ranks, the clone-certainty marker, and relation labels only.
- One surface version bump, the matching MCP pin, a new MCP `clones` tool, and the new `similar` controls on the MCP `similar` tool.
- Command documentation for every new flag and for `clones`.

**Out of scope:**

- Any change to the `similar` or `search` ranking.
  Relation labels and exclusions never enter the rank fusion.
  An earlier change (`graph-aware-similarity`, archived 2026-08-14) was reverted because graph evidence inside the fusion degraded the ranking.
- Method-to-method implementation links.
  The store holds class-to-class `type_hierarchy` edges only.
  The scip-python indexer, version 0.6.6, emits SCIP `is_implementation` relationships from a subclass method to the base method it overrides, and c10r reads no SCIP relationships today.
  Consuming them belongs in its own change; `design.md` § RelationSource gives the reasons.
- Structural protocol satisfaction: a class that matches a protocol without declaring it as a base gets no `type_family` label.
- A corpus-wide list of shared fragments.
  `clones` reports fragments for one subject at a time.
- Near-miss clones with inserted, deleted, or reordered statements.
  Fragment certainty is exact or variant only.
- Fragment rows in `similar` answers.
- Approximate nearest-neighbor indexes.

## Approach

**Documentation leaves the token sequence.**
The token walk that feeds the clone keys skips a Python leading docstring, using the same docstring detector the interface tier uses.
Rust doc comments are comment nodes and are already skipped.

**Relation labels after ranking.**
`similar` computes its rows as today.
Each row then gets a set of labels from edge and classification lookups.
An exclusion drops labeled rows from the ordered list before the page is cut, so the remaining rows keep their relative order.

**Batch and mutual pairs in one process.**
One process opens the store once and computes each subject's answer with the same code that answers one subject, so each answer is identical to the single-subject answer.
Mutual pairs walk the corpus in canonical-identity order and page by the lower member of each pair.
`design.md` § AllPairsDiscovery compares this with clone-key grouping, approximate indexes, and MinHash with locality-sensitive hashing.

**Clone groups from the keys.**
The corpus clone groups are a grouping on the persisted substitution-insensitive key, with exact subgroups on the formatting-insensitive key.
No vector work is involved.

**Fragments by winnowing.**
The build selects winnowing fingerprints over each leaf's token sequence with identifiers and literals replaced by their class.
At query time, a shared fingerprint gives a candidate position, the query re-reads both token sequences from the indexed document text, and only verified token runs are reported.
`design.md` § FragmentDetection compares this with suffix-array, AST, embedding, late-interaction, and alignment approaches.

## Open Questions

- OPEN QUESTION: the guaranteed fragment length, in tokens.
  `design.md` proposes 40 tokens, about five Python lines, and gates the value on the audit's five-line miss.
- OPEN QUESTION: whether `--mutual` belongs in this change.
  The audit's deliverable was the mutual pairs, but batch mode alone reproduces them at the cost of client-side matching.
- OPEN QUESTION: the names `clones`, `--subjects-from`, `--mutual`, `--exclude`, and the four relation labels.
  The spec fixes them because they are the CLI contract, and the user may rename them before apply.
