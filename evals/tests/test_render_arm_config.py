"""`render-arm-config.py`'s manifest boundary: missing file, missing revision, disagreeing trees."""

import importlib.util
from pathlib import Path

import pytest

from c10r_evals.dataset import write_dataset_manifest
from c10r_evals.split import Split, write_split_manifest

SCRIPT_PATH = Path(__file__).parent.parent / "scripts" / "render-arm-config.py"


def _load_script():
    """`scripts/` is not a package and its filename is not a valid module name."""
    spec = importlib.util.spec_from_file_location("render_arm_config", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


render_arm_config = _load_script()


def write_manifests(out: Path, dataset_revision: str, split_revision: str) -> None:
    write_dataset_manifest(out / "dataset-manifest.json", dataset_revision, instance_count=2)
    write_split_manifest(out / "split-manifest.json", Split(dev=["a"], frozen=["b"], seed=0), split_revision)


def test_read_revision_returns_the_agreeing_revision(tmp_path):
    write_manifests(tmp_path, "rev-a", "rev-a")
    assert render_arm_config.read_revision(tmp_path / "dataset-manifest.json") == "rev-a"


def test_read_revision_refuses_a_missing_manifest(tmp_path):
    with pytest.raises(SystemExit, match="not found"):
        render_arm_config.read_revision(tmp_path / "dataset-manifest.json")


def test_read_revision_refuses_a_manifest_without_a_revision(tmp_path):
    manifest = tmp_path / "dataset-manifest.json"
    manifest.write_text("{}")
    with pytest.raises(SystemExit, match="records no 'revision'"):
        render_arm_config.read_revision(manifest)


def test_read_revision_refuses_disagreeing_manifests(tmp_path):
    write_manifests(tmp_path, "rev-b", "rev-a")
    with pytest.raises(SystemExit, match="half-written"):
        render_arm_config.read_revision(tmp_path / "dataset-manifest.json")
