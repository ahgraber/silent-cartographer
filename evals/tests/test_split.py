"""Seeded split behavior and manifest contents."""

import json

from c10r_evals.split import (
    DEV_SIZE,
    FROZEN_SIZE,
    Split,
    make_split,
    read_split_manifest,
    split_conflicts,
    write_split_manifest,
)


def test_split_manifest_seeded_disjoint(tmp_path):
    ids = [f"inst-{i:03d}" for i in range(500)]

    first = make_split(ids, seed=42)
    second = make_split(list(reversed(ids)), seed=42)
    assert first == second
    assert len(first.dev) == DEV_SIZE
    assert len(first.frozen) == FROZEN_SIZE
    assert not set(first.dev) & set(first.frozen)

    manifest_path = tmp_path / "split.json"
    write_split_manifest(manifest_path, first, dataset_revision="rev-abc")
    manifest = json.loads(manifest_path.read_text())
    assert manifest["seed"] == 42
    assert manifest["dataset_revision"] == "rev-abc"

    round_tripped, revision = read_split_manifest(manifest_path)
    assert round_tripped == first
    assert revision == "rev-abc"


def test_a_repeated_dataset_id_does_not_shrink_a_subset():
    """The dataset repeats an instance id; a repeat is one episode, and the sizes must still hold.

    Topping the draw up must extend it, not redraw it: instances already run keep their subset.
    """
    clean = [f"inst-{i:03d}" for i in range(500)]
    repeated = list(clean)
    repeated[7] = repeated[300]  # the same id present twice, as the upstream dataset has it

    split = make_split(repeated, seed=0, dev_size=20, frozen_size=200)

    assert len(split.dev) == len(set(split.dev)) == 20
    assert len(split.frozen) == len(set(split.frozen)) == 200
    assert not set(split.dev) & set(split.frozen)

    without_topping_up = make_split(repeated, seed=0, dev_size=20, frozen_size=199)
    assert without_topping_up.dev == split.dev
    assert set(without_topping_up.frozen) < set(split.frozen)


def test_an_identical_or_extended_split_is_not_a_conflict():
    """Topping the run up to its planned size keeps every instance already run in its subset."""
    ids = [f"inst-{i:03d}" for i in range(500)]
    on_disk = make_split(ids, seed=0, frozen_size=199)

    assert split_conflicts(on_disk, "rev-a", on_disk, "rev-a") == []
    assert split_conflicts(on_disk, "rev-a", make_split(ids, seed=0, frozen_size=200), "rev-a") == []


def test_a_redraw_a_shrink_or_a_revision_move_conflicts():
    ids = [f"inst-{i:03d}" for i in range(500)]
    on_disk = make_split(ids, seed=0, frozen_size=200)

    redrawn = make_split(ids, seed=1, frozen_size=200)
    conflicts = split_conflicts(on_disk, "rev-a", redrawn, "rev-a")
    assert any("leave frozen" in c for c in conflicts)
    assert any("seed changes from 0 to 1" in c for c in conflicts)

    shrunk = make_split(ids, seed=0, frozen_size=150)
    assert any("leave frozen" in c for c in split_conflicts(on_disk, "rev-a", shrunk, "rev-a"))

    moved = split_conflicts(on_disk, "rev-a", on_disk, "rev-b")
    assert moved == ["dataset revision changes from rev-a to rev-b"]


def test_an_instance_crossing_between_subsets_conflicts():
    """A dev instance reappearing in frozen would score an instance the prompt was tuned on."""
    on_disk = Split(dev=["a", "b"], frozen=["c", "d"], seed=0)
    crossed = Split(dev=["a", "c"], frozen=["b", "d"], seed=0)

    conflicts = split_conflicts(on_disk, "rev-a", crossed, "rev-a")

    assert any("leave dev: b" in c for c in conflicts)
    assert any("leave frozen: c" in c for c in conflicts)


def test_dev_growing_does_not_excuse_frozen_shrinking():
    on_disk = Split(dev=["a", "b"], frozen=["c", "d"], seed=0)
    lopsided = Split(dev=["a", "b", "e"], frozen=["c"], seed=0)

    assert split_conflicts(on_disk, "rev-a", lopsided, "rev-a") == ["1 instance(s) leave frozen: d"]


def test_an_instance_dropping_out_of_the_dataset_conflicts():
    """An id withdrawn upstream leaves the subset it was already run under."""
    on_disk = Split(dev=["a", "b"], frozen=["c", "d"], seed=0)
    without_d = Split(dev=["a", "b"], frozen=["c", "e"], seed=0)

    assert split_conflicts(on_disk, "rev-a", without_d, "rev-a") == ["1 instance(s) leave frozen: d"]


def test_a_larger_frozen_size_extends_the_split_rather_than_redrawing_it():
    """Raising the frozen size must not move instances already run under the smaller one."""
    ids = [f"inst-{i:03d}" for i in range(500)]

    smaller = make_split(ids, seed=0, frozen_size=100)
    larger = make_split(ids, seed=0, frozen_size=200)

    assert larger.dev == smaller.dev
    assert set(smaller.frozen) < set(larger.frozen)
    assert not set(larger.dev) & set(larger.frozen)
