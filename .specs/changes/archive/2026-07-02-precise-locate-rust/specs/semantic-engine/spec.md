# Delta for semantic-engine

## ADDED Requirements

### Requirement: Mandatory extraction contract

The system SHALL define a backend contract — the intersection every semantic backend must satisfy — under which a backend, given a project, produces the project's symbols with their resolved canonical descriptors and the occurrences of each symbol, where every occurrence is classified by role (at minimum, definition versus reference) and carries a source range that maps unambiguously to a file position.
A backend SHALL be usable through this contract if and only if it satisfies the contract in full, verified by a backend conformance suite that the Rust adapter (`rust-analyzer scip` with `tree-sitter-rust`) passes as the exemplar.

Serves: locate-symbol

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

### Requirement: Backend provenance reporting

The system SHALL obtain, with every index a backend produces, the backend's analyzer identity and version, and SHALL make that provenance available to consumers of the resulting graph.

Serves: trust-the-answer

#### Scenario: Provenance accompanies an index

- **GIVEN** a backend that has produced an index for a project
- **WHEN** the index is ingested
- **THEN** the analyzer name and version are recorded and retrievable as provenance of that index

### Requirement: Optional capabilities are queryable

The system SHALL expose capabilities that not every backend guarantees — such as enclosure information, base-index eligibility, or live incremental updates — as queryable feature detection, and SHALL NOT assume an undeclared capability is present.

Serves: trust-the-answer, understand-structure

#### Scenario: Declared capability is usable

- **GIVEN** a backend that declares it supplies enclosure information
- **WHEN** the system queries that capability before use
- **THEN** the capability is reported present and the system relies on it

#### Scenario: Undeclared capability is not assumed

- **GIVEN** a backend that does not declare base-index eligibility
- **WHEN** the system inspects the backend
- **THEN** the backend is not used as an authoritative base index, and its absence is represented explicitly rather than inferred as present
