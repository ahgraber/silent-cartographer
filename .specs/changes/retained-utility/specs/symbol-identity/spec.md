# Delta for Symbol Identity

## ADDED Requirements

### Requirement: A structural identity names a declaration by its file and its nesting

In a structural index, every canonical identity SHALL be computed only from the workspace, the path of the declaration's file relative to the workspace root, and the declarations that lexically enclose it in that file, outermost first, followed by the declaration's own name.
A file's own identity SHALL be the workspace and the file's path, with no further segment.
The file path SHALL be written with a leading `./`, so that no package name, which never begins with `.`, can be mistaken for it.
The file path SHALL be written with `%`, `:`, and `#` percent-encoded, and each declaration name with `%` and `#` percent-encoded, so that the path ends at the first `::` after it and no name can be mistaken for a disambiguator.
Two distinct declarations in one file whose structural identities would otherwise be equal SHALL carry disambiguators that follow their order in the file.
Within one workspace identity, a structural identity SHALL NOT be equal to any semantic identity, and the system SHALL tell the two kinds apart from the identity text and the workspace identity.
Indexing unchanged sources again SHALL give every declaration the same structural identity, byte for byte.

Serves: structural-navigation-without-semantic-indexer, calibrated-capability-disclosure

#### Scenario: A Bash function is named by its file

- **GIVEN** a workspace whose file `scripts/deploy.sh` defines the function `deploy`
- **WHEN** a structural index is built
- **THEN** the identity of `deploy` names the workspace, the path `scripts/deploy.sh`, and the segment `deploy`

#### Scenario: A Python method nests in its class within its file

- **GIVEN** a Python workspace whose file `src/app/models.py` defines the method `save` on the class `User`, built structurally
- **WHEN** a structural index is built
- **THEN** the identity of `save` names the workspace, the path `src/app/models.py`, and the segments `User`, `save`, and no segment holds a module name derived from the path

#### Scenario: A Rust item in an inline module nests in the module within its file

- **GIVEN** a Rust workspace whose file `src/lib.rs` declares `mod a { pub struct S; }`, built structurally
- **WHEN** a structural index is built
- **THEN** the identity of `S` names the workspace, the path `src/lib.rs`, and the segments `a`, `S`

#### Scenario: A redefined Bash function is disambiguated in file order

- **GIVEN** a Bash file that defines the function `log` twice
- **WHEN** a structural index is built twice, with files visited in different orders
- **THEN** the two definitions carry distinct identities, the first definition in the file carries the first disambiguator, and each identity is the same in both builds

#### Scenario: A Bash name containing the separator is disambiguated

- **GIVEN** a Bash file that defines the top-level function `lib::log`, and later the function `lib` with the function `log` nested inside it
- **WHEN** a structural index is built
- **THEN** `lib::log` and the nested `log` carry distinct identities, and the one defined first in the file carries the first disambiguator

#### Scenario: A structural identity is distinguishable from a semantic one

- **GIVEN** the structural identity of a declaration and the semantic identity of any declaration under the same workspace identity, including a workspace identity that contains `::./`
- **WHEN** the two identity texts are compared
- **THEN** they are not equal, and each is recognized as its own kind from its text and the workspace identity

#### Scenario: A path containing the separator cannot collide across files

- **GIVEN** a Bash workspace whose file `lib.sh` defines the function `log.sh`, and which also holds a file named `lib.sh::log.sh`
- **WHEN** a structural index is built
- **THEN** the function and the second file have distinct identities, and the second file's identity holds its path with `:` percent-encoded

#### Scenario: A name containing `#` cannot be mistaken for a disambiguator

- **GIVEN** one file's declarations named `foo#0`, `foo`, and `foo`
- **WHEN** structural identities are assigned to them
- **THEN** the three identities are distinct, and the identity of `foo#0` holds its `#` percent-encoded

## MODIFIED Requirements

### Requirement: Deterministic canonical identity

The system SHALL give each symbol of a semantic index a canonical identity computed only from the workspace, the symbol's package and unit, and the symbol's qualified name.
Indexing unchanged sources again SHALL give every symbol the same identity, byte for byte.
`A structural identity names a declaration by its file and its nesting` governs the identities of a structural index.

> Previously: the rule applied to every indexed symbol, because every index was semantic.

Serves: calibrated-capability-disclosure

#### Scenario: Stable across re-index

- **GIVEN** a Rust source tree that has been indexed once
- **WHEN** the same tree is indexed again with no source changes
- **THEN** every symbol's canonical identity is byte-for-byte identical to its prior identity

#### Scenario: Independent of discovery order

- **GIVEN** two indexing runs over the same sources that visit files in different orders
- **WHEN** the canonical identities are compared
- **THEN** each symbol resolves to the same identity regardless of the order in which it was encountered

### Requirement: Identity uniqueness within a workspace

Within one workspace, no two distinct symbols SHALL share a canonical identity.
In a semantic index, two symbols in different units differ by their unit, so neither SHALL carry a disambiguator, and only distinct symbols in the same unit with the same qualified name SHALL carry a disambiguator.
In a structural index, only distinct declarations in the same file whose identities would otherwise be equal SHALL carry a disambiguator.
A disambiguator SHALL be the same on every run over unchanged sources, whatever order the files and symbols are read in.

> Previously: the disambiguator rule named units only, because every index was semantic.

Serves: structural-navigation-without-semantic-indexer

#### Scenario: Distinct symbols never collide

- **GIVEN** a workspace whose sources contain many distinct symbols
- **WHEN** canonical identities are assigned
- **THEN** no two distinct symbols are assigned the same identity

#### Scenario: Disambiguation is detected, not assumed

- **GIVEN** a language whose descriptors are already unique without a parameter-based disambiguator
- **WHEN** identities are assigned for that language
- **THEN** no disambiguator suffix is added, and uniqueness still holds

#### Scenario: True duplicates receive distinct identities

- **GIVEN** two distinct definitions in one unit that the indexer records with the same descriptor
- **WHEN** canonical identities are assigned
- **THEN** each definition receives a distinct identity

#### Scenario: Duplicate identities stable across discovery order

- **GIVEN** the same sources indexed twice with files visited in different orders
- **WHEN** the identities of the duplicated definitions are compared across the two runs
- **THEN** each definition carries the same identity in both runs

#### Scenario: Same descriptor in two units carries no disambiguator

- **GIVEN** a Cargo package `libbin` whose library and binary `tool` each declare `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the library's `Config` and the binary's `Config` have identities that differ only in their unit, and neither has a disambiguator

#### Scenario: Same name in two files of a structural index carries no disambiguator

- **GIVEN** a Bash workspace whose files `a.sh` and `b.sh` each define the function `log`
- **WHEN** a structural index is built
- **THEN** the two identities differ only in their file path, and neither has a disambiguator

### Requirement: Segments follow the language's declared nesting

After the workspace, package, and unit, a semantic identity SHALL have one segment for each nesting level the language places the declaration in, outermost first, then one segment for the declaration itself.
The identity SHALL NOT add a segment for a level the language does not have, and SHALL NOT merge two of the language's levels into one segment.
A module's identity SHALL be its qualified name alone, with no extra segment that marks it as a module.

> Previously: the rule applied to every identity, because every index was semantic.

Serves: calibrated-capability-disclosure

#### Scenario: Python module path splits per component

- **GIVEN** a Python project with the class `Widget` in the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Widget` ends in the segments `pkg`, `shapes`, `Widget`, and no segment holds the dotted name `pkg.shapes`

#### Scenario: Python module and package have no `__init__` segment

- **GIVEN** a Python project with the package directory `pkg` and the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the module's identity ends in the segments `pkg`, `shapes`, the package directory's identity ends in the segment `pkg`, and neither identity has an `__init__` segment

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

- **GIVEN** a Rust library whose file `src/lib.rs` declares `mod a { mod b { pub struct S; } }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `S` ends in the segments `a`, `b`, `S`

#### Scenario: Rust module loaded through a path attribute keeps its declared name

- **GIVEN** a Rust library that declares `#[path = "generated/schema.rs"] mod model;`, and that file defines `pub struct Record;`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Record` ends in the segments `model`, `Record`, and no segment is named `generated` or `schema`

### Requirement: Identity names the declaration's package and unit

Every semantic identity SHALL name the declaration's package and unit, in the form `workspace::package[+marker]::segments`, where the marker is:

- none, for a declaration in the package's default unit;
- `+kind.name`, for a declaration in any other unit, with the kind and name the build tool gives that unit, such as `+bin.tool`;
- `+shared`, for a declaration whose source file is compiled into more than one unit of its package; it has one identity, not one per unit.

A language whose packages have a single unit, such as Python, never uses a marker.
A unit's marker SHALL depend only on that unit's own kind and name, never on which other units the package has.
When the system cannot determine a declaration's unit, the build SHALL still succeed, and the declaration SHALL have no marker.
The build answer SHALL then name the affected package or document and say why its unit could not be determined.

> Previously: the rule applied to every canonical identity, because every index was semantic.

Serves: calibrated-capability-disclosure

#### Scenario: Rust library declarations are unmarked

- **GIVEN** a Cargo package `libbin` whose library declares `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of the library's `Config` is `<workspace>::libbin::util::Config`

#### Scenario: Rust binary declarations are marked by kind and name

- **GIVEN** a Cargo package `libbin` whose binary `tool` at `src/bin/tool.rs` declares `mod util { pub struct Config; }`
- **WHEN** canonical identities are assigned
- **THEN** the identity of the binary's `Config` is `<workspace>::libbin+bin.tool::util::Config`

#### Scenario: Default binary with the package's name is distinguished by kind

- **GIVEN** a Cargo package `libbin` with a library at `src/lib.rs` and a default binary at `src/main.rs`, both declaring `fn run()`
- **WHEN** canonical identities are assigned
- **THEN** the library's `run` is `<workspace>::libbin::run` and the binary's `run` is `<workspace>::libbin+bin.libbin::run`

#### Scenario: Source compiled into two units has one shared identity

- **GIVEN** a Cargo package `libbin` whose integration tests `tests/a.rs` and `tests/b.rs` each declare `mod common;`, which loads `tests/common/mod.rs`, and each call `common::fixture()`, which that file defines
- **WHEN** canonical identities are assigned
- **THEN** `fixture` has the one identity `<workspace>::libbin+shared::common::fixture`

#### Scenario: Adding a binary changes no other unit's identities

- **GIVEN** a Cargo package `libbin` indexed with a library and a binary `tool`
- **WHEN** a second binary `src/bin/second.rs` declaring `mod util { pub struct Config; }` is added and the package is indexed again
- **THEN** every identity in the library and in `tool` is unchanged, and the new binary's `Config` is `<workspace>::libbin+bin.second::util::Config`

#### Scenario: Python identity names the project with no marker

- **GIVEN** the shared Python conformance project, whose project name is `python-conformance`, with the class `Widget` in the module `pkg.shapes`
- **WHEN** canonical identities are assigned
- **THEN** the identity of `Widget` is `<workspace>::python-conformance::pkg::shapes::Widget`

#### Scenario: Package whose units cannot be listed has no markers

- **GIVEN** a Cargo package `libbin` with a library and a binary `tool`, and the build tool does not list the package's units within the time limit
- **WHEN** the workspace is built
- **THEN** the build succeeds, no declaration in `libbin` has a marker, and the build answer says that the units of `libbin` could not be determined, and why

#### Scenario: File that no unit loads has no marker

- **GIVEN** a Cargo package `libbin` whose file `src/orphan.rs` declares `pub fn stray()`, and no `mod` declaration in any unit loads that file
- **WHEN** the workspace is built
- **THEN** the build succeeds, `stray` has no marker, every other declaration has its unit's marker, and the build answer says that `src/orphan.rs` belongs to no known unit because no unit loads it

### Requirement: Identity does not depend on source location

Moving a declaration to another file or directory SHALL NOT change its semantic identity, as long as its qualified name, package, and unit stay the same.
Changing a declaration's qualified name SHALL change its identity.
A structural identity names the declaration's file, so moving a declaration to another file SHALL change its structural identity.

> Previously: the rule applied to every identity, because every index was semantic.

Serves: calibrated-capability-disclosure

#### Scenario: Rust inline module moved to its own file

- **GIVEN** a Rust library indexed with `mod util { pub fn load() {} }` inline in `src/lib.rs`
- **WHEN** the module is changed to `mod util;` with the body in `src/util.rs` and the library is indexed again
- **THEN** the identity of `load` is unchanged

#### Scenario: Rust module file renamed to a directory module

- **GIVEN** a Rust library indexed with the module `util` in `src/util.rs`
- **WHEN** the file is moved to `src/util/mod.rs` and the library is indexed again
- **THEN** every identity under `util` is unchanged

#### Scenario: Python package moved between source roots

- **GIVEN** a Python project indexed with the module `acme.tools` at `src/acme/tools.py`
- **WHEN** the directory is moved to `acme/tools.py` at the project root, the project configuration is updated so the import name stays `acme.tools`, and the project is indexed again
- **THEN** every identity under `acme.tools` is unchanged

#### Scenario: A module rename changes identity

- **GIVEN** a Rust library indexed with the module `util`
- **WHEN** the module is renamed to `helpers` and the library is indexed again
- **THEN** the declarations in the module have identities ending in `helpers`, and the prior identities under `util` resolve to no symbol

#### Scenario: A moved Bash function changes its structural identity

- **GIVEN** a Bash workspace indexed with the function `deploy` in `scripts/deploy.sh`
- **WHEN** the function is moved to `scripts/release.sh` and the workspace is indexed again
- **THEN** the identity of `deploy` names `scripts/release.sh`, and the prior identity resolves to no symbol
