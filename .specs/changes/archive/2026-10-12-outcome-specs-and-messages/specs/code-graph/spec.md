# Delta for Code Graph Specification

## MODIFIED Requirements

### Requirement: Per-symbol tier content

> Previously: tier content was described as taken from the same syntax tree that the build uses to match its two indexes, and a note explained how the body tier came to be named; the behavior is unchanged.

The system SHALL persist, for every in-workspace symbol whose definition span is persisted, content at three fixed tiers over the symbol's own source: a signature tier — the declaration form without its body — an interface tier — the signature together with the symbol's own documentation — and a body tier — the definition span's source text.
The signature and interface tiers are derived from the source; the body tier is the source, verbatim.
A symbol carrying no documentation SHALL have an interface tier equal to its signature tier, and a declaration with no distinct body SHALL have a signature tier equal to its full declaration.
A module defined by a document rather than an in-document declaration carries no declaration form in source, so its signature tier SHALL be its qualified name.
Tier content SHALL be taken from the sources as they were when the build ran, SHALL be persisted with the build, and SHALL be wholly superseded by each subsequent build.

Serves: fetch-at-depth, search-ready-substrate

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

### Requirement: Staleness reflects underlying change

> Previously: two scenarios also checked a reindex flag, which no requirement sentence promised.

The system SHALL mark a persisted result as stale whenever the sources it was derived from no longer match the indexed content, or any element of its recorded provenance — the analyzer identity and version, or an environment fact the backend declared material — differs from the one in effect.
While none has changed, the result SHALL be reported as fresh.

Serves: trust-the-answer, python-calibrated-answers

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

> Previously: the enclosing declaration was located in the build's syntax tree; the behavior is unchanged.

The system SHALL attribute every aligned reference occurrence to the nearest persisted declaration that encloses it in the indexed source, and SHALL attribute an occurrence enclosed by no narrower declaration to its module — never to a fabricated declaration.
Where more than one persisted definition shares a location, the system SHALL NOT attribute an occurrence to any of them, and SHALL attribute it as though no persisted declaration enclosed it.
The work performed to resolve one occurrence's attribution SHALL be bounded by that occurrence's enclosing-declaration chain, and SHALL NOT grow with the number of definitions the build persists.

Serves: understand-structure, trust-the-answer, rebuild-costs-what-changed

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

### Requirement: Builds wholly supersede prior derived state

> Previously: the requirement and its scenario were stated in terms of stored rows; the behavior is unchanged.

After a build completes, the store SHALL describe exactly that build's sources: every symbol, occurrence, edge, and discrepancy the store holds derives from the completed build, nothing from any prior build remains, and a build that fails SHALL leave the prior build's state intact and authoritative.

Serves: disclosed-ambiguity

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the symbols, occurrences, and edges the store holds are identical to the first build's

#### Scenario: Rows for vanished entities do not linger

- **GIVEN** a store built from sources containing a symbol
- **WHEN** the sources no longer contain that symbol and the store is rebuilt
- **THEN** no row for the vanished symbol remains retrievable

### Requirement: Incompatible index stores are replaced or refused, never half-used

> Previously: a refusal named one recovery action, rebuilding, even when a newer binary had written the store.

The system SHALL record the schema version in every store it creates; a build over a store the system recognizes as its own whose recorded schema version differs from the version the binary writes SHALL replace the store and succeed; and every query against such a recognized, version-mismatched store SHALL refuse with an error naming the store's version, the expected version, and the recovery that applies — rebuilding when the store is older than the binary, and upgrading the binary or using another store path when the store is newer — never surfacing a storage-level failure mid-operation.
A file not recognized as the system's own store is outside replacement's reach and is governed by the Store ownership recognition requirement.

Serves: recoverable-index-state, own-store-only, actionable-messages

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
