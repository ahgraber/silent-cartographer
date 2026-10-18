# Delta for Command Surface

## MODIFIED Requirements

### Requirement: Exit-code taxonomy

The system SHALL signal each invocation's outcome through a closed exit-code taxonomy, using a distinct code per category, that distinguishes success — including a typed-empty result — from a usage error, an absent index, a store unusable because its recorded schema version differs, a target refused because the system cannot confirm it is a store the system created, an indexer or setup failure, and an answer the index's tier cannot give: one whose outcome is `unavailable`, or an `impact` answer whose dependents are unavailable.
Such an answer SHALL still be written to standard output in full.

> Previously: the taxonomy had no category for an answer the index's tier cannot give.

Serves: calibrated-capability-disclosure

#### Scenario: Success on a non-empty answer

- **GIVEN** an indexed workspace
- **WHEN** a query returns one or more results, none of them an answer the index's tier cannot give
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

#### Scenario: An unavailable answer is distinct and still written

- **GIVEN** a structural index
- **WHEN** `trace` over the `dependents` relation is invoked with `--json`
- **THEN** the process exits with the unavailable code, distinct from success and from every failure code, and standard output holds the full `unavailable` answer

#### Scenario: A structural impact answer with seeds exits with the unavailable code

- **GIVEN** a structural Bash index, and a diff that changes the body of an indexed function
- **WHEN** `impact` is invoked with `--json`
- **THEN** the process exits with the unavailable code, and standard output holds the full answer with its seeds

#### Scenario: A structural impact answer that touches nothing is success

- **GIVEN** a structural Bash index, and a diff that changes only a comment outside every function
- **WHEN** `impact` is invoked
- **THEN** the process exits with the success code

### Requirement: Non-interactive operation

The system SHALL complete every command without interactive prompting, taking all input from arguments and flags, and SHALL treat a non-terminal environment as headless.
The one exception is the structural-index confirmation of a Rust or Python build, which SHALL be asked only when standard input and standard error are both attached to a terminal, and never in a build that a refresh hook runs.

> Previously: no command asked a question.

Serves: consent-before-structural-index

#### Scenario: Runs to completion without a terminal

- **GIVEN** a command invoked with standard input and output not attached to a terminal
- **WHEN** it runs
- **THEN** it completes without waiting for interactive input

#### Scenario: A headless build with an unavailable indexer does not ask

- **GIVEN** a Python workspace on a host without the selected indexer
- **WHEN** a build runs with standard input not attached to a terminal
- **THEN** it completes without waiting for input

### Requirement: Versioned structural surface index

The system SHALL provide a command that emits, as its machine-readable answer, a structural index of the CLI surface — a surface version, the commands with each command's valid argument and flag values and defaults, and the current index state — and the surface version SHALL change whenever the command or flag structure changes, so that a caller can detect that its understanding of the surface is out of date.
The reported index state SHALL distinguish an absent index, a file at the index path that is not recognized as the system's own store, a path at the index path whose contents cannot be examined, and a store the system recognizes as its own whose recorded schema version differs from the version the binary expects, without failing the command.
For a built index, the reported index state SHALL state whether the index is structural or semantic.

> Previously: the index state did not name a tier, because every index was semantic.

Serves: calibrated-capability-disclosure

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

#### Scenario: Index state names a structural tier

- **GIVEN** a structural Bash index at the index path
- **WHEN** the surface-index command runs
- **THEN** the answer's index state reports a built index whose tier is structural
