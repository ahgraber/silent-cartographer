# Tasks: python-adapter

Standing rules for every task, restated so no task needs outside context:

- Never read, grep, or index anything under `/Users/mithras/_code/_worktrees/silent-cartographer/old-c10r-worktree/` (clean-room wall).
- Test-first for every contract scenario: write the named failing test, then make it pass.
  Never weaken an existing test to make it pass.
- Do not loosen the default name-token alignment rule to raise Python's aligned count — refusals are the correct launch behavior (design.md, launch posture).
- Finish every group with `cargo fmt`, `cargo clippy --all-targets`, `cargo test --all-targets` all green before starting the next group.
- No commits.

## Shared SCIP Translation (pure refactor, no behavior change)

Definition of done: the SCIP-protobuf translation lives in a language-neutral module; the Rust adapter delegates to it; every existing test passes unchanged.

- [x] Create `src/semantic/scip.rs` and move `translate_index`, `parse_scip_symbol`, `translate_suffix`, `read_range`, `symbol_kind_from`, and the `DEFINITION_ROLE_BIT` constant there from `src/semantic/rust_adapter.rs`, moving their unit tests with them.
- [x] Re-export or import the moved items in `rust_adapter.rs` so `RustAdapter::analyze` behavior is byte-identical; delete nothing else.
- [x] Gate: run the full suite; zero test changes allowed in this group (if a test must change, the refactor altered behavior — stop and fix the refactor instead).

## Python Syntax Oracle

Definition of done: the join can consume Python documents through the same queries it uses for Rust.

- [x] Add the `tree-sitter-python` crate (pinned in Cargo.toml, committed in Cargo.lock).
- [x] Introduce a language dimension in `src/graph/syntax.rs`: the existing Rust node-kind tables (declaration kinds, name-node kinds, module-declaration query) become per-language tables selected by an enum (`Language::Rust`, `Language::Python`); the `SyntaxTree` API surface (`enclosing_declarations`, `name_node_containing`, `has_construct_at`, `text_at`, `is_module_declaration_name`, declaration enumeration) stays unchanged for callers.
- [x] Python declaration-kind table: `module` (the document), `class_definition`, `function_definition` — where a declaration is wrapped in `decorated_definition`, the declaration's span is the wrapper's span (decorators included) and its name node is the inner definition's `name` field; async functions are `function_definition` nodes and need no special case.
- [x] Python name-node table: `identifier`; `is_module_declaration_name` returns false for every Python span (Python has no `mod`-style declaration; the module-chain locality rule is simply inert for Python).
- [x] Test `python_declarations_enumerate_with_names_and_spans`: parse a fixture source with a class containing a method, a decorated function, and an async function; assert each declaration's kind, name text, and span (decorated span includes the decorator line).
- [x] Test `python_enclosing_declarations_innermost_first`: a position inside a method body returns [method, class]; a position at module top level returns [].
- [x] Test `python_name_node_containment`: a span inside an identifier returns that identifier's span; a span on a keyword returns None.

## Python Adapter (environment, invocation, provenance)

Definition of done: `PythonAdapter` implements `SemanticEngine`; environment handling follows the design's detection order with typed refusals; environment facts ride provenance.

- [x] Environment resolution in `src/semantic/python_adapter.rs` (new file): resolve in order — explicit path argument, `$VIRTUAL_ENV`, `.venv/` then `venv/` under the workspace root — returning a typed error when none resolves; never fall back to a system interpreter.
- [x] Test `env_resolution_prefers_explicit_then_virtual_env_then_dot_venv` (unit; build the three-branch precedence over temp dirs and a scoped env var).
- [x] Test `env_resolution_refuses_when_nothing_resolves`: no flag, no `$VIRTUAL_ENV`, no venv dir → the typed error names the three mechanisms (scenario: Unresolvable environment refuses with guidance — adapter-level half; the build-level half is in the Store & Selection group).
- [x] Environment facts: interpreter version (`<python> --version` output), resolved environment path, and the installed-package fingerprint — the sorted list of `*.dist-info` directory names under the environment's `site-packages`, hashed; expose as a serializable struct on `ExtractedIndex` (nullable/absent for Rust).
- [x] Test `package_fingerprint_changes_when_dist_info_set_changes` (unit over a temp site-packages: add one `foo-1.0.dist-info` dir, fingerprint differs; same set in different creation order, fingerprint identical).
- [x] scip-python invocation: run `scip-python index` in the workspace root with the resolved environment's interpreter active, output to a temp SCIP file, parse through the shared `scip` module, thread environment facts onto the index, `library_roots` left empty; provenance = analyzer name `scip-python` + the version the tool reports.
- [x] Typed unavailability: `scip-python` missing from PATH → `SemanticError::Unavailable` naming the tool and how to install it (scenario: Missing indexer tool refuses with guidance — adapter half).
- [x] Test `python_adapter_reports_unavailable_when_tool_missing` (point PATH at an empty dir; assert the error type and that its message names scip-python).

## Store, Provenance & Selection (schema v7)

Definition of done: environment facts persist and drive staleness; build selects exactly one backend; all refusal paths are typed.

- [x] Bump `SCHEMA_VERSION` to 7 in `src/graph/schema.rs`; add a nullable `environment` TEXT column (JSON) to `index_metadata`; thread it through `IndexMetadata`, `write_metadata`, `read_metadata` in `src/graph/store.rs` (absent for the Rust adapter).
- [x] Test `environment_provenance_round_trips_through_metadata` (scenario: Declared environment facts ride the provenance).
- [x] Freshness: extend the staleness comparison so a recorded environment differing from the environment in effect marks results stale and flagged for reindex; unchanged environment stays fresh (`src/commands.rs` `current_state`/freshness path).
- [x] Test `changed_environment_marks_stale` (scenario: Changed environment marks stale — vary the fingerprint) and extend the existing fresh-path test to cover an unchanged recorded environment.
- [x] Language selection in `src/commands.rs` `run_build`: `Cargo.toml` present → Rust backend; `pyproject.toml` present → Python backend; both → typed error naming `--language`; `--language rust|python` (clap value enum in `src/cli.rs`) overrides detection; neither manifest → typed error saying no supported project was detected.
- [x] Test `python_manifest_selects_python_backend` (scenario: Single-language workspace selects its backend; assert provenance analyzer name).
- [x] Test `two_manifests_without_selection_refuse` (scenario: Two manifests without a selection refuse; assert the error names `--language` and no store is written).
- [x] Test `explicit_selection_overrides_detection` (scenario: Explicit selection overrides detection).
- [x] Test `failed_build_leaves_existing_store_untouched`: a store exists; a build attempt fails on unavailable tool; the store's rows and metadata are byte-identical afterward (scenario: Missing indexer tool refuses with guidance — the "store untouched" clause; the whole-build transaction from duplicate-identity already provides this, the test pins it for the new failure paths).

## Conformance (per-backend gating, committed Python fixture)

Definition of done: the conformance suite runs per backend; Python conformance is verifiable without scip-python installed except for one clearly-skipped live leg.

- [x] Commit a minimal Python fixture project under `tests/fixtures/python-conformance/`: a package with `__init__.py`, one module defining a class with a method and a base class, a second module importing and calling across the boundary, and a `pyproject.toml`.
- [x] Generate and commit the fixture's SCIP index once via an `#[ignore]` generator test (mirror the existing `generate_exemplar_scip_fixture` pattern); record the scip-python version used inside the fixture directory.
- [x] Parameterize `src/semantic/conformance.rs` by backend so the suite runs the same clauses against each adapter's output (scenario: Backends gate independently — test `nonconformant_backend_does_not_affect_the_other` using a deliberately broken fixture engine beside a conforming one).
- [x] Test `python_definition_occurrence_extracted` and `python_reference_occurrence_extracted` from the committed fixture index (scenarios: Python definition/reference occurrence extracted).
- [x] Live leg: a test that runs the real `scip-python` against the fixture project when the tool is on PATH and compares to the committed index's shape; when the tool is absent it must emit an explicit skip (e.g. `eprintln!("SKIP: scip-python not installed")` + early return), never a silent pass.

## Join & Navigation over Python (fixture-level)

Definition of done: every Python spec scenario below has a named test driven from fixture data (no live tool needed).

- [x] Test `python_name_token_aligns_under_default_rule` (scenario: Python name token accepted under the default rule; assert rule provenance is the default rule).
- [x] Test `python_non_name_occurrence_is_refused` (scenario: Python occurrence outside the default rule stays refused; assert it lands in a typed discrepancy, not aligned).
- [x] Test `python_import_produces_imports_edge` (scenario: Python import produces an imports edge — module-scope reference occurrence attributed to the module).
- [x] Test `python_base_class_produces_type_hierarchy_edge` (scenario: Python base class produces an edge) — derive from the base-name token occurrence in the class definition header, resolved through aligned occurrences exactly as Rust impl headers are; skip-never-guess when the base token has no aligned occurrence.
- [x] Test `python_multiple_bases_produce_one_edge_each` (scenario: Multiple bases each produce an edge).
- [x] Test `python_body_retrieval_is_byte_exact` (scenario: Retrieve full body of a Python symbol — fixture body spans several indentation levels; compare bytes).
- [x] Test `python_get_by_position_resolves_enclosing_method` (scenario: Retrieve Python symbol by position).
- [x] Test `python_dependents_trace_carries_kind_and_distance` (scenario: Trace dependents of a Python symbol — a `uses` dependent and an `imports` dependent).
- [x] Test `python_dotted_qualified_name_resolves` (scenario: Python dotted qualified name resolves — supply `pkg.module.Class` style reference; assert exactly one symbol resolves).

## Dogfood & Ground Truth (httpx2)

Definition of done: a recorded `dogfood.md` with reproducible numbers from a pinned tag, hand-checked ground truth, and categorized refusals filed as follow-up rule-family candidates.

- [x] Clone `https://github.com/pydantic/httpx2` at a release tag chosen now and recorded in `dogfood.md`; create its venv and install the project into it (the index must resolve third-party imports).
- [x] Build the index; record the full accounting line (aligned per rule, text_mismatch, semantic_only, duplicate_ambiguous, syntax_only) and the duplicated-group disclosure in `dogfood.md`.
- [x] Cross-check duplicated groups against the raw SCIP index's multi-definition population with the scip-check tool (it is SCIP-generic); any mismatch is an over/under-split defect — stop and fix before proceeding.
- [x] Hand ground truth, recorded symbol by symbol: definition location of 5 symbols via `get`; the complete reference set of 2 small symbols vs a manual read; every declared base class in 2 modules vs `type_hierarchy` edges; direct dependents of 1 widely-used symbol vs an occurrence-derived count (the blast-radius cross-check recipe).
- [x] Categorize the top refusal families in `text_mismatch`/`semantic_only` by sampled inspection (candidates from design: decorators, properties, re-exports through `__init__`, `self`/`cls`); file each family with counts in brainstorm §11 as rule-family candidates — do not implement any rule in this change.
- [x] Verify environment staleness end-to-end on the dogfood repo: `pip install` one new package into the venv; `status` reports stale; rebuild reports fresh.
- [x] Record everything in `.specs/changes/python-adapter/dogfood.md`, including the scip-python version and the pinned tag.
