# Delta for Code Navigation

## ADDED Requirements

### Requirement: Heuristic-grade answers are structurally labeled

The system SHALL carry, in every `tests` answer — in the machine answer and in the human rendering alike — a structural marker identifying the answer as derived from convention-based test classification rather than from resolved semantic fact; each returned site SHALL carry, as provenance, the convention rule that classified its enclosing declaration; and the marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
An empty `tests` answer asserts only that no convention-classified reference site was found — never that nothing tests the subject — and SHALL carry the marker like any other `tests` answer.
An ambiguous-reference or unresolved-subject answer terminates before classification is consulted and contains no classification-derived content, so it SHALL NOT carry the marker — the marker asserts a derivation, never merely the relation requested.
An answer of a relation derived only from resolved reference evidence SHALL NOT carry the heuristic-grade marker.

Serves: honest-heuristic-label

#### Scenario: Machine answer carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is returned as JSON
- **THEN** it carries a structural field marking the answer as convention-based classification

#### Scenario: Human render carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is rendered for a human
- **THEN** the rendering states that the results are convention-classified test code, not resolved semantic fact

#### Scenario: Each site carries its classification rule

- **GIVEN** a `tests` answer whose sites were classified under more than one convention rule
- **WHEN** the answer is returned as JSON
- **THEN** each site carries the convention rule that classified its enclosing declaration

#### Scenario: Resolved relations carry no heuristic marker

- **GIVEN** a `references` query against the same subject
- **WHEN** the answer is returned as JSON
- **THEN** it carries no heuristic-grade marker

#### Scenario: Unresolved subject carries no marker

- **GIVEN** a `tests` query whose subject resolves to no symbol
- **WHEN** the answer is returned as JSON
- **THEN** the typed absence carries no classification marker

#### Scenario: Ambiguous subject carries no marker

- **GIVEN** a `tests` query whose subject resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the ambiguous answer carries no classification marker

#### Scenario: Empty answer keeps the marker

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is returned as JSON
- **THEN** the empty answer carries the convention-based marker alongside its typed absence

#### Scenario: Empty human render states scoped absence

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is rendered for a human
- **THEN** the rendering presents the empty result as no convention-classified test reference found, not as proof that nothing tests the subject

#### Scenario: Heuristic marker composes with staleness

- **GIVEN** a `tests` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the heuristic-grade marker, each independently

## MODIFIED Requirements

### Requirement: Relationship trace

> Previously: the supported relations were `containers`, `contains`, `references`, `dependents`, `importers`, and `implementers`; no relation separated test call sites from other reference sites.

The system SHALL provide a CLI command `trace` that, given a subject symbol and a relation kind, returns the symbols standing in that relation to the subject.
Relation kinds follow a shared-root directional convention (e.g. `containers`/`contains`), and the supported relations are `containers` (the declaration that directly encloses a subject), `contains` (the symbols a subject directly contains), `references` (the sites that reference a subject, which for a type subject are its type-occurrences), `dependents` (the symbols that depend on the subject directly or transitively, subject to a depth bound), `importers` (the modules that import the subject), `implementers` (the types that declare the subject as a supertype — a trait's implementors or a base type's subtypes), and `tests` (the reference sites of a subject whose enclosing declaration is classified test code — the answer to "what test code exercises this symbol").
The command's self-description SHALL present `dependents` as impact assessment — the answer to "what could break if this symbol changes" — and SHALL present `tests` as convention-based classification rather than resolved semantic fact.

Serves: tests-for-symbol, locate-a-symbols-tests

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

#### Scenario: Trace tests of a Rust symbol

- **GIVEN** an indexed Rust symbol referenced both from a test-classified function and from a production function
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** exactly the reference sites whose enclosing declaration is classified test code are returned, and the production site is not

#### Scenario: Tests reach through a shared helper

- **GIVEN** an indexed symbol referenced only from a test-classified helper declaration
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** the helper's reference site is returned rather than an empty answer

#### Scenario: Module-scope test references count

- **GIVEN** an indexed symbol named by an import statement at module scope in a test-classified document
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** that module-scope reference site is returned, attributed to the document's module

#### Scenario: Trace tests of a Python symbol

- **GIVEN** an indexed Python function referenced from a declaration in a test-classified document
- **WHEN** `trace` is invoked for it over the `tests` relation
- **THEN** that reference site is returned with its location

#### Scenario: Empty relation is typed absence

- **GIVEN** an indexed symbol that stands in no instance of the requested relation
- **WHEN** `trace` is invoked for it over that relation
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer
