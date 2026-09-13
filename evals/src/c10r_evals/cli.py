"""Command-line entry point: `c10r-evals generate|import|analyze`."""

import argparse
import logging
import os
import sys
from collections.abc import Sequence
from pathlib import Path

from c10r_evals.run import ARMS, DEFAULT_BLOCK_SIZE
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
    generate.add_argument(
        "--force-split",
        action="store_true",
        help="Overwrite an existing split manifest that the new draw contradicts. Discards the comparability "
        "of every trial already recorded under it.",
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
        "--model",
        default=None,
        help="Only include trials run against this model. Required once the store holds more than one.",
    )
    analyze.add_argument("--out", type=Path, default=Path("report.md"), help="Report output path.")
    analyze.add_argument(
        "--price-schedule",
        type=Path,
        default=None,
        help="Named, dated price-schedule JSON for deriving per-trajectory financial cost. Optional; "
        "the report marks financial cost unavailable when omitted or when a required token category "
        "is unrecorded.",
    )
    analyze.add_argument(
        "--plots-dir",
        type=Path,
        default=None,
        help="Directory for the four registered plots; defaults to the report's own directory.",
    )
    analyze.set_defaults(func=_cmd_analyze)

    run = subparsers.add_parser(
        "run",
        help="Run both arms until every cell holds the target number of scorable attempts.",
    )
    run.add_argument("subset", choices=("dev", "frozen"), help="Which subset the named config must run.")
    run.add_argument("--cfg", required=True, help="Config directory under configs/, e.g. dev-recover.")
    run.add_argument(
        "--arm",
        choices=ARMS,
        default=None,
        help="Run one arm only. Both run by default, baseline first.",
    )
    run.add_argument(
        "--attempts",
        type=int,
        default=1,
        help="Target scorable attempts per cell. Cells already at the target run nothing. Default 1.",
    )
    run.add_argument(
        "--criteria", type=Path, default=Path("criteria.json"), help="Decision criteria, for the frozen attempt bound."
    )
    run.add_argument(
        "--over-attempts",
        action="store_true",
        help="Allow more attempts per cell than the frozen run is pre-registered for.",
    )
    run.add_argument(
        "--block-size",
        type=int,
        default=DEFAULT_BLOCK_SIZE,
        help="Instances per alternating block. Smaller blocks pair the arms more closely in time "
        "and cost more Pier invocations.",
    )
    run.add_argument("--dry-run", action="store_true", help="Print the Pier invocations without running them.")
    run.set_defaults(func=_cmd_run)

    build_manifest = subparsers.add_parser(
        "build-manifest",
        help="Merge per-instance image build records into the index build manifest, which records each "
        "task's one-time index build cost separately from the per-episode treatment measures.",
    )
    build_manifest.add_argument("manifest", type=Path, help="Build manifest to merge into; created if absent.")
    build_manifest.add_argument("records", type=Path, help="Newline-delimited build records from an image build.")
    build_manifest.set_defaults(func=_cmd_build_manifest)

    pending = subparsers.add_parser(
        "pending",
        help="List the instances of one arm still owed a scorable attempt, for resuming an interrupted sweep.",
    )
    pending.add_argument("config", type=Path, help="Arm config, e.g. configs/dev/baseline.yaml.")
    pending.add_argument(
        "--attempts",
        type=int,
        default=1,
        help="Target scorable attempts per instance; an instance below it is pending. Default 1, one full pass.",
    )
    pending.add_argument(
        "--dataset-path",
        action="store_true",
        help="Print the arm's task tree instead of the pending instances; Pier needs it to accept filters.",
    )
    pending.set_defaults(func=_cmd_pending)

    return parser


def _not_implemented(args: argparse.Namespace) -> int:
    print(f"c10r-evals {args.command}: not implemented yet", file=sys.stderr)
    return 2


def _cmd_generate(args: argparse.Namespace) -> int:
    from c10r_evals.dataset import TreeRevisionMismatch, load_instances, read_tree_revision, write_dataset_manifest
    from c10r_evals.repotree import clone_cache, tracked_python_paths
    from c10r_evals.split import make_split, read_split_manifest, split_conflicts, write_split_manifest
    from c10r_evals.taskgen import generate_tasks

    manifest_path = args.out / "split-manifest.json"
    dataset_path = args.out / "dataset-manifest.json"
    if not args.force_split and manifest_path.is_file() and dataset_path.is_file():
        try:
            read_tree_revision(dataset_path)
        except TreeRevisionMismatch as mismatch:
            print(
                f"c10r-evals generate: {args.out} is half-written — {mismatch}. A previous run "
                "stopped between the two writes. Re-run with --force-split to rewrite both.",
                file=sys.stderr,
            )
            return 2

    instances = load_instances(args.revision)
    split = make_split([i.instance_id for i in instances], args.seed)

    if manifest_path.is_file() and not args.force_split:
        existing, existing_revision = read_split_manifest(manifest_path)
        conflicts = split_conflicts(existing, existing_revision, split, args.revision)
        if conflicts:
            print(
                f"c10r-evals generate: this draw contradicts {manifest_path}:\n"
                + "\n".join(f"  - {c}" for c in conflicts)
                + "\nTrials already recorded were run under the split on disk. Re-run with --force-split "
                "only if you intend to discard them.",
                file=sys.stderr,
            )
            return 2

    write_dataset_manifest(dataset_path, args.revision, len(instances))
    write_split_manifest(manifest_path, split, args.revision)

    chosen_ids = set(split.dev if args.subset == "dev" else split.frozen)
    # The dataset carries more than one row for some instance ids. Each id is one episode, so
    # the first row wins; without this the rows overwrite each other's task directory and the
    # last one silently decides that episode's gold files.
    by_id = {}
    repeated = []
    for instance in instances:
        if instance.instance_id not in chosen_ids:
            continue
        if instance.instance_id in by_id:
            repeated.append(instance.instance_id)
            continue
        by_id[instance.instance_id] = instance

    chosen = list(by_id.values())
    # One clone per distinct repository: SWE-bench-Live instances repeat repositories, and one
    # cache directory serves the whole run, so this is one network fetch per repository, not per
    # instance. The clones are discarded when the run ends unless the cache is set to persist.
    with clone_cache() as cache_dir:
        tracked_paths_by_instance = {
            instance.instance_id: tracked_python_paths(instance.repo, instance.base_commit, cache_dir)
            for instance in chosen
        }
    task_dirs = generate_tasks(
        chosen, args.out / args.subset, args.revision, tracked_paths_by_instance=tracked_paths_by_instance
    )
    if repeated:
        log_fields(
            logger,
            logging.WARNING,
            "duplicate_dataset_rows_ignored",
            subset=args.subset,
            instance_ids=sorted(set(repeated)),
        )
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


def _cmd_pending(args: argparse.Namespace) -> int:
    """Instance names go to stdout so a shell can consume them; counts go to stderr."""
    from c10r_evals.resume import ArmConfigError, arm_status, dataset_path

    try:
        if args.dataset_path:
            print(dataset_path(args.config))
            return 0
        status = arm_status(args.config, args.attempts)
    except (ArmConfigError, ValueError) as exc:
        print(f"c10r-evals pending: {exc}", file=sys.stderr)
        return 2
    for instance in status.pending:
        print(instance)
    print(
        f"{len(status.pending)} of {status.planned} instance(s) pending for {args.config} "
        f"at {status.attempts} attempt(s) each; {status.owed} attempt(s) owed; "
        f"attempts per instance {status.distribution}",
        file=sys.stderr,
    )
    if status.above_target:
        held = sorted(set(status.above_target.values()))
        print(
            f"warning: {len(status.above_target)} of {status.planned} instance(s) already hold "
            f"{', '.join(str(h) for h in held)} attempt(s), more than the {status.attempts} asked for. "
            "Nothing runs for them and nothing is discarded. Check you named the config you meant.",
            file=sys.stderr,
        )
    return 0


def _cmd_run(args: argparse.Namespace) -> int:
    from c10r_evals.criteria import Criteria
    from c10r_evals.resume import ArmConfigError
    from c10r_evals.run import (
        RunRefused,
        arm_config,
        check_attempts,
        check_credentials,
        check_subset,
        execute,
        plan,
        render_plan,
    )

    config_dir = Path("configs") / args.cfg
    arms = (args.arm,) if args.arm else ARMS
    try:
        if args.attempts < 1:
            raise RunRefused(f"--attempts must be at least 1, got {args.attempts}")
        if not args.dry_run:
            check_credentials()
        planned_attempts = Criteria.load(args.criteria).attempts_per_cell if args.criteria.is_file() else None
        if not args.over_attempts:
            check_attempts(args.subset, args.attempts, planned_attempts)
        for arm in arms:
            check_subset(arm_config(config_dir, arm), args.subset)
        groups = plan(config_dir, arms, args.attempts, block_size=args.block_size)
    except (RunRefused, ArmConfigError) as exc:
        print(f"c10r-evals run: {exc}", file=sys.stderr)
        return 2

    if not groups:
        print(
            f"nothing to run: every instance of {', '.join(arms)} in {config_dir} already has "
            f"{args.attempts} scorable attempt(s). Raise --attempts to add more, or run "
            f"`c10r-evals analyze` to report what is there.",
            file=sys.stderr,
        )
        return 0

    print(render_plan(groups))
    if args.dry_run:
        return 0
    return execute(groups)


def _cmd_build_manifest(args: argparse.Namespace) -> int:
    from c10r_evals.armtree import merge_build_manifest, read_build_records

    records = read_build_records(args.records)
    merged = merge_build_manifest(args.manifest, records)
    log_fields(
        logger,
        logging.INFO,
        "build_manifest_merged",
        manifest=str(args.manifest),
        merged=len(records),
        total=len(merged),
    )
    return 0


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
    import json

    from c10r_evals.analysis import NoPairedInstances, PriceSchedule, analyze, trajectory_level_output
    from c10r_evals.criteria import Criteria
    from c10r_evals.plots import render_plots
    from c10r_evals.report import render_report
    from c10r_evals.split import read_split_manifest
    from c10r_evals.telemetry import query_trials

    criteria = Criteria.load(args.criteria)
    tracking_uri = resolve_tracking_uri(args.tracking_uri)
    price_schedule = PriceSchedule.load(args.price_schedule) if args.price_schedule is not None else None

    trials = query_trials(tracking_uri)
    if args.model is not None:
        trials = [t for t in trials if t["params"].get("model") == args.model]
    if args.instruction_version is not None:
        trials = [
            t for t in trials if t["arm"] != "treatment" or t["instruction_set_version"] == args.instruction_version
        ]

    frozen_ids = None
    if args.split_manifest is not None:
        split, _ = read_split_manifest(args.split_manifest)
        frozen_ids = set(split.frozen)

    try:
        result = analyze(trials, criteria, frozen_ids=frozen_ids, price_schedule=price_schedule)
    except NoPairedInstances as empty:
        print(f"c10r-evals analyze: {empty}", file=sys.stderr)
        return 2
    args.out.parent.mkdir(parents=True, exist_ok=True)
    # Render plots first so the report cannot link to missing files.
    plots_dir = args.plots_dir if args.plots_dir is not None else args.out.parent
    plot_paths = render_plots(result, plots_dir)
    plot_links = {name: Path(os.path.relpath(path, args.out.parent)).as_posix() for name, path in plot_paths.items()}
    args.out.write_text(render_report(result, plot_links))
    trajectory_path = args.out.with_suffix(".trajectories.json")
    trajectory_path.write_text(json.dumps(trajectory_level_output(result), indent=2) + "\n")
    log_fields(
        logger,
        logging.INFO,
        "analysis_complete",
        pairs=len(result.pairs),
        exclusions=len(result.exclusions),
        accuracy_lambda=result.accuracy.lambda_,
        token_use_ratio=result.token_use.ratio,
        report=str(args.out),
        trajectory_output=str(trajectory_path),
        plots=[str(p) for p in plot_paths.values()],
    )
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    setup_process(args.command)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
