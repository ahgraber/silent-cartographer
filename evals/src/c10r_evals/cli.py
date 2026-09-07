"""Command-line entry point: `c10r-evals generate|import|analyze`."""

import argparse
import logging
import sys
from collections.abc import Sequence
from pathlib import Path

from c10r_evals.runtime import log_fields, setup_process

logger = logging.getLogger(__name__)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="c10r-evals",
        description="c10r evaluation harness: generate localization tasks, import trial telemetry, run paired analysis.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    generate = subparsers.add_parser("generate", help="Generate Harbor-format localization tasks from the dataset.")
    generate.add_argument("--revision", required=True, help="Pinned SWE-bench-Live dataset revision.")
    generate.add_argument(
        "--out", type=Path, default=Path("tasks"), help="Output directory for manifests and task trees."
    )
    generate.add_argument("--seed", type=int, default=0, help="Seed for the dev/frozen split.")
    generate.add_argument(
        "--subset", choices=("dev", "frozen"), default="dev", help="Which subset to generate tasks for."
    )
    generate.set_defaults(func=_cmd_generate)

    import_ = subparsers.add_parser("import", help="Import Pier trial outputs into the MLflow experiment store.")
    import_.add_argument("sweeps", type=Path, nargs="+", help="Sweep directories (sweep-meta.json + jobs/ tree).")
    import_.add_argument(
        "--tracking-uri",
        default=None,
        help="MLflow tracking URI; defaults to MLFLOW_TRACKING_URI or sqlite:///mlflow.db.",
    )
    import_.set_defaults(func=_cmd_import)

    analyze = subparsers.add_parser("analyze", help="Run the paired analysis and render the report.")
    analyze.add_argument("--criteria", type=Path, required=True, help="Decision-criteria JSON (required input).")
    analyze.add_argument(
        "--tracking-uri",
        default=None,
        help="MLflow tracking URI; defaults to MLFLOW_TRACKING_URI or sqlite:///mlflow.db.",
    )
    analyze.add_argument(
        "--split-manifest", type=Path, default=None, help="Split manifest; restricts the report to the frozen subset."
    )
    analyze.add_argument(
        "--instruction-version", default=None, help="Only include treatment trials of this instruction-set version."
    )
    analyze.add_argument(
        "--build-manifest", type=Path, default=None, help="Index build manifest for the amortization break-even."
    )
    analyze.add_argument("--out", type=Path, default=Path("report.md"), help="Report output path.")
    analyze.set_defaults(func=_cmd_analyze)

    return parser


def _not_implemented(args: argparse.Namespace) -> int:
    print(f"c10r-evals {args.command}: not implemented yet", file=sys.stderr)
    return 2


def _cmd_generate(args: argparse.Namespace) -> int:
    from c10r_evals.dataset import load_instances, write_dataset_manifest
    from c10r_evals.split import make_split, write_split_manifest
    from c10r_evals.taskgen import generate_tasks

    instances = load_instances(args.revision)
    write_dataset_manifest(args.out / "dataset-manifest.json", args.revision, len(instances))

    split = make_split([i.instance_id for i in instances], args.seed)
    write_split_manifest(args.out / "split-manifest.json", split, args.revision)

    chosen_ids = set(split.dev if args.subset == "dev" else split.frozen)
    chosen = [i for i in instances if i.instance_id in chosen_ids]
    task_dirs = generate_tasks(chosen, args.out / args.subset, args.revision)
    log_fields(
        logger,
        logging.INFO,
        "tasks_generated",
        subset=args.subset,
        instances=len(chosen),
        tasks=len(task_dirs),
        out=str(args.out / args.subset),
    )
    return 0


def resolve_tracking_uri(explicit: str | None) -> str:
    import os

    return explicit or os.environ.get("MLFLOW_TRACKING_URI") or "sqlite:///mlflow.db"


def _cmd_import(args: argparse.Namespace) -> int:
    from c10r_evals.telemetry import import_sweep

    tracking_uri = resolve_tracking_uri(args.tracking_uri)
    for sweep in args.sweeps:
        summary = import_sweep(sweep, tracking_uri)
        log_fields(
            logger,
            logging.INFO,
            "import_complete",
            sweep=str(sweep),
            imported=summary.imported,
            skipped=summary.skipped,
        )
    return 0


def _cmd_analyze(args: argparse.Namespace) -> int:
    from c10r_evals.analysis import Criteria, analyze
    from c10r_evals.armtree import read_build_manifest
    from c10r_evals.report import render_report
    from c10r_evals.split import read_split_manifest
    from c10r_evals.telemetry import query_trials

    criteria = Criteria.load(args.criteria)
    tracking_uri = resolve_tracking_uri(args.tracking_uri)

    trials = query_trials(tracking_uri)
    if args.instruction_version is not None:
        trials = [
            t for t in trials if t["arm"] != "treatment" or t["instruction_set_version"] == args.instruction_version
        ]

    frozen_ids = None
    if args.split_manifest is not None:
        split, _ = read_split_manifest(args.split_manifest)
        frozen_ids = set(split.frozen)

    build_entries = read_build_manifest(args.build_manifest) if args.build_manifest else None
    result = analyze(trials, criteria, frozen_ids=frozen_ids, build_manifest_entries=build_entries)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(render_report(result))
    log_fields(
        logger,
        logging.INFO,
        "analysis_complete",
        pairs=len(result.pairs),
        exclusions=len(result.exclusions),
        cost_outcome=result.cost.outcome,
        accuracy_outcome=result.accuracy.outcome,
        report=str(args.out),
    )
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    setup_process(args.command)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
