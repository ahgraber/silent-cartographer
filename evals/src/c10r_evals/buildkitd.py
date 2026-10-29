"""Translate the builder's `buildkitd.toml` into buildkitd command-line flags (`just builder`).

`docker buildx create --config` copies the file into the builder container before starting it,
and socktainer, the Docker API server for Apple's container runtime, refuses that copy. The flags
carry the same settings as container arguments, so the TOML file stays the place to edit them.
Usage: `python -m c10r_evals.buildkitd buildkitd.toml` prints the flags, shell-quoted.
"""

import re
import shlex
import sys
import tomllib
from pathlib import Path
from typing import Any

from setproctitle import setproctitle

from c10r_evals.runtime import PROCESS_NAME

# buildkitd parses sizes with docker/go-units RAMInBytes: every unit is 1024-based.
_UNIT_BYTES = {"": 1, "k": 1024, "m": 1024**2, "g": 1024**3, "t": 1024**4, "p": 1024**5}
_SIZE = re.compile(r"(\d+(?:\.\d+)?)\s*([kmgtp]?)(?:i?b)?", re.IGNORECASE)
_OCI_KEYS = {"gc", "gckeepstorage", "labels"}


def size_to_mb(text: str) -> int:
    """A buildkitd size such as "60GB" in the whole decimal megabytes the gc flag takes.

    Raises `ValueError` for a percentage or any text buildkitd would not read as a size.
    """
    match = _SIZE.fullmatch(text.strip())
    if not match:
        raise ValueError(f"{text!r} is not a size the gc flag can express")
    return int(float(match[1]) * _UNIT_BYTES[match[2].lower()]) // 1_000_000


def buildkitd_flags(config: dict[str, Any]) -> list[str]:
    """The buildkitd flags equivalent to a parsed `buildkitd.toml`.

    Raises `ValueError` naming every key with no translation, so no setting is silently dropped.
    """
    worker = config.get("worker", {})
    oci = worker.get("oci", {})
    unknown = (
        [key for key in config if key != "worker"]
        + [f"worker.{key}" for key in worker if key != "oci"]
        + [f"worker.oci.{key}" for key in oci if key not in _OCI_KEYS]
    )
    if unknown:
        raise ValueError(f"no buildkitd flag translation for: {', '.join(sorted(unknown))}")

    flags = []
    if "gc" in oci:
        flags.append(f"--oci-worker-gc={str(oci['gc']).lower()}")
    if "gckeepstorage" in oci:
        # One value sets reserved space, the field the deprecated `gckeepstorage` key sets.
        flags.append(f"--oci-worker-gc-keepstorage={size_to_mb(oci['gckeepstorage'])}")
    flags.extend(f"--oci-worker-labels={key}={value}" for key, value in oci.get("labels", {}).items())
    return flags


def main() -> None:
    setproctitle(f"{PROCESS_NAME} buildkitd-flags")
    try:
        flags = buildkitd_flags(tomllib.loads(Path(sys.argv[1]).read_text()))
    except (OSError, tomllib.TOMLDecodeError, ValueError) as exc:
        raise SystemExit(f"buildkitd config: {exc}") from exc
    print(shlex.join(flags))


if __name__ == "__main__":
    main()
