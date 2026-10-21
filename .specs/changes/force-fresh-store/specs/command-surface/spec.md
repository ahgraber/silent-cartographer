# Delta for Command Surface

## ADDED Requirements

### Requirement: A forced build requires confirmation

A forced build SHALL proceed only when it is confirmed, either by an affirmative answer to a question the build asks or by the build's confirmation flag.
The build SHALL ask only when standard input and standard error are both attached to a terminal and the flag is absent, and SHALL NOT ask in a build that a refresh hook runs.
The question SHALL state that the build replaces the stored index.
An answer other than an affirmative one, including an empty answer or the end of input, SHALL refuse the build.
A forced build that is neither confirmed by the flag nor able to ask SHALL be refused with a usage error that names the confirmation flag.
A refused forced build SHALL change no store and remove no file.
The confirmation flag SHALL be accepted only together with the force flag; given alone, it SHALL be rejected as a usage error.

Serves: a-forced-build-is-a-choice

#### Scenario: The flag confirms a forced build without a terminal

- **GIVEN** a workspace with a built index
- **WHEN** a forced build runs without a terminal and with the confirmation flag
- **THEN** no question is asked and the build replaces the store

#### Scenario: A forced build without a terminal or the flag is refused

- **GIVEN** a workspace with a built index
- **WHEN** a forced build runs without a terminal and without the confirmation flag
- **THEN** the build exits with the usage-error code, the diagnostic names the confirmation flag, and the store is unchanged

#### Scenario: A yes at the terminal confirms a forced build

- **GIVEN** a workspace with a built index
- **WHEN** a forced build runs at a terminal without the confirmation flag and the user answers yes
- **THEN** the question states that the build replaces the stored index, and the build replaces the store

#### Scenario: Any other answer refuses a forced build

- **GIVEN** a workspace with a built index
- **WHEN** a forced build runs at a terminal without the confirmation flag and the user answers no, answers nothing, or closes the input
- **THEN** the build is refused and the store is unchanged

#### Scenario: The confirmation flag alone is a usage error

- **GIVEN** any workspace
- **WHEN** a build runs with the confirmation flag and without the force flag
- **THEN** the build exits with the usage-error code and no store changes

## MODIFIED Requirements

### Requirement: Non-interactive operation

> Previously: no command asked a question.

The system SHALL complete every command without interactive prompting, taking all input from arguments and flags, and SHALL treat a non-terminal environment as headless.
The exception is a confirmation question that a requirement names; such a question SHALL be asked only when standard input and standard error are both attached to a terminal, and never in a build that a refresh hook runs.

Serves: a-forced-build-is-a-choice

#### Scenario: Runs to completion without a terminal

- **GIVEN** a command invoked with standard input and output not attached to a terminal
- **WHEN** it runs
- **THEN** it completes without waiting for interactive input
