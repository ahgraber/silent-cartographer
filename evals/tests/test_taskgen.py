"""Task generation: instance fidelity, episode contract, determinism."""

import json
import tomllib
from pathlib import Path

from c10r_evals.taskgen import ANSWER_PATH, ARMS, generate_tasks

REVISION = "rev-abc"


def _tree_files(root: Path) -> dict[str, bytes]:
    return {str(p.relative_to(root)): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def test_instance_fidelity(tmp_path, instances):
    task_dirs = generate_tasks(instances, tmp_path, REVISION)

    assert len(task_dirs) == len(instances) * len(ARMS)
    for arm in ARMS:
        arm_tasks = sorted((tmp_path / arm).iterdir())
        assert len(arm_tasks) == len(instances)

    for instance in instances:
        for arm in ARMS:
            task_dir = tmp_path / arm / instance.instance_id
            dockerfile = (task_dir / "environment" / "Dockerfile").read_text()
            assert instance.base_commit in dockerfile
            assert instance.repo in dockerfile
            instruction = (task_dir / "instruction.md").read_text()
            assert instance.problem_statement.strip() in instruction
            meta = tomllib.loads((task_dir / "task.toml").read_text())
            assert meta["task"]["name"] == f"{arm}/{instance.instance_id}"
            assert meta["metadata"]["instance_id"] == instance.instance_id
            assert meta["metadata"]["arm"] == arm
            assert meta["metadata"]["dataset_revision"] == REVISION


def test_episode_contract(tmp_path, instances):
    generate_tasks(instances, tmp_path, REVISION)
    task_dir = tmp_path / "baseline" / instances[0].instance_id

    instruction = (task_dir / "instruction.md").read_text()
    assert ANSWER_PATH in instruction
    assert '"files"' in instruction
    assert (
        "files that must change" in instruction.lower()
        or "files in this repository that must change" in instruction.lower()
    )
    assert "do not modify any file" in instruction.lower()
    assert "do not run the repository's test suite" in instruction.lower()
    # The episode measures locating code, so reproducing the issue is out of scope:
    # left in, the debugging loop dominates the token cost the comparison reads.
    assert "do not run the code" in instruction.lower()
    assert "do not install packages" in instruction.lower()

    test_sh = (task_dir / "tests" / "test.sh").read_text()
    assert "grade.py" in test_sh
    dockerfile = (task_dir / "environment" / "Dockerfile").read_text()
    for text in (dockerfile, test_sh):
        assert "pytest" not in text
        assert "tox" not in text


def test_deterministic_generation(tmp_path, instances):
    first = tmp_path / "run1"
    second = tmp_path / "run2"
    generate_tasks(instances, first, REVISION)
    generate_tasks(instances, second, REVISION)

    files_first = _tree_files(first)
    files_second = _tree_files(second)
    assert files_first == files_second


def test_gold_embedded_in_verifier_data(tmp_path, instances):
    generate_tasks(instances, tmp_path, REVISION)
    gold = json.loads((tmp_path / "baseline" / "demo__repo-2" / "tests" / "gold.json").read_text())
    assert gold["gold_files"] == ["pkg/core.py", "pkg/util/paths.py"]

    grader = (tmp_path / "baseline" / "demo__repo-2" / "tests" / "grade.py").read_text()
    packaged = Path("src/c10r_evals/verifier_grade.py").read_text()
    assert grader == packaged
