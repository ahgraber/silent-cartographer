# Code Graph Specification

## Purpose

Defines the persistent store that unifies the syntax and semantic oracles into one graph: the guarded positional join, lossless symbol persistence, enclosure, staleness, reference attribution, and join-alignment accounting.

## Requirements

### Requirement: Guarded positional join

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the occurrence satisfies a named alignment rule's exact expectation.
The default rule is name-token equality: the source text at the location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path; a tuple-field index token is a name token for this purpose.
Kind-scoped rules extend the default rule, each scoped to the language whose constructs it reconciles.
For Rust, eight: a crate-root module occurrence is accepted when the source token is the descriptor's own package name or the keyword `crate`; a reference occurrence of a desugared-operator method is accepted when its location holds the operator construct that method desugars from, per a closed correspondence, each occurrence the construct carries accepted independently; a reference occurrence resolving to a range type is accepted at a range expression's operator token when the expression's shape corresponds to that type, per a closed shape correspondence; a module definition occurrence is accepted when its range spans the module's whole document; a reference occurrence resolving to a type — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation's self type is that type, generic arguments aside, and a reference occurrence resolving to a trait — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation implements that trait, generic arguments aside; a module occurrence at a `self` token inside a use-list is accepted when the enclosing use path names that module; a module occurrence at a path-start `self` token is accepted when the resolved module is the containing module; and a module occurrence at a `super` token is accepted when the resolved module is the containing module's parent — the containing module being the document's own module extended by any inline module blocks enclosing the location.
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

#### Scenario: Tuple-field index accepted under the default rule

- **GIVEN** a reference occurrence of a tuple field at a source token spelling that field's index
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the default rule

#### Scenario: Crate-root reference accepted under its rule

- **GIVEN** a reference occurrence resolving to a crate-root descriptor at a source token spelling that crate's package name
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the crate-root rule

#### Scenario: Desugared operator accepted under its rule

- **GIVEN** a reference occurrence of a method within the desugar correspondence, located at the operator construct it desugars from
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the operator-desugar rule

#### Scenario: Indexing occurrences accepted at both bracket tokens

- **GIVEN** two reference occurrences of the indexing method, one at each bracket token of a single index expression
- **WHEN** the join runs
- **THEN** both occurrences are persisted as aligned under the operator-desugar rule

#### Scenario: Method outside the correspondence stays refused

- **GIVEN** a reference occurrence of a method not in the desugar correspondence, whose location does not spell its name token
- **WHEN** the join runs
- **THEN** the occurrence is refused, not accepted by any rule

#### Scenario: Range literal accepted under its shape correspondence

- **GIVEN** a reference occurrence resolving to a range type, at the operator token of a range expression whose shape corresponds to that type
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the range-literal rule with that rule as provenance

#### Scenario: Range occurrence with a mismatched shape stays refused

- **GIVEN** a reference occurrence resolving to a range type, at the operator token of a range expression whose shape corresponds to a different range type
- **WHEN** the join runs
- **THEN** the range-literal rule does not accept it and the occurrence is refused

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

#### Scenario: Trait reference accepted at a self-type keyword within its implementation

- **GIVEN** a reference occurrence resolving to a trait or to an implementation of that trait, located at a self-type keyword inside an implementation of that trait
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the self-keyword rule

#### Scenario: Self keyword in a foreign implementation stays refused

- **GIVEN** a reference occurrence whose expected symbol matches neither the self type nor the implemented trait of the implementation enclosing its location
- **WHEN** the join runs
- **THEN** the self-keyword rule does not accept it and the occurrence is refused

#### Scenario: Use-list self token accepted for the path's module

- **GIVEN** a module occurrence at a `self` token inside a use-list, where the enclosing use path names that module
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the use-list-self rule with that rule as provenance

#### Scenario: Use-list self token for a different module stays refused

- **GIVEN** a module occurrence at a `self` token inside a use-list, where the enclosing use path names a different module
- **WHEN** the join runs
- **THEN** the use-list-self rule does not accept it and the occurrence is refused

#### Scenario: Path-start self token accepted for the containing module

- **GIVEN** a module occurrence at a path-start `self` token, where the resolved module is the containing module — at file level or inside an inline module block
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the self-name rule

#### Scenario: Path-start self token for a foreign module stays refused

- **GIVEN** a module occurrence at a path-start `self` token, where the resolved module is not the containing module
- **WHEN** the join runs
- **THEN** the self-name rule does not accept it and the occurrence is refused

#### Scenario: Super token accepted for the parent module

- **GIVEN** a module occurrence at a `super` token, where the resolved module is the containing module's parent
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the super-keyword rule with that rule as provenance

#### Scenario: Super token inside an inline module resolves from the inline chain

- **GIVEN** a module occurrence at a `super` token inside an inline module block, where the resolved module is the parent of the inline-extended containing module
- **WHEN** the join runs
- **THEN** the occurrence is persisted as aligned under the super-keyword rule

#### Scenario: Super token for a non-parent module stays refused

- **GIVEN** a module occurrence at a `super` token, where the resolved module is not the containing module's parent
- **WHEN** the join runs
- **THEN** the super-keyword rule does not accept it and the occurrence is refused

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

### Requirement: Per-symbol tier content

The system SHALL persist, for every in-workspace symbol whose definition span is persisted, content at fixed tiers derived from the symbol's own source: a signature tier — the declaration form without its body — and an interface tier — the signature together with the symbol's own documentation.
A symbol carrying no documentation SHALL have an interface tier equal to its signature tier, and a declaration with no distinct body SHALL have a signature tier equal to its full declaration.
A module defined by a document rather than an in-document declaration carries no declaration form in source, so its signature tier SHALL be its qualified name.
Tier content SHALL be extracted from the same build's fresh syntax tree that anchors the guarded join, and SHALL be persisted with the build, wholly superseded by each subsequent build.

#### Scenario: Documented Rust function tiers

- **GIVEN** an indexed Rust function carrying a doc comment and a body
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's declaration without its body, and the interface tier carries both the doc comment and the signature and not the body

#### Scenario: Undocumented symbol falls back to signature

- **GIVEN** an indexed symbol with no documentation of its own
- **WHEN** its interface tier is retrieved
- **THEN** it equals the symbol's signature tier

#### Scenario: Python function tiers carry the docstring

- **GIVEN** an indexed Python function with a docstring
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's header, and the interface tier carries the header together with the docstring

#### Scenario: Rust module interface carries its module documentation

- **GIVEN** an indexed Rust module whose document opens with inner doc comments
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module documentation

#### Scenario: Python module interface carries its module docstring

- **GIVEN** an indexed Python module whose document opens with a module docstring
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module docstring

#### Scenario: Declaration without a distinct body

- **GIVEN** an indexed declaration with no body distinct from its declaration form
- **WHEN** its signature tier is retrieved
- **THEN** it equals the full declaration

### Requirement: Module bodies span their documents

The system SHALL persist, for every in-workspace module symbol whose definition is a document itself rather than an in-document declaration, that whole defining document as the module's definition span, such that the module's body is the document's full source text, uniformly across supported languages; a module defined by an in-document module declaration SHALL keep that declaration's span as its body.

#### Scenario: Rust module body equals its document

- **GIVEN** an indexed Rust workspace
- **WHEN** a module symbol's body is retrieved
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Python module body equals its document

- **GIVEN** an indexed Python module whose definition occurrence is the zero-width span at its document's origin
- **WHEN** the module symbol is persisted and its body is retrieved
- **THEN** the returned text equals the module's document byte-for-byte

#### Scenario: Inline module declarations keep their declaration spans

- **GIVEN** an indexed Rust workspace containing an inline `mod` declaration with a body
- **WHEN** that module symbol's body is retrieved
- **THEN** the returned text equals the inline declaration's source text, not the whole document

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

The system SHALL record the schema version in every store it creates; a build over a store the system recognizes as its own whose recorded schema version differs from the version the binary writes SHALL replace the store and succeed; and every query against such a recognized, version-mismatched store SHALL refuse with an error naming the store's version, the expected version, and the recovery action — never surfacing a storage-level failure mid-operation.
A file not recognized as the system's own store is outside replacement's reach and is governed by the Store ownership recognition requirement.

#### Scenario: Build replaces an incompatible store

- **GIVEN** a store the system created, recorded under a different schema version than the binary writes
- **WHEN** the index is built
- **THEN** the build succeeds and the resulting store carries the current schema version

#### Scenario: Query refuses an incompatible store with guidance

- **GIVEN** a store the system created, recorded under a different schema version than the binary expects
- **WHEN** any query or status request runs against it
- **THEN** a typed error names the store's version, the expected version, and the recovery action, and no storage-level error surfaces

#### Scenario: Matching version operates normally

- **GIVEN** a store the system created, recorded under the schema version the binary expects
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

### Requirement: Per-symbol test classification

The system SHALL classify, at build time, every persisted in-workspace symbol as test code or not, from statically observable language-convention signals, and SHALL persist the classification with the build, wholly superseded by each subsequent build.
A symbol SHALL be classified test code exactly when a convention rule for the workspace's language accepts it, and a symbol no rule accepts SHALL be classified non-test.
For Rust, three rules: a test-attribute rule accepting a declaration bearing an attribute whose path's terminal segment is `test`; a test-configuration rule accepting a module gated to the test configuration and every symbol it transitively contains; and a test-directory rule accepting every symbol whose document lies under a directory named `tests` — the static reading of the integration-test layout, symmetric with the Python directory rule.
For Python, two rules following the test runners' file-collection conventions: a test-file rule accepting every symbol in a document whose file name is test-prefixed or test-suffixed (`test_*.py`, `*_test.py`), the conventional single-file test module `tests.py`, or the reserved fixture file `conftest.py`; and a test-directory rule accepting every symbol in a document lying under a directory named `tests`.
A declaration-name convention alone — a test-prefixed function in a document no rule accepts — SHALL NOT classify a symbol as test code, because the runner's own collection would not reach it and a confident false positive is worse than a disclosed miss.
Every test classification SHALL carry the convention rule that stamped it as provenance; rule families are per-language and are extended as calibration evidence justifies each one, so a stronger future rule can join the set without redefining the classification.
A consumer MUST treat the rule vocabulary as open, reading an unrecognized rule name as a valid classification rather than an error.
The classification SHALL NOT alter join alignment, reference attribution, or dependency-edge derivation.

#### Scenario: Test-attribute function classified

- **GIVEN** a Rust function bearing the plain test attribute
- **WHEN** the index is built
- **THEN** the function is classified test code with the test-attribute rule as provenance

#### Scenario: Composed test attribute classified

- **GIVEN** a Rust function bearing an async runtime's test attribute whose path ends in the test segment
- **WHEN** the index is built
- **THEN** the function is classified test code under the test-attribute rule

#### Scenario: Test-configured module classifies transitively

- **GIVEN** a Rust module gated to the test configuration, containing a helper function that bears no test attribute
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-configuration rule

#### Scenario: Test-configured module classifies across documents

- **GIVEN** a Rust module declaration gated to the test configuration, whose module body lives in its own document
- **WHEN** the index is built
- **THEN** the symbols in that document are classified test code under the test-configuration rule

#### Scenario: Integration-test directory classifies its helpers

- **GIVEN** a Rust helper function without a test attribute, in a document under the package's integration-test directory
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-directory rule

#### Scenario: Production symbol is non-test

- **GIVEN** a Rust function in a production document, bearing no test attribute and enclosed by no test-configured module
- **WHEN** the index is built
- **THEN** the function is classified non-test

#### Scenario: Python test file classifies all its symbols

- **GIVEN** a Python document whose file name matches the test-file convention, containing a helper function whose own name carries no test prefix
- **WHEN** the index is built
- **THEN** the helper is classified test code under the test-file rule

#### Scenario: Python tests directory classifies shared fixtures

- **GIVEN** a Python fixture module under a tests directory whose file name does not itself match the test-file convention
- **WHEN** the index is built
- **THEN** its symbols are classified test code under the test-directory rule

#### Scenario: Test-prefixed name outside test territory stays non-test

- **GIVEN** a Python function whose name carries a test prefix, in a production document no rule accepts
- **WHEN** the index is built
- **THEN** the function is classified non-test

#### Scenario: Near-miss file name stays non-test

- **GIVEN** a Python document whose file name merely begins with the word test (such as `testimony.py`) and matches no test-file name form
- **WHEN** the index is built
- **THEN** its symbols are classified non-test

#### Scenario: Every classification carries its rule

- **GIVEN** a build producing test classifications under more than one convention rule
- **WHEN** the classifications are retrieved
- **THEN** each carries the convention rule that stamped it as provenance

#### Scenario: Classification does not alter the graph

- **GIVEN** a test-classified function whose body references a production symbol
- **WHEN** the index is built
- **THEN** the reference is attributed and its dependency edge derived exactly as from a non-test declaration

### Requirement: Store ownership recognition

The system SHALL mark every index store it creates so that the store is recognizable as the system's own artifact independently of the store's schema version, and SHALL NOT write to, alter, or delete a file it does not recognize as a store it created.
An operation refused for lack of recognition SHALL leave the target file unchanged, and its error SHALL be typed — never a storage-level failure — naming the path and stating the recovery for a genuine but unrecognizable index (rebuild) separately from the recovery for an unrelated file (correct the path), never an unconditional instruction to delete.
A path whose contents cannot be examined at all SHALL be refused in the same category rather than read as recognized, as absent, or as a bare storage failure; its error SHALL name the path and the reason the examination failed, and SHALL NOT assert that the target is, or is not, a store the system created.
Creating a store SHALL NOT overwrite a file already occupying the path it builds at; that refusal SHALL leave the file unchanged and SHALL state removal only as conditional on the file being the system's own leftover from an interrupted build.

#### Scenario: Foreign database refused by build

- **GIVEN** a database file at the store path that the system did not create, holding its own data
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the file's contents are unchanged

#### Scenario: Version-coincident foreign database refused by build

- **GIVEN** a database file the system did not create whose version stamp happens to equal the schema version the binary writes
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the file's contents are unchanged

#### Scenario: Version-coincident foreign database refused by query

- **GIVEN** a database file the system did not create whose version stamp happens to equal the schema version the binary expects
- **WHEN** any query or status request runs against it
- **THEN** the request refuses with the typed ownership error and nothing is written into the file

#### Scenario: Non-database file refused

- **GIVEN** a file at the store path that is not a database at all
- **WHEN** the index is built or queried
- **THEN** the operation refuses with the typed ownership error, not a storage-level failure, and the file is unchanged

#### Scenario: Unmarked legacy store refused with rebuild guidance

- **GIVEN** an index store created by a system version that predates ownership marking
- **WHEN** any command that opens the store runs
- **THEN** the operation refuses, the error's rebuild branch names the recovery, and the file is unchanged

#### Scenario: Redirected path to a foreign database refused

- **GIVEN** a store path that resolves through a symlink to a database the system did not create
- **WHEN** the index is built
- **THEN** the build refuses with the typed ownership error and the link target is unchanged

#### Scenario: Recognized store operates normally

- **GIVEN** a store the system created carrying the expected schema version
- **WHEN** the index is built or queried
- **THEN** the operation proceeds with no ownership error

#### Scenario: Unexaminable path refused without an ownership claim

- **GIVEN** a store path naming something whose contents cannot be read at all, such as a directory
- **WHEN** the index is built or queried
- **THEN** the operation refuses in the ownership category, names the path and why the examination failed, claims neither that the target is nor that it is not the system's own store, and leaves the target unchanged

#### Scenario: An occupied build path is not overwritten

- **GIVEN** a file already occupying the path a new store would be built at
- **WHEN** the index is built
- **THEN** the build refuses in the ownership category, the occupying file is unchanged, and the refusal states removal only as conditional on the file being an interrupted build's leftover

### Requirement: Read operations create no store

For any query or status request naming a store path where no file exists, the system SHALL refuse with an error naming the build action as the remedy, and SHALL NOT create a file at that path.

#### Scenario: Query against a missing store creates nothing

- **GIVEN** a store path at which no file exists
- **WHEN** a query runs against it
- **THEN** the refusal names the build action as the remedy and no file exists at the path afterward

#### Scenario: Status against a missing store creates nothing

- **GIVEN** a store path at which no file exists
- **WHEN** a status request runs against it
- **THEN** the refusal names the build action as the remedy and no file exists at the path afterward

### Requirement: Stores record and disclose their workspace

The system SHALL record in every store it creates the identity of the workspace the store describes; every answer derived from a store whose recorded workspace identity differs from the workspace being queried SHALL carry a workspace-mismatch marker — in the machine answer and the human rendering alike — distinct from and composing with staleness; an answer from a store whose recorded identity matches SHALL carry no such marker; an answer for which the comparison cannot be evaluated SHALL disclose the workspace relationship as unknown, never presenting it as matched; and a build over a recognized store carrying the current schema version and recorded for a different workspace SHALL disclose both identities, proceed, and record the workspace the store now describes.

#### Scenario: Matching workspace carries no marker

- **GIVEN** a store built from the workspace being queried
- **WHEN** any query runs
- **THEN** the answer carries no workspace-mismatch marker

#### Scenario: Different workspace carries the marker

- **GIVEN** a store recorded for one workspace
- **WHEN** a query runs against it from a different workspace
- **THEN** the answer carries the workspace-mismatch marker in the machine answer and the human rendering

#### Scenario: Mismatch marker composes with staleness

- **GIVEN** a store recorded for a different workspace whose sources have also drifted from the indexed content
- **WHEN** a query runs against it
- **THEN** the answer carries the workspace-mismatch marker and the staleness flag, each independently

#### Scenario: Unavailable comparison disclosed as unknown

- **GIVEN** a store whose recorded workspace identity cannot be compared with the workspace being queried
- **WHEN** a query runs against it
- **THEN** the answer discloses the workspace relationship as unknown rather than carrying no marker

#### Scenario: Build over a different workspace's store discloses and re-records

- **GIVEN** a recognized store carrying the expected schema version, recorded for a different workspace
- **WHEN** the index is built from the current workspace
- **THEN** the build discloses both workspace identities, succeeds, and the resulting store records the workspace it was built from

## Technical Notes

- **Implementation**: `src/graph/`
- **Dependencies**: symbol-identity, semantic-engine
