# Delta for Symbol Identity

## MODIFIED Requirements

### Requirement: Deterministic canonical identity

> Previously: the identity was a pure function of the symbol's resolved semantic descriptor.

The system SHALL assign each indexed symbol a canonical identity that is a pure function of the workspace identity, the symbol's owner, and the symbol's qualified name as the language declares it, such that re-indexing unchanged sources yields the identical identity for every symbol.

Serves: saved-identity-stays-valid

#### Scenario: Stable across re-index

- **GIVEN** a Rust source tree that has been indexed once
- **WHEN** the same tree is indexed again with no source changes
- **THEN** every symbol's canonical identity is byte-for-byte identical to its prior identity

#### Scenario: Independent of discovery order

- **GIVEN** two indexing runs over the same sources that visit files in different orders
- **WHEN** the canonical identities are compared
- **THEN** each symbol resolves to the same identity regardless of the order in which it was encountered

### Requirement: Identity uniqueness within a workspace

> Previously: a disambiguator was introduced wherever the resolved descriptor collided, so same-descriptor declarations in two units of one package received numbered twin identities.

The system SHALL guarantee that within one workspace no two distinct symbols share a canonical identity.
Symbols with different owners SHALL differ by owner alone and carry no disambiguator.
Only distinct symbols with the same owner and the same qualified name SHALL carry a disambiguator.
A disambiguated identity SHALL stay the same on every run over unchanged sources, whatever order the files and symbols are read in.

Serves: saved-identity-stays-valid

#### Scenario: Distinct symbols never collide

- **GIVEN** a workspace whose sources contain many distinct symbols
- **WHEN** canonical identities are assigned
- **THEN** no two distinct symbols are assigned the same identity

#### Scenario: Disambiguation is detected, not assumed

- **GIVEN** a language whose descriptors are already unique without a parameter-based disambiguator
- **WHEN** identities are assigned for that language
- **THEN** no disambiguator suffix is added, and uniqueness still holds

#### Scenario: True duplicates receive distinct identities

- **GIVEN** two distinct definitions with one owner that the semantic backend describes with an identical resolved descriptor
- **WHEN** canonical identities are assigned
- **THEN** each definition receives a distinct identity

#### Scenario: Duplicate identities stable across discovery order

- **GIVEN** the same sources indexed twice with files visited in different orders
- **WHEN** the identities of the duplicated definitions are compared across the two runs
- **THEN** each definition carries the same identity in both runs

#### Scenario: Same descriptor in two units carries no disambiguator

- **GIVEN** a Cargo package `libbin` whose library and binary `tool` each declare `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the library's `Config` and the binary's `Config` have distinct identities that differ in their owner, and neither carries a disambiguator suffix

## ADDED Requirements

### Requirement: Segments follow the language's declared nesting

After the workspace and owner, a canonical identity SHALL consist of one segment per namespace unit the language places the declaration in, from the outermost unit inward, followed by one segment for the declaration itself.
The identity SHALL NOT contain a segment for a nesting level the language does not define, and SHALL NOT join two nesting levels the language defines into one segment.
A module or package SHALL be identified by the segments of its qualified name alone, with no segment that marks it as a module.

Serves: identity-reads-as-qualified-name, saved-identity-stays-valid

#### Scenario: Python module path splits per component

- **GIVEN** a Python project with the class `Widget` in the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Widget` ends in the segments `pkg`, `shapes`, `Widget`, and no segment holds the dotted name `pkg.shapes`

#### Scenario: Python module and package carry no marker

- **GIVEN** a Python project with the package `pkg` and the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the module's identity ends in the segments `pkg`, `shapes`, the package's identity ends in the segment `pkg`, and neither identity contains an `__init__` segment

#### Scenario: Python declaration in a package initializer belongs to the package

- **GIVEN** a Python project whose file `pkg/__init__.py` defines the class `Client`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Client` ends in the segments `pkg`, `Client`

#### Scenario: Python method nests in its class

- **GIVEN** a Python project with the method `render` on the class `Widget` in the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `render` ends in the segments `pkg`, `shapes`, `Widget`, `render`

#### Scenario: Python module global is one segment

- **GIVEN** a Python project whose module `pkg.globals` assigns the annotated module global `current_app: Flask = ...`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `current_app` ends in the segments `pkg`, `globals`, `current_app`, and the name `current_app` appears once

#### Scenario: Rust inline modules nest without a directory

- **GIVEN** a Rust crate whose file `src/lib.rs` declares `mod a { mod b { pub struct S; } }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `S` ends in the segments `a`, `b`, `S`

#### Scenario: Rust module loaded through a path attribute keeps its declared name

- **GIVEN** a Rust crate that declares `#[path = "generated/schema.rs"] mod model;` and the file defines `pub struct Record;`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Record` ends in the segments `model`, `Record`, and no segment is named `generated` or `schema`

### Requirement: Identity names the declaration's owner

Every canonical identity SHALL name the declaration's owner between the workspace and the segments, in the text form `workspace::package[+unit]::segments`.
The owner SHALL name the package that supplies the declaration.
For a language in which two units of one package may declare the same qualified name, the owner SHALL also name the unit, as follows: the package's default unit carries no marker; any other unit is marked `+kind.name`, where kind and name are the unit's kind and name as the build tool declares them; a declaration whose source is compiled into more than one unit of its package has one identity, marked `+shared`.
For a language with no such units, the owner is the package alone.
Whether a unit carries a marker, and the marker's text, SHALL depend only on that unit's own kind and name, never on which other units the package has.
When the system cannot determine which unit owns a declaration, the build SHALL succeed, the declaration SHALL carry the package-only owner, and the build answer SHALL disclose that the owner's unit could not be determined, naming the package, or the document, whose units are unknown, and the reason they could not be determined.

Serves: saved-identity-stays-valid, identity-reads-as-qualified-name

#### Scenario: Rust library declarations are unmarked

- **GIVEN** a Cargo package `libbin` whose library declares `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of the library's `Config` is `<workspace>::libbin::util::Config`

#### Scenario: Rust binary declarations are marked by kind and name

- **GIVEN** a Cargo package `libbin` whose binary target `tool` at `src/bin/tool.rs` declares `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of the binary's `Config` is `<workspace>::libbin+bin.tool::util::Config`

#### Scenario: Default binary with the package's name is distinguished by kind

- **GIVEN** a Cargo package `libbin` with a library at `src/lib.rs` and a default binary at `src/main.rs`, both declaring `fn run()`
- **WHEN** canonical identities are assigned
- **THEN** the library's `run` is `<workspace>::libbin::run` and the binary's `run` is `<workspace>::libbin+bin.libbin::run`

#### Scenario: Source compiled into two units has one shared identity

- **GIVEN** a Cargo package `libbin` whose integration tests `tests/a.rs` and `tests/b.rs` each declare `mod common;` loading `tests/common/mod.rs`, which defines `pub fn fixture()`, and each call `common::fixture()`
- **WHEN** canonical identities are assigned
- **THEN** `fixture` has the one identity `<workspace>::libbin+shared::common::fixture`

#### Scenario: Adding a target changes no other unit's identities

- **GIVEN** a Cargo package `libbin` indexed with a library and a binary `tool`
- **WHEN** a second binary `src/bin/second.rs` declaring `mod util { pub struct Config; }` is added and the package is indexed again
- **THEN** every identity of the library and of `tool` is identical to its prior identity, and the new binary's `Config` is `<workspace>::libbin+bin.second::util::Config`

#### Scenario: Python owner is the project with no marker

- **GIVEN** the shared Python conformance project, whose project name is `python-conformance`, with the class `Widget` in the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Widget` is `<workspace>::python-conformance::pkg::shapes::Widget`

#### Scenario: Unavailable list of targets degrades to package-only owners with disclosure

- **GIVEN** a Cargo package `libbin` with a library and a binary `tool`, on a host where the build tool does not return the package's list of targets within the probe bound
- **WHEN** the workspace is built
- **THEN** the build succeeds, every declaration of the package carries the owner `libbin` with no unit marker, and the build answer discloses that the crate targets of `libbin` could not be determined and why

#### Scenario: Document reached from no target root degrades to the package-only owner with disclosure

- **GIVEN** a Cargo package `libbin` whose file `src/orphan.rs` declares `pub fn stray()` and is loaded by no `mod` declaration of any target
- **WHEN** the workspace is built
- **THEN** the build succeeds, `stray` carries the owner `libbin` with no unit marker, every other declaration carries its unit's owner, and the build answer discloses that `src/orphan.rs` belongs to no known unit because no target loads it

### Requirement: Identity does not depend on the indexer

For any declaration that two conforming indexers for a language both report as one declaration with the same qualified name, the canonical identity SHALL be the same whichever of them produced the index.
This guarantee does not cover a declaration that one indexer reports and another does not, and does not cover a name for which the indexers report different numbers of definitions; the identities of such definitions are governed by `Identity uniqueness within a workspace`.

Serves: saved-identity-stays-valid

#### Scenario: scip-python yields the Python reference identities

- **GIVEN** the shared Python conformance project and its reference list of canonical identities, written from the identity form this spec defines
- **WHEN** the project is indexed through scip-python and canonical identities are projected
- **THEN** every module, class, function, method, and module global the index reports has the identity the reference list gives it

#### Scenario: rust-analyzer yields the Rust reference identities

- **GIVEN** the shared Rust conformance project and its reference list of canonical identities, written from the identity form this spec defines
- **WHEN** the project is indexed through rust-analyzer and canonical identities are projected
- **THEN** every module, struct, function, and method the index reports, in the library and in the binary, has the identity the reference list gives it

### Requirement: Identity does not depend on source location

For any declaration, the canonical identity SHALL be unchanged by a change of the file or directory that holds the declaration, while the declaration's qualified name and owner are unchanged.
A change to the declaration's qualified name SHALL change its canonical identity.

Serves: saved-identity-stays-valid

#### Scenario: Rust inline module moved to its own file

- **GIVEN** a Rust crate indexed with `mod util { pub fn load() {} }` inline in `src/lib.rs`
- **WHEN** the module is changed to `mod util;` with the body in `src/util.rs` and the crate is indexed again
- **THEN** the identity of `load` is identical to its prior identity

#### Scenario: Rust module file renamed to a directory module

- **GIVEN** a Rust crate indexed with the module `util` in `src/util.rs`
- **WHEN** the file is moved to `src/util/mod.rs` and the crate is indexed again
- **THEN** every identity under `util` is identical to its prior identity

#### Scenario: Python package moved between source roots

- **GIVEN** a Python project indexed with the module `acme.tools` at `src/acme/tools.py`
- **WHEN** the package is moved to `acme/tools.py` at the project root, the project configuration is updated so the import name stays `acme.tools`, and the project is indexed again
- **THEN** every identity under `acme.tools` is identical to its prior identity

#### Scenario: A module rename changes identity

- **GIVEN** a Rust crate indexed with the module `util`
- **WHEN** the module is renamed to `helpers` and the crate is indexed again
- **THEN** the declarations in the module have identities ending in `helpers`, and the prior identities under `util` resolve to no symbol
