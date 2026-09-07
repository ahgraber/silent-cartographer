"""Idempotent import of Pier trial outputs into the MLflow experiment store.

The importer walks a sweep directory (`sweep-meta.json` + Pier's `jobs/` tree),
assigns each scheduled trial a terminal state, and logs exactly one MLflow run
per trial keyed by a deterministic trial identity.
Re-import finds the existing record by identity and skips it; an identity match
with a differing artifact digest fails loudly and alters nothing.
"""

import hashlib
import json
import logging
import re
from dataclasses import dataclass
from pathlib import Path

from mlflow.tracking import MlflowClient

from c10r_evals.runtime import log_fields

logger = logging.getLogger(__name__)

EXPERIMENT_NAME = "c10r-evals"

COMPLETED = "completed"
AGENT_FAILURE = "agent-failure"
INFRA_FAILURE = "infra-failure"

# Pier records a failed trial as an exception type, not a status string. The agent's
# own execution timeout is always the agent's doing.
AGENT_FAILURE_EXCEPTIONS = {"AgentTimeoutError"}

# A non-zero agent exit covers two different events. The agent CLI exits non-zero both
# when it exhausts a limit of its own — the turn cap, or the per-response output
# ceiling a repetition loop reaches — and when the endpoint refuses the request. Only
# the first is the agent's doing, and the two are told apart by whether the episode
# consumed anything: an endpoint that refuses produces no tokens, while an agent that
# talks itself past a ceiling produces many.
AMBIGUOUS_EXIT_EXCEPTIONS = {"NonZeroAgentExitCodeError"}

_C10R_INVOCATION = re.compile(r"(?:^|[;&|()`]\s*)c10r\b")
_SEARCH_INVOCATION = re.compile(r"(?:^|[;&|()`]\s*)(?:grep|egrep|fgrep|rg|fd|find)\b")
_BASH_TOOL_NAMES = {"bash", "shell", "terminal"}
_SEARCH_TOOL_NAMES = {"grep", "glob"}

EMPTY_ANSWER_REWARD = {
    "reward": 0.0,
    "precision": 0.0,
    "recall": 0.0,
    "f1": 0.0,
    "any_gold_hit": 0,
    "unparsed": True,
}


class ImportMismatchError(Exception):
    """A trial's identity matches an existing record but its artifacts differ."""


@dataclass(frozen=True)
class SweepMeta:
    """Run-level context recorded when a sweep is launched."""

    arm: str
    instruction_set_version: str
    dataset_revision: str
    model: str
    agent: str = "claude-code"
    platform: str = "linux/amd64"
    # Episode bounds, defaulted so a sweep recorded before they existed still loads.
    agent_timeout_sec: float | None = None
    max_output_tokens: int | None = None

    @classmethod
    def load(cls, sweep_dir: Path) -> "SweepMeta":
        raw = json.loads((sweep_dir / "sweep-meta.json").read_text())
        return cls(**raw)


def trial_identity(meta: SweepMeta, instance_id: str, attempt: str) -> str:
    """Deterministic identity key for one scheduled trial."""
    sep = "\x1f"
    material = (
        f"{meta.dataset_revision}{sep}{instance_id}{sep}{meta.arm}{sep}{meta.instruction_set_version}{sep}{attempt}"
    )
    return hashlib.sha256(material.encode()).hexdigest()[:24]


def artifact_digest(trial_dir: Path) -> str:
    """Content digest over every file in the trial directory, order-independent."""
    digest = hashlib.sha256()
    for file in sorted(p for p in trial_dir.rglob("*") if p.is_file()):
        digest.update(str(file.relative_to(trial_dir)).encode())
        digest.update(b"\x00")
        digest.update(hashlib.sha256(file.read_bytes()).digest())
    return digest.hexdigest()[:24]


def instance_id_from_result(result: dict, fallback: str) -> str:
    """Read the instance identifier from a Pier trial result.

    Pier names a trial's task `<arm>/<instance-id>` and records the task path as a
    structured id, so the instance is the last segment of either.
    """
    task_name = result.get("task_name")
    if isinstance(task_name, str) and task_name:
        return task_name.rsplit("/", 1)[-1]
    task_id = result.get("task_id")
    if isinstance(task_id, dict) and isinstance(task_id.get("path"), str):
        return task_id["path"].rstrip("/").rsplit("/", 1)[-1]
    return fallback


def episode_consumed_tokens(trial_dir: Path) -> bool:
    """Whether the agent got far enough to spend anything on the model."""
    path = trial_dir / "agent" / "trajectory.json"
    if not path.is_file():
        return False
    final = json.loads(path.read_text()).get("final_metrics") or {}
    return (final.get("total_prompt_tokens", 0) + final.get("total_completion_tokens", 0)) > 0


def classify_terminal_state(trial_dir: Path) -> str:
    """Map a trial's recorded outcome to its terminal state.

    - No exception and a written reward: the episode finished and was graded.
    - The agent's own execution timeout: the agent caused the failure, so it is an outcome.
    - A non-zero agent exit that consumed tokens: the agent exhausted a limit of its
      own, which is also an outcome. The same exit with no tokens spent means nothing
      ran, so it is an infrastructure failure.
    - Anything else, including an ungraded trial: infrastructure failure.

    Classifying a limit the agent reached as infrastructure would drop that episode from
    the analysis instead of scoring it, which favours whichever arm exhausts limits more
    often. An agent that exhausts its turn or spend cap instead exits zero, arrives here
    as `completed`, and is graded on whatever answer it left behind.
    """
    result_path = trial_dir / "result.json"
    if not result_path.is_file():
        return INFRA_FAILURE
    exception_info = json.loads(result_path.read_text()).get("exception_info")
    if exception_info:
        exception_type = exception_info.get("exception_type")
        if exception_type in AGENT_FAILURE_EXCEPTIONS:
            return AGENT_FAILURE
        if exception_type in AMBIGUOUS_EXIT_EXCEPTIONS and episode_consumed_tokens(trial_dir):
            return AGENT_FAILURE
        return INFRA_FAILURE
    if (trial_dir / "verifier" / "reward.json").is_file():
        return COMPLETED
    return INFRA_FAILURE


def count_invocations(trajectory: dict) -> tuple[int, int]:
    """Count c10r invocations (uptake) and search-tool invocations from ATIF tool calls.

    A step carries no `tool_calls` key at all when the agent called no tool.
    """
    uptake = 0
    search = 0
    for step in trajectory.get("steps", []):
        for call in step.get("tool_calls") or []:
            name = str(call.get("function_name", "")).lower()
            if name in _SEARCH_TOOL_NAMES:
                search += 1
            elif name in _BASH_TOOL_NAMES:
                command = str(call.get("arguments", {}).get("command", ""))
                uptake += len(_C10R_INVOCATION.findall(command))
                search += len(_SEARCH_INVOCATION.findall(command))
    return uptake, search


def _parse_timestamp(value: object) -> float | None:
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            from datetime import datetime

            return datetime.fromisoformat(value).timestamp()
        except ValueError:
            return None
    return None


def trajectory_wall_clock(trajectory: dict) -> float | None:
    """Elapsed seconds between the first and last step timestamps, when present."""
    timestamps = [
        ts
        for ts in (_parse_timestamp(step.get("timestamp")) for step in trajectory.get("steps", []))
        if ts is not None
    ]
    if len(timestamps) < 2:
        return None
    return max(timestamps) - min(timestamps)


def _read_trajectory(trial_dir: Path) -> dict | None:
    path = trial_dir / "agent" / "trajectory.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text())


def _read_reward(trial_dir: Path) -> dict | None:
    path = trial_dir / "verifier" / "reward.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text())


def _ensure_experiment(client: MlflowClient) -> str:
    experiment = client.get_experiment_by_name(EXPERIMENT_NAME)
    if experiment is not None:
        return experiment.experiment_id
    return client.create_experiment(EXPERIMENT_NAME)


def _find_run_by_tag(client: MlflowClient, experiment_id: str, tag: str, value: str):
    runs = client.search_runs([experiment_id], filter_string=f"tags.{tag} = '{value}'", max_results=1)
    return runs[0] if runs else None


def _sweep_parent_run(client: MlflowClient, experiment_id: str, sweep_key: str) -> str:
    existing = _find_run_by_tag(client, experiment_id, "sweep_key", sweep_key)
    if existing is not None:
        return existing.info.run_id
    run = client.create_run(experiment_id, tags={"sweep_key": sweep_key}, run_name=sweep_key)
    client.set_terminated(run.info.run_id)
    return run.info.run_id


@dataclass
class ImportSummary:
    imported: int = 0
    skipped: int = 0


def import_sweep(sweep_dir: Path, tracking_uri: str) -> ImportSummary:
    """Import every scheduled trial under `sweep_dir/jobs/` — exactly one record each."""
    meta = SweepMeta.load(sweep_dir)
    client = MlflowClient(tracking_uri=tracking_uri)
    experiment_id = _ensure_experiment(client)
    sweep_key = f"{meta.arm}:{meta.instruction_set_version}:{sweep_dir.name}"
    parent_run_id = _sweep_parent_run(client, experiment_id, sweep_key)

    summary = ImportSummary()
    jobs_dir = sweep_dir / "jobs"
    trial_dirs = sorted(
        p for job in sorted(jobs_dir.iterdir()) if job.is_dir() for p in sorted(job.iterdir()) if p.is_dir()
    )
    for trial_dir in trial_dirs:
        _import_trial(client, experiment_id, parent_run_id, meta, trial_dir, summary)
    log_fields(
        logger,
        logging.INFO,
        "sweep_imported",
        sweep=str(sweep_dir),
        imported=summary.imported,
        skipped=summary.skipped,
    )
    return summary


def _import_trial(
    client: MlflowClient,
    experiment_id: str,
    parent_run_id: str,
    meta: SweepMeta,
    trial_dir: Path,
    summary: ImportSummary,
) -> None:
    result_path = trial_dir / "result.json"
    instance_id = (
        instance_id_from_result(json.loads(result_path.read_text()), trial_dir.name)
        if result_path.is_file()
        else trial_dir.name
    )
    attempt = trial_dir.name
    identity = trial_identity(meta, instance_id, attempt)
    digest = artifact_digest(trial_dir)

    existing = _find_run_by_tag(client, experiment_id, "trial_key", identity)
    if existing is not None:
        if existing.data.tags.get("artifact_digest") != digest:
            raise ImportMismatchError(
                f"trial {instance_id} ({attempt}): artifacts changed since import; "
                f"existing record kept, re-import refused"
            )
        summary.skipped += 1
        return

    terminal_state = classify_terminal_state(trial_dir)
    trajectory = _read_trajectory(trial_dir)
    uptake, search = count_invocations(trajectory) if trajectory else (0, 0)

    run = client.create_run(
        experiment_id,
        tags={
            "trial_key": identity,
            "artifact_digest": digest,
            "terminal_state": terminal_state,
            "instance_id": instance_id,
            "arm": meta.arm,
            "instruction_set_version": meta.instruction_set_version,
            "mlflow.parentRunId": parent_run_id,
        },
        run_name=f"{meta.arm}/{instance_id}/{attempt}",
    )
    run_id = run.info.run_id

    for key, value in (
        ("instance_id", instance_id),
        ("arm", meta.arm),
        ("agent", meta.agent),
        ("model", meta.model),
        ("instruction_set_version", meta.instruction_set_version),
        ("dataset_revision", meta.dataset_revision),
        ("platform", meta.platform),
        ("attempt", attempt),
        ("terminal_state", terminal_state),
        ("agent_timeout_sec", meta.agent_timeout_sec),
        ("max_output_tokens", meta.max_output_tokens),
    ):
        client.log_param(run_id, key, value)

    if trajectory is not None:
        final = trajectory.get("final_metrics", {})
        prompt_tokens = final.get("total_prompt_tokens", 0)
        completion_tokens = final.get("total_completion_tokens", 0)
        client.log_metric(run_id, "total_tokens", prompt_tokens + completion_tokens)
        for key in (
            "total_prompt_tokens",
            "total_completion_tokens",
            "total_cached_tokens",
            "total_cost_usd",
            "total_steps",
        ):
            if key in final:
                client.log_metric(run_id, key, final[key])
        wall_clock = trajectory_wall_clock(trajectory)
        if wall_clock is not None:
            client.log_metric(run_id, "wall_clock_s", wall_clock)

    reward = _read_reward(trial_dir)
    if terminal_state == AGENT_FAILURE:
        reward = dict(EMPTY_ANSWER_REWARD)
    if reward is not None and terminal_state != INFRA_FAILURE:
        for key in ("reward", "precision", "recall", "f1", "any_gold_hit"):
            if key in reward:
                client.log_metric(run_id, key, float(reward[key]))
        client.log_metric(run_id, "unparsed", 1.0 if reward.get("unparsed") else 0.0)

    client.log_metric(run_id, "uptake_count", uptake)
    client.log_metric(run_id, "search_count", search)

    for artifact in (trial_dir / "agent" / "trajectory.json", trial_dir / "verifier" / "reward.json", result_path):
        if artifact.is_file():
            client.log_artifact(run_id, str(artifact))
    client.set_terminated(run_id)
    summary.imported += 1


def query_trials(
    tracking_uri: str,
    arm: str | None = None,
    instruction_set_version: str | None = None,
) -> list[dict]:
    """Select recorded trials by arm and instruction version from the store alone."""
    client = MlflowClient(tracking_uri=tracking_uri)
    experiment = client.get_experiment_by_name(EXPERIMENT_NAME)
    if experiment is None:
        return []
    clauses = ["tags.trial_key != ''"]
    if arm is not None:
        clauses.append(f"tags.arm = '{arm}'")
    if instruction_set_version is not None:
        clauses.append(f"tags.instruction_set_version = '{instruction_set_version}'")
    runs = client.search_runs([experiment.experiment_id], filter_string=" and ".join(clauses), max_results=10000)
    return [
        {
            "instance_id": run.data.tags.get("instance_id"),
            "arm": run.data.tags.get("arm"),
            "instruction_set_version": run.data.tags.get("instruction_set_version"),
            "terminal_state": run.data.tags.get("terminal_state"),
            "metrics": dict(run.data.metrics),
            "params": dict(run.data.params),
            # Provenance: which recorded attempt this is, and when it ran. The attempt
            # key carries no ordering of its own, so selection among repeats needs the
            # timestamp the store holds.
            "attempt": run.data.params.get("attempt"),
            "trial_key": run.data.tags.get("trial_key"),
            "started_at": run.info.start_time,
        }
        for run in runs
    ]
