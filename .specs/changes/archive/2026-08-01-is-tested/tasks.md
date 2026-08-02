# Tasks: is-tested

## Schema and storage (code-graph foundation)

- [x] Extend the store round-trip test to persist and retrieve a symbol's `test_rule` — both a rule name and `NULL` — and confirm it fails against the current schema.
- [x] Add the nullable `test_rule` TEXT column to the `symbols` table DDL and bump `SCHEMA_VERSION` from 11 to 12 with a migration note (rebuild required; replace-or-refuse handles old stores); the round-trip test goes green.
- [x] Verify the existing schema-mismatch tests pass against version 12 unchanged (build replaces a v11 store; query refuses naming found/expected versions and the recovery action).

## Build-time classification: Rust (code-graph)

- [x] Write failing classification tests for the test-attribute rule — a plain `#[test]` function and a composed attribute (`#[tokio::test]`-shaped path) both stamped with test-attribute provenance — then implement the rule to green.
- [x] Write failing tests for the test-configuration rule — a helper without a test attribute inside a `#[cfg(test)]` module, including one nested more than one level deep — then implement transitive classification to green.
- [x] Write a failing test for the out-of-line case — a `#[cfg(test)]` module declaration whose body lives in its own document classifies that document's symbols — then implement propagation through the persisted enclosure relation after all documents are parsed.
- [x] Write a failing test for the test-directory rule — an attribute-less helper in `tests/`, including a `tests/common/`-style subdirectory module — then implement the rule to green.

## Build-time classification: Python (code-graph)

- [x] Write failing tests for the test-file rule — a helper without a test-prefixed name inside a `test_*.py` document, a root-level `conftest.py`, and a `tests.py` all classified; a `testimony.py`-style near-miss not classified — then implement the rule to green.
- [x] Write a failing test for the test-directory rule — a fixture module under `tests/` whose file name matches no test-file pattern — then implement the rule to green.

## Classification cross-cutting (code-graph)

- [x] Write a failing precedence test — a `#[test]` function inside a `#[cfg(test)]` module records test-attribute, not test-configuration — then implement the fixed order attribute > configuration > file > directory.
- [x] Add the negative-classification tests: a production Rust function with no signal, and a `test_`-prefixed Python function in a document no rule accepts, both persist `test_rule = NULL`.
- [x] Add a provenance-coverage test over a mixed build (several rules firing in one workspace) asserting every classification carries its rule.
- [x] Add a supersession test: a symbol classified in one build, whose source moves out of test territory, is `NULL` after rebuild.
- [x] Add a graph-invariance test: a test-classified function referencing a production symbol yields the same attribution and `uses` edge as an identical non-test function.

## The `tests` relation (code-navigation)

- [x] Write the failing trace tests, then implement the `tests` relation — the reverse-reference query filtered on the attributed declaration's `test_rule` (`enclosing_id`, or the document's module when the site attributes to the module itself), preserving the `references` ordering — to green:
  - mixed callers: a Rust symbol referenced from a test-classified function and a production function returns exactly the test-classified site;
  - shared helper: a symbol referenced only from a test-classified helper returns the helper's site, not an empty answer;
  - module-scope site: a symbol named by an import statement at module scope in a test-classified document returns that site through the document-module fallback;
  - Python: a function referenced from a declaration in a test-classified document returns that site;
  - typed absence: a subject with only production references returns a definite empty set with the success exit code.
- [x] Add the determinism test: repeated identical `tests` queries return identical ordering, matching the order the same sites carry in a `references` answer.
- [x] Add the detail-projection test: a `tests` query at signature detail carries the attributed declaration's signature tier without changing the result set.
- [x] Add the bounding test: a `tests` answer past the result limit is truncated with disclosure and resumes deterministically via its continuation token.

## Heuristic-grade labeling (code-navigation)

- [x] Write the failing labeling tests, then implement the answer-level `classification: "convention"` field, per-site `test_rule`, and the one-line convention-based notice in the human render — to green:
  - a `tests` answer carries the classification field and per-site rules; a `references` answer over the same subject carries neither;
  - an empty `tests` answer still carries the classification marker in JSON;
  - the rendered `tests` answer states the results are convention-classified;
  - the rendered empty answer presents "no convention-classified test reference found", not proof that nothing tests the subject;
  - a `tests` query against a stale index carries both the staleness flag and the classification marker.
- [x] Write a failing self-description test — `tests` presented as convention-based classification, mirroring the existing dependents-as-impact assertion — then extend the trace self-description to green.

## Surface and docs

- [x] Write a failing surface-index test asserting `tests` appears among the trace relation values, then bump the manifest `SURFACE_VERSION` to green.
- [x] Document the `tests` relation and its heuristic-grade labeling in the README alongside the other trace relations.
- [x] Dogfood spot-check on the persistent clones (httpx2, Flask, ripgrep, fd): `trace <known symbol> tests` returns plausibly test-classified sites with rule provenance; record observations in the change notes (`notes.md`).

## Review remediation

- [x] Unaligned definitions classify by document: fall back to the extracted definition occurrence's document for the document-scoped rules when the join refused the definition, with Rust and Python regressions (a refused definition in `tests/` / `pkg/test_api.py` still classifies).
- [x] Add the `*_test.py` suffix-form classification test the contract names.
- [x] Revise the code-graph delta's Rust test-directory clause to the ratified static rule (any directory named `tests`), aligning spec, proposal, and design.
- [x] Clarify in the code-navigation delta that ambiguous/unresolved answers precede classification and carry no marker; pin with a scenario and an absent-answer test.
- [x] Route the foreign-database replace hazard to a new `store-ownership-guard` proposal stub (probe-confirmed; contracted baseline behavior, out of this change's scope).
- [x] Record the limit-does-not-bound-work and classification-reparse observations in the `internal-efficiency` stub.

## Verify remediation

- [x] Assert the convention marker on the truncated and resumed pages of the `tests`-relation bounding test (partition-incomplete evidence at the paged answer reconstruction).
- [x] Pin the ambiguous-subject no-marker arm with a delta scenario and an ambiguous `tests` answer test.
