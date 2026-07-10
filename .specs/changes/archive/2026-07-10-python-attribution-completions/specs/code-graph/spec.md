# Delta for Code Graph

> Sequencing: this delta modifies the Guarded positional join as amended by the `module-scope-rules` change; that change syncs first.

## MODIFIED Requirements

### Requirement: Guarded positional join

> Previously: (post module-scope-rules) Python had one kind-scoped rule — the module-name rule over the occurrence's own span text; no alias bindings, no self-name idiom, no marker acceptance.

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

Serves: alias-aware-references, idiomatic-module-evidence

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
