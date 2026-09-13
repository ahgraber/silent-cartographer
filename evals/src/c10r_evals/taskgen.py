"""Harbor-format localization task generation: one logical episode per instance, one task per (instance, arm).

Both arm trees leave generation identical except for the recorded arm; the treatment
tree's c10r injection is a separate transformation applied afterwards.
Generated content is a pure function of its inputs — no timestamps — so regeneration
over the same dataset revision is equivalent.
"""

import importlib.resources
import json
import re
from collections import Counter
from pathlib import Path

from c10r_evals.dataset import Instance
from c10r_evals.gold import parse_changed_files

ARMS = ("baseline", "treatment")
ANSWER_PATH = "/answer.json"

INSTRUCTION_TEMPLATE = """\
# Localization task

## Issue

{issue}

## Your job

Identify the minimal set of files in this repository that must change to resolve the issue above.
You may optionally name the symbols (functions, classes, methods) that must change within each file.

## Answer format (required)

Write exactly one JSON object to `{answer_path}` with this shape:

```json
{{"files": ["path/relative/to/repo/root.py"], "symbols": {{"path/relative/to/repo/root.py": ["symbol_name"]}}}}
```

- `files` is required: repository-root-relative paths of the files that must change.
- `symbols` is optional.
- Write only the JSON object to `{answer_path}` — no surrounding prose.

## Rules

- Identify the files by reading the repository. Do not run the code, do not reproduce the issue, and do not install packages.
- Do not modify any file in the repository. Do not write a patch or a fix.
- Do not run the repository's test suite.
- After writing `{answer_path}`, stop.
"""

DOCKERFILE_TEMPLATE = """\
FROM python:3.11-slim
COPY --from=ghcr.io/astral-sh/uv:0.12.5 /uv /uvx /usr/local/bin/
RUN apt-get update \\
    && apt-get install -y --no-install-recommends git ripgrep \\
    && rm -rf /var/lib/apt/lists/*
WORKDIR /repo
RUN git clone https://github.com/{repo}.git /repo && git -C /repo checkout --quiet {base_commit}
# Resolved Python environment in the workspace root, identical across arms; the
# project itself installs best-effort (no deps) so language tooling can resolve it.
RUN uv venv /repo/.venv \\
    && (uv pip install --python /repo/.venv/bin/python --quiet --no-deps -e /repo || true)
"""

TASK_TOML_TEMPLATE = """\
[task]
name = "{arm}/{instance_id}"
description = "SWE-bench-Live localization episode {instance_id} ({arm} arm)"
keywords = ["swe-bench-live", "localization"]

[metadata]
instance_id = "{instance_id}"
arm = "{arm}"
dataset_revision = "{dataset_revision}"
source_file_count = {source_file_count}
source_file_cue = {source_file_cue}
"""

TEST_SH = """\
#!/bin/sh
set -eu
exec python3 "$(dirname "$0")/grade.py"
"""


def _grader_source() -> str:
    return importlib.resources.files("c10r_evals").joinpath("verifier_grade.py").read_text()


def compute_characteristics(issue_text: str, tracked_python_paths: list[str]) -> tuple[int, bool]:
    """Pre-run task characteristics: tracked `.py` count and whether the issue names a source file.

    An issue names a source file when its text contains an exact tracked `.py` path, or a `.py`
    basename that occurs in exactly one tracked path. Basename matches require a token boundary
    (not immediately preceded or followed by a word character or `-`) so an issue mentioning
    `myutils.py` does not count as containing the tracked basename `utils.py`. The boundary
    excludes only word characters and `-`, not `/` or `.`: an issue routinely cites a file via a
    partial path (e.g. `tablib/core.py` for a tracked `src/tablib/core.py`), and that still
    unambiguously names the file when its basename is unique among tracked sources. The
    exact-path check has no boundary requirement at all, since a full path is unambiguous
    wherever it appears.
    """
    source_file_count = len(tracked_python_paths)

    if any(path in issue_text for path in tracked_python_paths):
        return source_file_count, True

    basename_counts = Counter(Path(path).name for path in tracked_python_paths)
    unique_basenames = (name for name, n in basename_counts.items() if n == 1)
    for name in unique_basenames:
        pattern = r"(?<![\w-])" + re.escape(name) + r"(?![\w-])"
        if re.search(pattern, issue_text):
            return source_file_count, True

    return source_file_count, False


def write_task(
    task_dir: Path,
    instance: Instance,
    arm: str,
    dataset_revision: str,
    tracked_python_paths: list[str],
) -> None:
    """Write one Harbor task directory for (instance, arm).

    Layout follows Pier's task contract: `instruction.md` + `task.toml` +
    `environment/Dockerfile` + `tests/test.sh` (the verifier entry point, which
    writes the reward into the environment's verifier logs directory).

    `tracked_python_paths` contains tracked `.py` paths at the instance's base commit; it is
    required, and both arms must use the same list.
    """
    (task_dir / "environment").mkdir(parents=True, exist_ok=True)
    (task_dir / "tests").mkdir(parents=True, exist_ok=True)

    (task_dir / "instruction.md").write_text(
        INSTRUCTION_TEMPLATE.format(issue=instance.problem_statement.strip(), answer_path=ANSWER_PATH)
    )
    (task_dir / "environment" / "Dockerfile").write_text(
        DOCKERFILE_TEMPLATE.format(repo=instance.repo, base_commit=instance.base_commit)
    )
    source_file_count, source_file_cue = compute_characteristics(instance.problem_statement, tracked_python_paths)
    (task_dir / "task.toml").write_text(
        TASK_TOML_TEMPLATE.format(
            instance_id=instance.instance_id,
            arm=arm,
            dataset_revision=dataset_revision,
            source_file_count=source_file_count,
            source_file_cue=str(source_file_cue).lower(),
        )
    )
    gold_files = parse_changed_files(instance.patch)
    (task_dir / "tests" / "gold.json").write_text(json.dumps({"gold_files": gold_files}, indent=2) + "\n")
    (task_dir / "tests" / "grade.py").write_text(_grader_source())
    test_sh = task_dir / "tests" / "test.sh"
    test_sh.write_text(TEST_SH)
    test_sh.chmod(0o755)


def generate_tasks(
    instances: list[Instance],
    out_dir: Path,
    dataset_revision: str,
    tracked_paths_by_instance: dict[str, list[str]],
    arms: tuple[str, ...] = ARMS,
) -> list[Path]:
    """Generate one task per (instance, arm) under `out_dir/<arm>/<instance_id>/`.

    Refuses repeated instance ids: the task directory is named for the id, so a repeat would
    overwrite the earlier task and leave the episode graded against the later row's fix.

    `tracked_paths_by_instance` maps each instance to tracked `.py` paths at its base commit;
    every instance is required, and both arms use the same entry.
    """
    counts = Counter(instance.instance_id for instance in instances)
    repeated = sorted(instance_id for instance_id, n in counts.items() if n > 1)
    if repeated:
        raise ValueError(f"instances repeat these ids, which would overwrite tasks: {', '.join(repeated)}")

    missing = sorted(
        instance.instance_id for instance in instances if instance.instance_id not in tracked_paths_by_instance
    )
    if missing:
        raise ValueError(f"tracked_paths_by_instance is missing these instance ids: {', '.join(missing)}")

    task_dirs: list[Path] = []
    for arm in arms:
        for instance in instances:
            task_dir = out_dir / arm / instance.instance_id
            write_task(task_dir, instance, arm, dataset_revision, tracked_paths_by_instance[instance.instance_id])
            task_dirs.append(task_dir)
    return task_dirs
