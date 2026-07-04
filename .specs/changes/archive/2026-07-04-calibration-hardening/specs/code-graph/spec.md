# Delta for code-graph

## ADDED Requirements

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

When more than one distinct definition shares an identical resolved descriptor, the system SHALL attribute each definition occurrence to the definition at its own location, and SHALL record every non-definition occurrence of that descriptor as a typed duplicate-ambiguous outcome, never attributing it to any single one of the duplicates.

Serves: stable-identity-under-duplicates

#### Scenario: Definition occurrences attach to their own duplicate

- **GIVEN** two distinct definitions sharing an identical resolved descriptor
- **WHEN** the join runs
- **THEN** each definition occurrence is attributed to the definition at its own location

#### Scenario: Reference to a duplicated descriptor is typed ambiguous

- **GIVEN** a non-definition occurrence naming a descriptor shared by multiple definitions
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Unduplicated descriptors are unaffected

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the join runs
- **THEN** its non-definition occurrences are attributed through the ordinary guarded join, never marked duplicate-ambiguous

### Requirement: Join discrepancies are inspectable

For every semantic occurrence whose join outcome is not aligned, the system SHALL persist the discrepancy's location, outcome kind, expected symbol name, and the source text found at that location, wholly superseded by each subsequent build; SHALL make that detail retrievable through the CLI; the default listing SHALL be a bounded summary whose aggregation covers the full discrepancy set and SHALL state when entries are withheld by the bound; and an explicit request SHALL return the complete detail.

Serves: diagnosable-join

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

Serves: recoverable-index-state

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

## MODIFIED Requirements

### Requirement: Guarded positional join

> Previously: an attribution was persisted as aligned only under name-token equality; structurally true non-name-shaped resolutions (operator desugaring, crate roots, whole-file module spans) were refused as text mismatches.

The system SHALL attribute each semantic occurrence to the syntactic construct at the corresponding source location, and SHALL persist an attribution as aligned only when the occurrence satisfies a named alignment rule's exact expectation.
The default rule is name-token equality: the source text at the location matches the occurrence's expected symbol name — its name token, the terminal segment of the descriptor, not the qualified path.
Four kind-scoped rules extend it: a crate-root module occurrence is accepted when the source token is the descriptor's own package name or the keyword `crate`; a reference occurrence of a desugared-operator method is accepted when its location holds the operator construct that method desugars from, per a closed correspondence; a module definition occurrence is accepted when its range spans the module's whole document; and a reference occurrence resolving to a type — or to an implementation of one — is accepted at a self-type keyword when the enclosing implementation's self type is that type, generic arguments aside.
Every aligned attribution SHALL carry the rule that accepted it as provenance.
An occurrence satisfying no rule SHALL NOT be persisted as an aligned attribution; when a semantic occurrence and the syntax at its location cannot be reconciled under any rule, the system SHALL surface the discrepancy and SHALL NOT persist it as a confident attribution.

Serves: rule-typed-alignment

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

### Requirement: Join alignment accounting

> Previously: a single aligned count with three refusal outcomes (text-mismatch, semantic-only) — this change first added duplicate-ambiguous, then split acceptance per alignment rule.

The system SHALL record, for every build, the number of semantic occurrences accepted under each named alignment rule and the number in each refusal outcome — text-mismatch, semantic-only, and duplicate-ambiguous — plus the number of unresolved syntax-only constructs, and SHALL make these counts retrievable; the per-rule acceptance counts and the refusal counts together SHALL sum to the total semantic occurrences the build processed.

Serves: diagnosable-join, stable-identity-under-duplicates, rule-typed-alignment

#### Scenario: Outcome counts recorded

- **GIVEN** a build over sources that produce occurrences in more than one join outcome
- **WHEN** the build completes
- **THEN** each per-rule acceptance count and each refusal count is recorded and retrievable alongside the index's provenance

#### Scenario: Counts conserve the occurrence total

- **GIVEN** a completed build
- **WHEN** the per-rule acceptance counts and the text-mismatch, semantic-only, and duplicate-ambiguous counts are summed
- **THEN** the sum equals the total number of semantic occurrences the build processed
