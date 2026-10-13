# Delta for Semantic Engine Specification

## MODIFIED Requirements

### Requirement: Same-descriptor definitions extracted as distinct symbols

> Previously: the requirement described the occurrences as kept for a later attribution step; the behavior is unchanged.

When a backend's output contains more than one distinct definition under an identical resolved descriptor, extraction through the backend contract SHALL yield one symbol per definition — distinct definitions SHALL never be merged on descriptor equality — and SHALL return every non-definition occurrence of the duplicated descriptor without assigning it to any single one of the definitions.

Serves: twins-stay-distinct

#### Scenario: Twin definitions yield distinct symbols

- **GIVEN** a project in which two different definitions receive a byte-identical resolved descriptor from the backend
- **WHEN** the project is indexed through the backend contract
- **THEN** extraction yields two distinct symbols, each carrying the definition occurrence at its own location

#### Scenario: References to a duplicated descriptor survive extraction unassigned

- **GIVEN** a duplicated descriptor that is also referenced away from its definitions
- **WHEN** the project is indexed through the backend contract
- **THEN** every reference occurrence of that descriptor is preserved and none is exclusively assigned to a single definition by extraction itself

#### Scenario: Unique descriptors are unaffected

- **GIVEN** a descriptor carried by exactly one definition
- **WHEN** the project is indexed through the backend contract
- **THEN** extraction yields one symbol with its definition and reference occurrences, exactly as before

### Requirement: Unavailable backend is a typed refusal, never a partial index

> Previously: only a scenario stated that an existing store is left untouched.

When a backend's external toolchain is unavailable, or the project's environment cannot be resolved well enough to produce a faithful index, indexing SHALL fail with a typed, actionable error naming what is missing, and SHALL NOT produce or persist a partial or degraded index in its place.
Indexing that fails this way SHALL leave any existing store unchanged.

Serves: python-calibrated-answers

#### Scenario: Missing indexer tool refuses with guidance

- **GIVEN** a project whose backend indexer tool is not present in the environment
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming the missing tool, and any existing store is left untouched

#### Scenario: Unresolvable environment refuses with guidance

- **GIVEN** a Python project whose interpreter environment cannot be resolved
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming the environment problem, and no index is persisted from the failed attempt

### Requirement: Optional capabilities are queryable

> Previously: the scenarios checked that the system relies on a declared capability, which no requirement sentence promised.

The system SHALL expose capabilities that not every backend guarantees — such as enclosure information, base-index eligibility, or live incremental updates — as queryable feature detection, and SHALL NOT assume an undeclared capability is present.

Serves: trust-the-answer, understand-structure

#### Scenario: Declared capability is usable

- **GIVEN** a backend that declares it supplies enclosure information
- **WHEN** the system queries that capability before use
- **THEN** the capability is reported present

#### Scenario: Undeclared capability is not assumed

- **GIVEN** a backend that does not declare base-index eligibility
- **WHEN** the system inspects the backend
- **THEN** the capability is reported absent, and the backend is not used as an authoritative base index
