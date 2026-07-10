# Delta for Code Graph

## MODIFIED Requirements

### Requirement: Guarded positional join

> Previously: Python had no kind-scoped rule; every Python module occurrence refused under the default rule because scip-python names modules with an `__init__` descriptor terminal.

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the occurrence satisfies a named alignment rule's exact expectation.
The default rule is name-token equality: the source text at the location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path.
Kind-scoped rules extend the default rule, each scoped to the language whose constructs it reconciles.
For Rust, four: a crate-root module occurrence is accepted when the source token is the descriptor's own package name or the keyword `crate`; a reference occurrence of a desugared-operator method is accepted when its location holds the operator construct that method desugars from, per a closed correspondence; a module definition occurrence is accepted when its range spans the module's whole document; and a reference occurrence resolving to a type — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation's self type is that type, generic arguments aside.
For Python, one: a module occurrence is accepted when its source token, after any leading relative-import dots, equals a trailing component-run of the module's dotted namespace name — the full name and the bare terminal component included; a token spelling only leading components, or any other text, is not evidence for that module.
A language for which no kind-scoped rule has been established accepts occurrences under the default rule only, refusing the rest; rule families are added per language as calibration evidence justifies each one.
Every aligned attribution SHALL carry the rule that accepted it as provenance.
An occurrence satisfying no rule SHALL NOT be persisted as an aligned attribution; when a semantic occurrence and the syntax at its location cannot be reconciled under any rule, the system SHALL surface the discrepancy and SHALL NOT persist it as a confident attribution.

Serves: python-module-impact

<!-- modified-removes: Python occurrence outside the default rule stays refused -->
<!-- reworded to "Python occurrence outside every rule stays refused" now that the Python module-name kind-scoped rule exists; the refusal contract is preserved and generalized to require failing both rules. -->

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

#### Scenario: Non-module occurrence is outside the module-name rule

- **GIVEN** a Python occurrence resolving to a non-module symbol whose source location does not spell its name token
- **WHEN** the join runs
- **THEN** the module-name rule does not accept it and the occurrence is refused

#### Scenario: Python occurrence outside every rule stays refused

- **GIVEN** a Python semantic occurrence whose source location neither spells the symbol's name token nor satisfies the module-name rule
- **WHEN** the join runs
- **THEN** the occurrence is refused and surfaced as a discrepancy, never persisted as a confident attribution

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

> Previously: locality evidence was document-grained only — a reference in a document associated with more than one duplicate, or with none, was always typed duplicate-ambiguous; same-document twins therefore refused every group reference by construction.

When more than one distinct definition shares an identical resolved descriptor, the system SHALL attribute each definition occurrence to the definition at its own location; SHALL attribute a non-definition occurrence of that descriptor to one of the duplicates only when the occurrence's containing document is associated with exactly that one duplicate and the occurrence also satisfies the guarded join's alignment rules, except that an occurrence whose source token spells the package's own name rather than the descriptor's name — a token that denotes the package's library target regardless of where it sits — SHALL be attributed to the duplicate whose definition document the build system's authoritative target description names as the library target's root, and SHALL be recorded duplicate-ambiguous when no authoritative target description is available or it names no persisted duplicate; SHALL, when the occurrence's containing document is associated with more than one duplicate, attribute the occurrence to a duplicate by declaration scope — the innermost declaration enclosing the occurrence that contains at least one of the duplicates' definitions decides, and the occurrence is attributed to the single duplicate defined within it — only when exactly one duplicate's definition lies within that deciding declaration and the occurrence also satisfies the guarded join's alignment rules; every attribution SHALL carry its locality evidence as provenance; and SHALL record a non-definition occurrence for which no locality evidence discriminates a single duplicate — a containing document associated with no duplicate, or shared territory whose deciding declaration contains more than one duplicate or does not exist — as a typed duplicate-ambiguous outcome, never attributing it to any single duplicate arbitrarily.

Serves: twin-scope-precision

<!-- modified-removes: Reference in shared territory is typed ambiguous -->
<!-- superseded by declaration-scope locality: a shared-territory reference is no longer always ambiguous — it resolves to the single twin inside its deciding declaration when exactly one is enclosed ("Same-document reference inside exactly one twin's scope is attributed to it"), and stays ambiguous otherwise ("Same-document reference whose deciding scope holds several twins stays ambiguous"). See design.md § Scope-grained locality. -->

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

### Requirement: Module-level dependency edges (imports)

> Previously: unchanged contract text; no scenario covered a module importing another module by name (Python module references could not align before the module-name rule).

The system SHALL persist an `imports` dependency edge from a module to a symbol whenever an aligned reference occurrence of that symbol is attributed to the module itself rather than to any narrower declaration.

Serves: python-module-impact

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
