# Tasks: unresolved-calls

## Setup

- [x] Create the worktree `.worktrees/unresolved-calls` on branch `unresolved-calls` from `main`.

## Fixtures

- [x] Add `tests/fixtures/unresolved-calls/python/` with a `pyproject.toml` and a package holding: a class with a method `fetch`; a test function whose unannotated parameter `client` is called as `client.fetch()`; a function whose parameter is annotated with that class and calls `.fetch()` on it, so that `fetch` has a resolved reference; a second method `drain` with no resolved reference, called only through an unannotated parameter; a function that calls its parameter as `callback()`; a function that calls a local variable as `helper()`; a module-scope attribute call through an unannotated name; and an unresolved call whose receiver is a call with a multi-line argument longer than 200 characters.
- [x] Generate the fixture's scip-python index (`index.scip`) and ty-scip index (`ty-scip.scip`), and commit them with `SCIP-PYTHON-VERSION` and `TY-SCIP-VERSION` files in the format of `tests/fixtures/python-conformance/`.
- [x] Confirm, by decoding both committed Python indexes, that `client.fetch()` and `drain`'s call have no occurrence at the called name and that the annotated `.fetch()` call has one, under each indexer.
- [x] Add `tests/fixtures/unresolved-calls/rust/`, a crate holding a type `Widget` with methods `render` and `build`, a resolved call of each, a function that calls its closure parameter as `callback()`, and a function under `#[cfg(any())]` that calls `value.render()` and `Widget::build()`.
  `#[cfg(any())]` is false on every host, so the committed index does not depend on the host that generated it.
- [x] Generate the crate's rust-analyzer index (`index.scip`) and commit it with a `RUST-ANALYZER-VERSION` file in the format of `tests/fixtures/rust-conformance/`.
- [x] Add support helpers in `tests/support/mod.rs` that load each new fixture's sources and indexes, following `python_fixture_index_from` and `rust_fixture_index`.

## Local occurrence ranges (semantic-engine)

- [x] Add a per-document list of local occurrence ranges to `ExtractedIndex`, keep local-symbol occurrences in that list in `translate_index` in [src/semantic/scip.rs](../../../src/semantic/scip.rs), and return it from the Python and Rust adapters, following `design.md` § Carry local occurrence ranges across the extraction boundary.
- [x] Extend the backend conformance suite with the clause `Local occurrence ranges extracted`, and run it against the scip-python, ty-scip, and rust-analyzer indexes of the new fixtures: the Python `helper` token's range and the Rust `callback` token's range are each in their document's list, and no persisted symbol or occurrence comes from them.
- [x] Test: ingesting an index with local occurrence ranges persists the same symbols, occurrences, and edges as ingesting it with the list empty.

## Call extraction (code-graph)

- [x] Add a `SyntaxTree` method in [src/graph/syntax.rs](../../../src/graph/syntax.rs) that yields each call's called-name span, name text, and receiver span, following `design.md` § Identify calls and called names from the syntax tree.
- [x] Unit test the method on Python source: a plain call, an attribute call, an attribute call whose receiver is itself a call, a decorator call, and a call of a subscript, which yields no call.
- [x] Unit test the method on Rust source: a plain call, a method call, a path call, a turbofish call `Vec::<u8>::new()`, a call inside a macro's arguments, which yields no call, and a call of a parenthesized closure, which yields no call.

## Recording (code-graph)

- [x] Add the `unresolved_calls` table (with the receiver's shortened flag) and its name index, and the `discrepancy_symbols` table and its symbol index, to `SCHEMA_SQL` in [src/graph/schema.rs](../../../src/graph/schema.rs); bump `SCHEMA_VERSION` from 19 to 20, and add a version note to its doc comment.
- [x] Add row types, `insert_unresolved_calls`, `unresolved_calls_named` (ordered by document path, then start offset), the `discrepancy_symbols` writer, and `unconfirmed_uses_of` (ordered the same way) to [src/graph/store.rs](../../../src/graph/store.rs), and delete both tables' rows in `clear_derived` before discrepancies and symbols are deleted.
- [x] Add the unresolved-call pass to `ingest_with_params` in [src/graph/mod.rs](../../../src/graph/mod.rs), following `design.md` § Detect unresolved calls by exact span equality against every semantic occurrence, § Store receiver text up to a bound, and § A new store table, cleared with the rest of the derived state.
- [x] Write the `discrepancy_symbols` rows for refused reference occurrences in `ingest_with_params`, following `design.md` § Refused reference occurrences record the symbols they may denote.
- [x] Test (`A Python attribute call the indexer cannot resolve is recorded`): ingest the Python fixture with each of its two indexes, and assert the `client.fetch()` call is recorded with name `fetch`, receiver `client`, its location, and the test function as enclosing declaration.
- [x] Test (`A Python plain call with no occurrence is recorded without a receiver`): ingest a one-document index whose source holds a plain call with no occurrence at its name, and assert the call is recorded with no receiver.
- [x] Test (`A Rust call in code the analyzer does not analyze is recorded`, `A Rust path call carries its path as receiver`): ingest the Rust fixture, and assert `value.render()` is recorded with receiver `value` and `Widget::build()` with receiver `Widget`.
- [x] Test (`A resolved call is not recorded`): with the Python fixture under each index and with the Rust fixture, assert the resolved calls, including the annotated `.fetch()` call, are absent from the recorded calls.
- [x] Test (`A call resolved to a local binding is not recorded`): from the committed raw indexes through the adapters' translation to ingestion, assert `callback()` is not recorded, for the Python fixture under each index and for the Rust fixture, and that the Python fixture's `helper()` is not recorded under each index.
- [x] Test: ingest a one-document index in which the join refuses the occurrence at a call's name, and assert the call is not recorded.
- [x] Test (`A module-level call attributes to the module`): assert the fixture's module-scope call is recorded with the module symbol as its enclosing identity.
- [x] Test (`A long receiver is shortened with a marker`): assert the fixture's long receiver is recorded as its first 200 characters with the shortened flag set, and that a short receiver has the flag clear.
- [x] Test (`An unresolved call carries line and column`): ingest a one-document index whose unresolved call follows a multi-byte character on its line, and assert the recorded start column counts that character once and the byte offsets denote the same position.
- [x] Test (`A duplicate-ambiguous reference records every duplicate`): ingest a hand-built index with three definitions sharing one descriptor and a reference the join records duplicate-ambiguous, and assert its discrepancy links all three.
- [x] Test (`A text-mismatch reference records the symbol it names`, `A refused occurrence of no persisted symbol records none`): ingest a hand-built index with a reference occurrence at a position whose text is not the symbol's name, once for a persisted symbol and once for a symbol that is not persisted.
- [x] Test: a refused definition occurrence links no symbol.
- [x] Test (`Builds wholly supersede prior derived state`): build a store from a source with an unresolved call and a refused reference, rebuild it after both are removed, and assert no recorded call and no discrepancy link remains.
- [x] Update the schema-version tests in [src/graph/store.rs](../../../src/graph/store.rs) for version 20.

## References answer (code-navigation)

- [x] Add the group types and the `unresolved_calls` and `unconfirmed_uses` fields to `Answer` in [src/query/output.rs](../../../src/query/output.rs), serialized only when present, with the shapes in `design.md`.
- [x] In the `References` arm of `QueryEngine::trace` in [src/query/mod.rs](../../../src/query/mod.rs), attach both groups, on the found and the empty answer: the unresolved calls for the last component of the subject's display name, and the unconfirmed uses linked to the subject.
- [x] Test (`Resolved references and unresolved calls appear side by side`): on the Python fixture under each index, `references` of `fetch` returns the annotated call as a result and the `client.fetch()` call in the group.
- [x] Test (`A subject with no matching call carries an empty group`, `A subject with no refused use carries an empty group`): a `references` answer for a symbol with neither carries both groups with total zero.
- [x] Test (`A module-level call names its module`): the JSON entry for the fixture's module-scope call names the module symbol.
- [x] Test (`A Rust subject carries the group`): on the Rust fixture, `references` of `Widget::render` lists the `value.render()` call.
- [x] Test (`A use ambiguous between duplicates is disclosed on each duplicate`): on the hand-built duplicate index, the `references` answer of each of the three definitions lists the use with all three candidates.
- [x] Test (`A use refused for mismatched text is disclosed with the text found`).
- [x] Test (`A refused use with no location is listed with typed absence`): a hand-built index with a reference occurrence whose range lies beyond its document; the JSON use carries its document and refusal kind and omits location and found text, and the human rendering prints `(position unknown)`.
- [x] Test (`A use ambiguous among many duplicates lists a bounded candidate set`): a hand-built index with 12 definitions sharing one descriptor; the use lists 10 candidates, carries 12, and is marked shortened.
- [x] Test: a use refused for mismatched text whose found text exceeds 200 characters carries its first 200 characters marked shortened.
- [x] Test (`An ambiguous subject carries no group`): a short name shared by several symbols returns a candidate set with neither group, and an unresolved subject returns typed absence with neither group.
- [x] Test (`The group carries no heuristic-grade marker`): a `references` answer whose groups list entries carries no heuristic-grade marker in its JSON.
- [x] Cap both groups at the effective limit in `apply_pagination` in [src/query/page.rs](../../../src/query/page.rs), on the found and the empty outcome alike, and pass both fields through unchanged in every other `apply_*_pagination` function.
- [x] Test (`The group is capped at the result limit`, `The unconfirmed-use group is capped at the result limit`): with more entries than a requested limit, on an answer with resolved references and on one with none, each group lists exactly the limit's number, carries the full total, is marked truncated, and carries no continuation token.
- [x] Test (`An unbounded answer lists every unresolved call`): with `--limit 0`, the group lists every matching call and is not marked truncated.
- [x] Test (`Every page carries the same group`): through the CLI, the first page and the page its continuation token resumes carry equal groups.
- [x] Test (`The group does not change the paging counts`): through the CLI, the paging disclosure's returned and total counts equal those of resolved references alone.

## Empty answers and rendering (code-navigation)

- [x] Add the internal, unserialized empty-answer scope field to `Answer`, set it in `QueryEngine::trace` for `references` and `importers` and in `QueryEngine::dependents` for every dependents answer, its empty return included, and pass it through every `apply_*_pagination` function.
- [x] In the `Outcome::Empty` arm of [src/render.rs](../../../src/render.rs), print the scoped line for each of the three scopes, with both group totals for `references`, following `design.md` § Empty answers of reference-derived relations name what c10r cannot see.
- [x] In `render_found` in [src/render.rs](../../../src/render.rs), print `no resolved dependent recorded` with the "not seen" parenthesis after the seed list of a `Seeded` impact report with no dependent rows; leave the `NoIndexedSymbolTouched` and `NoneResolvable` lines unchanged.
- [x] Print each non-empty group after the results or the empty line, with the receiver whitespace-collapsed, sanitized, and followed by `…` when shortened, each unconfirmed use with its reason, and the `list all` line only when a group is truncated.
- [x] Test (`An empty references answer states what it cannot see`): the human rendering of an empty `references` answer with both groups at zero names both zero totals and the unseen cases, and does not contain `definite`.
- [x] Test (`An empty references answer with unresolved calls gives their total`): on the Python fixture, the empty answer for `drain` states the unresolved total and does not contain `definite`.
- [x] Test (`An empty dependents answer states what it cannot see`, `An empty importers answer states what it cannot see`): through the CLI `trace` command, not a hand-built answer.
- [x] Test (`An impact answer with no dependents states what it cannot see`): through the CLI `impact` command, the rendering lists the seed and the scoped line.
- [x] Test (`An impact answer that touches no indexed symbol stays definite`): a comment-only diff renders the definite none line.
- [x] Test (`Structural relations keep the definite wording`, `An empty structural relation is a definite none`): an empty `contains` answer renders the definite message.
- [x] Test (`Empty relation is typed absence`): an empty `references` answer's JSON outcome is `empty`, distinct from absence and failure.
- [x] Test (`The group renders after the results with its label`, `The unconfirmed-use group renders with its reasons`).
- [x] Test: a shortened multi-line receiver holding a terminal-control character renders on one line, with the character replaced and `…` appended.
- [x] Update the doc comment of `Outcome::Empty` in [src/query/output.rs](../../../src/query/output.rs) and the `trace` relation help text in [src/cli.rs](../../../src/cli.rs) to say that an empty answer of `references`, `dependents`, `importers`, or `impact` means no resolved instance was recorded.
- [x] Update the `trace` and `impact` tool descriptions in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py) the same way, and run the MCP test suite.

## Other relations (code-navigation)

- [x] Test (`Dependents ignore unresolved calls`): a symbol called only through unresolved calls has an empty `dependents` answer with neither group.
- [x] Test (`Tests ignore unresolved calls`): an unresolved call inside a test-classified function is not a `tests` site, and the `tests` answer has neither group.
- [x] Test (`Dependents ignore refused uses`): a symbol whose only use is refused has an empty `dependents` answer.
- [x] Test: `impact`, `importers`, and `implementers` answers carry neither group.

## Documentation

- [x] Add a `references` section to [docs/commands/trace.md](../../../docs/commands/trace.md): both groups, their fields, the cap at `--limit` with no continuation, the 200-character receiver bound, matching by last name component, the exclusion of Rust macro arguments, the dependence of Rust results on the build host's `cfg` conditions, and the meaning of an empty answer for each relation.
- [x] Update the `references` row of the README command table to mention both groups.

## Dogfood

- [x] Build Flask with scip-python and with ty-scip from the working tree, and record in `dogfood.md` the recorded-call counts by call form next to the 2026-10-03 and 2026-10-10 measurements, and the count of discrepancy links.
- [x] Record in `dogfood.md` the human `references` answers for Flask's `blog.index` and `Config.__init__`, and for `locate_app`'s `raise_if_not_found` definitions.
- [x] Build ai-vfs with scip-python and record in `dogfood.md` the `references` answer for `MetadataStore.touch_blob`: the group's total and whether it lists the 35 calls that text search found.
- [x] Build ripgrep with rust-analyzer and record in `dogfood.md` the recorded-call count next to the 2026-10-16 measurement.
- [x] Record in `dogfood.md` the byte size of the JSON `references` answer with `--limit 0` for the most common unresolved name in each Python dogfood repository.

## Checks

- [x] Run the full Rust test suite and the MCP test suite.
- [x] Run the pre-commit hooks on every changed and new file, and fix any failure.

## Verification remediation

- [x] Add a conformance clause in [src/semantic/conformance.rs](../../../src/semantic/conformance.rs) that checks every local occurrence range the way occurrence ranges are checked: its document declares a position encoding and the range is well formed.
  Test it with an index whose local range violates each condition.
- [x] Keep the range of every occurrence that has no usable symbol, a local symbol or a symbol string that does not parse, in the per-document list in `translate_index`, so a call at such an occurrence is not recorded.
  Unit test it with an unparsable symbol string.
- [x] Render an empty `tests` answer as `empty: no convention-classified test reference found (not proof that nothing tests the subject; <unseen cases>)`, and test it through the CLI (`An empty tests answer states what it cannot see`).
- [x] Test, through the CLI, that `trace --help` and `impact --help` describe an empty answer as no resolved instance recorded and do not contain `definite` for those relations.
- [x] Test, in the MCP suite, that the `trace` and `impact` tool descriptions say the same.
- [x] Make the unbounded-answer test build more unresolved calls than the default result limit.
- [x] Run the local-range ingestion equivalence test on the ty-scip index too.
- [x] Test that the calls of a source the analyzer left out of its index are recorded, and say so in `docs/commands/trace.md`.
- [x] Add a Python parenthesized callee `(f)()` to the call walker's unit test.
- [x] Update the `Answer::empty` docstring in [src/query/output.rs](../../../src/query/output.rs), and the `exact` row in [docs/commands/impact.md](../../../docs/commands/impact.md) so it does not read as a completeness claim.
- [x] Add `tests` to the empty-answer section of `docs/commands/trace.md`, and to the `trace` help text and MCP `trace` description if they list the relations whose empty answer means no resolved instance recorded.
- [x] Run the full Rust suite, the MCP suite, and the hooks.
