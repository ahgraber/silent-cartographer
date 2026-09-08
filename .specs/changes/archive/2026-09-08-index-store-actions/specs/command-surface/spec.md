# Delta for command-surface

## MODIFIED Requirements

### Requirement: Index reset

> Previously: naming the index-store command with no further argument removed the store.

The system SHALL provide a command that removes the stored index for the workspace and reports the path affected; removal SHALL apply only to an index store the system recognizes as its own creation, and any other target — including a database another application created and versioned — SHALL be refused as a failure naming the manual alternative, leaving the target intact.
Removing an index that is already absent SHALL be a success, and an operating-system error encountered while removing it SHALL be reported as a failure naming the affected path.
The removal SHALL be reachable only through an explicitly named action of that command; an invocation naming the command without an action SHALL be rejected as a usage error that names the valid actions, and SHALL leave any store at the path intact.
When the store path is itself a symbolic link, the removal SHALL remove the link, SHALL leave the store the link names in place, and SHALL disclose that the link rather than the store was removed, so that success is never reported for an index that still exists.

Serves: deletion-is-named

#### Scenario: Existing index removed

- **GIVEN** a workspace with a stored index
- **WHEN** the removal action runs
- **THEN** the index is removed, the affected path is reported, and the process exits with the success code

#### Scenario: A non-index target is refused

- **GIVEN** a file at the index path that is not a recognizable index store
- **WHEN** the removal action runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: A foreign versioned database is refused

- **GIVEN** a database file at the index path that another application created and stamped with its own version
- **WHEN** the removal action runs
- **THEN** the file is left intact and the refusal names the manual removal alternative, exiting with a failure code

#### Scenario: Nothing to remove is success

- **GIVEN** a workspace with no stored index
- **WHEN** the removal action runs
- **THEN** the process exits with the success code rather than a failure

#### Scenario: Removal error names the path

- **GIVEN** a stored index that cannot be removed because of an operating-system error
- **WHEN** the removal action runs
- **THEN** the failure is reported naming the affected path and the process exits with a failure code

#### Scenario: A symbolic link at the store path is removed and disclosed

- **GIVEN** a store path that is a symbolic link naming a recognizable index store elsewhere
- **WHEN** the removal action runs
- **THEN** the link is removed, the store it names is left in place, and the answer discloses that the link rather than the store was removed

#### Scenario: Naming no action removes nothing

- **GIVEN** a workspace with a stored index
- **WHEN** the index-store command runs with no action named
- **THEN** the invocation is rejected with the usage code, the rejection names the valid actions, and the index is still present

## ADDED Requirements

### Requirement: Index store location report

The system SHALL provide an action that reports the directory containing the index store at the discovered store path, and SHALL report it whether or not a store exists at that path, exiting with the success code in both cases.
The reported directory SHALL be derived from the store path as given, without resolving it against the workspace root, and a store path naming no parent directory SHALL report the working directory rather than an empty answer.
The machine-readable answer SHALL carry the store path itself alongside the directory.

Serves: locate-and-measure-the-store

#### Scenario: Directory reported for an existing store

- **GIVEN** a workspace with a stored index
- **WHEN** the location action runs
- **THEN** the directory containing the store is reported and the process exits with the success code

#### Scenario: Directory reported when no store exists

- **GIVEN** a workspace with no stored index
- **WHEN** the location action runs
- **THEN** the directory the store would occupy is reported and the process exits with the success code

#### Scenario: A store path without a parent reports the working directory

- **GIVEN** a store path that is a bare filename
- **WHEN** the location action runs
- **THEN** the reported directory is the working directory rather than an empty answer

#### Scenario: Machine answer carries the store path

- **GIVEN** any store path
- **WHEN** the location action runs with the machine-readable output requested
- **THEN** the answer carries both the directory and the store path

### Requirement: Index store size report

The system SHALL provide an action that reports the on-disk size of the index store as the total bytes of every file the removal action would delete, whenever the store path holds a store the system recognizes as its own, including a store recorded under a schema version the binary cannot read.
For any other target — an absent store, a file the system does not recognize as its own, or a path whose contents cannot be examined — the action SHALL NOT report a size, and SHALL exit with the success code rather than a failure.
When a store the system recognizes as its own cannot be measured, the action SHALL report its state without a size rather than report a size that understates what is at the path, and SHALL exit with the success code.
The machine-readable answer SHALL name the target's state in every case, using the same distinctions the structural surface index reports.

Serves: locate-and-measure-the-store

#### Scenario: Size of a recognized store

- **GIVEN** a workspace with a stored index the system recognizes as its own
- **WHEN** the size action runs
- **THEN** the store's byte count is reported and the process exits with the success code

#### Scenario: Auxiliary files count toward the total

- **GIVEN** a recognized store accompanied by the auxiliary files the removal action deletes alongside it
- **WHEN** the size action runs
- **THEN** the reported total covers the store and those auxiliary files together

#### Scenario: An incompatible store reports a size

- **GIVEN** a store the system created, recorded under a schema version the binary does not read
- **WHEN** the size action runs
- **THEN** its byte count is reported, the answer names the incompatible state, and the process exits with the success code

#### Scenario: A recognized store that cannot be measured reports no size

- **GIVEN** a store the system recognizes as its own whose size cannot be established, such as one accompanied by an auxiliary file that cannot be examined
- **WHEN** the size action runs
- **THEN** the answer names the recognized state, reports no byte count, and the process exits with the success code

#### Scenario: An absent store reports no size

- **GIVEN** a workspace with no stored index
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names the absent state, and the process exits with the success code

#### Scenario: An unrecognized file reports no size

- **GIVEN** a file at the store path that the system does not recognize as its own store
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names the unrecognized state, and the process exits with the success code

#### Scenario: An unexaminable path reports no size

- **GIVEN** a path at the store path whose contents cannot be read at all, such as a directory
- **WHEN** the size action runs
- **THEN** no byte count is reported, the answer names that state distinctly from both absent and unrecognized, and the process exits with the success code
