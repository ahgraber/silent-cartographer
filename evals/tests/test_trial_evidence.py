import importlib

from c10r_evals import telemetry


def test_trial_evidence_is_canonical_for_terminal_state_decoding():
    evidence = importlib.import_module("c10r_evals.trial_evidence")

    assert evidence.instance_id_from_result is telemetry.instance_id_from_result
    assert evidence.episode_consumed_tokens is telemetry.episode_consumed_tokens
    assert evidence.classify_terminal_state is telemetry.classify_terminal_state
    assert evidence.COMPLETED == telemetry.COMPLETED
    assert evidence.AGENT_FAILURE == telemetry.AGENT_FAILURE
    assert evidence.INFRA_FAILURE == telemetry.INFRA_FAILURE


def test_trial_evidence_is_canonical_for_trajectory_decoding():
    evidence = importlib.import_module("c10r_evals.trial_evidence")

    assert evidence.count_invocations is telemetry.count_invocations
    assert evidence.trajectory_wall_clock is telemetry.trajectory_wall_clock


def test_trial_evidence_is_canonical_for_task_and_artifact_decoding():
    evidence = importlib.import_module("c10r_evals.trial_evidence")

    assert evidence.task_characteristics is telemetry.task_characteristics
    assert evidence.read_trajectory is telemetry._read_trajectory
    assert evidence.read_reward is telemetry._read_reward
