# Design: eval-localization

> `evals/docs/experiment-design.md` is the source of truth for the experiment's measures, analyses, reference boundaries, and run size.
> This file records implementation decisions.
> On conflict, `evals/docs/experiment-design.md` is correct.

## Context

- `evals/` is a new uv-managed Python project inside this repo, separate from the Rust workspace.
  It consumes the shipped c10r CLI as an opaque binary; nothing in this change alters c10r product behavior.
- Repo rules that bind the harness: dependencies pinned via `pyproject.toml` + `uv.lock`; every Python process sets a descriptive title via `setproctitle`; secrets (API keys, proxy tokens, tracking-server credentials) live in a gitignored `.env` and reach the code through environment variables; raw inputs and derived artifacts must permit replay; structured logging.
- External substrate facts this design relies on (verified 2026-08-05):
  - SWE-bench-Live's Python `verified` split is a frozen set of 500 instances; each instance carries the issue text, `base_commit`, a gold `patch` (code fix), and a separate `test_patch`.
  - Pier runs Harbor-format task directories with `--env docker`, drives the `claude-code` agent, and exposes per-agent YAML config: `append_system_prompt`, `allowed_tools`, `max_turns`, `max_budget_usd`, and `env` (`ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN`, with any configured base URL auto-added to the egress allowlist).
  - Pier writes per-trial outputs under `jobs/<job>/<trial>/`: `result.json`, `agent/` (ATIF v1.7 trajectory with token/cost totals), `verifier/reward.json`.
  - Pier's agent `install_spec()` is hardcoded per agent class; injecting extra install steps agent-side requires a fork.
- Contamination caveat, accepted: models can localize memorized repos from memory, which shrinks the edge a navigation tool can show.
  Pairing controls per-instance difficulty; it does not cancel memorization, which can interact with the treatment by leaving less localization work to improve.
  The effect attenuates the measurable delta, so observed effects read as conservative; the report carries the caveat.

## Decisions

### Decision: SubstrateHarborPier

**Chosen:** author localization episodes as Harbor-format task directories and run them with Pier (`--env docker`, `claude-code` agent).

**Rationale:** the later fix-rate evaluations already require Harbor + Pier; running the localization benchmark on the same substrate means one runner, one trajectory format (ATIF), and one telemetry pipeline across every evaluation — the reusable-runway story.

**Alternatives considered:**

- Bespoke laptop harness (git clones + headless agent + ad-hoc logging): less setup today, but 100% throwaway — every downstream evaluation would rebuild runner and telemetry.

### Decision: DatasetSweBenchLiveVerified

**Chosen:** SWE-bench-Live Python `verified` split, pinned to a recorded dataset revision.

**Rationale:** 500 frozen real-issue instances; gold code fix (`patch`) ships separately from test changes (`test_patch`), which gives file-level localization ground truth for free; fresher issues than the original SWE-bench reduce memorization.

**Alternatives considered:**

- SWE-bench Verified (original): heavily memorized; useful later only as a "does c10r help even on repos the model knows cold" probe.
- SWE-bench Pro: each task ships a human-written requirements brief that hands the agent much of the localization answer — dilutes exactly the measured signal.
- DeepSWE: contamination-free but small and expensive per task; reserved for the full-fix evaluation.

### Decision: EstimandTreatmentBundle

**Chosen:** the measured estimand is the effect of the c10r treatment bundle, which includes the binary and its instruction prompt, on agent cost and accuracy.
The benchmark estimates that bundle's effect and never attributes an effect to the tool alone.

**Rationale:** the arms differ in two coupled elements (tool availability and instructions), and a CLI tool with no instructions has no uptake path — the bundle is the deployable unit.
Separating the prompt's effect from the tool's would need a third arm this change does not fund.

**Alternatives considered:**

- Three-arm design (baseline / instructions-only / full treatment): isolates the prompt effect at 1.5× the run cost; worth considering only after the two-arm delta proves worth decomposing.

### Decision: InstanceAllocation

**Chosen:** 20 dev instances (instruction-set iteration) and 300 frozen instances (reported run), disjoint, drawn deterministically from the 500 with a recorded seed; the remaining 180 stay unassigned as headroom. (User decision, 2026-08-15; frozen size set 2026-09-11.)

**Rationale:** 20 instances give enough variety to iterate the prompt.
The frozen size balances runtime against the precision of the two primary estimates (see PairedStatistics); it is not sized to guarantee any formal claim.
At 300 issues the run takes about 93 hours under the measured configuration and leaves 180 issues for later work.

The draw is by index from the sorted instance ids, so raising the frozen size extends the frozen set and leaves the dev set and the instances already drawn under a smaller size untouched.
A frozen size is therefore raised, never redrawn, and trials already recorded keep their subset.

**Alternatives considered:**

- 225 frozen: 450 episodes and about 70 hours; moving from 225 to 300 costs 150 episodes and narrows the expected intervals by about 13%.
- 480 frozen: 960 episodes and about 149 hours, intervals about 21% narrower than 300, and no headroom left for a second run.

### Decision: ArmConstructionDatasetSide

**Chosen:** the generator emits two task-directory trees per subset: a pristine baseline tree, and a treatment tree whose environment Dockerfiles gain appended install lines (COPY a prebuilt c10r binary into the image, add it to PATH) plus an index build step.
Each instance yields one logical episode, materialized as exactly one task per (instance, arm); the two trees are the two arm variants of the same episode set.

**Rationale:** Pier's per-agent `install_spec()` is hardcoded, so agent-side injection means forking Pier; dataset-side injection needs zero Pier changes, and the arm difference becomes an explicit, diffable property of the task trees — which is also how the Arm Parity contract gets tested.

**Alternatives considered:**

- Pier agent subclass/fork: couples the harness to Pier internals and adds a maintenance branch for one COPY line.

### Decision: IndexBakedAtBuild

**Chosen:** the treatment image builds the c10r index at image-build time; the agent starts each episode with a warm index.
Index build cost (wall-clock, index size) is recorded per treatment image in the build manifest.
The manifest is a captured artifact for ad-hoc analysis, not report content: the report answers one question, whether the treatment changes per-episode token use and historical-fix file recovery, and a one-time setup cost does not bear on it. (User decision, 2026-09-13.)

**Rationale:** models the deployed steady state — a persistent index that many queries amortize; charging index construction to every episode would bill the treatment arm for a cost real usage pays once.
Recording the cost keeps it available to anyone who later asks what adoption costs, without widening the report's question.

**Alternatives considered:**

- Agent indexes at episode start: charges indexing to treatment tokens and wall-clock; rejected for the primary comparison, and the recorded manifest keeps the trade-off measurable outside it.
- Rendering index build cost as a report section: makes the trade-off visible to every reader without them seeking it out, at the price of answering a question the report was not scoped to ask.

### Decision: PromptViaAgentConfig

**Chosen:** the treatment instruction set is delivered through Pier's `claude-code` YAML `append_system_prompt`, with `allowed_tools` admitting `c10r *` Bash invocations.
Instruction sets are versioned files in `evals/` (e.g. `prompts/c10r-first.md`); the version identifier is part of every run's configuration and telemetry.

**Rationale:** Pier already exposes this surface — no fork; a file-per-version makes the freeze a plain git fact.

**Alternatives considered:**

- Writing instructions into each task's `instruction.md`: violates Arm Parity (task instructions must be identical across arms) and entangles prompt version with dataset generation.

### Decision: EffectiveToolPolicy

**Chosen:** the baseline uses the agent's standard shipped toolset, including file reading, glob, grep/search, and shell, with c10r absent everywhere; the treatment arm runs the identical toolset with `c10r *` additionally admitted through `allowed_tools`.
Each arm's effective allowed and disallowed tool lists are recorded as run parameters, and arm parity is checked against those lists, not only against caps.

**Rationale:** without an exact effective-tool-policy comparison, arm parity would rest on caps alone; the comparison target is the agent as it ships, since that is what a real user would run without c10r.

**Alternatives considered:**

- Stripping the baseline below the standard toolset (e.g., removing Glob): makes the baseline an artificial handicap rather than the real alternative to c10r.

### Decision: ModelRoutingProxyFirst

**Chosen:** runs route through an API-compatible proxy endpoint configured via Pier's `env` (`ANTHROPIC_BASE_URL` + auth token, auto-allowlisted): local models behind the proxy now; the direction of travel is a generic API endpoint with a model manifest against OpenRouter.
The harness treats model identity as a pinned per-run parameter recorded in telemetry — it never assumes a vendor.
The frozen run's model is chosen at freeze time and recorded through the same parameter. (User decision, 2026-08-15.)

**Rationale:** local models make iteration free and allow replication; keeping model identity a parameter means switching to Sonnet-or-other for the citable run is a config change, not a harness change.

**Alternatives considered:**

- Sonnet on API billing from the start: burns budget during prompt iteration where signal quality doesn't need it.
- Claude subscription auth inside Pier containers: Pier assumes token env keys; subscription auth in containerized fleets is impractical and terms-gray — if a subscription route materializes it lands as runner config, not harness design.

### Decision: GradingMechanism

**Chosen:** the graded quantity is **historical-fix file recovery** — how well the answer recovers the files the instance's historical fix changed — and every claim is worded that way, never as localization correctness.
The gold file set is parsed from the instance's `patch` field (unified diff → changed file paths); `test_patch` separation keeps test-only changes out.
The agent's answer is a JSON document (`files: [...]`, optional `symbols` per file) extracted from the trial output; paths are compared repo-root-relative after normalization.
The verifier writes file-level precision/recall/F1 and any-gold-file hit into the task reward; `symbols` entries are recorded with the trial but never graded — unified-diff hunk headers are heuristic function context, not reliable symbol ground truth.
A missing, malformed, or unparseable answer scores as the empty set, is flagged `unparsed`, and never raises.

**Rationale:** the gold patch is free ground truth; empty-answer scoring keeps grading total so weak instruction-following (a real risk with local models) shows up as measured accuracy, not as pipeline crashes.

**Alternatives considered:**

- Grading against a re-derived "minimal" fix set: no defensible oracle exists; the localization literature grades against the gold patch.
- Adjudicating answer-vs-gold disagreements: expensive and subjective; the honest reframe to historical-fix recovery makes the proxy explicit instead.

### Decision: FailureStatePolicy

**Chosen:** every scheduled trial gets a recorded terminal state: `completed`, `agent-failure` (timeout, turn or budget exhaustion, missing or unusable output), or `infra-failure` (provisioning, container, or harness fault independent of agent behavior).
Agent failures are outcomes: accuracy scores as an empty answer and cost counts what was consumed.
Infra failures are not outcomes: the trial reruns under a fixed, recorded bounded-retry policy, the failed attempt stays recorded, and an instance whose trial cannot reach an agent-attributable state is excluded from paired statistics and enumerated in the report.

**Rationale:** completion is itself a post-treatment outcome; analyzing only completed trials silently deletes exactly the failures an arm causes — survivorship bias in the headline comparison.
Under this policy, differential failure moves the estimate instead of vanishing from it.

**Alternatives considered:**

- Pairwise-complete analysis (exclude any instance lacking two completed trials): biased whenever failure rates differ by arm; an exclusion list does not repair the estimate.

### Decision: TelemetryMlflowConfigurable

**Chosen:** MLflow as the experiment store, addressed exclusively through the standard tracking URI (`MLFLOW_TRACKING_URI`): default local `sqlite:///mlflow.db` for testing (MLflow 3 put the `./mlruns` file store into maintenance mode), the user's existing tracking server for real runs. (User decision, 2026-08-15.) The importer walks Pier's `jobs/` tree post-hoc: one MLflow run per trial, tagged with a deterministic trial identity key derived from (dataset revision, instance id, arm, instruction-set version, attempt); import checks for an existing run with that key before writing, which is what makes re-import and interrupted-import convergent.
Params carry instance, arm, agent, model, prompt version, effective tool policy, terminal state, and harness versions; metrics carry ATIF token/cost totals, reward components, uptake (count of `c10r` invocations in the ATIF tool-call records), and search-tool invocation counts (grep/glob/search calls from the same records) so the report can show displacement — c10r use alongside the change in search calls; the raw trajectory and `reward.json` attach as artifacts.
Each record stores a digest of its source artifacts; an import that finds an existing record with the same identity key but a differing artifact digest fails loudly and alters nothing.
A parent run per (arm × subset × instruction version) sweep groups trials.

**Rationale:** post-hoc import needs no Pier changes and can be re-run safely after fixes; the identity-key check is the entire idempotency mechanism; URI-only addressing keeps the importer backend-agnostic.

**Alternatives considered:**

- Parsing ATIF ad hoc at analysis time: re-parses trajectories on every question and leaves no queryable record across runs — the comparable-telemetry story exists to prevent exactly this.

### Decision: PrimaryCostMetric

**Chosen:** the primary cost metric is total tokens per trial (prompt + completion as counted by the trajectory's totals, including the treatment's appended system prompt).
Secondary cost metrics: cost in USD, tool-call count, wall-clock, peak context tokens.
The report presents all of them, but the registered token-use claims ride on the primary alone.
Each token category the model or harness exposes is recorded with the model, provider, harness, and caching configuration.
If the report derives a financial figure, it records the named price schedule and pricing date.
If required token categories are missing, it marks the figure unavailable and does not impute them.

**Rationale:** tokens compare across API and local-model runs where dollars do not; the appended prompt is a real recurring cost of the treatment bundle, so it counts against the treatment; declaring one primary metric before the run prevents selecting whichever metric flatters the result.

**Alternatives considered:**

- USD as primary: meaningless under local models and repriceable after the fact.
- Tool calls as primary: behavioral rather than economic; kept secondary.

### Decision: PairedStatistics

**Chosen:** the experiment reports two paired effect estimates and evaluates the registered formal claims beside them.
It issues no adoption verdict and does not combine the accuracy and token-use results into one outcome.

The accuracy estimate is the paired difference in `any_gold_hit` rates, `lambda = p_treatment - p_baseline`.
The report includes its two-sided 95% Tango score interval and the four paired outcome counts: both hit, treatment only, baseline only, and neither hit.

The primary token-use estimate is the mean paired log total-token ratio, `theta = mean(log(tokens_treatment / tokens_baseline))`.
The report presents `exp(theta)` with a two-sided 95% percentile-bootstrap interval over instances.
It also reports the median paired ratio and the ratio of total tokens.

The registered reference boundaries are 2.5 percentage points for accuracy and 10% for token use.
Applicable comparison plots show the boundaries even when an interval crosses them.
The experiment fixes the boundaries before the frozen run and does not widen them after it observes the results.

Accuracy registers superiority, non-inferiority, equivalence, harm, and material harm.
Token use registers superiority, non-inferiority, harm, and material harm.
Token-use equivalence is not registered because lower token use is beneficial.
The report evaluates each applicable claim independently under the exact rules in `evals/docs/experiment-design.md`.

Instances are treated as independent, with no adjustment for instances from the same repository. (User decision, 2026-09-09.)

**Registered before the frozen run (2026-09-11):** accuracy boundary `0.025`, token-use boundary `0.10`, one-sided confidence level `0.95`, `K = 1` attempt per instance and arm, `N = 300` frozen pairs, and 600 episodes.
The run has no early-stopping rule.

The expected two-sided accuracy interval has a half-width of about 4.9 percentage points at `N = 300`.
The expected two-sided token-ratio interval is about `[0.91, 1.09]` around the estimate.
The planning estimates use variation from three attempts per arm on each of the 20 dev issues.
They depend on the model and configuration and do not enter the frozen-run results.

**Rationale:** the estimates and intervals show the magnitude and uncertainty of each effect even when no formal claim is supported.
The boundaries give readers preregistered reference points while the trajectory-level data let them apply different decision rules.

**Alternatives considered:**

- Both axes as pass/fail non-inferiority tests, with the pair of outcomes as the experiment's verdict: both tests must pass, so the chance of a clear result is the product of the two marginal powers — a few percent at the dev point estimates.
  An underpowered run would then have been recorded as a negative product decision.
- A three-zone rule on the estimate (adopt, reject, inconclusive) with pre-registered probability thresholds: names the inconclusive state instead of hiding it inside a failure, but still converts the estimate into a verdict this experiment does not need to issue.
- Sequential monitoring with futility stopping: caps the cost of an uninformative run, but each interim look needs an error-spending rule to keep the registered claims honest, and the deliverable is the estimate, which only improves with all 300 pairs.
- Harm detection as the decision rule, with the treatment advancing unless harm is demonstrated: inverts the burden so that a small or noisy run endorses the treatment by default.
  Harm remains a registered claim; it is not a gate.
- Unpaired arm means with t-tests: throws away pairing, needs far larger N, and is sensitive to per-instance difficulty spread.
- Cluster-robust intervals over repository: the frozen set draws its instances from a smaller number of repositories, so shared difficulty is plausible, but the effect cannot be measured from the data at hand and the user chose to assume independence.
  The resulting intervals are somewhat narrower than the truth.

### Decision: PreRunTaskCharacteristics

**Chosen:** task generation computes two characteristics from the issue text and the repository at the base commit, and records them with the task metadata before either arm runs: the repository's tracked `.py` file count, and whether the issue text names a source file (an exact tracked `.py` path, or a `.py` basename that occurs exactly once among the repository's tracked Python sources).
The report shows the paired accuracy and token-ratio estimates against each characteristic.
Source-file count stays continuous and is plotted on a log scale, with no data-derived size threshold.
These estimates are descriptive; no formal subgroup claim is registered.

**Rationale:** a reader deciding when to make c10r available can only condition on what is knowable before an agent starts.
Measures produced during a trajectory — baseline step count, token use, search counts — explain a result but cannot select a tool in advance.
Computing the two characteristics at generation time, before either arm runs, keeps them independent of the outcome; choosing a size threshold after seeing the data would let the split be picked to flatter the result.

**Alternatives considered:**

- Deriving the characteristics at analysis time from the stored tasks: the repository checkout is no longer at hand, so the computation would depend on re-fetching state the run does not pin.
- A binary large/small repository split: the threshold has no defensible prior value, and any value chosen after the run is a researcher degree of freedom.

### Decision: BlockedArmInterleaving

**Chosen:** the runner divides the stable issue order into blocks of 10 and runs both arms within each block, reversing which arm leads from one block to the next.

**Rationale:** paired episodes then sit close together in time, so a change in endpoint load affects both arms of a pair similarly, and each arm is spread across the whole run rather than concentrated in one window.
Reversing the lead arm keeps the same instance from running in the same arm-position on consecutive blocks.
Load can still vary within a block, so elapsed-time and failure differences keep a time-varying component; this is recorded as a limitation rather than claimed away.

**Alternatives considered:**

- Running one arm to completion and then the other (the original plan): rejected at proposal time on the grounds that trials share no state giving run order a mechanism.
  Two baseline runs under identical settings later differed by 3 instances and by 2 agent failures, which is consistent with endpoint load affecting outcomes, so arm and time are no longer allowed to be confounded.
- Full randomization of the episode order: breaks the same confound, but gives up the reproducible run order that makes an interrupted sweep resumable.

### Decision: StaticBinaryTarget

**Chosen:** c10r is built as a self-contained static musl binary and copied into treatment images; the architecture is a per-sweep parameter recorded in telemetry, not a constant.
The generated environments are multi-arch (`python:3.11-slim` base, arch-neutral verifier), so dev iteration MAY run native (e.g. `aarch64` on Apple Silicon — no emulation); the frozen run pins one architecture for every sweep it reports (`x86_64`, matching the later evaluations' substrate).

**Rationale:** task images vary in base distro; a static musl binary runs on all of them without runtime dependencies.
Nothing in the episode contract depends on architecture, and pinning it globally would force emulated local iteration for no measurement benefit; what the measurement needs is arch homogeneity _within_ a reported run, which the recorded per-sweep parameter enforces.
This is a build target addition — build tooling, not a product contract change.

**Alternatives considered:**

- Building c10r inside each task image: adds a Rust toolchain to every image build and couples image builds to the repo checkout.

## Architecture

```text
SWE-bench-Live Python verified (HF, pinned revision, seed-recorded 20 dev / 300 frozen split)
        │
        │  evals: task generation
        ▼
tasks/ (Harbor format)
  ├── baseline tree (pristine)          ──►  Pier run, arm A ─┐
  └── treatment tree                    ──►  Pier run, arm B ─┤   claude-code agent, model via proxy,
      (+ c10r musl binary, + baked          (prompt vN via    │   identical caps, --env docker
         index, Dockerfile-appended)         append_system_prompt)
                                                              ▼
                                        jobs/<job>/<trial>/{result.json, agent/ (ATIF), verifier/reward.json}
                                                              │
                                                              │  evals: importer (idempotent, keyed by trial identity)
                                                              ▼
                                         MLflow (MLFLOW_TRACKING_URI: sqlite:///mlflow.db or tracking server)
                                                              │
                                                              │  evals: paired analysis
                                                              ▼
              report: paired estimates + intervals + plots + registered claims + uptake + exclusions
```

## Risks

- **Uptake failure** — the prompt may not get the agent to reach for c10r at all: dev-subset iteration measures uptake explicitly before anything freezes; intent-to-treat keeps the headline honest either way.
- **Local-model instruction-following** — weaker models may emit malformed answers or ignore c10r: grading totality turns malformed answers into measured zeros instead of crashes; the answer parser is strict on schema but salvages a JSON block embedded in prose.
- **Contamination** — memorized repos can shrink the measurable effect and can interact with the treatment; pairing controls shared issue difficulty but does not cancel memorization; the report carries the caveat and treats observed effects as conservative.
- **Gold-file noise** — a valid alternative fix may touch non-gold files: accepted and literature-standard, but not guaranteed symmetric — a treatment that surfaces valid alternatives more often is penalized for them, which is why the report claims historical-fix recovery, not localization correctness.
- **Differential failure rates** — one arm may time out or exhaust budget more often: every scheduled trial records a terminal state and agent failures score as outcomes (FailureStatePolicy), so failures move the estimate instead of vanishing from it.
- **Substrate drift** — Pier/Harbor/MLflow API changes: versions pinned in `uv.lock` and recorded as run params.
- **x86 images on Apple Silicon** — emulation is slow: run fleets on a Linux host; the harness itself is architecture-neutral.
- **Dataset availability** — HF dataset moves or changes: pin the dataset revision; generated task trees are themselves replayable artifacts.

## Verification Waivers

Both entries cover requirements whose contract is about behavior inside a built container.
The test suite runs without a container runtime, so neither can be demonstrated in-suite; the evidence is a captured manual run instead.
Both waivers were added on 2026-09-13, during this change's implementation and after a verify run flagged the gap.
That provenance is recorded here so a later reader does not mistake them for constraints established before the work began.

- **Requirement:** Runner-Compatible Task Format **Reason:** the contract is that the selected runner accepts a generated task without task-specific runner modifications.
  Demonstrating it requires Pier, a Docker daemon, and an image build.
  The suite has none of these, and adding them would make every test run depend on a container runtime.
  **Manual evidence:** `.specs/changes/eval-localization/notes.md`, "Pier end-to-end smoke (2026-08-25)" — command `uv run pier run -p tasks/dev/baseline -i 'jazzband__tablib-613' --agent nop --env docker`; result 1 trial, 0 exceptions, 17 s, with Pier building the environment, running the agent, executing `tests/test.sh` as verifier, and parsing the reward.
  **Recorded:** 2026-09-13

- **Requirement:** Treatment Provisioning **Reason:** the contract is that c10r is executable and answers queries against a pre-built index with no network access.
  Only a built treatment image can show this.
  The in-suite test asserts the injected Dockerfile lines, which is a weaker claim than the requirement makes.
  **Manual evidence:** `.specs/changes/eval-localization/notes.md`, "Treatment provisioning evidence (2026-08-25)" — command `docker run --rm --network none c10r-eval-treatment-jazzband__tablib-613 sh -c 'c10r status && c10r find Dataset'`; `status` reported a fresh index with pinned model identity, and `find Dataset` returned results with a resume cursor.
  **Scope limit:** the capture covered one image out of 17 built from 20 dev instances, and it predates later c10r changes.
  It is evidence that provisioning worked at that revision, not that every frozen-run image will.
  The re-capture at frozen-run provisioning is tracked as an unchecked task in `tasks.md`.
  **Recorded:** 2026-09-13
