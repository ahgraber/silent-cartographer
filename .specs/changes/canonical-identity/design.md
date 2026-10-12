# Design: canonical-identity

## Context

- A canonical identity is projected in `src/identity.rs` as `workspace :: package :: segments`, with a `#<rank>` suffix for colliding projections ranked by definition document path and range.
  The descriptor's package and segments are read verbatim from the SCIP symbol (`src/semantic/scip.rs`, `parse_scip_symbol`).
  Identities are the store's primary key (`symbols.canonical_id`), the join key, and the string every answer returns.

- scip-python 0.6.6 writes a module as one namespace segment holding the dotted name (`` `pkg.shapes`/ ``) and writes two symbols per module: `` `pkg.shapes`/__init__: `` carries the module's definition and its import references, and `` `pkg.shapes`/ `` carries the `__name__` reference.
  An annotated module global is written with its name twice (`` `flask.globals`/current_app.current_app. ``).
  Sampled 2026-10-11 from `tests/fixtures/python-conformance/index.scip` and a Flask store (`._scratch/research/identity-forms/findings.md`).
  Three places read these shapes today: `classify_module_kinds` in `src/semantic/python_adapter.rs` (the `__init__` meta terminal), and `module_dotted_name` and `module_identity_parts` in `src/graph/join.rs`.

- rust-analyzer writes one namespace descriptor per module (`graph/store/GraphStore#`) under the Cargo package name, and names no crate target.
  Every target of a package (library, binaries, integration tests, examples, benches, build script) lands under the package name, so same-path declarations in two targets share one SCIP symbol. c10r splits them into definition-anchored twins (`util::Config#0`, `#1`).
  The twin numbers depend on sibling file names: adding `src/bin/aaa.rs` renumbered the library's `load` from `#1` to `#2` (`._scratch/research/identity-forms/owner-review-fable.md`).

- The join's package-name rule (`src/graph/join.rs`, `LocalityRule::TargetMetadata`) attributes an occurrence to the library twin only when the occurrence's own token spells the package name.
  The call `libbin::util::load()` in a binary has `load` as its token, so it falls to defining-document locality and is attributed to the binary's own `load` (`findings.md`).

- The Rust adapter already runs `cargo metadata --no-deps --format-version 1` under the probe deadline and keeps only the library root per package (`library_roots`, `src/semantic/rust_adapter.rs`).
  Any failure degrades to an empty map.
  The join derives which document declares each module file from `mod` items (`parent_document_map`, `src/graph/join.rs`), after identities are projected (`src/graph/mod.rs`, `ingest_with_params`).

- A malformed `Cargo.toml` or a missing `cargo` makes `rust-analyzer scip` fail before c10r's target lookup matters (tested 2026-10-11 on `samples/rust-libbin`).
  The owner-unknown path is reached only when `cargo metadata` exceeds the probe deadline, or when a non-Cargo Rust project is built with `--language rust`.

- Conformance fixtures: `tests/fixtures/python-conformance/` is a Python project with a committed scip-python index and `SCIP-PYTHON-VERSION`.
  Rust has only `tests/fixtures/exemplar.scip`, an index constructed by a test generator, and no conformance project.

- `ty-scip-backend` (worktree `.worktrees/ty-scip-backend`) is paused and depends on this change.
  It currently rewrites ty-scip descriptors to scip-python's form; after this change it removes that rewrite, adds ty-scip against the Python reference list, and carries the repeated-assignment boundary scenario, which needs a second Python backend to sample.

- Store schema version is 18 (`src/graph/schema.rs`).
  The code-graph requirement `Incompatible index stores are replaced or refused, never half-used` already governs a version mismatch.

## Decisions

### Decision: SegmentsFollowDeclaredNesting

**Chosen:** an identity's segments after the owner are one segment per namespace unit the language places the declaration in, outermost first, then one segment for the declaration itself.
No segment is added for a nesting level the language lacks, and no two levels are joined.
A module or package is its segments alone, with no marker segment.

**Rationale:** the identity then equals the language's own qualified name, which a reader can type from the code in front of them, and it does not depend on the indexer's choice of segment boundaries or on file layout.
Python packages nest (`import a.b` binds `a`), and Rust modules form a tree, so one segment per component is the language's own structure.

**Alternatives considered:**

- Physical path structure (split on the path separator everywhere): breaks for Rust inline modules, `#[path]`, and Python namespace packages, where the file layout is not the module tree.
- The incumbent indexer's form as canonical (scip-python's dotted segment): ties the public identity to one tool's spelling and keeps the `__init__` marker and the doubled names.

### Decision: ScipPythonMapsToCanonicalForm

**Chosen:** the Python adapter rewrites scip-python descriptors after translation and before module classification:

1. A module namespace segment holding a dotted name splits on `.` into one module segment per component.
2. A trailing `__init__` meta segment after the module run is dropped.
   The two scip-python module symbols then share one descriptor, and their occurrences merge into one symbol before `normalize`.
3. Two consecutive term segments with equal names directly after the module run collapse into one.

Everything else (class, method, parameter, attribute segments) is unchanged.

**Rationale:** ty-scip's sampled form is already canonical, so the conversion belongs on the scip-python path, and the adapter is the only component that knows which indexer ran.

**Open check before implementing step 3:** confirm against a Flask store that scip-python never writes two consecutive equal term segments for a module global that legitimately has an attribute of its own name (for example `x.x = 1` at module level).
If it does, the collapse must use the definition role or the occurrence range to tell the cases apart.
Record the result in this section.

**Alternatives considered:**

- Rewrite in identity projection: identity projection would then know indexer-specific forms, which the backend contract exists to keep out of the core.
- Keep the `__init__` symbol separate as a distinct "module marker" symbol: two identities for one module, one of them unreadable.

### Decision: ModuleClassificationStaysInThePythonAdapter

**Chosen:** `classify_module_kinds` recognizes a module as a descriptor made only of module segments, and it runs only inside the Python adapter.
The join's module-name rule reads a module's dotted name as its module segments joined with `.`.

**Rationale:** Rust module descriptors (`graph/store/`) are also module-only.
A shared classification would reclassify nothing wrongly today, because rust-analyzer already reports module kinds, but it would become a hidden coupling.
A test over the Rust exemplar index guards it.

### Decision: OwnerGrammar

**Chosen:** the identity text is `workspace::package[+unit]::segments`.

- No `+unit`: the package's default unit (Rust: the library; a language with no units below the package: the whole package).
- `+kind.name`: the one other unit a declaration belongs to, with kind and name as the build tool declares them (Rust: `+bin.tool`, `+test.a`, `+example.x`, `+bench.x`, `+custom-build.build-script-build`).
- `+shared`: a declaration whose source is compiled into more than one unit of the package.

Whether a unit carries a marker, and its text, depends only on that unit's kind and name.
`+` separates the marker from the package name because a Cargo package name cannot contain `+`, and `.` separates kind from name because a Rust identifier cannot contain `.`; a task verifies the Cargo rule.
A Python identity never carries a unit marker, because Python has no units below the project.
Its package position holds the workspace identity, which can contain `+` (a root directory named `c++tools`); this is not ambiguous, because no Python identity has a marker and identity lookup matches the stored string exactly without splitting it.
No package name is refused for containing `+`.

**Rationale:** library identities equal the Rust path (`libbin::util::Config`), so every saved library identity stays valid and qualified-name lookup keeps working.
The library is the only target other code can name, so the marked targets are exactly the ones with no native spelling to match.
Adding a target never changes another target's identities.
The default binary (`src/main.rs`) has the package's name, so the marker carries the kind: `libbin+bin.libbin` is distinct from the unmarked library.

**Alternatives considered:**

- Mark every unit (`libbin+lib::util::Config`): every Rust identity changes once, and the native path no longer resolves without teaching `resolve` to skip the marker.
- Mark only when the package has several targets: adding a binary would change every library identity.
- A marker without the kind (`+tool`): cannot distinguish the default binary from the library.

### Decision: RustOwnersFromTargetRootsAndModuleChain

**Chosen:** the Rust adapter keeps the full target inventory from `cargo metadata --no-deps` (package, kind, name, workspace-relative root file) in place of the library-root map.
Each document is assigned to a target by following `mod` declarations from each target root: a root document belongs to its target; a document declared by a `mod` item in a document of target T belongs to T. A document reached from exactly one target gets that target's owner; a document reached from more than one target gets `+shared`; a document reached from none gets the package-only owner with a disclosure (see OwnerUnknownDegradesWithDisclosure).
The derivation runs before identity projection, so the `mod` chain evidence moves from the join into ingest, where the prepared syntax trees are already available.
Library kinds are `lib`, `rlib`, `dylib`, `cdylib`, `staticlib`, and `proc-macro`; every other kind is marked.

**Rationale:** rust-analyzer emits no target, and a directory convention (`src/` is the library) is wrong for `mod cli;` in `src/main.rs` loading `src/cli.rs` and for `#[path]`.
The `mod` chain is the evidence c10r already extracts.
`+shared` matches the indexer's output (one definition for `tests/common/mod.rs`) and the store's one-definition-per-span assumption; one identity per loading crate would duplicate passages, embeddings, and clone keys for one source text.

**Alternatives considered:**

- One identity per loading target for a shared file: rejected above.
- Listing the loading targets in the marker (`+test.a+test.b`): changes whenever a test starts or stops using the helper.
- The first loading target by path order: identity would depend on sibling file names, the same defect as twin numbers.

### Decision: OwnerInDescriptorTwinsKeyedByBackendSymbol

**Chosen:** `Descriptor` gains an owner marker alongside `package`, and `projection_body` writes `package[+marker]::segments`.
`package` stays a separate field because the join compares source tokens against it.
The owner is set per definition occurrence from the definition's document, before `normalize`, so two definitions of one SCIP symbol in different targets become distinct descriptors with distinct base projections and no `#` suffix.
Twin groups (`DuplicateGroup`, `twin_symbols_by_descriptor`, the `duplicated` flag) are keyed by the backend's symbol, that is the descriptor with the owner stripped, so the locality rules still attribute references among same-path definitions across targets, and the duplicated-descriptor disclosure still lists them.

**Rationale:** the reference occurrences carry no owner from the indexer, so attribution still needs the group; only the identity changes.

**Alternatives considered:**

- Owner as an extra leading segment: `resolve`'s suffix match and the join's terminal-name guard would treat it as a module.

### Decision: PackageQualifiedPathResolvesToLibrary

**Chosen:** the package-name rule in the join fires when the occurrence's token spells the package name (today) or when the token is the final component of a qualified path whose first component spells the package name.
The syntax tree supplies the enclosing path node (`scoped_identifier` in tree-sitter-rust).
The selected twin is the one whose owner is the package's default unit.
When the target inventory is unavailable, the occurrence stays duplicate-ambiguous, as today.
A reference through a `use` import (`use libbin::util::load; load()`) keeps its containing document's locality; this is a known residual, recorded here and not fixed in this change.

**Rationale:** `libbin::util::load()` names the library by its spelling, regardless of the document it sits in.
Verified on the sample by the `references` of the library's `load` gaining `src/bin/tool.rs:14`.
The store keeps the locality rule that selected each attribution, for diagnosis.
No answer shows it, so the spec does not promise it.

### Decision: TargetDescriptionIsACurrencyInput

**Chosen:** the currency check (`current_index_accounting`) includes the target inventory the build used (each target's package, kind, name, and root) and the manifest files that produced it (`Cargo.toml` of every workspace member).
The inventory is recorded in the store's metadata beside the analyzer provenance.

**Rationale:** Rust source collection omits `Cargo.toml`, so a target renamed in the manifest alone would leave stale owners in an index that reads current.

**Alternatives considered:**

- Collect `Cargo.toml` as a source: the manifest would then be indexed as a document; recording the inventory is the smaller change.

### Decision: MigrationBySchemaVersion

**Chosen:** `SCHEMA_VERSION` goes from 18 to 19.
Every Python identity and every non-library Rust identity changes, so a store built under 18 is replaced by `build` and refused by every query under the existing code-graph requirement.
No translation of saved identities is offered; a saved identity in the prior form resolves to no symbol.

**Rationale:** the user's decision; the existing requirement already states the behavior.

### Decision: OwnerUnknownDegradesWithDisclosure

**Chosen:** when the target inventory is unavailable, every declaration of the package carries the package-only owner, and the build answer names the package and the reason.
When a document is reached from no target root, its declarations carry the package-only owner, and the build answer names the document and the reason.
The build answer carries both lists, always present and empty when every owner was determined, beside `changed_during_build`, which follows the same convention:

```json
"owners": {
  "undetermined_packages": [
    {"package": "libbin", "reason": "metadata_timeout"}
  ],
  "documents_without_unit": [
    {"document_path": "src/old.rs", "reason": "no_target_loads_it"}
  ]
}
```

Package reasons: `metadata_timeout` (`cargo metadata` did not finish within the probe deadline), `no_manifest` (no `Cargo.toml`; the build was forced with `--language rust`), and `metadata_failed`, which also carries `detail`, the first line of Cargo's error output.

The human render prints one warning per entry after the accounting line.
Each warning states, in the active voice, what c10r could not do, why, what it means for answers, and what the user can do:

```text
warning: could not determine the crate targets of package libbin, because `cargo metadata` did not finish within 10 s.
  Code in its binaries, tests, and examples is labeled as library code, so a reference between same-named items in different targets may point to the wrong one.
  Run `c10r build` again; if this repeats, run `cargo metadata --no-deps` in the workspace to see what is slow.

warning: could not place src/old.rs in any crate target, because no `mod` declaration in a target loads it.
  Its code is labeled as library code. If the file is no longer used, deleting it removes this warning.
```

The `no_manifest` cause reads "because the workspace has no `Cargo.toml` (the build was forced with `--language rust`)", and `metadata_failed` reads "because `cargo metadata` failed: <detail>".

**Rationale:** the user's decision.
The path is rare: a malformed manifest or a missing `cargo` already fails rust-analyzer before c10r's target lookup (tested 2026-10-11), so refusing the build here would refuse only the timeout and non-Cargo cases, where a disclosed package-only owner is more useful than no index.

**Alternatives considered:**

- Refuse the build with a typed error: rejected by the user for the reason above.

### Decision: ReferenceIdentityListsPerConformanceProject

**Chosen:** each conformance project carries a committed text file `IDENTITIES.txt`, one canonical identity per line, sorted, for the declarations the project defines.
The conformance test projects identities from each backend's committed index and asserts that every reported declaration's identity is in the list and that no listed identity is missing from the index of a backend that reports that declaration.
Python: `tests/fixtures/python-conformance/IDENTITIES.txt` against `index.scip` (scip-python).
Rust: a new `tests/fixtures/rust-conformance/` project with a library and a binary `tool` that both declare `mod util { pub struct Config; pub fn load() }`, the binary's `main` calling `util::load()` and `libbin::util::load()`, two integration tests sharing `tests/common/mod.rs`, a committed `index.scip` from `rust-analyzer scip`, a `RUST-ANALYZER-VERSION` file, and `IDENTITIES.txt`.

**Rationale:** the user requires per-backend evidence, not a constructed imitation index.
When every backend for a language matches one list, any two of them give the same identity, so the lists are the evidence for `Identity does not depend on the indexer`.
The spec states that outcome; the lists are test fixtures, so they are recorded here and not in the spec.
The list is the contract a later backend (ty-scip) is held to.
The Rust project also carries the owner scenarios, so one fixture serves both requirements.

### Decision: IncompleteNamesAreAFutureAdapterRule

**Chosen:** no requirement.
Guidance for a future adapter: when an index omits enclosing name levels (scip-dotnet 0.2.14 writes `Shapes/Circle#` for `Acme.Geometry.Shapes.Circle`), the adapter must obtain the full name from another source; it must not assign an identity derived from the partial name.
Whether the build then refuses or discloses a typed absence is decided when the first such indexer is adopted.

## Language mappings

### Python (decision)

Owner: the project name the indexer records as the package; no units, so never a marker.
Segments: one module segment per component of the module's qualified import name, then the declaration segments as the language nests them (class, then method or attribute; function; module variable). scip-python mapping: see ScipPythonMapsToCanonicalForm.
Declarations in a package initializer (`pkg/__init__.py`) are members of `pkg`; the conformance project gains a class there so the shape is sampled.

### Rust (decision)

Owner: the Cargo package name, plus a unit marker per OwnerGrammar and RustOwnersFromTargetRootsAndModuleChain.
Segments: one module segment per module from the crate root, then the item, then any member.
The crate root is not a segment; the owner stands where the crate name stands in a Rust path. rust-analyzer resolves inline modules, `mod x;`, `x/mod.rs`, and `#[path]` modules to module names, so no rewrite is needed for the segments.

### Go (guidance for a future adapter)

The namespace unit is the package, identified by its complete import path, one segment; packages do not nest.
The short name in the `package` clause is not the unit.
A major-version suffix stays in the segment.
Owner: the module; the import path already distinguishes packages, so no marker.
An external test package (`foo_test`) is a distinct import path.

### Java (guidance for a future adapter)

The namespace unit is the package by its complete name, one segment; a package and its subpackages have no membership relation (JLS §7.1).
Types nest inside the package segment, nested types inside their enclosing type.
Owner: the Maven or Gradle module; the `main` source set is the default unit, other source sets are marked by kind and name. scip-java writes one segment per package component and emits every prefix as a symbol; the adapter joins the components and assigns no identity to a prefix that is not a package of the project.

### TypeScript (guidance for a future adapter)

The namespace unit is the module: a file module by its resolved module path, one segment, with declared namespaces nested inside; an ambient module by its declared string name; global declarations and augmentations belong to the scope they declare or augment.
Owner: the npm package; tsconfig projects are not identity-bearing, because a declaration's qualified name already begins with its module path.
Definition occurrences identical in path and range (one file in two projects) are one declaration.
An ambient module name can contain `::`, so the identity encoding needs a reversible-boundary rule before TypeScript is supported.

### C# (guidance for a future adapter)

The namespace unit is the namespace, split per level of the fully qualified name regardless of syntax; types nest inside; a partial type is one declaration with several locations.
Owner: the project (one assembly); target frameworks are configurations, not owners; shared projects and linked files take `+shared`. scip-dotnet 0.2.14 omits outer namespaces; see IncompleteNamesAreAFutureAdapterRule.

## Architecture

```text
rust-analyzer scip ──▶ translate_index ──▶ owner assignment ──▶ normalize ──▶ project_identities
cargo metadata ──▶ target inventory ──┘   (document → target     (twins keyed      workspace::package[+unit]::segments
                    (kind, name, root)      via mod chain;        by backend
                                            +shared; unknown →    symbol)
                                            package-only +
                                            disclosure)

scip-python ──▶ translate_index ──▶ canonical rewrite ──▶ classify_module_kinds ──▶ normalize ──▶ project_identities
                                    (split dotted module,   (module-only descriptor)
                                     drop __init__ and merge,
                                     collapse doubled term)

join: package-name rule fires on `libbin` tokens and on the final token of `libbin::…` paths → default-unit twin
currency: sources + target inventory + manifests + analyzer + environment + chunk parameters
store: SCHEMA_VERSION 19; build answer carries "owners" disclosures
```

## Risks

- **Rust identity churn for non-library targets.**
  Every identity in binaries, tests, examples, benches, and build scripts changes once, and a binary-only package changes every identity.
  Mitigation: library identities are byte-identical to today; the schema bump forces one rebuild; the README names the new grammar.
- **Twin numbering.**
  Cross-target twins lose their `#n` suffix, so a saved `util::load#1` no longer resolves.
  Those suffixes were already unstable under layout changes, and the rebuild is forced.
  Same-unit twins keep their numbering.
- **The `+` separator.**
  If a Cargo package name could contain `+`, a Rust unit marker would be ambiguous.
  Mitigation: a setup task verifies the Cargo naming rule (package names are restricted to alphanumerics, `-`, and `_`).
  A Python identity carries no marker, so a `+` in its workspace-derived package position is not ambiguous; nothing is refused.
- **Doubled-name collapse false positive.**
  A module global with an attribute of its own name could be collapsed wrongly.
  Mitigation: the open check in ScipPythonMapsToCanonicalForm before the rule is written.
- **`cargo metadata` timeout.**
  A slow metadata run yields an index with package-only owners and a disclosure, not a refusal.
  Mitigation: the disclosure is in the answer and the human render; the next build with a responsive `cargo metadata` detects the changed target inventory and rebuilds.
