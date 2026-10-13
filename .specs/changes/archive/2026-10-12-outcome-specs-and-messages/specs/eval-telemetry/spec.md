# Delta for Eval Telemetry Specification

## MODIFIED Requirements

### Requirement: Cross-Run Comparability

> Previously: the requirement and its scenarios stated which data a selection reads; the promise is unchanged.

Recorded trials SHALL be selectable by instance, arm, and instruction-set version, and each trial's record SHALL carry its pre-run task characteristics as recorded values, so that neither selecting trials nor reading those characteristics needs the trial's trajectory or task tree.

Serves: comparable-telemetry, reusable-runway

#### Scenario: Selection by arm and version

- **GIVEN** a store holding trials from two arms and two instruction-set versions
- **WHEN** trials are selected by one arm and one version
- **THEN** exactly the matching trials return

#### Scenario: Task characteristics read from the store

- **GIVEN** a store holding a paired run's trials
- **WHEN** the task-level analyses read each trial's pre-run task characteristics
- **THEN** each trial's pre-run task characteristics are present on its record
