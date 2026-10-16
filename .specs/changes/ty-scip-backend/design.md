# Design: ty-scip Python Backend

## Context

- c10r reads four things from a SCIP index: document path, position encoding, occurrence symbol and range, and the Definition role bit (`src/semantic/scip.rs`).
  Both Python indexers emit SCIP, so the shared translation serves both.

- `build` hard-codes the executable name `scip-python` (`src/main.rs:335`).
  `status` checks Python currency against the live scip-python version (`src/commands.rs:342`).
  `backend_language` maps a recorded analyzer name to a language and refuses unknown names (`src/graph/mod.rs:198`).
  `doctor` probes rust-analyzer and scip-python and treats both as required (`src/commands.rs:2473`).

- The join's module-name rule reads a module's dotted name from a descriptor made only of module segments (`module_dotted_name`, `src/graph/join.rs`).
  `canonical-identity` gives both indexers that module form: ty-scip emits it directly, and translation rewrites scip-python's namespace-plus-`__init__` form into it.

- The store already persists analyzer name and version as provenance, and currency already compares them (`A build over unchanged inputs does no work`, `Staleness reflects underlying change`).

- Measurements, 2026-09-29, ty-scip 0.1.0 against scip-python 0.6.6:

  | Target              | Result                                                                                      |
  | ------------------- | ------------------------------------------------------------------------------------------- |
  | Flask, indexer only | ty-scip 0.48 s / 120 MB; scip-python 2.8 s / 517 MB                                         |
  | Flask, c10r join    | ty-scip 15181 aligned, 303 duplicate-ambiguous, 158 text-mismatch; scip-python 13452, 4, 57 |
  | sympy 1.14.0        | ty-scip 27 s / 2.4 GB after three excludes; scip-python out of memory                       |
  | sphinx              | ty-scip refused; indexed 429 files in 10 s with `exclude-scripts = true`                    |

- ty-scip has one author, started 2026-09-09, and states that its symbol scheme may change.

- `status` lists every staleness reason (`stale_reasons`), and an analyzer change appears there as `stale_version`.

- Three other proposed changes relate to this one:

  - `c10r-settings` (placeholder) proposes project-level settings, with the analyzer among the candidates.
    This change keeps an indexer per store; a project-wide choice that applies before any store exists belongs there.
  - `ty-scip-default` (placeholder) decides from this change's gate results whether ty-scip becomes the default.
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

**Chosen:** for ty-scip, pass each line of standard error through to c10r's standard error as it arrives, and keep a copy for classifying a failed run.
Printing the output only after the run would leave a long run (sympy indexes for about 45 s) silent, and a run interrupted with Ctrl-C would lose its output.
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

**Chosen:** `select_python_indexer(explicit: Option<PythonIndexer>, recorded: Option<PythonIndexer>, probe) -> Result<(PythonIndexer, String), SemanticError>`, which returns the selected indexer and its probed version.
It reuses the `Unavailable` and `Timeout` variants, which already map to the setup exit code.
It is a pure function over the outcomes of the existing bounded version probes (`run_bounded`, `PROBE_DEADLINE`), and `build` and `status` both call it.
`recorded` is the analyzer name in the existing store's provenance, or none when no store exists.
The function picks one candidate: the explicit selection, else the recorded indexer, else scip-python.
It then probes only that candidate.
Present selects it, absent is `Unavailable` with the candidate's install hint, and unresponsive is `Timeout` naming the candidate.

**Rationale:** one function keeps the commands from disagreeing about which indexer is in effect.
Probing one candidate means a build never uses the other indexer in place of an unavailable one.
The index would otherwise depend on a transient condition, and the user would not know which indexer ran.
Environment resolution follows the same rule.

**Alternatives considered:**

- Fall back to the other indexer when the candidate is absent or hangs: rejected for the reason above.

### Decision: StoreKeepsItsIndexer

**Chosen:** a build without the option selects the indexer recorded in the existing store, through the `recorded` argument of `select_python_indexer`.
A Python store is current only while the selection equals the recorded analyzer name and the probed version equals the recorded version.
`build` passes its explicit selection and the recorded indexer.
`status` passes no explicit selection, so it judges the store against its recorded indexer's live version.
When the recorded indexer is absent, or its probe times out, `status` keeps its existing behavior.
It discloses a timeout, then falls back to the recorded provenance.
An explicit selection that differs from the recorded indexer makes the build's currency check fail, so the build rebuilds the store with the new indexer.
No store schema change is needed, because analyzer name and version are already persisted.

**Rationale:** refresh hooks and the MCP `build` tool always run a build without the option.
If a plain build always meant scip-python, those builds would replace a ty-scip store on every run, and `status` would report every ty-scip store stale.
Keeping the recorded indexer makes an explicit choice last until another explicit choice replaces it.

**Alternatives considered:**

- A plain build always selects scip-python: ty-scip would be usable only in a separate store that no hook or MCP build touches.
- Autodetection with ty-scip preferred: makes ty-scip the effective default wherever it is installed, before the gate results exist.
  That decision belongs to `ty-scip-default`.

### Decision: ModuleNameReadsTheCanonicalForm

**Chosen:** `module_dotted_name` reads only the canonical module form: a descriptor whose segments are all module segments, joined with `.`.
This change adds no module-name code of its own.

**Rationale:** `canonical-identity` rewrites scip-python's module descriptors into the canonical form during translation, and ty-scip already emits that form.
One input form needs one reading in the join.
The baseline rule is written in terms of "the module's dotted namespace name" and names no descriptor shape, so no spec delta is needed.

**Alternatives considered:**

- Read each indexer's raw module form in the join: the implementation before `canonical-identity`, which ties the join to each indexer's descriptor form.

### Decision: TyScipIsHeldToTheReferenceListWithinTheRequirement

**Chosen:** ty-scip's index of the conformance project is held to the hand-written reference list, except for the two declarations that `Identity does not depend on the indexer` does not cover:

- `Widget.size`, a property with a getter and a setter: scip-python reports one definition, ty-scip reports two, so the ty-scip store holds two distinct identities.
- `Widget._size`, an instance attribute assigned only through `self` in the setter: scip-python reports it, and ty-scip reports no symbol for it.

**Rationale:** the requirement excludes a declaration that only one indexer reports and a name with different numbers of definitions.
An exact-match test would fail on behavior the requirement permits.
The twice-defined property is the conformance project's instance of the repeated-definition boundary, so it needs no fixture change, and the committed indexes of both indexers stay as they are.

### Decision: BuildOptionAndLanguageMapping

**Chosen:**

- `build --python-indexer <ty-scip|scip-python>` is an `Option` with no default, and omitting it selects the recorded indexer, or scip-python for a new store.
- The option on a build whose selected language is Rust is `Failure::Usage`.
- Both executables are found on `PATH` by fixed name, as scip-python is today.
- `backend_language` maps `ty-scip` to `Language::Python`.

**Rationale:** an `Option` distinguishes "not given" from an explicit value, which the Rust usage refusal and the recorded-indexer rule need.

**Alternatives considered:**

- Executable-path options: not requested; `PATH` already controls the choice.

### Decision: DoctorReportsAnOptionalIndexer

**Chosen:**

- `run_doctor` probes rust-analyzer, scip-python, and ty-scip.
- rust-analyzer and scip-python stay required; ty-scip is reported as optional.
- The exit code signals the indexer/setup failure only for a required indexer that is absent or unresponsive.
- The ty-scip install hint is `uv tool install ty-scip`.

**Rationale:** doctor answers whether a build with default settings can run here, and that does not depend on ty-scip.
The ty-scip entry shows an operator whether opting in would work.

### Decision: SurfaceVersionAndMcpPinMoveTogether

**Chosen:**

- Bump `SURFACE_VERSION` in `src/manifest.rs` and the MCP pin in `mcp/src/c10r_mcp/surface.py` by one from the value on `main` when the branch is cut: 13 to 14 today.
- Regenerate `tests/fixtures/surface_manifest.snapshot.json`.
- The MCP `build` tool gains no parameter; MCP builds use the recorded indexer, or scip-python for a new store.

**Rationale:** the new option changes the flag structure, and the snapshot test and MCP startup check require the two constants to move together.

### Decision: GateInformsTheDefault

**Chosen:** the comparison gate runs before the change merges, and its results are recorded.
A failed gate does not block the merge, and `ty-scip-default` requires a passed gate.
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
If the gate fails, ty-scip stays opt-in, and the user decides whether to merge the change or stop it.

**Rationale:** only a build that selects ty-scip explicitly uses it, so the evidence has to come before the default changes, not before merge.
The thresholds are fixed now so that the default decision cannot move them after the results are known.

## Architecture

```text
build --python-indexer? ─┐   store provenance (recorded indexer)
status (no option) ──────┤        │
                         ▼        ▼
     select_python_indexer(explicit, recorded, probe) ──────────▶ PythonIndexer
                         │                                          │
  candidate: explicit, else recorded, else scip-python; probe it    ▼
                         ▼                               PythonAdapter{indexer}
  Unavailable / Timeout → typed setup error               │  resolve env, facts
                                                                  │  run <exe> index …
                                                                  │  ty-scip: map rejected file
                                                                  ▼
                                                   SCIP → translate_index → classify_module_kinds
                                                                  ▼
                                               join (module_dotted_name reads the canonical module form)
                                                                  ▼
                                    store: provenance {analyzer name, version, environment}
                                                                  ▲
        currency / staleness: recorded analyzer == selection, recorded version == probed version
```

## Risks

- **Store-dependent indexer.**
  Two stores of the same code can hold indexes from different indexers.
  Mitigation: provenance names the indexer, and a harness that needs a fixed indexer passes `--python-indexer`.
- **ty-scip scheme or output changes between versions.**
  The install hint does not pin a version, so a new install can get a version the gate never measured; the gate measured ty-scip 0.1.0, and the README names that version.
  Mitigation: the version is part of the staleness identity, so a version change makes an existing store stale, and the gate is re-run before any decision that depends on ty-scip's results.
- **Reassigned names become duplicate-ambiguous.** ty-scip emits a definition per reassignment (303 refused references on Flask, against 4).
  Mitigation: measured in the gate, and reported upstream, not patched in the join.
- **Third-party references are lost under ty-scip.**
  Mitigation: stated in the README; scip-python stays the default.
- **All-or-nothing refusal on rejected files.**
  Mitigation: a typed error with a cause-specific remedy, and an upstream request for skip-with-warning.
- **Single upstream maintainer.**
  Mitigation: forking ty-scip is the named contingency, and scip-python remains the default.

## Spec Changes

- **Change:** symbol-identity: add scenario `ty-scip yields the Python reference identities` to `Identity does not depend on the indexer` **Source:** upstream: proposal § Approach, which makes this change carry the ty-scip evidence for that requirement **Rationale:** the requirement covers every conforming indexer, and its scenarios held only scip-python and rust-analyzer to their reference lists.
  **Approved by user:** 2026-10-16
- **Change:** symbol-identity: add scenario `A name defined twice has one identity per definition under ty-scip` to `Identity does not depend on the indexer` **Source:** upstream: the requirement's own exclusion of a name for which the indexers report different numbers of definitions **Rationale:** the exclusion had no scenario; the conformance project's property with a getter and a setter is a name that scip-python reports once and ty-scip twice.
  **Approved by user:** 2026-10-16
