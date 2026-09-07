# c10r-evals

A paired, two-arm benchmark that measures whether c10r makes an agent cheaper at finding the files behind a bug report.

Both arms get the same SWE-bench-Live issue and the same repository.
The baseline arm has grep and the usual file tools.
The treatment arm has those plus the c10r binary, a pre-built index, and an instruction set telling it to query the index first.
The test is whether the treatment lowers token cost while recovering the same files the historical fix touched, within a margin fixed before the run.

The contract lives in `.specs/` (capabilities `eval-episodes`, `eval-arms`, `eval-telemetry`, `eval-analysis`).
Decisions and their reasons live in the change's `design.md`.

## Pipeline

```text
generate ──► arm trees ──► pier runs ──► ingest ──► analyze
(tasks +      (baseline     (one sweep    (MLflow   (paired report)
 manifests)    plain,        per arm)      store)
               treatment
               +c10r+index)
```

Every step has a `just` recipe.
Run `just` to list them, grouped as project, prepare, run, and results.
Each section below shows the recipe first, then the commands it runs.

`just experiment <criteria>` runs the whole chain: both arms, ingest, report, then the MLflow server.

## Environment

Configuration lives in `.env` beside this file, which is gitignored. direnv loads it on `cd`, and `just` loads it too for shells where direnv is not active.
Every value is baked into the arm configs when they are rendered, so run `just config` after changing `.env`.

`just env-check` prints what is set, shows the token's length rather than the token, and fails if a required variable is missing, if a host credential would override the proxy, or if the container daemon is unreachable.

| Variable                 | Default               | Meaning                                                                                                                                                                                                                                                                                                                              |
| ------------------------ | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `EVAL_PROXY_BASE_URL`    | required              | The inference endpoint, addressed as a trial container sees it. `localhost` there means the container itself, so use a hostname the container can resolve. The endpoint must speak the Anthropic Messages API with streaming and tool use, and must return usage token counts, because the cost metric is read from what it reports. |
| `EVAL_PROXY_TOKEN`       | required              | The endpoint's auth token. Configs store the literal `${EVAL_PROXY_TOKEN}` and Pier resolves it when the sweep starts, so the token is never written to a file.                                                                                                                                                                      |
| `EVAL_MODEL_NAME`        | required              | The model the endpoint serves. Because a base URL is set, Pier points every Claude Code model alias at this one name, so the endpoint receives no other.                                                                                                                                                                             |
| `EVAL_MAX_TURNS`         | 40                    | Turn cap per episode, the same in both arms.                                                                                                                                                                                                                                                                                         |
| `EVAL_MAX_BUDGET_USD`    | 2.0                   | Spend cap per episode. Claude Code computes cost from its own price table for the model name it sees, so a locally served model can report zero and never reach the cap. Set 0 to remove the cap.                                                                                                                                    |
| `EVAL_CONCURRENCY`       | 3                     | Trials running at once within an arm. Choose it together with the timeout (see below).                                                                                                                                                                                                                                               |
| `EVAL_AGENT_TIMEOUT_SEC` | 1200                  | Wall-clock limit for one episode. Without it Pier waits on the agent with no limit. Passing it raises `AgentTimeoutError`, which the importer records as an agent failure and scores as an empty answer.                                                                                                                             |
| `EVAL_MAX_OUTPUT_TOKENS` | 8000                  | Limit on one model response, passed to Claude Code as `CLAUDE_CODE_MAX_OUTPUT_TOKENS`. Claude Code's own default is 32000; a lower limit ends a runaway response sooner.                                                                                                                                                             |
| `MLFLOW_TRACKING_URI`    | `sqlite:///mlflow.db` | Where results are stored. The sqlite path is relative to this directory, which is where every recipe runs.                                                                                                                                                                                                                           |

Do not set `ANTHROPIC_BASE_URL` on the host.
The renderer writes it and `ANTHROPIC_AUTH_TOKEN` into the arm configs.

Keep `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, and `CLAUDE_CODE_OAUTH_TOKEN` unset in the shell that runs a sweep.
Pier reads Claude Code credentials from the host environment when the config does not supply them, so any of them would be copied into the container and used instead of the proxy.
`just env-check` fails when it finds one.

### Concurrency and the agent timeout

Set these two together, because concurrency changes how long a trial takes and the timeout is measured in wall-clock time.

A batching endpoint serves several requests at once up to its own limit, so raising concurrency raises total throughput while slowing each request.
Measure the endpoint before choosing: send N requests at once and record the batch size it reaches, the aggregate token rate, and the per-request token rate.
Set `EVAL_CONCURRENCY` no higher than the batch size the endpoint reaches, because past that point requests wait without adding throughput.

Then set `EVAL_AGENT_TIMEOUT_SEC` from the per-request rate at that concurrency, not from a single-trial measurement.
The timeout exists to stop a runaway episode, so it should almost never end a healthy one.
When it fires often, it is censoring slow trials rather than catching broken ones, and a trial that ran under heavier load is recorded as an empty answer for a reason that has nothing to do with its arm.

Measured on the reference endpoint (`--max-num-seqs` 4): 1 request decodes at 10.9 tok/s, 4 at once decode at 6.1 tok/s each for 21.3 tok/s in total, and 8 or 16 at once add queueing delay with no further throughput.
So concurrency 4 is worth about 2.2x the total throughput of concurrency 1, at the cost of each trial taking roughly 1.8x as long.

Concurrency 3 stays under that batch limit.
The 1200-second default timeout proved too short: in the first dev sweep it ended 21 of 40 episodes, nearly all of which were still opening files they had not read when it fired.
`.env.example` therefore sets `EVAL_AGENT_TIMEOUT_SEC` to 3600.

After any change of endpoint or model, check the timeout against what happened rather than against an estimate.
Take the wall-clock times of the episodes that finished and confirm the timeout sits well above the slowest of them.
If more than a few trials in a hundred end in `AgentTimeoutError`, the timeout is ending healthy episodes and both the timeout and the concurrency need revisiting.

## 1. Generate tasks

```sh
just generate <dataset-revision>
uv run c10r-evals generate --revision <dataset-revision> --seed 0 --subset dev --out tasks/
```

This fetches the SWE-bench-Live Python `verified` split at the pinned revision, writes `dataset-manifest.json` and the seeded 20-dev / 100-frozen `split-manifest.json`, and writes one Harbor task per instance per arm under `tasks/<subset>/<arm>/`.
Generating again from the same revision produces the same files.

## 2. Build the binary and set up the treatment arm

```sh
just binary                     # static musl binary → vendor/c10r/aarch64/c10r
just inject                     # prints how many task trees it refreshed
just images                     # only needed when inject refreshed something
```

```sh
scripts/build-c10r-musl.sh aarch64
uv run python -c "from pathlib import Path; from c10r_evals.armtree import inject_c10r; inject_c10r(Path('tasks/dev/treatment'), Path('vendor/c10r/aarch64/c10r'))"
scripts/build-treatment-images.sh tasks/dev/treatment build-manifest.json linux/arm64
```

`inject` copies the binary into each treatment task and appends the c10r install and `c10r build` steps to its Dockerfile.
The baseline task trees are left alone.
`images` builds each treatment image and records its index cost, in seconds and bytes, into the build manifest that the break-even calculation reads.

Architecture is chosen per sweep.
Build `aarch64` for local work on Apple Silicon, which avoids emulation, or `x86_64` for the frozen run.
The injected binary's architecture must match the image platform, and each sweep records its platform.
The frozen run uses one architecture throughout.

Every recipe that takes an architecture also takes the arguments in the same order: cfg, subset, arch, out.
So `just binary x86_64`, but `just inject dev x86_64`.

## 3. Run the arms

```sh
just env-check                  # variables, host credentials, daemon
just config                     # → configs/dev, from prompts/c10r-first.md
just smoke jazzband__tablib-613 # one trial, to check auth and token accounting
just sweep                      # baseline, then treatment
```

```sh
uv run python scripts/render-arm-config.py --prompt prompts/c10r-first.md --out configs/dev \
    --tasks-root tasks/dev --platform linux/arm64
uv run pier run -c configs/dev/baseline.yaml -p tasks/dev/baseline \
    -i jazzband__tablib-613 -o jobs/smoke
uv run pier run -c configs/dev/baseline.yaml
uv run pier run -c configs/dev/treatment.yaml
```

Both arms come from one shared base, so model, caps, timeouts, and endpoint routing are identical.
The treatment adds two things: the instruction set as an appended system prompt, and the `c10r` entry in the tool policy.
The base reads the environment variables above, and the dataset revision comes from `tasks/dataset-manifest.json`, so a config cannot name a revision the task trees were not built from.

Rendering writes `configs/dev/{baseline,treatment}.yaml` and one directory per arm holding `sweep-meta.json`, which is where Pier writes `jobs/`.
`just config <name> <dir>` renders a different instruction set into its own directory.

A single-instance run needs `-p` as well as `-i`, because Pier filters the dataset named on the command line, not the one named in the config.

## 4. Ingest results

```sh
just ingest                     # both arms of configs/dev
uv run c10r-evals import configs/dev/baseline configs/dev/treatment
```

The importer writes one MLflow run per trial, keyed so that importing twice does not duplicate.
Each run records the terminal state (`completed`, `agent-failure`, or `infra-failure`), token and cost totals, reward scores, c10r and search counts, and the raw artifacts.
Importing again skips trials already stored; if a trial's identity matches a stored one but its artifacts differ, the import stops and changes nothing.

The importer writes the sqlite store directly and needs no server.
`just mlflow` serves the store at <http://127.0.0.1:5000> for reading, and `just mlflow-stop` shuts it down.

## 5. Analyze

```sh
just analyze criteria.json
uv run c10r-evals analyze --criteria criteria.json --split-manifest tasks/split-manifest.json \
    --instruction-version c10r-first --build-manifest build-manifest.json --out report.md
```

The criteria file is required, and must be written before the frozen run starts:

```json
{
  "version": "criteria-v1",
  "primary_cost_metric": "total_tokens",
  "non_inferiority_margin": 0.1,
  "confidence_level": 0.95,
  "failure_policy": {
    "agent_failure": "empty-answer outcome",
    "infra_failure": "exclude after bounded reruns"
  }
}
```

The report covers cost (Wilcoxon signed-rank on the paired differences), accuracy (a Tango score interval on the paired difference in hit rate, judged against the margin), per-arm summaries, per-instance differences, c10r use and search displacement, the point where indexing cost pays for itself, the list of excluded instances, and the standing caveats.

## Instruction sets

Instruction sets are files under `prompts/`.
The file stem becomes the recorded `instruction_set_version`, so it identifies the arm in sweep metadata, in the store, and in the report.
`c10r-first.md` is the current one.

Iterate on the dev subset, then fix one instruction set before the frozen run.

## What the containers can reach

The generated tasks set no network policy, so Pier's default applies and trial containers reach the internet.
They need it: Pier installs Claude Code into the image when the trial starts.

## Replay

Every derived file traces back to a recorded input.
The dataset manifest pins the revision, the split manifest pins the seed and the subsets, task generation is deterministic, sweep metadata pins the arm, model, instruction set, platform, and episode limits, and each imported trial carries a digest of its artifacts.
Regenerating or re-importing produces the same store rather than a second copy.

## Development

```sh
just sync
just test
just lint
```
