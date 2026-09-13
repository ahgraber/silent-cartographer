# Delta for eval-telemetry

## ADDED Requirements

### Requirement: Complete Trial Capture

For every scheduled trial that recorded an outcome, the experiment store SHALL contain exactly one record carrying the trial's identity (dataset revision, instance, arm, agent, model, attempt, platform, and — for treatment trials — instruction-set version), the pre-run task characteristics recorded with its task, its terminal state, its cost totals, its reward scores, its invocation counts, and its raw trajectory and reward artifacts where they exist.

A trial killed before it recorded an outcome SHALL NOT be imported, because its instance is not recoverable from its outputs; such a trial SHALL be reported as pending so it can be run again.

Serves: comparable-telemetry, evidence-of-utility

#### Scenario: Graded treatment trial captured

- **GIVEN** a completed treatment trial with a parsed answer
- **WHEN** import runs over the trial's outputs
- **THEN** one record exists carrying identity, the task's pre-run characteristics, cost totals, reward scores, uptake, and attached trajectory and reward artifacts

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

#### Scenario: Trial killed before recording an outcome reported as pending

- **GIVEN** a scheduled trial whose run was interrupted before it wrote its outcome
- **WHEN** import runs and the arm's pending instances are listed
- **THEN** no record exists for that trial and its instance is listed as pending

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

### Requirement: Token Category Capture

For every trial, the experiment store SHALL record each token category the trial's trajectory reports, and SHALL record the model, provider, harness, and caching configuration the trial ran under; a token category the trajectory does not report SHALL be absent from the record rather than recorded as zero.

Serves: comparable-telemetry, evidence-of-utility

#### Scenario: Reported categories recorded

- **GIVEN** a trial whose trajectory reports input, cached-input, output, and reasoning token counts
- **WHEN** import runs
- **THEN** the record carries each of those categories alongside the trial's serving configuration

#### Scenario: Unreported category absent

- **GIVEN** a trial whose trajectory reports no cache-write token count
- **WHEN** import runs
- **THEN** the record carries no cache-write figure and substitutes no zero

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

Recorded trials SHALL be selectable by instance, arm, and instruction-set version, and their pre-run task characteristics SHALL be readable, without reading raw trajectories.

Serves: comparable-telemetry, reusable-runway

#### Scenario: Selection by arm and version

- **GIVEN** a store holding trials from two arms and two instruction-set versions
- **WHEN** trials are selected by one arm and one version
- **THEN** exactly the matching trials return, and the selection reads no raw trajectory

#### Scenario: Task characteristics read from the store

- **GIVEN** a store holding a paired run's trials
- **WHEN** the task-level analyses read each trial's pre-run task characteristics
- **THEN** the values return from the store and no task tree or raw trajectory is read
