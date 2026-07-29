# Code Graph — Delta: dependents-traversal-cost

## MODIFIED Requirements

### Requirement: Dependents traversal

Serves: responsive-blast-radius

> Previously: the requirement constrained only the answer's content — membership, uniqueness, connecting kind, and shortest distance — and said nothing about the work performed to produce it, so a traversal that re-derived the same answer many times over satisfied it.

Given a seed symbol, the system SHALL compute the seed's dependents as the symbols whose dependency edges — `uses`, `imports`, or `type_hierarchy` — lead to the seed directly or transitively, SHALL NOT propagate dependence through enclosure (`contains`), and SHALL report each dependent exactly once, carrying the kind of dependency edge that connected it and its distance from the seed in hops — the shortest such distance when several paths exist.

The work the traversal performs SHALL be bounded by the reachable set rather than by the depth bound: once no symbol remains newly reachable, raising the bound SHALL neither change the answer nor materially increase the time taken to produce it.

Given several seed symbols, the system SHALL compute their combined dependents so that each dependent is reported once at its shortest distance from any of the seeds, and the work performed SHALL be bounded by the combined reachable set rather than growing with the number of seeds.

#### Scenario: Direct dependent

- **GIVEN** a function that uses the seed symbol
- **WHEN** dependents of the seed are computed
- **THEN** the function is reported at distance one with kind `uses`

#### Scenario: Transitive dependent

- **GIVEN** a function that uses another function which in turn uses the seed
- **WHEN** dependents of the seed are computed
- **THEN** the outer function is reported at distance two

#### Scenario: Dependent via imports

- **GIVEN** a module whose use statement names the seed symbol
- **WHEN** dependents of the seed are computed
- **THEN** the module is reported with kind `imports`

#### Scenario: Dependent via trait implementation

- **GIVEN** a type that implements the seed trait
- **WHEN** dependents of the seed trait are computed
- **THEN** the implementing type is reported with kind `type_hierarchy`

#### Scenario: Enclosure never propagates dependence

- **GIVEN** a seed symbol whose containing module has no dependency edge to it
- **WHEN** dependents of the seed are computed
- **THEN** the containing module is not reported as a dependent by virtue of containment

#### Scenario: Multiple paths report the shortest distance

- **GIVEN** a symbol that reaches the seed both directly and through an intermediate symbol
- **WHEN** dependents of the seed are computed
- **THEN** that symbol is reported exactly once at distance one

#### Scenario: Cyclic dependencies terminate

- **GIVEN** two symbols that each use the other, one of them the seed
- **WHEN** dependents of the seed are computed
- **THEN** each symbol is reported at most once and the computation completes

#### Scenario: A closed reachable set does not pay for the remaining depth bound

- **GIVEN** a densely connected graph whose reachable set from the seed is closed well before the depth bound
- **WHEN** dependents of the seed are computed at that bound
- **THEN** the answer is produced within a bounded time rather than in time growing with the bound

#### Scenario: Raising the bound past closure changes nothing

- **GIVEN** a seed whose reachable set is closed at some distance
- **WHEN** dependents of the seed are computed at that distance and again at a much larger bound
- **THEN** the two answers are identical in membership, distances, connecting kinds, and order

#### Scenario: Several seeds cost no more than their combined reach

- **GIVEN** several seeds whose dependents overlap heavily
- **WHEN** their combined dependents are computed
- **THEN** each dependent is reported once at its shortest distance from any seed, and the answer is produced within a bounded time rather than in time growing with the number of seeds

#### Scenario: A shared dependent takes its shortest distance from any seed

- **GIVEN** a symbol reachable at distance one from one seed and at distance three from another
- **WHEN** the combined dependents of both seeds are computed
- **THEN** that symbol is reported exactly once, at distance one

#### Scenario: A seed reached through a saturated intermediate keeps its true distance

- **GIVEN** three seeds where the third reaches the other two only through an intermediate symbol that depends on both of them directly
- **WHEN** the combined dependents of the three seeds are computed
- **THEN** the third seed is reported exactly once, at distance two
