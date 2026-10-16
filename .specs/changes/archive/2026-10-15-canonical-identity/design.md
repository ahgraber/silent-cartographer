# Design: canonical-identity

The terms workspace, package, unit, kind, default unit, marker, module, declaration, document (a source file), qualified name, segment, descriptor, and disambiguator are defined in the symbol-identity spec of this change, under Terms.
A duplicate is one of two or more declarations that an indexer records with the same descriptor; the code calls duplicates "twins".

## Context

- c10r computes each canonical identity in `src/identity.rs`.
  Before this change the identity was `workspace::package::segments`, copied from the indexer's descriptor (`src/semantic/scip.rs`, `parse_scip_symbol`), plus a `#n` disambiguator when two identities were equal; the duplicates were numbered in order of their file path and position.
  The identity is the store's primary key (`symbols.canonical_id`), the key that matches the indexer's records to the syntax tree, and the string every answer returns.

- scip-python 0.6.6 writes a module's name as one segment holding the dotted name (`` `pkg.shapes`/ ``).
  It writes two symbols for each module: `` `pkg.shapes`/__init__: `` holds the module's definition and its import references, and `` `pkg.shapes`/ `` holds the `__name__` reference.
  It writes an annotated module global with its name twice (`` `flask.globals`/current_app.current_app. ``).
  These shapes were sampled on 2026-10-11 from `tests/fixtures/python-conformance/index.scip` and a Flask store (`._scratch/research/identity-forms/findings.md`).
  Three functions read them: `classify_module_kinds` in `src/semantic/python_adapter.rs`, and `module_dotted_name` and `module_identity_parts` in `src/graph/join.rs`.

- rust-analyzer writes one descriptor per module (`graph/store/GraphStore#`) under the Cargo package name, and does not say which unit a declaration belongs to.
  Every unit of a package (the library, binaries, integration tests, examples, benchmarks, and the build script) lands under the package name, so the same qualified name in two units gives one indexer symbol. c10r split such a symbol into duplicates numbered by definition location (`util::Config#0`, `#1`).
  The numbers depended on the names of other files: adding `src/bin/aaa.rs` renumbered the library's `load` from `#1` to `#2` (`._scratch/research/identity-forms/owner-review-fable.md`).

- When a reference names a duplicated declaration, a set of selection rules in the join picks which duplicate it means.
  One of them, the package-name rule (`LocalityRule::TargetMetadata`), picked the library's duplicate only when the reference's own token spelled the package name.
  The call `libbin::util::load()` in a binary has `load` as its token, so another rule attributed it to the binary's own `load` (`findings.md`).

- The Rust adapter already ran `cargo metadata --no-deps --format-version 1` within the time limit for external tools, and kept only each package's library root file (`library_roots`, `src/semantic/rust_adapter.rs`).
  Any failure gave an empty map.
  The join found which file declares each module file from its `mod` lines (`parent_document_map`, `src/graph/join.rs`), after identities were computed (`ingest_with_params` in `src/graph/mod.rs`).

- A malformed `Cargo.toml` or a missing `cargo` makes `rust-analyzer scip` fail before c10r needs the unit list (tested 2026-10-11 on `samples/rust-libbin`).
  So the unit list is missing only when `cargo metadata` exceeds the time limit, or when a Rust project without Cargo is built with `--language rust`.

- `tests/fixtures/python-conformance/` is a Python project with a committed scip-python index and `SCIP-PYTHON-VERSION`.
  Rust had only `tests/fixtures/exemplar.scip`, an index built by a test generator, and no conformance project.

- `ty-scip-backend` depends on this change.
  It adds ty-scip to the Python reference list check and carries the repeated-assignment scenario, which needs a second Python indexer to sample.

- The store schema version was 18 (`src/graph/schema.rs`).
  The code-graph requirement `Incompatible index stores are replaced or refused, never half-used` already governs a version change.

## Decisions

### Decision: SegmentsFollowDeclaredNesting

**Chosen:** after the package and unit, an identity has one segment for each nesting level the language places the declaration in, outermost first, then one segment for the declaration.
It adds no segment for a level the language does not have, and merges no two levels.
A module's identity is its qualified name alone.

**Rationale:** the identity is then the language's own qualified name, which a reader can type from the code.
It does not depend on how an indexer splits names or on how files are laid out.
Python packages nest (`import a.b` binds `a`), and Rust modules form a tree, so one segment per name component follows the language.

**Alternatives considered:**

- Split on the file path: wrong for Rust inline modules, `#[path]`, and Python namespace packages, where the file layout is not the module tree.
- Keep scip-python's form, with the dotted module name in one segment: ties every identity to one indexer's spelling, and keeps the `__init__` segment and the doubled names.

### Decision: ScipPythonMapsToCanonicalForm

**Chosen:** the Python adapter rewrites scip-python's descriptors after it reads the index and before it classifies modules:

1. A module segment that holds a dotted name is split on `.` into one module segment per component.
2. An `__init__` segment at the end, after the module segments, is dropped.
   The two symbols scip-python writes for one module then have the same descriptor, and they are merged into one symbol with all their references.
3. Two term segments with the same name, directly after the module segments, become one segment.

Class, method, parameter, and attribute segments are not changed.

**Rationale:** ty-scip already writes this form, so the rewrite belongs on the scip-python path, and the adapter is the only component that knows which indexer ran.

**Evidence for step 3 (2026-10-12):** the risk was a module global with an attribute of its own name (`x.x = 1`), which step 3 would merge wrongly.
The Flask store (4,616 symbols) and the httpx2 store (6,751 symbols), both built by scip-python 0.6.6, hold 53 identities whose first two segments after the module are equal.
52 are module globals (store kind `constant`), such as `flask.globals::current_app::current_app` and `sys::path::path`.
The other one, `select::poll::poll` (store kind `method`), is not two term segments, so step 3 does not apply to it.
No module global with an attribute of its own name was found, so step 3 uses only the segment kinds and names.

**Alternatives considered:**

- Rewrite during identity computation: that code would then depend on one indexer's spelling, which the backend boundary keeps out of the core.
- Keep the `__init__` symbol as a separate symbol: one module would have two identities, and one of them would be unreadable.

### Decision: ScipPythonRelativeImportsMapToTheirModule

**Context:** scip-python 0.6.6 records a relative import as an external module named by the text after the dots.
`from .globals import g` in `src/flask/__init__.py` refers to the module `flask::globals`, not `flask::flask::globals`.
One such external module can stand for several real modules: in Flask, `app` means `flask.app` when imported from `flask/` and `flask.sansio.app` when imported from `flask/sansio/`.
Its references can also sit in files with no relative import, when a package re-exports the name: `flask.json.dumps` in `tests/test_json.py` goes through `from . import json` in `flask/__init__.py`. ty-scip and rust-analyzer resolve relative imports to the real module (probed 2026-10-13).

**Chosen:** on the scip-python path, the Python adapter moves these references to the real module, as part of the same rewrite:

1. For each file, read its relative import statements (`from .x import …`, `from .. import a`).
   The file's own module and the number of dots give the absolute module path.
   A package's `__init__.py` is in that package; any other module is in its parent package; each dot after the first goes up one package.
   Dots that go above the top package give no path.
2. A reference inside an import statement moves to the workspace module at that statement's path.
   Another reference in the same file to an external module of the same name moves only when every statement in the file that names it gives the same path.
   An imported name with no workspace module at its path gives no path.
3. A reference in a file with no such statement moves only when every relative import of that name in the workspace gives the same workspace module.
4. A reference with no workspace module at its path, or one that rule 3 cannot settle, stays on the external module.

The adapter reads the import statements from the source files, because the index does not record the number of dots for every form (`from . import shapes` has its range on `shapes`).

**Evidence:** on Flask, all 26 relative-import mappings resolve to a workspace module (166 import references and 10 later uses of an imported name).
13 references to example and test apps (`from flaskr.db import get_db` in `examples/tutorial/tests/`) are absolute imports of apps outside the configured source roots; they stay external.

### Decision: ModuleClassificationStaysInThePythonAdapter

**Chosen:** `classify_module_kinds` treats a descriptor made only of module segments as a module, and only the Python adapter calls it.
The join's module-name rule builds a module's dotted name by joining its module segments with `.`.
The exact-name rule skips Python modules.
A module's last segment is now its last name component (`shapes`), so without the skip the exact-name rule would accept a bare `shapes` token first, and the reference would be stored under the wrong rule; the code-graph scenarios require the module-name rule.

**Rationale:** Rust module descriptors (`graph/store/`) are also made only of module segments.
Running the classification on Rust would change no kind today, because rust-analyzer already reports module kinds, but it would couple the two adapters without anyone seeing it.
A test over the Rust exemplar index checks that the Rust path does not run it.

### Decision: UnitGrammar

**Chosen:** an identity is written `workspace::package[+marker]::segments`.

- No marker: the package's default unit.
  In Rust, the library.
  In a language whose packages have one unit, the whole package.
- `+kind.name`: any other unit, with the kind and name the build tool gives it: `+bin.tool`, `+test.a`, `+example.x`, `+bench.x`, `+custom-build.build-script-build`.
- `+shared`: a declaration whose source is compiled into more than one unit of the package.

A unit's marker depends only on that unit's kind and name.
`+` separates the marker from the package name, because a Cargo package name cannot contain `+`: "The name must use only alphanumeric characters or `-` or `_`, and cannot be empty" (The Cargo Book, The Manifest Format, § The `name` field, <https://doc.rust-lang.org/cargo/reference/manifest.html>, read 2026-10-12).
`.` separates kind from name, because a Rust identifier cannot contain `.`.
A Python identity never has a marker, because a Python project has one unit.
Its package position holds the workspace name, which can contain `+` (a directory named `c++tools`).
That is not ambiguous: no Python identity has a marker, and lookup matches the whole stored identity without splitting it.
No package name is refused for containing `+`.

**Rationale:** a library identity is the Rust path (`libbin::util::Config`), so saved library identities stay valid and qualified-name lookup keeps working.
The library is the only unit other code can name, so the units that get a marker are exactly the ones with no Rust spelling to match.
Adding a unit never changes another unit's identities.
The default binary (`src/main.rs`) has the package's name, so the marker includes the kind: `libbin+bin.libbin` differs from the unmarked library.

**Alternatives considered:**

- Mark every unit (`libbin+lib::util::Config`): every Rust identity changes, and the Rust path no longer resolves unless lookup learns to skip the marker.
- Mark only when the package has several units: adding a binary would change every library identity.
- A marker without the kind (`+tool`): cannot tell the default binary from the library.

### Decision: RustUnitsFromTargetRootsAndModuleChain

**Chosen:** the Rust adapter keeps Cargo's full target list from `cargo metadata --no-deps` (package, kind, name, and the root file relative to the workspace) in place of the map of library root files. c10r finds each file's unit by following `mod` lines from each unit's root file.
A root file belongs to its unit, and a file that a `mod` line in one of the unit's files loads also belongs to that unit.
A file reached from exactly one unit gets that unit's marker; a file reached from two or more units gets `+shared`; a file reached from none gets no marker, and the build reports it (see UnitUnknownDegradesWithDisclosure).
This runs before identities are computed, so the `mod`-line evidence moves from the join into the store step (`ingest_with_params`), where the parsed syntax trees already exist.
When the library and a binary each have a module with the same name, such as `util`, rust-analyzer records one descriptor for both, and the index keeps their `mod util;` lines on the duplicate group. c10r matches each such line to the module whose file it loads: the file a `#[path]` attribute names, or else `util.rs` or `util/mod.rs` in the declaring file's module directory.
A line that matches no module, or more than one, is not used.
The library kinds are `lib`, `rlib`, `dylib`, `cdylib`, `staticlib`, and `proc-macro`; every other kind gets a marker.

**Rationale:** rust-analyzer does not report units, and a directory rule (`src/` is the library) is wrong for `mod cli;` in `src/main.rs` loading `src/cli.rs`, and for `#[path]`.
The `mod` lines are evidence c10r already reads.
`+shared` matches what the indexer reports (one definition for `tests/common/mod.rs`) and the store's rule of one definition per location; one identity per unit would store the same source text two or more times in passages, embeddings, and clone keys.

**Alternatives considered:**

- One identity per unit for a shared file: rejected above.
- List the units in the marker (`+test.a+test.b`): the identity would change whenever a test starts or stops using the helper.
- Use the first unit in path order: the identity would depend on the names of other files, the same defect as the old `#n` numbers.

### Decision: UnitInProjectionTwinsKeyedByBackendSymbol

**Chosen:** the unit marker is kept beside the descriptor in the input to identity computation (`ProjectionInput`), and the computation writes `package[+marker]::segments`.
`Descriptor` has no unit field, so a descriptor taken from the index cannot hold a marker.
`package` stays a separate field, because the join compares source tokens with it.
The index already splits one indexer symbol into one symbol per definition (`normalize`), so the store step computes one unit per symbol from the file of its definition.
Two definitions of one indexer symbol in different units then get different identities and no disambiguator.
The index's own descriptors have no unit, so duplicate groups (`DuplicateGroup`, `twin_symbols_by_descriptor`, the `duplicated` flag) are still keyed by the indexer's symbol, unchanged.
So the selection rules still choose among same-name definitions in different units, and the duplicate report still lists them.
The store step gives the join each symbol's unit, so the package-name rule can pick the library's duplicate.
When the defining-file and module-chain rules find no answer, a new rule picks the duplicate that belongs to the one unit that compiles the reference's file, if exactly one duplicate belongs to that unit (stored rule name `unit_membership`); the chosen reference still has to pass the usual alignment rules.
Each duplicated symbol stores the identity of its descriptor without a marker in `symbols.duplicate_key`, and the duplicate report groups by it, so duplicates in different units stay listed together.
In that unit list, a unit that could not be determined is kept apart from the library, although neither has a marker, so the package-name rule never picks an undetermined duplicate as the library's.

**Rationale:** the indexer's references carry no unit, so choosing a duplicate still needs the duplicate group; only the identity text changes.

**Alternatives considered:**

- The unit as an extra first segment: lookup's suffix match and the join's last-name check would treat it as a module.

### Decision: PackageQualifiedPathResolvesToLibrary

**Chosen:** the package-name rule fires when the reference's token spells the package name, as before, and also when the token is the last name of a path whose first name spells the package name.
The syntax tree supplies the path (`scoped_identifier` in tree-sitter-rust).
The rule picks the duplicate in the package's default unit.
Looking up a qualified name follows the same rule: when an identity is exactly the workspace followed by the name, only such identities match, so a binary's nested module with the same name cannot make a package-qualified name ambiguous.
When the unit list is unavailable, the reference is recorded duplicate-ambiguous, as before.
A reference through a `use` line (`use libbin::util::load; load()`) is still attributed by the file that holds it; this change does not fix that case.

**Rationale:** `libbin::util::load()` names the library by its spelling, whatever file it is in.
On the sample, the references of the library's `load` gained `src/bin/tool.rs:14`.
The store keeps the rule that chose each attribution, for diagnosis.
No answer shows it, so the spec does not promise it.

### Decision: TargetDescriptionIsACurrencyInput

**Chosen:** the check that decides whether a stored index is current (`current_index_accounting`) includes the Cargo target list the build used (each target's package, kind, name, and root file) and the manifests it came from (the `Cargo.toml` of every workspace member).
The store records the target list with a hash of each manifest in its metadata (`index_metadata.targets`), beside the analyzer details.
The check compares the manifest hashes and does not run `cargo metadata`: Cargo builds the target list from the manifests and the source files, and the check already compares the source files.
A store is current only when it holds a hash for the root `Cargo.toml` and for every manifest a target names.
The store also records the list of undetermined units, so a build that finds the index current reports the same undetermined units as the build that wrote it.
A store recorded without a target list is never current, so the next build runs `cargo metadata` again and adds the missing markers.

**Rationale:** c10r does not index `Cargo.toml`, so a target renamed only in the manifest would leave stale markers in an index that reads current.
Running `cargo metadata` in the check would add its run time to every build over an unchanged workspace, including the builds the refresh hooks start.

**Alternatives considered:**

- Index `Cargo.toml` as a source file: the manifest would become a document; recording the target list is the smaller change.
- Run `cargo metadata` in the check and compare target lists: the same answer at a higher cost per build.

### Decision: MigrationBySchemaVersion

**Chosen:** `SCHEMA_VERSION` goes from 18 to 19.
Every Python identity and every Rust identity outside the library changes, so `build` replaces a store built under 18, and every query refuses it, under the existing code-graph requirement.
Saved identities in the old form are not translated; they resolve to no symbol.

**Rationale:** the user's decision; the existing requirement already states the behavior.

### Decision: UnitUnknownDegradesWithDisclosure

**Chosen:** when the target list is unavailable, no declaration in the package gets a marker, and the build answer names the package and the reason.
When no unit's `mod` lines reach a file, the declarations in that file get no marker, and the build answer names the file and the reason.
The build answer always carries both lists, empty when every unit was determined, beside `changed_during_build`, which follows the same rule:

```json
"undetermined_units": {
  "packages": [
    {"package": "libbin", "reason": "metadata_timeout"}
  ],
  "documents": [
    {"document_path": "src/old.rs", "reason": "no_target_loads_it"}
  ]
}
```

The package reasons are `metadata_timeout` (`cargo metadata` did not finish within the time limit), `no_manifest` (no `Cargo.toml`; the build was forced with `--language rust`), and `metadata_failed`, which also carries `detail`, the first line of Cargo's error output.

The human output prints one warning per entry after the accounting line.
Each warning says what c10r could not do, why, what that means for answers, and what the user can do:

```text
warning: could not determine the Cargo targets of package libbin, because `cargo metadata` did not finish within 10 s.
  Code in its binaries, tests, and examples is labeled as library code, so a reference between same-named items in different targets may point to the wrong one.
  Run `c10r build` again; if this repeats, run `cargo metadata --no-deps` in the workspace to see what is slow.

warning: could not place src/old.rs in any Cargo target, because no `mod` declaration in a target loads it.
  Its code is labeled as library code. If the file is no longer used, deleting it removes this warning.
```

The `no_manifest` cause reads "because the workspace has no `Cargo.toml` (the build was forced with `--language rust`)", and the `metadata_failed` cause reads "because `cargo metadata` failed: <detail>".

**Rationale:** the user's decision.
These cases are rare: a malformed manifest or a missing `cargo` already makes rust-analyzer fail first (tested 2026-10-11).
Refusing the build would refuse only the timeout and the Rust-without-Cargo cases, where an index without markers, with a warning, is more useful than no index.

**Alternatives considered:**

- Refuse the build with a typed error: rejected by the user for the reason above.

### Decision: ReferenceIdentityListsPerConformanceProject

**Chosen:** each conformance project has a committed file `IDENTITIES.txt` that lists, one per line and sorted, the identity of each declaration the project defines.
The conformance test computes identities from each indexer's committed index, and checks that every reported declaration's identity is in the list and that the index reports every listed identity.
Python: `tests/fixtures/python-conformance/IDENTITIES.txt` against `index.scip` (scip-python).
Rust: a new project, `tests/fixtures/rust-conformance/`, with a library and a binary `tool` that both declare `mod util { pub struct Config; pub fn load() }`, the binary's `main` calling `util::load()` and `libbin::util::load()`, two integration tests sharing `tests/common/mod.rs`, a committed `index.scip` from `rust-analyzer scip`, a `RUST-ANALYZER-VERSION` file, and `IDENTITIES.txt`.

**Rationale:** the user requires evidence from each real indexer, not a constructed index.
When every indexer for a language matches one list, any two of them give the same identity, so the lists are the evidence for `Identity does not depend on the indexer`.
The spec states that outcome; the lists are test fixtures, so they are recorded here, not in the spec.
A later indexer, such as ty-scip, is held to the same list.
The Rust project also carries the unit scenarios, so one fixture serves both requirements.

### Decision: IncompleteNamesAreAFutureAdapterRule

**Chosen:** no requirement.
Guidance for a future adapter: when an index leaves out enclosing name levels (scip-dotnet 0.2.14 writes `Shapes/Circle#` for `Acme.Geometry.Shapes.Circle`), the adapter must get the full name from another source, and must not give a declaration an identity built from the partial name.
Whether the build then refuses or reports a typed absence is decided when the first such indexer is added.

### Decision: RequirementsStatedAsOutcomes

**Chosen:** on 2026-10-12 the delta requirements were restated as outcomes a user can observe (commit `ce85b81`), and four promises were removed:

1. Code-graph no longer promises that each attribution carries the rule that chose it.
   The store keeps the rule in a column for diagnosis, but no command shows it.
2. Symbol-identity no longer promises that a disambiguator is anchored to the definition location.
   The requirement keeps the outcome a user depends on: the same identity on every run over unchanged sources, in any discovery order.
3. `Identity does not depend on the indexer` no longer names the reference identity lists.
   The lists are test fixtures, recorded in ReferenceIdentityListsPerConformanceProject.
4. The scenarios `scip-python yields the Python reference identities`, `rust-analyzer yields the Rust reference identities`, and `Source compiled into two units has one shared identity` no longer check that references resolve.
   Reference attribution is the subject of the code-graph scenarios.

**Rationale:** a requirement states what a user can observe.
None of the four removals changes an answer.
For item 2, location anchoring already gave no stability across file additions: duplicate numbers shifted when a sibling file was added.

**Alternatives considered:**

- Keep the provenance promise: it would commit c10r to a store column that no answer exposes.

## Language mappings

### Python (decision)

Package: the project name the indexer records as the package.
Unit: the project has one unit, so an identity never has a marker.
Segments: one module segment per component of the module's import name, then the declaration's segments in the language's nesting (class, then method or attribute; function; module variable).
The scip-python rewrite is in ScipPythonMapsToCanonicalForm.
Declarations in a package initializer (`pkg/__init__.py`) belong to `pkg`; the conformance project has a class there so this case is tested.

### Rust (decision)

Package: the Cargo package name.
Unit: as in UnitGrammar and RustUnitsFromTargetRootsAndModuleChain.
Segments: one module segment per module below the unit's root module, then the item, then any member.
The root module is not a segment of the identities below it; the package and marker stand where the crate name stands in a Rust path. rust-analyzer reports each unit's root module as a module named `crate`, so the root module's own identity is `libbin::crate` (or `libbin+bin.tool::crate`), as before this change.
The spec does not name the root module's own identity.
`crate` is Rust's keyword for the root module, not a tool marker, so the identity keeps it (decided with the user, 2026-10-13). rust-analyzer gives inline modules, `mod x;`, `x/mod.rs`, and `#[path]` modules their module names, so the segments need no rewrite.

### Go (guidance for a future adapter)

The nesting level is the package, named by its complete import path in one segment; packages do not nest.
The short name in the `package` clause is not used.
A major-version suffix stays in the segment.
Package: the module.
Unit: the import path already tells packages apart, so no marker.
An external test package (`foo_test`) has its own import path.

### Java (guidance for a future adapter)

The nesting level is the package, named by its complete name in one segment; a package and its subpackages have no membership relation (JLS §7.1).
Types nest inside the package segment, and nested types inside their enclosing type.
Package: the Maven or Gradle module.
Unit: the `main` source set is the default unit, and other source sets get a marker with their kind and name. scip-java writes one segment per package component and emits every prefix as a symbol; the adapter joins the components and gives no identity to a prefix that is not a package of the project.

### TypeScript (guidance for a future adapter)

The nesting level is the module: a file module by its resolved module path in one segment, with declared namespaces nested inside; an ambient module by its declared string name; global declarations and augmentations belong to the scope they declare or augment.
Package: the npm package.
Unit: tsconfig projects do not get markers, because a declaration's qualified name already starts with its module path.
Definitions at the same path and range (one file in two projects) are one declaration.
An ambient module name can contain `::`, so the identity text needs an escaping rule before TypeScript is supported.

### C# (guidance for a future adapter)

The nesting level is the namespace, split per level of the fully qualified name whatever the syntax; types nest inside; a partial type is one declaration with several locations.
Package: the project (one assembly).
Unit: target frameworks are build configurations, not units; shared projects and linked files get `+shared`. scip-dotnet 0.2.14 leaves out outer namespaces; see IncompleteNamesAreAFutureAdapterRule.

## Architecture

```text
rust-analyzer scip ──▶ translate_index ──▶ unit assignment ──▶ normalize ──▶ project_identities
cargo metadata ──▶ target list ───────┘   (file → unit via      (duplicates keyed   workspace::package[+marker]::segments
                    (kind, name, root)      mod lines; +shared;   by the indexer's
                                            unknown → no marker   symbol)
                                            + warning)

scip-python ──▶ translate_index ──▶ scip-python rewrite ──▶ classify_module_kinds ──▶ normalize ──▶ project_identities
                                    (split dotted module,     (module-only descriptor)
                                     drop __init__ and merge,
                                     merge doubled name,
                                     move relative imports)

join: the package-name rule fires on `libbin` tokens and on the last name of `libbin::…` paths → the library's duplicate
currency: sources + target list + manifest hashes + analyzer + environment + chunk parameters
store: SCHEMA_VERSION 19; the build answer carries "undetermined_units"
```

## Risks

- **Rust identities outside the library change.**
  Every identity in binaries, tests, examples, benchmarks, and build scripts changes once, and a package with only binaries changes every identity.
  Mitigation: library identities are byte-identical to before; the schema change forces one rebuild; the README describes the new form.
- **Duplicate numbers.**
  Duplicates in different units lose their `#n` suffix, so a saved `util::load#1` no longer resolves.
  Those suffixes already changed when files were added, and the rebuild is forced.
  Duplicates in one unit keep their numbers.
- **The `+` separator.**
  If a Cargo package name could contain `+`, a Rust marker would be ambiguous.
  Mitigation: the Cargo naming rule, cited in UnitGrammar, allows only alphanumerics, `-`, and `_`.
  A Python identity has no marker, so a `+` in its workspace-derived package position is not ambiguous; no name is refused.
- **Merging a doubled name wrongly.**
  A module global with an attribute of its own name could be merged wrongly.
  Mitigation: the evidence in ScipPythonMapsToCanonicalForm found no such case before the rule was written.
- **`cargo metadata` timeout.**
  A slow `cargo metadata` gives an index without markers and a warning, not a refusal.
  Mitigation: the warning is in the JSON and the human output, and the next build with a responsive `cargo metadata` sees that the store has no target list and rebuilds.

## Spec Changes

- 2026-10-14, user decision: the symbol-identity delta begins with a Terms section that defines workspace, package, unit, kind, default unit, marker, module, declaration, document, qualified name, segment, descriptor, and disambiguator.
  Sync places it in the baseline symbol-identity spec between Purpose and Requirements.
- 2026-10-14, user decision: the identity form is written `workspace::package[+marker]::segments`, because the bracketed part is the marker text, and a declaration in the default unit has a unit but no marker.
- 2026-10-12 and 2026-10-14, user decisions, wording only: these scenarios were renamed so their titles use plain words and the defined terms.
  Each line gives the title in the proposal commit, then the current title.
  The code-graph renames of baseline scenarios carry a `modified-removes` marker, because the sync check compares titles and cannot tell a rename from a deletion.
  - code-graph: "Definition occurrences attach to their own duplicate" is now "Two duplicates keep separate definition locations".
  - code-graph: "Reference in one duplicate's territory is attributed to it" is now "A reference in one duplicate's unit is attributed to it".
  - code-graph: "Locality does not bypass the guarded join" is now "A reference in one duplicate's unit is not attributed if no alignment rule accepts it".
  - code-graph: "Package-name reference resolves to the library target" is now "A reference by package name is attributed to the library".
  - code-graph: "Package-qualified path resolves to the library's declaration" is now "A path that starts with the package name is attributed to the library".
  - code-graph: "Package-name reference without target metadata is typed ambiguous" is now "A reference by package name with no unit list is ambiguous".
  - code-graph: "Reference outside every duplicate's territory is typed ambiguous" is now "A reference outside every duplicate's unit is ambiguous".
  - code-graph: "Same-document reference inside exactly one twin's scope is attributed to it" is now "A reference in a declaration that holds exactly one duplicate is attributed to it".
  - code-graph: "Same-document reference whose deciding scope holds several twins stays ambiguous" is now "A reference in a declaration that holds several duplicates is ambiguous".
  - code-graph: "Same-document reference enclosed by no twin-bearing declaration stays ambiguous" is now "A reference outside every declaration that holds a duplicate is ambiguous".
  - code-graph: "Scope locality does not bypass the guarded join" is now "A reference in a declaration that holds one duplicate is not attributed if no alignment rule accepts it".
  - code-graph: "Unduplicated descriptors are unaffected" is now "A descriptor with one definition has no ambiguous references".
  - code-graph: "A renamed build target rebuilds" is now "A renamed binary rebuilds".
  - symbol-identity: "True duplicates keep definition-anchored identities" is now "True duplicates receive distinct identities"; its outcome change is in Decision RequirementsStatedAsOutcomes.
  - symbol-identity: "Python module and package carry no marker" is now "Python module and package have no `__init__` segment".
  - symbol-identity: "Adding a target changes no other unit's identities" is now "Adding a binary changes no other unit's identities".
  - symbol-identity: "Python owner is the project with no marker" is now "Python identity names the project with no marker".
  - symbol-identity: "Unavailable target description degrades to package-only owners with disclosure" is now "Package whose units cannot be listed has no markers".
  - symbol-identity: "Document reached from no target root degrades to the package-only owner with disclosure" is now "File that no unit loads has no marker".
  - code-navigation: "Owner-marked identity round-trips" is now "Identity with a marker round-trips".
