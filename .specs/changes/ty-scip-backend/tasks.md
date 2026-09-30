# Tasks: ty-scip Python Backend

## Setup

- [ ] Create the worktree `.worktrees/ty-scip-backend` on branch `ty-scip-backend` from `main`, and move the untracked change directory into it.
  If `stale-files-and-line-positions` is being implemented at the same time, agree the merge order with the user first, because both changes edit the `status` freshness code in `src/commands.rs` and both bump `SURFACE_VERSION`.
- [ ] Install `ty-scip` 0.1.0 with `uv tool install ty-scip==0.1.0` and record `ty-scip --version` output in `gate.md`.

## Indexer profiles and selection (semantic-engine)

- [ ] Add the `PythonIndexer` enum (`TyScip`, `ScipPython`) to [src/semantic/python_adapter.rs](../../../src/semantic/python_adapter.rs) with its executable name, analyzer name, and install hint.
- [ ] Change `PythonAdapter::new` and `PythonAdapter::discover_version` to take a `PythonIndexer`, and report the indexer's analyzer name as provenance.
- [ ] Unit test: each `PythonIndexer` variant yields its executable name, analyzer name, and install hint, and the adapter's provenance carries that analyzer name.
- [ ] Add `select_python_indexer(explicit, probe)` as a pure function over probe outcomes, following the resolution order in `design.md` § SelectionIsOneFunctionOverProbes.
- [ ] Unit test `select_python_indexer` for each partition: explicit present, explicit absent, explicit unresponsive; no selection with both present, only ty-scip, only scip-python, neither, and ty-scip unresponsive with scip-python present.
- [ ] Add `SemanticError::RejectedFile { path, remedy }` and map it to the exit code that `SemanticError::Analysis` already uses.
- [ ] Capture ty-scip standard error, replay it to standard error after the run, and map a `project file has no importable module name: <path>` failure to `RejectedFile` with the remedy chosen by the named file's PEP 723 header.
- [ ] Unit test the remedy choice: a file whose leading comments hold `# /// script` gets the `exclude-scripts` remedy; a file under a non-identifier directory gets the `exclude` remedy; any other nonzero exit stays `SemanticError::Analysis`.
- [ ] Integration test (`A workspace file the indexer rejects is a typed refusal`): a ty-scip build of a workspace holding a PEP 723 script file fails with `RejectedFile` naming the file and the `exclude-scripts` remedy, and no store is created.
- [ ] Integration test: a ty-scip build of a workspace holding a file under a `test-examples/` directory fails with `RejectedFile` naming the file and the `exclude` remedy, and no store is created.
- [ ] Integration test: a ty-scip build that is refused over a workspace with an existing store leaves the store byte-identical.
- [ ] Generate a ty-scip index of `tests/fixtures/python-conformance/` through the adapter, commit it next to the scip-python index with a `TY-SCIP-VERSION` file, and extend the conformance test to run the suite on each Python index (`Each Python adapter passes the conformance suite`).

## Graph

- [ ] Map the analyzer name `ty-scip` to `Language::Python` in `backend_language` ([src/graph/mod.rs](../../../src/graph/mod.rs)).
- [ ] Unit test: `backend_language` returns Python for `ty-scip` and `scip-python`, Rust for `rust-analyzer`, and refuses an unknown name.
- [ ] Change `module_dotted_name` in [src/graph/join.rs](../../../src/graph/join.rs) to return `Option<Cow<str>>` and read both descriptor shapes per `design.md` § ModuleNameReadsBothShapes.
- [ ] Unit test the module-name rule on the namespace-segment shape: a reference at the terminal component is accepted, a reference at the full dotted name is accepted, a relative-import token is accepted, and a token spelling only a leading component is refused.
- [ ] Unit test the self-name rule when the document's module has the namespace-segment shape: a `__name__` occurrence resolving to that module is accepted, and one resolving to another module is refused.

## Build, status, and doctor (code-graph, command-surface)

- [ ] Add `--python-indexer <ty-scip|scip-python>` to `BuildArgs` in [src/cli.rs](../../../src/cli.rs) as an `Option` with no default, and pass it from [src/main.rs](../../../src/main.rs) to `run_build` in place of the hard-coded `scip-python`.
- [ ] In `run_build`, refuse the option with `Failure::Usage` when the selected language is Rust, and otherwise construct the adapter from `select_python_indexer`.
- [ ] Extend the fake-executable helpers already used by `tests/command_surface.rs` so a test can place fake `ty-scip` and `scip-python` executables on a temporary `PATH`, each configurable to answer `--version`, hang, or be absent, and to write a fixed SCIP index.
- [ ] Integration tests for selection in `build` (`A Python build selects its indexer`): explicit `scip-python` with both present; no option with both present selects ty-scip; only scip-python; only ty-scip; neither refuses naming both install hints; unresponsive ty-scip refuses naming ty-scip; the option on a Rust workspace is a usage error.
  Each asserts the recorded provenance or that no store was persisted.
- [ ] Make the build-side currency check (`current_index_accounting`) compare the recorded analyzer against `select_python_indexer` with the build's explicit selection.
- [ ] Make the `status` freshness check in [src/commands.rs](../../../src/commands.rs) compare the recorded analyzer against `select_python_indexer(None, …)`, keeping the existing timeout disclosure and recorded-provenance fallback.
- [ ] Change the human label for `stale_version` in [src/render.rs](../../../src/render.rs) to "stale (analyzer or analyzer version changed)", leaving the JSON value unchanged, and update the tests that assert the old label.
- [ ] Integration test: a store built by scip-python reads stale in `status` once ty-scip becomes available, and a following build without the option rebuilds it with ty-scip provenance.
- [ ] Integration test: an unchanged store reads current on rebuild and fresh in `status`, once for a store built by ty-scip and once for a store built by scip-python.
- [ ] Integration test: a store built with `--python-indexer scip-python` while ty-scip is available reads stale in `status`.
- [ ] Update `run_doctor` to probe rust-analyzer, ty-scip, and scip-python, add the selected Python indexer to the report and its human render, and signal the setup failure only when rust-analyzer is not ready or the Python selection fails.
- [ ] Integration tests for `doctor` (`Indexer readiness report`): all three present names ty-scip and succeeds; rust-analyzer absent fails; a hanging sole indexer is unresponsive and fails; ty-scip absent with scip-python present names scip-python and succeeds; both Python indexers absent fails; ty-scip hanging with scip-python present fails.

## Surface and MCP

- [ ] Bump `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs) by one from the base branch's value.
- [ ] Regenerate [tests/fixtures/surface_manifest.snapshot.json](../../../tests/fixtures/surface_manifest.snapshot.json) with `UPDATE_SURFACE_SNAPSHOT=1 cargo test --test surface_manifest snapshot -- --nocapture`, and confirm the `build` entry carries `--python-indexer` with values `ty-scip` and `scip-python` and no default.
- [ ] Bump the pin in [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py) to the same value and run `mcp/tests/test_startup.py`.

## Documentation

- [ ] Update the README Python section: both install commands, autodetection order, `--python-indexer`, the indexer as part of staleness, the two rejected-file remedies, and the loss of third-party references under ty-scip.
- [ ] Update [docs/commands/build.md](../../../docs/commands/build.md) for `--python-indexer`, autodetection, and the rejected-file refusal.
- [ ] Update [docs/commands/status.md](../../../docs/commands/status.md) for the indexer as part of freshness.
- [ ] Update [docs/commands/doctor.md](../../../docs/commands/doctor.md) for the three indexers, the selected Python indexer, and the new failure rule.

## Eval harness check

- [ ] Find every place the eval harness runs `c10r build` on a Python workspace, record whether the treatment images could contain `ty-scip`, and report to the user.
  Change the harness only on the user's instruction.

## Comparison gate (blocks merge)

- [ ] Write the comparison scripts in `._scratch/research/scip-python/gate/`: build a repository once per indexer with an explicit `--python-indexer` and separate `--db`, then report edge retention, join acceptance, answer identity, and every residual that changes an in-workspace `dependents` or `impact` answer.
- [ ] Run the gate on Flask, httpx2, and Faker from copies of the clones in `~/.cache/silent-cartographer`, run ty-scip twice per repository, and compare the two indexes byte for byte.
- [ ] Review a sample of 100 accepted ty-scip edges per repository, and every answer-changing residual, for wrong targets.
- [ ] Run Flask for time and peak memory; run sympy with its documented exclude list; run sphinx with only `exclude-scripts = true` added.
- [ ] Record all results, thresholds, and pass or fail per target in `gate.md`, and present them to the user for the merge decision.

## Upstream reports (drafts only)

- [ ] Draft a ty-scip issue asking to skip files with no importable module name, with a warning, instead of refusing the index; include the sphinx and sympy cases.
  The user files it.
- [ ] Draft a ty-scip issue on one definition per reassignment of the same name, with the Flask counts.
  The user files it.

## Checks

- [ ] Run the full Rust test suite and the MCP test suite.
- [ ] Run the pre-commit hooks on every changed and new file, and fix any failure.
