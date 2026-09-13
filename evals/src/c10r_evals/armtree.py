"""Treatment-tree transformation: inject c10r into a generated treatment arm tree.

The generator emits both arm trees identical; this transformation is the only
difference between them on disk — it copies the static c10r binary into each
treatment task's build context and appends the install + index-build steps to its
environment Dockerfile.
The baseline tree is never touched.
"""

import json
import shutil
import tomllib
from pathlib import Path

SCIP_PYTHON_VERSION = "0.6.6"

INJECTION_MARKER = "# --- c10r treatment injection ---"

REPO_ANCHOR = "WORKDIR /repo\n"

# A layer's cache key includes its parent chain, so the toolchain goes above the clone to share
# one layer across tasks; below it each task stores its own copy.
TOOLING_BLOCK = f"""\
{INJECTION_MARKER}
COPY c10r /usr/local/bin/c10r
RUN chmod +x /usr/local/bin/c10r \\
    && apt-get update \\
    && apt-get install -y --no-install-recommends nodejs npm \\
    && rm -rf /var/lib/apt/lists/* \\
    && npm install -g @sourcegraph/scip-python@{SCIP_PYTHON_VERSION}
"""

# Indexing reads the checked-out repository, so it stays last.
INDEX_BLOCK = """\
RUN c10r build --language python
"""


def _task_arm(task_dir: Path) -> str:
    meta = tomllib.loads((task_dir / "task.toml").read_text())
    return meta["metadata"]["arm"]


def injected_dockerfile(content: str) -> str:
    """Return `content` with the toolchain above the clone and indexing at the end."""
    if REPO_ANCHOR not in content:
        raise ValueError(f"generated Dockerfile has no {REPO_ANCHOR.strip()!r} line to inject above")
    head, anchor, tail = content.partition(REPO_ANCHOR)
    return head + TOOLING_BLOCK + anchor + tail.rstrip("\n") + "\n" + INDEX_BLOCK


def inject_c10r(tree_dir: Path, binary_path: Path) -> list[Path]:
    """Inject c10r into every task of a treatment tree; refuse non-treatment tasks."""
    if not binary_path.is_file():
        raise FileNotFoundError(f"c10r binary not found: {binary_path}")
    task_dirs = sorted(p for p in tree_dir.iterdir() if (p / "task.toml").is_file())
    if not task_dirs:
        raise ValueError(f"no task directories under {tree_dir}")
    for task_dir in task_dirs:
        arm = _task_arm(task_dir)
        if arm != "treatment":
            raise ValueError(f"refusing to inject c10r into arm '{arm}' task: {task_dir}")

    source_bytes = binary_path.read_bytes()
    injected: list[Path] = []
    for task_dir in task_dirs:
        dockerfile = task_dir / "environment" / "Dockerfile"
        binary_dest = task_dir / "environment" / "c10r"
        content = dockerfile.read_text()
        if INJECTION_MARKER in content:
            # Already injected: refresh the binary if it went stale, never duplicate the block.
            if not binary_dest.exists() or binary_dest.read_bytes() != source_bytes:
                shutil.copy2(binary_path, binary_dest)
                injected.append(task_dir)
            continue
        shutil.copy2(binary_path, binary_dest)
        dockerfile.write_text(injected_dockerfile(content))
        injected.append(task_dir)
    return injected


BUILD_MANIFEST_KEYS = ("instance_id", "wall_clock_s", "index_bytes")


def _check_entries(entries: list[dict]) -> None:
    for entry in entries:
        for key in BUILD_MANIFEST_KEYS:
            if key not in entry:
                raise ValueError(f"build manifest entry missing '{key}': {entry}")


def write_build_manifest(path: Path, entries: list[dict]) -> None:
    """Record per-task index build cost: wall-clock seconds and index size in bytes.

    The treatment image bakes its index at build time, so this cost is a one-time cost of
    preparing the treatment arm, not part of any episode's measures; the manifest keeps it
    recorded separately, for reporting on its own.
    """
    _check_entries(entries)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"entries": entries}, indent=2) + "\n")


def read_build_manifest(path: Path) -> list[dict]:
    return json.loads(path.read_text())["entries"]


def read_build_records(path: Path) -> list[dict]:
    """Read newline-delimited build records, the form `build-treatment-images.sh` appends."""
    records = []
    for line in path.read_text().splitlines():
        if line.strip():
            records.append(json.loads(line))
    return records


def merge_build_manifest(path: Path, entries: list[dict]) -> list[dict]:
    """Merge records by instance ID, replacing matches and preserving unmatched existing entries."""
    _check_entries(entries)
    existing: list[dict] = []
    if path.is_file() and path.read_text().strip():
        existing = read_build_manifest(path)
    by_id = {entry["instance_id"]: entry for entry in existing}
    for entry in entries:
        by_id[entry["instance_id"]] = entry
    merged = [by_id[instance_id] for instance_id in sorted(by_id)]
    write_build_manifest(path, merged)
    return merged
