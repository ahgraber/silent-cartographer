"""Planning a sweep: deficit grouping, the argv Pier is given, and the guards that refuse a run."""

from pathlib import Path

import pier_fixtures as fx
import pytest
import yaml

from c10r_evals.run import (
    RunRefused,
    arm_config,
    build_pier_argv,
    check_attempts,
    check_credentials,
    check_subset,
    plan,
    plan_arm,
    render_plan,
)

PIER = Path("/venv/bin/pier")


def write_arm(tmp_path, instances, arm="baseline", subset="dev", cfg="cfg"):
    """A task tree and a Pier config pointing at it, in the layout the harness renders."""
    tasks = tmp_path / "tasks" / subset / arm
    for instance in instances:
        (tasks / instance).mkdir(parents=True)
    sweep = tmp_path / "configs" / cfg / arm
    sweep.mkdir(parents=True)
    config = tmp_path / "configs" / cfg / f"{arm}.yaml"
    config.write_text(yaml.safe_dump({"datasets": [{"path": str(tasks)}], "jobs_dir": str(sweep / "jobs")}))
    return config, sweep


def attempts_for(sweep, instance, n, arm="baseline"):
    for i in range(n):
        fx.make_trial(sweep, instance, arm=arm, job=f"job-{i + 1}", attempt_suffix=str(i + 1))


# --- the argv Pier is given


def test_a_whole_tree_pass_names_no_instances(tmp_path):
    """Pier runs the config's own dataset; naming every instance would only be noise."""
    config, _ = write_arm(tmp_path, ["inst-a", "inst-b"])

    groups = plan_arm("baseline", config, attempts=1, pier=PIER)

    assert len(groups) == 1
    assert groups[0].instances is None
    assert groups[0].argv == (str(PIER), "run", "-c", str(config))


def test_a_partial_pass_names_the_tree_and_each_instance(tmp_path):
    """`-i` needs an explicit `-p`; Pier refuses a local dataset filter without one."""
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts_for(sweep, "inst-a", 1)

    groups = plan_arm("baseline", config, attempts=1, pier=PIER)

    argv = groups[0].argv
    assert "-p" in argv
    assert argv[argv.index("-p") + 1] == str(tmp_path / "tasks" / "dev" / "baseline")
    assert [argv[i + 1] for i, a in enumerate(argv) if a == "-i"] == ["inst-b", "inst-c"]


def test_a_target_above_one_passes_the_attempt_count(tmp_path):
    config, _ = write_arm(tmp_path, ["inst-a"])

    argv = plan_arm("baseline", config, attempts=3, pier=PIER)[0].argv

    assert argv[-2:] == ("-k", "3")


def test_one_attempt_needs_no_attempt_flag(tmp_path):
    config, _ = write_arm(tmp_path, ["inst-a"])

    assert "-k" not in plan_arm("baseline", config, attempts=1, pier=PIER)[0].argv


# --- deficit grouping


def test_a_ragged_arm_is_grouped_by_how_much_each_instance_lacks(tmp_path):
    """Pier's attempt count is uniform per invocation, so each shortfall needs its own call."""
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b", "inst-c", "inst-d"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts_for(sweep, "inst-a", 3)  # complete
    attempts_for(sweep, "inst-b", 2)  # lacks 1
    attempts_for(sweep, "inst-c", 1)  # lacks 2
    # inst-d lacks 3

    groups = plan_arm("baseline", config, attempts=3, pier=PIER)

    assert [(g.attempts, g.instances) for g in groups] == [
        (1, ("inst-b",)),
        (2, ("inst-c",)),
        (3, ("inst-d",)),
    ]


def test_an_even_arm_needs_one_invocation(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts_for(sweep, "inst-a", 1)
    attempts_for(sweep, "inst-b", 1)

    groups = plan_arm("baseline", config, attempts=3, pier=PIER)

    assert len(groups) == 1
    assert groups[0].attempts == 2
    assert groups[0].instances is None  # every planned instance, so the whole tree


def test_an_arm_at_the_target_plans_nothing(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts_for(sweep, "inst-a", 3)

    assert plan_arm("baseline", config, attempts=3, pier=PIER) == []


# --- interleaving the arms


def test_the_arms_alternate_in_blocks_rather_than_running_one_then_the_other(tmp_path):
    """Running a whole arm then the other would confound the arm with hours of endpoint drift."""
    instances = [f"inst-{i:02d}" for i in range(6)]
    write_arm(tmp_path, instances, arm="baseline")
    write_arm(tmp_path, instances, arm="treatment")

    groups = plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER, block_size=2)

    assert [g.block for g in groups] == [0, 0, 1, 1, 2, 2]
    # Each block covers both arms, so neither arm is concentrated in one stretch of the run.
    assert {g.arm for g in groups if g.block == 0} == {"baseline", "treatment"}


def test_the_leading_arm_inverts_between_blocks(tmp_path):
    """A fixed leader would put the same arm first in every pair and confound arm with position."""
    instances = [f"inst-{i:02d}" for i in range(6)]
    write_arm(tmp_path, instances, arm="baseline")
    write_arm(tmp_path, instances, arm="treatment")

    groups = plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER, block_size=2)

    assert [g.arm for g in groups] == [
        "baseline",
        "treatment",
        "treatment",
        "baseline",
        "baseline",
        "treatment",
    ]


def test_a_block_runs_only_its_own_instances(tmp_path):
    instances = [f"inst-{i:02d}" for i in range(4)]
    write_arm(tmp_path, instances, arm="baseline")
    write_arm(tmp_path, instances, arm="treatment")

    groups = plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER, block_size=2)

    assert groups[0].instances == ("inst-00", "inst-01")
    assert groups[2].instances == ("inst-02", "inst-03")


def test_a_block_covering_one_arm_only_plans_that_arm(tmp_path):
    """Work already done in one arm must not force an empty invocation for it."""
    instances = ["inst-00", "inst-01"]
    _, base_sweep = write_arm(tmp_path, instances, arm="baseline")
    write_arm(tmp_path, instances, arm="treatment")
    fx.write_sweep_meta(base_sweep, arm="baseline")
    attempts_for(base_sweep, "inst-00", 1)

    groups = plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER, block_size=1)

    # inst-00's baseline is already scored, so block 0 plans treatment alone.
    assert [(g.block, g.arm, g.instances) for g in groups] == [
        (0, "treatment", ("inst-00",)),
        (1, "treatment", ("inst-01",)),
        (1, "baseline", ("inst-01",)),
    ]


def test_one_arm_alone_does_not_block(tmp_path):
    """Alternating needs two arms; a single-arm run is the plain pass it always was."""
    write_arm(tmp_path, ["inst-a", "inst-b"], arm="baseline")

    groups = plan(tmp_path / "configs" / "cfg", ("baseline",), attempts=1, pier=PIER)

    assert len(groups) == 1
    assert groups[0].instances is None


def test_a_block_size_below_one_is_refused(tmp_path):
    write_arm(tmp_path, ["inst-a"], arm="baseline")
    write_arm(tmp_path, ["inst-a"], arm="treatment")

    with pytest.raises(RunRefused, match="block-size"):
        plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER, block_size=0)


def test_a_missing_arm_config_is_refused(tmp_path):
    write_arm(tmp_path, ["inst-a"], arm="baseline")

    with pytest.raises(RunRefused, match="does not exist"):
        plan(tmp_path / "configs" / "cfg", ("baseline", "treatment"), attempts=1, pier=PIER)


def test_a_missing_config_is_refused_before_anything_reads_it(tmp_path):
    """Otherwise the subset check opens the file first and the run dies on an unhandled error."""
    with pytest.raises(RunRefused, match="does not exist"):
        arm_config(tmp_path / "configs" / "absent", "baseline")


# --- what the caller sees


def test_the_printed_plan_is_the_argv_that_would_run(tmp_path):
    config, sweep = write_arm(tmp_path, ["inst-a", "inst-b"])
    fx.write_sweep_meta(sweep, arm="baseline")
    attempts_for(sweep, "inst-a", 1)

    groups = plan_arm("baseline", config, attempts=1, pier=PIER)
    rendered = render_plan(groups)

    for token in groups[0].argv:
        assert token in rendered
    assert "1 instance(s)" in rendered


# --- guards


def test_a_host_credential_refuses_the_run():
    """Pier copies these into the container, where they outrank the proxy the config names."""
    for leaked in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"):
        with pytest.raises(RunRefused, match=leaked):
            check_credentials({leaked: "secret", "EVAL_PROXY_TOKEN": "t"})


def test_a_missing_proxy_token_refuses_the_run():
    with pytest.raises(RunRefused, match="EVAL_PROXY_TOKEN"):
        check_credentials({})


def test_a_clean_environment_is_accepted():
    check_credentials({"EVAL_PROXY_TOKEN": "t"})


def test_a_config_running_another_subset_is_refused(tmp_path):
    """Running the dev tree while asking for frozen would report dev issues as the frozen result."""
    config, _ = write_arm(tmp_path, ["inst-a"], subset="dev")

    with pytest.raises(RunRefused, match="'dev' subset, not 'frozen'"):
        check_subset(config, "frozen")

    check_subset(config, "dev")


def test_more_attempts_than_the_frozen_run_declared_is_refused():
    with pytest.raises(RunRefused, match="pre-registered"):
        check_attempts("frozen", attempts=3, planned_attempts=1)


def test_the_frozen_attempt_bound_does_not_apply_to_dev():
    check_attempts("dev", attempts=3, planned_attempts=1)
    check_attempts("frozen", attempts=1, planned_attempts=1)
    check_attempts("frozen", attempts=3, planned_attempts=None)


def test_build_pier_argv_is_the_single_source_of_the_invocation():
    """`--dry-run` prints what this returns, so the printed and executed argv cannot differ."""
    argv = build_pier_argv(PIER, Path("c.yaml"), Path("tasks/dev/baseline"), ["x", "y"], 2)

    assert argv == (
        str(PIER),
        "run",
        "-c",
        "c.yaml",
        "-p",
        "tasks/dev/baseline",
        "-i",
        "x",
        "-i",
        "y",
        "-k",
        "2",
    )
