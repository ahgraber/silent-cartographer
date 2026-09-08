# Tasks: index-store-actions

## Surface definition

- [x] In [src/cli.rs](../../../src/cli.rs), add a `CacheAction` `ValueEnum` with `Clear`, `Dir`, and `Size`, each carrying a doc comment that becomes its help text, and a `CacheArgs` struct holding it as a required positional — the same shape `HookAction`/`HooksArgs` already use.
- [x] Change `Command::Cache` to `Cache(CacheArgs)` and rewrite its doc comment: the command is the index store, and the action decides whether it is removed, located, or measured.
- [x] Update the module doc comment at the top of [src/cli.rs](../../../src/cli.rs) where it lists the operational set, so the `cache` entry names the action axis.

## Shared store classification

- [x] In [src/commands.rs](../../../src/commands.rs), extract the target classification currently inlined in `index_state` into a `store_state` function returning an enum over absent, unrecognized, unexaminable, incompatible, and recognized, keeping the existing comments that explain why an unexaminable path and an incompatible store are each a distinct state.
- [x] Rewrite `index_state` to call `store_state` and map its result onto the JSON it emits today, leaving the emitted field names and values byte-identical.
- [x] Run the existing `manifest` index-state tests (`manifest_distinguishes_an_unrecognized_store_from_an_absent_one`, `manifest_distinguishes_an_unexaminable_path`, `manifest_distinguishes_an_incompatible_store`, `manifest_enumerates_the_surface_through_the_binary_with_and_without_an_index` in [tests/surface_manifest.rs](../../../tests/surface_manifest.rs)) and confirm they pass unchanged.
  This is the evidence that the extraction did not change `manifest`'s answer.

## The `dir` action

- [x] Add a `CacheDirOutcome` and a `run_cache_dir` in [src/commands.rs](../../../src/commands.rs) that derives the directory from the `--db` path as given, yielding `.` when the path has no parent component, and touches no file.
- [x] Add `render_cache_dir_report`: the directory alone in the human rendering, and the directory plus the store path under `--json`, both routed through `crate::render::sanitize` as the other operational reports are.
- [x] Add a unit test in [src/commands.rs](../../../src/commands.rs) covering the path arithmetic directly: a nested path reports its parent, and a bare filename reports `.`.

## The `size` action

- [x] Add a `CacheSizeOutcome` in [src/commands.rs](../../../src/commands.rs) carrying the target's state and an optional byte total, with the state spelled as `index_state` spells it (`unrecognized`, `unreadable`, `incompatible`) plus `recognized` and `absent`.
- [x] Add `run_cache_size`: classify through `store_state`; for a recognized or incompatible store, sum the store file and its `-wal`/`-shm` sidecars, building each sidecar path by pushing the suffix onto the primary path's os-string exactly as `run_cache` does; for every other state, report the state with no byte total.
  Return `Ok` in all cases.
- [x] Add `render_cache_size_report`: the byte total with the store path in the human rendering, a state-naming line when there is no total, and the state plus the optional total under `--json`.

## Dispatch

- [x] In [src/main.rs](../../../src/main.rs), replace the `Command::Cache` arm with a match over `args.action` dispatching to the three handlers and their renderers, mirroring the `Command::Hooks` arm.

## Surface version and the MCP pin

- [x] Bump `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) from 9 to 10.
- [x] Regenerate [tests/fixtures/surface_manifest.snapshot.json](../../../tests/fixtures/surface_manifest.snapshot.json) with `UPDATE_SURFACE_SNAPSHOT=1 cargo test --test surface_manifest snapshot -- --nocapture`, the command the snapshot test's failure message gives, and confirm the `cache` entry now carries a required positional argument whose values are `clear`, `dir`, and `size`.
- [x] Bump `SURFACE_VERSION` in [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py) from 9 to 10 and run `mcp/tests/test_startup.py`, which asserts the server refuses a binary reporting any other version.
- [x] Confirm `test_no_tool_removes_the_stored_index` in [mcp/tests/test_tools.py](../../../mcp/tests/test_tools.py) still passes.
  It asserts that no advertised tool is named for the cache or for a reset, and this change adds no MCP tool.

## Existing tests move onto the explicit action

- [x] Update every `cache` invocation in [tests/operational_commands.rs](../../../tests/operational_commands.rs) to `cache clear`, and update the module doc comment naming what the file covers.
- [x] Update the two `cache` invocations in [tests/command_surface.rs](../../../tests/command_surface.rs) (`cache_rejects_bounding_flags_and_removes_nothing`, and the literal-store removal at the end of the `file:`-prefixed store test) to `cache clear`.
- [x] Run the completions tests in [tests/completions.rs](../../../tests/completions.rs) and confirm the top-level command list still holds.
  `cache` remains a top-level command, so the emitted script needs no fixture change beyond what the parser generates.

## Process-level evidence for the new contract

- [x] Add a test in [tests/operational_commands.rs](../../../tests/operational_commands.rs) that drives the built binary's bare `cache` over a workspace with a stored index, and asserts the process exits with the usage code, standard error names all three actions, and the index is still on disk.
- [x] Add a test that `cache dir` over a workspace with a stored index reports the containing directory and exits with the success code, and that `cache dir --json` carries both the directory and the store path.
- [x] Add a test that `cache dir` against a `--db` path with no store present still reports the directory and exits with the success code.
- [x] Add a test that `cache size --json` over a recognized store reports a byte total equal to the store file's own length, with the state `recognized`.
- [x] Add a test that `cache size --json` over a recognized store accompanied by `-wal` and `-shm` files reports a total equal to the sum of all three.
  The bare-store test above does not cover the sidecars.
- [x] Add a test that `cache size --json` over a store carrying an older schema version reports a byte total, the state `incompatible`, and the success code, reusing the fixture idiom `cache_removes_a_store_with_an_older_user_version` already uses to stamp one.
- [x] Add a test that `cache size --json` against an absent store reports the `absent` state with no byte total and the success code.
- [x] Add a test that `cache size --json` against a non-c10r file at the store path reports the `unrecognized` state with no byte total, exits with the success code, and leaves the file intact.
- [x] Add a test that `cache size --json` against a directory at the store path reports the `unreadable` state with no byte total and exits with the success code.

## Documentation

- [x] Rewrite [docs/commands/cache.md](../../../docs/commands/cache.md) around the three actions: what each answers, the required-action rule, `size`'s definition as the size of the files `clear` removes, and the states `size` reports instead of a number.
- [x] Update the `cache` row of the command table in [README.md](../../../README.md) so the question it answers covers all three actions.
- [x] Confirm the README's MCP paragraph is still accurate: no tool on that surface removes an index store.
  Leave it otherwise unchanged.

## A symbolic link at the store path

An external review found `clear` reporting a bare removal for a symlinked `--db` while the store it named survived.
The removal is unchanged — unlinking a link cannot reach the file behind it — but the answer now discloses which one went.

- [x] In `run_cache` ([src/commands.rs](../../../src/commands.rs)), test the store path with `symlink_metadata`, which does not follow the link, and carry the verdict on `CacheOutcome`.
- [x] Warn on standard error when the path is a link, naming that the link went and the index it names stayed.
- [x] Give the human rendering its own line for that case, and let the `--json` answer carry the flag, so a caller reading only the machine answer still learns the index survives.
- [x] Add a test in [tests/operational_commands.rs](../../../tests/operational_commands.rs) that clears a symlinked store path and asserts the link is gone, the store survives, standard error warns, and the machine answer discloses the link.
- [x] Document the behavior in the `clear` section of [docs/commands/cache.md](../../../docs/commands/cache.md).

## Gate

- [x] Run the full Rust test suite and the MCP test suite, and report both results.
- [x] Run every `.pre-commit-config.yaml` hook over everything changed since `HEAD`, staged or not, including new files, and report the result.

## Overridden verify finding

The suite carries one failing test that this change did not cause and cannot fix here.
See `design.md` § Verification Overrides for the record and its constraints.

- [ ] Run `cargo test --release --test semantic_engine python_live_tool_matches_committed_fixture_shape` where `python3 -m venv` can create an environment, and record the result.
  The repository's Rust workflow runs on `ubuntu-latest`, so the pushed branch is the nearest place this executes.
