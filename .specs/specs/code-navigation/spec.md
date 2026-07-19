# Code Navigation Specification

## Purpose

Defines the query surface over the code graph: resolving a symbol reference across identity, qualified name, and shortname tiers; retrieving a symbol at a chosen detail; tracing named relations; and the calibrated output contract every answer carries.

## Requirements

### Requirement: Symbol retrieval at a chosen detail

The system SHALL provide a CLI command `get` that, given a symbol identified either by name or by a source position, returns that symbol at a requested detail level — its location, its signature, its interface (the signature together with the symbol's own documentation), or its full source body — where retrieval by position resolves to the symbol enclosing that position, and where a symbol carrying no documentation serves its signature as its interface.

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

### Requirement: Relationship trace

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), and `implementers` (the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes).
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes."

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

#### Scenario: Trace importers

- **GIVEN** an indexed module imported by several other modules
- **WHEN** `trace` is invoked for it over the `importers` relation
- **THEN** exactly the modules that import it are returned

#### Scenario: Trace implementers

- **GIVEN** an indexed trait implemented by several types
- **WHEN** `trace` is invoked for it over the `implementers` relation
- **THEN** exactly the types that declare it as a supertype are returned

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Trace results at a chosen detail

The system SHALL support, on any relationship trace, a per-query detail level that projects each returned result at a chosen tier — location, signature, interface, or body — with location as the default; a result that denotes a symbol SHALL project that symbol's own tier content, a result that denotes a reference site SHALL project the tier content of the declaration the site is attributed to, and the chosen detail SHALL NOT alter which results are returned or their order.

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

### Requirement: Depth-bounded impact answer with an honest horizon

For a dependents query, the system SHALL return detailed results only up to a depth bound; each detailed result SHALL identify the dependent symbol, the kind of dependency edge that connected it, and its distance from the subject in hops; dependents beyond the bound SHALL be reported in aggregate — counts by edge kind and distance — up to a stated horizon; and the answer SHALL always distinguish between reach that ends within the bound, reach that extends beyond the bound, and reach whose aggregate is itself cut off at the horizon.

#### Scenario: Reach ends within the bound

- **GIVEN** a subject whose every dependent lies within the requested depth
- **WHEN** dependents are queried at that depth
- **THEN** the answer conveys that no reach extends beyond what is detailed

#### Scenario: Reach extends beyond the bound

- **GIVEN** a subject with dependents deeper than the requested depth
- **WHEN** dependents are queried at that depth
- **THEN** the detailed results stop at the bound and the answer reports aggregate counts of the deeper dependents by edge kind and distance

#### Scenario: Aggregate discloses its own horizon

- **GIVEN** a subject whose dependency network extends beyond the aggregate horizon
- **WHEN** dependents are queried
- **THEN** the answer states that the aggregate itself is bounded rather than presenting it as the total reach

#### Scenario: Bound at or beyond the horizon is still disclosed

- **GIVEN** a subject whose dependency network reaches the horizon
- **WHEN** dependents are queried with a depth bound at or beyond the horizon
- **THEN** the answer states that the traversal was cut off at the horizon rather than reporting that reach ends within the bound

#### Scenario: Detailed results carry kind and distance

- **GIVEN** a subject with dependents connected through more than one edge kind
- **WHEN** dependents are queried
- **THEN** each detailed result identifies its dependent symbol, the connecting edge kind, and its hop distance

### Requirement: Symbol reference resolution

The system SHALL resolve a symbol reference supplied as a short name, a qualified name, or a canonical identity to the symbol it denotes, and SHALL return a typed candidate set when a reference denotes more than one symbol rather than selecting one arbitrarily.

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

### Requirement: Calibrated output contract

The system SHALL return every query result with its provenance and freshness, SHALL identify each returned symbol by both its stable canonical identity and a human-readable name, SHALL represent an empty result as typed absence distinct from an unavailable or failed answer, SHALL order results deterministically for identical inputs, and SHALL offer the result as structured JSON.

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

### Requirement: Symbol search by name fragment

The system SHALL provide a CLI command `find` that returns the indexed symbols whose name contains a supplied fragment, matched case-insensitively over ASCII letters (a non-ASCII character matches exactly, case-sensitively), as a bounded result set, distinct from exact reference resolution — where a fragment matching no symbol is a typed-empty answer, not a failure.

#### Scenario: Fragment matches several symbols

- **GIVEN** several indexed symbols whose names share a common substring
- **WHEN** `find` is invoked with that substring
- **THEN** every symbol whose name contains the substring is returned

#### Scenario: Matching is case-insensitive

- **GIVEN** an indexed symbol whose name contains mixed-case ASCII letters
- **WHEN** `find` is invoked with the fragment in a different case
- **THEN** the symbol is still returned

#### Scenario: No match is typed absence

- **GIVEN** an indexed workspace
- **WHEN** `find` is invoked with a fragment no symbol's name contains
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Bounded and resumable answers

The system SHALL bound every result-bearing answer by default: a per-query result limit caps how many results are returned, and per-result content is capped to a content-size bound, each applied from a documented default when the caller requests no explicit bound.
A caller MAY request an explicit bound, and MAY request an unbounded answer explicitly, which the system SHALL honor.
Any truncation — of the result set or of a result's content — SHALL be disclosed in the answer.
A caller MAY request a positioned window into a result's content, and when the returned content is a proper part of the whole, the answer SHALL disclose which part it holds and the whole extent; a window requested beyond the content SHALL yield empty content with that disclosure, not a failure.
When a result set is truncated, the answer SHALL carry an opaque continuation token that deterministically resumes the remaining results, and a token presented against query parameters it was not issued for SHALL be rejected rather than silently resumed against the wrong results.

#### Scenario: Result set capped and truncation disclosed

- **GIVEN** a query whose results exceed the requested result limit
- **WHEN** the query runs at that limit
- **THEN** at most that many results are returned and the answer discloses that the result set was truncated

#### Scenario: A default bound applies when none is requested

- **GIVEN** a result-bearing query whose results exceed the documented default limit, with no explicit limit requested
- **WHEN** the query runs
- **THEN** at most the documented default number of results are returned and the answer discloses that the result set was truncated

#### Scenario: Windowed content is addressable and disclosed

- **GIVEN** a result whose content is longer than a requested content window into it
- **WHEN** the query runs for that window
- **THEN** exactly that window of the content is returned, and the answer discloses the window's position within the content and the content's whole extent

#### Scenario: Continuation resumes deterministically

- **GIVEN** a truncated result set and the continuation token from its answer
- **WHEN** the query is re-issued with that token
- **THEN** the next results resume from where the prior page ended, deterministically for identical inputs

#### Scenario: Content bounded with truncation disclosed

- **GIVEN** a result whose content exceeds the content-size bound
- **WHEN** the query runs
- **THEN** the content is capped to the bound and the answer discloses that the content was truncated

#### Scenario: Mismatched continuation token refused

- **GIVEN** a continuation token issued for one query
- **WHEN** it is presented with different query parameters
- **THEN** it is rejected as a usage error rather than resumed against the wrong results

#### Scenario: Impact detail rows are bounded and resumable

- **GIVEN** a subject whose direct dependents exceed the result limit
- **WHEN** the impact relation runs
- **THEN** at most the limit's rows are detailed, the truncation is disclosed with a continuation, and the beyond-bound aggregate and horizon disclosure accompany every page

## Technical Notes

- **Implementation**: `src/query/`, `src/cli.rs`
- **Dependencies**: symbol-identity, semantic-engine, code-graph
