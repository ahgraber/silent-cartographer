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
- **WHEN** both are indexed into the store
- **THEN** the two symbols have distinct canonical identities and neither query nor reference resolution conflates them

### Requirement: Identity uniqueness within a workspace

The system SHALL guarantee that within one workspace no two distinct symbols share a canonical identity, introducing a disambiguator only where the resolved descriptor would otherwise collide.

#### Scenario: Distinct symbols never collide

- **GIVEN** a workspace whose sources contain many distinct symbols
- **WHEN** canonical identities are assigned
- **THEN** no two distinct symbols are assigned the same identity

#### Scenario: Disambiguation is detected, not assumed

- **GIVEN** a language whose descriptors are already unique without a parameter-based disambiguator
- **WHEN** identities are assigned for that language
- **THEN** no disambiguator suffix is added, and uniqueness still holds

## Technical Notes

- **Implementation**: `src/identity.rs`
- **Dependencies**: none
