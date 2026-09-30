# Delta for Command Surface

## MODIFIED Requirements

### Requirement: Indexer readiness report

The system SHALL provide a command that reports, for each language indexer, whether it is present and its version, and an install hint when it is absent.
A language is ready when a build could select an indexer for it; for a language with alternative indexers, the report SHALL name the indexer a build without an explicit selection would use.
The command SHALL signal an indexer/setup failure through the exit-code taxonomy when any language is not ready, and SHALL NOT signal one when an alternative indexer is absent but its language is ready.
Each indexer's readiness probe SHALL complete within a bounded time, and an indexer that is present but does not respond within that bound SHALL be reported as unresponsive — distinct from absent — with an investigation hint; a language whose selected indexer is unresponsive is not ready.

> Previously: every listed indexer was required, and any absent or unresponsive indexer signaled the failure.

Serves: choose-python-indexer

#### Scenario: All indexers present

- **GIVEN** a host with rust-analyzer, scip-python, and ty-scip installed
- **WHEN** the readiness command runs
- **THEN** each indexer is reported present with its version, ty-scip is named as the Python indexer a build would use, and the process exits with the success code

#### Scenario: A missing indexer is reported with a hint

- **GIVEN** a host with no Rust indexer installed
- **WHEN** the readiness command runs
- **THEN** that indexer is reported absent with an install hint and the process exits with the indexer/setup-failure code

#### Scenario: An unresponsive indexer is reported within a bounded time

- **GIVEN** an indexer executable that hangs when probed, and no other indexer for its language
- **WHEN** the readiness command runs
- **THEN** it reports the indexer unresponsive with an investigation hint within a bounded time and exits with the indexer/setup-failure code

#### Scenario: One Python indexer absent, the other present

- **GIVEN** a host with rust-analyzer and scip-python installed and ty-scip absent
- **WHEN** the readiness command runs
- **THEN** ty-scip is reported absent with an install hint, scip-python is named as the Python indexer a build would use, and the process exits with the success code

#### Scenario: Both Python indexers absent

- **GIVEN** a host with rust-analyzer installed and neither Python indexer installed
- **WHEN** the readiness command runs
- **THEN** both Python indexers are reported absent with install hints and the process exits with the indexer/setup-failure code

#### Scenario: An unresponsive preferred Python indexer is not ready

- **GIVEN** a host where ty-scip is present but hangs when probed, and scip-python is available
- **WHEN** the readiness command runs
- **THEN** ty-scip is reported unresponsive with an investigation hint, Python is reported not ready, and the process exits with the indexer/setup-failure code
