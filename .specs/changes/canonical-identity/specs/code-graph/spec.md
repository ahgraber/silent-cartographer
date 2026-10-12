# Delta for Code Graph

## MODIFIED Requirements

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

> Previously: only an occurrence whose own source token spelled the package name was attributed to the library target; a reference through a path beginning with the package name (`libbin::util::load()`) fell to containing-document locality and was attributed to the containing unit's own declaration.

When more than one distinct definition shares an identical resolved descriptor, the system SHALL attribute each definition occurrence to the definition at its own location; SHALL attribute a non-definition occurrence of that descriptor to one of the duplicates only when the occurrence's containing document is associated with exactly that one duplicate and the occurrence also satisfies the guarded join's alignment rules, except that an occurrence whose spelling names the package's library — a source token that spells the package's own name rather than the descriptor's name, or a source token that is the final component of a qualified path whose first component spells the package's own name — denotes the package's library target regardless of where it sits, and SHALL be attributed to the duplicate the package's library target owns, as the build system's authoritative target description identifies it, and SHALL be recorded duplicate-ambiguous when no authoritative target description is available or it names no persisted duplicate; SHALL, when the occurrence's containing document is associated with more than one duplicate, attribute the occurrence to a duplicate by declaration scope — the innermost declaration enclosing the occurrence that contains at least one of the duplicates' definitions decides, and the occurrence is attributed to the single duplicate defined within it — only when exactly one duplicate's definition lies within that deciding declaration and the occurrence also satisfies the guarded join's alignment rules; every attribution SHALL carry its locality evidence as provenance; and SHALL record a non-definition occurrence for which no locality evidence discriminates a single duplicate — a containing document associated with no duplicate, or shared territory whose deciding declaration contains more than one duplicate or does not exist — as a typed duplicate-ambiguous outcome, never attributing it to any single duplicate arbitrarily.

Serves: blast-radius-crosses-crate-targets

#### Scenario: Definition occurrences attach to their own duplicate

- **GIVEN** two distinct definitions sharing an identical resolved descriptor
- **WHEN** the join runs
- **THEN** each definition occurrence is attributed to the definition at its own location

#### Scenario: Reference in one duplicate's territory is attributed to it

- **GIVEN** a non-definition occurrence of a duplicated descriptor whose containing document is associated with exactly one of the duplicates
- **WHEN** the join runs
- **THEN** the occurrence is attributed to that duplicate and the attribution carries its locality evidence as provenance

#### Scenario: Locality does not bypass the guarded join

- **GIVEN** a non-definition occurrence in a document associated with exactly one duplicate, whose source text satisfies no alignment rule's expectation
- **WHEN** the join runs
- **THEN** the occurrence is refused by the guarded join and is not attributed to the duplicate

#### Scenario: Package-name reference resolves to the library target

- **GIVEN** several crate-root definitions sharing an identical resolved descriptor, and an authoritative target description naming the library target's root document
- **WHEN** the join processes a reference occurrence of that descriptor whose source token spells the package name rather than the descriptor's name
- **THEN** the occurrence is attributed to the duplicate defined at the library target's root — never to the containing document's own crate root — and the attribution carries the target-description evidence as provenance

#### Scenario: Package-qualified path resolves to the library's declaration

- **GIVEN** a Cargo package `libbin` whose library and binary `tool` each declare `mod util { pub fn load() {} }`, an authoritative target description, and the binary's `main` calling `util::load()` and `libbin::util::load()`
- **WHEN** the join runs
- **THEN** the call `libbin::util::load()` is attributed to the library's `load` with the target-description evidence as provenance, the call `util::load()` is attributed to the binary's `load`, and the references of the library's `load` include the call at `libbin::util::load()`

#### Scenario: Package-name reference without target metadata is typed ambiguous

- **GIVEN** several crate-root definitions sharing an identical resolved descriptor, and no authoritative target description available
- **WHEN** the join processes a reference occurrence of that descriptor whose source token spells the package name
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome, not attributed to the containing document's own crate root

#### Scenario: Reference outside every duplicate's territory is typed ambiguous

- **GIVEN** a non-definition occurrence of a duplicated descriptor whose containing document is associated with none of the duplicates
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Same-document reference inside exactly one twin's scope is attributed to it

- **GIVEN** several definitions sharing an identical resolved descriptor within one document, and a non-definition occurrence of that descriptor whose innermost enclosing declaration containing any of the definitions contains exactly one of them
- **WHEN** the join runs
- **THEN** the occurrence is attributed to that single duplicate and the attribution carries the declaration-scope evidence as locality provenance

#### Scenario: Same-document reference whose deciding scope holds several twins stays ambiguous

- **GIVEN** several definitions sharing an identical resolved descriptor within one document, and a non-definition occurrence whose innermost enclosing declaration containing any of the definitions contains more than one of them
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Same-document reference enclosed by no twin-bearing declaration stays ambiguous

- **GIVEN** several definitions sharing an identical resolved descriptor at the top level of one document, and a non-definition occurrence elsewhere in that document none of whose enclosing declarations contains any of the definitions
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Scope locality does not bypass the guarded join

- **GIVEN** a non-definition occurrence inside exactly one twin's scope whose source text satisfies no alignment rule's expectation
- **WHEN** the join runs
- **THEN** the occurrence is refused by the guarded join and is not attributed to the duplicate

#### Scenario: Unduplicated descriptors are unaffected

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the join runs
- **THEN** its non-definition occurrences are attributed through the ordinary guarded join, never marked duplicate-ambiguous

### Requirement: A build over unchanged inputs does no work

> Previously: the inputs compared were the workspace identity and root, sources, analyzer, declared environment, and chunk parameters; the build description that assigns owners was not among them.

Before analyzing a workspace, the system SHALL determine whether the stored index already describes that workspace: the same workspace identity and root, and the workspace's current sources, build description that assigns owners, analyzer, declared environment, and chunk parameters.
When it does, the system SHALL analyze nothing, leave the store unchanged, report the index as already current, and succeed.
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

#### Scenario: A renamed build target rebuilds

- **GIVEN** a Rust workspace whose index was just built, with a binary target `tool`
- **WHEN** the target is renamed to `cli` in `Cargo.toml` with no other change and the workspace is built again
- **THEN** the workspace is analyzed, the store is rebuilt, and the binary's declarations carry the owner `+bin.cli`

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
