"""Decode Pier trial artifacts without experiment-store dependencies."""

import json
import re
import tomllib
from pathlib import Path

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

# A command starts at the beginning of the string or after a separator, and a newline is a
# separator: one Bash tool call routinely carries several commands on their own lines.
# Requiring the separator is what keeps an argument from counting as an invocation — `c10r find`
# names a c10r subcommand, not the search tool, and `--exclude-dir=.c10r` names the index
# directory. The trailing lookahead does the same job on the other side, for `c10r-wrapper.sh`.
_COMMAND_START = r"(?:^|[;&|()`\n]\s*)"
_C10R_INVOCATION = re.compile(rf"{_COMMAND_START}c10r(?![\w./-])")
_SEARCH_INVOCATION = re.compile(rf"{_COMMAND_START}(?:grep|egrep|fgrep|rg|fd|find)(?![\w./-])")
_BASH_TOOL_NAMES = {"bash", "shell", "terminal"}
_SEARCH_TOOL_NAMES = {"grep", "glob"}

# The two pre-run task characteristics every generated task.toml's [metadata] table
# carries (see taskgen.py); generation refuses to write a task without them.
TASK_CHARACTERISTIC_KEYS = ("source_file_count", "source_file_cue")


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


def _resolve_task_dir(result: dict, project_root: Path) -> Path:
    """Where the trial's task tree lives on disk, from its recorded `task_id.path`.

    Relative paths are resolved against `project_root`; absolute paths are used as-is.
    """
    task_id = result.get("task_id")
    path = task_id.get("path") if isinstance(task_id, dict) else None
    if not isinstance(path, str) or not path:
        raise ValueError("trial result.json carries no task_id.path; its task tree is unknown")
    task_dir = Path(path)
    return task_dir if task_dir.is_absolute() else project_root / task_dir


def task_characteristics(result: dict, project_root: Path) -> dict[str, object]:
    """Read a trial's pre-run task characteristics from its generated `task.toml`.

    Raises `ValueError` when the task tree is missing or either required characteristic is absent.
    """
    task_dir = _resolve_task_dir(result, project_root)
    task_toml = task_dir / "task.toml"
    if not task_toml.is_file():
        raise ValueError(f"task tree not found at {task_toml}; the trial's task_id.path does not resolve")
    metadata = tomllib.loads(task_toml.read_text()).get("metadata", {})
    missing = [key for key in TASK_CHARACTERISTIC_KEYS if key not in metadata]
    if missing:
        raise ValueError(f"{task_toml} is missing {', '.join(missing)}; regenerate the task tree")
    return {key: metadata[key] for key in TASK_CHARACTERISTIC_KEYS}


def read_trajectory(trial_dir: Path) -> dict | None:
    path = trial_dir / "agent" / "trajectory.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text())


def read_reward(trial_dir: Path) -> dict | None:
    path = trial_dir / "verifier" / "reward.json"
    if not path.is_file():
        return None
    return json.loads(path.read_text())
