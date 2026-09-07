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
