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

The system SHALL persist, for every in-workspace symbol whose definition span is persisted, content at three fixed tiers over the symbol's own source: a signature tier — the declaration form without its body — an interface tier — the signature together with the symbol's own documentation — and a body tier — the definition span's source text.
The signature and interface tiers are derived from the source; the body tier is the source, verbatim.
A symbol carrying no documentation SHALL have an interface tier equal to its signature tier, and a declaration with no distinct body SHALL have a signature tier equal to its full declaration.
A module defined by a document rather than an in-document declaration carries no declaration form in source, so its signature tier SHALL be its qualified name.
Tier content SHALL be taken from the sources as they were when the build ran, SHALL be persisted with the build, and SHALL be wholly superseded by each subsequent build.

#### Scenario: Documented Rust function tiers

- **GIVEN** an indexed Rust function carrying a doc comment and a body
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's declaration without its body, and the interface tier carries both the doc comment and the signature and not the body

#### Scenario: The body tier is the definition span verbatim

- **GIVEN** an indexed symbol whose definition span is persisted
- **WHEN** its body tier is retrieved
- **THEN** it is the source text of that span, unaltered

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
- **THEN** results from that index are reported as stale

#### Scenario: Changed environment marks stale

- **GIVEN** an index recorded with a declared interpreter environment as provenance
- **WHEN** the environment in effect no longer matches the recorded one
- **THEN** results from that index are reported as stale

### Requirement: Reference occurrences carry enclosing-declaration attribution

The system SHALL attribute every aligned reference occurrence to the nearest persisted declaration that encloses it in the indexed source, and SHALL attribute an occurrence enclosed by no narrower declaration to its module — never to a fabricated declaration.
Where more than one persisted definition shares a location, the system SHALL NOT attribute an occurrence to any of them, and SHALL attribute it as though no persisted declaration enclosed it.
The work performed to resolve one occurrence's attribution SHALL be bounded by that occurrence's enclosing-declaration chain, and SHALL NOT grow with the number of definitions the build persists.

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

#### Scenario: An ambiguous enclosing location attributes to no declaration

- **GIVEN** an aligned reference occurrence whose nearest enclosing declaration's location holds the definitions of two distinct persisted symbols
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to its module rather than to either symbol

#### Scenario: A larger workspace does not cost more per attribution

- **GIVEN** two workspaces with the same enclosing-declaration chains, one persisting many times as many definitions as the other
- **WHEN** references are attributed in each
- **THEN** the work performed per attribution is the same in both

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

When two or more distinct definitions share one resolved descriptor, they are duplicates, and the system SHALL NOT guess which duplicate a reference to them points to.
This requirement covers references that an alignment rule accepts; a reference that no alignment rule accepts SHALL be attributed to no duplicate.
Each duplicate SHALL keep its own definition location, and no duplicate SHALL be given another duplicate's location.
A reference written with the package's name SHALL be attributed to the library's duplicate, in whatever file the reference appears.
This covers a reference that is the package's name itself, and the last name in a path that starts with the package's name, such as `load` in `libbin::util::load()`.
If the package's unit list is unavailable, or none of the duplicates belongs to the library, such a reference SHALL be recorded duplicate-ambiguous.
Any other reference SHALL be attributed to a duplicate only in one of these cases:

- the file that holds the reference belongs to the unit (for Rust, the library, a binary, a test, or an example) of exactly one duplicate;
- the file belongs to the units of several duplicates, and, moving outward from the reference, the first declaration that holds any duplicate holds exactly one.

Every other reference SHALL be recorded duplicate-ambiguous and attributed to no duplicate.

#### Scenario: Two duplicates keep separate definition locations

- **GIVEN** a library file `src/lib.rs` and a binary file `src/bin/tool.rs` that each define `fn load()` in one package
- **WHEN** the workspace is built
- **THEN** the library's `load` is defined at its line in `src/lib.rs`, the binary's `load` at its line in `src/bin/tool.rs`, and neither is given the other's location

#### Scenario: A reference in one duplicate's unit is attributed to it

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of exactly one of them
- **WHEN** the workspace is built
- **THEN** the reference is attributed to that duplicate

#### Scenario: A reference in one duplicate's unit is not attributed if no alignment rule accepts it

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of exactly one of them, and no alignment rule accepts the reference
- **WHEN** the workspace is built
- **THEN** the reference is not attributed to that duplicate

#### Scenario: A reference by package name is attributed to the library

- **GIVEN** a package whose library and binary each have a root module, whose unit list is available, and a reference that is the package's name
- **WHEN** the workspace is built
- **THEN** the reference is attributed to the library's root module, never to the root module of the unit that holds the reference

#### Scenario: A path that starts with the package name is attributed to the library

- **GIVEN** a Cargo package `libbin` with a library and a binary `tool`, which each declare `mod util { pub fn load() {} }`, and the binary's `main` calls `util::load()` and `libbin::util::load()`, and the package's unit list is available
- **WHEN** the workspace is built
- **THEN** the call `libbin::util::load()` is attributed to the library's `load`, the call `util::load()` is attributed to the binary's `load`, and the references of the library's `load` include the call at `libbin::util::load()`

#### Scenario: A reference by package name with no unit list is ambiguous

- **GIVEN** a package whose library and binary each have a root module, whose unit list is unavailable, and a reference that is the package's name
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous, not attributed to the root module of the unit that holds the reference

#### Scenario: A reference outside every duplicate's unit is ambiguous

- **GIVEN** a reference to duplicates, in a file that belongs to the unit of none of them
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference in a declaration that holds exactly one duplicate is attributed to it

- **GIVEN** duplicates defined in one file, and a reference in that file where, moving outward from the reference, the first declaration that holds any duplicate holds exactly one
- **WHEN** the workspace is built
- **THEN** the reference is attributed to that duplicate

#### Scenario: A reference in a declaration that holds several duplicates is ambiguous

- **GIVEN** duplicates defined in one file, and a reference in that file where, moving outward from the reference, the first declaration that holds any duplicate holds more than one
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference outside every declaration that holds a duplicate is ambiguous

- **GIVEN** duplicates defined at the top level of one file, and a reference elsewhere in that file that no declaration holding a duplicate encloses
- **WHEN** the workspace is built
- **THEN** the reference is recorded duplicate-ambiguous and attributed to no duplicate

#### Scenario: A reference in a declaration that holds one duplicate is not attributed if no alignment rule accepts it

- **GIVEN** a reference inside a declaration that holds exactly one duplicate, and no alignment rule accepts the reference
- **WHEN** the workspace is built
- **THEN** the reference is not attributed to that duplicate

#### Scenario: A descriptor with one definition has no ambiguous references

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the workspace is built
- **THEN** no reference to it is recorded duplicate-ambiguous

### Requirement: Builds wholly supersede prior derived state

After a build completes, the store SHALL describe exactly that build's sources: every symbol, occurrence, edge, and discrepancy the store holds derives from the completed build, nothing from any prior build remains, and a build that fails SHALL leave the prior build's state intact and authoritative.

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the symbols, occurrences, and edges the store holds are identical to the first build's

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

The system SHALL record the schema version in every store it creates; a build over a store the system recognizes as its own whose recorded schema version differs from the version the binary writes SHALL replace the store and succeed; and every query against such a recognized, version-mismatched store SHALL refuse with an error naming the store's version, the expected version, and the recovery that applies — rebuilding when the store is older than the binary, and upgrading the binary or using another store path when the store is newer — never surfacing a storage-level failure mid-operation.
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

#### Scenario: A query against a newer store names the upgrade

- **GIVEN** a store the system recognizes as its own, recorded under a schema version newer than the one the binary writes
- **WHEN** a query runs against it
- **THEN** the query is refused with an error naming both versions and directing the user to upgrade the binary or use another store path, never to rebuild

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

### Requirement: Source discovery excludes undecodable files

Source discovery SHALL exclude every discovered file whose bytes are not valid UTF-8 or whose path is not valid Unicode, SHALL report each exclusion in a diagnostic identifying that file, and SHALL complete over the files that remain.
An excluded file SHALL contribute nothing to the syntax layer, and a semantic occurrence the analyzer reports in it SHALL be recorded under the semantic-only refusal outcome rather than aligned or dropped.
A symbol every one of whose occurrences lands in an excluded file SHALL be reported absent by every query, never as a found result carrying no content.
The discovered set SHALL be a function of the workspace alone and not of the operation asking for it, so a workspace holding an undecodable file presents the same set to a build as to a currency check.
A discovered file that cannot be read for any reason other than its encoding SHALL fail the invocation.

#### Scenario: An undecodable file does not stop the build

- **GIVEN** a workspace holding one source file whose bytes are not valid UTF-8 alongside decodable sources
- **WHEN** the index is built
- **THEN** the build completes and the index holds no symbol from the undecodable file

#### Scenario: Symbols from the other files are unaffected

- **GIVEN** the same workspace
- **WHEN** a symbol defined in a decodable file is requested
- **THEN** it is returned exactly as it would be from a workspace holding no undecodable file

#### Scenario: The exclusion names the file

- **GIVEN** the same workspace
- **WHEN** the index is built
- **THEN** a diagnostic names the excluded file, and the invocation still reports success

#### Scenario: Semantic coverage of an excluded file is refused, not aligned

- **GIVEN** a workspace whose analyzer reports occurrences in a file source discovery excluded
- **WHEN** the index is built
- **THEN** each of those occurrences is counted under the semantic-only refusal outcome, and the alignment accounting still sums to the total occurrences processed

#### Scenario: A symbol found only in an excluded file is reported absent, not empty

- **GIVEN** a workspace whose analyzer reports a symbol whose every occurrence is in a file source discovery excluded
- **WHEN** that symbol is requested by name
- **THEN** it is reported absent, not returned as a found result with no content

#### Scenario: The currency check sees the same discovered set

- **GIVEN** an index built over a workspace holding an undecodable file, with nothing since changed
- **WHEN** a build runs again
- **THEN** it reports the index already current and does no work

#### Scenario: An unreadable file still fails the invocation

- **GIVEN** a workspace holding a discovered source file that cannot be read for a reason other than its encoding
- **WHEN** the index is built
- **THEN** the build fails and names the file, rather than excluding it

#### Scenario: A file whose path is not valid Unicode is excluded, not fatal

- **GIVEN** a workspace holding a discovered source file whose path is not valid Unicode, alongside decodable sources
- **WHEN** the index is built
- **THEN** the build completes, the non-Unicode-path file is excluded, and its decodable siblings are indexed

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

### Requirement: Semantic corpus

The system SHALL derive, at build time, a semantic corpus of passages over the in-workspace symbols whose tier content is persisted: a symbol that contains no other corpus-contributing symbol contributes one passage derived from its own source content together with its identity, and a symbol that contains other corpus-contributing symbols contributes one passage derived from its interface tier together with its identity.
A symbol whose every persisted content tier is exactly its own name token SHALL contribute no passage: such content carries nothing beyond the symbol's identity, and name-only passages would displace content-bearing candidates in every answer.
Containment counts contributing symbols only, so a symbol enclosing nothing but name-only symbols is a leaf: no nested passage exists whose content its body could double-count.
No passage SHALL derive from the full body of a symbol that contains other corpus-contributing symbols, and every corpus-contributing symbol SHALL appear in the corpus exactly once.
Corpus derivation SHALL be deterministic: identical build inputs yield identical passages.

#### Scenario: Leaf declaration contributes its own content

- **GIVEN** an indexed function that contains no other corpus-contributing symbol
- **WHEN** the index is built
- **THEN** the corpus holds exactly one passage for the function, derived from its own source content and identity

#### Scenario: Container contributes its interface only

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type's passage derives from its interface tier, and no passage carries the methods' bodies through the type

#### Scenario: Module passage excludes member bodies

- **GIVEN** an indexed module whose document opens with module documentation and contains several declarations
- **WHEN** the index is built
- **THEN** the module's passage derives from its interface tier, not from the whole document

#### Scenario: Every contributor appears exactly once

- **GIVEN** an indexed workspace whose modules, types, and members are all corpus-eligible
- **WHEN** the index is built
- **THEN** the corpus holds exactly one passage per contributing symbol

#### Scenario: Corpus derivation is deterministic

- **GIVEN** the same sources built into the same store twice
- **WHEN** the passages are retrieved after each build
- **THEN** the two corpora are identical

#### Scenario: A name-only symbol contributes nothing

- **GIVEN** an indexed symbol (a function parameter) whose persisted content tiers all hold only its own name token
- **WHEN** the index is built
- **THEN** the corpus holds no passage for that symbol

### Requirement: Semantic representations persisted per build

For every passage, the system SHALL persist with the build one or more vector representations and one lexical representation derived from that passage's content, wholly superseded by each subsequent build; a build that fails SHALL leave the prior build's representations intact and authoritative; and a rebuild over unchanged sources under unchanged parameters SHALL yield representations identical to the prior build's.
Each of a passage's vector representations SHALL derive from one chunk — the passage's identifying header together with a bounded run of its content — so that content drawn from anywhere in the passage is represented as content of that symbol; the lexical representation SHALL derive from the passage's whole render.

#### Scenario: Every passage is represented

- **GIVEN** a completed build with a non-empty corpus
- **WHEN** the semantic representations are retrieved
- **THEN** each passage carries at least one vector representation and exactly one lexical representation

#### Scenario: Every chunk carries its passage's header

- **GIVEN** a passage whose content exceeds the chunk size in effect
- **WHEN** its vector representations are retrieved
- **THEN** each derives from a chunk carrying the passage's identifying header

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again under the same parameters
- **THEN** the persisted vector and lexical representations are identical to the first build's

#### Scenario: Representations of vanished symbols do not linger

- **GIVEN** a store built from sources containing a symbol
- **WHEN** the sources no longer contain that symbol and the store is rebuilt
- **THEN** no representation for the vanished symbol remains retrievable

#### Scenario: An edited symbol is re-represented

- **GIVEN** a store built from a set of sources
- **WHEN** a corpus-contributing symbol's source content is changed and the store is rebuilt
- **THEN** that symbol's persisted representations derive from the new content

#### Scenario: A failed build leaves prior representations authoritative

- **GIVEN** a store holding a completed build's representations
- **WHEN** a subsequent build fails before completing
- **THEN** the prior build's representations remain retrievable, unchanged

### Requirement: Clone-equivalence keys

For every corpus-contributing symbol that contains no other corpus-contributing symbol, the system SHALL persist two equivalence keys over the symbol's source: a formatting-insensitive key that two symbols share exactly when their token sequences are identical after comments, whitespace, and documentation are disregarded, and a substitution-insensitive key that two symbols share exactly when their token sequences are additionally identical under a consistent one-to-one substitution of identifiers and literal values.
Documentation here is the text the system reads as a symbol's own documentation for its interface tier: a Rust doc comment, and a Python docstring, which is the leading string statement of a function, class, or module body that Python reads as its docstring.
A string statement that is not in a docstring position SHALL remain in the token sequence as a literal value.
A symbol that contains other corpus-contributing symbols SHALL carry no equivalence key.
Keys SHALL be persisted with the build and wholly superseded by each subsequent build.

#### Scenario: Formatting variants share the formatting-insensitive key

- **GIVEN** two indexed functions whose sources differ only in whitespace and comments
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A consistently renamed copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently renames the other's identifiers
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: A literal-substituted copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently replaces the other's literal values
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: An inconsistent renaming shares neither key

- **GIVEN** two indexed functions alike except that one merges two distinct identifiers of the other into a single name
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: Unrelated code shares neither key

- **GIVEN** two indexed functions with different token sequences under every normalization
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: A container carries no equivalence key

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type carries no equivalence key

#### Scenario: Keys are superseded by each build

- **GIVEN** an indexed leaf whose source then changes
- **WHEN** the store is rebuilt
- **THEN** the leaf's persisted keys are those of the new source and the prior build's keys are gone

#### Scenario: Python docstrings that differ share the formatting-insensitive key

- **GIVEN** two indexed Python functions identical except for the text of their docstrings
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A Python docstring present in only one copy is disregarded

- **GIVEN** two indexed Python functions identical except that only one has a docstring
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A renamed Python copy with a different docstring shares the substitution-insensitive key

- **GIVEN** two indexed Python test functions identical except for consistently renamed identifiers and a docstring present in only one
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: Rust doc comments that differ share the formatting-insensitive key

- **GIVEN** two indexed Rust functions identical except for the text of their doc comments
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A string statement outside a docstring position stays a literal

- **GIVEN** two indexed Python functions identical except for the text of a bare string statement that follows the first statement of the body
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

### Requirement: Shared-fragment evidence persisted per build

For every corpus-contributing symbol with comparable content, the system SHALL persist with the build the evidence that the shared-fragment answer reads, derived from that comparable content.
A symbol that carries equivalence keys has as comparable content the documentation-disregarding token sequence the keys read.
A function or method that contains other corpus-contributing symbols has as comparable content the same documentation-disregarding token sequence over its own span, with the tokens of each contained corpus-contributing symbol replaced by one boundary that no run of shared tokens can include.
Any other symbol, such as a type or module that contains other corpus-contributing symbols, has no comparable content and SHALL contribute no evidence.
The evidence SHALL be wholly superseded by each subsequent build, a build that fails SHALL leave the prior build's evidence intact and authoritative, and a rebuild over unchanged sources SHALL yield identical evidence.
The build SHALL record with the evidence the guaranteed fragment length and the data-stretch length it was derived under, and the shared-fragment answer SHALL apply that recorded data-stretch length and apply and report that recorded guaranteed length.

#### Scenario: A leaf contributes evidence

- **GIVEN** an indexed function longer than the guaranteed fragment length
- **WHEN** the index is built
- **THEN** the store holds shared-fragment evidence for the function

#### Scenario: A container contributes no evidence

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the store holds no shared-fragment evidence for the type itself

#### Scenario: A function that contains a nested function contributes evidence for its own code

- **GIVEN** an indexed function that defines a nested function, and whose own code outside the nested function is longer than the guaranteed fragment length
- **WHEN** the index is built
- **THEN** the store holds shared-fragment evidence for the function derived from its own code, and separate evidence for the nested function

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the persisted shared-fragment evidence is identical to the first build's

#### Scenario: Evidence of a changed leaf is replaced

- **GIVEN** an indexed leaf whose source then changes
- **WHEN** the store is rebuilt
- **THEN** the leaf's evidence derives from the new source and no evidence from the prior source remains

#### Scenario: A failed build leaves prior evidence authoritative

- **GIVEN** a store holding a completed build's shared-fragment evidence
- **WHEN** a subsequent build fails before completing
- **THEN** the prior build's evidence remains retrievable, unchanged

#### Scenario: The answer applies the length the store records

- **GIVEN** a store whose recorded guaranteed fragment length differs from the length the binary builds with
- **WHEN** a shared-fragment answer is computed from that store
- **THEN** the answer applies and reports the recorded length

### Requirement: Semantic-index identity recorded

The system SHALL record, with every build it persists, the semantic-index identity in effect — the embedding model identity, the corpus definition version, and the chunk parameters the build ran under — and SHALL make it retrievable alongside the store's provenance.

#### Scenario: Identity recorded and retrievable

- **GIVEN** a completed build
- **WHEN** the store's provenance is retrieved
- **THEN** it carries the embedding model identity, the corpus definition version, and the chunk parameters that produced the build's semantic representations

#### Scenario: Two stores built under different parameters are distinguishable

- **GIVEN** two stores built from the same sources under different chunk parameters
- **WHEN** each store's provenance is retrieved
- **THEN** their recorded semantic-index identities differ

### Requirement: A build over unchanged inputs does no work

The system SHALL treat the stored index as current when everything it recorded still matches the workspace: the workspace identity and root, the sources, the declared packages and units, the analyzer, the declared environment, and the chunk parameters.
When the index is current, a build SHALL analyze nothing, leave the store unchanged, report the index as already current, and succeed.
When any of those inputs differs, or no compatible store exists, the system SHALL build.
The system SHALL offer an explicit way to build even when the stored index is already current.

#### Scenario: Rebuilding an unchanged workspace analyzes nothing

- **GIVEN** a workspace whose index was just built
- **WHEN** the workspace is built again with no source, analyzer, environment, or parameter change
- **THEN** no analysis runs, the store is unchanged, and the index is reported as already current

#### Scenario: An edited source rebuilds

- **GIVEN** a workspace whose index was just built
- **WHEN** one source file is edited and the workspace is built again
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A renamed binary rebuilds

- **GIVEN** a Rust workspace whose index was just built, with a binary `tool`
- **WHEN** the binary is renamed to `cli` in `Cargo.toml` with no other change, and the workspace is built again
- **THEN** the workspace is analyzed, the store is rebuilt, and the binary's declarations have the marker `+bin.cli`

#### Scenario: A changed analyzer rebuilds

- **GIVEN** a workspace whose index was built by one analyzer version
- **WHEN** the workspace is built again with a different analyzer version
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed environment rebuilds

- **GIVEN** a workspace whose index records a declared interpreter environment
- **WHEN** the workspace is built again against a different environment
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed chunk parameter rebuilds

- **GIVEN** a workspace whose index was built under one set of chunk parameters
- **WHEN** the workspace is built again, unchanged in every other input, under a different chunk size or overlap
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A store recorded for another workspace builds

- **GIVEN** a store whose recorded workspace identity or root differs from the one being built
- **WHEN** the workspace is built over that store
- **THEN** the workspace is analyzed, the handoff is disclosed, and the store records the workspace it now describes

#### Scenario: An absent index builds

- **GIVEN** a workspace with no index
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is written

#### Scenario: A store built under an unrecognized schema version builds

- **GIVEN** a workspace whose store the system created under a schema version it does not recognize
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is replaced

#### Scenario: An explicit rebuild ignores currency

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built with the explicit rebuild option
- **THEN** the workspace is analyzed and the store is rebuilt

### Requirement: Deriving the graph costs one pass over each document

The system SHALL parse each document it analyzes at most once per build, and the parse work a build performs SHALL NOT grow with the number of symbols or definitions its documents declare.

#### Scenario: A symbol-dense document costs no more than a sparse one of the same size

- **GIVEN** two documents of equal size, one declaring many symbols and one declaring few
- **WHEN** each is built
- **THEN** the parse work performed for each is the same

#### Scenario: Adding declarations to a document does not add parse work

- **GIVEN** a workspace built once, and the same workspace after further declarations are added to one document
- **WHEN** each is built
- **THEN** the parse work performed for that document is unchanged

#### Scenario: Parse work follows the documents analyzed

- **GIVEN** a workspace of several documents
- **WHEN** the workspace is built
- **THEN** the parse work performed is proportional to those documents

### Requirement: Bounded chunk scope

Every vector representation SHALL derive from a chunk within the recorded chunk size, and the chunks of one passage SHALL together cover that passage's whole content — no part of a passage's content is left unrepresented.
The chunk size SHALL bound the whole text embedded, the passage header carried on each chunk included.

Where the content offers structural boundaries — declarations and statements in code, paragraphs and sentences in prose — a chunk SHALL begin and end at one.
Only content offering no such boundary within the chunk size SHALL be divided at an arbitrary point, and a chunk SHALL never be empty.

When the recorded overlap is non-zero, each chunk after the first SHALL additionally carry the trailing content of its predecessor up to the overlap, taken as whole boundary-delimited units wherever the content offers them.

#### Scenario: Content within the chunk size is one chunk

- **GIVEN** a passage whose content does not exceed the chunk size in effect
- **WHEN** the index is built
- **THEN** the passage carries exactly one vector representation, derived from its whole content

#### Scenario: Content exceeding the chunk size is covered by several chunks

- **GIVEN** a passage whose content exceeds the chunk size in effect
- **WHEN** the index is built
- **THEN** the passage carries more than one vector representation, and every part of its content lies within at least one chunk

#### Scenario: No chunk exceeds the chunk size

- **GIVEN** a passage whose content far exceeds the chunk size in effect
- **WHEN** the index is built
- **THEN** every chunk embedded is within the chunk size, its passage header included

#### Scenario: Chunks divide at structural boundaries

- **GIVEN** a passage whose content exceeds the chunk size and consists of several declarations
- **WHEN** the index is built
- **THEN** no chunk begins or ends part-way through a declaration

#### Scenario: Prose divides at prose boundaries

- **GIVEN** a passage whose content exceeds the chunk size and is documentation prose the syntax oracle reports as one indivisible construct
- **WHEN** the index is built
- **THEN** no chunk begins or ends part-way through a sentence

#### Scenario: Content offering no boundary is still bounded and covered

- **GIVEN** a passage whose content exceeds the chunk size and offers no structural boundary within it, such as a single literal
- **WHEN** the index is built
- **THEN** each chunk is within the chunk size and the chunks together cover the whole literal

#### Scenario: Overlap carries whole units

- **GIVEN** a passage whose content exceeds the chunk size, built with a non-zero overlap
- **WHEN** the index is built
- **THEN** each chunk after the first begins with trailing units of its predecessor, none of them partial

#### Scenario: No overlap means no shared content

- **GIVEN** a passage whose content exceeds the chunk size, built with an overlap of none
- **WHEN** the index is built
- **THEN** no content appears in more than one chunk

### Requirement: Vector independence from batch composition

A vector representation SHALL be a function of the chunk it represents alone: the vector persisted for a given chunk SHALL be identical whether that chunk is embedded by itself or alongside any other chunks, and SHALL NOT depend on the size, order, or content of the group it was embedded with.

#### Scenario: Grouping does not change a vector

- **GIVEN** a chunk that a build embeds alongside other chunks
- **WHEN** the same chunk is embedded by itself
- **THEN** the two vectors are identical

#### Scenario: A large neighbor does not change a small chunk's vector

- **GIVEN** a corpus holding both a very short chunk and a chunk orders of magnitude longer
- **WHEN** the index is built
- **THEN** the short chunk's vector is identical to the vector it carries in a corpus holding it alone

#### Scenario: The order chunks are embedded in does not change a vector

- **GIVEN** a group of chunks differing widely in length, embedded together in one order
- **WHEN** the same group is embedded together in a different order
- **THEN** every chunk carries the same vector under both orders

### Requirement: Chunk parameters recorded and honored

The system SHALL record, with every build it persists, the chunk parameters in effect — the chunk size and the overlap between adjacent chunks — and every persisted vector SHALL derive from a chunk taken under the parameters recorded with that same build.

#### Scenario: Parameters recorded with the build

- **GIVEN** a build run with explicit chunk parameters
- **WHEN** the store's recorded parameters are retrieved
- **THEN** they are the values the build ran with

#### Scenario: Changed parameters re-derive every vector

- **GIVEN** a store built at one chunk size
- **WHEN** the same sources are built into that store at a different chunk size
- **THEN** every persisted vector derives from the new chunk size, and none is retained from the prior build

### Requirement: Recorded locations carry line and column

For every location the index records — a symbol's definition, a reference occurrence, and a join discrepancy — the system SHALL record its start and its end both as byte offsets and as a line and a column, and both forms SHALL denote the same positions in the source text the build indexed.
A line SHALL be numbered from 1, and a line SHALL end at a line feed, at a carriage return followed by a line feed, or at a carriage return not followed by a line feed.
A column SHALL be numbered from 1 and SHALL count Unicode characters from the start of the line, not bytes and not UTF-16 code units.
The end SHALL be the position immediately after the location's last character, as the end byte offset is.
A byte offset between the carriage return and the line feed of one line ending, which no line and column can name, SHALL have the line and column of that carriage return; every other byte offset and its line and column SHALL denote the same position.
A location whose byte offsets are absent SHALL have its line and column absent too.

#### Scenario: A location in ASCII text

- **GIVEN** a source file whose third line is `fn target() {}` with no indentation
- **WHEN** the index is built
- **THEN** the definition of `target` starts at line 3, column 1 and ends at line 3, column 15, and its byte offsets denote the same positions

#### Scenario: Columns count characters, not bytes

- **GIVEN** a source line in which a character encoded in more than one byte, such as `é`, precedes a reference occurrence
- **WHEN** the index is built
- **THEN** the occurrence's start column counts that character once

#### Scenario: Columns count characters, not UTF-16 code units

- **GIVEN** a source line in which a character outside the Basic Multilingual Plane, such as an emoji, precedes a reference occurrence
- **WHEN** the index is built
- **THEN** the occurrence's start column counts that character once

#### Scenario: Each line ending starts a new line

- **GIVEN** three source files that separate their lines with line feeds, with carriage return and line feed pairs, and with lone carriage returns
- **WHEN** the index is built
- **THEN** a definition on the fifth line of each file is recorded at line 5, and the carriage return and line feed pair counts as one line ending

#### Scenario: A position inside a line ending

- **GIVEN** a location whose byte offset falls between the carriage return and the line feed of a line ending
- **WHEN** the index is built
- **THEN** that position is recorded at the line and column of the carriage return

#### Scenario: A location that spans lines

- **GIVEN** a function whose definition span runs from its first line to its closing brace several lines later
- **WHEN** the index is built
- **THEN** the definition's end line is the line of the closing brace and its end column is the column after the brace

#### Scenario: A join discrepancy carries line and column

- **GIVEN** a semantic occurrence that the join refuses, at coordinates that normalize to a byte range
- **WHEN** the index is built
- **THEN** the persisted discrepancy carries the line and column of that byte range

#### Scenario: Absent coordinates stay absent

- **GIVEN** a semantic occurrence that the join refuses, at coordinates that cannot normalize to a byte range
- **WHEN** the index is built
- **THEN** the persisted discrepancy carries neither byte offsets nor a line and column

### Requirement: Recorded declarations carry their header end

For every symbol the index records with a definition span, the system SHALL record where the symbol's header ends, both as a byte offset and as a line and a column, and both forms SHALL denote the same position in the source text the build indexed.
A symbol's header SHALL be the part of its definition span that precedes the declaration's body, and SHALL start where the definition span starts.
A declaration with no body distinct from its form, such as a constant, a field, or a parameter, SHALL have a header equal to its whole definition span.
A document's own module, whose header does not appear in the source, SHALL have no header end, and neither SHALL a symbol without a definition span.
Line and column SHALL follow the same numbering as every other recorded location.

#### Scenario: A function's header ends before its body

- **GIVEN** a function whose declaration form precedes a body
- **WHEN** the index is built
- **THEN** the function's header end is the position where its body begins, not the end of its definition span

#### Scenario: A header spans the lines its declaration form spans

- **GIVEN** a function whose parameter list runs over several lines, or a declaration preceded by decorators that its definition span includes
- **WHEN** the index is built
- **THEN** the header covers every line from the start of the definition span through the end of the declaration form

#### Scenario: A declaration without a body

- **GIVEN** a constant, a field, or a parameter that the index records with a definition span
- **WHEN** the index is built
- **THEN** its header end equals its definition span's end

#### Scenario: A document's own module has no header end

- **GIVEN** any indexed source file
- **WHEN** the index is built
- **THEN** the module the file defines carries no header end, while a module declared inside the file carries one

### Requirement: A build names sources that changed while it ran

After a build writes the store, the system SHALL discover the workspace's sources again and compare each with the text the build indexed.
A source discovered again with different text SHALL be named changed, a source discovered only now SHALL be named added, and an indexed source no longer discovered SHALL be named removed.
The build SHALL succeed whether or not any source is named, and both its human answer and its machine answer SHALL name every such source with its classification.
When a source cannot be read again for a reason other than no longer existing, the build SHALL fail, although the store is already written.
When no source is named, the machine answer SHALL carry an empty list.
A build that analyzes nothing because the stored index is already current SHALL carry an empty list.

#### Scenario: An edit during the build is named

- **GIVEN** a workspace whose analyzer rewrites one source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds, its answer names that file as changed, and `status` then reports the index stale for changed content

#### Scenario: A file created during the build is named

- **GIVEN** a workspace whose analyzer creates a new source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds and its answer names that file as added

#### Scenario: A file deleted during the build is named

- **GIVEN** a workspace whose analyzer deletes one indexed source file while it runs
- **WHEN** the workspace is built
- **THEN** the build succeeds and its answer names that file as removed

#### Scenario: A source that becomes unreadable fails the build

- **GIVEN** a workspace whose analyzer creates a source file that cannot be read while it runs
- **WHEN** the workspace is built
- **THEN** the build fails with an error that names that file, and the store holds the new index

#### Scenario: An undisturbed build names nothing

- **GIVEN** a workspace that no process edits during the build
- **WHEN** the workspace is built
- **THEN** the machine answer carries an empty list and the human answer names no file

#### Scenario: An already-current build names nothing

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built without the explicit rebuild option
- **THEN** no analysis runs and the machine answer carries an empty list

## Technical Notes

- **Implementation**: `src/graph/`
- **Dependencies**: symbol-identity, semantic-engine
