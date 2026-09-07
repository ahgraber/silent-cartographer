# Proposal: eval-localization

## Intent

The north star defines c10r's single measure of success — an agent trusts c10r enough to stop grepping — but nothing measures it.
All utility claims (precise navigation at lower token cost than grep) currently rest on dogfood anecdotes.
This change builds the evaluation foundation and runs its first evaluation: a paired, two-arm localization benchmark on real GitHub issues testing one precise claim — for a predeclared model and task sample, the c10r treatment (the binary plus its instruction prompt) lowers the primary agent cost metric while paired historical-fix file recovery stays within a pre-registered non-inferiority margin of the same agent with its standard search tools alone.
It also produces two durable assets: a frozen, evidence-tested c10r instruction set, and a run-to-analysis telemetry pipeline that later evaluations reuse unchanged.

## User Stories

### Story: evidence-of-utility

As a c10r maintainer, I want paired head-to-head measurements of a coding agent with c10r versus grep-only on real issue-localization work, so that the claim "c10r finds the right code with fewer tokens at equal accuracy" rests on data instead of intuition.

Ladders to: north-star outcomes 1 (precise locate) and 2 (blast radius), and the north-star success measure ("an agent trusts c10r enough to stop grepping").

### Story: instruction-set

As a c10r maintainer, I want an iterated, versioned, and then frozen instruction set that reliably gets an agent to reach for c10r instead of grep, so that agent-facing guidance ships from evidence and later evaluations test a fixed treatment rather than a moving prompt.

Ladders to: the north-star success measure and the agent-native principle — uptake is the behavioral form of "stop grepping."

### Story: comparable-telemetry

As a c10r maintainer, I want every trial's cost, accuracy, and c10r-uptake recorded as structured experiment data queryable across runs, so that arms, prompt versions, and future evaluations are comparable without re-parsing raw trajectories.

Ladders to: the north-star success measure — the claim is only as good as the measurement's reproducibility.

### Story: reusable-runway

As a c10r maintainer, I want the localization benchmark to run on the same task format, runner, and telemetry pipeline the later fix-rate evaluations will use, so that those evaluations add datasets and arms rather than rebuilding infrastructure.

Ladders to: the north-star success measure — the evidence plan only reaches its citable evaluations if the foundation carries.

## Scope

**In scope:**

- `evals/` — a uv-managed Python project in this repo housing the harness (task generation, grading, analysis, telemetry import).
- Episode dataset: task generation from the SWE-bench-Live Python `verified` split into Harbor-format localization tasks (issue + repo checkout at `base_commit`; no test execution).
- Localization verifier: grade the agent's structured answer against the gold patch's file set; emit accuracy scores as the task reward.
- Arm definitions: baseline (grep-only) and treatment (c10r on PATH + versioned instruction prompt), differing only in c10r availability and instructions; c10r built as a static Linux binary and injected into treatment task images.
- Pier-driven runs: `claude-code` agent, docker environment, pinned model and caps identical across arms.
- Telemetry: MLflow import of every scheduled trial with its terminal state (cost, accuracy, uptake, artifacts), idempotent across re-imports.
- Paired analysis: per-task deltas and arm-level summaries for cost superiority and accuracy non-inferiority.
- Instruction-set protocol: iterate on a dev subset, then freeze and version for the reported run.

**Out of scope:**

- The later fix-rate evaluations (full-fix on DeepSWE; SWE-bench-Live full-fix runs at volume and the SWE-bench-Live→Harbor converter) — future changes, sketched in the appendix.
- The Codex harness (the DeepSWE full-fix evaluation adds the second harness).
- The SlopCodeBench claim-3 satellite.
- MCP integration (CLI-only by decision).
- Local-model routing (supported by Pier configuration if wanted during iteration, but not contracted here).
- Any change to c10r product behavior — the harness consumes the existing CLI; a static-binary build target is build tooling, not a contract change.

## Approach

Mechanism sketch, to be formalized in `design.md`:

- **Substrate.**
  Harbor task format run by Pier (`--env docker`): one task per SWE-bench-Live instance; the environment Dockerfile checks out the repo at `base_commit` onto a shared base image; treatment-arm task images additionally contain the c10r binary and a pre-built index.
- **Episode contract.**
  The instruction asks the agent to identify the minimal set of files (and symbols) that must change to resolve the issue, emit a structured JSON answer, and stop — no patching.
  Identical turn/tool-call caps per arm.
- **Grading.**
  Gold file set parsed from the instance's `patch` field (test changes live separately in `test_patch`); verifier scores file-level precision/recall/F1 plus any-gold-file hit, graded as historical-fix file recovery; symbol entries in answers are recorded but not graded.
  Unparseable agent output scores as an empty answer and is recorded, never crashes the trial; every scheduled trial carries a terminal state, and agent-caused failures score as outcomes.
- **Arms and uptake.**
  Treatment prompt delivered via the agent's system-prompt append; arms share an identical effective tool policy except for c10r admission; c10r invocations counted from the ATIF trajectory as the uptake measure, alongside search-tool invocation counts for displacement; analysis is intent-to-treat.
- **Telemetry.**
  Importer walks Pier's `jobs/` tree; one MLflow run per scheduled trial keyed by a stable trial identity (idempotent re-import, terminal state recorded); params carry task, arm, model, prompt version; metrics carry ATIF token/cost totals, reward components, uptake and search counts; trajectory and reward files attach as artifacts.
- **Analysis.**
  Paired per-task deltas; Wilcoxon signed-rank on the primary cost metric (total tokens); a one-sided paired-difference confidence interval judged against the non-inferiority margin for accuracy; report renders a summary table.
  Dev/frozen split of instances chosen up front so prompt iteration never touches the reported subset.

## Open Questions

- Instance allocation — **resolved (design, 2026-08-15):** 20 dev / 100 frozen, disjoint, seed-recorded; remainder unassigned headroom.
- Model — **resolved (design, 2026-08-15):** local models via proxy for iteration; model identity is a recorded per-run parameter; the frozen run's model is chosen at freeze time (direction: generic API endpoint with a model manifest, e.g. OpenRouter).
- MLflow backend — **resolved (design, 2026-08-15):** configurable via the standard tracking URI; local `mlruns/` default for testing, existing tracking server for real runs.
- Non-inferiority margin for accuracy: margin, interval method, and a power calculation are recorded together before the frozen run starts, sized from dev-run discordance (the earlier 3-point lean is underpowered at 100 pairs — see design PairedStatistics).

## Appendix: future evaluations (out of scope, recorded for continuity)

Full exploration lives in `.specs/eval-benchmark-ideation.md` (benchmark verdicts, Pier source findings, cost model, SlopCodeBench assessment; that document uses "Stage 0/1/2" for what this proposal names descriptively).
Condensed runway:

| Evaluation                          | Substrate                                                                                                                 | Adds                                                                                                                                  | Gate                                                           |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Full-fix (DeepSWE)                  | DeepSWE via Pier (Harbor registry `datacurve/deep-swe`, ~34 Py + ~5 Rust tasks)                                           | Full fix attempts on contamination-free tasks; Codex as second harness; Modal as scale-out env                                        | localization run shows cost signal and a working frozen prompt |
| Full-fix at volume (SWE-bench-Live) | SWE-bench-Live Python `verified` at volume (needs the HF→Harbor converter, ~1 day)                                        | Citable cost-superiority + accuracy non-inferiority numbers at N=200–500; optional old SWE-bench Verified as the memorized-repo probe | DeepSWE full-fix shows signal                                  |
| Rust localization                   | `abundant/swe-gen-rust` (Harbor-native, 1,000 tasks; ships gold `fix.patch` → Rust variant of the localization benchmark) | Rust sample size without conversion work; sample-and-validate task quality first                                                      | opportunistic                                                  |
| Claim-3 satellite                   | SlopCodeBench (Harbor-native, 36 problems)                                                                                | Verbosity/duplication under iterative extension — does structural self-awareness reduce slop; write-heavy index-churn stress test     | after prompt freeze                                            |

Carry-overs designed for in this change: Harbor task format, Pier ATIF telemetry, the MLflow importer schema, the arm/instruction-set versioning, and the paired-analysis tooling all apply to every evaluation above.
