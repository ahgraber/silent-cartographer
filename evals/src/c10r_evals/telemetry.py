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
from dataclasses import dataclass, field, fields
from pathlib import Path

from mlflow.tracking import MlflowClient

from c10r_evals import trial_evidence
from c10r_evals.armconfig import recorded_setting_keys
from c10r_evals.runtime import log_fields

logger = logging.getLogger(__name__)

EXPERIMENT_NAME = "c10r-evals"

COMPLETED = trial_evidence.COMPLETED
AGENT_FAILURE = trial_evidence.AGENT_FAILURE
INFRA_FAILURE = trial_evidence.INFRA_FAILURE
instance_id_from_result = trial_evidence.instance_id_from_result
episode_consumed_tokens = trial_evidence.episode_consumed_tokens
classify_terminal_state = trial_evidence.classify_terminal_state
count_invocations = trial_evidence.count_invocations
trajectory_wall_clock = trial_evidence.trajectory_wall_clock
task_characteristics = trial_evidence.task_characteristics
_read_trajectory = trial_evidence.read_trajectory
_read_reward = trial_evidence.read_reward

EMPTY_ANSWER_REWARD = {
    "reward": 0.0,
    "precision": 0.0,
    "recall": 0.0,
    "f1": 0.0,
    "any_gold_hit": 0,
    "unparsed": True,
}

# Categories Pier's FinalMetrics model carries in `extra` rather than as first-class
# fields: `extra` is reserved for "custom aggregate metrics" precisely because not every
# harness reports every category (Claude Code, for example, only populates the
# cache-write count when a step actually wrote to the cache).
TOKEN_EXTRA_METRIC_KEYS = (
    "total_reasoning_tokens",
    "total_cache_creation_input_tokens",
)


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
    # Every other recorded key, kept as written. A run setting added to the renderer is
    # recorded here and logged as a parameter without changing this class. `load` requires
    # every currently declared setting to be present, so a sweep whose metadata predates
    # one fails to import rather than silently recording fewer params than another sweep.
    settings: dict = field(default_factory=dict)

    @classmethod
    def load(cls, sweep_dir: Path) -> "SweepMeta":
        meta_path = sweep_dir / "sweep-meta.json"
        raw = json.loads(meta_path.read_text())
        named = {f.name for f in fields(cls)} - {"settings"}
        settings = {k: v for k, v in raw.items() if k not in named}
        missing = sorted(recorded_setting_keys() - settings.keys())
        if missing:
            raise ValueError(
                f"{meta_path} is missing declared setting(s): {', '.join(missing)}; "
                "re-render the sweep configuration before importing"
            )
        return cls(**{k: v for k, v in raw.items() if k in named}, settings=settings)


def trial_identity(meta: SweepMeta, instance_id: str, attempt: str) -> str:
    """Deterministic identity key for one scheduled trial."""
    sep = "\x1f"
    material = (
        f"{meta.dataset_revision}{sep}{instance_id}{sep}{meta.arm}{sep}{meta.instruction_set_version}{sep}{attempt}"
    )
    return hashlib.sha256(material.encode()).hexdigest()[:24]


DIGEST_EXCLUDED_DIRS = ("agent/sessions",)


def artifact_digest(trial_dir: Path) -> str:
    """Content digest over the trial's recorded evidence, order-independent.

    `agent/sessions` is excluded because it is not measurement evidence.
    """

    def included(path: Path) -> bool:
        # Test the path before touching the filesystem: an excluded file may be unreadable,
        # and `is_file()` would raise on it.
        relative = path.relative_to(trial_dir).as_posix()
        if any(relative == excluded or relative.startswith(f"{excluded}/") for excluded in DIGEST_EXCLUDED_DIRS):
            return False
        return path.is_file()

    digest = hashlib.sha256()
    for file in sorted(p for p in trial_dir.rglob("*") if included(p)):
        digest.update(file.relative_to(trial_dir).as_posix().encode())
        digest.update(b"\x00")
        digest.update(hashlib.sha256(file.read_bytes()).digest())
    return digest.hexdigest()[:24]


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


def import_sweep(sweep_dir: Path, tracking_uri: str, *, project_root: Path | None = None) -> ImportSummary:
    """Import every scheduled trial under `sweep_dir/jobs/` — exactly one record each.

    `project_root` anchors relative task paths and defaults to the current working directory.
    """
    project_root = project_root if project_root is not None else Path.cwd()
    meta = SweepMeta.load(sweep_dir)
    client = MlflowClient(tracking_uri=tracking_uri)
    experiment_id = _ensure_experiment(client)
    sweep_key = f"{meta.arm}:{meta.instruction_set_version}:{sweep_dir.name}"
    parent_run_id = _sweep_parent_run(client, experiment_id, sweep_key)

    summary = ImportSummary()
    jobs_dir = sweep_dir / "jobs"
    if not jobs_dir.is_dir():
        # An arm that has not run yet is not an error; the other arm still imports.
        log_fields(logger, logging.INFO, "sweep_skipped", sweep=str(sweep_dir), reason="no jobs directory")
        return summary
    trial_dirs = sorted(
        p for job in sorted(jobs_dir.iterdir()) if job.is_dir() for p in sorted(job.iterdir()) if p.is_dir()
    )
    # A trial directory without result.json never produced a record: the run was cancelled or
    # killed mid-flight. Importing one would invent an instance id from the directory name.
    trial_dirs = [p for p in trial_dirs if (p / "result.json").is_file()]
    for trial_dir in trial_dirs:
        _import_trial(client, experiment_id, parent_run_id, meta, trial_dir, summary, project_root)
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
    project_root: Path,
) -> None:
    result_path = trial_dir / "result.json"
    if not result_path.is_file():
        raise ValueError(
            f"trial {trial_dir.name} has no result.json, so its instance is unknown; "
            "import_sweep filters these out before reaching here"
        )
    result = json.loads(result_path.read_text())
    instance_id = instance_id_from_result(result, trial_dir.name)
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

    # Read before anything is written for this trial: a task tree that is missing or
    # missing characteristics means something upstream is wrong, and that must abort
    # before a partial record for this trial exists to leave behind.
    characteristics = task_characteristics(result, project_root)

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
        *sorted(meta.settings.items()),
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
        extra = final.get("extra") or {}
        for key in TOKEN_EXTRA_METRIC_KEYS:
            if key in extra:
                client.log_metric(run_id, key, extra[key])
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
    client.log_metric(run_id, "source_file_count", float(characteristics["source_file_count"]))
    client.log_metric(run_id, "source_file_cue", 1.0 if characteristics["source_file_cue"] else 0.0)

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
