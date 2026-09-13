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

### Requirement: Paired Effect Estimates

The analysis SHALL report the paired difference in any-gold-file hit rate with a two-sided 95% Tango score interval.
It SHALL compute `theta = mean(log(treatment total_tokens / baseline total_tokens))` over paired instances and report `exp(theta)` with a two-sided 95% percentile-bootstrap interval over instances.
It SHALL report both estimates whether or not any registered claim is supported.

Serves: evidence-of-utility

#### Scenario: Estimates carry intervals

- **GIVEN** a completed paired run
- **WHEN** the analysis runs
- **THEN** the report states the paired hit-rate difference and the mean paired log token ratio, each with its two-sided confidence interval

#### Scenario: Estimates reported when no claim is supported

- **GIVEN** a completed paired run in which no registered claim is supported
- **WHEN** the analysis runs
- **THEN** the report still states both estimates and their intervals

### Requirement: Registered Claims

The analysis SHALL evaluate the claims registered before the frozen run against the accuracy boundary of `0.025` and the token-use ratio boundary of `1.10`.
Before the frozen run, the criteria input SHALL declare `total_tokens` as the primary token-use metric, the interval methods and confidence levels, both reference boundaries, and the failure-state scoring policy.
For accuracy, superiority requires a one-sided 95% lower score bound above `0`; non-inferiority requires that bound above `-0.025`; equivalence requires the two-sided 90% score interval inside `[-0.025, +0.025]`; harm requires a one-sided 95% upper score bound below `0`; and material harm requires that bound below `-0.025`.
For token use, superiority requires a one-sided 95% upper bootstrap ratio bound below `1.00`; non-inferiority requires that bound below `1.10`; harm requires a one-sided 95% lower bootstrap ratio bound above `1.00`; and material harm requires that bound above `1.10`.
Token-use equivalence SHALL NOT be registered.
The analysis SHALL evaluate each applicable claim independently and SHALL NOT combine the accuracy and token-use results into one outcome.

Serves: evidence-of-utility

#### Scenario: Every registered claim reported

- **GIVEN** a completed frozen run and pre-declared criteria
- **WHEN** the analysis runs
- **THEN** the report states the outcome of every claim registered for each axis, the criteria applied, and the observed effect estimates

#### Scenario: Overlapping claims both reported

- **GIVEN** paired results whose accuracy upper bound is below zero and whose lower bound is above the negated accuracy boundary
- **WHEN** the analysis runs
- **THEN** the report states both the harm claim and the non-inferiority claim as supported

#### Scenario: No combined verdict

- **GIVEN** a completed analysis
- **WHEN** the report is inspected
- **THEN** no reported field states a single pass, fail, or adoption outcome across the two axes

### Requirement: Registered Task-Level Analyses

The analysis SHALL report the paired accuracy and token-ratio estimates against each recorded pre-run task characteristic and against baseline trajectory length, each with its uncertainty.
Repository source-file count SHALL remain continuous, appear on a log scale, and use no threshold derived from the results.
The analysis SHALL label all task-level results as descriptive and as supporting no formal claim.

Serves: evidence-of-utility

#### Scenario: Task characteristics analyzed

- **GIVEN** a completed paired run whose trials carry the recorded pre-run task characteristics
- **WHEN** the analysis runs
- **THEN** the report states the paired accuracy and token-ratio estimates against each characteristic, with uncertainty

#### Scenario: Task-level findings carry no claim

- **GIVEN** a completed analysis containing task-level results
- **WHEN** the report is inspected
- **THEN** each task-level result is labelled as supporting no formal claim

### Requirement: Summary Report

The analysis SHALL produce a report containing arm-level summaries per metric, paired effect estimates, the paired accuracy outcome table, the distribution of per-trajectory token ratios with the median paired ratio and the ratio of total tokens beside the mean paired log ratio, paired per-instance deltas, uptake and search-displacement summaries, failure and exclusion summaries, and the registered plots.
The report SHALL state numeric results before formal claims and interpretation.
It SHALL label exploratory findings and SHALL NOT convert a formal claim that lacks support into a negative product decision.

Serves: evidence-of-utility, comparable-telemetry

#### Scenario: Report contents

- **GIVEN** a store holding a paired run's trials
- **WHEN** the analysis runs
- **THEN** one report renders arm-level summaries, paired effect estimates, the paired accuracy outcome table, the token-ratio distribution with all three token summaries, paired deltas, uptake and search-displacement summaries, and failure and exclusion summaries

#### Scenario: Boundaries drawn when the interval crosses them

- **GIVEN** a paired run whose confidence interval crosses a reference boundary
- **WHEN** the registered plots render
- **THEN** each applicable comparison plot still draws its reference boundary

#### Scenario: Results precede interpretation

- **GIVEN** a completed analysis
- **WHEN** the report is inspected
- **THEN** numeric results appear before formal claims and interpretation, and exploratory findings carry an explicit label

### Requirement: Registered Plots

The analysis SHALL render these four plots:

1. Paired localization accuracy SHALL show the four paired outcome counts beside the paired hit-rate difference and its two-sided 95% Tango score interval, with a line at `0` and a shaded region from `-0.025` to `+0.025`.
2. Paired token-ratio distribution SHALL show an equal-width histogram on a log scale, an observation rug, a box-and-whisker summary, the geometric mean and its two-sided 95% percentile-bootstrap interval, the median, and reference lines at `1.00` and `1.10`.
3. Comparative token summaries SHALL show arm-level box-and-whisker plots for `total_tokens` and average tokens per turn on log scales, with paired observations connected.
4. Turn count and average turn size SHALL use separate arm panels to plot average tokens per turn against turns used, with distinct hit and miss markers and curves for equal `total_tokens` values.

Average tokens per turn SHALL equal `total_tokens / total_steps`.

Serves: evidence-of-utility

#### Scenario: All registered plots render

- **GIVEN** a completed paired analysis
- **WHEN** the report renders its plots
- **THEN** it emits all four registered plots with the required observations, summaries, intervals, and applicable reference boundaries

### Requirement: Financial Cost Under A Named Price Schedule

Where the recorded token categories are sufficient, the report MAY derive a per-trajectory financial cost, and SHALL name the price schedule and pricing date it applied; where a required token category is unavailable, the report SHALL mark the financial estimate unavailable and SHALL NOT impute it.

Serves: evidence-of-utility

#### Scenario: Cost derived and attributed

- **GIVEN** trials whose records carry every token category a named price schedule requires
- **WHEN** the report renders
- **THEN** it states the per-trajectory financial cost together with the price schedule and its pricing date

#### Scenario: Missing category marks the estimate unavailable

- **GIVEN** trials whose records lack a token category a named price schedule requires
- **WHEN** the report renders
- **THEN** it marks the financial estimate unavailable and reports no imputed value

### Requirement: Reproducible Trajectory-Level Output

The analysis SHALL emit the per-trajectory data the report is computed from, so that a reader can reproduce the reported estimates and apply different reference boundaries without rerunning the experiment.

Serves: evidence-of-utility, comparable-telemetry

#### Scenario: Per-trajectory data emitted

- **GIVEN** a completed analysis
- **WHEN** its outputs are inspected
- **THEN** they include the per-trajectory measures and per-instance pairings behind every reported estimate
