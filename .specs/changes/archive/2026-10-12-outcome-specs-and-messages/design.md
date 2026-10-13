# Design: Outcome Specs and Messages

## Context

- `messages.md` grades 122 messages from `src/` and `mcp/src/c10r_mcp/` on four parts: what happened, cause, implication, and remedy.
  60 rows lack at least one part; one of them (`C13`) is a cross-reference that is fixed through the messages it carries.
- The MCP server passes the CLI's diagnostic through unchanged (`Command failures surface as typed tool errors`), so one rewrite serves both surfaces.
- No requirement fixes message text word for word; requirements fix which facts a message names (`messages.md` § 3).
  So the rewrites need no spec change beyond the ADDED requirement and the modified `Incompatible index stores are replaced or refused, never half-used`.
- The restated requirements describe behavior the code already has, except the newer-store recovery.
  Their existing tests are their evidence.

## Decisions

### Decision: MessagesStateOnlyObservedCauses

**Chosen:** a message states a cause only when the code at the reporting point holds it.
Where several causes reach one message and the code cannot tell them apart, the message names the possible causes together or none.

**Rationale:** the requirement forbids a cause the system did not observe.
The freshness check is the main case: an environment that does not resolve, and one whose interpreter times out, both become an absent environment before the comparison runs (`src/commands.rs` around line 327).
So the human label reads `stale (environment changed or could not be checked)`, and the machine value `stale_environment` is unchanged.

**Alternatives considered:** carrying the reason through to the label.
That adds a machine-answer value, which this change excludes; it is an open question in the proposal.

### Decision: RemediesAreCliCommands

**Chosen:** a remedy names a CLI command and its flags, for example `c10r build --force` or `--db <path>`.

**Rationale:** the CLI is the shared vocabulary of both surfaces.
An MCP agent maps a flag to the tool parameter of the same name; the MCP layer does not rewrite messages.

### Decision: NewerStoreRecoveryBranchesOnVersion

**Chosen:** the schema-mismatch refusal compares the store's version with the binary's.
An older store gets the rebuild remedy.
A newer store gets "upgrade c10r, or pass `--db` with another path", and the message states that a build would replace the newer store.

**Rationale:** both versions are in scope at the refusal, and a rebuild destroys a store that a newer binary still uses (the installed and working-tree builds share `.c10r/index.db` by default).
`build` still replaces a newer store; whether it should refuse is an open question in the proposal.

### Decision: DamagedStoreRemedyIsForcedBuild

**Chosen:** messages that report a damaged store name `c10r build --force`.

**Rationale:** a plain `c10r build` reports the index current and does nothing when the sources, analyzer, and environment are unchanged, so "rebuild the index" does not work as written.

### Decision: SkippedFileWarningsUseNeutralWording

**Chosen:** the warnings for a path that is not valid Unicode and a file that is not valid UTF-8 say "c10r skipped <file>" with no mention of a build.

**Rationale:** the same source scan runs for a build and for the freshness check of every query, and the scan does not know which one called it.
Printing only during a build is an open question in the proposal.

### Decision: TestsAssertFactsNotWording

**Chosen:** each message test asserts that the message contains the facts the requirement names (the file or tool, the cause, the effect, the remedy command), not the whole string.

**Rationale:** whole-string assertions break on every wording edit and add no protection the fact checks miss.

### Decision: UnreachableMessagesAreRewrittenWithoutScenarios

**Chosen:** messages that normal use cannot reach (`QueryError::MissingSemanticIndex` and `QueryError::MissingFragmentParameters`) are rewritten to the same standard, with unit tests on their text and no spec scenario.

**Rationale:** the store's metadata columns these errors guard are `NOT NULL` in one row, so no test can build the state through the CLI.
The text still matters if a future schema change makes them reachable.

### Decision: WrappersNeverAppendAfterTheMessageTheyWrap

**Chosen:** a wrapper whose inner message is c10r's own complete text passes it through unchanged.
A wrapper that adds context to text c10r does not control (git's output, SQLite's error text, a subprocess's standard error) puts its own sentence first and the outside text last.

**Rationale:** verification found composed messages such as `…did not complete.. The index is unchanged.`, because wrappers appended a sentence after an inner message that already ended with a period.
With the inner or outside text always last, nothing follows text whose ending c10r cannot predict, so the defect cannot occur.

**Alternatives considered:** a helper that trims the inner message's trailing period before appending.
The user rejected it as handling the symptom; the ordering rule removes the cause.

## Verification Waivers

These rewritten messages have no test that produces them, because a test cannot cause their failure reliably.
Their text is checked by review against `Messages say what happened, why, what it means, and what to do`.

| Requirement                                                                     | Message                                                                      | Reason automation is infeasible                                                                 | Manual evidence                      | Recorded   |
| ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ------------------------------------ | ---------- |
| command-surface: Messages say what happened, why, what it means, and what to do | SQLite failure while opening or checking a store (`StoreOpenError::Storage`) | needs SQLite to fail on an otherwise valid store                                                | review of the text at the variant    | 2026-10-12 |
| same                                                                            | file operation failure on a store (`StoreOpenError::Io`)                     | needs a rename or remove to fail partway                                                        | review of the text at the variant    | 2026-10-12 |
| same                                                                            | failure while writing a build (`IngestError::Store`)                         | needs SQLite to fail inside the write transaction                                               | review of the text at the variant    | 2026-10-12 |
| same                                                                            | a path that is not valid Unicode                                             | the test hosts' file systems refuse such a path                                                 | review of the text at the warning    | 2026-10-12 |
| same                                                                            | content-hash mismatch during ingest (`IngestError::ContentHashMismatch`)     | only test code reaches it                                                                       | review of the text at the variant    | 2026-10-12 |
| same                                                                            | failure to remove an index file in `cache`                                   | needs a read-only file system                                                                   | review of the text at the removal    | 2026-10-12 |
| same                                                                            | an unreadable stored edge kind                                               | the traversal reads only the three dependency kinds, so stored data cannot reach the conversion | review of the text at the conversion | 2026-10-12 |

## Known Gaps That Predate This Change

Verification listed coverage gaps in requirements this change restated.
They existed before the change, and the restatement did not create them.
The user chose to fix only the gaps likely to hide a bug; these stay open:

- No production code calls the capability queries of `Optional capabilities are queryable`.
- No test selects eval trials by instance.
- The failed-build clause of `Builds wholly supersede prior derived state` is tested by dropping the write transaction, not by an injected ingest failure.
- The whole-file unmappable clause is tested only for a file the index holds.
- No test passes a twin-bearing index through a live adapter.
- The no-confirmation sentence is tested for `build` only; `hooks_install` uses the same check.
- No test builds over a store a newer c10r wrote; whether `build` should replace such a store is an open question in the proposal.

## Architecture

```text
src/semantic/*_adapter.rs ─┐
src/graph/store.rs ────────┤                                  ┌─▶ stderr (CLI)
src/graph/mod.rs ──────────┼─▶ typed error / warning text ─▶──┤
src/query/mod.rs ──────────┤      (rewritten here)            └─▶ MCP tool error (unchanged text)
src/git.rs, src/hooks.rs ──┤
src/commands.rs, main.rs ──┤
src/render.rs ─────────────┘  (human-render labels and disclosures)
mcp/src/c10r_mcp/*.py ────────▶ MCP-only messages (binary missing, surface mismatch)
```

## Risks

- A message test that matches the old text fails after the rewrite.
  Mitigation: the tasks update such tests to assert facts, per `TestsAssertFactsNotWording`.
- Documentation quotes old message text.
  Mitigation: a task searches `README.md` and `docs/` for each changed string.
- A rewrite states a cause the code does not hold.
  Mitigation: each task names the facts in scope at its site; `messages.md` flags where a cause is not available.
