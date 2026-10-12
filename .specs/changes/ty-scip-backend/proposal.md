# Proposal: ty-scip Python Backend

## Intent

c10r indexes Python only through scip-python. scip-python cannot index some large repositories: on sympy it runs out of memory at every heap size available, and the `NODE_OPTIONS` workaround does not reach it through c10r (measured 2026-09-14).
Its upstream has had no human commits since its 0.6.6 release on 2025-09-05. ty-scip is a SCIP indexer for Python built on ty.
On 2026-09-29 it indexed Flask in 0.48 s and 120 MB, against scip-python's 2.8 s and 517 MB.
It indexed sympy in 27 s and 2.4 GB after three workspace excludes.
This change adds ty-scip as an opt-in Python backend, and scip-python stays the default.
A comparison gate measures ty-scip against scip-python before the change merges.
Whether ty-scip becomes the default is decided after the gate, in the change `ty-scip-default`.

## User Stories

### Story: index-large-python

As an agent working in a large Python repository, I want c10r to build an index where scip-python runs out of memory, so that I get precise locate and blast radius instead of falling back to grep.

### Story: choose-python-indexer

As an operator of c10r, I want to select ty-scip explicitly while scip-python stays the default, and I want later builds to keep the indexer a store was built with, so that I can use and compare ty-scip without changing the indexer of any other build.

### Story: honest-python-refusal

As a user whose repository holds a file the indexer cannot map to a module, I want the build to refuse and name that file and the remedy, so that I never receive a partial index.

## Scope

**In scope:**

- semantic-engine: a ty-scip adapter that passes the existing backend conformance suite; a typed refusal when ty-scip rejects a workspace file that has no importable module name.
- code-graph: Python indexer selection.
  An explicit `--python-indexer` option wins.
  Without it, a build uses the indexer recorded in the existing store, and scip-python when no store exists.
  A build never uses the other indexer in place of an unavailable one.
  The selected indexer is recorded in provenance and is part of the staleness identity: a store is current only while the indexer the build selects is the recorded one at the recorded version.
  The module-name alignment rule accepts module descriptors made only of namespace segments, which is ty-scip's module shape.
- command-surface: the readiness report covers ty-scip as an optional indexer, whose absence does not signal a setup failure.
- The `SURFACE_VERSION` bump, the MCP server pin, and the surface snapshot that the new option requires.
  The MCP `build` tool does not expose the option; MCP builds use the store's recorded indexer, or scip-python for a new store.
- A comparison gate on Flask, httpx2, Faker, sympy, and sphinx with pass thresholds fixed before the run.
  The gate runs before the change merges, and its results are recorded in this change directory.
  A failed gate does not block the merge, because ty-scip is opt-in.
  `ty-scip-default` requires a passed gate.

**Out of scope:**

- Exposing the indexer selection through the MCP `build` tool.
- Making ty-scip the default.
  That decision is the change `ty-scip-default`.
- Removing scip-python.
- Forking ty-scip or writing an in-tree indexer on ty.
  These are contingencies if an upstream defect blocks the gate.
- Third-party package targets. ty-scip omits them by design.
- Join rules for repeated-assignment definitions. ty-scip emits one definition per reassignment, which c10r splits into twin symbols and refuses their references.
  This belongs upstream.
- Automatic exclusion of files ty-scip rejects.

## Approach

The debate record is in `._scratch/research/scip-python/debate/`: round-1 and round-2 positions from two independent reviewers, both supporting an opt-in ty-scip backend with conditions.
The user chose an opt-in ty-scip with scip-python as the default, and a store that keeps the indexer it was built with.
Measurements are in `._scratch/research/scip-python/` (`raw/tyscip-*.md`, `probe-tyscip/`, `probe-unresolved/`).
The mechanism, the refusal causes, and the gate thresholds are in `design.md`.

This change depends on `canonical-identity`, which defines the identity form every Python indexer must produce.
It is paused until that change lands, and is then rebased onto it.
After the rebase, ty-scip's output needs no descriptor conversion, and this change carries the ty-scip evidence for `canonical-identity`'s requirement `Identity does not depend on the indexer`.
