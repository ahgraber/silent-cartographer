# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Symbol retrieval at a chosen detail

> Previously: unchanged contract text; scenarios sampled Rust symbols only.

The system SHALL provide a CLI command `get` that, given a symbol identified either by name or by a source position, returns that symbol at a requested detail level — its location, its signature, or its full source body — where retrieval by position resolves to the symbol enclosing that position.

Serves: python-precise-locate

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

#### Scenario: Retrieve full body of a Python symbol

- **GIVEN** an indexed Python function whose body spans several indentation levels
- **WHEN** `get` is invoked for it at body detail
- **THEN** the returned text equals the function's source span byte-for-byte, indentation included

#### Scenario: Retrieve Python symbol by position

- **GIVEN** a source position inside a Python method body
- **WHEN** `get` is invoked for that position
- **THEN** the enclosing method is returned

### Requirement: Relationship trace

> Previously: unchanged contract text; scenarios sampled Rust symbols only.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), and `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound).
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes."

Serves: python-blast-radius

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

#### Scenario: Trace dependents

- **GIVEN** an indexed Rust symbol that other declarations use
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the depending symbols are returned, each labeled with the kind of dependency that connected it and its distance from the subject

#### Scenario: Trace dependents of a Python symbol

- **GIVEN** an indexed Python function used by other functions and imported by other modules
- **WHEN** `trace` is invoked for it over the `dependents` relation
- **THEN** the depending symbols are returned, each labeled with the kind of dependency that connected it and its distance from the subject

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Symbol reference resolution

> Previously: unchanged contract text; scenarios sampled Rust naming only.

The system SHALL resolve a symbol reference supplied as a short name, a qualified name, or a canonical identity to the symbol it denotes, and SHALL return a typed candidate set when a reference denotes more than one symbol rather than selecting one arbitrarily.

Serves: python-precise-locate

#### Scenario: Qualified name resolves uniquely

- **GIVEN** a qualified name that denotes exactly one indexed symbol
- **WHEN** it is supplied as a reference to a query
- **THEN** that single symbol is resolved and queried

#### Scenario: Python dotted qualified name resolves

- **GIVEN** an indexed Python symbol and its dotted module-qualified name
- **WHEN** the dotted name is supplied as a reference to a query
- **THEN** exactly that symbol is resolved

#### Scenario: Ambiguous short name returns candidates

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** it is supplied as a reference
- **THEN** a typed candidate set of the matching symbols is returned rather than an arbitrary one being chosen

#### Scenario: Canonical identity round-trips

- **GIVEN** the canonical identity of an indexed symbol
- **WHEN** it is supplied as a reference
- **THEN** exactly that symbol is resolved
