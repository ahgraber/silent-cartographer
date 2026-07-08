# Semantic Engine Specification

## Purpose

Defines the backend contract for producing a project's symbols and occurrences from a semantic analyzer.
The contract is the intersection every backend must satisfy; capabilities only some backends offer are exposed as queryable feature detection, keeping backends substitutable.

## Requirements

### Requirement: Mandatory extraction contract

The system SHALL define a backend contract — the intersection every semantic backend must satisfy — under which a backend, given a project, produces the project's symbols with their resolved canonical descriptors and the occurrences of each symbol, where every occurrence is classified by role (at minimum, definition versus reference) and carries a source range that maps unambiguously to a file position.
A backend SHALL be usable through this contract if and only if it satisfies the contract in full, verified by a backend conformance suite that the Rust adapter (`rust-analyzer scip` with `tree-sitter-rust`) passes as the exemplar.

#### Scenario: Definition occurrence extracted

- **GIVEN** a Rust project containing a symbol declaration
- **WHEN** the project is indexed through the backend contract
- **THEN** the symbol appears with its resolved descriptor and a definition-role occurrence whose range maps to the declaration's position

#### Scenario: Reference occurrence extracted

- **GIVEN** a Rust project where a symbol is used away from its declaration
- **WHEN** the project is indexed through the backend contract
- **THEN** a reference-role occurrence for that symbol is produced with a range that maps to the use site

#### Scenario: Conformance suite gates usability

- **GIVEN** a candidate backend that fails any clause of the extraction contract
- **WHEN** it is run against the conformance suite
- **THEN** the suite reports the backend as non-conformant and the system does not treat it as usable

### Requirement: Same-descriptor definitions extracted as distinct symbols

When a backend's output contains more than one distinct definition under an identical resolved descriptor, extraction through the backend contract SHALL yield one symbol per definition — distinct definitions SHALL never be merged on descriptor equality — and SHALL preserve every non-definition occurrence of the duplicated descriptor for downstream attribution rather than assigning it to any single one of the definitions at extraction.

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

### Requirement: Backend provenance reporting

The system SHALL obtain, with every index a backend produces, the backend's analyzer identity and version, and SHALL make that provenance available to consumers of the resulting graph.

#### Scenario: Provenance accompanies an index

- **GIVEN** a backend that has produced an index for a project
- **WHEN** the index is ingested
- **THEN** the analyzer name and version are recorded and retrievable as provenance of that index

### Requirement: Optional capabilities are queryable

The system SHALL expose capabilities that not every backend guarantees — such as enclosure information, base-index eligibility, or live incremental updates — as queryable feature detection, and SHALL NOT assume an undeclared capability is present.

#### Scenario: Declared capability is usable

- **GIVEN** a backend that declares it supplies enclosure information
- **WHEN** the system queries that capability before use
- **THEN** the capability is reported present and the system relies on it

#### Scenario: Undeclared capability is not assumed

- **GIVEN** a backend that does not declare base-index eligibility
- **WHEN** the system inspects the backend
- **THEN** the backend is not used as an authoritative base index, and its absence is represented explicitly rather than inferred as present

## Technical Notes

- **Implementation**: `src/semantic/`
- **Dependencies**: symbol-identity
