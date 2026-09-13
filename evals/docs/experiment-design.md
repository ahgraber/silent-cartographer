# Experiment design: `c10r` against grep

> **Status.** This document is the source of truth for the experiment's measures, analyses, reference boundaries, and run size.
> Where it disagrees with a change record under `.specs/changes/`, this document is correct because change records are archived after their changes ship.
> A design change belongs here first.
> Any spec or code change that implements that decision must update this document in the same commit.
> Requirement contracts for the experiment software remain in the specs.

## The experiment estimates how `c10r` changes localization

An agent working in a codebase must find the files and functions that require changes.
Agents commonly use grep for that work.
`c10r` indexes a codebase and answers structural questions about it.

The experiment compares an agent with `c10r` against the same agent with its standard search tools.
It estimates how `c10r` changes localization accuracy, token use, and search behavior.
The report gives readers the estimates, uncertainty, distributions, and task-level results they need to decide when `c10r` fits their work.

The experiment does not issue an adoption verdict.
It registers formal superiority, non-inferiority, equivalence, and harm claims before the frozen run, but none of those claims determines whether the experiment succeeded.
An estimate remains useful when the data support none of the formal claims.

## SWE-bench-Live supplies tasks with known target files

The tasks come from SWE-bench-Live, a public benchmark built from GitHub issues that were later fixed.
The experiment pins one dataset revision so the issue set cannot drift.

Each benchmark issue has a known fix and a known set of changed files.
The agent reads the issue and names the files that a fix must change.
The main accuracy score, `any_gold_hit`, records whether the answer includes at least one file from the known fix.
The agent does not write the fix, run the code, or install dependencies.

## The arms differ by `c10r` access and instruction

| Arm       | Tools                                                          | Instructions                     |
| --------- | -------------------------------------------------------------- | -------------------------------- |
| baseline  | The agent's standard file reading, glob, grep, and shell tools | None                             |
| treatment | The baseline tools, the `c10r` program, and a pre-built index  | A versioned prompt in `prompts/` |

Both arms use the same task text, model, serving configuration, turn limit, timeout, and output limit.
Each frozen issue receives one attempt in each arm.
The analysis pairs the two trajectories for that issue.

## Development and frozen issues remain separate

The experiment divides the issues once with a recorded random seed and stores the result in `tasks/split-manifest.json`.

| Split       | Size | Purpose                                                               |
| ----------- | ---: | --------------------------------------------------------------------- |
| dev         |   20 | Improve the instruction and harness and estimate run-to-run variation |
| frozen      |  300 | Produce the reported estimates                                        |
| unallocated |  180 | Reserve issues for later work                                         |

The splits do not overlap.
The experiment never reports dev results as findings from the frozen run.
Using an issue in both the dev and frozen sets would invalidate the frozen estimate.

Before the frozen run, three attempts per arm run on the 20 dev issues.
These attempts estimate run-to-run variation and expose mechanical problems in the instruction or harness.
The model, instruction, dataset revision, run limits, analysis code, and reference boundaries are then frozen.

The frozen run collects all 300 usable issue pairs.
It has no early-stopping rule.
Interim outcomes do not change the issue count.

## The report estimates accuracy and token use

Each episode records the following measures:

| Measure        | Meaning                                                                                   |
| -------------- | ----------------------------------------------------------------------------------------- |
| `any_gold_hit` | Whether the answer names at least one file changed by the known fix                       |
| `total_tokens` | All tokens reported for the model trajectory, including instructions and repeated context |
| `total_steps`  | Agent turns                                                                               |
| `wall_clock_s` | Episode duration                                                                          |
| `uptake_count` | `c10r` invocations                                                                        |
| `search_count` | Grep and glob invocations                                                                 |

### Accuracy uses the paired difference in hit rates

The primary accuracy estimate is:

> `lambda = p_T - p_B`
>
> `p_T` is the treatment hit rate, and `p_B` is the baseline hit rate.

A `lambda` of `-0.025` means that the treatment found a known fix file on 2.5 percentage points _fewer_ issues.
The report includes the paired score confidence interval and the four paired outcomes: both hit, treatment only, baseline only, and neither hit.

### Token use uses the paired trajectory ratio

For issue `i`, let `T_i` be the treatment trajectory's tokens and `B_i` be the baseline trajectory's tokens.
The primary token-use estimate is:

> `theta = mean(ln(T_i / B_i))`
>
> `token_ratio = exp(theta)`

This estimate gives every issue equal weight and limits the influence of a few very large trajectories.
A `token_ratio` of `1.10` means that the geometric mean treatment trajectory used 10% more tokens than its paired baseline trajectory.

The report also includes the median paired ratio and the ratio of total tokens across arms.
The median describes the middle issue.
The total ratio describes the billable volume for the sampled workload, but large trajectories receive more weight.

## Financial cost depends on the model, provider, and harness

`total_tokens` measures token use.
It does not by itself measure financial cost.
Input, cached input, cache writes, output, and reasoning tokens can have different prices.
Harnesses also differ in how they construct prompts and use caches.

The experiment records each token category that the model or harness exposes.
It also records the model, provider, harness, caching configuration, and pricing date used for any financial estimate.
When the available telemetry supports a calculation, the report can derive a per-trajectory financial cost under a named price schedule.
If required token categories are unavailable, the report marks that financial estimate unavailable and does not impute it.

Financial results apply only to the named configuration.
Readers can recalculate them for another price schedule when the recorded token categories are sufficient.

## The report presents estimates before formal claims

The main report contains:

1. Arm-level summaries for every measure.
2. Paired effect estimates with two-sided 95% confidence intervals.
3. The paired accuracy outcome table.
4. The distribution of per-trajectory token ratios and arm-level token summaries.
5. Uptake, search displacement, failure, and exclusion summaries.
6. Registered task-level analyses with their uncertainty.
7. Trajectory-level data needed to reproduce the report and apply other decision rules.

Four plots carry the main comparison:

| Plot                             | Contents                                                                                                                                                                                                 | Reader question                                                                           |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Paired localization accuracy     | The four paired outcome counts beside the paired hit-rate difference, its two-sided 95% score interval, a line at `0`, and a shaded region from `-0.025` to `+0.025`                                     | How large and uncertain is the accuracy change, and which arm won when outcomes differed? |
| Paired token-ratio distribution  | A histogram with equal-width bins on a log scale, an observation rug, a box-and-whisker summary, the geometric mean and its two-sided 95% bootstrap interval, the median, and lines at `1.00` and `1.10` | How large and variable is the token-use change per task?                                  |
| Comparative token summaries      | Arm-level box-and-whisker plots for total tokens and average tokens per turn on log scales, with paired observations connected                                                                           | Did token use change through trajectory length, average turn size, or both?               |
| Turn count and average turn size | Separate arm panels plotting average tokens per turn against turns used, with hit and miss markers and curves for equal total-token levels                                                               | What combinations of trajectory length and turn size produced the totals?                 |

The report states the numeric result before its interpretation.
It labels exploratory findings and does not convert a formal claim that lacks support into a negative product decision.

## Reference boundaries support optional formal claims

The reference boundaries express differences that the experiment owner considers practically small:

| Measure   |              Boundary | Practical interpretation                                 |
| --------- | --------------------: | -------------------------------------------------------- |
| Accuracy  | 2.5 percentage points | Two or three additional misses per 100 issues            |
| Token use |                   10% | A geometric mean trajectory ratio no greater than `1.10` |

The plots show these boundaries even when the confidence intervals cross them.
Readers can apply different boundaries to the published estimates and trajectory-level data.
The registered formal claims continue to use the original boundaries after the frozen results are known.

All formal claims use the paired estimates.
Accuracy superiority, non-inferiority, and harm use one-sided 95% score bounds.
Accuracy equivalence uses two one-sided tests at `alpha = 0.05`, which is equivalent to requiring the two-sided 90% score interval to fit within `[-0.025, +0.025]`.
Token-use claims use bootstrap bounds for the mean paired log ratio.

The report evaluates each claim independently:

| Claim         | Accuracy rule                                 | Token-use rule                                       |
| ------------- | --------------------------------------------- | ---------------------------------------------------- |
| Superior      | The lower bound is above `0`                  | The upper ratio bound is below `1.00`                |
| Non-inferior  | The lower bound is above `-0.025`             | The upper ratio bound is below `1.10`                |
| Equivalent    | The 90% interval is inside `[-0.025, +0.025]` | Not registered because lower token use is beneficial |
| Harm          | The upper bound is below `0`                  | The lower ratio bound is above `1.00`                |
| Material harm | The upper bound is below `-0.025`             | The lower ratio bound is above `1.10`                |

These claims can overlap.
For example, the experiment can establish that `c10r` is less accurate while also establishing that the loss is smaller than 2.5 percentage points.
The report does not combine the accuracy and token-use claims into one verdict.

## The frozen run contains 300 paired issues

`N = 300` frozen issues and one attempt per issue per arm produce 600 episodes.
The size balances runtime against the precision of the main estimates.
It was not selected to guarantee any secondary claim.

The planning estimates use variation measured from the dev runs for one model:

| Estimate                    |                        Dev variation | Expected precision at `N = 300`                                          |
| --------------------------- | -----------------------------------: | ------------------------------------------------------------------------ |
| Accuracy difference         | Standard error `0.0286` at `N = 225` | Two-sided 95% interval half-width of about 4.9 percentage points         |
| Mean paired log token ratio |           Standard deviation `0.794` | Two-sided 95% ratio interval of about `[0.91, 1.09]` around the estimate |

The token interval is multiplicative.
For example, an estimated ratio of `1.08` would have an approximate interval from `1.08 * 0.91` to `1.08 * 1.09`.

The practical boundaries are tighter than the expected accuracy interval.
If the arms are truly equal, the planning approximation gives about 26% power for 2.5-point accuracy non-inferiority and about 67% power for 10% token-use non-inferiority.
The expected accuracy interval is too wide to establish 2.5-point equivalence.
These limits affect the available formal claims, but they do not prevent the run from estimating larger benefits or harms.

At the configuration measured so far, one episode averages about 28 minutes and three episodes run at a time.
Six hundred episodes therefore take about 93 hours.
The run continues through all 300 usable pairs and does not stop when an interim estimate crosses a claim boundary.

> [!IMPORTANT]
> These precision estimates depend on the model, provider, harness, and dev sample.
> The final report uses the observed frozen-run uncertainty.
> A result for another model or configuration requires a separate run and a new precision calculation.

## Task-level analyses help readers choose when to use `c10r`

The overall estimates average across different tasks.
The report also shows where the paired results differ across task characteristics.
These analyses help a reader decide whether to make `c10r` available for a subset of work.

The experiment registers two task characteristics that are available before an agent starts:

| Characteristic               | Definition                                                                                                                                                  | Reader question                                                      |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Repository source-file count | The number of tracked paths ending in `.py` at the pinned base commit                                                                                       | Does the treatment effect change with repository size?               |
| Issue names a source file    | Whether the issue text contains an exact tracked `.py` path or a `.py` basename that occurs exactly once among the repository's tracked Python source files | Does `c10r` help more when the issue does not identify likely files? |

Task generation computes both characteristics from the issue text and repository at the base commit.
It records them with the task metadata before either arm runs.
Repository source-file count remains continuous in the analysis and appears on a log scale without a data-derived size threshold.
The report shows the paired accuracy and token-ratio estimates against source-file count and separately for issues with and without a source-file cue.
These estimates are descriptive and do not support formal subgroup claims.

Measures produced during a trajectory can explain a result, but a user cannot use them to select a tool before the trajectory runs.

The registered analysis plots token use against baseline trajectory length and plots the paired token ratio against baseline trajectory length.
Baseline length describes task difficulty under the current workflow.
Dev results suggest that treatment costs more on short trajectories and less on long trajectories.
On issues where the baseline exceeded 25 steps, treatment used 0.72 to 0.82 times the baseline tokens; on shorter issues, it used 1.62 to 1.74 times the baseline tokens.
The frozen report estimates this relationship with uncertainty and labels it as task-level analysis.
Because a user does not know the baseline trajectory length before choosing a tool, this analysis explains the result but does not define a prospective selection rule.

The report also shows accuracy outcomes against baseline trajectory length.
It avoids formal claims for all task-level analyses because dividing 300 issues reduces precision.

## Supporting measures explain the observed effects

### Search displacement shows whether the agent used the treatment

On the dev set, the baseline used grep and glob 9.6 times per issue on average.
The treatment used them 4.6 times, a 52% reduction.
The frozen report includes `uptake_count` and `search_count` without conditioning the headline estimates on treatment uptake.

### Precision and recall qualify the hit score

`any_gold_hit` gives credit for one known fix file even if the answer includes unrelated files.
It also does not show whether the answer includes every file needed for the known fix.
Precision reports the share of named files that the known fix touched.
Recall reports the share of known fix files that the agent named.

### Steps and elapsed time describe operational behavior

`total_steps` shows how much interaction the trajectory required.
The report derives average tokens per turn as `total_tokens / total_steps`.
Plotting this value against turns used decomposes each trajectory's total token use into its length and average turn size.
`wall_clock_s` records elapsed time, but server load and serving configuration affect it.
The report presents both as descriptive measures.

## Agent failures remain outcomes

| Terminal state | Meaning                                                                                 | Handling                      |
| -------------- | --------------------------------------------------------------------------------------- | ----------------------------- |
| completed      | The agent produced an answer                                                            | Score the episode             |
| agent-failure  | The agent spent tokens and stopped without an answer because of a timeout or turn limit | Score the episode as a miss   |
| infra-failure  | The episode did not run because of a build error, unavailable server, or cancellation   | Exclude and retry the episode |

Dropping an agent failure would favor the arm that fails more often.
An issue remains in the paired analysis when both arms have a completed or agent-failure outcome.
An issue is excluded only when an arm has no usable attempt after the retry policy finishes.
The report lists every exclusion.

## Recorded provenance fixes the scope of each result

Every trial records the issue, arm, instruction version, model, dataset revision, platform, attempt, repository source-file count, and source-file-cue indicator.
The arm configuration supplies the turn limit, timeout, concurrency, output limit, and server address.
The authentication token is never recorded.

The renderer refuses to reuse a recorded run when its configuration conflicts with stored settings.
A rerun therefore cannot relabel trials produced under different settings.

The instruction version is the prompt filename.
Renaming a prompt file would orphan trials recorded under its old name, so superseded prompt files remain available.
Settings take effect when the configuration is rendered.
Changing an environment variable without rendering a new configuration does not alter the recorded run.

## Known limitations narrow the interpretation

### Blocked arm interleaving reduces but does not remove load variation

The runner divides the stable issue order into blocks of 10 and runs both arms within each block.
It reverses which arm leads from one block to the next.
This schedule keeps paired episodes close in time and distributes each arm across the run.
Endpoint load and provider behavior can still change within or between blocks, so elapsed-time and failure differences can contain time-varying noise.

### Treatment tuning can overfit the dev set

The treatment instruction was revised against 20 dev issues.
An instruction tuned for one model and a small issue set might not generalize.
The frozen set measures the result after tuning stops.

### The dev set cannot resolve small accuracy changes

At 20 issues, the uncertainty on the accuracy difference is about 10 percentage points.
The experiment therefore evaluates instruction changes with mechanical measures such as command errors, batching, and step counts.
These measures provide hundreds of events instead of 20 accuracy outcomes.

### Serving configuration is part of the tested treatment

Both arms must use identical model weights and serving parameters.
A server restart during the frozen run invalidates affected pairs unless the configuration is known to match.
Timeouts and other agent-attributable limits remain part of the observed outcome under the frozen configuration.

### Repository clustering can make intervals too narrow

The analysis treats issues as independent.
The frozen set contains multiple issues from some repositories, so issues can share difficulty or structure.
The reported confidence intervals can therefore be narrower than the true uncertainty across repositories.

### Task-level analyses have limited precision

The run estimates overall effects more precisely than effects within task subsets.
Task-level findings support interpretation and later experiments.
They do not support unregistered subgroup claims.

### Each result applies to one model and configuration

Tokenization, caching, tool behavior, and model behavior vary across configurations.
A result applies to the model, provider, harness, instruction, and limits recorded for that run.

## Appendix A: issue count controls precision and runtime

More frozen issues narrow both confidence intervals at the cost of more episodes.
One issue produces two episodes, one per arm.

| Frozen issues | Episodes | Approximate accuracy 95% half-width | Approximate token-ratio 95% factor |
| ------------: | -------: | ----------------------------------: | ---------------------------------: |
|           100 |      200 |               8.4 percentage points |                     `[0.86, 1.17]` |
|           225 |      450 |               5.6 percentage points |                     `[0.90, 1.11]` |
|           300 |      600 |               4.9 percentage points |                     `[0.91, 1.09]` |
|           380 |      760 |               4.3 percentage points |                     `[0.92, 1.08]` |
|           480 |      960 |               3.8 percentage points |                     `[0.93, 1.07]` |

The precision estimates scale the dev variation to each issue count.
They are planning values rather than guarantees.

Moving from 225 to 300 issues adds 150 episodes and about 23 hours under the measured configuration.
It narrows the expected intervals by about 13%.
The 300-issue run leaves 180 issues unallocated for later work.

## Appendix B: more issues provide broader task coverage than repeats

One attempt per issue represents what a user receives from one agent trajectory.
Three attempts estimate the agent's average chance of success on one issue and reveal run-to-run variation.

Repeats reduce run-to-run noise but add no bugs or repositories.
The dev replication measured no stable between-issue differences, so the current planning estimate depends mainly on the total number of attempts.
Three attempts on 100 issues and one attempt on 300 issues both require 600 episodes and have similar estimated precision under that result.

The frozen run uses 300 issues because broader issue coverage is more useful for task-level interpretation.
The dev replication supplies a separate estimate of run-to-run variation.

## Appendix C: reference boundaries express practical similarity

The accuracy boundary of 2.5 percentage points permits two or three additional misses per 100 issues.
The token-use boundary of 10% permits a geometric mean paired trajectory ratio of `1.10`.

These values locate practical reference lines in the report.
They also define the registered secondary non-inferiority and material-harm claims.
The experiment does not widen them after seeing the frozen results.

A reader can choose different boundaries for a different use case.
The published estimates, intervals, plots, and trajectory-level data support that analysis without changing the registered claims.

## Appendix D: each token summary answers a different question

The dev data show why the report retains three token summaries.
The middle issue used 33% more tokens under treatment, while the mean paired log ratio converted to 12% more.
A few issues where treatment used much fewer tokens pulled the geometric mean down.

| Measure               | Question answered                                                 | Weighting                                                                |
| --------------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Mean paired log ratio | How did proportional token use change across paired trajectories? | Every issue has equal weight, with limited influence from extreme ratios |
| Median paired ratio   | What happened on the middle issue?                                | Only the order of issue ratios matters                                   |
| Ratio of total tokens | How did token volume change for this sampled workload?            | Large trajectories receive more weight                                   |

The mean paired log ratio is the primary token-use estimate because the user is interested in change per task trajectory.
The other summaries remain visible because they expose typical and workload-level behavior that the primary estimate can hide.
