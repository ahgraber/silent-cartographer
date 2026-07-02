# Code Graph Specification

## Purpose

Defines the persistent store that unifies the syntax and semantic oracles into one graph: the guarded positional join, lossless symbol persistence, enclosure, staleness, reference attribution, and join-alignment accounting.

## Requirements

### Requirement: Guarded positional join

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the source text at that location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path.
When a semantic occurrence and the syntax at its location cannot be reconciled, the system SHALL surface the discrepancy and SHALL NOT persist it as a confident attribution.

#### Scenario: Aligned occurrence persisted

- **GIVEN** a semantic occurrence whose source location holds syntax naming the same symbol
- **WHEN** the join runs
- **THEN** the occurrence is persisted as an aligned attribution to that syntactic construct

#### Scenario: Text mismatch refused

- **GIVEN** a semantic occurrence whose source location does not spell the occurrence's expected symbol name
- **WHEN** the join runs
- **THEN** the occurrence is not persisted as an aligned attribution and the mismatch is surfaced

#### Scenario: Semantic occurrence without matching syntax

- **GIVEN** a semantic occurrence for which no syntactic construct exists at its location
- **WHEN** the join runs
- **THEN** the occurrence is recorded as unaligned rather than attributed to an unrelated construct

#### Scenario: Syntax without semantic resolution

- **GIVEN** a syntactic construct the semantic backend did not resolve to a symbol
- **WHEN** the join runs
- **THEN** the construct is retained with its structural information and is not assigned a fabricated identity

### Requirement: Lossless symbol persistence

The system SHALL persist each symbol's canonical identity, its occurrences, and its source span such that a later query returns the same identity, occurrence set, and the exact source text of the symbol's span.

#### Scenario: Symbol round-trips

- **GIVEN** a symbol that has been indexed and persisted
- **WHEN** it is retrieved by canonical identity
- **THEN** its occurrences and the exact source text of its span are returned unchanged

### Requirement: Enclosure is persisted

The system SHALL persist the enclosure relation between symbols such that, for any symbol, the construct that encloses it and the symbols it directly contains are retrievable.

#### Scenario: Enclosing construct retrievable

- **GIVEN** a method declared inside a Rust type
- **WHEN** the enclosure of the method is queried
- **THEN** the enclosing type is returned

#### Scenario: Direct children retrievable

- **GIVEN** a Rust module containing several items
- **WHEN** the module's direct contents are queried
- **THEN** exactly the items it directly contains are returned

### Requirement: Staleness reflects underlying change

The system SHALL mark a persisted result as stale whenever the sources it was derived from no longer match the indexed content, or the analyzer identity and version recorded as its provenance differ from the analyzer in effect.
While neither has changed, the result SHALL be reported as fresh.

#### Scenario: Unchanged sources stay fresh

- **GIVEN** an index whose sources and analyzer provenance are unchanged since indexing
- **WHEN** a result derived from it is returned
- **THEN** the result is reported as fresh

#### Scenario: Changed source marks stale

- **GIVEN** an index whose underlying source content has changed since indexing
- **WHEN** a result derived from the changed sources is returned
- **THEN** the result is reported as stale

#### Scenario: Changed analyzer version marks stale

- **GIVEN** an index recorded under one analyzer version
- **WHEN** the analyzer in effect is a different version
- **THEN** results from that index are reported as stale and flagged for reindex

### Requirement: Reference occurrences carry enclosing-declaration attribution

The system SHALL attribute every aligned reference occurrence to the nearest enclosing persisted declaration in the fresh syntax tree, and SHALL attribute an occurrence enclosed by no narrower declaration to its module — never to a fabricated declaration.

#### Scenario: Reference inside a method attributes to the method

- **GIVEN** an aligned reference occurrence located inside a method body
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to that method

#### Scenario: Reference inside a closure attributes to the declaring function

- **GIVEN** an aligned reference occurrence inside a closure defined within a function
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to the enclosing function, since the closure is not a persisted symbol

#### Scenario: Module-level reference attributes to the module

- **GIVEN** an aligned reference occurrence at module scope, enclosed by no narrower declaration
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to the module

### Requirement: Join alignment accounting

The system SHALL record, for every build, the number of semantic occurrences in each join outcome — aligned, text-mismatch, and semantic-only — and the number of unresolved syntax-only constructs, and SHALL make these counts retrievable; the three semantic-side counts SHALL sum to the total semantic occurrences the build processed.

#### Scenario: Outcome counts recorded

- **GIVEN** a build over sources that produce occurrences in more than one join outcome
- **WHEN** the build completes
- **THEN** each outcome count is recorded and retrievable alongside the index's provenance

#### Scenario: Counts conserve the occurrence total

- **GIVEN** a completed build
- **WHEN** the aligned, text-mismatch, and semantic-only counts are summed
- **THEN** the sum equals the total number of semantic occurrences the build processed

## Technical Notes

- **Implementation**: `src/graph/`
- **Dependencies**: symbol-identity, semantic-engine
