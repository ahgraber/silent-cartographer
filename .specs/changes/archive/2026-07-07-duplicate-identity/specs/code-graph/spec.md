# Delta for Code Graph

## MODIFIED Requirements

### Requirement: Occurrences of duplicated descriptors are never arbitrarily attributed

> Previously: every non-definition occurrence of a duplicated descriptor was recorded as duplicate-ambiguous unconditionally, with no attribution path.

When more than one distinct definition shares an identical resolved descriptor, the system SHALL attribute each definition occurrence to the definition at its own location; SHALL attribute a non-definition occurrence of that descriptor to one of the duplicates only when the occurrence's containing document is associated with exactly that one duplicate and the occurrence also satisfies the guarded join's alignment rules, except that an occurrence whose source token spells the package's own name rather than the descriptor's name — a token that denotes the package's library target regardless of where it sits — SHALL be attributed to the duplicate whose definition document the build system's authoritative target description names as the library target's root, and SHALL be recorded duplicate-ambiguous when no authoritative target description is available or it names no persisted duplicate; every attribution SHALL carry its locality evidence as provenance; and SHALL record a non-definition occurrence whose containing document is associated with no duplicate, or with more than one, as a typed duplicate-ambiguous outcome — never attributing it to any single duplicate arbitrarily.

<!-- modified-removes: Reference to a duplicated descriptor is typed ambiguous -->

Serves: twins-stay-distinct, locality-keeps-precision, disclosed-ambiguity

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

#### Scenario: Reference in shared territory is typed ambiguous

- **GIVEN** a non-definition occurrence of a duplicated descriptor whose containing document is associated with more than one of the duplicates
- **WHEN** the join runs
- **THEN** the occurrence is recorded as a duplicate-ambiguous outcome and is attributed to no single duplicate

#### Scenario: Unduplicated descriptors are unaffected

- **GIVEN** a descriptor with exactly one definition
- **WHEN** the join runs
- **THEN** its non-definition occurrences are attributed through the ordinary guarded join, never marked duplicate-ambiguous

## ADDED Requirements

### Requirement: Builds wholly supersede prior derived state

After a build completes, the store SHALL describe exactly that build's sources: every persisted symbol, occurrence, edge, and discrepancy row derives from the completed build, no row from any prior build remains, and a build that fails SHALL leave the prior build's state intact and authoritative.

Serves: disclosed-ambiguity

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

Serves: disclosed-ambiguity

#### Scenario: Duplicated groups are retrievable

- **GIVEN** a build over sources containing at least one duplicated descriptor
- **WHEN** the duplicated-descriptor groups are requested
- **THEN** each group is returned identifying its shared descriptor and the definitions that share it

#### Scenario: No duplicates is typed absence

- **GIVEN** a build over sources containing no duplicated descriptors
- **WHEN** the duplicated-descriptor groups are requested
- **THEN** a definite empty set is returned, distinct from an unavailable or failed answer
