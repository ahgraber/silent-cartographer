# Symbol Identity Specification

## Purpose

Defines the canonical identity assigned to every indexed symbol: a deterministic, workspace-namespaced key that is stable across re-indexing and unique within a workspace.
This identity is the join key across the syntax and semantic oracles and the round-trip key exposed to consumers.

## Terms

These terms name the parts a canonical identity is built from.
The example is the Cargo package `libbin` in the workspace `ws`, with a library and a binary named `tool`:

```text
workspace  ws
└ package  libbin
  ├ unit   the library (default unit)
  │ └ module  util
  │   └ declaration  struct Config     identity: ws::libbin::util::Config
  └ unit   the binary tool (marker +bin.tool)
    └ module  util
      └ declaration  struct Config     identity: ws::libbin+bin.tool::util::Config
```

| Term           | Meaning                                                                                                                                                                                                                                                                                                          | Example                                                  |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Workspace      | The directory c10r indexes. Every identity starts with its name.                                                                                                                                                                                                                                                 | `ws`                                                     |
| Package        | What a developer names, publishes, and depends on: one Cargo package, or one Python project.                                                                                                                                                                                                                     | `libbin`, `python-conformance`                           |
| Unit           | A part of a package that the build tool compiles on its own. Two units of one package can each declare the same qualified name. In Rust, the library, each binary, each integration test, each example, each benchmark, and the build script are units; Cargo calls them targets. A Python project has one unit. | the library; the binary `tool`                           |
| Kind           | The type of a unit, as the build tool names it.                                                                                                                                                                                                                                                                  | `lib`, `bin`, `test`, `example`, `bench`, `custom-build` |
| Default unit   | The unit that other packages use. In Rust, the library. In Python, the project.                                                                                                                                                                                                                                  | the library                                              |
| Marker         | The text after `+` in an identity. It names the unit when the unit is not the default unit. Declarations in the default unit have no marker. `+shared` marks source that more than one unit compiles.                                                                                                            | `+bin.tool`, `+test.a`, `+shared`                        |
| Module         | A namespace inside a unit. A Rust module is a file or a `mod` block. A Python module is a file; a Python package directory, such as `pkg`, is also a module here, because c10r's package is the whole project.                                                                                                   | `util`, `pkg.shapes`                                     |
| Declaration    | One named item in the code that receives an identity: a module, type, function, method, field, or module global.                                                                                                                                                                                                 | `struct Config`                                          |
| Document       | One source file. A document holds declarations. An identity never depends on which document holds a declaration.                                                                                                                                                                                                 | `src/util.rs`                                            |
| Qualified name | A declaration's name with every module and type that encloses it, as the language writes it.                                                                                                                                                                                                                     | `util::Config`, `pkg.shapes.Widget`                      |
| Segment        | One level of a qualified name. An identity writes segments separated by `::`.                                                                                                                                                                                                                                    | `util`, `Config`                                         |
| Descriptor     | The name an indexer records for a declaration. Declarations in different units can have the same descriptor.                                                                                                                                                                                                     | `util/Config#`                                           |
| Disambiguator  | The `#n` suffix that separates two distinct declarations whose identities would otherwise be equal.                                                                                                                                                                                                              | `#0`, `#1`                                               |

## Requirements

### Requirement: Deterministic canonical identity

The system SHALL give each indexed symbol a canonical identity computed only from the workspace, the symbol's package and unit, and the symbol's qualified name.
Indexing unchanged sources again SHALL give every symbol the same identity, byte for byte.

#### Scenario: Stable across re-index

- **GIVEN** a Rust source tree that has been indexed once
- **WHEN** the same tree is indexed again with no source changes
- **THEN** every symbol's canonical identity is byte-for-byte identical to its prior identity

#### Scenario: Independent of discovery order

- **GIVEN** two indexing runs over the same sources that visit files in different orders
- **WHEN** the canonical identities are compared
- **THEN** each symbol resolves to the same identity regardless of the order in which it was encountered

### Requirement: Workspace-namespaced identity

The system SHALL qualify every canonical identity with the identity of the workspace it was indexed under, such that identical symbol descriptors originating from two different workspaces are never equal.

#### Scenario: Same descriptor in two workspaces stays distinct

- **GIVEN** two workspaces that each define a symbol with the same module path and qualified name
- **WHEN** each workspace is indexed into its own store
- **THEN** the two symbols have distinct canonical identities and neither query nor reference resolution conflates them

### Requirement: Identity uniqueness within a workspace

Within one workspace, no two distinct symbols SHALL share a canonical identity.
Two symbols in different units differ by their unit, so neither SHALL carry a disambiguator.
Only distinct symbols in the same unit with the same qualified name SHALL carry a disambiguator.
A disambiguator SHALL be the same on every run over unchanged sources, whatever order the files and symbols are read in.

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

### Requirement: Derived default workspace identity

When no workspace identity is supplied, the system SHALL derive the workspace identity deterministically from the workspace root's directory name, such that the same root always yields the same identity, and SHALL use a supplied workspace identity verbatim in preference to any derived default.

#### Scenario: Default derived from the root name

- **GIVEN** a build invoked with no workspace identity supplied
- **WHEN** the index is built for a workspace root
- **THEN** every canonical identity is namespaced by an identity derived from the root's directory name

#### Scenario: Same root always derives the same default

- **GIVEN** two builds of the same workspace root, neither supplying a workspace identity
- **WHEN** the derived workspace identities are compared
- **THEN** they are identical

#### Scenario: Supplied identity overrides the default

- **GIVEN** a build invoked with an explicit workspace identity
- **WHEN** the index is built
- **THEN** every canonical identity is namespaced by the supplied identity, not the derived default

### Requirement: Segments follow the language's declared nesting

After the workspace, package, and unit, a canonical identity SHALL have one segment for each nesting level the language places the declaration in, outermost first, then one segment for the declaration itself.
The identity SHALL NOT add a segment for a level the language does not have, and SHALL NOT merge two of the language's levels into one segment.
A module's identity SHALL be its qualified name alone, with no extra segment that marks it as a module.

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

Every canonical identity SHALL name the declaration's package and unit, in the form `workspace::package[+marker]::segments`, where the marker is:

- none, for a declaration in the package's default unit;
- `+kind.name`, for a declaration in any other unit, with the kind and name the build tool gives that unit, such as `+bin.tool`;
- `+shared`, for a declaration whose source file is compiled into more than one unit of its package; it has one identity, not one per unit.

A language whose packages have a single unit, such as Python, never uses a marker.
A unit's marker SHALL depend only on that unit's own kind and name, never on which other units the package has.
When the system cannot determine a declaration's unit, the build SHALL still succeed, and the declaration SHALL have no marker.
The build answer SHALL then name the affected package or document and say why its unit could not be determined.

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

### Requirement: Identity does not depend on the indexer

When two conforming indexers for a language both report a declaration with the same qualified name, its canonical identity SHALL be the same whichever indexer built the index.
This does not cover a declaration that only one indexer reports, or a name for which the indexers report different numbers of definitions; `Identity uniqueness within a workspace` governs those identities.

#### Scenario: scip-python yields the Python reference identities

- **GIVEN** the shared Python conformance project and its reference list of canonical identities, written by hand from this spec
- **WHEN** the project is indexed through scip-python and canonical identities are assigned
- **THEN** every module, class, function, method, and module global the index reports has the identity the reference list gives it

#### Scenario: rust-analyzer yields the Rust reference identities

- **GIVEN** the shared Rust conformance project and its reference list of canonical identities, written by hand from this spec
- **WHEN** the project is indexed through rust-analyzer and canonical identities are assigned
- **THEN** every module, struct, function, and method the index reports, in the library and in the binary, has the identity the reference list gives it

### Requirement: Relative imports resolve to their absolute path

A relative import SHALL resolve to the same symbol as its absolute import path.
For example, `from .shapes import Widget` in `pkg/consumer.py` refers to the module `pkg.shapes`, and `super::load()` in Rust refers to `load` in the parent module.
If the absolute path cannot be worked out, the reference is not attributed to any workspace symbol.

#### Scenario: Python import from the same package

- **GIVEN** `pkg/consumer.py` contains `from .shapes import Widget`
- **WHEN** the workspace is built
- **THEN** the reference is a reference to `pkg.shapes`, and `pkg.consumer` is one of its importers

#### Scenario: Python import from a parent package

- **GIVEN** `pkg/sub/deep.py` contains `from ..shapes import Widget`
- **WHEN** the workspace is built
- **THEN** the reference is a reference to `pkg.shapes`

#### Scenario: Python name bound by a relative import

- **GIVEN** a module contains `from . import shapes` and later `shapes.Widget()`
- **WHEN** the workspace is built
- **THEN** both references are references to `pkg.shapes`

#### Scenario: Same relative name in two packages

- **GIVEN** packages `a` and `a.sub` each contain a module `app`, and each package imports it with `from .app import run`
- **WHEN** the workspace is built
- **THEN** each import refers to the `app` module of its own package

#### Scenario: Rust `super` path

- **GIVEN** `src/lib.rs` declares `mod a { pub fn load() {} mod b { fn f() { super::load(); } } }`
- **WHEN** the workspace is built
- **THEN** the call `super::load()` is a reference to `a::load`

### Requirement: Identity does not depend on source location

Moving a declaration to another file or directory SHALL NOT change its canonical identity, as long as its qualified name, package, and unit stay the same.
Changing a declaration's qualified name SHALL change its identity.

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

## Technical Notes

- **Implementation**: `src/identity.rs`
- **Dependencies**: none
