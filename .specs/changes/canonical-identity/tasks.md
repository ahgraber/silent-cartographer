# Tasks: canonical-identity

## Setup and fixtures

- [ ] Create the worktree `.worktrees/canonical-identity` on branch `canonical-identity` from `main`.
- [ ] Verify from the Cargo reference that a Cargo package name cannot contain `+`; record the citation in `design.md` § OwnerGrammar.
- [ ] Add `tests/fixtures/rust-conformance/`: Cargo package `libbin` with `src/lib.rs` and `src/bin/tool.rs` each declaring `mod util { pub struct Config; pub fn load() {} }`, the binary's `main` calling `util::load()` and `libbin::util::load()`, `tests/a.rs` and `tests/b.rs` each declaring `mod common;` and calling `common::fixture()`, and `tests/common/mod.rs` defining `pub fn fixture()`.
- [ ] Add an ignored generator test in `tests/semantic_engine.rs` that runs `rust-analyzer scip` over the Rust conformance project and writes `tests/fixtures/rust-conformance/index.scip` and `RUST-ANALYZER-VERSION`; run it and commit the outputs.
- [ ] Add the class `Client` to `tests/fixtures/python-conformance/pkg/__init__.py` and the annotated module global `current_app: Widget = Widget()` to `pkg/consumer.py`; regenerate `index.scip` with the existing generator and commit it.
- [ ] Open check for the doubled-name collapse: decode a Flask scip-python index and list every descriptor with two consecutive equal term segments; confirm each is an annotated module global, and record the result in `design.md` § ScipPythonMapsToCanonicalForm.

## Identity contract (symbol-identity)

- [ ] Add an owner marker field to `Descriptor` in `src/identity.rs` (none, `+kind.name`, or `+shared`) and write it in `projection_body` as `package[+marker]::segments`.
- [ ] Unit test `projection_writes_owner_marker_after_package`: a descriptor with no marker projects to `ws::libbin::util::Config`, with `bin.tool` to `ws::libbin+bin.tool::util::Config`, with shared to `ws::libbin+shared::common::fixture`.
- [ ] Make `project_all` collide only on identical owner and segments, so two descriptors that differ in owner get no `#` suffix.
- [ ] Unit test `distinct_owners_need_no_disambiguator` (`Same descriptor in two units carries no disambiguator`) and keep `true_duplicates_ranked_by_definition_location` passing for same-owner twins.

## Python mapping (semantic-engine, Python adapter)

- [ ] Add `canonicalize_scip_python_descriptors` to `src/semantic/python_adapter.rs`, run after `translate_index` and before `classify_module_kinds`: split a dotted module segment per component; drop a trailing `__init__` meta segment; merge symbols whose descriptors become equal, uniting their occurrences.
- [ ] Unit tests, one per write-site: `dotted_module_splits_per_component` (`Python module path splits per component`), `init_marker_dropped_and_module_symbols_merge` (`Python module and package carry no marker`), `top_level_module_is_one_segment`, `class_and_method_segments_unchanged` (`Python method nests in its class`).
- [ ] Add the doubled-name collapse (two consecutive equal term segments directly after the module run) to the same function, per the recorded open check.
- [ ] Unit test `doubled_module_global_collapses_to_one_segment` (`Python module global is one segment`), and a negative test that `Widget#size.` and `Widget#_size.` are unchanged.
- [ ] Change `classify_module_kinds` to recognize a descriptor made only of module segments; keep it on the Python path only.
- [ ] Unit test `module_only_descriptor_classifies_as_module`, and a test over `tests/fixtures/exemplar.scip` that the Rust translation path leaves every symbol kind unchanged (`ModuleClassificationStaysInThePythonAdapter`).
- [ ] Change `module_dotted_name` and `module_identity_parts` in `src/graph/join.rs` to read a module's dotted name as its module segments joined with `.`, and remove the `__init__` reading.
- [ ] Integration test in `tests/code_graph.rs`: the Python conformance project's module-name rule acceptances (`Python module reference accepted under the module-name rule`, `Nested module accepted at trailing component-runs only`) hold under the canonical form.
- [ ] Integration test `package_initializer_declaration_belongs_to_package` (`Python declaration in a package initializer belongs to the package`): `Client` projects to `…::python-conformance::pkg::Client`.

## Rust owners (semantic-engine, Rust adapter and ingest)

- [ ] Replace `parse_library_roots` with `parse_targets` in `src/semantic/rust_adapter.rs`, keeping every target's package, kind, name, and workspace-relative root, and record the inventory on `ExtractedIndex` in place of `library_roots`.
- [ ] Unit test `parse_targets_keeps_every_target_kind_and_name` over a captured `cargo metadata` output with lib, bin, test, example, bench, and custom-build targets, including a default binary named like the package.
- [ ] Move the `mod`-chain derivation (`parent_document_map`) from the join into ingest so it runs before `project_identities`, and add `assign_owners` that walks from each target root and sets each definition's owner marker: one target gives `+kind.name` or none for a library kind; two or more give `+shared`; none gives no marker and a disclosure entry.
- [ ] Unit tests for `assign_owners` on a synthetic index: `library_root_documents_are_unmarked` (`Rust library declarations are unmarked`), `binary_documents_marked_by_kind_and_name` (`Rust binary declarations are marked by kind and name`), `default_binary_marked_by_kind` (`Default binary with the package's name is distinguished by kind`), `document_reached_from_two_roots_is_shared` (`Source compiled into two units has one shared identity`), `document_reached_from_no_root_is_disclosed` (`Document reached from no target root degrades to the package-only owner with disclosure`), `missing_inventory_leaves_package_only_owners_and_discloses` (`Unavailable target description degrades to package-only owners with disclosure`).
- [ ] Carry the two owner disclosure lists through ingest to the build answer as `"owners"`, with each entry naming its package or document and its reason, as `design.md` § OwnerUnknownDegradesWithDisclosure states, and render one warning per entry with its cause, consequence, and remedy.
- [ ] Unit test the human render of each reason (`metadata_timeout`, `no_manifest`, `metadata_failed`, `no_target_loads_it`): each warning names the package or document, states the cause after "because", and states what it means for answers.
- [ ] Integration tests in `tests/code_graph.rs` over the Rust conformance fixture: `rust_conformance_identities_carry_owners` asserts the library `Config`, the binary `Config`, and the shared `fixture` identities; `owners_disclosure_is_empty_when_all_determined`; and, with the inventory withheld through the test seam, `missing_inventory_discloses_undetermined_package`.
- [ ] Integration test `adding_a_binary_changes_no_other_identity` (`Adding a target changes no other unit's identities`): build a copy of the Rust conformance project, add `src/bin/second.rs`, rebuild, compare every library and `tool` identity byte for byte.

## Twins and attribution (code-graph)

- [ ] Key `DuplicateGroup` construction in `normalize` and `twin_symbols_by_descriptor` in the join by the descriptor with the owner stripped, and compute the `duplicated` flag in `src/graph/mod.rs` on the same key.
- [ ] Unit test `cross_target_twins_stay_one_group_with_distinct_identities`, and keep the existing duplicated-descriptor disclosure tests passing.
- [ ] Extend the package-name rule in `src/graph/join.rs` to fire when the occurrence token is the final component of a `scoped_identifier` whose first component spells the package name, selecting the twin whose owner is the package's default unit.
- [ ] Integration test `package_qualified_path_resolves_to_library` (`Package-qualified path resolves to the library's declaration`) over the Rust conformance fixture: `libbin::util::load()` is attributed to the library's `load` with `target_metadata` provenance, `util::load()` to the binary's `load`, and `references` of the library's `load` lists `src/bin/tool.rs` at the qualified call.
- [ ] Integration test `package_qualified_path_without_inventory_is_ambiguous`: the same call with the inventory withheld is duplicate-ambiguous.
- [ ] Keep every existing scenario test of `Occurrences of duplicated descriptors are never arbitrarily attributed` passing (crate-root package-name reference, scope locality, guarded join).

## Currency (code-graph)

- [ ] Record the target inventory and the digest of every workspace member's `Cargo.toml` in the store metadata, and compare them in `current_index_accounting`.
- [ ] Integration test `renamed_target_rebuilds` (`A renamed build target rebuilds`): rename `tool` to `cli` in `Cargo.toml` only, rebuild, assert `rebuilt: true` and the owner `+bin.cli`.
- [ ] Integration test: a rebuild with an unchanged manifest and inventory reports the index current (`Rebuilding an unchanged workspace analyzes nothing`, Rust and Python).

## Store (code-graph)

- [ ] Bump `SCHEMA_VERSION` to 19 in `src/graph/schema.rs`.
- [ ] Run the existing `Incompatible index stores are replaced or refused, never half-used` tests against a store written at version 18, and add a test that a version-18 Python store is replaced on build and refused on query.

## Resolution (code-navigation)

- [ ] Integration tests in `tests/code_navigation.rs` over the Rust conformance fixture: `package_qualified_name_resolves_to_library_only` (`Package-qualified name resolves to the library's declaration`), `path_in_two_units_returns_candidates` (`Path declared in two units returns candidates`), `owner_marked_identity_round_trips` (`Owner-marked identity round-trips`).
- [ ] Integration test: `pkg.shapes.Widget` still resolves uniquely under the canonical form (`Python dotted qualified name resolves`).

## Reference identity lists (symbol-identity)

- [ ] Write `tests/fixtures/python-conformance/IDENTITIES.txt` and `tests/fixtures/rust-conformance/IDENTITIES.txt` by hand from the spec's canonical form, one identity per line, sorted.
- [ ] Integration test `scip_python_yields_python_reference_identities` (`scip-python yields the Python reference identities`): every reported module, class, function, method, and module global has exactly its listed identity, and the reference to `Widget` from `consumer.py` resolves to the listed `Widget`.
- [ ] Integration test `rust_analyzer_yields_rust_reference_identities` (`rust-analyzer yields the Rust reference identities`): every reported declaration in the library, the binary, and the tests has exactly its listed identity, and `libbin::util::load()` resolves to the listed library `load`.
- [ ] Integration tests for `Identity does not depend on source location` over copies of the Rust and Python conformance projects: `inline_module_moved_to_file_keeps_identity`, `module_file_moved_to_directory_module_keeps_identity`, `python_package_moved_between_source_roots_keeps_identity`, `module_rename_changes_identity`.
- [ ] Integration tests for the remaining nesting scenarios over a copy of the Rust conformance project: `inline_modules_nest_without_directory` (`Rust inline modules nest without a directory`) and `path_attribute_module_keeps_declared_name` (`Rust module loaded through a path attribute keeps its declared name`).

## Fixtures, snapshots, and dogfood

- [ ] Regenerate every test fixture and snapshot that holds a Python identity in the dotted form, a non-library Rust identity, or a cross-target twin suffix (`tests/code_graph.rs`, `tests/code_navigation.rs`, `mcp/tests/`, `tests/fixtures/surface_manifest.snapshot.json` if the build answer's new field is listed there).
- [ ] Build `samples/rust-libbin` and a Flask clone copy with the branch binary; capture `get libbin::util::Config`, `trace --relation references` of the library's `load`, and `get flask.app.Flask`; record the outputs in this change directory.

## Documentation

- [ ] Document the identity grammar `workspace::package[+unit]::segments`, the marker forms, and the Python and Rust mappings in `docs/commands/common.md` under the reference forms, and add a short identity section to the README.
- [ ] Document the `owners` disclosure and the target inventory as a currency input in `docs/commands/build.md`.
- [ ] Add the schema bump and the identity change to the unreleased section of `CHANGELOG.md`.

## Checks

- [ ] Run the full Rust test suite and the MCP test suite.
- [ ] Run the pre-commit hooks on every changed and new file, and fix any failure.
