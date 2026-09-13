"""Task generation: instance fidelity, episode contract, determinism."""

import json
import tomllib
from pathlib import Path

import pytest

from c10r_evals.dataset import Instance
from c10r_evals.taskgen import ANSWER_PATH, ARMS, compute_characteristics, generate_tasks

REVISION = "rev-abc"


def _instance(problem_statement: str, instance_id: str = "demo__repo-1") -> Instance:
    return Instance(
        instance_id=instance_id,
        repo="demo/repo",
        base_commit="a" * 40,
        problem_statement=problem_statement,
        patch="",
        test_patch="",
    )


def _no_tracked_paths(instances: list[Instance]) -> dict[str, list[str]]:
    """A complete tracked-paths mapping for tests that don't care about the recorded values."""
    return {instance.instance_id: [] for instance in instances}


def test_repeated_instance_ids_are_refused(tmp_path, instances):
    """A task directory is named for its instance, so a repeat would overwrite the earlier task."""
    with pytest.raises(ValueError, match=instances[0].instance_id):
        generate_tasks([instances[0], instances[1], instances[0]], tmp_path, REVISION, _no_tracked_paths(instances))


def _tree_files(root: Path) -> dict[str, bytes]:
    return {str(p.relative_to(root)): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def test_instance_fidelity(tmp_path, instances):
    task_dirs = generate_tasks(instances, tmp_path, REVISION, _no_tracked_paths(instances))

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
    generate_tasks(instances, tmp_path, REVISION, _no_tracked_paths(instances))
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
    generate_tasks(instances, first, REVISION, _no_tracked_paths(instances))
    generate_tasks(instances, second, REVISION, _no_tracked_paths(instances))

    files_first = _tree_files(first)
    files_second = _tree_files(second)
    assert files_first == files_second


def test_characteristics_recorded_identically_per_arm(tmp_path):
    """Both arms of the same instance record the same pre-run characteristics."""
    instance = _instance("The fix probably belongs somewhere in paths.py.")
    tracked = {instance.instance_id: ["pkg/core.py", "pkg/util/paths.py"]}

    generate_tasks([instance], tmp_path, REVISION, tracked_paths_by_instance=tracked)

    metas = {
        arm: tomllib.loads((tmp_path / arm / instance.instance_id / "task.toml").read_text())["metadata"]
        for arm in ARMS
    }
    assert metas["baseline"]["source_file_count"] == 2
    assert metas["baseline"]["source_file_cue"] is True
    assert metas["baseline"]["source_file_count"] == metas["treatment"]["source_file_count"]
    assert metas["baseline"]["source_file_cue"] == metas["treatment"]["source_file_cue"]


def test_cue_exact_path():
    """An issue containing an exact tracked path is a hit, even with other similarly-named files."""
    tracked = ["pkg/core.py", "pkg/util/paths.py", "pkg/util/other_paths.py"]

    count, cue = compute_characteristics("Crash when pkg/util/paths.py is imported twice.", tracked)

    assert count == 3
    assert cue is True


def test_cue_unique_basename():
    """A bare basename mention counts when it names exactly one tracked file."""
    tracked = ["pkg/core.py", "pkg/util/paths.py"]

    count, cue = compute_characteristics("The bug lives somewhere in paths.py, not sure where.", tracked)

    assert count == 2
    assert cue is True


def test_cue_ambiguous_basename():
    """The same basename tracked under two paths makes the cue false, exact-path or not."""
    tracked = ["pkg/a/utils.py", "pkg/b/utils.py", "pkg/core.py"]

    count, cue = compute_characteristics("Something in utils.py looks wrong.", tracked)

    assert count == 3
    assert cue is False


def test_cue_absent():
    """No tracked path or basename appears anywhere in the issue text."""
    tracked = ["pkg/core.py", "pkg/util/paths.py"]

    count, cue = compute_characteristics("The application crashes on startup with no traceback.", tracked)

    assert count == 2
    assert cue is False


def test_cue_basename_requires_token_boundary():
    """`myutils.py` does not count as containing the tracked basename `utils.py`."""
    tracked = ["pkg/utils.py"]

    count, cue = compute_characteristics("Something in myutils.py looks wrong.", tracked)

    assert count == 1
    assert cue is False


def test_cue_basename_matches_within_a_partial_path():
    """A unique basename cited via a partial path (no exact tracked path present) still counts.

    `tablib/core.py` is not the exact tracked path (`src/tablib/core.py`), but it unambiguously
    names the tracked `core.py`, so the basename rule should fire.
    """
    tracked = ["src/tablib/core.py", "src/tablib/utils.py", "tests/test_core.py"]

    count, cue = compute_characteristics("the bug is in tablib/core.py", tracked)

    assert count == 3
    assert cue is True

    # The decoy that motivates the token-boundary rule in the first place still fails.
    count, cue = compute_characteristics("the bug is in mycore.py", tracked)
    assert cue is False


def test_missing_tracked_paths_entry_raises(tmp_path, instances):
    """An instance generation never resolved tracked paths for cannot record a computed value.

    Falling back to zero would be indistinguishable, later, from a repository that genuinely
    has no tracked `.py` files.
    """
    incomplete = {instances[0].instance_id: []}

    with pytest.raises(ValueError, match=instances[1].instance_id):
        generate_tasks(instances, tmp_path, REVISION, incomplete)


def test_gold_embedded_in_verifier_data(tmp_path, instances):
    generate_tasks(instances, tmp_path, REVISION, _no_tracked_paths(instances))
    gold = json.loads((tmp_path / "baseline" / "demo__repo-2" / "tests" / "gold.json").read_text())
    assert gold["gold_files"] == ["pkg/core.py", "pkg/util/paths.py"]

    grader = (tmp_path / "baseline" / "demo__repo-2" / "tests" / "grade.py").read_text()
    packaged = Path("src/c10r_evals/verifier_grade.py").read_text()
    assert grader == packaged
