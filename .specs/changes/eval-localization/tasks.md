# Tasks: eval-localization

## Spec alignment sweep

- [x] Delegate to a Sonnet subagent: sweep `evals/` and the change's documents for code the revised `evals/docs/experiment-design.md` and the delta specs now contradict, beyond the items already listed below.
  Give it the design document, the four delta specs, and this list as its inputs.
  It reports findings only — file, line, the spec clause the code or prose contradicts, and the smallest change that would resolve it — and edits nothing.
  Fold its findings into this list before starting the implementation groups. (Swept 2026-09-11.
  Findings folded into the Paired Analysis group below as the criteria-file, unregistered-machinery, and README tasks.
  The sweep found the specs, `design.md`, `proposal.md`, `split.py`, `run.py`, `gold.py`, `verifier_grade.py`, `armconfig.py`, `taskgen.py`, `dataset.py`, `telemetry.py`, and `resume.py` consistent with the revised design.)

## Evals Project Foundation

- [x] Scaffold `evals/` as a uv-managed Python project: `pyproject.toml` with pinned deps (pier, mlflow, scipy, datasets, pytest), `uv.lock`, `src/` package layout, `tests/` tree. (Pier's distribution name is `datacurve-pier`, pinned ==0.3.1; PyPI `pier` is an unrelated package.)
- [x] Add a CLI entry point with `generate`, `import`, and `analyze` subcommands; set a descriptive process title via `setproctitle` and configure structured logging at startup.
- [x] Add a build script that produces a static `x86_64-unknown-linux-musl` c10r binary; captured output: the binary answers `c10r --version` inside a bare Linux container with networking disabled. (Script is arch-parameterized; `aarch64` variant built and offline-verified 2026-08-15 — see notes.md; the `x86_64` artifact is produced with the same command at frozen-run provisioning.)

## Episode Generation (eval-episodes)

- [x] Implement the dataset loader: fetch the SWE-bench-Live Python `verified` split at a pinned revision; write the revision into a generated dataset manifest.
- [x] Implement the seeded split sampler: 20 dev / frozen disjoint instances by index from the sorted ids, so the frozen size is raised by extension and never redrawn; write a split manifest recording instance ids, seed, and dataset revision.
- [x] Test `test_split_manifest_seeded_disjoint`: the same seed reproduces the same disjoint split and the manifest records seed and revision.
- [x] Raise `FROZEN_SIZE` to the registered 300 and size the CLI test's dataset stand-in to the real 500 instances so the fixture stops tracking the frozen size.
- [x] Regenerate the split manifest at the raised size; captured output: the existing 20 dev ids and every previously drawn frozen id are unchanged, and the frozen list holds 300 distinct ids (the manifest on disk currently holds 200 listed / 199 distinct).
  (2026-09-11: dev ids unchanged, 0 previously drawn frozen ids dropped, frozen now 300 listed / 300 distinct with the earlier `conan-io__conan-18153` repeat resolved, dev and frozen disjoint — see notes.md.)
- [x] Implement the task generator: instance → Harbor task directory (instruction from issue text, environment Dockerfile checking out the repo at `base_commit`, task metadata, verifier hook), exactly one task per (instance, arm).
- [x] Test `test_instance_fidelity`: fixture instances yield environments pinned to `base_commit`, instructions containing the issue text, task-recorded instance and arm identity, and N tasks per arm.
- [x] Test `test_episode_contract`: the generated instruction states the JSON answer format, asks for the files that must change, and forbids repository modification; no task definition step executes the repository's tests.
- [x] Test `test_deterministic_generation`: two generation runs over the same fixture revision produce equivalent task trees.
- [x] Captured-output smoke for Runner-Compatible Task Format: run Pier end-to-end on one generated task with the `nop` agent; store the command and result under the change's evidence notes. (2026-08-25: 1 trial, 0 exceptions, empty-answer reward parsed by Pier — see notes.md, including the task.toml/verifier contract corrections made against the real parser.)
- [x] Implement pre-run task characteristics: count tracked `.py` paths at the base commit and decide the source-file cue from the issue text (exact tracked path, or a `.py` basename occurring exactly once among tracked sources); write both into the task metadata of each arm's task.
  The tracked-path list comes from a cached treeless clone (`git clone --filter=tree:0 --no-checkout`, then `git ls-tree -r --name-only <base_commit>`) in the new `repotree.py`, which holds the only network access in generation; `taskgen.py` stays a pure function of the instance and its resolved paths, so generation tests remain hermetic.
  The clones are a build-time intermediate rather than an artifact: generation writes the characteristics it derives from them into each task, nothing reads them afterwards because a treatment image clones its own repository, and replay comes from the pinned dataset revision and base commit.
  `clone_cache` therefore puts them in a temporary directory discarded when generation ends, with `C10R_EVALS_REPO_TREE_CACHE` keeping them for anyone iterating on the characteristics; one cache serves a whole run, so a subset clones each repository once (81 distinct repositories across the 300 frozen instances, about 370 MB).
  `task.toml` records `source_file_count` (int) and `source_file_cue` (bool); both parse as TOML scalars and are identical across an instance's two arms, which the telemetry carry-through task below reads.
- [x] Tests `test_characteristics_recorded_identically_per_arm`, `test_cue_exact_path`, `test_cue_unique_basename`, `test_cue_ambiguous_basename`, `test_cue_absent`.
  Review added two more after finding defects in the first implementation: `test_cue_basename_matches_within_a_partial_path` (the basename rule must fire on `tablib/core.py` for tracked `src/tablib/core.py`, since issues commonly cite partial paths, while the `mycore.py` decoy still misses) and `test_missing_tracked_paths_entry_raises` (an instance with no resolved paths aborts generation naming the instance, rather than recording a fabricated count of zero that the log-scale task-level analysis would silently consume).

## Grading (eval-episodes)

- [x] Implement gold-answer derivation: parse the instance `patch` (unified diff → changed file paths), exclude test-only changes via the separate `test_patch`, embed the gold set in the task's verifier data.
- [x] Tests `test_gold_single_file`, `test_gold_multi_file`, `test_gold_excludes_tests`.
- [x] Implement the verifier: extract the JSON answer (strict schema, salvaging a JSON block embedded in prose), normalize paths repo-root-relative, compute file-level precision/recall/F1 and any-gold-file hit, write the reward; missing/malformed answers score as the empty set with an `unparsed` flag and never raise.
- [x] Tests `test_grading_wellformed`, `test_grading_malformed_empty_flagged`, `test_grading_missing_empty_flagged`: all three paths produce a recorded reward and a zero verifier exit.
- [x] Tests `test_score_exact_match`, `test_score_partial_overlap`, `test_score_disjoint`.
- [x] Test `test_symbols_do_not_change_reward`: identical file sets with and without symbol entries receive identical rewards.
- [x] Test `test_path_normalization_equivalence`: `./`-prefixed and unnormalized answer paths grade identically to normalized ones.

## Arm Construction (eval-arms)

- [x] Implement the treatment-tree transformation: emit the baseline tree pristine and a treatment tree whose environment Dockerfiles append the c10r COPY, PATH setup, and index-build step.
- [x] Test `test_arm_instructions_identical`: per instance, the task instruction is byte-identical across the two arm trees.
- [x] Test `test_baseline_tree_pure`: the baseline tree contains no c10r binary reference and no index-build step.
- [x] Test `test_treatment_tree_injected`: every treatment Dockerfile carries the c10r install and index-build lines.
- [x] Captured-output integration for Treatment Provisioning: build one treatment task image and run a c10r query inside the container with networking disabled; store the output as evidence. (2026-08-25, tablib image, `c10r status` + `find` offline — see notes.md; 17/20 dev images built, 3 sphinx blocked on the non-UTF-8 c10r finding.)
- [x] Record index build wall-clock time and index size per treatment image in the build manifest for separate reporting.

## Run Configuration (eval-arms)

- [x] Author per-arm Pier agent YAML rendering from one shared base: shared model, caps, and proxy env; treatment adds `append_system_prompt` (versioned prompt file) and the `allowed_tools` entry admitting `c10r *`; emit each arm's effective tool-policy lists alongside the configs.
- [x] Test `test_arm_config_parity`: rendered arm configs are identical except the c10r prompt and the tool-policy admission; model and every cap are equal.
- [x] Test `test_tool_policy_recorded_delta`: the emitted allowed/disallowed lists differ only by the c10r admission entry.
- [x] Test `test_baseline_context_no_c10r`: the rendered baseline config and its appended context contain no c10r mention.
- [x] Refuse a re-render over a directory that already holds trials whose recorded settings changed, leaving the directory untouched; tests `test_rerender_over_existing_trials_refuses`, `test_a_refused_render_writes_nothing`, `test_every_scalar_setting_is_recorded`.
- [x] Test `test_prompt_version_in_config`: the treatment config names exactly one instruction-set version and the referenced prompt file exists.
- [x] Implement the sweep runner: divide the stable issue order into blocks of 10, run both arms within each block, and reverse the leading arm between blocks; refuse a run whose shell carries host model credentials or whose config targets another subset.
- [x] Tests `test_the_arms_alternate_in_blocks_rather_than_running_one_then_the_other`, `test_the_leading_arm_inverts_between_blocks`, `test_a_block_runs_only_its_own_instances`.

## Telemetry Import (eval-telemetry)

- [x] Build synthetic Pier `jobs/` fixtures covering: completed graded, unparsed, baseline, agent-failure (timeout and budget exhaustion), and infra-failure trials.

- [x] Implement the importer: walk `jobs/`, derive the trial identity key (dataset revision, instance, arm, instruction version, attempt), assign the terminal state, count c10r and search-tool invocations from the ATIF tool-call records, and log one MLflow run per trial (params, metrics, artifacts, artifact digest) through `MLFLOW_TRACKING_URI`.

- [x] Tests `test_capture_completed`, `test_capture_unparsed`, `test_capture_baseline_zero_uptake`, `test_capture_agent_failure`, `test_capture_infra_failure`: one record each with the fields the spec requires.

- [x] Tests `test_import_novel_once`, `test_reimport_no_duplicate`, `test_interrupted_import_converges`: partial import re-run equals the single-pass store state.

- [x] Test `test_changed_artifacts_rejected`: mutating a source artifact after import makes re-import fail explicitly and leaves the record unchanged.

- [x] Tests `test_uptake_count`, `test_uptake_zero`, `test_mixed_counts`: one c10r invocation plus twenty greps records uptake one and search count twenty.

- [x] Test `test_query_by_arm_and_version`: selection by arm and instruction version returns exactly the matching trials without reading trajectory files.

- [x] Carry the pre-run task characteristics and the trial's platform through import into the trial record, and read them back from the store; tests `test_task_characteristics_recorded`, `test_task_characteristics_read_without_trajectory`.
  Platform was already carried from `armconfig.py` through the sweep metadata; the characteristics are read from the task's `task.toml` and logged with the trial, so `query_trials` returns them without opening a task tree or trajectory.
  A task tree that resolves but carries no characteristics aborts the import before the record is created, because generation now refuses to write a task without them, so their absence means a stale or damaged tree rather than a legitimate gap.

- [x] Record every token category the trajectory reports plus the model, provider, harness, and caching configuration, with an unreported category absent rather than zero; tests `test_token_categories_recorded`, `test_unreported_category_absent`.
  Categories come from the trajectory's first-class fields plus the `extra` mapping, each guarded by a membership check, so a reported zero is recorded and an unreported category is absent.
  `test_reported_zero_token_category_recorded_as_zero` pins the other half of that rule; both tests were confirmed to fail against an implementation that defaults an unreported category to zero.
  Model, provider (the configured endpoint), and harness (the agent) are recorded; caching configuration is not, which the tasks below resolve.

- [x] Make prompt caching an explicit, recorded, and checked run setting, because the frozen run is only affordable with caching working.
  Caching does not affect the primary token-use estimate: Pier counts a cache read as a full prompt token (`prompt_tokens = input_tokens + cached_tokens + creation`), so `total_tokens` reads the same either way.
  It affects what the run costs to execute, which is an operator constraint rather than a measurement one.
  Cache reporting in the store is temporal rather than per-arm, which is consistent with a reporting defect in the configuration or the inference endpoint rather than caching being off:

  | Day        | Endpoint recorded | Trials | Reporting cache |
  | ---------- | ----------------- | -----: | --------------: |
  | 2026-09-07 | no                |    105 |              0% |
  | 2026-09-08 | yes               |     40 |              0% |
  | 2026-09-09 | yes               |     59 |              0% |
  | 2026-09-10 | yes               |     60 |            100% |

  Both arms flip together on each day, so no arm is advantaged either way.
  On 2026-09-10, the only day that reports cache use, cache reads were a median 11.7% of baseline prompt tokens and 13.7% of treatment prompt tokens over trajectories of 18 to 21 turns.
  That share is low for a trajectory whose prefix changes little between turns, so cache effectiveness on the endpoint is worth a separate look even though reporting now works.
  Two parts:

  - [x] Render the caching setting explicitly in both arms' configs so it is recorded, carried through import with the other settings, and compared by the arm-parity check.
    `ArmConfigBase.prompt_caching` defaults to enabled, is settable through `EVAL_PROMPT_CACHING`, and is recorded whether or not it changes the rendered environment.
    Enabling caching omits `DISABLE_PROMPT_CACHING` from the rendered environment rather than setting it to `"0"`: Pier's own `== "1"` comparison sits inside its Bedrock branch and never runs for a proxy-routed endpoint, while `agent.env` reaches the agent process regardless, so a `"0"` would arrive at a JavaScript CLI where a non-empty string is truthy and could be read as a request to disable caching.
    Absence cannot be misread, so `"1"` appears only when the operator asks for caching off.
  - [x] Report cache engagement per trial so a frozen run that silently loses caching is visible early rather than after the fact.
    A trajectory of several turns with near-zero cache reads is the signal.
    Reported per arm under the report's supporting measures, as the median cached share of prompt tokens and the count of trials at or above 5 turns whose cached share is under 1%; both constants are fixed and stated in the report line.
    A trial whose trajectory never reported the cache category is counted separately and contributes to neither figure, so an unreported category cannot be mistaken for caching that ran and returned nothing.
    Blast radius worth weighing before starting: adding a rendered setting makes existing rendered configs differ from newly rendered ones, which the Recorded Settings Immutability refusal treats as a changed setting, so any run directory already holding trials will refuse a re-render until the operator decides to discard or migrate it.

- [x] Record the same metadata keys for every trial.
  `ArmConfigBase.recorded_settings()` ended with `settings["endpoint"] = self.env.get("ANTHROPIC_BASE_URL")`, so an unset endpoint was recorded as `None` and the key then disappeared from the trial record: the 105 trials of 2026-09-07 carry no endpoint parameter while every later trial does.
  A required setting recorded as nothing leaves a trial without the provenance the design requires, and a query cannot tell that two trials are described by different key sets.
  Rendering now refuses when a required setting is missing, naming it, and import refuses a sweep whose metadata lacks a declared key, rather than recording the remaining keys and moving on.
  `recorded_setting_keys()` derives the declared set the same way `recorded_settings()` builds the values, so the two cannot drift apart.
  Both refusal paths resolve the settings before writing anything, so a refused render leaves the output directory absent rather than leaving job configs that disagree with the recorded settings beside them.
  Consequence accepted rather than shimmed: a sweep directory whose metadata predates these settings now fails re-import naming the missing key, until it is re-rendered.

## Paired Analysis (eval-analysis)

- [x] Implement paired-delta assembly: join trials per instance across arms; score agent failures per the declared failure policy; route unresolved infra failures to the exclusion list.
- [x] Tests `test_paired_delta_both_complete`, `test_agent_failure_scored_as_outcome`, `test_infra_failure_excluded_and_enumerated`.
- [x] Implement the criteria file (primary cost metric, reference boundaries, confidence level, failure policy) as a required, versioned analysis input.
- [x] Implement the Tango score confidence interval for the paired difference in proportions; test `test_tango_ci_reference_values` against published worked examples.
- [x] Test `test_intent_to_treat_includes_zero_uptake`.
- [x] Rewrite the criteria file against the registered boundaries: accuracy 0.025, token use 0.10, N 300, five accuracy claims, four token-use claims, and the expected precision at N in place of the per-axis power figures.
- [x] Delete the `power_if_arms_equal` block from `evals/criteria.json`; the rewrite above added `expected_precision_at_planned_pairs` beside it instead of in place of it, so the criteria file still carries per-axis power figures the estimation-first design dropped.
- [x] Rename the criteria keys to the design's vocabulary (`accuracy_margin` → accuracy reference boundary, `cost_margin` → token-use reference boundary) through `Criteria`, the report renderer, and the tests; do it with the claim work below so the vocabulary changes once.
- [x] Implement the two primary estimates with two-sided intervals: the paired hit-rate difference from the Tango score interval, and the mean paired log token ratio from the percentile bootstrap; tests `test_accuracy_estimate_two_sided_interval`, `test_token_ratio_estimate_two_sided_interval`.
- [x] Implement the registered claims per axis — superior, non-inferior, harm, material harm, and accuracy equivalence by two one-sided tests at α = 0.05 — each evaluated independently; tests `test_claim_outcome_directions`, `test_equivalence_requires_interval_inside_boundaries`, `test_overlapping_claims_both_reported`.
- [x] Test `test_no_combined_verdict`: no reported field states one pass, fail, or adoption outcome across the two axes.
- [x] Implement the paired accuracy outcome table (both hit, treatment only, baseline only, neither) and the per-trajectory token-ratio distribution; test `test_paired_outcome_counts_sum_to_pairs`.
- [x] Implement the registered task-level analyses: paired accuracy and token-ratio estimates against source-file count (log scale, no derived threshold), against the source-file cue, and against baseline trajectory length, each with uncertainty and a no-formal-claim label; tests `test_task_level_estimates_present`, `test_task_level_results_labelled`.
  The two continuous covariates are expressed as the slope of the per-pair delta on the covariate with a percentile-bootstrap interval, so no cut point is ever chosen; the source-file cue is a recorded boolean and keeps a genuine two-group split.
- [x] Implement the four registered plots: paired localization accuracy, paired token-ratio distribution, comparative token summaries, and turn count against average turn size; draw reference boundaries on the applicable comparison plots; tests `test_registered_plots_render`, `test_boundaries_drawn_when_interval_crosses`.
  Rendered with plotnine on a forced `Agg` backend so the suite needs no display.
  `test_turn_size_plot_draws_iso_token_curves` asserts every iso-total-token curve spans more than one point: the original fixture pinned `total_steps` to one value for every trial, which collapsed those curves to single points and left a required part of the plot untested.
- [x] Implement the financial-cost derivation under a named price schedule, marking the estimate unavailable when a required token category is missing; tests `test_cost_names_schedule_and_date`, `test_missing_category_marks_unavailable`.
- [x] Emit the per-trajectory data behind every reported estimate; test `test_trajectory_level_output_reproduces_estimates`.
- [x] Implement the report renderer: arm-level summaries per metric, paired deltas, uptake and search-displacement summary, exclusion list, and the historical-fix-recovery and contamination caveats.
- [x] Tests `test_report_contents_complete`, `test_report_headline_not_uptake_conditioned`, `test_dev_instances_excluded_from_report`.
- [x] Remove every unregistered statistical result from the analysis and report, and update the affected tests.
  The registered claim rules in `evals/docs/experiment-design.md` use Tango score bounds for accuracy and bootstrap ratio bounds for token use, so four pieces of machinery have no registered rule behind them: the Wilcoxon cost-superiority test (`stats.py` `cost_superiority`, `report.py` "Secondary cost metrics"), the exact McNemar harm test (`stats.py` `harm_test`, `report.py` headline harm line), the index amortization break-even (`analysis.py` `_break_even`, `report.py` "Index amortization", the `--build-manifest` flag in `cli.py`), and the Spearman-plus-median-split difficulty section (`analysis.py`, `report.py` "Cost against task difficulty"), whose median split is a data-derived threshold and whose registered replacement is the baseline-trajectory-length task-level analysis below.
  Removing break-even also leaves three comments describing a deleted code path: `armtree.py` (the build-manifest docstring), `scripts/build-treatment-images.sh` (the header comment), and the `cli.py` flag help; restate them as the build manifest recording index build cost as a separately reported one-time metric, per design § IndexBakedAtBuild, which keeps that recording task's output meaningful.
- [x] Add failure summaries and explicit exploratory labels to the report; tests `test_failure_summary_present`, `test_exploratory_results_labelled`.
- [x] Order the report so estimates precede claims; test `test_estimates_rendered_before_claims`.

## Quality Audit

- [x] Code cleanliness and quality audit pass over `evals/`, invoked by the user once the groups above are complete.
  Covers dead code left by the estimation-first rewrite, duplicated logic across `analysis.py`, `report.py`, and `stats.py`, module size and cohesion, naming against the design's vocabulary, docstring and comment currency, test shape (one behavior per test, public behavior rather than internals), and the structured-logging and process-title rules.

## Verification Remediation

Findings from the `sdd-verify` run of 2026-09-13.
Every assertion added below was confirmed to fail against a deliberately broken implementation before being kept.

- [x] Waive the two requirements whose contract is behavior inside a built container, since the suite runs without a container runtime: Runner-Compatible Task Format and Treatment Provisioning.
  Recorded in `design.md` § Verification Waivers with the captured commands from `notes.md`, the provenance (added during implementation, after the verify run), and the scope limit on the Treatment Provisioning capture.
  The re-capture against frozen-run images is the unchecked task in the frozen-run group below.
- [x] Cover `_cmd_analyze`, the only CLI handler no test executed: `test_analyze_writes_report_and_trajectory_output`, `test_analyze_report_links_resolve_to_the_written_plots`, `test_analyze_split_manifest_restricts_to_the_frozen_subset`, `test_analyze_instruction_version_keeps_every_baseline_trial`, `test_analyze_exits_2_with_no_paired_instances`.
  This also closed the untested `--instruction-version` filter, which is the only mechanism enforcing that reported treatment trials carry the frozen instruction-set version.
- [x] Assert the recorded fields that import wrote but no test read back: `attempt`, `total_cost_usd`, `total_steps`, `precision`, `recall`, `f1`, and `prompt_caching`.
- [x] Assert that the base-commit checkout survives `inject_c10r`'s Dockerfile rewrite (`test_treatment_tree_retains_instance_fidelity_after_injection`); no test previously covered the composed generate-then-inject Dockerfile a real image build consumes.
- [x] Assert the enumerated elements of all four registered plots against the layer specification rather than against "the file rendered": outcome counts, hit-rate interval, zero line, histogram, rug, box-and-whisker, median, geometric mean with interval, paired connectors, log scales, hit/miss markers, and arm faceting.
- [x] Cover `read_revision` in `scripts/render-arm-config.py`, which had no test at all, across its success path and three failure branches.
- [x] Correct `split.py`'s module comment and `make_split` docstring, which described the draw as "by index" when the implementation is `random.Random(seed).sample`; they now state the prefix guarantee that actually holds.
- [x] Link the four registered plots from the report, per the Summary Report requirement listing them as report content.
  `render_report` takes the plot links and renders a Figures section; `_cmd_analyze` renders the plots before the report so the links resolve and so a plot failure leaves no report claiming figures that were never written.
  Links are relative to the report's directory, which matters because `--plots-dir` can point elsewhere.
- [x] Resolve the mismatch between `design.md` § IndexBakedAtBuild, which promised index build cost "reported separately as a one-time cost", and the implementation, which records the cost in the build manifest and never reads it.
  Resolved by amending the decision rather than adding the report section (user decision, 2026-09-13): the report answers whether the treatment changes per-episode token use and historical-fix file recovery, and a one-time setup cost does not bear on that question.
  The manifest stays a captured artifact so the cost remains available to ad-hoc analysis.
  `eval-arms` gains an Index Build Cost Recording requirement covering the manifest contents and merge behavior, which also gives the `build-manifest` subcommand the requirement it previously lacked.
  The requirement is worded against the manifest rather than against the build script, because the merge is tested and the shell script is not.

## Instruction-Set Iteration and Freeze (operational)

- [x] Draft the instruction set: when to reach for c10r, the core query commands, and answer discipline. (Current version `evals/prompts/c10r-dev-refined.md`; superseded versions stay readable under `evals/prompts/archive/` so trials recorded under their names keep their instruction-set identity.)
- [x] Run dev-subset paired sweeps through the proxy-routed local model; import and review uptake, displacement, and accuracy per prompt version; iterate versions as new files.
  Four versions were iterated and are distinguishable in the store: `c10r-first` (40 trials), `c10r-bounded` (20), `c10r-substitute` (20), and `c10r-dev-refined` (80, including the K = 3 replication); the three superseded ones stay readable under `prompts/archive/`.
  `c10r-dev-refined` is the chosen set.
  Running the rewritten analysis over these trials on 2026-09-13 exercised the pipeline end to end for the first time and found two defects, both fixed with regression tests: the experiment store returns every metric as a float, so a turn count arrives as `16.0` and the iso-curve sampling in plot 4 raised `TypeError`; and an analysis matching no paired instance raised `n must be positive` from inside the interval solver instead of naming the filters that discarded the trials.
- [ ] Freeze the chosen instruction-set version and record it in the run configuration for the frozen subset.
  The set is chosen (`c10r-dev-refined`); what remains is rendering a frozen run configuration, which `configs/` does not yet hold — it carries only `dev`, `dev-bounded`, `dev-recover`, and `dev-substitute`.
  Rendering it now records `prompt_caching` and the endpoint as settings the dev configs predate, so it belongs with the rest of frozen-run provisioning rather than as an edit to a dev directory.
- [x] Pre-registration: record the reference boundaries, confidence level, K, N, and the expected precision (fed by the dev replication) in `evals/docs/experiment-design.md` and `design.md` § PairedStatistics before any frozen-subset trial runs.

## Frozen Run and Report (operational)

- [ ] Re-capture the two waived requirements against the frozen-run images, since the waivers in `design.md` § Verification Waivers rest on a 2026-08-25 capture of one dev image.
  Runner-Compatible Task Format: run Pier end to end on one frozen task.
  Treatment Provisioning: run a c10r query with networking disabled inside a frozen treatment image.
  Record both commands and their output in `notes.md` and update the waiver evidence to name the frozen capture.
- [ ] Execute the frozen paired run (300 issues × 2 arms = 600 episodes) with the frozen prompt and pinned model snapshot, blocked and arm-interleaved, with no early stop; store run configs beside the outputs.
- [ ] Import all frozen trials; captured output: store record count matches the scheduled trial count.
- [ ] Run the analysis and produce the report artifact, attaching the criteria file and split manifest.
- [x] Write `evals/README.md`: pipeline walkthrough (generate → run → import → analyze), configuration knobs (`MLFLOW_TRACKING_URI`, proxy env vars, prompt versioning), and replay instructions.
- [x] Realign `evals/README.md` with the estimation-first design, after the analysis work above lands so the prose describes shipped behavior.
  Beyond the stale passages, the README invoked five recipes the justfile does not define: `experiment`, `sweep`, `sweep-resume`, `resume`, and `arm`.
  The interrupted-run section documented a three-recipe resume workflow that `just run` already provides by skipping instances at their attempt target, so that section collapsed into the run step.
  The inline criteria example and the paragraph describing the report's contents were deleted rather than rewritten, since `criteria.json` and `docs/experiment-design.md` hold both and the README already links the latter.
  Net 24 insertions against 27 deletions.
