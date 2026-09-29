"""Rewriting the agent's declared limits across a config directory and the trials run under it."""

import json

import pytest
import yaml

from c10r_evals.reconfigure import LOG_NAME, Limits, ReconfigureError, reconfigure


def agent(timeout, max_turns, output_tokens, budget):
    """The agent entry, in the shape both the rendered and the recorded configs use."""
    kwargs = {"max_turns": max_turns}
    if budget:
        kwargs["max_budget_usd"] = budget
    return {
        "name": "claude-code",
        "override_timeout_sec": timeout,
        "kwargs": kwargs,
        "env": {"CLAUDE_CODE_MAX_OUTPUT_TOKENS": str(output_tokens)},
    }


def write_config_dir(
    tmp_path,
    timeout=3600.0,
    max_turns=80,
    output_tokens=8000,
    budget=0.0,
    concurrency=3,
    trials=("job-1/inst-a", "job-1/inst-b"),
):
    """A rendered config directory with arm configs, sweep metadata, and recorded trials."""
    config_dir = tmp_path / "configs" / "frozen"
    for arm in ("baseline", "treatment"):
        (config_dir / arm).mkdir(parents=True)
        (config_dir / f"{arm}.yaml").write_text(
            yaml.safe_dump(
                {
                    "agents": [agent(timeout, max_turns, output_tokens, budget)],
                    "datasets": [{"path": f"tasks/frozen/{arm}"}],
                    "jobs_dir": f"{config_dir / arm}/jobs",
                    "n_concurrent_trials": concurrency,
                }
            )
        )
        (config_dir / arm / "sweep-meta.json").write_text(
            json.dumps(
                {
                    "arm": arm,
                    "agent_timeout_sec": timeout,
                    "max_turns": max_turns,
                    "max_budget_usd": budget,
                    "max_output_tokens": output_tokens,
                    "n_concurrent_trials": concurrency,
                }
            )
        )
        for trial in trials:
            trial_dir = config_dir / arm / "jobs" / trial
            trial_dir.mkdir(parents=True)
            (trial_dir / "config.json").write_text(
                json.dumps(
                    {
                        "agent": agent(timeout, max_turns, output_tokens, budget),
                        "verifier": {"override_timeout_sec": None},
                    }
                )
            )
    return config_dir


def test_a_later_run_picks_up_the_new_limits(tmp_path):
    """The arm config is the only file a run reads, so the rewrite has to reach it."""
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(agent_timeout_sec=7200.0, max_turns=120))

    for arm in ("baseline", "treatment"):
        agent = yaml.safe_load((config_dir / f"{arm}.yaml").read_text())["agents"][0]
        assert agent["override_timeout_sec"] == 7200.0
        assert agent["kwargs"]["max_turns"] == 120


def test_the_recorded_trials_report_the_new_limits(tmp_path):
    config_dir = write_config_dir(tmp_path)

    rewrite = reconfigure(config_dir, Limits(agent_timeout_sec=7200.0))

    assert rewrite.trial_configs == 4
    for path in config_dir.glob("*/jobs/*/*/config.json"):
        assert json.loads(path.read_text())["agent"]["override_timeout_sec"] == 7200.0


def test_the_verifier_timeout_is_left_alone(tmp_path):
    """It is a separate limit that shares the field name; rewriting it would change grading."""
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(agent_timeout_sec=7200.0))

    for path in config_dir.glob("*/jobs/*/*/config.json"):
        assert json.loads(path.read_text())["verifier"]["override_timeout_sec"] is None


def test_the_turn_cap_is_untouched_when_not_named(tmp_path):
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(agent_timeout_sec=7200.0))

    agent = yaml.safe_load((config_dir / "baseline.yaml").read_text())["agents"][0]
    assert agent["kwargs"]["max_turns"] == 80


def test_the_log_keeps_what_the_files_said_before(tmp_path):
    """The rewrite discards what the earlier trials ran under, so it has to be recoverable."""
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(agent_timeout_sec=7200.0, max_turns=120))

    entry = json.loads((config_dir / LOG_NAME).read_text())[-1]
    assert entry["previous"]["agent_timeout_sec"] == [3600.0]
    assert entry["previous"]["max_turns"] == [80]
    assert entry["set"] == {"agent_timeout_sec": 7200.0, "max_turns": 120}
    assert entry["trial_configs"] == 4


def test_the_log_accumulates_across_rewrites(tmp_path):
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(agent_timeout_sec=5400.0))
    reconfigure(config_dir, Limits(agent_timeout_sec=7200.0))

    history = json.loads((config_dir / LOG_NAME).read_text())
    assert [e["set"]["agent_timeout_sec"] for e in history] == [5400.0, 7200.0]
    assert history[-1]["previous"]["agent_timeout_sec"] == [5400.0]


def test_a_dry_run_writes_nothing(tmp_path):
    config_dir = write_config_dir(tmp_path)

    rewrite = reconfigure(config_dir, Limits(agent_timeout_sec=7200.0), dry_run=True)

    assert rewrite.trial_configs == 4
    assert not (config_dir / LOG_NAME).exists()
    agent = yaml.safe_load((config_dir / "baseline.yaml").read_text())["agents"][0]
    assert agent["override_timeout_sec"] == 3600.0


def test_a_directory_holding_no_arm_config_is_refused(tmp_path):
    """Naming the arm subdirectory instead of the config directory would silently rewrite nothing."""
    config_dir = write_config_dir(tmp_path)

    with pytest.raises(ReconfigureError, match="no arm config"):
        reconfigure(config_dir / "baseline", Limits(agent_timeout_sec=7200.0))


def test_a_non_positive_timeout_is_refused(tmp_path):
    config_dir = write_config_dir(tmp_path)

    with pytest.raises(ReconfigureError, match="must be positive"):
        reconfigure(config_dir, Limits(agent_timeout_sec=0))


def test_naming_no_bound_is_refused():
    """An empty rewrite would report success while changing nothing."""
    with pytest.raises(ReconfigureError, match="at least one bound"):
        Limits()


# --- the bounds beyond the timeout


def test_concurrency_reaches_the_arm_config_and_sweep_metadata(tmp_path):
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(concurrency=2))

    assert yaml.safe_load((config_dir / "baseline.yaml").read_text())["n_concurrent_trials"] == 2
    assert json.loads((config_dir / "baseline" / "sweep-meta.json").read_text())["n_concurrent_trials"] == 2


def test_concurrency_does_not_touch_the_recorded_trials(tmp_path):
    """It is a job-level setting, so a trial never recorded one to rewrite."""
    config_dir = write_config_dir(tmp_path)

    rewrite = reconfigure(config_dir, Limits(concurrency=2))

    assert rewrite.trial_configs == 0
    for path in config_dir.glob("*/jobs/*/*/config.json"):
        assert "n_concurrent_trials" not in json.loads(path.read_text())


def test_the_output_ceiling_stays_a_string(tmp_path):
    """Claude Code reads it from the environment, so an int would change its type."""
    config_dir = write_config_dir(tmp_path)

    reconfigure(config_dir, Limits(max_output_tokens=16000))

    env = yaml.safe_load((config_dir / "baseline.yaml").read_text())["agents"][0]["env"]
    assert env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] == "16000"


def test_a_budget_is_added_where_the_renderer_had_omitted_it(tmp_path):
    """The renderer expresses no limit by leaving the key out, so setting one has to insert it."""
    config_dir = write_config_dir(tmp_path, budget=0.0)

    reconfigure(config_dir, Limits(max_budget_usd=5.0))

    assert yaml.safe_load((config_dir / "baseline.yaml").read_text())["agents"][0]["kwargs"]["max_budget_usd"] == 5.0


def test_a_zero_budget_removes_the_limit(tmp_path):
    config_dir = write_config_dir(tmp_path, budget=5.0)

    reconfigure(config_dir, Limits(max_budget_usd=0.0))

    assert "max_budget_usd" not in yaml.safe_load((config_dir / "baseline.yaml").read_text())["agents"][0]["kwargs"]


def test_every_bound_can_be_set_at_once(tmp_path):
    config_dir = write_config_dir(tmp_path)

    reconfigure(
        config_dir,
        Limits(agent_timeout_sec=7200.0, max_turns=120, max_budget_usd=5.0, max_output_tokens=16000, concurrency=2),
    )

    config = yaml.safe_load((config_dir / "baseline.yaml").read_text())
    assert config["n_concurrent_trials"] == 2
    assert config["agents"][0]["override_timeout_sec"] == 7200.0
    assert config["agents"][0]["kwargs"] == {"max_turns": 120, "max_budget_usd": 5.0}
    assert config["agents"][0]["env"]["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] == "16000"
