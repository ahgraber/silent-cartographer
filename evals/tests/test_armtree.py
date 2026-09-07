"""Arm construction: baseline purity, treatment injection, instruction parity, build manifest."""

import json

import pytest

from c10r_evals.armtree import (
    INJECTION_MARKER,
    inject_c10r,
    read_build_manifest,
    write_build_manifest,
)
from c10r_evals.taskgen import generate_tasks

REVISION = "rev-abc"


@pytest.fixture
def trees(tmp_path, instances):
    generate_tasks(instances, tmp_path / "tasks", REVISION)
    binary = tmp_path / "c10r"
    binary.write_bytes(b"\x7fELF-fake-static-binary")
    injected = inject_c10r(tmp_path / "tasks" / "treatment", binary)
    assert len(injected) == len(instances)
    return tmp_path / "tasks"


def test_arm_instructions_identical(trees, instances):
    for instance in instances:
        baseline = (trees / "baseline" / instance.instance_id / "instruction.md").read_bytes()
        treatment = (trees / "treatment" / instance.instance_id / "instruction.md").read_bytes()
        assert baseline == treatment


def test_baseline_tree_pure(trees, instances):
    for instance in instances:
        task_dir = trees / "baseline" / instance.instance_id
        assert not (task_dir / "environment" / "c10r").exists()
        for file in sorted(p for p in task_dir.rglob("*") if p.is_file()):
            assert "c10r" not in file.read_text().lower(), f"c10r reference in baseline file {file}"


def test_treatment_tree_injected(trees, instances):
    for instance in instances:
        task_dir = trees / "treatment" / instance.instance_id
        dockerfile = (task_dir / "environment" / "Dockerfile").read_text()
        assert INJECTION_MARKER in dockerfile
        assert "COPY c10r /usr/local/bin/c10r" in dockerfile
        assert "RUN c10r build --language python" in dockerfile
        assert (task_dir / "environment" / "c10r").is_file()


def test_injection_is_idempotent(trees, instances, tmp_path):
    binary = tmp_path / "c10r-again"
    binary.write_bytes(b"\x7fELF-fake-static-binary")
    injected = inject_c10r(trees / "treatment", binary)
    assert injected == []
    dockerfile = (trees / "treatment" / instances[0].instance_id / "environment" / "Dockerfile").read_text()
    assert dockerfile.count(INJECTION_MARKER) == 1


def test_reinjection_refreshes_stale_binary(trees, instances, tmp_path):
    new_binary = tmp_path / "c10r-v2"
    new_binary.write_bytes(b"\x7fELF-new-build-with-fix")
    injected = inject_c10r(trees / "treatment", new_binary)
    assert len(injected) == len(instances)
    for instance in instances:
        task_dir = trees / "treatment" / instance.instance_id
        assert (task_dir / "environment" / "c10r").read_bytes() == b"\x7fELF-new-build-with-fix"
        assert (task_dir / "environment" / "Dockerfile").read_text().count(INJECTION_MARKER) == 1


def test_injection_refuses_baseline_tree(trees, tmp_path):
    binary = tmp_path / "c10r-2"
    binary.write_bytes(b"\x7fELF")
    with pytest.raises(ValueError, match="refusing to inject"):
        inject_c10r(trees / "baseline", binary)


def test_build_manifest_round_trip(tmp_path):
    entries = [
        {"instance_id": "demo__repo-1", "wall_clock_s": 41, "index_bytes": 1_234_567},
        {"instance_id": "demo__repo-2", "wall_clock_s": 55, "index_bytes": 2_000_000},
    ]
    path = tmp_path / "build-manifest.json"
    write_build_manifest(path, entries)
    assert read_build_manifest(path) == entries
    assert json.loads(path.read_text())["entries"][0]["instance_id"] == "demo__repo-1"


def test_build_manifest_rejects_incomplete_entry(tmp_path):
    with pytest.raises(ValueError, match="missing 'index_bytes'"):
        write_build_manifest(tmp_path / "m.json", [{"instance_id": "x", "wall_clock_s": 1}])
