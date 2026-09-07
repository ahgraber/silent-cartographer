"""Seeded split behavior and manifest contents."""

import json

from c10r_evals.split import make_split, read_split_manifest, write_split_manifest


def test_split_manifest_seeded_disjoint(tmp_path):
    ids = [f"inst-{i:03d}" for i in range(500)]

    first = make_split(ids, seed=42)
    second = make_split(list(reversed(ids)), seed=42)
    assert first == second
    assert len(first.dev) == 20
    assert len(first.frozen) == 100
    assert not set(first.dev) & set(first.frozen)

    manifest_path = tmp_path / "split.json"
    write_split_manifest(manifest_path, first, dataset_revision="rev-abc")
    manifest = json.loads(manifest_path.read_text())
    assert manifest["seed"] == 42
    assert manifest["dataset_revision"] == "rev-abc"

    round_tripped, revision = read_split_manifest(manifest_path)
    assert round_tripped == first
    assert revision == "rev-abc"
