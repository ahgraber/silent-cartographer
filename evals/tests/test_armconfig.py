"""Run configuration: arm parity, tool-policy delta, baseline purity, prompt version, sweep metadata."""

import copy
import json
import shlex

import pytest
import yaml

from c10r_evals.armconfig import (
    C10R_TOOL_ADMISSION,
    DEFAULT_AGENT_TIMEOUT_SEC,
    DEFAULT_CONCURRENCY,
    DEFAULT_MAX_BUDGET_USD,
    DEFAULT_MAX_OUTPUT_TOKENS,
    DEFAULT_MAX_TURNS,
    ArmConfigBase,
    base_from_env,
    render_arm_configs,
    tool_flag,
)


@pytest.fixture
def rendered(tmp_path):
    prompt = tmp_path / "prompts" / "v1.md"
    prompt.parent.mkdir()
    # Shell metacharacters on purpose: the real instruction sets carry backticks,
    # quotes, angle brackets, and newlines, and Pier interpolates this unquoted.
    prompt.write_text('Use the c10r CLI: `c10r find <fragment>`.\nDon\'t grep first; try `c10r get "name"` (body).\n')
    base = ArmConfigBase(
        model_name="proxy/some-local-model",
        max_turns=40,
        max_budget_usd=2.0,
        env={"ANTHROPIC_BASE_URL": "http://host.docker.internal:4000", "ANTHROPIC_AUTH_TOKEN": "${EVAL_PROXY_TOKEN}"},
    )
    paths = render_arm_configs(
        base,
        prompt,
        tmp_path / "configs",
        tasks_root=tmp_path / "tasks" / "dev",
        dataset_revision="rev-abc",
        platform="linux/arm64",
    )
    return paths, prompt


def _agent(config: dict) -> dict:
    assert len(config["agents"]) == 1
    return config["agents"][0]


def test_arm_config_parity(rendered):
    paths, _ = rendered
    baseline = yaml.safe_load(paths["baseline"].read_text())
    treatment = yaml.safe_load(paths["treatment"].read_text())

    assert _agent(baseline)["model_name"] == _agent(treatment)["model_name"]
    assert _agent(baseline)["kwargs"]["max_turns"] == _agent(treatment)["kwargs"]["max_turns"]
    assert _agent(baseline)["kwargs"]["max_budget_usd"] == _agent(treatment)["kwargs"]["max_budget_usd"]
    assert _agent(baseline)["env"] == _agent(treatment)["env"]
    assert baseline["environment"] == treatment["environment"]
    assert baseline["n_concurrent_trials"] == treatment["n_concurrent_trials"]

    stripped = copy.deepcopy(treatment)
    agent = _agent(stripped)
    del agent["kwargs"]["append_system_prompt"]
    agent["kwargs"]["allowed_tools"] = tool_flag(
        [t for t in shlex.split(agent["kwargs"]["allowed_tools"])[0].split(",") if t != C10R_TOOL_ADMISSION]
    )
    # The arm's task tree and output dir are the arm materialization, not config drift.
    arm_specific = ("datasets", "jobs_dir")
    stripped = {k: v for k, v in stripped.items() if k not in arm_specific}
    baseline_stripped = {k: v for k, v in baseline.items() if k not in arm_specific}
    assert stripped == baseline_stripped


def test_datasets_point_at_arm_trees(rendered):
    paths, _ = rendered
    baseline = yaml.safe_load(paths["baseline"].read_text())
    treatment = yaml.safe_load(paths["treatment"].read_text())
    assert baseline["datasets"][0]["path"].endswith("dev/baseline")
    assert treatment["datasets"][0]["path"].endswith("dev/treatment")


def test_agent_kwargs_survive_the_shell(rendered):
    """Pier interpolates string kwargs into a shell command unquoted; each must be one safe word."""
    paths, prompt = rendered
    treatment = _agent(yaml.safe_load(paths["treatment"].read_text()))["kwargs"]
    baseline = _agent(yaml.safe_load(paths["baseline"].read_text()))["kwargs"]

    for kwargs in (baseline, treatment):
        for key in ("allowed_tools", "disallowed_tools"):
            assert isinstance(kwargs[key], str), f"{key} must be a string; Pier rejects a list"
            assert len(shlex.split(kwargs[key])) == 1, f"{key} must survive the shell as one argument"

    assert shlex.split(treatment["allowed_tools"])[0].split(",")[-1] == C10R_TOOL_ADMISSION
    assert shlex.split(treatment["append_system_prompt"]) == [prompt.read_text()]


def test_zero_budget_omits_the_spend_cap(tmp_path):
    """A zero cap means no spend cap; passing `--max-budget-usd 0` would leave the stop condition ambiguous."""
    prompt = tmp_path / "v1.md"
    prompt.write_text("prompt\n")
    base = ArmConfigBase(model_name="m", max_turns=40, max_budget_usd=0.0)
    paths = render_arm_configs(
        base, prompt, tmp_path / "configs", tasks_root=tmp_path / "tasks", dataset_revision="r", platform="linux/arm64"
    )
    for arm in ("baseline", "treatment"):
        kwargs = _agent(yaml.safe_load(paths[arm].read_text()))["kwargs"]
        assert "max_budget_usd" not in kwargs
        assert kwargs["max_turns"] == 40


@pytest.mark.parametrize("tool", ["Edit", "NotebookEdit", "WebFetch", "WebSearch", "Task"])
def test_episode_exclusions_are_disallowed_not_merely_unlisted(rendered, tool):
    """The agent runs with permissions bypassed, so only the disallowed list removes a tool.

    Leaving an exclusion off it and out of the allowed list leaves the tool available:
    that is how repository edits and web lookups reach an episode that forbids both.
    """
    paths, _ = rendered
    for arm in ("baseline", "treatment"):
        kwargs = _agent(yaml.safe_load(paths[arm].read_text()))["kwargs"]
        disallowed = shlex.split(kwargs["disallowed_tools"])[0].split(",")
        assert tool in disallowed
        assert tool not in shlex.split(kwargs["allowed_tools"])[0].split(",")


def test_episode_bounds_applied_to_both_arms(rendered):
    """Pier waits on the agent forever without a timeout, and the token ceiling caps a repetition loop."""
    paths, _ = rendered
    for arm in ("baseline", "treatment"):
        agent = _agent(yaml.safe_load(paths[arm].read_text()))
        assert agent["override_timeout_sec"] == DEFAULT_AGENT_TIMEOUT_SEC
        assert agent["env"]["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] == str(DEFAULT_MAX_OUTPUT_TOKENS)

    baseline = _agent(yaml.safe_load(paths["baseline"].read_text()))
    treatment = _agent(yaml.safe_load(paths["treatment"].read_text()))
    assert baseline["override_timeout_sec"] == treatment["override_timeout_sec"]
    assert baseline["env"] == treatment["env"]


def test_episode_bounds_recorded_per_sweep(rendered):
    """Both bounds change what a trial costs, so a sweep is not replayable without them."""
    paths, _ = rendered
    for arm in ("baseline", "treatment"):
        meta = json.loads((paths[f"{arm}_sweep"] / "sweep-meta.json").read_text())
        assert meta["agent_timeout_sec"] == DEFAULT_AGENT_TIMEOUT_SEC
        assert meta["max_output_tokens"] == DEFAULT_MAX_OUTPUT_TOKENS


def test_episode_bounds_from_env():
    base = base_from_env(
        {
            "EVAL_PROXY_BASE_URL": "http://proxy:4000",
            "EVAL_MODEL_NAME": "m",
            "EVAL_AGENT_TIMEOUT_SEC": "300",
            "EVAL_MAX_OUTPUT_TOKENS": "4096",
        }
    )
    assert base.agent_timeout_sec == 300.0
    assert base.max_output_tokens == 4096


def test_tool_policy_recorded_delta(rendered):
    paths, _ = rendered
    policy = json.loads(paths["tool_policy"].read_text())
    assert policy["baseline"]["disallowed_tools"] == policy["treatment"]["disallowed_tools"]
    extra = set(policy["treatment"]["allowed_tools"]) - set(policy["baseline"]["allowed_tools"])
    assert extra == {C10R_TOOL_ADMISSION}
    assert set(policy["baseline"]["allowed_tools"]) - set(policy["treatment"]["allowed_tools"]) == set()


def test_baseline_context_no_c10r(rendered):
    paths, _ = rendered
    config = yaml.safe_load(paths["baseline"].read_text())
    # The tmp dirs this test renders into contain the test's own name (and thus
    # "c10r"); purity is about config content, so scrub the path-valued fields.
    content = {k: v for k, v in config.items() if k not in ("jobs_dir", "datasets")}
    assert "c10r" not in yaml.safe_dump(content).lower()
    assert "c10r" not in (paths["baseline_sweep"] / "sweep-meta.json").read_text().lower()


def test_prompt_version_in_config(rendered):
    paths, prompt = rendered
    meta = json.loads(paths["treatment_meta"].read_text())
    assert meta["instruction_set_version"] == "v1"
    assert meta["prompt_file"] == str(prompt)
    assert prompt.is_file()
    version_keys = [k for k in meta if "version" in k]
    assert version_keys == ["instruction_set_version"]

    treatment = yaml.safe_load(paths["treatment"].read_text())
    assert shlex.split(treatment["agents"][0]["kwargs"]["append_system_prompt"]) == [prompt.read_text()]


def test_sweep_meta_written_per_arm(rendered):
    paths, _ = rendered
    baseline_meta = json.loads((paths["baseline_sweep"] / "sweep-meta.json").read_text())
    treatment_meta = json.loads((paths["treatment_sweep"] / "sweep-meta.json").read_text())

    assert baseline_meta["arm"] == "baseline"
    assert baseline_meta["instruction_set_version"] == "none"
    assert treatment_meta["arm"] == "treatment"
    assert treatment_meta["instruction_set_version"] == "v1"
    for meta in (baseline_meta, treatment_meta):
        assert meta["dataset_revision"] == "rev-abc"
        assert meta["model"] == "proxy/some-local-model"
        assert meta["platform"] == "linux/arm64"

    # Pier's jobs_dir lands inside the sweep dir the importer reads.
    baseline = yaml.safe_load(paths["baseline"].read_text())
    assert baseline["jobs_dir"] == str(paths["baseline_sweep"] / "jobs")


def test_secret_stays_templated(rendered):
    paths, _ = rendered
    for arm in ("baseline", "treatment"):
        agent_env = yaml.safe_load(paths[arm].read_text())["agents"][0]["env"]
        assert agent_env["ANTHROPIC_AUTH_TOKEN"] == "${EVAL_PROXY_TOKEN}"


def test_base_from_env_defaults_the_optional_knobs():
    base = base_from_env(
        {"EVAL_PROXY_BASE_URL": "http://host.containers.internal:4000", "EVAL_MODEL_NAME": "proxy/some-local-model"}
    )
    assert base.model_name == "proxy/some-local-model"
    assert base.env == {
        "ANTHROPIC_BASE_URL": "http://host.containers.internal:4000",
        "ANTHROPIC_AUTH_TOKEN": "${EVAL_PROXY_TOKEN}",
    }
    assert (base.max_turns, base.max_budget_usd, base.n_concurrent_trials) == (
        DEFAULT_MAX_TURNS,
        DEFAULT_MAX_BUDGET_USD,
        DEFAULT_CONCURRENCY,
    )


def test_base_from_env_overrides_the_caps():
    base = base_from_env(
        {
            "EVAL_PROXY_BASE_URL": "http://proxy:4000",
            "EVAL_MODEL_NAME": "m",
            "EVAL_MAX_TURNS": "25",
            "EVAL_MAX_BUDGET_USD": "0.5",
            "EVAL_CONCURRENCY": "4",
        }
    )
    assert (base.max_turns, base.max_budget_usd, base.n_concurrent_trials) == (25, 0.5, 4)


@pytest.mark.parametrize(
    ("env", "expected"),
    [
        ({}, "EVAL_PROXY_BASE_URL"),
        ({"EVAL_PROXY_BASE_URL": "http://proxy:4000"}, "EVAL_MODEL_NAME"),
        ({"EVAL_PROXY_BASE_URL": "  ", "EVAL_MODEL_NAME": "m"}, "EVAL_PROXY_BASE_URL"),
    ],
)
def test_base_from_env_names_the_unset_variable(env, expected):
    with pytest.raises(ValueError, match=expected):
        base_from_env(env)


def test_base_from_env_rejects_a_non_numeric_cap():
    env = {"EVAL_PROXY_BASE_URL": "http://proxy:4000", "EVAL_MODEL_NAME": "m", "EVAL_MAX_TURNS": "forty"}
    with pytest.raises(ValueError, match="EVAL_MAX_TURNS"):
        base_from_env(env)
