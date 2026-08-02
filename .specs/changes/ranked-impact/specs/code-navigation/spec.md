# Delta for Code Navigation

## ADDED Requirements

### Requirement: Ranked ordering of dependents

Under the default `ranked` ordering, the system SHALL order the detailed rows of every `dependents` trace and every `impact` answer by distance first, nearest first, and within equal distance by the dependent's codebase-wide structural importance, most important first.
The ordering SHALL NOT alter which symbols the answer contains, the depth bound, the beyond-bound aggregates, or the horizon disclosure.
When the result limit truncates the detailed rows, the rows returned SHALL be the first of the ordered sequence, so a bounded page holds the nearest, most important dependents the answer has to offer.

Serves: ranked-blast-radius

#### Scenario: Within a layer the widely-depended-upon dependent ranks first

- **GIVEN** an indexed symbol with two direct dependents, one that much of the codebase transitively depends on and one that nothing else depends on
- **WHEN** `trace` is invoked for it over the `dependents` relation under the `ranked` ordering
- **THEN** the widely-depended-upon dependent is returned before the other

#### Scenario: Distance outranks importance

- **GIVEN** an indexed symbol whose distance-2 reach includes a dependent more widely depended upon than any of its direct dependents
- **WHEN** `trace` is invoked for it over the `dependents` relation under the `ranked` ordering
- **THEN** every distance-1 row is returned before the first distance-2 row

#### Scenario: Impact answers order each layer by importance

- **GIVEN** a workspace whose diff changes several indexed symbols with dependents of differing codebase-wide importance at the same shortest distance
- **WHEN** `impact` is invoked under the `ranked` ordering
- **THEN** the detailed rows within each distance layer are ordered most important first

#### Scenario: A truncated page holds the head of the ordered sequence

- **GIVEN** a subject whose distance-1 dependents alone exceed the result limit
- **WHEN** `trace` is invoked over the `dependents` relation under the `ranked` ordering at that limit
- **THEN** the detailed rows are the most important distance-1 dependents, and the truncation is disclosed with a continuation

#### Scenario: Ordering leaves the aggregates untouched

- **GIVEN** a subject whose reach extends beyond the requested depth bound
- **WHEN** `trace` is invoked over the `dependents` relation under the `ranked` ordering
- **THEN** the beyond-bound aggregate counts and the horizon disclosure are identical to those of the same query under the `unranked` ordering

### Requirement: Dependents order selector

The system SHALL support, on `dependents` traces and `impact` assessments, a per-query order selector offering exactly two orderings: `ranked` (the default) — distance first, then structural importance within each layer — and `unranked` — derived only from the answer's own stable structural keys: distance, then dependency kind, then canonical identity.
The selector SHALL be accepted only where its orderings are defined — a `dependents` trace or an `impact` assessment — and an order requested anywhere else SHALL be rejected as a usage error before any traversal.
The selector SHALL NOT alter which symbols the answer contains.
The selector and the ranking model's version SHALL be part of the query identity a continuation token binds to, so a token issued under one ordering, or under a prior ranking model, is rejected rather than resumed against a differently-ordered sequence.

Serves: unranked-order, ranked-blast-radius

#### Scenario: No selector yields the ranked ordering

- **GIVEN** an indexed symbol with dependents of differing codebase-wide importance at the same distance
- **WHEN** `trace` is invoked over the `dependents` relation with no order requested
- **THEN** the rows within a distance layer are ordered most important first

#### Scenario: Unranked ordering on request

- **GIVEN** an indexed symbol with several dependents
- **WHEN** `trace` is invoked over the `dependents` relation with the `unranked` ordering requested
- **THEN** the rows are ordered by distance, then dependency kind, then canonical identity

#### Scenario: Both orderings return the same answer set

- **GIVEN** an indexed symbol with several dependents
- **WHEN** the same `dependents` trace is invoked once under each ordering
- **THEN** both answers contain exactly the same symbols with the same distances and kinds

#### Scenario: A continuation token binds its ordering

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented with the `unranked` ordering requested
- **THEN** it is rejected as a usage error rather than resumed

#### Scenario: A continuation token does not outlive its ranking model

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented after the ranking model's version has changed
- **THEN** it is rejected as a usage error rather than resumed against a differently-ordered sequence

#### Scenario: An order requested outside its relations is refused

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation with an order requested
- **THEN** the invocation is rejected as a usage error before any traversal, naming where the selector applies

### Requirement: Ordering disclosure

Every `dependents` trace and `impact` answer, a typed-empty answer included, SHALL disclose which ordering is in effect through a single structural field in the machine answer.
A `ranked` answer SHALL be presented, in the human rendering and the command's self-description alike, as ordered within each distance layer by a structural-importance heuristic rather than resolved semantic fact; an `unranked` answer SHALL NOT be presented as heuristic.
The disclosure SHALL accompany, never replace, the provenance and freshness labeling every answer carries.

Serves: honest-ordering-label

#### Scenario: Machine answer discloses the ordering in effect

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is returned as JSON under the `ranked` ordering
- **THEN** it carries a single structural field identifying the ordering as ranked

#### Scenario: Human render states the heuristic

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is rendered for a human under the `ranked` ordering
- **THEN** the rendering states that rows within a distance layer are ordered by a structural-importance heuristic

#### Scenario: Unranked answers are not presented as heuristic

- **GIVEN** a `dependents` query with one or more results
- **WHEN** the answer is returned under the `unranked` ordering
- **THEN** the machine answer identifies the ordering as unranked and neither it nor the human rendering presents the ordering as heuristic

#### Scenario: An empty answer keeps the disclosure

- **GIVEN** a `dependents` query whose subject has no dependents
- **WHEN** the answer is returned as JSON under the `ranked` ordering
- **THEN** the typed-empty answer still carries the field identifying the ordering in effect
