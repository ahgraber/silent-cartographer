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
