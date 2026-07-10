# Code Graph Specification

## Purpose

Defines the persistent store that unifies the syntax and semantic oracles into one graph: the guarded positional join, lossless symbol persistence, enclosure, staleness, reference attribution, and join-alignment accounting.

## Requirements

### Requirement: Guarded positional join

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the occurrence satisfies a named alignment rule's exact expectation.
The default rule is name-token equality: the source text at the location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path.
Kind-scoped rules extend the default rule, each scoped to the language whose constructs it reconciles.
For Rust, four: a crate-root module occurrence is accepted when the source token is the descriptor's own package name or the keyword `crate`; a reference occurrence of a desugared-operator method is accepted when its location holds the operator construct that method desugars from, per a closed correspondence; a module definition occurrence is accepted when its range spans the module's whole document; and a reference occurrence resolving to a type — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation's self type is that type, generic arguments aside.
For Python, three: a module occurrence is accepted when its source token — or the smallest dotted construct enclosing its location — after any leading relative-import dots, equals a trailing component-run of the module's dotted namespace name, a token spelling only leading components remaining no evidence for that module; a module occurrence at a `__name__` or `__file__` token is accepted when the resolved module is the containing document's own module; and a module definition occurrence whose range is the empty span at its document's origin is accepted as that module's definition attribution.
An import-alias rule applies across languages to the alias-binding forms each language's syntax declares: a reference occurrence is accepted at a token spelling a name that the occurrence's containing document binds to the occurrence's resolved symbol through a declared alias-binding form; a language declaring no binding forms never accepts under this rule, and a binding outside the containing document is not evidence.
An occurrence whose span covers an alias-binding statement's whole binding text is evaluated at the binding's target token, accepted under whichever alignment rule that token's evidence satisfies for the occurrence's resolved symbol.
A language for which no kind-scoped rule has been established accepts occurrences under the default rule only, refusing the rest; rule families are added per language as calibration evidence justifies each one.
Every aligned attribution SHALL carry the rule that accepted it as provenance.
An occurrence satisfying no rule SHALL NOT be persisted as an aligned attribution; when a semantic occurrence and the syntax at its location cannot be reconciled under any rule, the system SHALL surface the discrepancy and SHALL NOT persist it as a confident attribution.

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

#### Scenario: Python module reference accepted under the module-name rule

- **GIVEN** a Python reference occurrence resolving to a module descriptor, at a source token spelling the terminal component of that module's dotted namespace name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-name rule with that rule as provenance

#### Scenario: Nested module accepted at trailing component-runs only

- **GIVEN** a Python reference occurrence resolving to a nested module (a dotted namespace of several components), at a source token spelling the namespace's terminal component or its full dotted name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-name rule, and an occurrence of the same module at a token spelling only a leading (non-terminal) component is refused

#### Scenario: Relative-import module reference accepted

- **GIVEN** a Python reference occurrence resolving to a module, at a relative-import token consisting of leading dots followed by a trailing component-run of the module's dotted namespace name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-name rule

#### Scenario: Module reference accepted through its enclosing dotted construct

- **GIVEN** a Python reference occurrence resolving to a module, whose span covers only a leading token but whose smallest enclosing dotted construct's text equals a trailing component-run of the module's dotted namespace name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-name rule

#### Scenario: Module self-name token accepted for its own module

- **GIVEN** a Python occurrence resolving to a module, at a `__name__` or `__file__` token inside a document whose own module is that module
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the self-name rule with that rule as provenance

#### Scenario: Self-name token for a foreign module stays refused

- **GIVEN** a Python occurrence resolving to a module, at a `__name__` token inside a document whose own module is a different module
- **WHEN** the join runs
- **THEN** the self-name rule does not accept it and the occurrence is refused

#### Scenario: Module origin marker accepted as its definition

- **GIVEN** a Python module definition occurrence whose range is the empty span at its document's origin
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the module-marker rule

#### Scenario: Zero-width occurrence of a non-module stays refused

- **GIVEN** a Python occurrence resolving to a non-module symbol whose range is the empty span at its document's origin
- **WHEN** the join runs
- **THEN** the module-marker rule does not accept it and the occurrence is refused

#### Scenario: Alias token accepted under its document's binding

- **GIVEN** a reference occurrence whose containing document declares an alias binding from a local name to the occurrence's resolved symbol, at a token spelling that local name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the import-alias rule with that rule as provenance

#### Scenario: Alias bound to a different symbol stays refused

- **GIVEN** a reference occurrence at a token spelling a local name the containing document binds to a symbol other than the occurrence's resolved symbol
- **WHEN** the join runs
- **THEN** the import-alias rule does not accept it and the occurrence is refused

#### Scenario: Alias binding outside the containing document is not evidence

- **GIVEN** a reference occurrence whose resolved symbol is alias-bound only in a document other than the occurrence's own
- **WHEN** the join runs
- **THEN** the import-alias rule does not accept it and the occurrence is refused

#### Scenario: Binding-site occurrence accepted at its binding's target token

- **GIVEN** a reference occurrence whose span covers an alias binding's whole binding text, where the binding's target token spells the occurrence's resolved symbol's name token
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the rule the target token's evidence satisfies

#### Scenario: Binding-site occurrence with a foreign target stays refused

- **GIVEN** a reference occurrence whose span covers an alias binding's whole binding text, where the binding's target token spells neither the occurrence's resolved symbol's name token nor any module-rule evidence form
- **WHEN** the join runs
- **THEN** the occurrence is refused

#### Scenario: Non-module occurrence is outside the module-name rule

- **GIVEN** a Python occurrence resolving to a non-module symbol whose source location does not spell its name token
- **WHEN** the join runs
- **THEN** the module-name rule does not accept it and the occurrence is refused

#### Scenario: Python occurrence outside every rule stays refused

- **GIVEN** a Python semantic occurrence whose source location satisfies no alignment rule — spelling neither the symbol's name token nor any module-rule or alias-rule evidence form
- **WHEN** the join runs
- **THEN** the occurrence is refused and surfaced as a discrepancy, never persisted as a confident attribution

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

The system SHALL mark a persisted result as stale whenever the sources it was derived from no longer match the indexed content, or any element of its recorded provenance — the analyzer identity and version, or an environment fact the backend declared material — differs from the one in effect.
While none has changed, the result SHALL be reported as fresh.

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

The system SHALL record, for every build, the number of semantic occurrences accepted under each named alignment rule and the number in each refusal outcome — text-mismatch, semantic-only, and duplicate-ambiguous — plus the number of unresolved syntax-only constructs, and SHALL make these counts retrievable; the per-rule acceptance counts and the refusal counts together SHALL sum to the total semantic occurrences the build processed.

#### Scenario: Outcome counts recorded

- **GIVEN** a build over sources that produce occurrences in more than one join outcome
- **WHEN** the build completes
- **THEN** each per-rule acceptance count and each refusal count is recorded and retrievable alongside the index's provenance

#### Scenario: Counts conserve the occurrence total

- **GIVEN** a completed build
- **WHEN** the per-rule acceptance counts and the text-mismatch, semantic-only, and duplicate-ambiguous counts are summed
- **THEN** the sum equals the total number of semantic occurrences the build processed

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

When more than one distinct definition shares an identical resolved descriptor, the system SHALL attribute each definition occurrence to the definition at its own location; SHALL attribute a non-definition occurrence of that descriptor to one of the duplicates only when the occurrence's containing document is associated with exactly that one duplicate and the occurrence also satisfies the guarded join's alignment rules, except that an occurrence whose source token spells the package's own name rather than the descriptor's name — a token that denotes the package's library target regardless of where it sits — SHALL be attributed to the duplicate whose definition document the build system's authoritative target description names as the library target's root, and SHALL be recorded duplicate-ambiguous when no authoritative target description is available or it names no persisted duplicate; SHALL, when the occurrence's containing document is associated with more than one duplicate, attribute the occurrence to a duplicate by declaration scope — the innermost declaration enclosing the occurrence that contains at least one of the duplicates' definitions decides, and the occurrence is attributed to the single duplicate defined within it — only when exactly one duplicate's definition lies within that deciding declaration and the occurrence also satisfies the guarded join's alignment rules; every attribution SHALL carry its locality evidence as provenance; and SHALL record a non-definition occurrence for which no locality evidence discriminates a single duplicate — a containing document associated with no duplicate, or shared territory whose deciding declaration contains more than one duplicate or does not exist — as a typed duplicate-ambiguous outcome, never attributing it to any single duplicate arbitrarily.

#### Scenario: Definition occurrences attach to their own duplicate

- **GIVEN** two distinct definitions sharing an identical resolved descriptor
- **WHEN** the join runs
- **THEN** each definition occurrence is attributed to the definition at its own location

#### Scenario: Reference in one duplicate's territory is attributed to it

- **GIVEN** a non-definition occurrence of a duplicated descriptor whose containing document is associated with exactly one of the duplicates
- **WHEN** the join runs
- **THEN** the occurrence is attributed to that duplicate and the attribution carries its locality evidence as provenance

#### Scenario: Locality does not bypass the guarded join

- **GIVEN** a non-definition occurrence in a document associated with exactly one duplicate, whose source text satisfies no alignment rule's expectation
- **WHEN** the join runs
- **THEN** the occurrence is refused by the guarded join and is not attributed to the duplicate

#### Scenario: Package-name reference resolves to the library target

- **GIVEN** several crate-root definitions sharing an identical resolved descriptor, and an authoritative target description naming the library target's root document
- **WHEN** the join processes a reference occurrence of that descriptor whose source token spells the package name rather than the descriptor's name
- **THEN** the occurrence is attributed to the duplicate defined at the library target's root — never to the containing document's own crate root — and the attribution carries the target-description evidence as provenance

#### Scenario: Package-name reference without target metadata is typed ambiguous

- **GIVEN** several crate-root definitions sharing an identical resolved descriptor, and no authoritative target description available
- **WHEN** the join processes a reference occurrence of that descriptor whose source token spells the package name
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome, not attributed to the containing document's own crate root

#### Scenario: Reference outside every duplicate's territory is typed ambiguous

- **GIVEN** a non-definition occurrence of a duplicated descriptor whose containing document is associated with none of the duplicates
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Same-document reference inside exactly one twin's scope is attributed to it

- **GIVEN** several definitions sharing an identical resolved descriptor within one document, and a non-definition occurrence of that descriptor whose innermost enclosing declaration containing any of the definitions contains exactly one of them
- **WHEN** the join runs
- **THEN** the occurrence is attributed to that single duplicate and the attribution carries the declaration-scope evidence as locality provenance

#### Scenario: Same-document reference whose deciding scope holds several twins stays ambiguous

- **GIVEN** several definitions sharing an identical resolved descriptor within one document, and a non-definition occurrence whose innermost enclosing declaration containing any of the definitions contains more than one of them
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Same-document reference enclosed by no twin-bearing declaration stays ambiguous

- **GIVEN** several definitions sharing an identical resolved descriptor at the top level of one document, and a non-definition occurrence elsewhere in that document none of whose enclosing declarations contains any of the definitions
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Scope locality does not bypass the guarded join

- **GIVEN** a non-definition occurrence inside exactly one twin's scope whose source text satisfies no alignment rule's expectation
- **WHEN** the join runs
- **THEN** the occurrence is refused by the guarded join and is not attributed to the duplicate

#### Scenario: Unduplicated descriptors are unaffected

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the join runs
- **THEN** its non-definition occurrences are attributed through the ordinary guarded join, never marked duplicate-ambiguous

### Requirement: Builds wholly supersede prior derived state

After a build completes, the store SHALL describe exactly that build's sources: every persisted symbol, occurrence, edge, and discrepancy row derives from the completed build, no row from any prior build remains, and a build that fails SHALL leave the prior build's state intact and authoritative.

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the persisted symbol, occurrence, and edge row sets are identical to the first build's

#### Scenario: Rows for vanished entities do not linger

- **GIVEN** a store built from sources containing a symbol
- **WHEN** the sources no longer contain that symbol and the store is rebuilt
- **THEN** no row for the vanished symbol remains retrievable

### Requirement: Duplicated descriptors are disclosed

The system SHALL make retrievable, for every build, the duplicated-descriptor groups the build encountered — each group identifying the shared descriptor and the definitions that share it — and the build's summary accounting SHALL disclose the presence of duplicated descriptors alongside the join-outcome counts.

#### Scenario: Duplicated groups are retrievable

- **GIVEN** a build over sources containing at least one duplicated descriptor
- **WHEN** the duplicated-descriptor groups are requested
- **THEN** each group is returned identifying its shared descriptor and the definitions that share it

#### Scenario: No duplicates is typed absence

- **GIVEN** a build over sources containing no duplicated descriptors
- **WHEN** the duplicated-descriptor groups are requested
- **THEN** a definite empty set is returned, distinct from an unavailable or failed answer

### Requirement: Join discrepancies are inspectable

For every semantic occurrence whose join outcome is not aligned, the system SHALL persist the discrepancy's location, outcome kind, expected symbol name, and the source text found at that location, wholly superseded by each subsequent build; SHALL make that detail retrievable through the CLI; the default listing SHALL be a bounded summary whose aggregation covers the full discrepancy set and SHALL state when entries are withheld by the bound; and an explicit request SHALL return the complete detail.

#### Scenario: Discrepancy detail persisted and retrievable

- **GIVEN** a build that produces at least one non-aligned occurrence
- **WHEN** the build completes
- **THEN** the occurrence's location, outcome kind, expected symbol name, and found source text are retrievable

#### Scenario: Truncation is typed and the summary covers the full set

- **GIVEN** more persisted discrepancies than the bounded listing displays
- **WHEN** the default listing is requested
- **THEN** the listing states that it is truncated and its aggregate summary reflects every persisted discrepancy, not only the displayed entries

#### Scenario: Explicit request returns everything

- **GIVEN** more persisted discrepancies than the bounded listing displays
- **WHEN** the complete detail is explicitly requested
- **THEN** every persisted discrepancy is returned

#### Scenario: Discrepancies are superseded per build

- **GIVEN** discrepancies persisted by a prior build
- **WHEN** a new build completes
- **THEN** only the new build's discrepancies are retrievable

### Requirement: Incompatible index stores are replaced or refused, never half-used

The system SHALL record the schema version in every store it creates; a build over a store whose recorded schema version differs from the version the binary writes SHALL replace the store and succeed; and every query against such a store SHALL refuse with an error naming the store's version, the expected version, and the recovery action — never surfacing a storage-level failure mid-operation.

#### Scenario: Build replaces an incompatible store

- **GIVEN** a store recorded under a different schema version than the binary writes
- **WHEN** the index is built
- **THEN** the build succeeds and the resulting store carries the current schema version

#### Scenario: Query refuses an incompatible store with guidance

- **GIVEN** a store recorded under a different schema version than the binary expects
- **WHEN** any query or status request runs against it
- **THEN** a typed error names the store's version, the expected version, and the recovery action, and no storage-level error surfaces

#### Scenario: Matching version operates normally

- **GIVEN** a store recorded under the schema version the binary expects
- **WHEN** any command runs against it
- **THEN** it operates normally with no version error

### Requirement: Declaration-level dependency edges (uses)

The system SHALL persist a `uses` dependency edge from a declaration to a symbol whenever an aligned reference occurrence of that symbol is attributed to that declaration, and SHALL derive dependency edges only from aligned occurrences — an occurrence the join refused contributes no edge.
The `uses` edge asserts reference-grade dependency: any mention of the symbol within the declaration's body — a call, a type usage, a constant read — establishes it; invocation is not required.

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

#### Scenario: Use statement produces an imports edge

- **GIVEN** a module containing a use statement naming a symbol from elsewhere
- **WHEN** the index is built
- **THEN** an `imports` edge from the module to that symbol is persisted

#### Scenario: Python import produces an imports edge

- **GIVEN** a Python module containing an import statement naming a symbol defined in another module
- **WHEN** the index is built
- **THEN** an `imports` edge from the importing module to that symbol is persisted

#### Scenario: Python module import produces a module-to-module edge

- **GIVEN** a Python module containing an import statement naming another module
- **WHEN** the index is built
- **THEN** an `imports` edge from the importing module to the imported module's symbol is persisted

#### Scenario: Reference inside a declaration is not an import

- **GIVEN** a reference occurrence attributed to a function inside a module
- **WHEN** the index is built
- **THEN** the reference produces a `uses` edge from the function and no `imports` edge from the module

### Requirement: Declared subtype edges (type_hierarchy)

The system SHALL persist a `type_hierarchy` edge from a subtype to the supertype it explicitly declares — a Rust type's declared trait implementation, or a Python class's declared base class — for every such declaration in the indexed sources whose subtype and supertype are both persisted symbols, including declarations whose supertype name carries generic or parameterized forms, and one edge per declared supertype when a declaration names several.

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

### Requirement: Dependents traversal

Given a seed symbol, the system SHALL compute the seed's dependents as the symbols whose dependency edges — `uses`, `imports`, or `type_hierarchy` — lead to the seed directly or transitively, SHALL NOT propagate dependence through enclosure (`contains`), and SHALL report each dependent exactly once, carrying the kind of dependency edge that connected it and its distance from the seed in hops — the shortest such distance when several paths exist.

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

## Technical Notes

- **Implementation**: `src/graph/`
- **Dependencies**: symbol-identity, semantic-engine
