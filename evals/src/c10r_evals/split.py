"""Seeded, disjoint dev/frozen instance split and its manifest."""

import json
import random
from dataclasses import dataclass
from pathlib import Path

DEV_SIZE = 20
FROZEN_SIZE = 100


@dataclass(frozen=True)
class Split:
    dev: list[str]
    frozen: list[str]
    seed: int


def make_split(
    instance_ids: list[str],
    seed: int,
    dev_size: int = DEV_SIZE,
    frozen_size: int = FROZEN_SIZE,
) -> Split:
    """Draw disjoint dev and frozen subsets deterministically from the sorted instance ids."""
    ids = sorted(instance_ids)
    if dev_size + frozen_size > len(ids):
        raise ValueError(f"split needs {dev_size + frozen_size} instances, dataset has {len(ids)}")
    sample = random.Random(seed).sample(ids, dev_size + frozen_size)
    return Split(dev=sorted(sample[:dev_size]), frozen=sorted(sample[dev_size:]), seed=seed)


def write_split_manifest(path: Path, split: Split, dataset_revision: str) -> None:
    manifest = {
        "seed": split.seed,
        "dataset_revision": dataset_revision,
        "dev": split.dev,
        "frozen": split.frozen,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2) + "\n")


def read_split_manifest(path: Path) -> tuple[Split, str]:
    manifest = json.loads(path.read_text())
    split = Split(dev=manifest["dev"], frozen=manifest["frozen"], seed=manifest["seed"])
    return split, manifest["dataset_revision"]
