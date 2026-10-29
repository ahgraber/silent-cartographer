"""Builder config translation: buildkitd.toml settings become buildkitd flags with the same meaning."""

import tomllib
from pathlib import Path

import pytest

from c10r_evals.buildkitd import buildkitd_flags, size_to_mb

CONFIG_PATH = Path(__file__).parent.parent / "buildkitd.toml"


def test_size_uses_buildkitd_binary_units_and_decimal_flag_megabytes():
    """buildkitd reads "60GB" as 60 GiB; the flag takes whole decimal megabytes."""
    assert size_to_mb("60GB") == 64424
    assert size_to_mb("512mib") == 536
    assert size_to_mb("1.5g") == 1610


def test_size_rejects_what_the_flag_cannot_express():
    for text in ("10%", "sixty", ""):
        with pytest.raises(ValueError):
            size_to_mb(text)


def test_repo_config_translates_every_setting():
    flags = buildkitd_flags(tomllib.loads(CONFIG_PATH.read_text()))
    assert flags == [
        "--oci-worker-gc=true",
        "--oci-worker-gc-keepstorage=64424",
        "--oci-worker-labels=c10r-cache-cap=60GB",
    ]


def test_untranslated_key_fails_rather_than_being_dropped():
    config = {"worker": {"oci": {"gc": True, "gcpolicy": [{"keepBytes": "1GB"}]}}, "debug": True}
    with pytest.raises(ValueError, match="debug.*worker.oci.gcpolicy"):
        buildkitd_flags(config)
