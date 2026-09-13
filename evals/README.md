# c10r-evals

`c10r-evals` runs a paired, two-arm SWE-bench-Live localization benchmark.
Both arms receive the same issue and repository.
The baseline arm has the standard file tools; the treatment arm also has c10r, a pre-built index, and an instruction set that tells the agent to query c10r first.
The analysis estimates changes in token use and recovery of the files touched by the historical fix, then evaluates each registered claim separately.

See [docs/experiment-design.md](docs/experiment-design.md) for the registered measures, analyses, and run design.

## Pipeline

```text
generate -> build treatment -> run paired arms -> ingest -> analyze
```

Each stage has a `just` recipe, grouped in the CLI output from `just`.

## Environment

Store local configuration in the gitignored `.env` file beside this README. direnv loads it on `cd`; `just` also loads it when direnv is inactive.
Run `just config` after changing a value used in the arm configs.

`just env-check` reports the active settings without printing the proxy token.
It fails when a required value is missing, a host credential can override the proxy, or the container daemon is unavailable.

| Variable                 | Default               | Meaning                                                                                                                                         |
| ------------------------ | --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `EVAL_PROXY_BASE_URL`    | required              | Inference endpoint reachable from the trial container. It must support the Anthropic Messages API, streaming, tool use, and usage counts.       |
| `EVAL_PROXY_TOKEN`       | required              | Endpoint token. The config stores `${EVAL_PROXY_TOKEN}`; Pier resolves it when the sweep starts.                                                |
| `EVAL_MODEL_NAME`        | required              | Model served by the endpoint. Pier maps every Claude Code model alias to this name.                                                             |
| `EVAL_MAX_TURNS`         | 40                    | Turn limit per episode. Both arms use the same value.                                                                                           |
| `EVAL_MAX_BUDGET_USD`    | 2.0                   | Spend limit per episode. Set it to 0 to remove the limit. A locally served model can report zero cost if Claude Code has no price for its name. |
| `EVAL_CONCURRENCY`       | 3                     | Trials that run at once within an arm.                                                                                                          |
| `EVAL_AGENT_TIMEOUT_SEC` | 1200                  | Wall-clock limit per episode. A timeout records an `AgentTimeoutError`, which analysis scores as an empty-answer agent failure.                 |
| `EVAL_MAX_OUTPUT_TOKENS` | 8000                  | Maximum tokens in one model response. Claude Code's default is 32000.                                                                           |
| `EVAL_PROMPT_CACHING`    | true                  | Allows repeated prompt prefixes to use the endpoint cache. Both arms use the same value. Cache reads still count toward `total_tokens`.         |
| `MLFLOW_TRACKING_URI`    | `sqlite:///mlflow.db` | Result store. Relative SQLite paths resolve from this directory.                                                                                |

Do not set `ANTHROPIC_BASE_URL` on the host; the renderer writes it into each arm config.
Keep `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, and `CLAUDE_CODE_OAUTH_TOKEN` unset when running a sweep.
Pier can copy those host credentials into the container and bypass the proxy.

### Concurrency and timeout

Choose concurrency from the endpoint's batch capacity, then measure episode duration at that concurrency before setting the timeout.
Increasing concurrency can raise total throughput while reducing the token rate of each request.
Requests above the endpoint's batch capacity wait without adding throughput.

On the reference endpoint (`--max-num-seqs` 4), total throughput rose from 10.9 tok/s with one request to 21.3 tok/s with four concurrent requests, while per-request speed fell to 6.1 tok/s.
Eight or sixteen concurrent requests added queueing delay without increasing throughput.

The default concurrency of 3 stays below that batch limit.
The first dev sweep showed that 1200 seconds was too short: it ended 21 of 40 episodes while they were still reading files.
For this reason, `.env.example` sets `EVAL_AGENT_TIMEOUT_SEC` to 3600.

After changing the endpoint or model, inspect the duration of completed episodes.
If more than a few trials per hundred end in `AgentTimeoutError`, raise the timeout or reduce concurrency.

## 1. Generate tasks

```sh
just generate <dataset-revision>
```

This command fetches the SWE-bench-Live Python `verified` split at the specified revision.
It writes `dataset-manifest.json` and a seeded split manifest for 20 dev and 300 frozen instances.
It also writes one Harbor task per instance and arm under `tasks/<subset>/<arm>/`.
The same revision produces the same files.

## 2. Prepare the treatment arm

```sh
just binary
just inject
just images
```

For the frozen subset, name it on the recipes that take one:

```sh
just binary
just inject frozen
just images frozen
```

`binary` builds the static c10r binary.
Rebuild it only when c10r itself changes; one binary serves every subset.
`inject` copies it into each treatment task and adds the c10r install and index-build steps to the Dockerfile.
It does not change the baseline tasks.
`images` builds the treatment images and records the index build time and size.

Every recipe that takes an architecture defaults to `aarch64`, so a local Apple Silicon run passes none.
The binary architecture must match the image platform, and one sweep must use one architecture.
For example, run `just binary x86_64` and `just inject dev x86_64` to prepare an x86-64 dev tree.

Wrap the image build in `caffeinate -ims` so the machine stays awake through it.

## 3. Run the arms

```sh
just env-check
just config
just smoke jazzband__tablib-613
just run
just status
```

For the frozen subset:

```sh
just env-check
just config c10r-dev-refined frozen frozen
just run frozen frozen
just status frozen
```

`config` writes `configs/dev/{baseline,treatment}.yaml` and a `sweep-meta.json` file in each arm directory.
Both configs share the model, limits, timeout, endpoint, and proxy settings; the treatment config adds the c10r instruction set and tool-policy entry.
Run `just config <prompt> <output> <subset>` to render another prompt into another config directory.
Arguments are positional, so reaching a later one means naming the ones before it.

`smoke` runs one instance without adding it to an imported sweep.
`run` starts both arms and skips instances that already have the requested number of scorable attempts.
Completed episodes and agent failures count as scorable; infrastructure failures run again.
Its arguments are `<subset> <config> <attempts>`, so `just run dev dev 3` requests three scorable attempts per instance.
`status` reports the remaining work.

Wrap a full sweep in `caffeinate -ims` so the machine stays awake through it.

Pier writes a new job directory for each attempt.
The importer combines those directories, so rerunning after an interruption preserves earlier results.

## 4. Ingest results

```sh
just ingest
```

Name the config directory for any sweep other than `dev`, as in `just ingest frozen`.

The importer writes one MLflow run per trial.
Each run records its terminal state, token totals, reward scores, c10r and search counts, settings, and raw artifacts.
Repeated imports skip existing trials.
If an existing trial has the same identity but different artifacts, the import stops without changing that record.

The importer accesses the SQLite store directly.
Run `just mlflow` to browse it at <http://127.0.0.1:5000>, then run `just mlflow-stop` when finished.

## 5. Analyze

```sh
just analyze criteria.json
```

Its arguments are `<criteria> <prompt> <output>`, as in `just analyze criteria.json c10r-dev-refined frozen-report.md`.

`criteria.json` records the reference boundaries, confidence level, primary token-use metric, and failure policy.
Write it before the frozen run starts.

Analysis writes `report.md`, `report.trajectories.json`, and the four registered plots.
The trajectory file contains the measures behind each estimate.
The optional `--price-schedule` argument adds financial cost under a named, dated price table.

## Instruction sets

Instruction sets are Markdown files under `prompts/`.
The filename stem is recorded as `instruction_set_version` in sweep metadata, the store, and the report.
`c10r-dev-refined.md` is the current version.

Test new versions on the dev subset, then select one version before the frozen run.
Keep superseded files under `prompts/archive/` so recorded trials still resolve to the prompt that produced them.

## Container network access

Generated tasks use Pier's default network policy, which allows internet access.
Pier needs network access to install Claude Code when a trial starts.

## Replay

The dataset manifest records the dataset revision.
The split manifest records the seed and subsets.
Sweep metadata records the arm, model, instruction set, platform, and episode limits.
Each imported trial records an artifact digest, so repeated imports do not create duplicate records.

## Development

```sh
just sync
just test
just lint
```
