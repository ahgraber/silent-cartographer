# Tasks: eval-localization

## Evals Project Foundation

- [x] Scaffold `evals/` as a uv-managed Python project: `pyproject.toml` with pinned deps (pier, mlflow, scipy, datasets, pytest), `uv.lock`, `src/` package layout, `tests/` tree. (Pier's distribution name is `datacurve-pier`, pinned ==0.3.1; PyPI `pier` is an unrelated package.)
- [x] Add a CLI entry point with `generate`, `import`, and `analyze` subcommands; set a descriptive process title via `setproctitle` and configure structured logging at startup.
- [x] Add a build script that produces a static `x86_64-unknown-linux-musl` c10r binary; captured output: the binary answers `c10r --version` inside a bare Linux container with networking disabled. (Script is arch-parameterized; `aarch64` variant built and offline-verified 2026-08-15 — see notes.md; the `x86_64` artifact is produced with the same command at frozen-run provisioning.)

## Episode Generation (eval-episodes)

- [x] Implement the dataset loader: fetch the SWE-bench-Live Python `verified` split at a pinned revision; write the revision into a generated dataset manifest.
- [x] Implement the seeded split sampler: 20 dev / 100 frozen disjoint instances; write a split manifest recording instance ids, seed, and dataset revision.
- [x] Test `test_split_manifest_seeded_disjoint`: the same seed reproduces the same disjoint split and the manifest records seed and revision.
- [x] Implement the task generator: instance → Harbor task directory (instruction from issue text, environment Dockerfile checking out the repo at `base_commit`, task metadata, verifier hook), exactly one task per (instance, arm).
- [x] Test `test_instance_fidelity`: fixture instances yield environments pinned to `base_commit`, instructions containing the issue text, task-recorded instance and arm identity, and N tasks per arm.
- [x] Test `test_episode_contract`: the generated instruction states the JSON answer format, asks for the files that must change, and forbids repository modification; no task definition step executes the repository's tests.
- [x] Test `test_deterministic_generation`: two generation runs over the same fixture revision produce equivalent task trees.
- [x] Captured-output smoke for Runner-Compatible Task Format: run Pier end-to-end on one generated task with the `nop` agent; store the command and result under the change's evidence notes. (2026-08-25: 1 trial, 0 exceptions, empty-answer reward parsed by Pier — see notes.md, including the task.toml/verifier contract corrections made against the real parser.)

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
- [x] Record index build cost (mechanism: `scripts/build-treatment-images.sh` + manifest reader/writer with tests; values recorded when images build) (wall-clock, index size) per treatment image into a build manifest consumed by analysis for the break-even derivation.

## Run Configuration (eval-arms)

- [x] Author per-arm Pier agent YAML rendering from one shared base: shared model, caps, and proxy env; treatment adds `append_system_prompt` (versioned prompt file) and the `allowed_tools` entry admitting `c10r *`; emit each arm's effective tool-policy lists alongside the configs.
- [x] Test `test_arm_config_parity`: rendered arm configs are identical except the c10r prompt and the tool-policy admission; model and every cap are equal.
- [x] Test `test_tool_policy_recorded_delta`: the emitted allowed/disallowed lists differ only by the c10r admission entry.
- [x] Test `test_baseline_context_no_c10r`: the rendered baseline config and its appended context contain no c10r mention.
- [x] Test `test_prompt_version_in_config`: the treatment config names exactly one instruction-set version and the referenced prompt file exists.

## Telemetry Import (eval-telemetry)

- [x] Build synthetic Pier `jobs/` fixtures covering: completed graded, unparsed, baseline, agent-failure (timeout and budget exhaustion), and infra-failure trials.
- [x] Implement the importer: walk `jobs/`, derive the trial identity key (dataset revision, instance, arm, instruction version, attempt), assign the terminal state, count c10r and search-tool invocations from the ATIF tool-call records, and log one MLflow run per trial (params, metrics, artifacts, artifact digest) through `MLFLOW_TRACKING_URI`.
- [x] Tests `test_capture_completed`, `test_capture_unparsed`, `test_capture_baseline_zero_uptake`, `test_capture_agent_failure`, `test_capture_infra_failure`: one record each with the fields the spec requires.
- [x] Tests `test_import_novel_once`, `test_reimport_no_duplicate`, `test_interrupted_import_converges`: partial import re-run equals the single-pass store state.
- [x] Test `test_changed_artifacts_rejected`: mutating a source artifact after import makes re-import fail explicitly and leaves the record unchanged.
- [x] Tests `test_uptake_count`, `test_uptake_zero`, `test_mixed_counts`: one c10r invocation plus twenty greps records uptake one and search count twenty.
- [x] Test `test_query_by_arm_and_version`: selection by arm and instruction version returns exactly the matching trials without reading trajectory files.

## Paired Analysis (eval-analysis)

- [x] Implement paired-delta assembly: join trials per instance across arms; score agent failures per the declared failure policy; route unresolved infra failures to the exclusion list.
- [x] Tests `test_paired_delta_both_complete`, `test_agent_failure_scored_as_outcome`, `test_infra_failure_excluded_and_enumerated`.
- [x] Implement the decision-criteria file (primary cost metric, margin δ, confidence level, failure policy) as a required, versioned analysis input.
- [x] Implement cost superiority: Wilcoxon signed-rank on paired primary-cost deltas with effect-size summary.
- [x] Implement accuracy non-inferiority: Tango score confidence interval for the paired difference in proportions; test `test_tango_ci_reference_values` against published worked examples.
- [x] Test `test_ni_outcome_directions`: synthetic data on either side of −δ yields non-inferior and not-demonstrated outcomes respectively.
- [x] Test `test_intent_to_treat_includes_zero_uptake`.
- [x] Implement the report renderer: arm-level summaries per metric, paired deltas, criteria outcomes with effect sizes, uptake and search-displacement summary, index amortization break-even (build manifest + per-episode deltas), exclusion list, and the historical-fix-recovery and contamination caveats.
- [x] Tests `test_report_contents_complete`, `test_report_headline_not_uptake_conditioned`, `test_dev_instances_excluded_from_report`.

## Instruction-Set Iteration and Freeze (operational)

- [x] Draft the instruction set (`evals/prompts/c10r-first.md`): when to reach for c10r, the core query commands, and answer discipline.
- [ ] Run dev-subset paired sweeps through the proxy-routed local model; import and review uptake, displacement, and accuracy per prompt version; iterate versions as new files.
- [ ] Freeze the chosen instruction-set version and record it in the run configuration for the frozen subset.
- [ ] Pre-registration amendment: record margin δ, confidence level, and the power calculation (fed by dev-run discordance) in `design.md` § PairedStatistics before any frozen-subset trial runs; promote headroom instances into the split manifest first if power is inadequate.

## Frozen Run and Report (operational)

- [ ] Execute the frozen paired run (frozen subset × 2 arms) with the frozen prompt and pinned model snapshot; store run configs beside the outputs.
- [ ] Import all frozen trials; captured output: store record count matches the scheduled trial count.
- [ ] Run the analysis and produce the report artifact, attaching the decision-criteria file and split manifest.
- [x] Write `evals/README.md`: pipeline walkthrough (generate → run → import → analyze), configuration knobs (`MLFLOW_TRACKING_URI`, proxy env vars, prompt versioning), and replay instructions.
