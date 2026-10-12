# Tasks: Outcome Specs and Messages

Message IDs (`A1`, `C12`, …) refer to `messages.md` in this change directory, which gives each message's location, current text, missing parts, and proposed text.
Every rewrite follows `Messages say what happened, why, what it means, and what to do` and the decisions in `design.md`.
Every message test asserts facts, not whole strings (`TestsAssertFactsNotWording`).

## Setup

- [ ] Create the worktree `.worktrees/outcome-specs-and-messages` on branch `outcome-specs-and-messages` from `main`, and run `git update-index --assume-unchanged evals/.env.example` in it.
- [ ] Run the full Rust and MCP test suites before any change and record the counts.

## Semantic engine messages

- [ ] Rewrite the Rust adapter messages `A1`–`A6`: a missing `rust-analyzer` names the install command (`rustup component add rust-analyzer`) and the `--rust-analyzer` flag; an unresponsive one is reported as unresponsive, not unavailable; a failed `--version` keeps the exit status and error output the code holds; a nonzero `rust-analyzer scip` exit says that the tool's own output is printed above and that no index was built.
- [ ] Rewrite the Python adapter messages `A7`, `A9`, and `A11`–`A16` the same way; a failed `--version` from an installed `scip-python` no longer says to install it.
- [ ] Unit tests for the adapter messages: a missing tool names the install command and the flag; an unresponsive tool is not called absent; a failed version probe carries its exit status.
- [ ] Integration test (`A missing indexer is reported with its install step`): a Rust build on a `PATH` without `rust-analyzer` fails with an error naming `rust-analyzer`, the cause, that no index was built, the install command, and `--rust-analyzer`.
- [ ] Confirm that an existing test covers `Unavailable backend is a typed refusal, never a partial index` leaving an existing store byte-identical; add one in `tests/semantic_engine.rs` if none does.
- [ ] Confirm that existing tests cover the restated `Same-descriptor definitions extracted as distinct symbols` and `Optional capabilities are queryable`, and update any test comment that quotes removed wording.

## Code graph messages

- [ ] Rewrite the schema-mismatch refusal `S5` per `NewerStoreRecoveryBranchesOnVersion`: an older store names `c10r build`; a newer store names upgrading c10r or `--db` with another path, and states that a build would replace the newer store.
- [ ] Integration test (`A query against a newer store names the upgrade`): stamp a store with a schema version above the binary's, run a query, and assert the refusal names both versions and the upgrade remedy and does not name `c10r build`.
- [ ] Integration test: a query against a store with an older schema version names both versions and `c10r build`.
- [ ] Rewrite `S6`–`S8` (store errors that pass raw SQLite text through) to state which store operation failed, on which path, and that no answer was produced.
- [ ] Rewrite the damaged-store messages `Q1`–`Q5` per `DamagedStoreRemedyIsForcedBuild` to name `c10r build --force`, and `Q6`–`Q7` (ingest errors) to state that the build stopped and the existing store is unchanged.
- [ ] Unit tests for `Q2` and `Q3` text per `UnreachableMessagesAreRewrittenWithoutScenarios`, and an integration test for one reachable damaged-store message naming `c10r build --force`.
- [ ] Confirm that existing tests cover the restated `Per-symbol tier content`, `Staleness reflects underlying change`, `Reference occurrences carry enclosing-declaration attribution`, and `Builds wholly supersede prior derived state`, and update any test comment that quotes removed wording.

## Code navigation messages

- [ ] Rewrite the git boundary messages `G1`, `G2`, `G3`, and `G5`: the absent-git message names the command that needed git instead of always `impact`; the not-a-worktree message prints the absolute path, never `.`; the timeout message says git did not answer within 10 s and that no answer was produced, without "bounded deadline".
- [ ] Integration tests: `hooks install` with no `git` on `PATH` does not mention `impact`; `impact` run from a directory outside any worktree prints the absolute path.
- [ ] Confirm that existing tests cover the eight restated code-navigation requirements, and update any test comment that quotes removed wording.

## Command surface messages

- [ ] Rewrite `M1` and `M3` in `src/main.rs`: the doctor failure says which indexer is not ready and why (absent or unresponsive) and that `build` cannot index that language; `hooks run` with no event lists the accepted events.
- [ ] Rewrite `C1`–`C4`, `C6`, `C12`, `C15`, `C16`, `C18`, `C30`, `C31`, and `C36`–`C38` in `src/commands.rs`; the path warnings `C2` and `C3` follow `SkippedFileWarningsUseNeutralWording`; the no-index messages `C4` and `C6` use one wording.
- [ ] Rewrite `C33` (a failed refresh from a hook): it says the index is now stale and names the command that refreshes it.
- [ ] Rewrite the hook messages `H1`, `H2`, and `H4` in `src/hooks.rs`.
- [ ] Rewrite the human-render labels and disclosures `R1`, `R3`, `R4`, `R6`, `R7`, and `R11` in `src/render.rs`; `R1` follows `MessagesStateOnlyObservedCauses`, and `R3`/`R4` state what a workspace mismatch means for the answer and how to build for this workspace.
- [ ] Integration test (`A skipped file is reported with its effect and remedy`): a workspace with a non-UTF-8 source file builds with a warning naming the file, the cause, that no answer shows its code, and re-saving as UTF-8.
- [ ] Integration test (`A stale label claims no cause the system did not observe`): a Python index recorded with an environment, queried where the environment does not resolve, renders a stale label that does not say the environment changed.
- [ ] Integration test (`A usage error names the accepted input`): `hooks run` with no event lists every accepted event.
- [ ] Integration test: a failed refresh from `hooks run` writes a message naming the event, the workspace, that the index is stale, and the refresh command, and exits 0.
- [ ] Confirm that existing tests cover the restated `Shell completion scripts` and `Refresh-hook installation`, and that `tests/hooks.rs` asserts one refresh for an amended commit (`An amended commit refreshes the index once`); add the assertion if it is missing.

## MCP surface messages

- [ ] Rewrite `P-3`, `P-6`, `P-7`, and `P-10` in `mcp/src/c10r_mcp/`: a missing `c10r` binary names the install command; a surface-version mismatch names both versions and which side to upgrade.
- [ ] MCP tests for the rewritten messages, asserting facts.
- [ ] Confirm that `test_an_unacknowledged_invocation_asks_the_client_nothing` covers the new sentence in `A lifecycle operation proceeds only on confirmed consent`.

## Eval telemetry

- [ ] Confirm that an existing test in `evals/tests/` selects trials and reads pre-run task characteristics from recorded values without the trajectory; add one if none does.

## Documentation

- [ ] Search `README.md` and `docs/` for every changed message text, and update each quote.

## Checks

- [ ] Run the full Rust test suite, the MCP test suite, and the eval test suite.
- [ ] Run the pre-commit hooks on every changed and new file, and fix any failure.
