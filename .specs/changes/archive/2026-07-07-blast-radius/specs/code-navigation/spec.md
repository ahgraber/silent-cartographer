# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Relationship trace

> Previously: the supported relations were `containers`, `contains`, and `references`.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), and `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound).
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes."

Serves: impact-before-change

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

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

## ADDED Requirements

### Requirement: Depth-bounded impact answer with an honest horizon

For a dependents query, the system SHALL return detailed results only up to a depth bound; each detailed result SHALL identify the dependent symbol, the kind of dependency edge that connected it, and its distance from the subject in hops; dependents beyond the bound SHALL be reported in aggregate — counts by edge kind and distance — up to a stated horizon; and the answer SHALL always distinguish between reach that ends within the bound, reach that extends beyond the bound, and reach whose aggregate is itself cut off at the horizon.

Serves: impact-before-change, honest-horizon

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
