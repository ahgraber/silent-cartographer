"""Arm construction: baseline purity, treatment injection, instruction parity, build manifest."""

import json

import pytest

from c10r_evals.armtree import (
    INJECTION_MARKER,
    inject_c10r,
    merge_build_manifest,
    read_build_manifest,
    read_build_records,
    write_build_manifest,
)
from c10r_evals.taskgen import generate_tasks

REVISION = "rev-abc"


@pytest.fixture
def trees(tmp_path, instances):
    generate_tasks(instances, tmp_path / "tasks", REVISION, {i.instance_id: [] for i in instances})
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


def test_shared_toolchain_is_installed_above_the_repository_steps(trees, instances):
    for instance in instances:
        dockerfile = (trees / "treatment" / instance.instance_id / "environment" / "Dockerfile").read_text()
        clone = dockerfile.index("git clone")

        assert dockerfile.index("COPY c10r /usr/local/bin/c10r") < clone, "c10r binary must precede the clone"
        assert dockerfile.index("scip-python") < clone, "indexer toolchain must precede the clone"
        assert clone < dockerfile.index("RUN c10r build --language python"), "indexing must follow the clone"


def test_treatment_tree_retains_instance_fidelity_after_injection(trees, instances):
    for instance in instances:
        task_dir = trees / "treatment" / instance.instance_id
        dockerfile = (task_dir / "environment" / "Dockerfile").read_text()
        assert instance.repo in dockerfile
        assert instance.base_commit in dockerfile


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


def entry(instance_id: str, wall: int = 10, index_bytes: int = 1_000) -> dict:
    return {"instance_id": instance_id, "wall_clock_s": wall, "index_bytes": index_bytes}


def test_merge_keeps_entries_the_current_build_did_not_measure(tmp_path):
    path = tmp_path / "build-manifest.json"
    write_build_manifest(path, [entry("dev-1"), entry("dev-2")])

    merged = merge_build_manifest(path, [entry("frozen-1")])

    assert [e["instance_id"] for e in merged] == ["dev-1", "dev-2", "frozen-1"]
    assert read_build_manifest(path) == merged


def test_merge_replaces_an_entry_for_the_same_instance(tmp_path):
    path = tmp_path / "build-manifest.json"
    write_build_manifest(path, [entry("dev-1", wall=10)])

    merged = merge_build_manifest(path, [entry("dev-1", wall=99)])

    assert merged == [entry("dev-1", wall=99)]


def test_merge_tolerates_a_missing_or_empty_manifest(tmp_path):
    absent = tmp_path / "build-manifest.json"
    assert merge_build_manifest(absent, [entry("dev-1")]) == [entry("dev-1")]

    empty = tmp_path / "empty.json"
    empty.write_text("")
    assert merge_build_manifest(empty, [entry("dev-1")]) == [entry("dev-1")]


def test_merge_rejects_an_incomplete_record_without_touching_the_manifest(tmp_path):
    path = tmp_path / "build-manifest.json"
    write_build_manifest(path, [entry("dev-1")])

    with pytest.raises(ValueError, match="missing 'index_bytes'"):
        merge_build_manifest(path, [{"instance_id": "dev-2", "wall_clock_s": 1}])

    assert read_build_manifest(path) == [entry("dev-1")]


def test_build_records_read_one_object_per_line(tmp_path):
    path = tmp_path / "records.jsonl"
    path.write_text(json.dumps(entry("dev-1")) + "\n\n" + json.dumps(entry("dev-2")) + "\n")

    assert read_build_records(path) == [entry("dev-1"), entry("dev-2")]


def test_a_rebuilt_instance_takes_its_latest_record(tmp_path):
    path = tmp_path / "records.jsonl"
    path.write_text(json.dumps(entry("dev-1", wall=10)) + "\n" + json.dumps(entry("dev-1", wall=99)) + "\n")

    merged = merge_build_manifest(tmp_path / "build-manifest.json", read_build_records(path))

    assert merged == [entry("dev-1", wall=99)]


def test_merge_refuses_a_malformed_manifest_rather_than_replacing_it(tmp_path):
    path = tmp_path / "build-manifest.json"
    path.write_text("{not json")

    with pytest.raises(json.JSONDecodeError):
        merge_build_manifest(path, [entry("dev-1")])

    assert path.read_text() == "{not json"


def test_build_records_refuse_a_malformed_line(tmp_path):
    path = tmp_path / "records.jsonl"
    path.write_text(json.dumps(entry("dev-1")) + '\n{"instance_id":"dev-2","wall_c\n')

    with pytest.raises(json.JSONDecodeError):
        read_build_records(path)
