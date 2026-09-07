# Delta for eval-telemetry

## ADDED Requirements

### Requirement: Complete Trial Capture

For every scheduled trial, the experiment store SHALL contain exactly one record carrying the trial's identity (instance, arm, agent, model, and — for treatment trials — instruction-set version), its terminal state, its cost totals, its reward scores, its invocation counts, and its raw trajectory and reward artifacts where they exist.

Serves: comparable-telemetry, evidence-of-utility

#### Scenario: Graded treatment trial captured

- **GIVEN** a completed treatment trial with a parsed answer
- **WHEN** import runs over the trial's outputs
- **THEN** one record exists carrying identity, cost totals, reward scores, uptake, and attached trajectory and reward artifacts

#### Scenario: Unparsed trial still captured

- **GIVEN** a completed trial whose answer was scored as empty for being unparseable
- **WHEN** import runs
- **THEN** one record exists carrying the empty-answer scores and the unparsed marker

#### Scenario: Baseline trial captured with zero uptake

- **GIVEN** a completed baseline trial
- **WHEN** import runs
- **THEN** one record exists and its uptake count is zero

#### Scenario: Agent-failure trial captured

- **GIVEN** a scheduled trial that ended in timeout or budget exhaustion
- **WHEN** import runs
- **THEN** one record exists carrying the agent-failure terminal state, empty-answer reward scores, and the cost totals consumed before failure

#### Scenario: Infrastructure-failure trial captured

- **GIVEN** a scheduled trial that failed on provisioning before the agent acted
- **WHEN** import runs
- **THEN** one record exists carrying the infrastructure-failure terminal state and no reward scores

### Requirement: Idempotent Import

Importing a trial that is already recorded SHALL NOT create a second record or alter the existing record's identity; an import interrupted partway and re-run SHALL converge to the same store state as a single complete import; importing outputs whose trial identity matches an existing record but whose artifact content differs SHALL fail with an explicit error and SHALL NOT alter the existing record.

Serves: comparable-telemetry

#### Scenario: Novel trial imported once

- **GIVEN** a trial not yet present in the store
- **WHEN** import runs
- **THEN** exactly one record for the trial exists

#### Scenario: Re-import creates no duplicate

- **GIVEN** a trial already present in the store
- **WHEN** import runs again over the same outputs
- **THEN** exactly one record for the trial exists

#### Scenario: Interrupted import converges on re-run

- **GIVEN** an import that stopped after recording some of a run's trials
- **WHEN** import runs again over the same run
- **THEN** the store holds exactly one record per trial, identical to the state a single uninterrupted import produces

#### Scenario: Changed artifacts rejected

- **GIVEN** a recorded trial whose source artifacts have changed since its import
- **WHEN** import runs again over those outputs
- **THEN** the import reports an explicit error for that trial and the existing record is unchanged

### Requirement: Invocation Count Fidelity

For every trial, the recorded uptake count SHALL equal the number of c10r invocations observable in that trial's trajectory, and the recorded search count SHALL equal the number of search-tool invocations (grep, glob, and equivalent search commands) observable in the same trajectory.

Serves: instruction-set, comparable-telemetry, evidence-of-utility

#### Scenario: Trajectory with invocations

- **GIVEN** a trial trajectory containing N c10r invocations
- **WHEN** import runs
- **THEN** the trial's recorded uptake count is N

#### Scenario: Trajectory without invocations

- **GIVEN** a trial trajectory containing no c10r invocation
- **WHEN** import runs
- **THEN** the trial's recorded uptake count is zero

#### Scenario: Mixed usage counted separately

- **GIVEN** a trial trajectory containing one c10r invocation and twenty grep invocations
- **WHEN** import runs
- **THEN** the trial's recorded uptake count is one and its recorded search count is twenty

### Requirement: Cross-Run Comparability

Recorded trials SHALL be selectable by instance, arm, and instruction-set version without reading raw trajectories.

Serves: comparable-telemetry, reusable-runway

#### Scenario: Selection by arm and version

- **GIVEN** a store holding trials from two arms and two instruction-set versions
- **WHEN** trials are selected by one arm and one version
- **THEN** exactly the matching trials return, and the selection reads no raw trajectory
