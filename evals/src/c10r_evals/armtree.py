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

INJECTION_BLOCK = f"""\
{INJECTION_MARKER}
COPY c10r /usr/local/bin/c10r
RUN chmod +x /usr/local/bin/c10r \\
    && apt-get update \\
    && apt-get install -y --no-install-recommends nodejs npm \\
    && rm -rf /var/lib/apt/lists/* \\
    && npm install -g @sourcegraph/scip-python@{SCIP_PYTHON_VERSION}
RUN c10r build --language python
"""


def _task_arm(task_dir: Path) -> str:
    meta = tomllib.loads((task_dir / "task.toml").read_text())
    return meta["metadata"]["arm"]


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
        dockerfile.write_text(content.rstrip("\n") + "\n" + INJECTION_BLOCK)
        injected.append(task_dir)
    return injected


def write_build_manifest(path: Path, entries: list[dict]) -> None:
    """Record per-task index build cost: wall-clock seconds and index size in bytes.

    Analysis reads this manifest to derive the index amortization break-even.
    """
    for entry in entries:
        for key in ("instance_id", "wall_clock_s", "index_bytes"):
            if key not in entry:
                raise ValueError(f"build manifest entry missing '{key}': {entry}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"entries": entries}, indent=2) + "\n")


def read_build_manifest(path: Path) -> list[dict]:
    return json.loads(path.read_text())["entries"]
