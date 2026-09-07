"""Harbor-format localization task generation: one logical episode per instance, one task per (instance, arm).

Both arm trees leave generation identical except for the recorded arm; the treatment
tree's c10r injection is a separate transformation applied afterwards.
Generated content is a pure function of its inputs — no timestamps — so regeneration
over the same dataset revision is equivalent.
"""

import importlib.resources
import json
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
"""

TEST_SH = """\
#!/bin/sh
set -eu
exec python3 "$(dirname "$0")/grade.py"
"""


def _grader_source() -> str:
    return importlib.resources.files("c10r_evals").joinpath("verifier_grade.py").read_text()


def write_task(task_dir: Path, instance: Instance, arm: str, dataset_revision: str) -> None:
    """Write one Harbor task directory for (instance, arm).

    Layout follows Pier's task contract: `instruction.md` + `task.toml` +
    `environment/Dockerfile` + `tests/test.sh` (the verifier entry point, which
    writes the reward into the environment's verifier logs directory).
    """
    (task_dir / "environment").mkdir(parents=True, exist_ok=True)
    (task_dir / "tests").mkdir(parents=True, exist_ok=True)

    (task_dir / "instruction.md").write_text(
        INSTRUCTION_TEMPLATE.format(issue=instance.problem_statement.strip(), answer_path=ANSWER_PATH)
    )
    (task_dir / "environment" / "Dockerfile").write_text(
        DOCKERFILE_TEMPLATE.format(repo=instance.repo, base_commit=instance.base_commit)
    )
    (task_dir / "task.toml").write_text(
        TASK_TOML_TEMPLATE.format(instance_id=instance.instance_id, arm=arm, dataset_revision=dataset_revision)
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
    arms: tuple[str, ...] = ARMS,
) -> list[Path]:
    """Generate one task per (instance, arm) under `out_dir/<arm>/<instance_id>/`."""
    task_dirs: list[Path] = []
    for arm in arms:
        for instance in instances:
            task_dir = out_dir / arm / instance.instance_id
            write_task(task_dir, instance, arm, dataset_revision)
            task_dirs.append(task_dir)
    return task_dirs
