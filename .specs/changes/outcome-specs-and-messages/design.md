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
