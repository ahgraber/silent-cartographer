"""Render both arms' Pier job configs from the environment (`just render`).

Endpoint, model, and caps come from the direnv-loaded `.env`; the dataset revision
comes from the generated `dataset-manifest.json`, so a rendered config cannot name
a revision the task trees were not generated from.
Run inside the project environment (`uv run python scripts/render-arm-config.py`),
not as a standalone script — it imports the `c10r_evals` package.
"""

import argparse
import json
import logging
import sys
from collections.abc import Sequence
from pathlib import Path

from c10r_evals.armconfig import base_from_env, render_arm_configs
from c10r_evals.runtime import log_fields, setup_process

logger = logging.getLogger(__name__)


def read_revision(manifest_path: Path) -> str:
    """Read the pinned dataset revision recorded when the tasks were generated."""
    if not manifest_path.is_file():
        raise SystemExit(f"{manifest_path} not found — generate the tasks first (`just generate`)")
    revision = json.loads(manifest_path.read_text()).get("revision")
    if not revision:
        raise SystemExit(f"{manifest_path} records no 'revision'")
    return str(revision)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="render-arm-config", description=__doc__)
    parser.add_argument("--prompt", type=Path, required=True, help="Instruction-set file; its stem is the version.")
    parser.add_argument("--out", type=Path, required=True, help="Config output directory (one per prompt version).")
    parser.add_argument("--tasks-root", type=Path, required=True, help="Subset root holding the two arm trees.")
    parser.add_argument("--platform", required=True, help="Image platform the sweep runs on, e.g. linux/arm64.")
    parser.add_argument(
        "--dataset-manifest",
        type=Path,
        default=Path("tasks/dataset-manifest.json"),
        help="Dataset manifest the pinned revision is read from.",
    )
    args = parser.parse_args(argv)
    setup_process("render-arm-config")

    paths = render_arm_configs(
        base_from_env(),
        args.prompt,
        args.out,
        tasks_root=args.tasks_root,
        dataset_revision=read_revision(args.dataset_manifest),
        platform=args.platform,
    )
    log_fields(
        logger,
        logging.INFO,
        "arm_configs_rendered",
        baseline=str(paths["baseline"]),
        treatment=str(paths["treatment"]),
        platform=args.platform,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
