# Delta for code-navigation

## ADDED Requirements

### Requirement: Symbol retrieval at a chosen detail

The system SHALL provide a CLI command `get` that, given a symbol identified either by name or by a source position, returns that symbol at a requested detail level — its location, its signature, or its full source body — where retrieval by position resolves to the symbol enclosing that position.

Serves: locate-symbol, understand-structure

#### Scenario: Retrieve definition location by name

- **GIVEN** an indexed Rust symbol
- **WHEN** `get` is invoked for it at location detail
- **THEN** the symbol's definition file and position are returned

#### Scenario: Retrieve full body by name

- **GIVEN** an indexed Rust symbol
- **WHEN** `get` is invoked for it at body detail
- **THEN** the returned text equals the symbol's source span byte-for-byte

#### Scenario: Retrieve signature by name

- **GIVEN** an indexed Rust symbol that has a declaration distinct from its body
- **WHEN** `get` is invoked for it at signature detail
- **THEN** the symbol's signature is returned without its full body

#### Scenario: Retrieve by position resolves the enclosing symbol

- **GIVEN** a source position inside a method body
- **WHEN** `get` is invoked for that position
- **THEN** the symbol enclosing the position is returned

### Requirement: Relationship trace

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and for this change the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), and `references` (the sites that reference a subject, which for a type subject are its type-occurrences).

Serves: locate-symbol, understand-structure

#### Scenario: Trace contains

- **GIVEN** an indexed Rust type with several members
- **WHEN** `trace` is invoked for it over the `contains` relation
- **THEN** exactly its directly contained members are returned

#### Scenario: Trace containers

- **GIVEN** an indexed Rust method declared inside a type
- **WHEN** `trace` is invoked for it over the `containers` relation
- **THEN** the enclosing type is returned

#### Scenario: Trace references

- **GIVEN** an indexed Rust symbol used in several places
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** every reference site is returned and none is omitted

#### Scenario: References of a type are its occurrences

- **GIVEN** an indexed Rust type used at several sites
- **WHEN** `trace` is invoked for it over the `references` relation
- **THEN** the type's use sites are returned with their locations

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Symbol reference resolution

The system SHALL resolve a symbol reference supplied as a short name, a qualified name, or a canonical identity to the symbol it denotes, and SHALL return a typed candidate set when a reference denotes more than one symbol rather than selecting one arbitrarily.

Serves: locate-symbol, trust-the-answer

#### Scenario: Qualified name resolves uniquely

- **GIVEN** a qualified name that denotes exactly one indexed symbol
- **WHEN** it is supplied as a reference to a query
- **THEN** that single symbol is resolved and queried

#### Scenario: Ambiguous short name returns candidates

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** it is supplied as a reference
- **THEN** a typed candidate set of the matching symbols is returned rather than an arbitrary one being chosen

#### Scenario: Canonical identity round-trips

- **GIVEN** the canonical identity of an indexed symbol
- **WHEN** it is supplied as a reference
- **THEN** exactly that symbol is resolved

### Requirement: Calibrated output contract

The system SHALL return every query result with its provenance and freshness, SHALL identify each returned symbol by both its stable canonical identity and a human-readable name, SHALL represent an empty result as typed absence distinct from an unavailable or failed answer, SHALL order results deterministically for identical inputs, and SHALL offer the result as structured JSON.

Serves: trust-the-answer

#### Scenario: Fresh result carries provenance

- **GIVEN** a query against an index whose sources and analyzer are unchanged
- **WHEN** the result is returned as JSON
- **THEN** it carries the analyzer provenance and is marked fresh

#### Scenario: Result identifies symbols by identity and name

- **GIVEN** a query that returns one or more symbols
- **WHEN** the result is returned as JSON
- **THEN** each symbol carries both its stable canonical identity and a human-readable name

#### Scenario: Stale result is flagged

- **GIVEN** a query against an index whose sources changed since indexing
- **WHEN** the result is returned
- **THEN** it is marked stale rather than presented as current

#### Scenario: Deterministic ordering

- **GIVEN** a query whose result contains multiple locations
- **WHEN** the same query is run repeatedly against the same index
- **THEN** the locations are returned in the same order every time
