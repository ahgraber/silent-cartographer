"""Build every treatment task's image, timing each build and measuring the baked index.

Each task's image bakes a c10r index at build time, a one-time cost of preparing the
treatment arm rather than part of any episode's measures. Building one task's image can take
minutes and a whole tree can take hours, so each record is appended to the records file as
soon as it is measured, and the file is never truncated: a build interrupted partway keeps
what it finished, and the caller merges the records into the index build manifest afterward.
Merging is keyed by instance with the latest record for an instance winning, so re-measuring
an instance and merging again is safe.
"""

import hashlib
import json
import re
import subprocess
import time
from collections.abc import Callable
from pathlib import Path

_TAG_DISALLOWED = re.compile(r"[^a-z0-9._-]")


def image_tag(instance_id: str) -> str:
    """The Docker tag for one instance: lowercased, with disallowed characters folded to '-'."""
    return "c10r-eval-treatment-" + _TAG_DISALLOWED.sub("-", instance_id.lower())


def env_digest(task_dir: Path) -> str:
    """Digest of the task's `environment/` directory, which is all its image is built from."""
    digest = hashlib.sha256()
    for path in sorted((task_dir / "environment").rglob("*")):
        if path.is_file():
            digest.update(path.relative_to(task_dir).as_posix().encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def _image_exists(tag: str) -> bool:
    completed = subprocess.run(
        ["docker", "image", "inspect", tag],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.returncode == 0


def task_dirs(tree_dir: Path) -> list[Path]:
    """Immediate children of `tree_dir` that hold a task, in sorted order."""
    dirs = sorted(p for p in tree_dir.iterdir() if (p / "task.toml").is_file())
    if not dirs:
        raise ValueError(f"no task directories under {tree_dir} — generate and inject the tasks first")
    return dirs


def _build(task_dir: Path, tag: str, platform: str, builder: str) -> int:
    """Build the task's image, returning the wall-clock seconds. Raises on a failed build."""
    start = time.monotonic()
    # `docker build` ignores buildx's current selection, so name the builder.
    subprocess.run(
        [
            "docker",
            "buildx",
            "build",
            "--builder",
            builder,
            "--platform",
            platform,
            "--load",
            "-t",
            tag,
            str(task_dir / "environment"),
        ],
        stdout=subprocess.DEVNULL,
        check=True,
    )
    return int(time.monotonic() - start)


def _index_bytes(tag: str, platform: str) -> int:
    """The baked index size in bytes, or 0 when it cannot be measured or parsed."""
    completed = subprocess.run(
        [
            "docker",
            "run",
            "--rm",
            "--platform",
            platform,
            tag,
            "sh",
            "-c",
            "du -sb /repo/.c10r 2>/dev/null | cut -f1 || echo 0",
        ],
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    )
    try:
        return int(completed.stdout.strip())
    except ValueError:
        return 0


def build_images(
    tree_dir: Path,
    records_path: Path,
    platform: str,
    builder: str,
    on_built: Callable[[str], None] | None = None,
    built: dict[str, str] | None = None,
) -> list[str]:
    """Build every treatment image, appending one record per measured build. Returns the failed instance IDs.

    `built` maps an instance to the digest its image was built from; an instance whose digest
    matches and whose image is still present is skipped.
    """
    built = built or {}
    failed: list[str] = []
    with records_path.open("a") as records_file:
        for task_dir in task_dirs(tree_dir):
            instance = task_dir.name
            tag = image_tag(instance)
            digest = env_digest(task_dir)
            if built.get(instance) == digest and _image_exists(tag):
                print(f"==> {instance} already built")
                if on_built is not None:
                    on_built(instance)
                continue
            print(f"==> building {instance}")
            try:
                wall = _build(task_dir, tag, platform, builder)
            except subprocess.CalledProcessError:
                print(f"    FAILED: {instance} (recorded; continuing)")
                failed.append(instance)
                if on_built is not None:
                    on_built(instance)
                continue
            index_bytes = _index_bytes(tag, platform)
            record = {
                "instance_id": instance,
                "wall_clock_s": wall,
                "index_bytes": index_bytes,
                "env_digest": digest,
            }
            records_file.write(json.dumps(record) + "\n")
            records_file.flush()
            print(f"    {wall}s, index {index_bytes} bytes")
            if on_built is not None:
                on_built(instance)
    return failed
