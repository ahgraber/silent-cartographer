# Delta for Code Graph

## MODIFIED Requirements

### Requirement: Guarded positional join

> Previously: the kind-scoped rules were stated as a fixed set of four with no language scoping; every scenario sampled Rust constructs.

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the occurrence satisfies a named alignment rule's exact expectation.
The default rule is name-token equality: the source text at the location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path.
Kind-scoped rules extend the default rule, each scoped to the language whose constructs it reconciles.
For Rust, four: a crate-root module occurrence is accepted when the source token is the descriptor's own package name or the keyword `crate`; a reference occurrence of a desugared-operator method is accepted when its location holds the operator construct that method desugars from, per a closed correspondence; a module definition occurrence is accepted when its range spans the module's whole document; and a reference occurrence resolving to a type — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation's self type is that type, generic arguments aside.
A language for which no kind-scoped rule has been established accepts occurrences under the default rule only, refusing the rest; rule families are added per language as calibration evidence justifies each one.
Every aligned attribution SHALL carry the rule that accepted it as provenance.
An occurrence satisfying no rule SHALL NOT be persisted as an aligned attribution; when a semantic occurrence and the syntax at its location cannot be reconciled under any rule, the system SHALL surface the discrepancy and SHALL NOT persist it as a confident attribution.

Serves: python-precise-locate, python-calibrated-answers

#### Scenario: Aligned occurrence persisted

- **GIVEN** a semantic occurrence whose source location holds syntax naming the same symbol
- **WHEN** the join runs
- **THEN** the occurrence is persisted as an aligned attribution to that syntactic construct

#### Scenario: Text mismatch refused

- **GIVEN** a semantic occurrence whose source location satisfies no alignment rule's expectation
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

#### Scenario: Crate-root reference accepted under its rule

- **GIVEN** a reference occurrence resolving to a crate-root descriptor at a source token spelling that crate's package name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the crate-root rule

#### Scenario: Desugared operator accepted under its rule

- **GIVEN** a reference occurrence of a method within the desugar correspondence, located at the operator construct it desugars from
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the operator-desugar rule

#### Scenario: Method outside the correspondence stays refused

- **GIVEN** a reference occurrence of a method not in the desugar correspondence, whose location does not spell its name token
- **WHEN** the join runs
- **THEN** the occurrence is refused, not accepted by any rule

#### Scenario: Module definition span accepted under its rule

- **GIVEN** a module definition occurrence whose range spans the module's whole document
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-span rule

#### Scenario: Whole-document span on a non-module stays refused

- **GIVEN** a non-module occurrence whose range spans a whole document
- **WHEN** the join runs
- **THEN** the module-span rule does not accept it

#### Scenario: Self keyword accepted within its own implementation

- **GIVEN** a reference occurrence resolving to a type or to an implementation of that type, located at a self-type keyword inside an implementation of that type
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the self-keyword rule

#### Scenario: Generic self types compare by base name

- **GIVEN** a reference occurrence whose expected type name carries generic arguments, located at a self-type keyword inside the implementation of that generic type
- **WHEN** the join runs
- **THEN** the occurrence is accepted under the self-keyword rule by comparing base names with generic arguments stripped

#### Scenario: Self keyword in a foreign implementation stays refused

- **GIVEN** a reference occurrence whose expected type differs from the self type of the implementation enclosing its location
- **WHEN** the join runs
- **THEN** the self-keyword rule does not accept it and the occurrence is refused

#### Scenario: Every acceptance carries its rule

- **GIVEN** attributions accepted under the default rule and under a kind-scoped rule in one build
- **WHEN** the aligned attributions are retrieved
- **THEN** each carries the rule that accepted it as provenance

#### Scenario: Python name token accepted under the default rule

- **GIVEN** a Python semantic occurrence whose source location spells the symbol's own name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the default rule

#### Scenario: Python occurrence outside the default rule stays refused

- **GIVEN** a Python semantic occurrence whose source location does not spell the symbol's name token, in the absence of any Python kind-scoped rule covering it
- **WHEN** the join runs
- **THEN** the occurrence is refused and surfaced as a discrepancy, never persisted as a confident attribution

### Requirement: Staleness reflects underlying change

> Previously: provenance drift covered the analyzer identity and version only; declared environment facts did not participate in staleness.

The system SHALL mark a persisted result as stale whenever the sources it was derived from no longer match the indexed content, or any element of its recorded provenance — the analyzer identity and version, or an environment fact the backend declared material — differs from the one in effect.
While none has changed, the result SHALL be reported as fresh.

Serves: python-calibrated-answers

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

#### Scenario: Changed environment marks stale

- **GIVEN** an index recorded with a declared interpreter environment as provenance
- **WHEN** the environment in effect no longer matches the recorded one
- **THEN** results from that index are reported as stale and flagged for reindex

### Requirement: Module-level dependency edges (imports)

> Previously: unchanged contract text; scenarios sampled Rust use statements only.

The system SHALL persist an `imports` dependency edge from a module to a symbol whenever an aligned reference occurrence of that symbol is attributed to the module itself rather than to any narrower declaration.

Serves: python-blast-radius

#### Scenario: Use statement produces an imports edge

- **GIVEN** a module containing a use statement naming a symbol from elsewhere
- **WHEN** the index is built
- **THEN** an `imports` edge from the module to that symbol is persisted

#### Scenario: Python import produces an imports edge

- **GIVEN** a Python module containing an import statement naming a symbol defined in another module
- **WHEN** the index is built
- **THEN** an `imports` edge from the importing module to that symbol is persisted

#### Scenario: Reference inside a declaration is not an import

- **GIVEN** a reference occurrence attributed to a function inside a module
- **WHEN** the index is built
- **THEN** the reference produces a `uses` edge from the function and no `imports` edge from the module

## REMOVED Requirements

### Requirement: Trait-implementation edges (type_hierarchy)

Removed because: generalized to the language-neutral "Declared subtype edges (type_hierarchy)" below, which subsumes its full contract and every scenario — Rust trait implementations remain one of the two declared-subtyping forms it covers.

## ADDED Requirements

### Requirement: Declared subtype edges (type_hierarchy)

The system SHALL persist a `type_hierarchy` edge from a subtype to the supertype it explicitly declares — a Rust type's declared trait implementation, or a Python class's declared base class — for every such declaration in the indexed sources whose subtype and supertype are both persisted symbols, including declarations whose supertype name carries generic or parameterized forms, and one edge per declared supertype when a declaration names several.

Serves: python-blast-radius

#### Scenario: Plain trait implementation produces an edge

- **GIVEN** a type with a declared implementation of a workspace trait
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to the trait is persisted

#### Scenario: Generic trait implementation produces an edge

- **GIVEN** a type with a declared implementation of a trait whose name carries generic parameters
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to that trait is persisted

#### Scenario: External trait implementation produces an edge

- **GIVEN** a workspace type with a declared implementation of a trait defined outside the workspace
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the type to the external trait's persisted symbol is persisted

#### Scenario: Python base class produces an edge

- **GIVEN** a Python class declaring a base class defined in the workspace
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge from the subclass to the base class is persisted

#### Scenario: Multiple bases each produce an edge

- **GIVEN** a Python class declaring more than one base class
- **WHEN** the index is built
- **THEN** a `type_hierarchy` edge is persisted from the subclass to each declared base

### Requirement: Build selects exactly one language backend

A build SHALL select the backend whose language the workspace's project manifest declares; when the workspace declares manifests for more than one supported language and no explicit selection is supplied, the build SHALL refuse with a typed error naming the selection mechanism; an explicit selection SHALL override detection; and the selected backend SHALL be recorded in the index's provenance.

Serves: python-precise-locate, python-calibrated-answers

#### Scenario: Single-language workspace selects its backend

- **GIVEN** a workspace declaring only a Python project manifest
- **WHEN** a build runs with no explicit language selection
- **THEN** the Python backend is selected and recorded in the index's provenance

#### Scenario: Two manifests without a selection refuse

- **GIVEN** a workspace declaring both a Rust and a Python project manifest
- **WHEN** a build runs with no explicit language selection
- **THEN** the build refuses with a typed error naming the explicit selection mechanism, and no index is persisted from the attempt

#### Scenario: Explicit selection overrides detection

- **GIVEN** a workspace declaring both a Rust and a Python project manifest
- **WHEN** a build runs with an explicit language selection
- **THEN** the build proceeds with the selected backend and records it in the index's provenance
