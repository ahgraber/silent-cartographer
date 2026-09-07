# Delta for eval-analysis

## ADDED Requirements

### Requirement: Paired Comparison

Arm comparisons SHALL be computed from per-instance deltas over instances whose trials reached an agent-attributable terminal state in both arms, with agent failures scored under the declared failure policy; an instance with an unresolved infrastructure failure in either arm SHALL be excluded from paired statistics and enumerated in the report.

Serves: evidence-of-utility

#### Scenario: Instance with both arms complete

- **GIVEN** an instance with a completed trial in each arm
- **WHEN** the paired analysis runs
- **THEN** the instance contributes one per-instance delta per metric

#### Scenario: Agent failure contributes as an outcome

- **GIVEN** an instance whose treatment trial ended in budget exhaustion
- **WHEN** the paired analysis runs
- **THEN** the instance contributes deltas computed from that trial's declared failure scores, not an exclusion

#### Scenario: Unresolved infrastructure failure excluded

- **GIVEN** an instance whose trial in one arm never reached an agent-attributable state despite the rerun policy
- **WHEN** the paired analysis runs
- **THEN** the instance contributes to no paired statistic and appears in the report's exclusion list

### Requirement: Intent-To-Treat

Arm-level results SHALL include every trial of the arm that reached an agent-attributable terminal state, regardless of measured uptake, and no reported headline result SHALL be conditioned on uptake.

Serves: evidence-of-utility, instruction-set

#### Scenario: Zero-uptake treatment trial included

- **GIVEN** a completed treatment trial whose uptake count is zero
- **WHEN** arm-level results are computed
- **THEN** the trial is included in the treatment arm's results

#### Scenario: Uptake reported alongside not instead

- **GIVEN** a completed analysis
- **WHEN** the report is inspected
- **THEN** uptake appears as its own summary and no headline comparison filters trials by uptake

### Requirement: Declared Decision Criteria

The analysis SHALL evaluate cost superiority and accuracy non-inferiority against criteria declared before the frozen run — including the primary cost metric, the non-inferiority margin and method, and the failure-state scoring policy — and SHALL report each criterion's outcome alongside the observed effect sizes.

Serves: evidence-of-utility

#### Scenario: Both criteria reported

- **GIVEN** a completed frozen run and pre-declared decision criteria
- **WHEN** the analysis runs
- **THEN** the report states the cost-superiority outcome on the primary cost metric, the accuracy non-inferiority outcome, the criteria applied, and the observed effect sizes

### Requirement: Summary Report

The analysis SHALL produce a report containing arm-level summaries per metric, paired per-instance deltas, an uptake and search-displacement summary, the index amortization break-even, and the excluded-instance list.

Serves: evidence-of-utility, comparable-telemetry

#### Scenario: Report contents

- **GIVEN** a store holding a paired run's trials
- **WHEN** the analysis runs
- **THEN** one report renders arm-level summaries, paired deltas, the uptake and search-displacement summary, the index amortization break-even, and the exclusion list
