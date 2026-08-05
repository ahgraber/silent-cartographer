# Delta for Command Surface

## MODIFIED Requirements

### Requirement: Exit-code taxonomy

> Previously: the taxonomy's named distinctions were success, usage error, absent index, and indexer or setup failure; store-related refusals were not individually named categories.

The system SHALL signal each invocation's outcome through a closed exit-code taxonomy, using a distinct code per category, that distinguishes success — including a typed-empty result — from a usage error, an absent index, a store unusable because its recorded schema version differs, a target refused because the system cannot confirm it is a store the system created, and an indexer or setup failure.

Serves: own-store-only

#### Scenario: Success on a non-empty answer

- **GIVEN** an indexed workspace
- **WHEN** a query returns one or more results
- **THEN** the process exits with the success code

#### Scenario: A typed-empty answer is still success

- **GIVEN** an indexed workspace
- **WHEN** a query resolves correctly but stands in no instance of the requested relation
- **THEN** the process exits with the success code, not a failure code

#### Scenario: Usage error is distinct

- **GIVEN** any command
- **WHEN** it is invoked with an invalid flag or malformed argument
- **THEN** the process exits with the usage-error code, distinct from the success and failure codes

#### Scenario: Absent index is distinct

- **GIVEN** a directory with no built index
- **WHEN** a query command is invoked
- **THEN** the process exits with the no-index code, distinct from a usage error

#### Scenario: Incompatible store is distinct

- **GIVEN** a store the system created under a different schema version than the binary expects
- **WHEN** a query command runs against it
- **THEN** the process exits with the incompatible-store code, distinct from the no-index code

#### Scenario: Ownership refusal is distinct

- **GIVEN** a file at the index path that the system does not recognize as its own store
- **WHEN** a build, query, or reset command runs against it
- **THEN** the process exits with the unrecognized-store code, distinct from the absent-index and incompatible-store codes

#### Scenario: Indexer setup failure is distinct

- **GIVEN** a workspace whose required language indexer is not installed
- **WHEN** a command that needs it runs
- **THEN** the process exits with the indexer/setup-failure code, distinct from the other categories

### Requirement: Versioned structural surface index

> Previously: the index state reported by the surface-index command did not distinguish an absent index from a file at the index path the system had not created, could not examine, or had created under a schema version it no longer reads.

The system SHALL provide a command that emits, as its machine-readable answer, a structural index of the CLI surface — a surface version, the commands with each command's valid argument and flag values and defaults, and the current index state — and the surface version SHALL change whenever the command or flag structure changes, so that a caller can detect that its understanding of the surface is out of date.
The reported index state SHALL distinguish an absent index, a file at the index path that is not recognized as the system's own store, a path at the index path whose contents cannot be examined, and a store the system recognizes as its own whose recorded schema version differs from the version the binary expects, without failing the command.

Serves: own-store-only

#### Scenario: Index enumerates the command surface

- **GIVEN** an installed build
- **WHEN** the surface-index command runs
- **THEN** its answer lists the commands with each command's valid argument and flag values and defaults

#### Scenario: Index carries a surface version

- **GIVEN** an installed build
- **WHEN** the surface-index command runs
- **THEN** its answer carries a surface version identifier

#### Scenario: Surface version distinguishes revisions

- **GIVEN** two builds whose command-or-flag structure differs
- **WHEN** the surface-index command runs against each
- **THEN** the two answers carry different surface versions

#### Scenario: Index state distinguishes unrecognized from absent

- **GIVEN** a file at the index path that the system does not recognize as its own store
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports the unrecognized condition, distinct from an absent index, and the command succeeds

#### Scenario: Index state distinguishes an unexaminable path

- **GIVEN** a path at the index path whose contents cannot be read at all, such as a directory
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports that condition, distinct from both an absent index and an unrecognized file, and the command succeeds

#### Scenario: Index state distinguishes an incompatible store

- **GIVEN** a store the system created at the index path, recorded under a different schema version than the binary expects
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports the incompatible condition, distinct from an absent index, and the command succeeds

### Requirement: Index reset

> Previously: recognition required only that the target be a database carrying any nonzero version stamp — a test that also matches databases other applications version, leaving reset willing to delete them.

The system SHALL provide a command that removes the stored index for the workspace and reports the path affected; removal SHALL apply only to an index store the system recognizes as its own creation, and any other target — including a database another application created and versioned — SHALL be refused as a failure naming the manual alternative, leaving the target intact.
Removing an index that is already absent SHALL be a success, and an operating-system error encountered while removing it SHALL be reported as a failure naming the affected path.

Serves: own-store-only

#### Scenario: Existing index removed

- **GIVEN** a workspace with a stored index
- **WHEN** the reset command runs
- **THEN** the index is removed, the affected path is reported, and the process exits with the success code

#### Scenario: A non-index target is refused

- **GIVEN** a file at the index path that is not a recognizable index store
- **WHEN** the reset command runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: A foreign versioned database is refused

- **GIVEN** a database file at the index path that another application created and stamped with its own version
- **WHEN** the reset command runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: Nothing to remove is success

- **GIVEN** a workspace with no stored index
- **WHEN** the reset command runs
- **THEN** the process exits with the success code rather than a failure

#### Scenario: Removal error names the path

- **GIVEN** a stored index that cannot be removed because of an operating-system error
- **WHEN** the reset command runs
- **THEN** the failure is reported naming the affected path and the process exits with a failure code
