"""Per-arm Pier job configurations rendered from one shared base.

Each arm gets a complete Pier job config (`pier run -c <arm>.yaml`) plus the
`sweep-meta.json` the telemetry importer reads.
The two arms differ only in the c10r instruction prompt, the tool-policy entry
admitting c10r, and the arm's task tree; every other field — model, caps,
endpoint routing, tool policy, concurrency — comes from the shared base.
Each arm's effective tool policy is emitted alongside the configs so arm parity
is checkable against recorded lists, not only against caps.

Secret handling: agent `env` values may be `${VAR}` templates; Pier resolves
them from the host environment at run time, so tokens never sit in the file.

Shell handling: Pier interpolates each string-valued agent kwarg into the shell
command that launches the agent, unquoted. Every such value is therefore emitted
already shell-quoted — a tool name carrying `(`, `)`, or `*`, and an instruction
prompt carrying newlines and backticks, are otherwise parsed by the shell.
"""

import json
import os
import shlex
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field, fields
from pathlib import Path

import yaml

C10R_TOOL_ADMISSION = "Bash(c10r:*)"
DEFAULT_ALLOWED_TOOLS = ["Bash", "Read", "Grep", "Glob", "Write"]
# Pier runs the agent with `--permission-mode=bypassPermissions`, under which the
# allow list does not restrict anything — only this list removes a tool. So every
# exclusion the episode depends on is named here: repository edits (the task is
# read-only), web access (the issues are public, and their fixes are online), and
# the delegation and scheduling tools, whose spend would land outside the trial.
DEFAULT_DISALLOWED_TOOLS = [
    "EnterPlanMode",
    "Edit",
    "NotebookEdit",
    "WebFetch",
    "WebSearch",
    "Task",
    "Workflow",
    "Skill",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
    "TaskOutput",
    "TaskStop",
    "CronCreate",
    "CronDelete",
    "CronList",
    "ScheduleWakeup",
    "SendMessage",
    "ListAgents",
    "EnterWorktree",
    "ExitWorktree",
]

ENV_PROXY_BASE_URL = "EVAL_PROXY_BASE_URL"
ENV_PROXY_TOKEN = "EVAL_PROXY_TOKEN"
ENV_MODEL_NAME = "EVAL_MODEL_NAME"
ENV_MAX_TURNS = "EVAL_MAX_TURNS"
ENV_MAX_BUDGET_USD = "EVAL_MAX_BUDGET_USD"
ENV_CONCURRENCY = "EVAL_CONCURRENCY"
ENV_AGENT_TIMEOUT_SEC = "EVAL_AGENT_TIMEOUT_SEC"
ENV_MAX_OUTPUT_TOKENS = "EVAL_MAX_OUTPUT_TOKENS"
ENV_PROMPT_CACHING = "EVAL_PROMPT_CACHING"

DEFAULT_MAX_TURNS = 40
DEFAULT_MAX_BUDGET_USD = 2.0
# Concurrency and the timeout are set together: a batching endpoint slows each
# request as it serves more at once, and the timeout is wall-clock. Three fits under
# the reference endpoint's batch limit of four. The 1200s timeout is a floor rather
# than a fitted value — a dev sweep at that setting ended half its episodes early —
# so a sweep sets EVAL_AGENT_TIMEOUT_SEC from its own endpoint's measured rate.
DEFAULT_CONCURRENCY = 3
# Two bounds on a runaway episode, both recorded per sweep because both change
# what a trial costs. Without a timeout Pier waits on the agent forever, which is
# how one degenerate episode ran 1h47m; the per-response token ceiling cuts a
# repetition loop off sooner than Claude Code's own 32000-token default.
DEFAULT_AGENT_TIMEOUT_SEC = 1200.0
DEFAULT_MAX_OUTPUT_TOKENS = 8000


class SweepMetadataConflict(Exception):
    """A render would relabel trials that already ran under different settings."""


@dataclass(frozen=True)
class ArmConfigBase:
    """Everything both arms share: model, caps, endpoint routing, and the standard tool policy."""

    model_name: str
    max_turns: int
    max_budget_usd: float
    agent_timeout_sec: float = DEFAULT_AGENT_TIMEOUT_SEC
    max_output_tokens: int = DEFAULT_MAX_OUTPUT_TOKENS
    # The operator's prompt-caching intent for the run. Recorded per sweep regardless of
    # whether it changes the rendered environment — see `_job_config` for how (and when)
    # `DISABLE_PROMPT_CACHING` is actually emitted.
    prompt_caching: bool = True
    env: dict[str, str] = field(default_factory=dict)
    allowed_tools: list[str] = field(default_factory=lambda: list(DEFAULT_ALLOWED_TOOLS))
    disallowed_tools: list[str] = field(default_factory=lambda: list(DEFAULT_DISALLOWED_TOOLS))
    n_concurrent_trials: int = 2

    def recorded_settings(self) -> dict[str, object]:
        """Return every scalar trial setting for the sweep record.

        `model_name` is emitted as `model`; `env`, `allowed_tools`, and `disallowed_tools`
        are omitted. Raises `ValueError` when `ANTHROPIC_BASE_URL` is missing.
        """
        settings: dict[str, object] = {}
        for f in fields(self):
            if f.name in ("env", "allowed_tools", "disallowed_tools", "model_name"):
                continue
            value = getattr(self, f.name)
            if isinstance(value, str | int | float | bool):
                settings[f.name] = value
        endpoint = self.env.get("ANTHROPIC_BASE_URL")
        if not endpoint:
            raise ValueError("recorded_settings: required setting 'endpoint' (ANTHROPIC_BASE_URL) is missing")
        settings["endpoint"] = endpoint
        return settings


def recorded_setting_keys() -> frozenset[str]:
    """Return all keys emitted by `recorded_settings()`."""
    excluded = {"env", "allowed_tools", "disallowed_tools", "model_name"}
    return frozenset(f.name for f in fields(ArmConfigBase) if f.name not in excluded) | {"endpoint"}


def _number_from_env(source: Mapping[str, str], key: str, default: float, cast: Callable[[str], float]) -> float:
    raw = source.get(key, "").strip()
    if not raw:
        return default
    try:
        return cast(raw)
    except ValueError as exc:
        raise ValueError(f"{key}={raw!r} is not a valid {cast.__name__}") from exc


def _bool_from_env(source: Mapping[str, str], key: str, default: bool) -> bool:
    raw = source.get(key, "").strip().lower()
    if not raw:
        return default
    if raw in ("1", "true", "yes"):
        return True
    if raw in ("0", "false", "no"):
        return False
    raise ValueError(f"{key}={raw!r} is not a valid boolean (use 1/0, true/false, or yes/no)")


def base_from_env(source: Mapping[str, str] | None = None) -> ArmConfigBase:
    """Build the shared arm base from the environment, as loaded from `.env` by direnv.

    The proxy token is deliberately not read here. The rendered config carries the
    literal `${EVAL_PROXY_TOKEN}` template, which Pier resolves from the host
    environment when the sweep starts, so the secret never lands in a file.
    """
    env = os.environ if source is None else source
    missing = [key for key in (ENV_PROXY_BASE_URL, ENV_MODEL_NAME) if not env.get(key, "").strip()]
    if missing:
        raise ValueError(f"unset required environment variable(s): {', '.join(missing)} — see evals/.env.example")
    return ArmConfigBase(
        model_name=env[ENV_MODEL_NAME].strip(),
        max_turns=int(_number_from_env(env, ENV_MAX_TURNS, DEFAULT_MAX_TURNS, int)),
        max_budget_usd=_number_from_env(env, ENV_MAX_BUDGET_USD, DEFAULT_MAX_BUDGET_USD, float),
        agent_timeout_sec=_number_from_env(env, ENV_AGENT_TIMEOUT_SEC, DEFAULT_AGENT_TIMEOUT_SEC, float),
        max_output_tokens=int(_number_from_env(env, ENV_MAX_OUTPUT_TOKENS, DEFAULT_MAX_OUTPUT_TOKENS, int)),
        prompt_caching=_bool_from_env(env, ENV_PROMPT_CACHING, True),
        env={
            "ANTHROPIC_BASE_URL": env[ENV_PROXY_BASE_URL].strip(),
            "ANTHROPIC_AUTH_TOKEN": f"${{{ENV_PROXY_TOKEN}}}",
        },
        n_concurrent_trials=int(_number_from_env(env, ENV_CONCURRENCY, DEFAULT_CONCURRENCY, int)),
    )


def tool_flag(tools: list[str]) -> str:
    """Render a tool list as the single shell-safe argument Claude Code's tool flags take."""
    return shlex.quote(",".join(tools))


def _job_config(base: ArmConfigBase, arm: str, tasks_root: Path, jobs_dir: Path) -> dict:
    kwargs: dict[str, object] = {
        "allowed_tools": tool_flag(base.allowed_tools),
        "disallowed_tools": tool_flag(base.disallowed_tools),
        "max_turns": base.max_turns,
    }
    # A zero cap is omitted rather than passed: it means no spend cap, and the turn
    # cap governs. Sending `--max-budget-usd 0` would leave the agent's stop
    # condition up to how the CLI compares a zero budget.
    if base.max_budget_usd > 0:
        kwargs["max_budget_usd"] = base.max_budget_usd
    # The ceiling is derived from the field rather than carried only in `env`, so it
    # cannot disagree with what the sweep metadata records.
    agent_env = {**base.env, "CLAUDE_CODE_MAX_OUTPUT_TOKENS": str(base.max_output_tokens)}
    if not base.prompt_caching:
        # Pier's claude-code agent only checks `DISABLE_PROMPT_CACHING` inside a branch
        # gated on Bedrock detection, which this proxy-routed setup (`ANTHROPIC_BASE_URL`)
        # never enters. `agent.env` still reaches the CLI's subprocess environment
        # unconditionally, through Pier's own env merge, and any non-empty string there
        # is truthy to whatever downstream code reads it — a literal "0" included. So this
        # is set only to disable caching; the enabled default omits the key entirely
        # rather than risk a falsy-looking value being read as true.
        agent_env["DISABLE_PROMPT_CACHING"] = "1"
    return {
        "jobs_dir": str(jobs_dir),
        "n_concurrent_trials": base.n_concurrent_trials,
        "environment": {"type": "docker"},
        "agents": [
            {
                "name": "claude-code",
                "model_name": base.model_name,
                # Pier waits on the agent forever when this is unset, so a degenerate
                # episode has no bound; exceeding it raises AgentTimeoutError, which
                # the importer records as the agent's own failure.
                "override_timeout_sec": base.agent_timeout_sec,
                "env": agent_env,
                "kwargs": kwargs,
            }
        ],
        "datasets": [{"path": str(tasks_root / arm)}],
    }


def _preflight_sweep_metadata(
    base: ArmConfigBase,
    arm_configs: tuple[tuple[str, dict, str], ...],
    out_dir: Path,
    dataset_revision: str,
    platform: str,
) -> dict[str, dict]:
    # Resolved before anything is written: `recorded_settings()` can refuse (a required
    # setting such as the endpoint is missing), and the conflict check below can too. Both
    # must leave the directory exactly as it was — job configs included — rather than
    # write a config that disagrees with an unwritten (or stale) record.
    planned = {
        arm: {
            "arm": arm,
            "instruction_set_version": version,
            "dataset_revision": dataset_revision,
            "model": config["agents"][0]["model_name"],
            "agent": "claude-code",
            "platform": platform,
            **base.recorded_settings(),
        }
        for arm, config, version in arm_configs
    }

    # The importer reads this file when the trials are loaded, not when they ran, so
    # rewriting it over a directory that already holds trials relabels them with settings
    # they did not run under. Only a value that was recorded before and has since changed
    # is a conflict: a key the record simply did not carry yet is a new setting, not a
    # different one. Every arm is checked before any is written, so a refusal leaves the
    # whole render untouched rather than half applied.
    for arm, meta in planned.items():
        meta_path = out_dir / arm / "sweep-meta.json"
        if not (meta_path.is_file() and (out_dir / arm / "jobs").exists()):
            continue
        recorded = json.loads(meta_path.read_text())
        changed = sorted(k for k, v in recorded.items() if k in meta and meta[k] != v)
        if changed:
            raise SweepMetadataConflict(
                f"{out_dir / arm} already holds trials recorded under different settings "
                f"({', '.join(changed)}); render into a new directory instead of overwriting it"
            )
    return planned


def render_arm_configs(
    base: ArmConfigBase,
    prompt_path: Path,
    out_dir: Path,
    *,
    tasks_root: Path,
    dataset_revision: str,
    platform: str,
) -> dict[str, Path]:
    """Write per-arm Pier job configs, sweep metadata, tool-policy record, and treatment metadata.

    Layout under `out_dir`: `<arm>.yaml` (run with `pier run -c`), `<arm>/sweep-meta.json`
    (the importer's sweep root is `out_dir/<arm>`, where Pier writes `jobs/`),
    `tool-policy.json`, and `treatment-meta.json`.
    """
    instruction_set_version = prompt_path.stem
    prompt_text = prompt_path.read_text()

    allowed = {"baseline": list(base.allowed_tools), "treatment": [*base.allowed_tools, C10R_TOOL_ADMISSION]}

    baseline = _job_config(base, "baseline", tasks_root, out_dir / "baseline" / "jobs")
    treatment = _job_config(base, "treatment", tasks_root, out_dir / "treatment" / "jobs")
    treatment_agent = treatment["agents"][0]
    treatment_agent["kwargs"]["allowed_tools"] = tool_flag(allowed["treatment"])
    treatment_agent["kwargs"]["append_system_prompt"] = shlex.quote(prompt_text)

    planned = _preflight_sweep_metadata(
        base,
        (
            ("baseline", baseline, "none"),
            ("treatment", treatment, instruction_set_version),
        ),
        out_dir,
        dataset_revision,
        platform,
    )

    out_dir.mkdir(parents=True, exist_ok=True)
    paths = {
        "baseline": out_dir / "baseline.yaml",
        "treatment": out_dir / "treatment.yaml",
        "tool_policy": out_dir / "tool-policy.json",
        "treatment_meta": out_dir / "treatment-meta.json",
    }
    paths["baseline"].write_text(yaml.safe_dump(baseline, sort_keys=True))
    paths["treatment"].write_text(yaml.safe_dump(treatment, sort_keys=True))

    for arm, meta in planned.items():
        sweep_dir = out_dir / arm
        sweep_dir.mkdir(parents=True, exist_ok=True)
        (sweep_dir / "sweep-meta.json").write_text(json.dumps(meta, indent=2) + "\n")
        paths[f"{arm}_sweep"] = sweep_dir

    # The record keeps the lists themselves; the configs carry their shell-quoted rendering.
    tool_policy = {
        arm: {"allowed_tools": list(tools), "disallowed_tools": list(base.disallowed_tools)}
        for arm, tools in allowed.items()
    }
    paths["tool_policy"].write_text(json.dumps(tool_policy, indent=2) + "\n")

    paths["treatment_meta"].write_text(
        json.dumps(
            {"instruction_set_version": instruction_set_version, "prompt_file": str(prompt_path)},
            indent=2,
        )
        + "\n"
    )
    return paths
