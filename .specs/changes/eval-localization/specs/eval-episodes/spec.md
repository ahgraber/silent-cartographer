# Delta for eval-episodes

## ADDED Requirements

### Requirement: Instance Fidelity

For any instance in the selected source-dataset split, task generation SHALL produce exactly one localization episode, materialized as exactly one task per (instance, arm), whose environment provides the instance's repository at the instance's base commit and whose instruction presents the instance's issue.

Serves: evidence-of-utility, reusable-runway

#### Scenario: Generated task reflects its instance

- **GIVEN** a dataset instance with repository R, base commit C, and issue text I
- **WHEN** task generation runs over the split containing that instance
- **THEN** the produced task's environment contains R checked out at C, and the task's instruction contains I

#### Scenario: One task per instance and arm

- **GIVEN** a split containing N instances and two arms
- **WHEN** task generation completes
- **THEN** exactly N tasks exist per arm, and each task records the identity of the instance and arm it derives from

### Requirement: Runner-Compatible Task Format

Every generated task SHALL be a valid Harbor-format task that the selected runner accepts without task-specific runner modifications.

Serves: reusable-runway

#### Scenario: Generated task runs unmodified

- **GIVEN** a generated localization task
- **WHEN** the runner is pointed at the task directory
- **THEN** the runner executes the task end to end without changes to the runner or the task

### Requirement: Deterministic Generation

Task generation SHALL be deterministic: two generation runs over the same dataset revision with the same generator version SHALL produce equivalent task definitions.

Serves: comparable-telemetry, reusable-runway

#### Scenario: Regeneration equivalence

- **GIVEN** a completed generation run over a pinned dataset revision
- **WHEN** generation runs again over the same revision with the same generator version
- **THEN** the produced task definitions are equivalent to the first run's

### Requirement: Localization Episode Contract

Every generated task SHALL require the agent to identify the set of files (and optionally symbols within them) that must change to resolve the issue, to deliver that answer in the declared structured answer format, and to stop without modifying the repository; the task SHALL NOT require test execution.

Serves: evidence-of-utility

#### Scenario: Instruction demands a structured answer and no patch

- **GIVEN** a generated localization task
- **WHEN** its instruction is inspected
- **THEN** the instruction states the structured answer format, asks for the files that must change, and directs the agent not to modify the repository

#### Scenario: Episode completes without test execution

- **GIVEN** a generated localization task
- **WHEN** the task runs to completion
- **THEN** no step of the task lifecycle requires executing the repository's tests

### Requirement: Gold Answer Derivation

For any instance, the task's gold answer SHALL be exactly the set of files changed by the instance's historical code fix, excluding changes that only affect tests.

Serves: evidence-of-utility

#### Scenario: Single-file fix

- **GIVEN** an instance whose code fix changes one file
- **WHEN** the task is generated
- **THEN** the gold answer is exactly that file

#### Scenario: Multi-file fix

- **GIVEN** an instance whose code fix changes several files
- **WHEN** the task is generated
- **THEN** the gold answer is exactly that file set

#### Scenario: Test changes excluded

- **GIVEN** an instance that carries test modifications separately from its code fix
- **WHEN** the task is generated
- **THEN** no test-only file appears in the gold answer

### Requirement: Grading Totality

The verifier SHALL record a score for every completed trial; a trial whose answer is missing, malformed, or unparseable SHALL be scored as an empty answer, SHALL be marked as unparsed, and SHALL NOT abort the trial or the run.

Serves: evidence-of-utility, comparable-telemetry

#### Scenario: Well-formed answer graded

- **GIVEN** a completed trial whose answer parses in the declared format
- **WHEN** the verifier grades the trial
- **THEN** the trial's recorded score derives from the parsed answer

#### Scenario: Malformed answer scored as empty

- **GIVEN** a completed trial whose answer does not parse in the declared format
- **WHEN** the verifier grades the trial
- **THEN** the trial receives the score of an empty answer, carries an unparsed marker, and the run continues

#### Scenario: Missing answer scored as empty

- **GIVEN** a completed trial that produced no answer
- **WHEN** the verifier grades the trial
- **THEN** the trial receives the score of an empty answer, carries an unparsed marker, and the run continues

### Requirement: Accuracy Scoring

For any parsed answer, the verifier SHALL emit file-level precision, recall, F1, and a binary any-gold-file hit, each computed against the task's gold answer as a measure of historical-fix file recovery, and these scores SHALL constitute the trial's reward; symbol entries in an answer SHALL NOT affect the reward.

Serves: evidence-of-utility

#### Scenario: Exact match

- **GIVEN** a parsed answer whose file set equals the gold answer
- **WHEN** the verifier grades the trial
- **THEN** precision, recall, and F1 are all 1 and the any-gold-file hit is true

#### Scenario: Partial overlap

- **GIVEN** a parsed answer that contains some gold files and some non-gold files
- **WHEN** the verifier grades the trial
- **THEN** precision and recall each reflect the respective overlap ratio and the any-gold-file hit is true

#### Scenario: Disjoint answer

- **GIVEN** a parsed answer that contains no gold file
- **WHEN** the verifier grades the trial
- **THEN** precision, recall, and F1 are all 0 and the any-gold-file hit is false

#### Scenario: Symbols do not change the reward

- **GIVEN** two parsed answers with the same file set, one with symbol entries and one without
- **WHEN** the verifier grades both
- **THEN** both receive identical reward scores
