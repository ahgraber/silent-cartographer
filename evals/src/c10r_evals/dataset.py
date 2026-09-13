"""SWE-bench-Live instance loading and the pinned-revision dataset manifest."""

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

DATASET_NAME = "SWE-bench-Live/SWE-bench-Live"
DATASET_SPLIT = "verified"


@dataclass(frozen=True)
class Instance:
    """One localization episode's source data."""

    instance_id: str
    repo: str
    base_commit: str
    problem_statement: str
    patch: str
    test_patch: str

    @classmethod
    def from_row(cls, row: dict[str, Any]) -> "Instance":
        return cls(
            instance_id=row["instance_id"],
            repo=row["repo"],
            base_commit=row["base_commit"],
            problem_statement=row["problem_statement"],
            patch=row["patch"],
            test_patch=row.get("test_patch", "") or "",
        )


def load_instances(revision: str, cache_dir: str | None = None) -> list[Instance]:
    """Fetch the Python `verified` split at a pinned dataset revision (network access required)."""
    from datasets import load_dataset

    rows = load_dataset(DATASET_NAME, split=DATASET_SPLIT, revision=revision, cache_dir=cache_dir)
    return [Instance.from_row(row) for row in rows]


class TreeRevisionMismatch(Exception):
    """The two generated manifests name different dataset revisions."""

    def __init__(self, dataset_revision: str, split_revision: str) -> None:
        self.dataset_revision = dataset_revision
        self.split_revision = split_revision
        super().__init__(
            f"dataset-manifest.json names {dataset_revision} and split-manifest.json names {split_revision}"
        )


def read_dataset_revision(path: Path) -> str:
    """The pinned revision the task trees were generated at."""
    return json.loads(path.read_text())["revision"]


def read_tree_revision(dataset_manifest: Path) -> str:
    """Return the revision shared by the dataset and split manifests.

    Raises `TreeRevisionMismatch` when the manifests name different revisions.
    """
    from c10r_evals.split import read_split_manifest

    dataset_revision = read_dataset_revision(dataset_manifest)
    _, split_revision = read_split_manifest(dataset_manifest.parent / "split-manifest.json")
    if dataset_revision != split_revision:
        raise TreeRevisionMismatch(dataset_revision, split_revision)
    return dataset_revision


def write_dataset_manifest(path: Path, revision: str, instance_count: int) -> None:
    """Record dataset identity, pinned revision, and instance count for replay."""
    manifest = {
        "dataset": DATASET_NAME,
        "split": DATASET_SPLIT,
        "revision": revision,
        "instance_count": instance_count,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2) + "\n")
