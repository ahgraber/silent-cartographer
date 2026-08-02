# Proposal: is-tested

## Intent

When an agent or developer is about to change a symbol, one of the first questions is what code tests that symbol — but c10r's reverse-reference graph today cannot separate a test call site from any other caller.
The edge substrate needed to answer this already exists: the tests that exercise symbol X are the reverse reference sites of X whose source is itself test code.
The only net-new graph content is a single per-symbol classification — `is_test` — stamping a symbol as test code by language convention, plus a `tests` trace relation that projects the existing reverse-reference traversal filtered to test-classified sources.
The classification is a heuristic, convention-based judgment, not a resolved semantic fact, so the central design tension is calibration honesty: the `tests` relation must be labeled heuristic-grade by structure and never presented with the confidence of the resolved edges it filters.

## User Stories

### Story: tests-for-symbol

As an agent or developer about to change a symbol, I want to ask what test code exercises that symbol, so that I know where its guard rail lives and where to look before I change it.
Ladders to north-star outcome #2 (Blast radius before change): the tests exercising a symbol are part of its dependent set, surfaced so a change does not silently break its own guard rail.

### Story: locate-a-symbols-tests

As a developer orienting in an unfamiliar codebase, I want to find the test code that references a given symbol, so that I can read the tests to understand the symbol's intended behavior and contract.
Ladders to north-star outcome #3 (Structural understanding): test call sites are a structural signal about what a symbol is for.

### Story: honest-heuristic-label

As any consumer of the `tests` relation, I want every answer structurally labeled as convention-based classification rather than resolved semantic fact, so that I calibrate how far to trust it and am never misled by a confident wrong classification.
Ladders to north-star outcome #5 (Calibrated trust) and the "Calibration over coverage" principle: an honestly-labeled heuristic keeps a consumer on the tool; a heuristic dressed as a semantic edge sends them back to grep.

## Scope

**In scope:**

- code-graph: a persisted per-symbol test classification (`is_test`), stamped during the build's syntax pass from language-convention signals, wholly superseded by each build, and carrying the convention rule that stamped it as provenance — mirroring how aligned attributions carry their accepting alignment rule.
- code-graph: convention rules per supported language, defined at the contract floor (every backend classifies from statically observable convention signals; the specific signals are per-language scenario detail):
  - Rust: test attributes (`#[test]` and attribute forms containing it, e.g. `#[tokio::test]`), enclosure in a `#[cfg(test)]` module, and files under a directory named `tests` (the integration-test layout, read statically).
  - Python (pytest and unittest conventions): `test_*.py` / `*_test.py` files, the reserved file names `tests.py` and `conftest.py`, and files under `tests/` directories.
- code-graph: schema version bump 11 → 12, riding the existing replace-or-refuse store contract (build replaces an old store; query refuses with recovery guidance).
- code-navigation: a `tests` trace relation — the reverse reference sites of the subject whose enclosing declaration is classified test code — as a new named relation on the existing `trace` surface.
- code-navigation: heuristic-grade labeling of every `tests` answer as a structural field of the output contract, distinct from the resolved relations it filters, composing with the existing provenance/staleness labeling.

**Out of scope:**

- Any change to the edge substrate — no new edge kind; `tests` is a filtered projection of the existing reverse-reference traversal.
- Running, executing, or measuring tests, and any notion of coverage as executed lines (c10r is not a test runner — north-star non-goal); `is_test` is a static, structural classification only.
- Test-runner enumeration (asking the runner what it would collect) — deliberately deferred, not rejected: the per-classification rule provenance is specified open so a stronger `runner-enumerated` rule can land later as an optional per-backend capability without schema or contract redesign.
- Semantic or assertion-level test analysis (which assertions target which symbol); the classification is convention-based, not a proof of what a test verifies.
- Rust doctests: they are not persisted symbols in the graph, so no classification can attach to them.
- A standalone declaration-name rule (classifying a `test_`-prefixed function in a non-test file): the test runner's own collection would not reach such a function, so classifying it would be a confident false positive — calibration over coverage.
- Any config file or configurable classification rules; the conventions are fixed defaults in this change.
- A deprecation schedule: no first MINOR release exists (v0.1.0, untagged), so the post-first-MINOR governance clause does not apply; the schema bump carries a migration note only.

## Approach

Classification is a build-time, syntax-pass judgment: the syntax oracle already visits every declaration, so the same pass stamps `is_test` from convention signals — attributes, enclosing test-configured modules, file path and name, declaration-name convention — without a second extraction.
The classification is persisted per symbol behind the schema bump and wholly superseded by each build, matching the builds-wholly-supersede contract all derived state obeys.
The `tests` relation reuses the reverse-reference traversal `trace` already exposes, filtered at the enclosing-declaration grain: a reference site counts when the declaration it is attributed to — or the document's module, when the site is attributed to the module itself — is classified test code.
This grain deliberately includes shared test helpers — a helper under a `tests/` tree that calls the subject is test code exercising it — so the relation never returns a false "nothing tests this" when coverage flows through helpers.
Calibration honesty is carried at two layers: each classification carries the convention rule that stamped it (per-result calibration and debuggability), and every `tests` answer carries a relation-level heuristic-grade marker (unmissable even without per-result inspection) — by structure, not caveat.
The human render surfaces the same marker so neither consumer class can miss it.
