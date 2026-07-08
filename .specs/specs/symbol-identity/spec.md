# Symbol Identity Specification

## Purpose

Defines the canonical identity assigned to every indexed symbol: a deterministic, workspace-namespaced key that is stable across re-indexing and unique within a workspace.
This identity is the join key across the syntax and semantic oracles and the round-trip key exposed to consumers.

## Requirements

### Requirement: Deterministic canonical identity

The system SHALL assign each indexed symbol a canonical identity that is a pure function of the symbol's resolved semantic descriptor, such that re-indexing unchanged sources yields the identical identity for every symbol.

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

The system SHALL guarantee that within one workspace no two distinct symbols share a canonical identity, introducing a disambiguator only where the resolved descriptor would otherwise collide; when distinct symbols share an identical resolved descriptor, their disambiguation SHALL be anchored to the symbols' definition locations, such that each symbol's identity is the same across indexing runs regardless of the order in which files or symbols are encountered.

#### Scenario: Distinct symbols never collide

- **GIVEN** a workspace whose sources contain many distinct symbols
- **WHEN** canonical identities are assigned
- **THEN** no two distinct symbols are assigned the same identity

#### Scenario: Disambiguation is detected, not assumed

- **GIVEN** a language whose descriptors are already unique without a parameter-based disambiguator
- **WHEN** identities are assigned for that language
- **THEN** no disambiguator suffix is added, and uniqueness still holds

#### Scenario: True duplicates keep definition-anchored identities

- **GIVEN** two distinct definitions the semantic backend describes with an identical resolved descriptor
- **WHEN** canonical identities are assigned
- **THEN** each definition receives a distinct identity determined by its own definition location

#### Scenario: Duplicate identities stable across discovery order

- **GIVEN** the same sources indexed twice with files visited in different orders
- **WHEN** the identities of the duplicated definitions are compared across the two runs
- **THEN** each definition carries the same identity in both runs

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

## Technical Notes

- **Implementation**: `src/identity.rs`
- **Dependencies**: none
