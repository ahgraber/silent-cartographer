# Delta for code-navigation

## ADDED Requirements

### Requirement: Trace results at a chosen detail

The system SHALL support, on any relationship trace, a per-query detail level that projects each returned result at a chosen tier — location, signature, interface, or body — with location as the default; a result that denotes a symbol SHALL project that symbol's own tier content, a result that denotes a reference site SHALL project the tier content of the declaration the site is attributed to, and the chosen detail SHALL NOT alter which results are returned or their order.

Serves: triage-trace-results

#### Scenario: Default trace rows carry no tier content

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation with no detail requested
- **THEN** each row identifies its symbol and location and carries no tier content

#### Scenario: Trace at signature detail

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation at signature detail
- **THEN** each row carries the signature tier of its symbol

#### Scenario: Reference sites project their enclosing declaration

- **GIVEN** an indexed symbol referenced inside a method body
- **WHEN** `trace` is invoked over the `references` relation at signature detail
- **THEN** the row for that site carries the signature tier of the method the site is attributed to

#### Scenario: Trace dependents at interface detail

- **GIVEN** an indexed symbol with dependents connected through dependency edges
- **WHEN** `trace` is invoked over the `dependents` relation at interface detail
- **THEN** each detailed row carries its dependent's interface tier alongside the edge kind and distance

#### Scenario: Detail does not change the result set

- **GIVEN** an indexed symbol standing in a relation to several symbols
- **WHEN** the same trace is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

## MODIFIED Requirements

### Requirement: Symbol retrieval at a chosen detail

> Previously: the detail axis offered location, signature, and full body only.

The system SHALL provide a CLI command `get` that, given a symbol identified either by name or by a source position, returns that symbol at a requested detail level — its location, its signature, its interface (the signature together with the symbol's own documentation), or its full source body — where retrieval by position resolves to the symbol enclosing that position, and where a symbol carrying no documentation serves its signature as its interface.

Serves: fetch-at-depth, orient-by-summary

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

#### Scenario: Retrieve interface by name

- **GIVEN** an indexed Rust symbol carrying a doc comment
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the doc comment and the signature are returned without the full body

#### Scenario: Interface without documentation falls back to signature

- **GIVEN** an indexed symbol with no documentation of its own
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the symbol's signature is returned

#### Scenario: Retrieve module interface

- **GIVEN** an indexed module whose document carries module documentation
- **WHEN** `get` is invoked for the module at interface detail
- **THEN** the module's signature and its module documentation are returned without the whole document

#### Scenario: Retrieve module body returns the whole document

- **GIVEN** an indexed module
- **WHEN** `get` is invoked for the module at body detail
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Retrieve by position resolves the enclosing symbol

- **GIVEN** a source position inside a method body
- **WHEN** `get` is invoked for that position
- **THEN** the symbol enclosing the position is returned

#### Scenario: Retrieve full body of a Python symbol

- **GIVEN** an indexed Python function whose body spans several indentation levels
- **WHEN** `get` is invoked for it at body detail
- **THEN** the returned text equals the function's source span byte-for-byte, indentation included

#### Scenario: Retrieve Python interface

- **GIVEN** an indexed Python function with a docstring
- **WHEN** `get` is invoked for it at interface detail
- **THEN** the function's header and docstring are returned without the full body

#### Scenario: Retrieve Python symbol by position

- **GIVEN** a source position inside a Python method body
- **WHEN** `get` is invoked for that position
- **THEN** the enclosing method is returned
