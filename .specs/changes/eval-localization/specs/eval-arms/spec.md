# Delta for eval-arms

## ADDED Requirements

### Requirement: Arm Parity

For each instance, the baseline trial and the treatment trial SHALL differ only in c10r availability and c10r instructions: task instruction, agent, model, resource caps, and effective tool policy SHALL be identical across the two arms, except that the treatment arm's tool policy additionally admits c10r invocations; each arm's effective tool policy SHALL be recorded with its runs.

Serves: evidence-of-utility

#### Scenario: Task instruction identical across arms

- **GIVEN** the baseline and treatment task generated from the same instance
- **WHEN** their task instructions are compared
- **THEN** the instructions are identical

#### Scenario: Run configuration identical across arms

- **GIVEN** a paired run of both arms
- **WHEN** the two arms' run configurations are compared
- **THEN** agent, model, and every resource cap are identical, and the only differences are c10r availability and the c10r instruction prompt

#### Scenario: Tool policy differs only by c10r admission

- **GIVEN** the recorded effective tool policies of a paired run's two arms
- **WHEN** the allowed and disallowed tool lists are compared
- **THEN** the lists are identical except for the treatment arm's entry admitting c10r invocations

### Requirement: Treatment Provisioning

In every treatment-arm task environment, c10r SHALL be executable and SHALL answer queries against a pre-built index of the task's repository without network access.

Serves: evidence-of-utility, instruction-set

#### Scenario: Offline c10r query inside the treatment environment

- **GIVEN** a treatment-arm task environment with no network access
- **WHEN** a c10r query runs against the task's repository
- **THEN** c10r returns an answer from the pre-built index without error

### Requirement: Index Build Cost Recording

The build manifest SHALL carry, for each treatment-arm task whose image was built, that task's index build wall-clock time and resulting index size, so the one-time cost of adopting c10r stays measurable outside the paired comparison.
Merging further build records into a manifest SHALL preserve the entries earlier builds recorded, and SHALL replace an entry only for a task the current records measure again.

The manifest is a captured artifact rather than report content; no requirement obliges the report to render it.

Serves: evidence-of-utility

#### Scenario: Manifest carries each built task's index cost

- **GIVEN** build records from a treatment-arm image build
- **WHEN** they are merged into the build manifest
- **THEN** the manifest carries each recorded task's index build wall-clock time and index size

#### Scenario: An interrupted build keeps what it measured

- **GIVEN** a manifest holding entries from an earlier build
- **WHEN** records from a later build covering only some of those tasks are merged
- **THEN** the manifest retains the entries the later build did not measure and replaces those it did

#### Scenario: An incomplete record is refused

- **GIVEN** a build record missing the task identity, the wall-clock time, or the index size
- **WHEN** it is merged
- **THEN** the merge is refused and the existing manifest is left unchanged

### Requirement: Baseline Purity

Baseline-arm task environments SHALL NOT contain c10r, and baseline-arm agent context SHALL NOT reference c10r.

Serves: evidence-of-utility

#### Scenario: Baseline environment has no c10r

- **GIVEN** a baseline-arm task environment
- **WHEN** the environment is inspected
- **THEN** no c10r executable or index is present

#### Scenario: Baseline context has no c10r reference

- **GIVEN** a baseline-arm trial
- **WHEN** the agent's instructions and appended context are inspected
- **THEN** c10r is not mentioned

### Requirement: Instruction-Set Version Identity

Every treatment trial SHALL be attributable to exactly one recorded instruction-set version, and trials run under different instruction-set versions SHALL be distinguishable in all recorded results.

Serves: instruction-set, comparable-telemetry

#### Scenario: Trial carries its instruction version

- **GIVEN** a completed treatment trial
- **WHEN** its recorded results are inspected
- **THEN** the results name exactly one instruction-set version

#### Scenario: Versions distinguishable across runs

- **GIVEN** two treatment runs executed under different instruction-set versions
- **WHEN** their recorded results are queried
- **THEN** each trial is attributable to the version it ran under

### Requirement: Recorded Settings Immutability

Rendering a run configuration into a directory that already holds trials SHALL be refused when a previously recorded setting has changed, so that a rerun cannot relabel trials produced under different settings; a refusal SHALL leave every arm's recorded settings unaltered.

Serves: comparable-telemetry, evidence-of-utility

#### Scenario: Changed setting refuses the render

- **GIVEN** a run directory holding trials recorded under a given model and resource caps
- **WHEN** a configuration with a different model is rendered into it
- **THEN** the render is refused and names the settings that changed

#### Scenario: Refusal leaves the directory untouched

- **GIVEN** a render that is refused for one arm
- **WHEN** the run directory is inspected
- **THEN** no arm's recorded settings have changed

#### Scenario: A setting the record never carried is not a conflict

- **GIVEN** a run directory whose recorded settings predate a newly added setting
- **WHEN** a configuration carrying that setting is rendered into it
- **THEN** the render proceeds and records the new setting

### Requirement: Blocked Arm Interleaving

The runner SHALL divide the stable issue order into fixed-size blocks and run both arms within each block, and SHALL reverse which arm leads from one block to the next, so that no arm is confined to one window of the run.

Serves: evidence-of-utility

#### Scenario: Both arms run within each block

- **GIVEN** a paired sweep over a subset spanning several blocks
- **WHEN** the run plan is inspected
- **THEN** every block schedules both arms over the same issues

#### Scenario: Leading arm alternates between blocks

- **GIVEN** a run plan spanning at least two blocks
- **WHEN** the leading arm of consecutive blocks is compared
- **THEN** the leading arm differs between them

### Requirement: Dev-Frozen Separation

The partition of instances into a development subset and a frozen subset SHALL be recorded before instruction-set iteration begins; reported results SHALL derive only from frozen-subset trials executed with the frozen instruction-set version.

Serves: instruction-set, evidence-of-utility

#### Scenario: Development trials excluded from the report

- **GIVEN** trials on development-subset instances run during instruction-set iteration
- **WHEN** the reported analysis runs
- **THEN** no development-subset trial contributes to the reported results

#### Scenario: Frozen results use the frozen version

- **GIVEN** the frozen subset and a frozen instruction-set version
- **WHEN** the reported run executes
- **THEN** every reported treatment trial carries the frozen instruction-set version
