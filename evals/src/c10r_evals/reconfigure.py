"""Set the declared episode bounds across a config directory and the trials already run under it.

Only the rendered arm configs affect a later run: the bounds are literals there, so editing `.env`
alone changes nothing. Nothing reads the recorded trial configs, so rewriting those only makes the
directory report one set of bounds, and loses what those episodes ran under. The log keeps that,
outside the trial directories so the artifact digests stay as the importer recorded them.
"""

import json
from collections import defaultdict
from dataclasses import dataclass, field, fields
from datetime import datetime
from pathlib import Path

import yaml

ARMS = ("baseline", "treatment")
LOG_NAME = "reconfigure-log.json"

# Claude Code reads its output ceiling from the environment, where every value is a string.
MAX_OUTPUT_TOKENS_VAR = "CLAUDE_CODE_MAX_OUTPUT_TOKENS"


class ReconfigureError(Exception):
    """The requested bounds or the named directory are not ones this can rewrite."""


@dataclass(frozen=True)
class Limits:
    """The bounds to set. `None` leaves one as it is; a budget of zero removes the limit."""

    agent_timeout_sec: float | None = None
    max_turns: int | None = None
    max_budget_usd: float | None = None
    max_output_tokens: int | None = None
    concurrency: int | None = None

    def __post_init__(self) -> None:
        if all(getattr(self, f.name) is None for f in fields(self)):
            raise ReconfigureError("name at least one bound to set")
        for name in ("agent_timeout_sec", "max_turns", "max_output_tokens", "concurrency"):
            value = getattr(self, name)
            if value is not None and value <= 0:
                raise ReconfigureError(f"{name} must be positive, got {value}")
        if self.max_budget_usd is not None and self.max_budget_usd < 0:
            raise ReconfigureError(f"max_budget_usd must not be negative, got {self.max_budget_usd}")


@dataclass
class Rewrite:
    """How many files a rewrite touched, and the bounds they held before it."""

    arm_configs: int = 0
    sweep_metas: int = 0
    trial_configs: int = 0
    previous: dict[str, set] = field(default_factory=lambda: defaultdict(set))

    @property
    def files(self) -> int:
        return self.arm_configs + self.sweep_metas + self.trial_configs


def _apply(mapping: dict, key: str, value: object, seen: set) -> bool:
    """Set `key` when the mapping already declares it, recording what it held."""
    if key not in mapping:
        return False
    if mapping[key] is not None:
        seen.add(mapping[key])
    if mapping[key] == value:
        return False
    mapping[key] = value
    return True


def _agent_bounds(agent: dict, limits: Limits, rewrite: Rewrite) -> bool:
    """Set the bounds carried by one agent entry, which the rendered and recorded configs share."""
    previous = rewrite.previous
    changed = False
    if limits.agent_timeout_sec is not None:
        changed |= _apply(agent, "override_timeout_sec", limits.agent_timeout_sec, previous["agent_timeout_sec"])
    kwargs = agent.get("kwargs")
    if isinstance(kwargs, dict):
        if limits.max_turns is not None:
            changed |= _apply(kwargs, "max_turns", limits.max_turns, previous["max_turns"])
        if limits.max_budget_usd is not None:
            changed |= _budget(kwargs, limits.max_budget_usd, previous["max_budget_usd"])
    env = agent.get("env")
    if limits.max_output_tokens is not None and isinstance(env, dict):
        changed |= _output_tokens(env, limits.max_output_tokens, previous["max_output_tokens"])
    return changed


def _output_tokens(env: dict, tokens: int, seen: set) -> bool:
    """Write the ceiling as the string the environment carries, but record the bound as a number."""
    held = env.get(MAX_OUTPUT_TOKENS_VAR)
    if held is None:
        return False
    seen.add(int(held))
    if held == str(tokens):
        return False
    env[MAX_OUTPUT_TOKENS_VAR] = str(tokens)
    return True


def _budget(kwargs: dict, budget: float, seen: set) -> bool:
    """Carry a budget only when there is one, which is how the renderer expresses no limit."""
    held = kwargs.get("max_budget_usd")
    if held is not None:
        seen.add(held)
    if budget <= 0:
        return kwargs.pop("max_budget_usd", None) is not None
    if held == budget:
        return False
    kwargs["max_budget_usd"] = budget
    return True


def _arm_config(path: Path, limits: Limits, rewrite: Rewrite, dry_run: bool) -> None:
    config = yaml.safe_load(path.read_text())
    changed = False
    for agent in config.get("agents") or []:
        changed |= _agent_bounds(agent, limits, rewrite)
    if limits.concurrency is not None:
        changed |= _apply(config, "n_concurrent_trials", limits.concurrency, rewrite.previous["concurrency"])
    if changed:
        rewrite.arm_configs += 1
        if not dry_run:
            path.write_text(yaml.safe_dump(config, sort_keys=True))


# Sweep metadata names concurrency as Pier does; the other bounds keep their `Limits` names.
SWEEP_META_KEYS = {"concurrency": "n_concurrent_trials"}


def _sweep_meta(path: Path, limits: Limits, rewrite: Rewrite, dry_run: bool) -> None:
    """Sweep metadata records every bound as a flat scalar, including the ones the agent nests."""
    meta = json.loads(path.read_text())
    changed = False
    for f in fields(limits):
        value = getattr(limits, f.name)
        if value is not None:
            changed |= _apply(meta, SWEEP_META_KEYS.get(f.name, f.name), value, rewrite.previous[f.name])
    if changed:
        rewrite.sweep_metas += 1
        if not dry_run:
            path.write_text(json.dumps(meta, indent=2) + "\n")


def _trial_config(path: Path, limits: Limits, rewrite: Rewrite, dry_run: bool) -> None:
    """Concurrency is a job-level setting, so a trial records only the agent's own bounds."""
    config = json.loads(path.read_text())
    # The verifier declares its own `override_timeout_sec`; only the agent's is ours to set.
    agent = config.get("agent")
    if not isinstance(agent, dict):
        return
    if _agent_bounds(agent, limits, rewrite):
        rewrite.trial_configs += 1
        if not dry_run:
            path.write_text(json.dumps(config, indent=2) + "\n")


def reconfigure(config_dir: Path, limits: Limits, dry_run: bool = False) -> Rewrite:
    """Set `limits` across `config_dir`, its sweep metadata, and every trial recorded under it."""
    if not config_dir.is_dir():
        raise ReconfigureError(f"{config_dir} does not exist; render the arm configs first")

    rewrite = Rewrite()
    for arm in ARMS:
        arm_yaml = config_dir / f"{arm}.yaml"
        if arm_yaml.is_file():
            _arm_config(arm_yaml, limits, rewrite, dry_run)
        meta = config_dir / arm / "sweep-meta.json"
        if meta.is_file():
            _sweep_meta(meta, limits, rewrite, dry_run)
        for trial_config in sorted((config_dir / arm).glob("jobs/*/*/config.json")):
            _trial_config(trial_config, limits, rewrite, dry_run)

    if not rewrite.arm_configs:
        raise ReconfigureError(
            f"{config_dir} holds no arm config these bounds would change; "
            "name the directory that holds baseline.yaml and treatment.yaml"
        )
    if not dry_run:
        _append_log(config_dir, limits, rewrite)
    return rewrite


def _append_log(config_dir: Path, limits: Limits, rewrite: Rewrite) -> None:
    path = config_dir / LOG_NAME
    history = json.loads(path.read_text()) if path.is_file() else []
    history.append(
        {
            "at": datetime.now().astimezone().isoformat(timespec="seconds"),
            "set": {f.name: getattr(limits, f.name) for f in fields(limits) if getattr(limits, f.name) is not None},
            "previous": {name: sorted(held) for name, held in sorted(rewrite.previous.items()) if held},
            "arm_configs": rewrite.arm_configs,
            "sweep_metas": rewrite.sweep_metas,
            "trial_configs": rewrite.trial_configs,
        }
    )
    path.write_text(json.dumps(history, indent=2) + "\n")
