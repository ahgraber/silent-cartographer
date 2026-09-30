# Design: ty-scip Python Backend

## Context

- c10r reads four things from a SCIP index: document path, position encoding, occurrence symbol and range, and the Definition role bit (`src/semantic/scip.rs`).
  Both Python indexers emit SCIP, so the shared translation serves both.

- `build` hard-codes the executable name `scip-python` (`src/main.rs:215`).
  `status` checks Python currency against the live scip-python version (`src/commands.rs:297`).
  `backend_language` maps a recorded analyzer name to a language and refuses unknown names (`src/graph/mod.rs:130`).
  `doctor` probes rust-analyzer and scip-python and treats both as required (`src/commands.rs:1600`).

- The join's module-name rule reads a module's dotted name only from scip-python's shape: a dotted namespace segment followed by an `__init__` meta terminal (`module_dotted_name`, `src/graph/join.rs:1580`). ty-scip emits a module as a sequence of namespace segments (`flask/app/`).
  On Flask, the rule accepted 919 occurrences under scip-python and 0 under ty-scip.

- The store already persists analyzer name and version as provenance, and currency already compares them (`A build over unchanged inputs does no work`, `Staleness reflects underlying change`).

- Measurements, 2026-09-29, ty-scip 0.1.0 against scip-python 0.6.6:

  | Target              | Result                                                                                      |
  | ------------------- | ------------------------------------------------------------------------------------------- |
  | Flask, indexer only | ty-scip 0.48 s / 120 MB; scip-python 2.8 s / 517 MB                                         |
  | Flask, c10r join    | ty-scip 15181 aligned, 303 duplicate-ambiguous, 158 text-mismatch; scip-python 13452, 4, 57 |
  | sympy 1.14.0        | ty-scip 27 s / 2.4 GB after three excludes; scip-python out of memory                       |
  | sphinx              | ty-scip refused; indexed 429 files in 10 s with `exclude-scripts = true`                    |

- ty-scip has one author, started 2026-09-09, and states that its symbol scheme may change.

- Three other proposed changes relate to this one:

  - `stale-files-and-line-positions` (committed 8cad9d7) raises `SURFACE_VERSION` from 11 to 12 and `SCHEMA_VERSION` from 16 to 17, and rewrites the `status` freshness report in `src/commands.rs` to list every staleness reason.
    It edits the same `status` code as this change.
  - `c10r-settings` (placeholder) proposes project-level settings, with the analyzer among the candidates.
    A durable per-project Python indexer choice belongs there, not here.
  - `retained-utility` (placeholder) proposes structural navigation when no semantic indexer can run.
    This change keeps today's behavior in that case: a Python build with no available indexer refuses.

## Decisions

### Decision: OneAdapterTwoIndexerProfiles

**Chosen:** parameterize the existing `PythonAdapter` by a `PythonIndexer` enum (`TyScip`, `ScipPython`).
The enum supplies:

- the executable name;
- the analyzer name recorded in provenance;
- the install hint;
- the rule that maps a failed run to an error.

Everything else is shared: environment resolution, environment facts, the invocation `index --project-name <workspace> --project-version 0 --output <file>`, the SCIP translation, and module-kind classification.

**Rationale:** ty-scip accepts scip-python's invocation (measured: exit 0 through the unchanged adapter via a shim).
The two indexers differ only in the four enum-supplied values, so a second adapter type would be a near-copy of the first.

**Alternatives considered:**

- A separate `TyScipAdapter` type: duplicates environment and invocation handling, which the project rules forbid.

### Decision: RejectedFileIsATypedRefusal

**Chosen:** for ty-scip, capture standard error, then replay it to c10r's standard error after the run.
A nonzero exit whose output holds `project file has no importable module name: <path>` becomes a new `SemanticError::RejectedFile { path, remedy }`.
The remedy depends on the named file:

- If its leading comment lines hold a PEP 723 block (`# /// script`), the remedy is `exclude-scripts = true` under `[src]` in `ty.toml`, or under `[tool.ty.src]` in `pyproject.toml`.
- Otherwise the remedy is an `exclude` entry for the path.

The error maps to the exit code that an analysis failure already uses.
Any other nonzero exit stays `SemanticError::Analysis`.

**Rationale:** the two known causes need different fixes. ty treats a file with inline script metadata as a standalone script with its own environment, so it has no module in the project (`ty_project/src/walk.rs`, `script.rs`; reproduced 2026-09-29).
That caused the sphinx refusal on `utils/convert_attestations.py`.
A path component that is not an identifier (`test-examples`, `.ci`) has no module name either.
That caused the sympy refusal.
A distinct variant lets tests assert the refusal without matching message text.

**Alternatives considered:**

- Pass `exclude-scripts` or generated excludes automatically: silently drops files from the index, which the proposal rules out.
- One generic remedy text: sends sphinx-style users to the wrong fix.

### Decision: SelectionIsOneFunctionOverProbes

**Chosen:** `select_python_indexer(explicit: Option<PythonIndexer>, probe) -> Result<PythonIndexer, SelectionError>`.
It is a pure function over the outcomes of the existing bounded version probes (`run_bounded`, `PROBE_DEADLINE`), and `build`, `status`, and `doctor` all call it.
It resolves in this order:

1. With an explicit selection, probe only that indexer.
   Present selects it.
   Absent is `Unavailable`, and unresponsive is `Timeout`.
2. Without one, probe ty-scip.
   Present selects ty-scip.
   Unresponsive is `Timeout` naming ty-scip.
   Absent moves to step 3.
3. Probe scip-python.
   Present selects scip-python.
   Unresponsive is `Timeout`.
   Absent is `Unavailable`, naming both install hints.

**Rationale:** one function keeps the three commands from disagreeing about which indexer is in effect.
Probing ty-scip first skips a Node start when ty-scip is installed.
Refusing on an unresponsive preferred indexer follows the rule that a build never falls back silently.
Environment resolution follows the same rule.

**Alternatives considered:**

- Fall back to scip-python when ty-scip hangs: the index would depend on a transient condition, and the user would not know which indexer ran.

### Decision: StalenessIdentityIsTheSelectedIndexer

**Chosen:** a Python store is current and fresh only while `select_python_indexer` returns the recorded analyzer name and the probed version equals the recorded version.
`build` passes its own explicit selection.
`status` passes none, so it judges against autodetection.
When selection fails because no indexer is present, or a probe times out, `status` keeps its existing behavior.
It discloses a timeout, then falls back to the recorded provenance.
No store schema change is needed, because analyzer name and version are already persisted.
The store already reports a different analyzer name as `stale_version` (`src/graph/store.rs:1862`), so an indexer switch uses that existing value, and appears in `stale_reasons` once `stale-files-and-line-positions` lands.
The human label for that value changes from "stale (analyzer version changed)" to "stale (analyzer or analyzer version changed)", because the old label misstates a switch.
The JSON value does not change.

**Rationale:** the user asked for the backend to be part of the staleness identity.
The rule states whether a plain build now would produce a different index.
That is the question staleness already answers for analyzer version and environment.

**Alternatives considered:**

- Judge only against the recorded indexer: a store from an uninstalled or superseded indexer would read fresh although a plain build would replace it.
- Record whether the selection was explicit, and keep an explicit choice for later builds: needs a store schema version bump and migration for a convenience the user did not ask for.
  A durable choice is the subject of `c10r-settings`.
  The chosen rule instead reports such a store stale, which is visible and correct.

### Decision: ModuleNameReadsBothShapes

**Chosen:** `module_dotted_name` returns `Option<Cow<str>>`.

- For scip-python's shape, it returns the namespace segment as today.
- For a descriptor whose segments are all module segments, it returns the segment names joined with `.`.

`classify_module_kinds` is unchanged: ty-scip's terminal namespace segment already classifies as `SymbolKind::Module` through `symbol_kind_from`.
Both callers (`src/graph/join.rs:1387`, `:1463`) are Python-only rule arms, so Rust module descriptors are not affected.

**Rationale:** the baseline rule is written in terms of "the module's dotted namespace name" and names no descriptor shape.
This is an implementation fix under the existing requirement, and needs no spec delta.

**Alternatives considered:**

- Rewrite ty-scip symbols into scip-python's `__init__:` shape during translation: hides the real descriptor and couples identity to one indexer's quirk.

### Decision: BuildOptionAndLanguageMapping

**Chosen:**

- `build --python-indexer <ty-scip|scip-python>` is an `Option` with no default, and omitting it means autodetection.
- The option on a build whose selected language is Rust is `Failure::Usage`.
- Both executables are found on `PATH` by fixed name, as scip-python is today.
- `backend_language` maps `ty-scip` to `Language::Python`.

**Rationale:** an `Option` distinguishes "not given" from an explicit value, which the Rust usage refusal needs.

**Alternatives considered:**

- An `auto` value: omitting the option already means auto.
- Executable-path options: not requested; `PATH` already controls the choice.

### Decision: DoctorReportsSelection

**Chosen:**

- `run_doctor` probes rust-analyzer, ty-scip, and scip-python.
- It computes the Python selection with `select_python_indexer(None, …)`.
- The report adds the selected Python indexer, or none.
- The exit code signals the indexer/setup failure when rust-analyzer is not ready or the Python selection fails.
- The ty-scip install hint is `uv tool install ty-scip`.

**Rationale:** doctor answers "can a build run here", which is now a per-language question.

### Decision: SurfaceVersionAndMcpPinMoveTogether

**Chosen:**

- Bump `SURFACE_VERSION` in `src/manifest.rs` and the MCP pin in `mcp/src/c10r_mcp/surface.py` by one from the value on `main` when the branch is cut: 11 to 12 today, or 12 to 13 if `stale-files-and-line-positions` merges first.
- Regenerate `tests/fixtures/surface_manifest.snapshot.json`.
- The MCP `build` tool gains no parameter; MCP builds use autodetection.

**Rationale:** the new option changes the flag structure, and the snapshot test and MCP startup check require the two constants to move together.

### Decision: GateBlocksMerge

**Chosen:** the change merges only after the comparison gate passes.
Each repository is built twice, once per indexer, with an explicit `--python-indexer` and a separate `--db`.
Thresholds for each of Flask, httpx2, and Faker:

- at least 95% of scip-python's accepted in-workspace cross-file edges are retained, and every lost edge is classified;
- zero wrong targets in a reviewed sample of 100 accepted edges, and in every residual that changes an in-workspace `dependents` or `impact` answer;
- join acceptance is at least 97%;
- two ty-scip runs give byte-identical indexes.

Further targets:

- Flask indexes in under 1 s and 250 MB;
- sympy builds and answers queries in under 60 s and 4 GB, with a documented exclude list;
- sphinx builds and answers queries with `exclude-scripts = true` as its only configuration change.

The share of identical in-workspace answers is reported for each repository, with no threshold.
Configuration changes are made in copies, never in the persistent clones under `~/.cache/silent-cartographer`.
Results go to `gate.md` in this change directory; comparison scripts go to `._scratch/research/scip-python/gate/`.
If the gate fails, the user decides between flipping the preference order to scip-python first and stopping the change.

**Rationale:** autodetection makes ty-scip the effective default wherever it is installed, so the evidence has to come before merge.

## Architecture

```text
build --python-indexer? ─┐
status (no option) ──────┼─▶ select_python_indexer(explicit, probes) ──▶ PythonIndexer
doctor (no option) ──────┘            │                                    │
                                      │ probes: ty-scip, then scip-python   ▼
                                      ▼                          PythonAdapter{indexer}
                          SelectionError → typed setup error      │  resolve env, facts
                                                                  │  run <exe> index …
                                                                  │  ty-scip: map rejected file
                                                                  ▼
                                                   SCIP → translate_index → classify_module_kinds
                                                                  ▼
                                               join (module_dotted_name reads both shapes)
                                                                  ▼
                                    store: provenance {analyzer name, version, environment}
                                                                  ▲
                    currency / staleness: recorded analyzer == selection(explicit | none) now
```

## Risks

- **Machine-dependent indexer.**
  Two hosts can build different stores from the same code.
  Mitigation: provenance names the indexer, and staleness reports a mismatch.
  Harnesses that need a fixed indexer pass `--python-indexer`.
  A task checks whether the eval harness builds Python stores without the option.
- **ty-scip scheme or output changes between versions.**
  Mitigation: the version is part of the staleness identity, the install hint pins a version, and the gate is re-run on each bump.
- **Reassigned names become duplicate-ambiguous.** ty-scip emits a definition per reassignment (303 refused references on Flask, against 4).
  Mitigation: measured in the gate, and reported upstream, not patched in the join.
- **Third-party references are lost under ty-scip.**
  Mitigation: stated in the README; scip-python stays selectable.
- **All-or-nothing refusal on rejected files.**
  Mitigation: a typed error with a cause-specific remedy, and an upstream request for skip-with-warning.
- **Single upstream maintainer.**
  Mitigation: forking ty-scip is the named contingency, and scip-python remains a fallback through the option.
