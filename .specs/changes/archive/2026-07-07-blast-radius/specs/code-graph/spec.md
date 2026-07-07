# Delta for Code Graph

## ADDED Requirements

### Requirement: Declaration-level dependency edges (uses)

The system SHALL persist a `uses` dependency edge from a declaration to a symbol whenever an aligned reference occurrence of that symbol is attributed to that declaration, and SHALL derive dependency edges only from aligned occurrences — an occurrence the join refused contributes no edge.
The `uses` edge asserts reference-grade dependency: any mention of the symbol within the declaration's body — a call, a type usage, a constant read — establishes it; invocation is not required.

Serves: trustworthy-dependency-edges, impact-before-change

#### Scenario: Call produces a uses edge

- **GIVEN** a function whose body calls another function
- **WHEN** the index is built
- **THEN** a `uses` edge from the calling function to the called function is persisted

#### Scenario: Type mention produces a uses edge

- **GIVEN** a function whose body mentions a type without calling anything on it
- **WHEN** the index is built
- **THEN** a `uses` edge from the function to the type is persisted

#### Scenario: Refused occurrence contributes no edge

- **GIVEN** a semantic occurrence the join refused under every alignment rule
- **WHEN** the index is built
- **THEN** no dependency edge is derived from that occurrence

### Requirement: Module-level dependency edges (imports)

The system SHALL persist an `imports` dependency edge from a module to a symbol whenever an aligned reference occurrence of that symbol is attributed to the module itself rather than to any narrower declaration.

Serves: trustworthy-dependency-edges, impact-before-change

#### Scenario: Use statement produces an imports edge

- **GIVEN** a module containing a use statement naming a symbol from elsewhere
- **WHEN** the index is built
- **THEN** an `imports` edge from the module to that symbol is persisted

#### Scenario: Reference inside a declaration is not an import

- **GIVEN** a reference occurrence attributed to a function inside a module
- **WHEN** the index is built
- **THEN** the reference produces a `uses` edge from the function and no `imports` edge from the module

#### Scenario: Module defined in several documents imports from each

- **GIVEN** a module symbol whose definition occurrences span more than one document, each document containing a module-scope reference
- **WHEN** the index is built
- **THEN** an `imports` edge is persisted for the reference in every such document

### Requirement: Trait-implementation edges (type_hierarchy)

The system SHALL persist a `type_hierarchy` edge from an implementing type to the implemented trait for every explicitly declared trait implementation in the indexed sources whose type and trait are both persisted symbols, including implementations whose trait name carries generic parameters.

Serves: trustworthy-dependency-edges, impact-before-change

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

### Requirement: Dependents traversal

Given a seed symbol, the system SHALL compute the seed's dependents as the symbols whose dependency edges — `uses`, `imports`, or `type_hierarchy` — lead to the seed directly or transitively, SHALL NOT propagate dependence through enclosure (`contains`), and SHALL report each dependent exactly once, carrying the kind of dependency edge that connected it and its distance from the seed in hops — the shortest such distance when several paths exist.

Serves: impact-before-change

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
