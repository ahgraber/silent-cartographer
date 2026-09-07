"""CLI skeleton behavior: subcommands exist, process title and structured logging are configured."""

import json
import logging

import setproctitle

from c10r_evals.cli import build_parser, main
from c10r_evals.runtime import JsonLineFormatter, log_fields, setup_process


def test_subcommands_registered():
    parser = build_parser()
    actions = [a for a in parser._subparsers._group_actions if hasattr(a, "choices")]
    assert set(actions[0].choices) == {"generate", "import", "analyze"}


def test_main_sets_process_title(tmp_path):
    sweep = tmp_path / "sweep"
    sweep.mkdir()
    (sweep / "sweep-meta.json").write_text(
        '{"arm": "baseline", "instruction_set_version": "none", "dataset_revision": "r", "model": "m"}'
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
