"""CLI skeleton behavior: subcommands exist, process title and structured logging are configured."""

import json
import logging

import pier_fixtures as fx
import pytest
import setproctitle

from c10r_evals import dataset, repotree, taskgen
from c10r_evals.cli import build_parser, main
from c10r_evals.dataset import Instance, write_dataset_manifest
from c10r_evals.runtime import JsonLineFormatter, log_fields, setup_process
from c10r_evals.split import FROZEN_SIZE, Split, make_split, write_split_manifest
from c10r_evals.telemetry import import_sweep


@pytest.mark.parametrize(
    ("argv", "handler"),
    [
        (["generate", "--revision", "rev"], "_cmd_generate"),
        (["run", "dev", "--cfg", "dev-recover"], "_cmd_run"),
        (["import", "sweep"], "_cmd_import"),
        (["analyze", "--criteria", "criteria.json"], "_cmd_analyze"),
        (["pending", "cfg.yaml"], "_cmd_pending"),
        (["build-manifest", "m.json", "r.jsonl"], "_cmd_build_manifest"),
    ],
)
def test_each_subcommand_parses_and_reaches_its_handler(argv, handler):
    assert build_parser().parse_args(argv).func.__name__ == handler


@pytest.fixture
def offline_generate(monkeypatch):
    """Stand in for the dataset fetch and the task writer, so `generate` runs without network or disk."""
    rows = [
        Instance(
            instance_id=f"inst-{i:03d}",
            repo="demo/repo",
            base_commit="c0ffee",
            problem_statement="p",
            patch="",
            test_patch="",
        )
        # The real split draws from a 500-instance dataset; the stand-in matches it so the
        # fixture does not have to grow with the frozen size.
        for i in range(500)
    ]
    monkeypatch.setattr(dataset, "load_instances", lambda revision, cache_dir=None: rows)
    monkeypatch.setattr(taskgen, "generate_tasks", lambda instances, out, revision, tracked_paths_by_instance=None: [])
    monkeypatch.setattr(repotree, "tracked_python_paths", lambda repo, base_commit, cache_dir=None: [])
    return rows


def test_generate_refuses_to_redraw_a_split_that_trials_were_run_under(tmp_path, offline_generate, capsys):
    out = tmp_path / "tasks"
    on_disk = make_split([i.instance_id for i in offline_generate], seed=7, dev_size=20, frozen_size=200)
    write_split_manifest(out / "split-manifest.json", on_disk, dataset_revision="rev-a")

    # A different seed draws a different split, which would move instances already run.
    code = main(["generate", "--revision", "rev-a", "--seed", "0", "--subset", "dev", "--out", str(out)])

    assert code == 2
    assert "--force-split" in capsys.readouterr().err
    assert json.loads((out / "split-manifest.json").read_text())["frozen"] == on_disk.frozen


def test_generate_overwrites_a_contradicted_split_when_forced(tmp_path, offline_generate):
    out = tmp_path / "tasks"
    on_disk = make_split([i.instance_id for i in offline_generate], seed=7, dev_size=20, frozen_size=200)
    write_split_manifest(out / "split-manifest.json", on_disk, dataset_revision="rev-a")

    code = main(
        ["generate", "--revision", "rev-a", "--seed", "0", "--subset", "dev", "--out", str(out), "--force-split"]
    )

    assert code == 0
    assert json.loads((out / "split-manifest.json").read_text())["seed"] == 0


def test_generate_refuses_a_half_written_task_tree(tmp_path, offline_generate, capsys):
    """Config rendering stamps the tree's revision into the store; the two manifests must agree."""
    out = tmp_path / "tasks"
    split = make_split([i.instance_id for i in offline_generate], seed=0, dev_size=20, frozen_size=200)
    write_split_manifest(out / "split-manifest.json", split, dataset_revision="rev-a")
    write_dataset_manifest(out / "dataset-manifest.json", "rev-b", len(offline_generate))

    code = main(["generate", "--revision", "rev-b", "--seed", "0", "--subset", "dev", "--out", str(out)])

    assert code == 2
    err = capsys.readouterr().err
    assert "half-written" in err and "rev-a" in err and "rev-b" in err

    forced = main(
        ["generate", "--revision", "rev-b", "--seed", "0", "--subset", "dev", "--out", str(out), "--force-split"]
    )
    assert forced == 0
    assert json.loads((out / "split-manifest.json").read_text())["dataset_revision"] == "rev-b"


def test_build_manifest_merges_records_through_the_cli(tmp_path):
    manifest = tmp_path / "build-manifest.json"
    manifest.write_text(json.dumps({"entries": [{"instance_id": "dev-1", "wall_clock_s": 9, "index_bytes": 5}]}))
    records = tmp_path / "records.jsonl"
    records.write_text(json.dumps({"instance_id": "frozen-1", "wall_clock_s": 70, "index_bytes": 80}) + "\n")

    assert main(["build-manifest", str(manifest), str(records)]) == 0

    merged = json.loads(manifest.read_text())["entries"]
    assert [e["instance_id"] for e in merged] == ["dev-1", "frozen-1"]


def test_generate_accepts_a_split_that_only_grows(tmp_path, offline_generate):
    """Regenerating to top the frozen set up to its planned size keeps every instance already run."""
    out = tmp_path / "tasks"
    smaller = make_split([i.instance_id for i in offline_generate], seed=0, dev_size=20, frozen_size=FROZEN_SIZE - 1)
    write_split_manifest(out / "split-manifest.json", smaller, dataset_revision="rev-a")

    code = main(["generate", "--revision", "rev-a", "--seed", "0", "--subset", "frozen", "--out", str(out)])

    assert code == 0
    frozen = json.loads((out / "split-manifest.json").read_text())["frozen"]
    assert len(frozen) == FROZEN_SIZE
    assert set(smaller.frozen) < set(frozen)


def test_main_sets_process_title(tmp_path):
    sweep = tmp_path / "sweep"
    sweep.mkdir()
    (sweep / "sweep-meta.json").write_text(
        json.dumps(
            {
                "arm": "baseline",
                "instruction_set_version": "none",
                "dataset_revision": "r",
                "model": "m",
                "max_turns": 40,
                "max_budget_usd": 2.0,
                "agent_timeout_sec": 1200.0,
                "max_output_tokens": 8000,
                "prompt_caching": True,
                "n_concurrent_trials": 3,
                "endpoint": "http://proxy:4000",
            }
        )
    )
    (sweep / "jobs").mkdir()
    exit_code = main(["import", str(sweep), "--tracking-uri", f"sqlite:///{tmp_path}/mlflow.db"])
    assert exit_code == 0
    assert setproctitle.getproctitle() == "c10r-evals import"


def test_setup_process_default_title():
    setup_process()
    assert setproctitle.getproctitle() == "c10r-evals"


def test_json_log_line_carries_fields(capsys):
    setup_process("generate")
    log_fields(logging.getLogger("c10r_evals.test"), logging.INFO, "task_generated", instance="inst-1", arm="baseline")
    line = capsys.readouterr().err.strip().splitlines()[-1]
    record = json.loads(line)
    assert record["event"] == "task_generated"
    assert record["level"] == "INFO"
    assert record["instance"] == "inst-1"
    assert record["arm"] == "baseline"


def test_json_formatter_is_valid_json_without_fields():
    record = logging.LogRecord("x", logging.WARNING, __file__, 1, "plain message", None, None)
    parsed = json.loads(JsonLineFormatter().format(record))
    assert parsed["event"] == "plain message"
    assert parsed["level"] == "WARNING"


@pytest.fixture
def tracking_uri(tmp_path):
    return f"sqlite:///{tmp_path}/mlflow.db"


def _seed_pair(tmp_path, instance_id, tracking_uri, treatment_version="v1"):
    baseline = tmp_path / f"sweep-{instance_id}-baseline"
    fx.write_sweep_meta(baseline, arm="baseline", instruction_set_version="none")
    fx.make_trial(baseline, instance_id, arm="baseline")
    import_sweep(baseline, tracking_uri)

    treatment = tmp_path / f"sweep-{instance_id}-treatment"
    fx.write_sweep_meta(treatment, arm="treatment", instruction_set_version=treatment_version)
    fx.make_trial(treatment, instance_id, arm="treatment")
    import_sweep(treatment, tracking_uri)


def _write_criteria(path):
    path.write_text(
        json.dumps(
            {
                "version": "criteria-v1",
                "primary_cost_metric": "total_tokens",
                "accuracy_reference_boundary": 0.1,
                "token_use_reference_boundary": 0.14,
                "confidence_level": 0.95,
                "failure_policy": {
                    "agent_failure": "empty-answer outcome",
                    "infra_failure": "exclude after bounded reruns",
                },
            }
        )
    )


def test_analyze_writes_report_and_trajectory_output(tmp_path, tracking_uri):
    _seed_pair(tmp_path, "demo__repo-1", tracking_uri)
    _seed_pair(tmp_path, "demo__repo-2", tracking_uri)
    criteria = tmp_path / "criteria.json"
    _write_criteria(criteria)
    out = tmp_path / "report.md"

    code = main(
        [
            "analyze",
            "--criteria",
            str(criteria),
            "--tracking-uri",
            tracking_uri,
            "--out",
            str(out),
            "--plots-dir",
            str(tmp_path / "plots"),
        ]
    )

    assert code == 0
    assert "## Decision criteria" in out.read_text()
    trajectory_output = json.loads(out.with_suffix(".trajectories.json").read_text())
    assert trajectory_output["trajectories"]
    assert trajectory_output["pairings"]


def test_analyze_report_links_resolve_to_the_written_plots(tmp_path, tracking_uri):
    from c10r_evals.plots import PLOT_FILENAMES
    from c10r_evals.report import FIGURE_TITLES

    _seed_pair(tmp_path, "demo__repo-1", tracking_uri)
    _seed_pair(tmp_path, "demo__repo-2", tracking_uri)
    criteria = tmp_path / "criteria.json"
    _write_criteria(criteria)
    out = tmp_path / "reports" / "report.md"

    code = main(
        [
            "analyze",
            "--criteria",
            str(criteria),
            "--tracking-uri",
            tracking_uri,
            "--out",
            str(out),
            "--plots-dir",
            str(tmp_path / "plots"),
        ]
    )

    assert code == 0
    report = out.read_text()
    for name, title in FIGURE_TITLES.items():
        link = f"../plots/{PLOT_FILENAMES[name]}"
        assert f"![{title}]({link})" in report
        assert (out.parent / link).is_file()


def test_analyze_split_manifest_restricts_to_the_frozen_subset(tmp_path, tracking_uri):
    _seed_pair(tmp_path, "frozen-inst", tracking_uri)
    _seed_pair(tmp_path, "dev-inst", tracking_uri)
    split = Split(dev=["dev-inst"], frozen=["frozen-inst"], seed=0)
    manifest = tmp_path / "split-manifest.json"
    write_split_manifest(manifest, split, dataset_revision="rev-a")
    criteria = tmp_path / "criteria.json"
    _write_criteria(criteria)
    out = tmp_path / "report.md"

    code = main(
        [
            "analyze",
            "--criteria",
            str(criteria),
            "--tracking-uri",
            tracking_uri,
            "--split-manifest",
            str(manifest),
            "--out",
            str(out),
            "--plots-dir",
            str(tmp_path / "plots"),
        ]
    )

    assert code == 0
    trajectory_output = json.loads(out.with_suffix(".trajectories.json").read_text())
    instance_ids = {row["instance_id"] for row in trajectory_output["trajectories"]}
    assert instance_ids == {"frozen-inst"}


def test_analyze_instruction_version_keeps_every_baseline_trial(tmp_path, tracking_uri):
    _seed_pair(tmp_path, "inst-v1", tracking_uri, treatment_version="v1")
    _seed_pair(tmp_path, "inst-v2", tracking_uri, treatment_version="v2")
    criteria = tmp_path / "criteria.json"
    _write_criteria(criteria)
    out = tmp_path / "report.md"

    code = main(
        [
            "analyze",
            "--criteria",
            str(criteria),
            "--tracking-uri",
            tracking_uri,
            "--instruction-version",
            "v1",
            "--out",
            str(out),
            "--plots-dir",
            str(tmp_path / "plots"),
        ]
    )

    assert code == 0
    trajectory_output = json.loads(out.with_suffix(".trajectories.json").read_text())
    instance_ids = {row["instance_id"] for row in trajectory_output["trajectories"]}
    assert instance_ids == {"inst-v1"}


def test_analyze_exits_2_with_no_paired_instances(tmp_path, tracking_uri, capsys):
    sweep = tmp_path / "sweep-baseline"
    fx.write_sweep_meta(sweep, arm="baseline", instruction_set_version="none")
    fx.make_trial(sweep, "inst-1", arm="baseline")
    import_sweep(sweep, tracking_uri)
    criteria = tmp_path / "criteria.json"
    _write_criteria(criteria)
    out = tmp_path / "report.md"

    code = main(["analyze", "--criteria", str(criteria), "--tracking-uri", tracking_uri, "--out", str(out)])

    assert code == 2
    err = capsys.readouterr().err
    assert "c10r-evals analyze" in err
    assert "no instance has a scorable trial in both arms" in err
