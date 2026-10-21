# Tasks: force-fresh-store

## Setup

- [ ] Create the worktree `.worktrees/force-fresh-store` on branch `force-fresh-store` from `main`.

## Store (code-graph)

- [ ] Add a `GraphStore` constructor in [src/graph/store.rs](../../../src/graph/store.rs) that creates an empty, stamped store at the temporary path beside a final path and returns it with that path, reusing `create`'s temporary-file steps, and a guard type that removes the temporary store and its side files when dropped unless it was published.
- [ ] Add the publish step: close the connection, rename the temporary store over the final path, and remove the old store's `-wal` and `-shm` side files.
- [ ] Add store-path resolution that follows a symbolic link chain at the final path with `read_link` to the first path that is not a link, including a link whose target does not exist.
- [ ] Add `libc` as a direct dependency in `Cargo.toml` and refresh `Cargo.lock`.
- [ ] Add the free-space check: a pure function that decides from the store size and the free byte count whether a forced build may proceed (store size plus 10%, rounded up), and a `libc::statvfs` reader for the free bytes of the store's directory.
- [ ] Unit test the free-space decision: exactly enough, one byte short, and a zero-size store.
- [ ] Add leftover discovery and removal: list files named `<store file name>.c10r-tmp-<pid>` in the store's directory, skip those whose process is running (`libc::kill(pid, 0)` succeeds or fails with `EPERM`), run `recognize_or_refuse`, and remove recognized ones with their `-journal`, `-wal`, and `-shm` side files, returning the removed paths.

## Build (code-graph)

- [ ] Add `replaced_store: Option<PathBuf>` and `removed_temporary_stores: Vec<PathBuf>` to `BuildOutcome` in [src/commands.rs](../../../src/commands.rs), and set them on every return path of `run_build`.
- [ ] Call leftover removal at the start of `run_build`, before the currency check, on every build.
- [ ] In `run_build`, for a forced build: resolve the store path, run `recognize_or_refuse` on it, run the free-space check when a store exists, then analyze, create the temporary store, ingest into it, publish it, and set `replaced_store` when a store existed.
- [ ] Render `replaced_store` and `removed_temporary_stores` in `build_outcome_json` (each only when present) and in `build_outcome_line`.
- [ ] Test (`A forced build over a current store starts from empty`): build a workspace, force-build it again with confirmation, and assert the workspace was analyzed, the store's rows equal a build from empty, and the answer names the replaced store.
- [ ] Test (`A forced build replaces a store whose tables differ`): create a recognized store at the current schema version whose tables lack a column the binary writes, force-build, and assert the build succeeds and the store has the column.
- [ ] Test (`A forced build with no store creates one`): force-build a workspace with no store and assert a store exists and no replaced store is named.
- [ ] Test (`A failed forced build keeps the existing store`): build, then force-build with an analyzer that fails after starting, and assert the old store is byte-identical, a query against it answers, and no temporary store remains.
- [ ] Test: a forced build whose ingestion fails after the temporary store is created leaves the old store byte-identical and no temporary store behind.
- [ ] Test (`A forced build without room for a second store fails first`): through the build path with the free-space reader replaced by a test value, assert the build fails before analysis with the required, available, and path values in the error, and the store is unchanged.
- [ ] Test (`A forced build through a symbolic link keeps the link`): build through a store path that is a symbolic link, force-build, and assert the path is still a link to the same location, the location holds the new store, and the answer names that location.
- [ ] Test: a forced build through a symbolic link whose target does not exist creates the store at the target and keeps the link.
- [ ] Test (`A forced build shrinks a store left large by an earlier build`): build a large workspace, rebuild the store for a small one without force, force-build, and assert the file is no larger than a store built from empty for the small workspace.
- [ ] Test: a forced build over a file the system does not recognize is refused with the ownership error and leaves the file unchanged.
- [ ] Test (`A stopped forced build's temporary store is removed`): place a recognized temporary store named with the process ID of a process that has exited, run a build, and assert the file is removed and named in the answer; once through a build that does work and once through a build of a current index.
- [ ] Test (`A running build's temporary store is kept`): place a recognized temporary store named with the current process's ID, run a build, and assert it remains.
- [ ] Test (`An unrecognized file with a temporary-store name is kept`): place an unrecognized file with a temporary-store name and an exited process's ID, run a build, and assert it remains.
- [ ] Test: a non-forced build's answer carries neither new field, and its JSON is byte-identical to the answer before this change for the same workspace.

## Confirmation (command-surface)

- [ ] Add `--yes` to the `build` arguments in [src/cli.rs](../../../src/cli.rs) with `requires = "force"`, and help text saying it confirms a forced build without asking.
- [ ] Add the `confirm` closure parameter to `run_build` and call it for a forced build without `--yes` after the free-space check and before the analyzer is resolved; a no returns `Failure::Usage` naming `--yes`.
- [ ] In [src/main.rs](../../../src/main.rs), build the closure: when standard input and standard error are both terminals, write the question to standard error and read one line; `y` or `yes`, ignoring case and surrounding space, is yes; anything else and end of input are no; without terminals, answer no without asking.
- [ ] Pass a closure that never asks from the hook runner and from every other `run_build` caller.
- [ ] Test (`The flag confirms a forced build without a terminal`): through the built binary with no terminal, `build --force --yes` replaces the store without reading input.
- [ ] Test (`A forced build without a terminal or the flag is refused`): through the built binary with no terminal, `build --force` exits with the usage-error code, names `--yes`, and leaves the store byte-identical.
- [ ] Test (`A yes at the terminal confirms a forced build`): through `run_build` with a closure that records the question and answers yes, assert the question names the replaced index and the store is replaced.
- [ ] Test (`Any other answer refuses a forced build`): through `run_build` with closures answering no, an empty line, and end of input, assert each is refused and the store is unchanged.
- [ ] Unit test the answer parsing in the closure: `y`, `Y`, `yes`, `YES`, `n`, empty, `yep`.
- [ ] Test (`The confirmation flag alone is a usage error`): through the built binary, `build --yes` exits with the usage-error code and creates no store.
- [ ] Test (`Runs to completion without a terminal`): the existing non-interactive tests still pass, and a forced build without a terminal never blocks on input.
- [ ] Raise `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) from 14 to 15, regenerate `tests/fixtures/surface_manifest.snapshot.json`, and update the MCP package's pinned surface version.

## MCP (mcp-surface)

- [ ] In [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), change the forced build's cost text to state that it replaces the stored index with a new one built from scratch, keep the displayed command and shell remedy as `c10r build --force`, and append `--yes` only to the arguments run after confirmation.
- [ ] Test (`A confirmed forced build replaces the index`): with a client that offers elicitation and confirms, the confirmation text states the replacement, the command run carries `--force --yes`, and the store is replaced.
- [ ] Test: the shell remedy for a client without elicitation names `c10r build --force` and not `--yes`.
- [ ] Test: an unforced build's confirmation and command are unchanged.

## Documentation

- [ ] Update [docs/commands/build.md](../../../docs/commands/build.md): what `--force` does now, `--yes`, the prompt, the free-space check and its 10% margin, symbolic links, leftover cleanup, and the two new answer fields.
- [ ] Update the `force` description of the MCP `build` tool to say a forced build replaces the index and is confirmed by the user.

## Checks

- [ ] Run the full Rust test suite and the MCP test suite.
- [ ] Run the pre-commit hooks on every changed and new file, and fix any failure.
