"""Resume: which instances of an arm still need to run after an interrupted sweep."""

import json

import pier_fixtures as fx
import pytest
import yaml

from c10r_evals.resume import (
    ArmConfigError,
    arm_status,
    dataset_path,
    pending_instances,
    planned_instances,
    scored_attempts,
    scored_instances,
)


def write_arm(tmp_path, instances, sweep_name="baseline"):
    """A task tree and a Pier config pointing at it, in the layout the harness renders."""
    tasks = tmp_path / "tasks" / "dev" / sweep_name
    for instance in instances:
        (tasks / instance).mkdir(parents=True)
    sweep = tmp_path / "configs" / "cfg" / sweep_name
    sweep.mkdir(parents=True)
    config = tmp_path / "configs" / "cfg" / f"{sweep_name}.yaml"
    config.write_text(yaml.safe_dump({"datasets": [{"path": str(tasks)}], "jobs_dir": str(sweep / "jobs")}))
    return config, sweep


def test_untouched_instances_are_pending(tmp_path):
    config, _ = write_arm(tmp_path, ["inst-a", "inst-b"])
    assert pending_instances(config) == ["inst-a", "inst-b"]


def test_completed_instances_are_not_pending(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline")

    assert pending_instances(config) == ["inst-b"]


def test_agent_failures_are_not_pending(tmp_path):
    """An agent that ran out of turns produced an outcome, so rerunning it would discard one."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline", exception_type="AgentTimeoutError")

    assert pending_instances(config) == []


def test_infra_failures_are_pending(tmp_path):
    """An episode that never ran produced nothing, so it must run again."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline", exception_type="EnvironmentBuildError", trajectory=None)

    assert pending_instances(config) == ["inst-a"]


def test_trials_without_a_result_are_pending(tmp_path):
    """A trial killed mid-flight leaves a directory but no record."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    trial = fx.make_trial(sweep, "inst-a", arm="baseline")
    (trial / "result.json").unlink()

    assert pending_instances(config) == ["inst-a"]


def test_scored_instances_union_across_job_directories(tmp_path):
    """A resumed sweep writes a new job directory; earlier ones still count as done."""
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline", job="job-1")
    fx.make_trial(sweep, "inst-b", arm="baseline", job="job-2")

    assert scored_instances(sweep) == {"inst-a", "inst-b"}
    assert pending_instances(config) == ["inst-c"]


def test_nothing_pending_once_the_pass_is_complete(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    for instance in ("inst-a", "inst-b"):
        fx.make_trial(sweep, instance, arm="baseline")

    assert pending_instances(config) == []


def test_a_sweep_that_never_ran_has_no_scored_instances(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a"])
    assert scored_instances(sweep) == set()
    assert pending_instances(config) == ["inst-a"]


def test_instance_id_comes_from_the_record_not_the_directory(tmp_path):
    """Pier truncates long trial directory names, so the directory is not the instance."""
    config, sweep = write_arm(tmp_path, ["some-very-long-instance-name-1234"])
    fx.write_sweep_meta(sweep, arm="baseline")
    trial = fx.make_trial(sweep, "truncated-name", arm="baseline")
    record = json.loads((trial / "result.json").read_text())
    record["task_name"] = "baseline/some-very-long-instance-name-1234"
    (trial / "result.json").write_text(json.dumps(record))

    assert pending_instances(config) == []


# --- Attempts per cell, for a sweep run at K > 1.


def attempts(sweep, instance, n, arm="baseline", **kwargs):
    """Record n scorable attempts for one instance, each in its own job directory."""
    for i in range(n):
        fx.make_trial(sweep, instance, arm=arm, job=f"job-{i + 1}", attempt_suffix=str(i + 1), **kwargs)


def test_attempts_are_counted_per_instance_across_job_directories(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 1)

    assert scored_attempts(sweep) == {"inst-a": 3, "inst-b": 1}
    assert scored_instances(sweep) == {"inst-a", "inst-b"}
    assert dataset_path(config).name == "baseline"


def test_a_cell_short_of_the_target_is_pending(tmp_path):
    """An interrupted K=3 run leaves cells at one of three; one full pass calls those done."""
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 1)

    assert pending_instances(config, attempts=1) == []
    assert pending_instances(config, attempts=3) == ["inst-b"]


def test_the_deficit_is_how_many_attempts_the_cell_still_owes(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 1)

    status = arm_status(config, attempts=3)

    assert status.deficits == {"inst-b": 2, "inst-c": 3}
    assert status.owed == 5
    assert status.pending == ["inst-b", "inst-c"]
    assert not status.complete


def test_infra_failures_do_not_count_toward_the_target(tmp_path):
    """An episode that never ran produced nothing, so it owes an attempt however often it failed."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 2, exception_type="EnvironmentBuildError", trajectory=None)
    fx.make_trial(sweep, "inst-a", arm="baseline", job="job-3", attempt_suffix="3")

    assert scored_attempts(sweep) == {"inst-a": 1}
    assert arm_status(config, attempts=2).deficits == {"inst-a": 1}


def test_the_attempt_distribution_shows_a_ragged_arm(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 1)

    assert arm_status(config, attempts=3).distribution == {0: 1, 1: 1, 3: 1}


def test_an_arm_at_the_target_is_complete(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 3)

    status = arm_status(config, attempts=3)

    assert status.complete
    assert status.owed == 0
    assert status.distribution == {3: 2}


def test_more_attempts_than_the_target_owe_nothing(tmp_path):
    """Asking for the K a sweep already has must run nothing, so the target is idempotent."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 4)

    assert arm_status(config, attempts=3).complete


def test_a_target_below_what_the_arm_holds_is_reported(tmp_path):
    """Nothing runs and nothing is discarded, but a low target usually means the wrong config."""
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 3)
    attempts(sweep, "inst-b", 4)
    attempts(sweep, "inst-c", 1)

    status = arm_status(config, attempts=1)

    assert status.complete
    assert status.above_target == {"inst-a": 3, "inst-b": 4}
    assert arm_status(config, attempts=4).above_target == {}


def test_a_target_below_one_is_refused(tmp_path):
    config, _ = write_arm(tmp_path, ["inst-a"])
    with pytest.raises(ValueError, match="at least 1"):
        arm_status(config, attempts=0)


def test_a_result_identified_only_by_task_id_still_counts(tmp_path):
    """The importer falls back to task_id, so counting attempts must too, or an arm is both
    scorable to the analysis and pending to the runner."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    trial = fx.make_trial(sweep, "inst-a", arm="baseline")
    record = json.loads((trial / "result.json").read_text())
    del record["task_name"]
    record["task_id"] = {"path": "tasks/dev/baseline/inst-a"}
    (trial / "result.json").write_text(json.dumps(record))

    assert scored_attempts(sweep) == {"inst-a": 1}
    assert pending_instances(config) == []


def test_two_attempts_in_one_job_directory_both_count(tmp_path):
    """Pier's -k writes several trials into a single job directory."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline", job="job-1", attempt_suffix="1")
    fx.make_trial(sweep, "inst-a", arm="baseline", job="job-1", attempt_suffix="2")

    assert scored_attempts(sweep) == {"inst-a": 2}
    assert arm_status(config, attempts=2).complete


def test_a_recorded_instance_outside_the_task_tree_is_not_counted_as_planned(tmp_path):
    """An instance dropped from the tree must not appear in the attempt distribution."""
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts(sweep, "inst-a", 2)
    attempts(sweep, "inst-gone", 2)

    status = arm_status(config, attempts=2)

    assert status.planned == 1
    assert status.distribution == {2: 1}
    assert status.above_target == {}


def test_a_job_directory_holding_loose_files_is_ignored(tmp_path):
    _, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    fx.make_trial(sweep, "inst-a", arm="baseline")
    (sweep / "jobs" / "job-1" / "summary.json").write_text("{}")
    (sweep / "jobs" / "stray.log").write_text("")

    assert scored_attempts(sweep) == {"inst-a": 1}


def test_dataset_path_is_read_from_the_config(tmp_path):
    config, _ = write_arm(tmp_path, ["inst-a"])
    assert dataset_path(config).name == "baseline"


def test_a_config_naming_two_datasets_is_refused(tmp_path):
    config = tmp_path / "two.yaml"
    config.write_text(yaml.safe_dump({"datasets": [{"path": "a"}, {"path": "b"}], "jobs_dir": "j"}))
    with pytest.raises(ArmConfigError):
        dataset_path(config)


def test_a_missing_task_tree_is_refused(tmp_path):
    with pytest.raises(ArmConfigError):
        planned_instances(tmp_path / "nope")
