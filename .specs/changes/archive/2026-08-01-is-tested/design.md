# Design: is-tested

## Context

The reverse-reference substrate this change filters already exists: aligned reference occurrences carry an enclosing-declaration attribution (`enclosing_id`), and the `trace` command exposes named relations over it (`references`, `dependents`, `importers`, `implementers`).
The baseline also establishes the two patterns this change composes: attributions carry the rule that accepted them as provenance, and the store's schema version is checked with a replace-or-refuse contract (a build replaces an incompatible store; a query refuses with recovery guidance).
The current schema version is 11; the crate is pre-first-MINOR (v0.1.0, untagged), so the governance deprecation-schedule clause does not apply and the bump carries a migration note only.
CLI-alignment (shipped 2026-07-19) delivered the folded `trace` surface and named-relation convention this change rides; no new command is added.

## Decisions

### Decision: Two-layer heuristic labeling

**Chosen:** each `is_test` classification carries the convention rule that stamped it (per-result provenance), and every `tests` answer carries a relation-level structural marker naming it convention-based classification; no global confidence-tier taxonomy is introduced.

**Rationale:** the rule-as-provenance layer reuses the exact pattern the guarded join established, gives per-result calibration (an attribute classification is near-certain; a directory classification is softer), and makes wrong classifications debuggable.
The relation-level marker makes the heuristic grade unmissable even for a consumer that never inspects per-result detail — calibration by structure, not caveat.

**Alternatives considered:**

- Global confidence-tier taxonomy (resolved vs. heuristic as named contract concepts): designing a general system around one data point; if a second heuristic-grade relation ever lands, promoting the marker to a named tier is a cheap additive spec change.
- Documentation-only caveat: a caveat is prose a consumer can skip; the north star requires the label to be structural.

### Decision: Enclosing-declaration filter grain

**Chosen:** a reverse-reference site belongs to the `tests` relation exactly when the declaration it is attributed to (its existing enclosing-declaration attribution) is classified test code.

**Rationale:** it is one predicate on data the occurrences already carry, and it can never produce a false "nothing tests this": a shared helper under a test tree that makes the only direct call to the subject is itself test-classified, so its site is returned.
Its known softness — sometimes naming a helper rather than a runnable test case — is exactly what the heuristic labeling already tells the consumer to expect.

**Alternatives considered:**

- Strict test-function grain (only sites whose enclosing declaration is itself a test case): returns a confidently wrong empty answer whenever coverage flows through helpers, the precise failure mode the honest-label story exists to prevent; fixing it would need multi-hop traversal through helpers, new machinery the proposal scopes out.
- File/module grain: coarser than the question; the consumer wants the declarations to read or run, not file names.

### Decision: Static convention rules as the floor, runner enumeration deferred

**Chosen:** classification is a pure function of source text — attribute, enclosure, file name, and path signals — with no execution of project tooling; the provenance field is an open set of rule names so a stronger `runner-enumerated` rule can join later as an optional per-backend capability.

**Rationale:** runner collection (`cargo test -- --list`, `pytest --collect-only`) compiles test binaries or imports modules — executing project code, a boundary c10r deliberately does not cross — and its answer depends on the environment, widening the freshness surface.
The static rules are the runners' own documented discovery conventions applied statically, are replay-honest, and hold at the contract floor across current and intended languages (Go, JS/TS, C#), where runner enumeration is fragmented.

**Alternatives considered:**

- Runner enumeration now: requires a working build/environment, executes arbitrary project code at index time, and cannot classify helpers (it enumerates runnable cases only), so the static classification would still be needed underneath.
- Hybrid (static + runner verification in one change): defers cleanly; nothing in the schema or contract has to be redesigned to add it later.

### Decision: No standalone declaration-name rule for Python

**Chosen:** Python classification uses file-name and directory rules only; a `test_`-prefixed function in a document no rule accepts stays non-test.

**Rationale:** pytest collects test functions only within collected files, so inside a test file the file rule already classifies everything (helpers included), and outside one a `test_`-named production function (a `test_connection()` health check) would be a confident false positive the runner itself would not collect.
Calibration over coverage: prefer the disclosed miss to the confident false positive.

**Alternatives considered:**

- Name rule everywhere: classifies runner-unreachable production functions as tests.
- Name rule scoped to test-classified documents: adds no classification the file and directory rules do not already make.

### Decision: Rule definitions and precedence

**Chosen:**

- Rust test-attribute: any attribute on the declaration whose path's terminal segment is `test` (`#[test]`, `#[tokio::test]`, `#[sqlx::test]`).
- Rust test-configuration: a module carrying `#[cfg(test)]` classifies itself and every symbol it transitively contains; containment is taken from the persisted enclosure relation after all documents are parsed, so an out-of-line module declaration whose body lives in its own document classifies that document's symbols through its gated declaration.
- Rust test-directory: documents lying under a directory named `tests` — the static approximation of Cargo's integration-test layout, since package-manifest locations are not visible to the build's source-and-index ingest; the path-collision false positive this widens is the same accepted, labeled risk the Python directory rule carries.
- Python test-file: file name matching `test_*.py` or `*_test.py` (pytest's discovery defaults), or exactly `tests.py` (the single-file convention unittest's default `test*.py` discovery pattern collects), or exactly `conftest.py` (pytest's reserved fixture/plugin file, test infrastructure wherever it sits).
- Python test-directory: any path component named `tests` between the workspace root and the document.
- When several rules accept one symbol, the recorded rule is the first in the fixed order attribute > configuration > file > directory (most specific evidence wins), so provenance is deterministic.

**Rationale:** the rules are anchored in the runners' documented discovery conventions — pytest's file patterns, unittest's discover pattern, and Cargo's test-target layout — and curated as c10r's calibration policy rather than copied wholesale; the precedence order records the strongest evidence without inventing a multi-rule provenance structure.
The full `test*.py` glob unittest discovery defaults to is deliberately not adopted: it would classify any production module that merely starts with `test` (`testimony.py`) as test code, so only its exact conventional instance `tests.py` is taken.
Known tail not covered: attribute macros that do not end in the `test` segment (`#[rstest]`, `proptest!`) outside test-classified territory, and `#[cfg(test)]` on a lone non-module declaration; both are rare, land in test modules or test directories in practice, and are the disclosed-miss side of the calibration trade.

### Decision: Storage shape and version bumps

**Chosen:** one nullable `test_rule` TEXT column on the `symbols` table — `NULL` means non-test, a rule name means test-classified with that provenance; `SCHEMA_VERSION` 11 → 12; the CLI surface manifest's `SURFACE_VERSION` bumps because `trace` gains a relation value.

**Rationale:** a single column carries both the predicate and its provenance (the predicate is `test_rule IS NOT NULL`), so the two cannot disagree; the existing replace-or-refuse contract handles old stores with no new machinery.

**Alternatives considered:**

- Separate `is_test` boolean plus rule column: two columns that must stay consistent for no added expressiveness.
- Side table of classifications: joins on every `tests` query for a value that is functionally one attribute of the symbol.

### Decision: Query shape and answer fields

**Chosen:** `trace <subject> tests` reuses the reverse-reference query filtered on the attributed declaration's `test_rule IS NOT NULL` — the site's `enclosing_id`, or, when the site attributes to the module itself (`enclosing_id` is `NULL`), the document's module symbol, mirroring the fallback the detail projection already uses — ordered exactly as `references` answers are ordered; the JSON answer carries an answer-level `classification: "convention"` field and each site carries its enclosing declaration's `test_rule`; the human render prints a one-line convention-based notice above the results.

**Rationale:** no new traversal machinery and no query-time parsing; determinism and bounding are inherited from the `references` path, and the marker fields are additive so no existing consumer's parse breaks.

## Architecture

```text
build (syntax pass)                                query (trace <subject> tests)
────────────────────                               ─────────────────────────────
syntax tree per document                           resolve subject
  │                                                  │
  ├─ classify declaration:                           ├─ reverse reference sites of subject
  │    attribute? cfg(test) enclosure?               │    (existing references traversal)
  │    test file name? tests/ path?                  │
  │         │                                        ├─ keep site whose attributed declaration
  │         ▼                                        │    (enclosing_id, else the document's
  │    symbols.test_rule = rule | NULL               │    module) has test_rule IS NOT NULL
  │                                                  │
  ├─ propagate cfg(test) through enclosure            ▼
  │    (out-of-line module bodies)                 answer: sites + per-site test_rule
  │                                                  + classification: "convention"
  └─ join / attribution / edges: unchanged           + provenance/freshness (unchanged)
```

## Risks

- **False negatives from overridden runner configs** (pytest `python_files` overrides, Cargo `harness = false`, attribute macros outside the `test`-segment convention): the relation-level marker discloses the grade, and the open rule set leaves room for a future `runner-enumerated` rule; not silently mitigated.
- **False positives from path collisions** (production code under a directory legitimately named `tests`): accepted and labeled; the per-site rule provenance makes the cause inspectable.
- **Schema bump forces a rebuild of every existing store**: the replace-or-refuse contract already names the recovery action, and the commit hook refreshes indexes on the next commit.
- **Determinism drift between `tests` and `references` ordering**: the filter must not reorder; the tests reuse the `references` ordering assertions to pin it.
