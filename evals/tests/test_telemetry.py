"""Telemetry import: capture partitions, idempotency, invocation counts, store-only queries."""

import json
import shutil

import pier_fixtures as fx
import pytest

from c10r_evals.telemetry import (
    AGENT_FAILURE,
    COMPLETED,
    INFRA_FAILURE,
    ImportMismatchError,
    import_sweep,
    query_trials,
)


@pytest.fixture
def tracking_uri(tmp_path):
    return f"sqlite:///{tmp_path}/mlflow.db"


def _single(trials, instance_id):
    matching = [t for t in trials if t["instance_id"] == instance_id]
    assert len(matching) == 1, f"expected exactly one record for {instance_id}, got {len(matching)}"
    return matching[0]


# --- Complete trial capture, one scenario per terminal-state partition.


def test_capture_completed(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-graded", trajectory=fx.make_trajectory([fx.bash_call("c10r get handler")]))

    summary = import_sweep(sweep, tracking_uri)
    assert summary.imported == 1

    record = _single(query_trials(tracking_uri), "inst-graded")
    assert record["terminal_state"] == COMPLETED
    assert record["params"]["arm"] == "treatment"
    assert record["params"]["model"] == "proxy/some-local-model"
    assert record["params"]["instruction_set_version"] == "v1"
    assert record["params"]["dataset_revision"] == "rev-abc"
    assert record["params"]["platform"] == "linux/amd64"
    assert record["metrics"]["total_tokens"] == 32_000
    assert record["metrics"]["reward"] == pytest.approx(0.8)
    assert record["metrics"]["any_gold_hit"] == 1
    assert record["metrics"]["uptake_count"] == 1


def test_capture_unparsed(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-unparsed", reward=fx.UNPARSED_REWARD)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-unparsed")
    assert record["terminal_state"] == COMPLETED
    assert record["metrics"]["unparsed"] == 1.0
    assert record["metrics"]["reward"] == 0.0


def test_capture_baseline_zero_uptake(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep, arm="baseline", instruction_set_version="none")
    fx.make_trial(sweep, "inst-base")

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-base")
    assert record["arm"] == "baseline"
    assert record["metrics"]["uptake_count"] == 0


def test_capture_agent_failure(tmp_path, tracking_uri):
    """The agent's own execution timeout is its doing: scored as an empty answer, cost retained."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    partial = fx.make_trajectory(
        [fx.bash_call("grep -rn x /repo")],
        {"total_prompt_tokens": 90_000, "total_completion_tokens": 5_000, "total_cost_usd": 0.4, "total_steps": 40},
    )
    fx.make_trial(sweep, "inst-timeout", exception_type="AgentTimeoutError", reward=None, trajectory=partial)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-timeout")
    assert record["terminal_state"] == AGENT_FAILURE
    assert record["metrics"]["reward"] == 0.0
    assert record["metrics"]["any_gold_hit"] == 0
    assert record["metrics"]["unparsed"] == 1.0
    assert record["metrics"]["total_tokens"] == 95_000  # consumed cost still counts


def test_capture_exhausted_caps_as_graded_outcome(tmp_path, tracking_uri):
    """An agent that exhausts its turn or spend cap exits zero, so the trial is graded, not excluded."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-exhausted", reward=fx.UNPARSED_REWARD)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-exhausted")
    assert record["terminal_state"] == COMPLETED
    assert record["metrics"]["unparsed"] == 1.0
    assert record["metrics"]["reward"] == 0.0


@pytest.mark.parametrize(
    "exception_type",
    ["NonZeroAgentExitCodeError", "AgentSetupTimeoutError", "EnvironmentStartTimeoutError", "HealthcheckError"],
)
def test_capture_infra_failure(tmp_path, tracking_uri, exception_type):
    """A harness or endpoint failure is excluded, not scored — an unreachable model is not a wrong answer."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    trial = fx.make_trial(sweep, "inst-infra", exception_type=exception_type, reward=None)
    shutil.rmtree(trial / "agent")

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-infra")
    assert record["terminal_state"] == INFRA_FAILURE
    assert "reward" not in record["metrics"]


def test_exhausted_limit_scores_as_agent_failure(tmp_path, tracking_uri):
    """The agent CLI exits non-zero when it talks past its output ceiling; that is its own doing."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    spent = fx.make_trajectory(
        [fx.bash_call("grep -rn x /repo")],
        {"total_prompt_tokens": 1_069_590, "total_completion_tokens": 242_914, "total_steps": 40},
    )
    fx.make_trial(sweep, "inst-ceiling", exception_type="NonZeroAgentExitCodeError", reward=None, trajectory=spent)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-ceiling")
    assert record["terminal_state"] == AGENT_FAILURE
    assert record["metrics"]["reward"] == 0.0
    assert record["metrics"]["unparsed"] == 1.0


def test_refused_endpoint_stays_infra_failure(tmp_path, tracking_uri):
    """The same exit code with nothing spent means the endpoint refused, so nothing ran to score."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    untouched = fx.make_toolless_trajectory({"total_prompt_tokens": 0, "total_completion_tokens": 0, "total_steps": 2})
    fx.make_trial(sweep, "inst-404", exception_type="NonZeroAgentExitCodeError", reward=None, trajectory=untouched)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-404")
    assert record["terminal_state"] == INFRA_FAILURE
    assert "reward" not in record["metrics"]


def test_ungraded_trial_is_infra_failure(tmp_path, tracking_uri):
    """No exception but no reward means the verifier never ran: not an answer to score."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-ungraded", reward=None)

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-ungraded")
    assert record["terminal_state"] == INFRA_FAILURE
    assert "reward" not in record["metrics"]


# --- Idempotent import: novel, re-import, interrupted resume, changed artifacts.


def test_import_novel_once(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-1")
    summary = import_sweep(sweep, tracking_uri)
    assert (summary.imported, summary.skipped) == (1, 0)
    assert len(query_trials(tracking_uri)) == 1


def test_reimport_no_duplicate(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-1")
    import_sweep(sweep, tracking_uri)
    summary = import_sweep(sweep, tracking_uri)
    assert (summary.imported, summary.skipped) == (0, 1)
    assert len(query_trials(tracking_uri)) == 1


def test_interrupted_import_converges(tmp_path, tracking_uri):
    full = tmp_path / "sweep"
    fx.write_sweep_meta(full)
    fx.make_trial(full, "inst-1")
    fx.make_trial(full, "inst-2")
    fx.make_trial(full, "inst-3")

    partial = tmp_path / "partial"
    fx.write_sweep_meta(partial)
    fx.make_trial(partial, "inst-1")
    import_sweep(partial, tracking_uri)  # the interrupted first pass recorded one trial

    summary = import_sweep(full, tracking_uri)
    assert (summary.imported, summary.skipped) == (2, 1)
    trials = query_trials(tracking_uri)
    assert sorted(t["instance_id"] for t in trials) == ["inst-1", "inst-2", "inst-3"]


def test_changed_artifacts_rejected(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    trial = fx.make_trial(sweep, "inst-1")
    import_sweep(sweep, tracking_uri)

    before = _single(query_trials(tracking_uri), "inst-1")
    (trial / "verifier" / "reward.json").write_text(json.dumps({**fx.GRADED_REWARD, "reward": 1.0}))

    with pytest.raises(ImportMismatchError, match="artifacts changed"):
        import_sweep(sweep, tracking_uri)

    after = _single(query_trials(tracking_uri), "inst-1")
    assert after == before


# --- Invocation counts.


def test_uptake_count(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    calls = [fx.bash_call("c10r get handler"), fx.bash_call("c10r trace callers handler && c10r find core")]
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_trajectory(calls))

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["uptake_count"] == 3


def test_uptake_zero(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_trajectory([fx.bash_call("cat pkg/core.py")]))

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["uptake_count"] == 0


def test_mixed_counts(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    calls = [fx.bash_call("c10r get handler")] + [fx.bash_call(f"grep -rn term{i} /repo") for i in range(20)]
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_trajectory(calls))

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["uptake_count"] == 1
    assert record["metrics"]["search_count"] == 20


def test_native_search_tools_counted(tmp_path, tracking_uri):
    """Search happens through the agent's own Grep and Glob tools as well as through Bash."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    calls = [fx.tool_call("Grep", pattern="handler"), fx.tool_call("Glob", pattern="**/*.py"), fx.bash_call("rg x")]
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_trajectory(calls))

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["search_count"] == 3
    assert record["metrics"]["uptake_count"] == 0


def test_toolless_steps_counted_as_zero(tmp_path, tracking_uri):
    """A step that called no tool omits `tool_calls`; counting must not treat that as missing data."""
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_toolless_trajectory())

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["uptake_count"] == 0
    assert record["metrics"]["search_count"] == 0


def test_instance_id_strips_the_arm_prefix(tmp_path, tracking_uri):
    """Pairing joins arms on the instance id, so the arm prefix Pier puts in the task name must come off."""
    baseline = tmp_path / "sweep-b"
    fx.write_sweep_meta(baseline, arm="baseline", instruction_set_version="none")
    fx.make_trial(baseline, "jazzband__tablib-613", arm="baseline")
    treatment = tmp_path / "sweep-t"
    fx.write_sweep_meta(treatment, arm="treatment")
    fx.make_trial(treatment, "jazzband__tablib-613", arm="treatment")

    import_sweep(baseline, tracking_uri)
    import_sweep(treatment, tracking_uri)

    ids = {t["instance_id"] for t in query_trials(tracking_uri)}
    assert ids == {"jazzband__tablib-613"}, "both arms must land on one instance id or no pair can form"


def test_wall_clock_from_timestamps(tmp_path, tracking_uri):
    sweep = tmp_path / "sweep"
    fx.write_sweep_meta(sweep)
    calls = [fx.bash_call("ls"), fx.bash_call("cat x"), fx.bash_call("ls /repo")]
    fx.make_trial(sweep, "inst-1", trajectory=fx.make_trajectory(calls, step_seconds=45.0))

    import_sweep(sweep, tracking_uri)
    record = _single(query_trials(tracking_uri), "inst-1")
    assert record["metrics"]["wall_clock_s"] == 90.0


# --- Cross-run comparability.


def test_query_by_arm_and_version(tmp_path, tracking_uri):
    treatment_v1 = tmp_path / "sweep-t1"
    fx.write_sweep_meta(treatment_v1, arm="treatment", instruction_set_version="v1")
    fx.make_trial(treatment_v1, "inst-1")
    treatment_v2 = tmp_path / "sweep-t2"
    fx.write_sweep_meta(treatment_v2, arm="treatment", instruction_set_version="v2")
    fx.make_trial(treatment_v2, "inst-1")
    baseline = tmp_path / "sweep-b"
    fx.write_sweep_meta(baseline, arm="baseline", instruction_set_version="none")
    fx.make_trial(baseline, "inst-1")

    for sweep in (treatment_v1, treatment_v2, baseline):
        import_sweep(sweep, tracking_uri)
        shutil.rmtree(sweep)  # selection must work from the store alone

    selected = query_trials(tracking_uri, arm="treatment", instruction_set_version="v2")
    assert len(selected) == 1
    assert selected[0]["instruction_set_version"] == "v2"
    assert len(query_trials(tracking_uri, arm="treatment")) == 2
    assert len(query_trials(tracking_uri)) == 3
