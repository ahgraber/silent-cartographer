# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Symbol reference resolution

> Previously: the requirement did not say how a qualified name resolves when two units of one package declare the same path.

The system SHALL resolve a symbol reference supplied as a short name, a qualified name, or a canonical identity to the symbol it denotes, and SHALL return a typed candidate set when a reference denotes more than one symbol rather than selecting one arbitrarily.
A qualified name that begins with a package's name SHALL denote only declarations in that package's default unit.

Serves: identity-reads-as-qualified-name, blast-radius-crosses-crate-targets

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

#### Scenario: Package-qualified name resolves to the library's declaration

- **GIVEN** a Cargo package `libbin` whose library and binary `tool` each declare `util::Config`
- **WHEN** `libbin::util::Config` is supplied as a reference
- **THEN** exactly the library's `Config` is resolved

#### Scenario: Path declared in two units returns candidates

- **GIVEN** a Cargo package `libbin` whose library and binary `tool` each declare `util::Config`
- **WHEN** `util::Config` is supplied as a reference
- **THEN** a typed candidate set holding the library's `Config` and the binary's `Config` is returned

#### Scenario: Identity with a marker round-trips

- **GIVEN** the canonical identity `<workspace>::libbin+bin.tool::util::Config` of an indexed binary declaration
- **WHEN** it is supplied as a reference
- **THEN** exactly that symbol is resolved
