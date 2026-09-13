"""Seeded, disjoint dev/frozen instance split and its manifest."""

import json
import random
from dataclasses import dataclass
from pathlib import Path

DEV_SIZE = 20
# Registered frozen-pair count from docs/experiment-design.md.
FROZEN_SIZE = 300


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
    """Return deterministic disjoint subsets. Growth retains members; too few unique IDs raise `ValueError`."""
    ids = sorted(instance_ids)
    wanted = dev_size + frozen_size
    distinct = len(set(ids))
    if wanted > distinct:
        raise ValueError(f"split needs {wanted} distinct instances, dataset has {distinct}")
    size = wanted
    drawn = list(dict.fromkeys(random.Random(seed).sample(ids, size)))
    while len(drawn) < wanted:
        size = min(size + (wanted - len(drawn)), len(ids))
        drawn = list(dict.fromkeys(random.Random(seed).sample(ids, size)))
    return Split(dev=sorted(drawn[:dev_size]), frozen=sorted(drawn[dev_size:wanted]), seed=seed)


def write_split_manifest(path: Path, split: Split, dataset_revision: str) -> None:
    manifest = {
        "seed": split.seed,
        "dataset_revision": dataset_revision,
        "dev": split.dev,
        "frozen": split.frozen,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2) + "\n")


def split_conflicts(existing: Split, existing_revision: str, drawn: Split, revision: str) -> list[str]:
    """Return changes that conflict with an existing split. Subset growth is allowed."""
    conflicts = []
    if existing_revision != revision:
        conflicts.append(f"dataset revision changes from {existing_revision} to {revision}")
    for name, before, after in (("dev", existing.dev, drawn.dev), ("frozen", existing.frozen, drawn.frozen)):
        dropped = sorted(set(before) - set(after))
        if dropped:
            shown = ", ".join(dropped[:3])
            more = f" and {len(dropped) - 3} more" if len(dropped) > 3 else ""
            conflicts.append(f"{len(dropped)} instance(s) leave {name}: {shown}{more}")
    if existing.seed != drawn.seed:
        conflicts.append(f"split seed changes from {existing.seed} to {drawn.seed}")
    return conflicts


def read_split_manifest(path: Path) -> tuple[Split, str]:
    manifest = json.loads(path.read_text())
    split = Split(dev=manifest["dev"], frozen=manifest["frozen"], seed=manifest["seed"])
    return split, manifest["dataset_revision"]
