# Proposal: ty-scip Python Backend

## Intent

c10r indexes Python only through scip-python. scip-python cannot index some large repositories: on sympy it runs out of memory at every heap size available, and the `NODE_OPTIONS` workaround does not reach it through c10r (measured 2026-09-14).
Its upstream has had no human commits since its 0.6.6 release on 2025-09-05. ty-scip is a SCIP indexer for Python built on ty.
On 2026-09-29 it indexed Flask in 0.48 s and 120 MB, against scip-python's 2.8 s and 517 MB.
It indexed sympy in 27 s and 2.4 GB after three workspace excludes.
This change adds ty-scip as a Python backend and selects it automatically when it is installed.
A comparison gate against scip-python must pass before the change merges.

## User Stories

### Story: index-large-python

As an agent working in a large Python repository, I want c10r to build an index where scip-python runs out of memory, so that I get precise locate and blast radius instead of falling back to grep.

### Story: choose-python-indexer

As an operator of c10r, I want the Python indexer chosen from what is installed, with an explicit override, so that a build works with either indexer and I can compare their answers.

### Story: honest-python-refusal

As a user whose repository holds a file the indexer cannot map to a module, I want the build to refuse and name that file and the remedy, so that I never receive a partial index.

## Scope

**In scope:**

- semantic-engine: a ty-scip adapter that passes the existing backend conformance suite; a typed refusal when ty-scip rejects a workspace file that has no importable module name.
- code-graph: Python indexer selection.
  An explicit `--python-indexer` option wins.
  Without it, ty-scip is used when installed, otherwise scip-python.
  The selected indexer is recorded in provenance and is part of the staleness identity: a store is current only while the indexer a build would select now is the recorded one.
  The module-name alignment rule accepts module descriptors made only of namespace segments, which is ty-scip's module shape.
- command-surface: the readiness report covers ty-scip, treats Python as ready when either indexer is available, and names the indexer a build would select.
- The `SURFACE_VERSION` bump, the MCP server pin, and the surface snapshot that the new option requires.
  The MCP `build` tool does not expose the option; MCP builds follow autodetection.
- A comparison gate on Flask, httpx2, Faker, sympy, and sphinx with pass thresholds fixed before the run.
  The change does not merge until the gate passes.
  Results are recorded in this change directory.

**Out of scope:**

- Exposing the indexer selection through the MCP `build` tool.
- Removing scip-python.
- Forking ty-scip or writing an in-tree indexer on ty.
  These are contingencies if an upstream defect blocks the gate.
- Third-party package targets. ty-scip omits them by design.
- Join rules for repeated-assignment definitions. ty-scip emits one definition per reassignment, which c10r splits into twin symbols and refuses their references.
  This belongs upstream.
- Automatic exclusion of files ty-scip rejects.

## Approach

The debate record is in `._scratch/research/scip-python/debate/`: round-1 and round-2 positions from two independent reviewers, both supporting an opt-in ty-scip backend with conditions.
The user then chose autodetection with ty-scip preferred, with the gate moved ahead of merge.
Measurements are in `._scratch/research/scip-python/` (`raw/tyscip-*.md`, `probe-tyscip/`) and `/tmp/claude-501/tyscip-round2/`.
The mechanism, the refusal causes, and the gate thresholds are in `design.md`.
