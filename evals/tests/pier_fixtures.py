"""Pier sweep/trial fixtures in the output shapes Pier 0.3.1 actually writes.

Field names here are load-bearing: they are what the importer reads. They were taken
from a real trial directory and from Pier's `TrialResult` and ATIF `ToolCall` models —
a trial result carries `task_name` and `exception_info` (never a `status` string), and
a tool call names its tool in `function_name`.
"""

import json
from datetime import UTC, datetime, timedelta
from pathlib import Path

DEFAULT_FINAL_METRICS = {
    "total_prompt_tokens": 30_000,
    "total_completion_tokens": 2_000,
    "total_cached_tokens": 25_000,
    "total_cost_usd": 0.12,
    "total_steps": 14,
}

# Reward files are flat numeric mappings (Pier's VerifierResult contract).
GRADED_REWARD = {
    "reward": 0.8,
    "precision": 1.0,
    "recall": 0.6666,
    "f1": 0.8,
    "any_gold_hit": 1,
    "unparsed": 0,
    "symbols_present": 0,
}

UNPARSED_REWARD = {
    "reward": 0.0,
    "precision": 0.0,
    "recall": 0.0,
    "f1": 0.0,
    "any_gold_hit": 0,
    "unparsed": 1,
    "symbols_present": 0,
}

START_TS = datetime(2026, 9, 3, 13, 38, 0, tzinfo=UTC)


def tool_call(function_name: str, **arguments: object) -> dict:
    return {
        "tool_call_id": f"call-{function_name}-{len(arguments)}",
        "function_name": function_name,
        "arguments": arguments,
    }


def bash_call(command: str) -> dict:
    return tool_call("Bash", command=command)


def make_trajectory(
    tool_calls: list[dict],
    final_metrics: dict | None = None,
    step_seconds: float = 30.0,
) -> dict:
    """One step per tool call, with the ISO-8601 timestamps Pier writes."""
    return {
        "schema_version": "1.7.0",
        "session_id": "session-1",
        "final_metrics": dict(final_metrics or DEFAULT_FINAL_METRICS),
        "steps": [
            {
                "step_id": f"step-{i}",
                "source": "agent",
                "timestamp": (START_TS + timedelta(seconds=i * step_seconds)).isoformat(),
                "tool_calls": [call],
            }
            for i, call in enumerate(tool_calls)
        ],
    }


def make_toolless_trajectory(final_metrics: dict | None = None) -> dict:
    """A trajectory whose steps ran no tool: Pier omits `tool_calls` entirely."""
    return {
        "schema_version": "1.7.0",
        "session_id": "session-1",
        "final_metrics": dict(final_metrics or DEFAULT_FINAL_METRICS),
        "steps": [
            {"step_id": "step-0", "source": "user", "timestamp": START_TS.isoformat()},
            {"step_id": "step-1", "source": "agent", "timestamp": (START_TS + timedelta(seconds=1)).isoformat()},
        ],
    }


def write_sweep_meta(
    sweep_dir: Path,
    arm: str = "treatment",
    instruction_set_version: str = "v1",
    dataset_revision: str = "rev-abc",
    model: str = "proxy/some-local-model",
) -> None:
    sweep_dir.mkdir(parents=True, exist_ok=True)
    (sweep_dir / "sweep-meta.json").write_text(
        json.dumps(
            {
                "arm": arm,
                "instruction_set_version": instruction_set_version,
                "dataset_revision": dataset_revision,
                "model": model,
            }
        )
    )


def make_trial(
    sweep_dir: Path,
    instance_id: str,
    exception_type: str | None = None,
    reward: dict | None = GRADED_REWARD,
    trajectory: dict | None = None,
    job: str = "job-1",
    attempt_suffix: str = "1",
    arm: str = "treatment",
) -> Path:
    """Write one trial directory in Pier's output shape and return its path."""
    trial_name = f"{instance_id}__{attempt_suffix}"
    trial_dir = sweep_dir / "jobs" / job / trial_name
    (trial_dir / "agent").mkdir(parents=True, exist_ok=True)
    (trial_dir / "verifier").mkdir(parents=True, exist_ok=True)

    result = {
        "task_name": f"{arm}/{instance_id}",
        "trial_name": trial_name,
        "task_id": {"path": f"tasks/dev/{arm}/{instance_id}"},
        "source": arm,
        "exception_info": (
            {"exception_type": exception_type, "exception_message": f"{exception_type} raised"}
            if exception_type
            else None
        ),
    }
    (trial_dir / "result.json").write_text(json.dumps(result))
    if trajectory is None:
        trajectory = make_trajectory([bash_call("ls /repo"), bash_call("grep -rn handler /repo")])
    (trial_dir / "agent" / "trajectory.json").write_text(json.dumps(trajectory))
    if reward is not None:
        (trial_dir / "verifier" / "reward.json").write_text(json.dumps(reward))
    return trial_dir
