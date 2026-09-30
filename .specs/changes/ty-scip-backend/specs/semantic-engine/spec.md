# Delta for Semantic Engine

## MODIFIED Requirements

### Requirement: Mandatory extraction contract

The system SHALL define a backend contract — the intersection every semantic backend must satisfy — under which a backend, given a project, produces the project's symbols with their resolved canonical descriptors and the occurrences of each symbol, where every occurrence is classified by role (at minimum, definition versus reference) and carries a source range that maps unambiguously to a file position.
A backend SHALL be usable through this contract if and only if it satisfies the contract in full, verified by a backend conformance suite that every shipped adapter passes — the Rust adapter (`rust-analyzer scip` with `tree-sitter-rust`) as the founding exemplar, the Python adapter (`scip-python` with `tree-sitter-python`) as the second conforming implementation, and the ty-scip Python adapter (`ty-scip` with `tree-sitter-python`) as the third.

> Previously: the conformance suite named two shipped adapters, the Rust adapter and the scip-python Python adapter.

Serves: choose-python-indexer, index-large-python

#### Scenario: Definition occurrence extracted

- **GIVEN** a Rust project containing a symbol declaration
- **WHEN** the project is indexed through the backend contract
- **THEN** the symbol appears with its resolved descriptor and a definition-role occurrence whose range maps to the declaration's position

#### Scenario: Reference occurrence extracted

- **GIVEN** a Rust project where a symbol is used away from its declaration
- **WHEN** the project is indexed through the backend contract
- **THEN** a reference-role occurrence for that symbol is produced with a range that maps to the use site

#### Scenario: Python definition occurrence extracted

- **GIVEN** a Python project containing a function or class definition
- **WHEN** the project is indexed through the backend contract
- **THEN** the symbol appears with its resolved descriptor and a definition-role occurrence whose range maps to the definition's position

#### Scenario: Python reference occurrence extracted

- **GIVEN** a Python project where a symbol is used in a different module than the one defining it
- **WHEN** the project is indexed through the backend contract
- **THEN** a reference-role occurrence for that symbol is produced with a range that maps to the use site

#### Scenario: Each Python adapter passes the conformance suite

- **GIVEN** the shared Python conformance project
- **WHEN** it is indexed once through the scip-python adapter and once through the ty-scip adapter
- **THEN** the conformance suite reports each adapter conformant on its own index

#### Scenario: Conformance suite gates usability

- **GIVEN** a candidate backend that fails any clause of the extraction contract
- **WHEN** it is run against the conformance suite
- **THEN** the suite reports the backend as non-conformant and the system does not treat it as usable

#### Scenario: Backends gate independently

- **GIVEN** one conforming adapter and a second candidate adapter that fails a contract clause
- **WHEN** the conformance suite runs
- **THEN** the failing adapter is reported non-conformant and the conforming adapter's usability is unaffected

## ADDED Requirements

### Requirement: A workspace file the indexer rejects is a typed refusal

When the selected Python indexer rejects the workspace because a project file has no importable module name, indexing SHALL fail with a typed, actionable error that names the rejected file and the configuration remedy for its cause, and SHALL NOT produce or persist a partial index in its place.
The system SHALL NOT exclude the rejected file on the user's behalf.

Serves: honest-python-refusal

#### Scenario: A file carrying inline script metadata is refused with the script remedy

- **GIVEN** a Python workspace holding a file that begins with PEP 723 inline script metadata, built with the ty-scip indexer
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming that file and the setting that excludes script files, and no index is persisted from the attempt

#### Scenario: A file whose path is not an importable name is refused with the exclude remedy

- **GIVEN** a Python workspace holding a source file under a directory whose name is not a valid Python identifier, built with the ty-scip indexer
- **WHEN** a build is attempted
- **THEN** the build fails with a typed error naming that file and the exclude setting, and no index is persisted from the attempt

#### Scenario: A refused build leaves the existing store untouched

- **GIVEN** a workspace with an existing store, and a new file the ty-scip indexer rejects
- **WHEN** a build is attempted
- **THEN** the build fails with the typed error and the existing store is unchanged
