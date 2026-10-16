# Delta for Command Surface

## MODIFIED Requirements

### Requirement: Indexer readiness report

The system SHALL provide a command that reports, for each required language indexer, whether it is present and its version, and an install hint when it is absent, and SHALL signal an indexer/setup failure through the exit-code taxonomy when a required indexer is missing.
The command SHALL report each optional indexer the same way; an optional indexer is one that a build uses only when the build selects it explicitly or the store records it, and an optional indexer that is absent or unresponsive SHALL NOT signal the indexer/setup failure.
Each indexer's readiness probe SHALL complete within a bounded time, and an indexer that is present but does not respond within that bound SHALL be reported as unresponsive — distinct from absent — with an investigation hint, and, for a required indexer, SHALL likewise signal the indexer/setup failure through the exit-code taxonomy.

> Previously: the command reported only required indexers.

Serves: choose-python-indexer

#### Scenario: All indexers present

- **GIVEN** a host with every required language indexer and the optional ty-scip indexer installed
- **WHEN** the readiness command runs
- **THEN** each indexer is reported present with its version and the process exits with the success code

#### Scenario: A missing indexer is reported with a hint

- **GIVEN** a host missing a required language indexer
- **WHEN** the readiness command runs
- **THEN** that indexer is reported absent with an install hint and the process exits with the indexer/setup-failure code

#### Scenario: An unresponsive indexer is reported within a bounded time

- **GIVEN** a required indexer executable that hangs when probed
- **WHEN** the readiness command runs
- **THEN** it reports the indexer unresponsive with an investigation hint within a bounded time and exits with the indexer/setup-failure code

#### Scenario: An absent optional indexer does not fail the report

- **GIVEN** a host with every required language indexer installed and ty-scip absent
- **WHEN** the readiness command runs
- **THEN** ty-scip is reported absent with an install hint and the process exits with the success code

#### Scenario: An unresponsive optional indexer does not fail the report

- **GIVEN** a host with every required language indexer installed and a ty-scip executable that hangs when probed
- **WHEN** the readiness command runs
- **THEN** ty-scip is reported unresponsive with an investigation hint within a bounded time and the process exits with the success code
