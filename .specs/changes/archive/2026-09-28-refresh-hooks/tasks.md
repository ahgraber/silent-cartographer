# Tasks: refresh-hooks

## Hook file model

- [x] Add a module `src/hooks.rs` (registered in `src/lib.rs`) that owns the generated hook file: the four refresh events as an enum (`post-commit`, `post-rewrite`, `post-checkout`, `post-merge`), the marker line, and a `render(event, workspaces)` function producing the script in design.md § HookFileLayout.
  Move `post_commit_hook_script`'s shell quoting into this module and delete the old function.
- [x] Add `parse(text) -> Option<Vec<WorkspaceLine>>` that returns `None` for a file without the marker and the workspace lines (root, db, workspace) for a c10r file.
- [x] Unit test: `render` then `parse` returns the same workspace lines, including a root and a db path that contain spaces and single quotes.
- [x] Unit test: a rendered script runs the kept hook before the `hooks run` lines and exits with the kept hook's status (run the rendered text with `sh` against a stub `c10r` on `PATH` and a kept hook that exits 3).

## prek shim recognition

- [x] Capture the post-commit, post-checkout, post-merge, and post-rewrite shims written by the installed prek (`prek --version` recorded in the fixture file name) into `tests/fixtures/prek-shims/<version>/`.
- [x] Add `is_recognized_prek_shim(text, event) -> bool` in `src/hooks.rs` that matches the whole file against each captured form, with the `# ID:` line and the `PREK=` path as the only variable parts.
- [x] Unit tests: each captured fixture is recognized for its own event and not for another event.
  A file with only the prek header, a fixture with the `exec … hook-impl` line removed, and a fixture with an extra line are not recognized.

## Refresh rules

- [x] Add `should_refresh(event, git_args, rebase_state) -> bool` in `src/hooks.rs` implementing design.md § RefreshRules, where `rebase_state` is `None`, `InProgress { todo_entries: usize }`.
- [x] Add `read_rebase_state(repo)` in [src/git.rs](../../../src/git.rs): resolve `git rev-parse --git-path rebase-merge`, and count `git-rebase-todo` lines that are neither blank nor `#` comments.
- [x] Unit tests for `should_refresh`, one per table row and branch: `post-commit` idle and in a rebase; `post-rewrite rebase` and `post-rewrite amend`; `post-checkout` flag `0`, flag `1` idle, flag `1` in a rebase with entries, flag `1` in a rebase with none; `post-merge 0` and `post-merge 1`.

## Placement

- [x] Add a `HookSlot` resolution in `src/hooks.rs` for one event: find a c10r file at `E` or `E.legacy`; otherwise pick `E.legacy` when the file at `E` is a recognized prek shim, and `E` otherwise (design.md § InstallPlacement).
- [x] Add `kept_hooks_identical(a, b)`: compare file types without following links; compare two symbolic links by link target string and two regular files by bytes; any other pair is different.
- [x] Add an atomic write helper: write to a temporary file in the hooks directory, set mode `0o755`, and rename into place.
- [x] Add `install_plan(hooks_dir, workspace, overwrite)` that reads all four events, checks that present c10r files agree on their workspace set, checks each `E.c10r-prior` collision, and returns either a list of file operations or a refusal naming the paths.
  It writes nothing.
- [x] Add `apply_plan` that performs the operations: move the slot's file to `E.c10r-prior` (or delete it with `--overwrite`), preserving a symbolic link as a link, then write c10r's file.
- [x] Add `uninstall_plan(hooks_dir, workspace)` and apply it: remove the workspace from every c10r file that names it; when a file is left with no workspace, delete it and move `E.c10r-prior` into its slot.
- [x] Unit tests on temporary directories for each placement case: empty directory; foreign file at `E`; recognized prek shim at `E`; recognized prek shim at `E` plus a foreign `E.legacy`; unrecognized file at `E` plus a separate `E.legacy`; c10r file already at `E` and at `E.legacy`; identical and different `E.c10r-prior`; symbolic links with the same and different targets; `--overwrite`; disagreeing c10r files.

## Command surface

- [x] In [src/cli.rs](../../../src/cli.rs), add `Uninstall` and `Run` to `HookAction`, and add to `HooksArgs` an optional positional `event`, `--overwrite`, `--root`, and trailing arguments after `--`, per design.md § HooksCommandShape.
  `Run`'s help text says the installed hooks call it.
- [x] Reject an argument given with the wrong action as a usage error that names the action it belongs to.
- [x] Rewrite `run_hooks_install` in [src/commands.rs](../../../src/commands.rs) to build and apply the install plan for the resolved workspace, and report every path written, kept, or left unchanged.
- [x] Add `run_hooks_uninstall` and `run_hooks_run`.
  `run_hooks_run` reads the rebase state, calls `should_refresh`, calls `run_build` with the line's root, db, and workspace, writes any failure to standard error naming the event and workspace, and always returns success.
  It prints the build report when it builds and prints nothing when the rules skip the event.
- [x] Dispatch the new actions in [src/main.rs](../../../src/main.rs), and extend `render_hook_report` for uninstall.
- [x] Update the `hooks install` unit test in [src/commands.rs](../../../src/commands.rs) that asserts the exact `post_commit_hook_script` text so it asserts the rendered `post-commit` script.
  Done by deletion: the test's function is gone, and the round-trip test in `src/hooks.rs` covers quoting of values with spaces and single quotes.

## Status report

- [x] Extend `doctor` in [src/commands.rs](../../../src/commands.rs) with a hook section: per event, one of installed (with workspaces), present with activation unverified, not installed (with the fix: the install command, or the remaining `E.c10r-prior` path and the need to remove or rename it), and a disagreement listing each file's workspaces.
  Outside a git worktree, report that hook status does not apply.
  Hook status never changes the exit code.
- [x] Add the hook section to `doctor`'s human rendering and its JSON answer.

## Stale refresh command

- [x] Add an optional `refresh` field to `Answer` in [src/query/output.rs](../../../src/query/output.rs), serialized only when present.
- [x] Build the command line in [src/commands.rs](../../../src/commands.rs) from the recorded root, the `--db` path, the workspace, the environment in effect for the query (from `current_state`), and the recorded chunk parameters when they differ from the defaults.
  When the root is not recorded, set the field to a text stating that the root is unknown.
  Set it on every stale answer.
- [x] Print the command after the stale marker in [src/render.rs](../../../src/render.rs).
- [x] Tests in [tests/code_navigation.rs](../../../tests/code_navigation.rs), one per scenario of **A stale answer names its refresh command**: stale after changed sources (the refresh names the index location, workspace, and root); stale after a changed analyzer; stale after a changed environment, where running the command and repeating the query under the same environment gives a fresh answer; running the command after changed sources makes the answer fresh; a fresh answer has no `refresh` field; the human rendering includes the command.
- [x] Test that a stale answer from each query command that sets the field (`get`, `trace`, `dependents`, `find`, `search`, `similar`, `impact`) carries the refresh command, because each command sets it separately.
- [x] Test that a stale answer from an index with no recorded root says that the root is unknown, and that the text is a shell comment, so pasting it into a shell runs nothing.

## Integration tests through the built binary

All tests in [tests/hooks.rs](../../../tests/hooks.rs) use real git repositories with the built binary first on `PATH`.
A test counts refreshes by the build reports in the git command's output, and checks that the index matches a commit with `status`.
A stand-in hook manager script, which moves the event file to `E.legacy` and runs it, stands in for the migration convention where prek itself is not required.

- [x] Replace `refuses_to_overwrite_an_existing_hook` with `keeps_an_existing_hook_and_runs_it`.
  The **Commit-hook installation** requirement it tested is removed by this change, so this replaces the test rather than altering a failing one.
- [x] Update the existing hook-content tests (`the_hook_carries_the_callers_db_workspace_and_root`, `the_installed_hook_runs_c10r_build_with_the_resolved_arguments`) to the four-event layout.
- [x] Refresh-hook installation: `hooks_installed_where_none_exist`, `a_second_workspace_joins_the_installed_hooks`, `reinstalling_a_workspace_changes_nothing`, `a_removed_refresh_hook_is_restored_for_every_workspace`, `disagreeing_refresh_hooks_are_refused`, `installation_leaves_git_configuration_unchanged`.
  Keep the existing outside-a-worktree, linked-worktree, relocated-hooks-path, and subdirectory tests passing.
- [x] Refresh timing: `a_commit_refreshes_the_index`, `an_amended_commit_refreshes_once`, `a_rebase_that_replays_several_commits_refreshes_once`, `a_rebase_with_nothing_to_replay_refreshes_once`, `an_apply_backend_rebase_refreshes_at_most_twice` (once with `--apply`, once with `rebase.backend=apply`), `a_paused_rebase_does_not_refresh`, `an_aborted_rebase_leaves_the_index_at_the_original_commit`, `a_branch_checkout_refreshes_the_index`, `a_merge_refreshes_the_index` (fast-forward and merge commit), `a_squash_merge_does_not_refresh`, `a_file_checkout_does_not_refresh`, `a_failed_refresh_does_not_fail_a_branch_checkout`.
- [x] Existing hooks keep running: `a_kept_hook_receives_the_events_standard_input`, `a_kept_hooks_failure_still_fails_the_checkout`, `a_non_executable_kept_file_is_not_run`, `a_kept_symbolic_link_keeps_its_target`, `an_unrecognized_hook_at_the_event_path_is_chained`, and `a_pre_existing_hook_survives_the_tools_installed_around_it` driven by the stand-in manager over every order in the scenario.
- [x] Existing hooks with a recognized prek shim: install a captured fixture shim (with `PREK` pointing at a stub that runs `E.legacy` and records a line), then check `a_supported_hook_manager_installed_first_keeps_running` and `a_supported_hook_manager_installed_later_keeps_both_running`.
- [x] Kept-hook collision: `an_identical_remaining_hook_is_replaced_silently`, `a_different_remaining_hook_stops_installation`, `symbolic_links_with_different_targets_are_different`.
- [x] Hook replacement on request: `overwrite_replaces_an_existing_hook`.
- [x] Refresh-hook removal: `removing_one_of_two_workspaces`, `removing_the_last_workspace_restores_a_kept_hook`, `removing_the_last_workspace_with_no_kept_hook`, `removing_while_a_supported_hook_manager_is_installed` (fixture shim with stub), `removing_from_disagreeing_refresh_hooks`, `removing_a_workspace_that_is_not_installed`, and a check that removal leaves git configuration unchanged.
- [x] Refresh-hook status report, in [tests/operational_commands.rs](../../../tests/operational_commands.rs): `installed_hooks_are_reported`, `hooks_under_a_supported_hook_manager_are_reported_installed`, `hooks_under_an_unrecognized_file_are_reported_unverified`, `a_removed_hook_is_reported_with_its_fix`, `a_remaining_kept_hook_is_reported_with_its_path`, `disagreeing_hooks_are_reported`, `hook_status_outside_a_git_worktree`.

## Surface version and MCP

- [x] Bump `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) and the pin in [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py).
- [x] Update the `hooks_install` tool description in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py) and the `acknowledge` refusal text to say the hooks refresh on commits, rebases, checkouts, and merges.
- [x] Run the MCP test suite (`mcp/tests`) and the surface-manifest tests, and confirm they pass against the bumped surface.

## Documentation

- [x] Rewrite [docs/commands/hooks.md](../../../docs/commands/hooks.md): the three actions, `--overwrite`, the events and when each refreshes, chaining, prek support, the `--apply` two-refresh bound, and what `reset --hard` and `stash pop` leave stale.
- [x] Add the hook section to [docs/commands/doctor.md](../../../docs/commands/doctor.md), and the `refresh` field to the answer documentation in [docs/commands/common.md](../../../docs/commands/common.md).
- [x] Update the `hooks` row and the hook paragraph in [README.md](../../../README.md).

## Canonical environment path

- [x] Change `resolve_environment` in [src/semantic/python_adapter.rs](../../../src/semantic/python_adapter.rs) to return the canonical absolute path for an explicit path, `$VIRTUAL_ENV`, and a `.venv` or `venv` directory, and the resolved path when canonicalization fails.
  Update its unit tests that assert a relative result.
- [x] Update the `refresh_command` docstring in [src/commands.rs](../../../src/commands.rs): the environment is now canonical and absolute.
- [x] Tests in [tests/code_navigation.rs](../../../tests/code_navigation.rs), one per scenario of **Freshness does not depend on how the workspace root is named**: a Python workspace with a `.venv` and no activated environment, built with an absolute root, answers fresh to a query from the workspace directory; the same workspace built with `.` answers fresh.
  Confirm the first test fails before the change.
- [x] Confirm the stale-environment refresh test still passes, and that the refresh command now names an absolute environment path.

## Fresh fixtures for the human-render sanitization tests

- [x] Add a test-support helper that records the fixture's real content hash and an analyzer version the query will see, so a fixture answers fresh.
- [x] Use it in the fixtures of the five sanitization tests in [tests/human_render.rs](../../../tests/human_render.rs), and restore their original line counts and comments, so the tests no longer depend on the stale-answer layout.

## Code review remediation

- [x] Hook builds and the stale refresh command use the backend the index records: `hooks run` passes the recorded language to `run_build` when an index exists, and `refresh_command` adds `--language`.
  Integration test: a workspace with both a `Cargo.toml` and a `pyproject.toml`, built with `--language rust`, refreshes on a commit, and its stale answer's refresh command makes the answer fresh.
- [x] Install writes c10r's file to a temporary file before moving the slot's file, and renames it into the slot last; uninstall restores a kept hook by renaming it over c10r's file.
  Unit test with an injected write failure: the kept hook stays at its original path.
- [x] The generated script runs the kept hook only when it is a regular file and executable.
  Test: a directory at `post-checkout.c10r-prior` does not fail a branch checkout.
- [x] Hook lines and the refresh command use the `--flag=value` form.
  Test: a workspace named `-foo` refreshes on a commit.
- [x] Installation refuses a root, index path, or workspace identity that contains a line break (spec scenario **A workspace path with a line break is refused**).
- [x] `doctor` reports an event installed only when git will run the refresh hook (executable file at `E`, or executable file at `E.legacy` under an executable recognized prek shim), and reports a c10r file whose content differs from the rendered file as modified, naming the kept hook as the place for the user's own commands.
  Tests for the two new spec scenarios; the doctor test helper sets the executable bit, as installation does.
- [x] Every `c10r` process the hook and doctor integration tests start gets the same git config isolation as their `git` processes.
- [x] Document the `modified` state in [docs/commands/doctor.md](../../../docs/commands/doctor.md), and in [docs/commands/hooks.md](../../../docs/commands/hooks.md) that installation and removal replace c10r's hook files and that the user's own commands belong in `<event>.c10r-prior`.

## Second review remediation

- [x] The doctor tests resolve their fixture hooks directory with the same git config isolation as the `c10r` they start, so a host `core.hooksPath` cannot receive fixture files.
  Test: run one doctor hook test with a global git config that sets `core.hooksPath`; nothing is written there and the test passes.
- [x] A `modified` refresh hook is not reported as refreshing its workspaces (JSON and human output name them as workspaces the file names, as `unverified` does).
  Update the spec scenario test **A hand-edited hook is reported modified** and `docs/commands/doctor.md`.
- [x] The install command `doctor` suggests uses the `--flag=value` form.
  Test: with a workspace named `-foo` and one event's refresh hook removed, running the suggested command restores it.
  Update the example in `docs/commands/doctor.md`.
- [x] If the final rename of c10r's file fails after a kept hook moved, rename the kept hook back.
  Unit test with an injected failure at the final rename: the kept hook is at its original path.
- [x] Stage every event's file before any file in the hooks directory changes, and let installation proceed when the refresh hooks differ only by the workspace being installed.
  Tests: a staging failure on the second event changes no file; the spec scenario **A retry completes an installation that failed partway**; files that differ by another workspace are still refused.
  If this proves impractical, fall back to naming the recovery commands (`c10r hooks uninstall` for the partly installed workspace, then `c10r hooks install`) in `doctor`'s disagreement report, and record the fallback here.

## Structural changes

- [x] Fold `EventInstallPlan`'s `write` and `kept` into one value so a plan cannot move a kept hook without writing c10r's file.
  Behavior is unchanged; verified by the full suite.
- [x] Move the staleness check into `Answer::with_refresh`, so a fresh answer cannot carry a refresh command, and remove the wrapper in `src/commands.rs` that performs the check.
  Fresh answers' JSON stays byte-identical; verified by the full suite.

## Evidence and checks

- [x] Copy `chain-v3.sh`, `matrix.sh`, `prek-late-legacy.sh`, and `reflog-action.sh` from `._scratch/refresh-hooks-debate/` into `.specs/changes/refresh-hooks/evidence/`, and change `chain-v3.sh` to call the built binary's `hooks install` and `hooks uninstall` in place of its simulated functions.
- [x] Run the changed matrix with the installed prek, and record the prek version, the commands, and the full output in `.specs/changes/refresh-hooks/evidence/prek.md`.
  This is the manual evidence the design's verification waiver names.
- [x] /simplify and /refactor audits, especially for code- and comment-sprawl.
- [x] Run `cargo fmt`, `cargo clippy`, the full Rust test suite, and the MCP test suite, and report the results.
- [x] Run every hook in `.pre-commit-config.yaml` on all changed and new files, and fix any failure.

## Verify remediation

- [x] Test `doctor`'s human hook section in [tests/operational_commands.rs](../../../tests/operational_commands.rs): one test that runs `doctor` without `--json` and asserts the line for each state (installed, modified, present with activation unverified, not installed with its fix), the disagreement line, and the not-applicable line outside a git worktree.
- [x] Test the two `--overwrite` paths that delete a kept hook: with c10r's file already installed and an `E.c10r-prior` present, and with an empty slot and an `E.c10r-prior` present.
  Each asserts that the kept hook is deleted and that it does not run on the next event.
- [x] Test the **Refresh-hook event action** requirement through the built binary: `hooks run post-checkout` with file flag `0` prints nothing and exits 0; `hooks install post-commit`, `hooks install --root=…`, `hooks install -- x`, `hooks uninstall --overwrite`, `hooks run --overwrite`, and `hooks run` without an event or `--root` exit with the usage-error code, name the owning action, and change no hook file.
- [x] Test that a plain install over existing hook files removes no file: compare the hooks directory listing and each kept hook's bytes before and after.
- [x] Test the line-break refusal for an index path and a workspace identity that contain a line break.
- [x] Test the retry of a partial installation through the built binary, with the first present hook file lacking the workspace being installed.
- [x] Kept-hook collision tests: assert that the refusal states the remaining hook must be removed or renamed; add a link-versus-regular-file pair with the same bytes, which is refused; assert that no file changes in the symbolic-link refusal tests.
- [x] Status report tests: a refresh hook at `E.legacy` under a recognized prek shim that is not executable, and under an executable shim with a legacy file that is not executable, are not reported installed; a modified hook at `E.legacy` is reported modified.
  `hook_status_outside_a_git_worktree` asserts that the exit code equals the indexer-readiness code with ready stub indexers.
  `a_non_executable_hook_is_not_reported_installed` asserts the `not_installed` state and its fix.
- [x] Test that uninstalling the last workspace restores a kept symbolic link as a link to the same target.
- [x] Test the error path of `hooks run` when the workspace root is not in a git worktree: exit 0, a standard-error line naming the event and the workspace, and nothing on standard output.
- [x] Test the stale refresh command for an index built with non-default chunk parameters (the command carries them, and running it makes the answer fresh) and for a workspace identity that starts with `-`.
- [x] Test that an environment given through `$VIRTUAL_ENV` with a non-canonical spelling is recorded and compared canonically, so the query answers fresh.
  Change `env_resolution_prefers_explicit_then_virtual_env_then_dot_venv` to use non-canonical inputs, so it fails without canonicalization.
- [x] `status` carries the `refresh` field when the index is stale, built by `refresh_command`, and omits it when fresh.
  Tests for the scenario **The status report names the refresh command**, in the machine answer and the human rendering.
- [x] `status` without `--json` prints labeled human lines projected from the JSON report (workspace, analyzer, freshness, refresh command when stale, workspace relation, join-alignment counts, duplicated-descriptor count, semantic-index identity, and the optional discrepancy and duplicate detail), with every field value sanitized.
  Test that every field the JSON report carries appears in the human rendering, and that a hostile value is sanitized.
  Update [docs/commands/status.md](../../../docs/commands/status.md) with an example.
- [x] Run `cargo fmt`, `cargo clippy`, the full Rust test suite, the MCP test suite, and every hook in `.pre-commit-config.yaml` on all changed files, and fix any failure.
