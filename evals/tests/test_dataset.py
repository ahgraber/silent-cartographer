"""Parsing a SWE-bench-Live row into an instance, and the revision the task trees carry."""

import pytest

from c10r_evals.dataset import (
    Instance,
    TreeRevisionMismatch,
    read_tree_revision,
    write_dataset_manifest,
)
from c10r_evals.split import Split, write_split_manifest

ROW = {
    "instance_id": "jazzband__tablib-613",
    "repo": "jazzband/tablib",
    "base_commit": "abc123",
    "problem_statement": "Dataset.load fails on empty input",
    "patch": "--- a/src/tablib/core.py\n+++ b/src/tablib/core.py\n",
    "test_patch": "--- a/tests/test.py\n+++ b/tests/test.py\n",
}


def test_row_fields_are_carried_through():
    instance = Instance.from_row(ROW)

    assert instance.instance_id == "jazzband__tablib-613"
    assert instance.repo == "jazzband/tablib"
    assert instance.base_commit == "abc123"
    assert instance.problem_statement == "Dataset.load fails on empty input"
    assert instance.patch == ROW["patch"]
    assert instance.test_patch == ROW["test_patch"]


@pytest.mark.parametrize("absent", [{}, {"test_patch": None}], ids=["missing", "null"])
def test_an_absent_test_patch_reads_as_empty(absent):
    """The upstream rows carry `test_patch` inconsistently; downstream code indexes it as a string."""
    instance = Instance.from_row({**{k: v for k, v in ROW.items() if k != "test_patch"}, **absent})

    assert instance.test_patch == ""


@pytest.mark.parametrize("missing", sorted(set(ROW) - {"test_patch"}))
def test_a_row_missing_a_required_field_is_refused(missing):
    with pytest.raises(KeyError):
        Instance.from_row({k: v for k, v in ROW.items() if k != missing})


def write_manifests(out, dataset_revision, split_revision):
    write_dataset_manifest(out / "dataset-manifest.json", dataset_revision, instance_count=2)
    write_split_manifest(out / "split-manifest.json", Split(dev=["a"], frozen=["b"], seed=0), split_revision)


def test_tree_revision_is_the_revision_both_manifests_agree_on(tmp_path):
    write_manifests(tmp_path, "rev-a", "rev-a")
    assert read_tree_revision(tmp_path / "dataset-manifest.json") == "rev-a"


def test_disagreeing_manifests_are_refused_rather_than_resolved(tmp_path):
    """Config rendering stamps this revision into the store, so a half-written tree cannot be read."""
    write_manifests(tmp_path, "rev-b", "rev-a")

    with pytest.raises(TreeRevisionMismatch) as raised:
        read_tree_revision(tmp_path / "dataset-manifest.json")

    assert raised.value.dataset_revision == "rev-b"
    assert raised.value.split_revision == "rev-a"


def test_a_named_dataset_manifest_is_the_one_checked(tmp_path):
    """Rendering accepts a dataset manifest path; the revision returned must be that file's."""
    write_manifests(tmp_path, "rev-a", "rev-a")
    write_dataset_manifest(tmp_path / "custom.json", "rev-c", instance_count=2)

    with pytest.raises(TreeRevisionMismatch) as raised:
        read_tree_revision(tmp_path / "custom.json")

    assert raised.value.dataset_revision == "rev-c"
