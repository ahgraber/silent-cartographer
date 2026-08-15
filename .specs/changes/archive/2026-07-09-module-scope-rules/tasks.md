# Tasks: module-scope-rules

Standing rules for every task, restated so no task needs outside context:

- Never read, grep, or index anything under `/Users/mithras/_code/_worktrees/silent-cartographer/old-c10r-worktree/` (clean-room wall).
- Test-first for every contract scenario: write the named failing test, then make it pass.
  Never weaken an existing test to make it pass; where this change's spec deltas alter a scenario an existing test pinned, the task says so explicitly and states the replacement behavior.
- Every failure direction is refusal: when in doubt, an occurrence stays refused/ambiguous — never attributed by guess.
- Finish every group with `cargo fmt`, `cargo clippy --all-targets`, `cargo test --all-targets` all green before starting the next group.
- macOS link gotcha: prefix cargo commands with `LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib"`.
- No git commits.

## Module Kind Classification (adapter boundary)

Definition of done: Python module symbols carry `SymbolKind::Module` everywhere downstream; the join and bookkeeping read kind, not scip-python's naming convention.

- [x] Add `classify_module_kinds(index: &mut ExtractedIndex)` in `src/semantic/python_adapter.rs`: set `kind = SymbolKind::Module` on every symbol whose descriptor's terminal segment is named `__init__` with the meta segment kind and is preceded by at least one segment (the module's namespace); call it from `PythonAdapter::analyze` between `translate_index` and `normalize`.
- [x] Apply the same enrichment on the committed-fixture path: `tests/support/mod.rs`'s Python fixture helper mirrors `analyze` (translate → enrich → normalize) — without this, fixture-driven tests see stale `other` kinds while live builds see `module` (two write-sites, both covered).
- [x] Test `python_module_symbols_classify_as_module_kind`: from the committed `tests/fixtures/python-conformance/index.scip`, every `__init__`-terminal symbol has kind `module` and no class/function/parameter symbol does.
- [x] Simplify the Python branch of the document→module mapping in `src/graph/mod.rs` (`module_by_doc`): keep the zero-width-origin definition association but require `sym.kind == SymbolKind::Module` instead of accepting any identity-bearing symbol; the existing `python_import_produces_imports_edge` test must stay green.

## Module-Name Alignment Rule (guarded join)

Definition of done: Python module occurrences align under a named `module_name` rule with its own accounting bucket; everything the rule does not cover keeps refusing.

- [x] Add `AlignmentRule::ModuleName` (provenance string `module_name`) in `src/graph/join.rs`, evaluated only for `Language::Python` and only when the occurrence's symbol has kind `module`: the occurrence's span must sit on an identifier name node (same structural gate as the default rule) whose text equals the terminal dotted component of the module's namespace name — the text after the last `.` of the descriptor segment preceding the `__init__` terminal, or the whole segment when it has no dot.
- [x] Add the `aligned_module_name` field to `JoinAccounting` (counted like the existing per-rule fields; conservation must still hold).
- [x] Test `python_module_reference_aligns_under_module_name_rule` (scenario: Python module reference accepted under the module-name rule): from the Python fixture, an `import`-site module occurrence aligns and its persisted rule provenance is `module_name`.
- [x] Test `nested_module_accepted_at_terminal_component_only` (scenario: Nested module accepted at its terminal component only): synthetic index over a two-component module (e.g. namespace `pkg.shapes`) — a `shapes` token aligns; a `pkg` token occurrence of the same `pkg.shapes` symbol is refused (build the two occurrences directly in a synthetic `ExtractedIndex`, the way existing join unit tests do).
- [x] Test `non_module_occurrence_is_outside_module_name_rule` (scenario: Non-module occurrence is outside the module-name rule): a class-kind occurrence at a non-matching token is refused, not rescued by the module rule.
- [x] Existing test `python_non_name_occurrence_is_refused` pinned the fixture's import-line module occurrence as its refusal sample — that exact occurrence now legitimately aligns under the module-name rule (spec scenario reworded to "Python occurrence outside every rule stays refused").
  Retarget the test to the fixture's zero-width module definition marker (document origin, no identifier at the span) and rename it `python_occurrence_outside_every_rule_stays_refused`; it must still assert a typed discrepancy and zero aligned rows for that occurrence.
- [x] Extend the fixture project `tests/fixtures/python-conformance/` with one bare module-import statement (e.g. `from pkg import shapes` or `import pkg.shapes`) in `consumer.py`, regenerate the committed `index.scip` once via the `#[ignore]` generator test, and refresh `SCIP-PYTHON-VERSION` if the tool version changed.
- [x] Test `python_module_import_produces_module_to_module_edge` (scenario: Python module import produces a module-to-module edge): from the regenerated fixture, an `imports` edge exists from the importing module's symbol to the imported module's symbol.

## Scope-Grained Twin Locality (join, twin groups)

Definition of done: a same-document twin-group reference is attributed to the twin whose scope encloses it when exactly one does, with `declaration_scope` locality provenance; every other case stays `duplicate_ambiguous`.

- [x] In the twin-group locality sequence in `src/graph/join.rs` (after defining-document, module-chain, and target-metadata all decline): compute the reference's enclosing-declaration chain from the already-parsed `SyntaxTree`, walk it innermost-outward, and find the first declaration whose full span contains at least one twin's definition location (same document required); exactly one twin contained → attribute the occurrence to that twin with locality `declaration_scope`; more than one, or no twin-bearing declaration in the chain → leave the existing `duplicate_ambiguous` outcome.
- [x] Extend the fixture project with a property getter/setter pair (e.g. a `size` property with `@size.setter` on `Widget` in `pkg/shapes.py`) so the committed index contains a real same-document twin group; regenerate the committed index via the generator (one regeneration may cover this and the module-import extension together).
  (Satisfied during the module-import group: the property pair and the bare import landed in the one fixture regeneration.)
- [x] Test `same_document_reference_inside_one_twin_scope_attributes` (scenario: Same-document reference inside exactly one twin's scope is attributed to it): from the regenerated fixture, the getter-body `self` reference attributes to the getter's twin and the setter-body `self` reference to the setter's twin; each persisted attribution carries locality `declaration_scope`.
- [x] Test `same_document_reference_with_shared_deciding_scope_stays_ambiguous` (scenario: Same-document reference whose deciding scope holds several twins stays ambiguous): synthetic index — two twin definitions inside one class, a reference in a sibling method of the same class; the innermost twin-bearing declaration (the class) holds both → `duplicate_ambiguous`.
- [x] Test `same_document_reference_outside_any_twin_scope_stays_ambiguous` (scenario: Same-document reference enclosed by no twin-bearing declaration stays ambiguous): synthetic index — two twins at document top level, a reference inside a function; no enclosing declaration contains a twin → `duplicate_ambiguous`.
- [x] Test `scope_locality_does_not_bypass_alignment` (scenario: Scope locality does not bypass the guarded join): synthetic index — a reference inside exactly one twin's scope whose source token does not satisfy any alignment rule → refused, zero aligned rows.
- [x] Confirm (extend if not already covered) that the duplicated-group disclosure and `duplicate_ambiguous` accounting reflect scope-resolved references leaving the ambiguous bucket — the existing conservation test must stay green with the new attribution path active.

## Schema v8 & Accounting Surface

Definition of done: the `module_name` bucket persists, is retrievable, and conserves; v7 stores are replaced on build and refused on query, per the standing migration contract.

- [x] Bump `SCHEMA_VERSION` to 8 in `src/graph/schema.rs`; add the `aligned_module_name_count` column to `index_metadata`; thread it through `IndexMetadata`, `write_metadata`, `read_metadata` in `src/graph/store.rs` and through `run_status` rendering in `src/commands.rs` (the always-shown accounting line and the JSON output).
- [x] Test `module_name_count_rides_metadata`: a metadata round-trip preserves the new count alongside the existing per-rule counts.
- [x] Extend the existing accounting-conservation test so the sum including `aligned_module_name` still equals the processed-occurrence total (scenario: Counts conserve the occurrence total — the new bucket is a new write-site of the conserved sum).
- [x] Confirm the pre-existing incompatible-store tests (`build_replaces_an_incompatible_store`, `query_refuses_an_incompatible_store_with_guidance`) pass against version 8 unchanged.

## Dogfood Amendments (from the first httpx2 rebuild)

Definition of done: the three deviations the first rebuild surfaced are fixed; the amended rule covers every clean-evidence span shape; the suite is green.

- [x] D1: the `build` command's printed accounting line omits the `module_name` bucket (it showed `aligned=34577` with buckets summing to 30777); add it wherever the per-rule buckets are rendered on the build path.
- [x] D2: language-gate the kind-scoped rules in `evaluate_rules` — the four Rust rules evaluate only for `Language::Rust` (the module-name rule is already Python-only); test `rust_kind_scoped_rules_do_not_fire_for_python` pins that a zero-width Python module occurrence on an empty document is refused, not accepted by the module-span rule.
- [x] D3: amend the module-name rule to the trailing-component-run condition (see the amended design decision): strip leading dots from the span text; accept iff the remainder equals a trailing component-run of the module's dotted namespace name at a component boundary; bare-identifier spans keep the name-node gate, dotted/relative spans are gated by the span text being exactly leading dots plus a dotted identifier path.
  (The pre-amendment test `nested_module_accepted_at_terminal_component_only` is renamed `nested_module_bare_terminal_aligns_and_prefix_refused` — its "only" claim no longer holds; both its assertions are kept.)
- [x] Test `python_module_relative_import_aligns_under_module_name_rule` (scenario: Relative-import module reference accepted) — fixture or synthetic span shaped `.mod`.
- [x] Test `python_module_dotted_path_aligns_under_module_name_rule` (scenario: Nested module accepted at trailing component-runs only, acceptance half) — span text equals the full dotted name.
- [x] Test `python_module_prefix_token_stays_refused` (scenario: Nested module accepted at trailing component-runs only, refusal half) — a token spelling only a leading component of the module's dotted name is refused.

## Dogfood, Regression & Alias Harvest

Definition of done: a recorded `dogfood.md` with before/after numbers on four targets against stated expectations, every deviation investigated, and a per-target alias-evidence section sufficient to design the re-export fast-follow without new discovery.

- [x] Rebuild httpx2 pinned at `v2.5.0` (the archived baseline: aligned 30700 / text_mismatch 5582 / duplicate_ambiguous 69; see `.specs/changes/archive/2026-07-09-python-adapter/dogfood.md`).
  Expectations: `text_mismatch` drops by ≈4428 (the module-name family) with `aligned_module_name` gaining the same magnitude; `duplicate_ambiguous` falls from 69 to ≈8 (property `self` twins resolve by scope; class-attribute twin references outside both twins' scopes remain, honestly).
  Any aligned-count growth beyond the consumed refusal families is over-acceptance — stop and fix.
- [x] Rebuild ripgrep at the same ref the duplicate-identity dogfood pinned (see `.specs/changes/archive/2026-07-07-duplicate-identity/dogfood.md`); expectation: same-document macro twins resolve only where the macro expansion is wrapped in a declaration; facade re-export refusals unchanged; no Rust alignment counts move (the module-name rule is Python-scoped).
- [x] Rebuild self and fd; expectation: byte-stable accounting against their archived baselines (pure regression check).
- [x] Probe Flask as the alias-dense third target: pin a release tag, create its venv, install it, and verify scip-python indexes it; on failure, substitute pydantic or requests and record the substitution and reason in `dogfood.md`.
- [x] Build the chosen alias-dense target; record its full accounting; spot-check 3 module-name attributions and 2 scope attributions by hand against source (the ground-truth discipline of prior dogfoods, scaled down since the families are already characterized).
- [x] Alias harvest, per target: group residual `text_mismatch` rows by expected-name family with counts; sample each family ≥3 sites; tag the alias-shaped families (source token is a runtime alias of the expected symbol — `import x as y`, re-export `as`-lists, `pytest.mark`-style binding objects) and record token/expected pairs and sample locations — the fast-follow's design input.
- [x] Cross-check duplicated-group disclosure against the raw index's multi-definition population with scip-check on every rebuilt target (the standing over/under-split gate) — any mismatch stops the dogfood.
- [x] Record everything in `.specs/changes/module-scope-rules/dogfood.md` (tags, tool versions, before/after tables, deviations and their investigations, alias harvest); update brainstorm §11: mark the module-name and same-document-twin families resolved with their residuals, and file the alias harvest under the fast-follow candidate.
