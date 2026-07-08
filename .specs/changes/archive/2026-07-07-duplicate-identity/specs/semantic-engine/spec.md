# Delta for Semantic Engine

## ADDED Requirements

### Requirement: Same-descriptor definitions extracted as distinct symbols

When a backend's output contains more than one distinct definition under an identical resolved descriptor, extraction through the backend contract SHALL yield one symbol per definition — distinct definitions SHALL never be merged on descriptor equality — and SHALL preserve every non-definition occurrence of the duplicated descriptor for downstream attribution rather than assigning it to any single one of the definitions at extraction.

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
