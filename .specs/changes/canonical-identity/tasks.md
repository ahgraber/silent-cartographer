# Tasks: canonical-identity

## Setup and fixtures

- [x] Verify from the Cargo reference that a Cargo package name cannot contain `+`; record the citation in `design.md` § UnitGrammar.
- [x] Add `tests/fixtures/rust-conformance/`: Cargo package `libbin` with `src/lib.rs` and `src/bin/tool.rs` each declaring `mod util { pub struct Config; pub fn load() {} }`, the binary's `main` calling `util::load()` and `libbin::util::load()`, `tests/a.rs` and `tests/b.rs` each declaring `mod common;` and calling `common::fixture()`, and `tests/common/mod.rs` defining `pub fn fixture()`.
- [x] Add an ignored generator test in `tests/semantic_engine.rs` that runs `rust-analyzer scip` over the Rust conformance project and writes `tests/fixtures/rust-conformance/index.scip` and `RUST-ANALYZER-VERSION`; run it and commit the outputs.
- [x] Add the class `Client` to `tests/fixtures/python-conformance/pkg/__init__.py` and the annotated module global `current_app: Widget = Widget()` to `pkg/consumer.py`; regenerate `index.scip` with the existing generator and commit it.
- [x] Open check for the doubled-name collapse: decode a Flask scip-python index and list every descriptor with two consecutive equal term segments; confirm each is an annotated module global, and record the result in `design.md` § ScipPythonMapsToCanonicalForm.

## Identity contract (symbol-identity)

- [x] Add an unit marker field to `Descriptor` in `src/identity.rs` (none, `+kind.name`, or `+shared`) and write it in `projection_body` as `package[+marker]::segments`.
- [x] Unit test `projection_writes_unit_marker_after_package`: a descriptor with no marker projects to `ws::libbin::util::Config`, with `bin.tool` to `ws::libbin+bin.tool::util::Config`, with shared to `ws::libbin+shared::common::fixture`.
- [x] Make `project_all` collide only on identical unit and segments, so two descriptors that differ in unit get no `#` suffix.
- [x] Unit test `distinct_units_need_no_disambiguator` (`Same descriptor in two units carries no disambiguator`) and keep `true_duplicates_ranked_by_definition_location` passing for same-unit twins.

## Python mapping (semantic-engine, Python adapter)

- [x] Add `canonicalize_scip_python_descriptors` to `src/semantic/python_adapter.rs`, run after `translate_index` and before `classify_module_kinds`: split a dotted module segment per component; drop a trailing `__init__` meta segment; merge symbols whose descriptors become equal, uniting their occurrences.
- [x] Unit tests, one per write-site: `dotted_module_splits_per_component` (`Python module path splits per component`), `init_marker_dropped_and_module_symbols_merge` (`Python module and package have no `**init**` segment`), `top_level_module_is_one_segment`, `class_and_method_segments_unchanged` (`Python method nests in its class`).
- [x] Add the doubled-name collapse (two consecutive equal term segments directly after the module run) to the same function, per the recorded open check.
- [x] Unit test `doubled_module_global_collapses_to_one_segment` (`Python module global is one segment`), and a negative test that `Widget#size.` and `Widget#_size.` are unchanged.
- [x] Change `classify_module_kinds` to recognize a descriptor made only of module segments; keep it on the Python path only.
- [x] Unit test `module_only_descriptor_classifies_as_module`, and a test over `tests/fixtures/exemplar.scip` that the Rust translation path leaves every symbol kind unchanged (`ModuleClassificationStaysInThePythonAdapter`).
- [x] Change `module_dotted_name` and `module_identity_parts` in `src/graph/join.rs` to read a module's dotted name as its module segments joined with `.`, and remove the `__init__` reading.
- [x] Integration test in `tests/code_graph.rs`: the Python conformance project's module-name rule acceptances (`Python module reference accepted under the module-name rule`, `Nested module accepted at trailing component-runs only`) hold under the canonical form.
- [x] Integration test `package_initializer_declaration_belongs_to_package` (`Python declaration in a package initializer belongs to the package`): `Client` projects to `…::python-conformance::pkg::Client`.

## Relative imports (symbol-identity)

- [x] Add to the Python conformance project: `pkg/relative.py` with `from .shapes import Widget`, `from . import shapes`, and a use `shapes.Widget()`; `pkg/sub/__init__.py` and `pkg/sub/deep.py` with `from ..shapes import Widget`; a module `app` in `pkg` and in `pkg/sub`, each imported in its package with `from .app import run`.
  Regenerate `index.scip` and `ty-scip.scip`, and add the new declarations to `IDENTITIES.txt`.
- [x] Map scip-python's relative-import references to their in-workspace module in the Python adapter, per document and then across documents, as `design.md` § ScipPythonRelativeImportsMapToTheirModule states.
- [x] Unit tests for each rule: same-package import, parent-package import, name bound by `from . import`, same name in two packages, a cross-document use moved only when unambiguous, and a relative import that names no workspace module left unchanged.
- [x] Integration tests in `tests/code_graph.rs` over the Python conformance fixture, one per scenario of `Relative imports resolve to their absolute path` (`Python import from the same package` including the importers answer, `Python import from a parent package`, `Python name bound by a relative import`, `Same relative name in two packages`), and the same assertions over `ty-scip.scip`.
- [x] Integration test over the Rust conformance fixture or a built copy for `Rust super path`.
- [x] Rebuild the Flask dogfood store and record in `dogfood.md` the importers of `flask.globals` and the remaining external modules that share a name with an in-workspace module.

## Rust units (semantic-engine, Rust adapter and ingest)

- [x] Replace `parse_library_roots` with `parse_targets` in `src/semantic/rust_adapter.rs`, keeping every target's package, kind, name, and workspace-relative root, and record the inventory on `ExtractedIndex` in place of `library_roots`.
- [x] Unit test `parse_targets_keeps_every_target_kind_and_name` over a captured `cargo metadata` output with lib, bin, test, example, bench, and custom-build targets, including a default binary named like the package.
- [x] Move the `mod`-chain derivation (`parent_document_map`) from the join into ingest so it runs before `project_identities`, and add `assign_units` that walks from each target root and sets each definition's unit marker: one target gives `+kind.name` or none for a library kind; two or more give `+shared`; none gives no marker and a disclosure entry.
- [x] Unit tests for `assign_units` on a synthetic index: `library_root_documents_are_unmarked` (`Rust library declarations are unmarked`), `binary_documents_marked_by_kind_and_name` (`Rust binary declarations are marked by kind and name`), `default_binary_marked_by_kind` (`Default binary with the package's name is distinguished by kind`), `document_reached_from_two_roots_is_shared` (`Source compiled into two units has one shared identity`), `document_reached_from_no_root_is_disclosed` (`File that no unit loads has no marker`), `missing_inventory_leaves_undetermined_units_and_discloses` (`Package whose units cannot be listed has no markers`).
- [x] Carry the two unit disclosure lists through ingest to the build answer as `"undetermined_units"`, with each entry naming its package or document and its reason, as `design.md` § UnitUnknownDegradesWithDisclosure states, and render one warning per entry with its cause, consequence, and remedy.
- [x] Unit test the human render of each reason (`metadata_timeout`, `no_manifest`, `metadata_failed`, `no_target_loads_it`): each warning names the package or document, states the cause after "because", and states what it means for answers.
- [x] Integration tests in `tests/code_graph.rs` over the Rust conformance fixture: `rust_conformance_identities_carry_units` asserts the library `Config`, the binary `Config`, and the shared `fixture` identities; `unit_disclosure_is_empty_when_all_determined`; and, with the inventory withheld through the test seam, `missing_inventory_discloses_undetermined_package`.
- [x] Integration test `adding_a_binary_changes_no_other_identity` (`Adding a binary changes no other unit's identities`): build a copy of the Rust conformance project, add `src/bin/second.rs`, rebuild, compare every library and `tool` identity byte for byte.

## Twins and attribution (code-graph)

- [x] Key `DuplicateGroup` construction in `normalize` and `twin_symbols_by_descriptor` in the join by the descriptor with the unit stripped, and compute the `duplicated` flag in `src/graph/mod.rs` on the same key.
- [x] Unit test `cross_target_twins_stay_one_group_with_distinct_identities`, and keep the existing duplicated-descriptor disclosure tests passing.
- [x] Extend the package-name rule in `src/graph/join.rs` to fire when the occurrence token is the final component of a `scoped_identifier` whose first component spells the package name, selecting the twin whose unit is the package's default unit.
- [x] Integration test `package_qualified_path_resolves_to_library` (`A path that starts with the package name is attributed to the library`) over the Rust conformance fixture: `libbin::util::load()` is attributed to the library's `load`, `util::load()` to the binary's `load`, and `references` of the library's `load` lists `src/bin/tool.rs` at the qualified call.
- [x] Integration test `package_qualified_path_without_inventory_is_ambiguous`: the same call with the inventory withheld is duplicate-ambiguous.
- [x] Keep every existing scenario test of `Occurrences of duplicated descriptors are never arbitrarily attributed` passing (crate-root package-name reference, scope locality, guarded join).
- [x] Update the test comments in `tests/code_graph.rs` that cite this requirement's scenarios to the renamed scenario titles.

## Currency (code-graph)

- [x] Record the target inventory and the digest of every workspace member's `Cargo.toml` in the store metadata, and compare them in `current_index_accounting`.
- [x] Integration test `renamed_target_rebuilds` (`A renamed binary rebuilds`): rename `tool` to `cli` in `Cargo.toml` only, rebuild, assert `rebuilt: true` and the marker `+bin.cli`.
- [x] Integration test: a rebuild with an unchanged manifest and inventory reports the index current (`Rebuilding an unchanged workspace analyzes nothing`, Rust and Python).

## Store (code-graph)

- [x] Bump `SCHEMA_VERSION` to 19 in `src/graph/schema.rs`.
- [x] Run the existing `Incompatible index stores are replaced or refused, never half-used` tests against a store written at version 18, and add a test that a version-18 Python store is replaced on build and refused on query.

## Resolution (code-navigation)

- [x] Integration tests in `tests/code_navigation.rs` over the Rust conformance fixture: `package_qualified_name_resolves_to_library_only` (`Package-qualified name resolves to the library's declaration`), `path_in_two_units_returns_candidates` (`Path declared in two units returns candidates`), `unit_marked_identity_round_trips` (`Identity with a marker round-trips`).
- [x] Integration test: `pkg.shapes.Widget` still resolves uniquely under the canonical form (`Python dotted qualified name resolves`).

## Reference identity lists (symbol-identity)

- [x] Write `tests/fixtures/python-conformance/IDENTITIES.txt` and `tests/fixtures/rust-conformance/IDENTITIES.txt` by hand from the spec's canonical form, one identity per line, sorted.
- [x] Integration test `scip_python_yields_python_reference_identities` (`scip-python yields the Python reference identities`): every reported module, class, function, method, and module global has exactly its listed identity, and the reference to `Widget` from `consumer.py` resolves to the listed `Widget`.
- [x] Integration test `rust_analyzer_yields_rust_reference_identities` (`rust-analyzer yields the Rust reference identities`): every reported declaration in the library, the binary, and the tests has exactly its listed identity, and `libbin::util::load()` resolves to the listed library `load`.
- [x] Integration tests for `Identity does not depend on source location` over copies of the Rust and Python conformance projects: `inline_module_moved_to_file_keeps_identity`, `module_file_moved_to_directory_module_keeps_identity`, `python_package_moved_between_source_roots_keeps_identity`, `module_rename_changes_identity`.
- [x] Integration tests for the remaining nesting scenarios over a copy of the Rust conformance project: `inline_modules_nest_without_directory` (`Rust inline modules nest without a directory`) and `path_attribute_module_keeps_declared_name` (`Rust module loaded through a path attribute keeps its declared name`).

## Fixtures, snapshots, and dogfood

- [x] Regenerate every test fixture and snapshot that holds a Python identity in the dotted form, a non-library Rust identity, or a cross-target twin suffix (`tests/code_graph.rs`, `tests/code_navigation.rs`, `mcp/tests/`, `tests/fixtures/surface_manifest.snapshot.json` if the build answer's new field is listed there).
- [x] Build `samples/rust-libbin` and a Flask clone copy with a binary built from this change; capture `get libbin::util::Config`, `trace --relation references` of the library's `load`, and `get flask.app.Flask`; record the outputs in this change directory.

## Documentation

- [x] Document the identity grammar `workspace::package[+marker]::segments`, the marker forms, and the Python and Rust mappings in `docs/commands/common.md` under the reference forms, and add a short identity section to the README.
- [x] Document the `units` disclosure and the target inventory as a currency input in `docs/commands/build.md`.
- [x] State the schema bump and the identity change in the implementation commit's `BREAKING CHANGE` footer; `just changelog` drafts the release section from it (`CHANGELOG.md` keeps no unreleased section, per `docs/releasing.md`).

## Review remediation

- [x] Python: a reference on a relative import statement takes that statement's target; other references in the document move only when the document's imports agree (`from .app import …` and `from ..app import …` in one file each resolve).
- [x] Python: a relative import whose dots climb past the top package resolves to nothing.
- [x] Python: a `from <dots> import name` whose name is not a workspace module records a failed lookup, so the workspace-wide fallback does not apply in that document.
- [x] Rust: unit assignment reads the `mod` declarations held on a duplicate module's group and matches each to the duplicate defined in the file it loads, so a file module shared by name between targets is reached from its target.
- [x] Rust: duplicate selection attributes a reference whose document belongs to exactly one target to the one duplicate belonging to that target.
- [x] Duplicate report: group duplicated rows by a stored backend-descriptor key, so duplicates in different targets are listed.
- [x] Resolution: a qualified name that matches an identity directly after the workspace returns only those matches.
- [x] Build: store the unit disclosure with the target inventory and return it from a build over an unchanged workspace.
- [x] Currency: a Rust store is current only when every manifest the inventory names has a recorded digest.

## Cleanup

- [x] Remove duplicated code in this change: one module-path check on `Descriptor`, one Python post-translation function shared by the adapter and the fixture helper, one `mod`-declaration lookup in the syntax tree, one unit map in the join; tighten the related docstrings and test comments.
- [x] Keep the unit marker off `Descriptor`: the projection input carries it.
- [x] Store each manifest digest inside the available target inventory.
- [x] Move the recorded build-target record into the store module, so the backend model imports nothing from the graph.
- [x] Give the build outcome named fields.
- [x] Pre-existing code, separate commit: merge the duplicated first step of the duplicate-selection walk; remove design-document references from join comments.
- [x] Name the part of a package a declaration belongs to a unit throughout code, docs, JSON, and the change's specs.

## Verification remediation

- [x] Test that the unit rule records a reference duplicate-ambiguous when the file's unit holds none of the duplicates (`A reference outside every duplicate's unit is ambiguous`).
- [x] Test that the unit rule records a reference duplicate-ambiguous when the file's unit holds two of the duplicates.
- [x] Test that the unit rule records a reference duplicate-ambiguous when the file is shared by two units.
- [x] Test that a reference by package name is recorded duplicate-ambiguous when the unit list is available and no duplicate is in the library (`Occurrences of duplicated descriptors are never arbitrarily attributed`).
- [x] Test through the build command that an unavailable target list reaches the build answer's `undetermined_units.packages` and the human warning (`Package whose units cannot be listed has no markers`).

## Checks

- [x] Run the full Rust test suite and the MCP test suite.
- [x] Run the pre-commit hooks on every changed and new file, and fix any failure.
