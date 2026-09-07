# Design: eval-localization

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

**Chosen:** the measured estimand is the effect of the c10r treatment bundle — the binary plus its instruction prompt — on agent cost and accuracy.
The benchmark supports "the c10r treatment helps / does not help"; it never attributes an effect to the tool alone.

**Rationale:** the arms differ in two coupled elements (tool availability and instructions), and a CLI tool with no instructions has no uptake path — the bundle is the deployable unit.
Separating the prompt's effect from the tool's would need a third arm this change does not fund.

**Alternatives considered:**

- Three-arm design (baseline / instructions-only / full treatment): isolates the prompt effect at 1.5× the run cost; worth considering only after the two-arm delta proves worth decomposing.

### Decision: InstanceAllocation

**Chosen:** 20 dev instances (instruction-set iteration) and 100 frozen instances (reported run), disjoint, drawn deterministically from the 500 with a recorded seed; the remaining 380 stay unassigned as headroom. (User decision, 2026-08-15.)

**Rationale:** 20 instances give enough variety to iterate the prompt; 100 paired instances keep the frozen run in the ~$20–80 band.
Whether 100 pairs power the accuracy margin is settled by the freeze-time power calculation (see PairedStatistics); promoting headroom instances is the escape hatch if they do not.

**Alternatives considered:**

- 30/200 or 20/480: more power at 2–5× the cost; headroom instances can be promoted in a later run without redesign.

### Decision: ArmConstructionDatasetSide

**Chosen:** the generator emits two task-directory trees per subset: a pristine baseline tree, and a treatment tree whose environment Dockerfiles gain appended install lines (COPY a prebuilt c10r binary into the image, add it to PATH) plus an index build step.
Each instance yields one logical episode, materialized as exactly one task per (instance, arm); the two trees are the two arm variants of the same episode set.

**Rationale:** Pier's per-agent `install_spec()` is hardcoded, so agent-side injection means forking Pier; dataset-side injection needs zero Pier changes, and the arm difference becomes an explicit, diffable property of the task trees — which is also how the Arm Parity contract gets tested.

**Alternatives considered:**

- Pier agent subclass/fork: couples the harness to Pier internals and adds a maintenance branch for one COPY line.

### Decision: IndexBakedAtBuild

**Chosen:** the treatment image builds the c10r index at image-build time; the agent starts each episode with a warm index.
Index build cost (wall-clock, index size) is recorded per task and reported separately as a one-time cost.
The report also derives the amortization break-even: the number of episodes at which the treatment's per-episode saving covers the one-time index cost.

**Rationale:** models the deployed steady state — a persistent index that many queries amortize; charging index construction to every episode would bill the treatment arm for a cost real usage pays once.

**Alternatives considered:**

- Agent indexes at episode start: charges indexing to treatment tokens and wall-clock; rejected for the primary comparison, but the separately-reported one-time cost keeps the trade-off visible.

### Decision: PromptViaAgentConfig

**Chosen:** the treatment instruction set is delivered through Pier's `claude-code` YAML `append_system_prompt`, with `allowed_tools` admitting `c10r *` Bash invocations.
Instruction sets are versioned files in `evals/` (e.g. `prompts/c10r-first.md`); the version identifier is part of every run's configuration and telemetry.

**Rationale:** Pier already exposes this surface — no fork; a file-per-version makes the freeze a plain git fact.

**Alternatives considered:**

- Writing instructions into each task's `instruction.md`: violates Arm Parity (task instructions must be identical across arms) and entangles prompt version with dataset generation.

### Decision: EffectiveToolPolicy

**Chosen:** "grep-only" is operationalized as the agent's standard shipped toolset — file reading, glob, grep/search, and shell — with c10r absent everywhere; the treatment arm runs the identical toolset with `c10r *` additionally admitted through `allowed_tools`.
Each arm's effective allowed and disallowed tool lists are recorded as run parameters, and arm parity is checked against those lists, not only against caps.

**Rationale:** without an exact effective-tool-policy comparison, "grep-only" is unverifiable and parity claims rest on caps alone; the comparison target is the agent as it ships, since that is what a real user would run without c10r.

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
The report presents all of them, but the cost-superiority claim rides on the primary alone.

**Rationale:** tokens compare across API and local-model runs where dollars do not; the appended prompt is a real recurring cost of the treatment bundle, so it counts against the treatment; declaring one primary metric before the run prevents selecting whichever metric flatters the result.

**Alternatives considered:**

- USD as primary: meaningless under local models and repriceable after the fact.
- Tool calls as primary: behavioral rather than economic; kept secondary.

### Decision: PairedStatistics

**Chosen:** the accuracy estimand is the paired difference in any-gold-file hit rate (treatment − baseline) over the frozen subset; the cost estimand is the paired difference in the primary cost metric.
Cost superiority: Wilcoxon signed-rank on paired primary-cost deltas.
Accuracy non-inferiority: a one-sided confidence interval for the paired difference in proportions (Tango's score interval); non-inferiority holds if the interval's lower bound stays above −δ.
The margin δ, the confidence level, and a power calculation fed by dev-run discordance rates MUST be recorded in this file, as an amendment, before the frozen run starts.
The proposal's 3-point lean is superseded: at 100 pairs and plausible discordance (~15%), a 3-point margin yields under 20% power, so the recorded margin must be sized jointly with N (promoting headroom instances if needed) to reach acceptable power.
Trials are independent containerized episodes with the model snapshot pinned per run; attempt identity is recorded, and replication (multiple attempts per instance under local models) is the variance lever.

**Rationale:** pairing is the main variance-reduction lever at N=100.
A two-sided paired test answers "do the arms differ?", which cannot conclude "no worse than baseline by δ" — the non-inferiority question needs an interval judged against the margin.
Declaring margin, method, and power together before the data exists is what makes "equal accuracy" falsifiable; declaring them earlier than freeze time adds nothing.

**Alternatives considered:**

- McNemar's test judged against the margin: tests whether paired proportions differ (marginal homogeneity), not whether treatment is within δ of baseline — the wrong hypothesis for the claim.
- Unpaired arm means with t-tests: throws away pairing, needs far larger N, and is sensitive to per-instance difficulty spread.
- Formal arm-order counterbalancing and paired sampling seeds: trials share no state that would give run order a mechanism, and the harness exposes no sampling seeds; pinned snapshots plus recorded attempts cover the real threat (provider drift).

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
SWE-bench-Live Python verified (HF, pinned revision, seed-recorded 20 dev / 100 frozen split)
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
                                              MLflow (MLFLOW_TRACKING_URI: local mlruns/ or tracking server)
                                                              │
                                                              │  evals: paired analysis
                                                              ▼
                                    report: cost superiority + accuracy non-inferiority + uptake + exclusions
```

## Risks

- **Uptake failure** — the prompt may not get the agent to reach for c10r at all: dev-subset iteration measures uptake explicitly before anything freezes; intent-to-treat keeps the headline honest either way.
- **Local-model instruction-following** — weaker models may emit malformed answers or ignore c10r: grading totality turns malformed answers into measured zeros instead of crashes; the answer parser is strict on schema but salvages a JSON block embedded in prose.
- **Contamination** — memorized repos shrink the measurable edge: accepted; pairing cancels it in deltas; the report carries the caveat and treats observed effects as conservative.
- **Gold-file noise** — a valid alternative fix may touch non-gold files: accepted and literature-standard, but not guaranteed symmetric — a treatment that surfaces valid alternatives more often is penalized for them, which is why the report claims historical-fix recovery, not localization correctness.
- **Differential failure rates** — one arm may time out or exhaust budget more often: every scheduled trial records a terminal state and agent failures score as outcomes (FailureStatePolicy), so failures move the estimate instead of vanishing from it.
- **Substrate drift** — Pier/Harbor/MLflow API changes: versions pinned in `uv.lock` and recorded as run params.
- **x86 images on Apple Silicon** — emulation is slow: run fleets on a Linux host; the harness itself is architecture-neutral.
- **Dataset availability** — HF dataset moves or changes: pin the dataset revision; generated task trees are themselves replayable artifacts.
