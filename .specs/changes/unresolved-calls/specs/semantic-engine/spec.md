# Delta for Semantic Engine

## MODIFIED Requirements

### Requirement: Mandatory extraction contract

> Previously: a backend reported nothing about occurrences of symbols local to one document, such as parameters and local variables.

The system SHALL define a backend contract — the intersection every semantic backend must satisfy — under which a backend, given a project, produces the project's symbols with their resolved canonical descriptors and the occurrences of each symbol, where every occurrence is classified by role (at minimum, definition versus reference) and carries a source range that maps unambiguously to a file position.
Under the same contract, a backend SHALL also produce, for each document, the source range of every occurrence of a symbol local to that document, such as a parameter, a local variable, or a closure binding, without a symbol identity; these ranges SHALL NOT become persisted symbols or occurrences.
A backend SHALL be usable through this contract if and only if it satisfies the contract in full, verified by a backend conformance suite that every shipped adapter passes — the Rust adapter (`rust-analyzer scip` with `tree-sitter-rust`) as the founding exemplar, the Python adapter (`scip-python` with `tree-sitter-python`) as the second conforming implementation, and the ty-scip Python adapter (`ty-scip` with `tree-sitter-python`) as the third.

Serves: references-name-what-they-cannot-see

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

#### Scenario: Local occurrence ranges extracted

- **GIVEN** a Python function that calls its parameter as `callback()`, and a Rust function that calls its closure parameter as `callback()`
- **WHEN** each project is indexed through the backend contract, the Python project once through each Python adapter
- **THEN** the range of each `callback` token is reported as a local occurrence range of its document, and no persisted symbol or occurrence is created for it

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
