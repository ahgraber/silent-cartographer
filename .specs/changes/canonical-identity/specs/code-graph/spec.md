# Delta for Code Graph

## MODIFIED Requirements

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

> Previously: only a reference whose own token spelled the package name was attributed to the library; a reference through a path that starts with the package name (`libbin::util::load()`) was attributed to the duplicate in the unit that holds the reference.
> Each attribution was also stored with the rule that chose it; the spec no longer promises that.

When two or more distinct definitions share one resolved descriptor, they are duplicates, and the system SHALL NOT guess which duplicate a reference to them points to.
This requirement covers references that an alignment rule accepts; a reference that no alignment rule accepts SHALL be attributed to no duplicate.
Each duplicate SHALL keep its own definition location, and no duplicate SHALL be given another duplicate's location.
A reference written with the package's name SHALL be attributed to the library's duplicate, in whatever file the reference appears.
This covers a reference that is the package's name itself, and the last name in a path that starts with the package's name, such as `load` in `libbin::util::load()`.
If the package's unit list is unavailable, or none of the duplicates belongs to the library, such a reference SHALL be recorded duplicate-ambiguous.
Any other reference SHALL be attributed to a duplicate only in one of these cases:

- the file that holds the reference belongs to the unit (for Rust, the library, a binary, a test, or an example) of exactly one duplicate;
- the file belongs to the units of several duplicates, and, moving outward from the reference, the first declaration that holds any duplicate holds exactly one.

Every other reference SHALL be recorded duplicate-ambiguous and attributed to no duplicate.

<!-- modified-removes: Definition occurrences attach to their own duplicate, Reference in one duplicate's territory is attributed to it, Locality does not bypass the guarded join, Package-name reference resolves to the library target, Package-name reference without target metadata is typed ambiguous, Reference outside every duplicate's territory is typed ambiguous, Same-document reference inside exactly one twin's scope is attributed to it, Same-document reference whose deciding scope holds several twins stays ambiguous, Same-document reference enclosed by no twin-bearing declaration stays ambiguous, Scope locality does not bypass the guarded join, Unduplicated descriptors are unaffected -->

Serves: blast-radius-crosses-crate-targets

#### Scenario: Two duplicates keep separate definition locations

- **GIVEN** a library file `src/lib.rs` and a binary file `src/bin/tool.rs` that each define `fn load()` in one package
- **WHEN** the workspace is built
- **THEN** the library's `load` is defined at its line in `src/lib.rs`, the binary's `load` at its line in `src/bin/tool.rs`, and neither is given the other's location

#### Scenario: A reference in one duplicate's unit is attributed to it

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of exactly one of them
- **WHEN** the workspace is built
- **THEN** the reference is attributed to that duplicate

#### Scenario: A reference in one duplicate's unit is not attributed if no alignment rule accepts it

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of exactly one of them, and no alignment rule accepts the reference
- **WHEN** the workspace is built
- **THEN** the reference is not attributed to that duplicate

#### Scenario: A reference by package name is attributed to the library

- **GIVEN** a package whose library and binary each have a root module, whose unit list is available, and a reference that is the package's name
- **WHEN** the workspace is built
- **THEN** the reference is attributed to the library's root module, never to the root module of the unit that holds the reference

#### Scenario: A path that starts with the package name is attributed to the library

- **GIVEN** a Cargo package `libbin` with a library and a binary `tool`, which each declare `mod util { pub fn load() {} }`, and the binary's `main` calls `util::load()` and `libbin::util::load()`, and the package's unit list is available
- **WHEN** the workspace is built
- **THEN** the call `libbin::util::load()` is attributed to the library's `load`, the call `util::load()` is attributed to the binary's `load`, and the references of the library's `load` include the call at `libbin::util::load()`

#### Scenario: A reference by package name with no unit list is ambiguous

- **GIVEN** a package whose library and binary each have a root module, whose unit list is unavailable, and a reference that is the package's name
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous, not attributed to the root module of the unit that holds the reference

#### Scenario: A reference outside every duplicate's unit is ambiguous

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of none of them
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference in a declaration that holds exactly one duplicate is attributed to it

- **GIVEN** duplicates defined in one file, and a reference in that file where, moving outward from the reference, the first declaration that holds any duplicate holds exactly one
- **WHEN** the workspace is built
- **THEN** the reference is attributed to that duplicate

#### Scenario: A reference in a declaration that holds several duplicates is ambiguous

- **GIVEN** duplicates defined in one file, and a reference in that file where, moving outward from the reference, the first declaration that holds any duplicate holds more than one
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference outside every declaration that holds a duplicate is ambiguous

- **GIVEN** duplicates defined at the top level of one file, and a reference elsewhere in that file that no declaration holding a duplicate encloses
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference in a declaration that holds one duplicate is not attributed if no alignment rule accepts it

- **GIVEN** a reference inside a declaration that holds exactly one duplicate, and no alignment rule accepts the reference
- **WHEN** the workspace is built
- **THEN** the reference is not attributed to that duplicate

#### Scenario: A descriptor with one definition has no ambiguous references

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the workspace is built
- **THEN** no reference to it is recorded duplicate-ambiguous

### Requirement: A build over unchanged inputs does no work

> Previously: the inputs compared were the workspace identity and root, sources, analyzer, declared environment, and chunk parameters; the packages and units the workspace declares were not among them.

The system SHALL treat the stored index as current when everything it recorded still matches the workspace: the workspace identity and root, the sources, the declared packages and units, the analyzer, the declared environment, and the chunk parameters.
When the index is current, a build SHALL analyze nothing, leave the store unchanged, report the index as already current, and succeed.
When any of those inputs differs, or no compatible store exists, the system SHALL build.
The system SHALL offer an explicit way to build even when the stored index is already current.

Serves: saved-identity-stays-valid

#### Scenario: Rebuilding an unchanged workspace analyzes nothing

- **GIVEN** a workspace whose index was just built
- **WHEN** the workspace is built again with no source, analyzer, environment, or parameter change
- **THEN** no analysis runs, the store is unchanged, and the index is reported as already current

#### Scenario: An edited source rebuilds

- **GIVEN** a workspace whose index was just built
- **WHEN** one source file is edited and the workspace is built again
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A renamed binary rebuilds

- **GIVEN** a Rust workspace whose index was just built, with a binary `tool`
- **WHEN** the binary is renamed to `cli` in `Cargo.toml` with no other change, and the workspace is built again
- **THEN** the workspace is analyzed, the store is rebuilt, and the binary's declarations have the marker `+bin.cli`

#### Scenario: A changed analyzer rebuilds

- **GIVEN** a workspace whose index was built by one analyzer version
- **WHEN** the workspace is built again with a different analyzer version
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed environment rebuilds

- **GIVEN** a workspace whose index records a declared interpreter environment
- **WHEN** the workspace is built again against a different environment
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed chunk parameter rebuilds

- **GIVEN** a workspace whose index was built under one set of chunk parameters
- **WHEN** the workspace is built again, unchanged in every other input, under a different chunk size or overlap
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A store recorded for another workspace builds

- **GIVEN** a store whose recorded workspace identity or root differs from the one being built
- **WHEN** the workspace is built over that store
- **THEN** the workspace is analyzed, the handoff is disclosed, and the store records the workspace it now describes

#### Scenario: An absent index builds

- **GIVEN** a workspace with no index
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is written

#### Scenario: A store built under an unrecognized schema version builds

- **GIVEN** a workspace whose store the system created under a schema version it does not recognize
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is replaced

#### Scenario: An explicit rebuild ignores currency

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built with the explicit rebuild option
- **THEN** the workspace is analyzed and the store is rebuilt
