# Delta for code-navigation

## ADDED Requirements

### Requirement: Symbol search by name fragment

The system SHALL provide a CLI command `find` that returns the indexed symbols whose name contains a supplied fragment, matched case-insensitively over ASCII letters (a non-ASCII character matches exactly, case-sensitively), as a bounded result set, distinct from exact reference resolution — where a fragment matching no symbol is a typed-empty answer, not a failure.

Serves: locate-by-fragment

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

Serves: context-bounded-answers

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

## MODIFIED Requirements

### Requirement: Relationship trace

> Previously: the supported relations were `containers`, `contains`, `references`, and `dependents`.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), and `implementers` (the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes).
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes."

Serves: trace-reverse-structure

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
