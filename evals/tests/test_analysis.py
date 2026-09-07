"""Paired analysis: Tango reference values, NI directions, pair assembly, ITT, criteria, report."""

import json

import pytest

from c10r_evals.analysis import Criteria, analyze, assemble_pairs
from c10r_evals.report import render_report
from c10r_evals.stats import cost_superiority, non_inferiority, tango_ci, tango_z
from c10r_evals.telemetry import AGENT_FAILURE, COMPLETED, INFRA_FAILURE

CRITERIA = Criteria(
    version="criteria-v1",
    primary_cost_metric="total_tokens",
    non_inferiority_margin=0.1,
    confidence_level=0.95,
    failure_policy={"agent_failure": "empty-answer outcome", "infra_failure": "exclude after bounded reruns"},
)


def trial(
    instance: str,
    arm: str,
    state: str = COMPLETED,
    hit: int = 1,
    tokens: float = 30_000,
    uptake: float = 0,
    search: float = 5,
    wall: float | None = 300.0,
    attempt: str | None = None,
    started_at: int = 0,
) -> dict:
    metrics = {"uptake_count": uptake, "search_count": search}
    if state != INFRA_FAILURE:
        metrics.update(
            {
                "total_tokens": tokens,
                "total_cost_usd": tokens / 250_000,
                "total_steps": 12,
                "any_gold_hit": hit,
                "reward": float(hit),
                "precision": float(hit),
                "recall": float(hit),
                "f1": float(hit),
                "unparsed": 0.0,
            }
        )
        if wall is not None:
            metrics["wall_clock_s"] = wall
    return {
        "instance_id": instance,
        "arm": arm,
        "instruction_set_version": "v1" if arm == "treatment" else "none",
        "terminal_state": state,
        "metrics": metrics,
        "params": {},
        "attempt": attempt or f"{instance}__{arm}",
        "trial_key": f"key-{instance}-{arm}-{attempt or '1'}",
        "started_at": started_at,
    }


def paired_trials(n: int = 10, treatment_tokens: float = 18_000, baseline_tokens: float = 30_000) -> list[dict]:
    trials = []
    for i in range(n):
        instance = f"inst-{i:02d}"
        trials.append(trial(instance, "baseline", tokens=baseline_tokens + i * 10, search=8, wall=400.0))
        trials.append(trial(instance, "treatment", tokens=treatment_tokens + i * 10, uptake=3, search=2, wall=250.0))
    return trials


# --- Tango score method against the published worked examples (Tango 1998).


def test_tango_ci_reference_values():
    # Karacan et al. sleeping-difficulties data as analysed in Tango (1998) §6.2:
    # n = 32 pairs, discordant counts 9 and 3; score-based 95% CI: -0.027 to 0.390.
    lower, upper = tango_ci(b=9, c=3, n=32, level=0.95)
    assert lower == pytest.approx(-0.027, abs=0.002)
    assert upper == pytest.approx(0.390, abs=0.002)

    # Soft-contact-lens cross-over trial, Tango (1998) §6.1: n = 44, b = 0, c = 1.
    # Z(delta = 0.1) = 1.709; 90% CI lower limit = -0.096.
    assert tango_z(b=0, c=1, n=44, delta=0.1) == pytest.approx(1.709, abs=0.005)
    lower_90, _ = tango_ci(b=0, c=1, n=44, level=0.90)
    assert lower_90 == pytest.approx(-0.096, abs=0.002)

    # McNemar special case (delta = 0): Z = (b - c)/sqrt(b + c) = 6/sqrt(12).
    assert tango_z(b=9, c=3, n=32, delta=0.0) == pytest.approx(1.732, abs=0.001)


def test_ni_outcome_directions():
    # Equal hit rates, small discordance, margin 0.1: comfortably non-inferior.
    good = non_inferiority(b=10, c=10, n=200, margin=0.1, confidence_level=0.95)
    assert good.outcome == "non-inferior"
    assert good.ci_lower > -0.1

    # Treatment hits 20 points less often than baseline against a 5-point margin: fails.
    bad = non_inferiority(b=0, c=40, n=200, margin=0.05, confidence_level=0.95)
    assert bad.outcome == "not-demonstrated"
    assert bad.ci_lower < -0.05


def test_cost_superiority_direction():
    cheaper = cost_superiority([-12_000 - i for i in range(10)], "total_tokens")
    assert cheaper.outcome == "superior"
    assert cheaper.median_delta < 0

    dearer = cost_superiority([12_000 + i for i in range(10)], "total_tokens")
    assert dearer.outcome == "not-demonstrated"


# --- Pair assembly under the failure policy.


def test_paired_delta_both_complete():
    trials = [trial("inst-1", "baseline", tokens=30_000), trial("inst-1", "treatment", tokens=18_000)]
    pairs, exclusions, dev_excluded = assemble_pairs(trials)
    assert len(pairs) == 1
    assert exclusions == []
    assert dev_excluded == 0
    assert pairs[0].delta("total_tokens") == -12_000


def test_agent_failure_scored_as_outcome():
    failed = trial("inst-1", "treatment", state=AGENT_FAILURE, hit=0, tokens=90_000)
    failed["metrics"]["unparsed"] = 1.0
    trials = [trial("inst-1", "baseline", tokens=30_000), failed]
    pairs, exclusions, _ = assemble_pairs(trials)
    assert len(pairs) == 1
    assert exclusions == []
    assert pairs[0].delta("total_tokens") == 60_000
    assert pairs[0].treatment["metrics"]["any_gold_hit"] == 0


def test_infra_failure_excluded_and_enumerated():
    trials = [
        trial("inst-1", "baseline"),
        trial("inst-1", "treatment", state=INFRA_FAILURE),
        trial("inst-2", "baseline"),
        trial("inst-2", "treatment"),
    ]
    pairs, exclusions, _ = assemble_pairs(trials)
    assert [p.instance_id for p in pairs] == ["inst-2"]
    assert [e["instance_id"] for e in exclusions] == ["inst-1"]
    assert "treatment" in exclusions[0]["reason"] and INFRA_FAILURE in exclusions[0]["reason"]


def test_missing_arm_excluded():
    pairs, exclusions, _ = assemble_pairs([trial("inst-1", "baseline")])
    assert pairs == []
    assert [e["instance_id"] for e in exclusions] == ["inst-1"]
    assert "treatment" in exclusions[0]["reason"]


def test_rerun_after_infra_failure_keeps_the_instance():
    """A bounded rerun exists to save the instance; excluding it anyway loses data the rerun recovered."""
    trials = [
        trial("inst-1", "baseline"),
        trial("inst-1", "treatment", state=INFRA_FAILURE, attempt="first", started_at=100),
        trial("inst-1", "treatment", attempt="rerun", started_at=200),
    ]
    pairs, exclusions, _ = assemble_pairs(trials)
    assert exclusions == []
    assert [p.instance_id for p in pairs] == ["inst-1"]
    assert pairs[0].provenance["treatment"]["attempt"] == "rerun"


def test_latest_usable_attempt_wins_and_records_what_it_replaced():
    trials = [
        trial("inst-1", "baseline"),
        trial("inst-1", "treatment", tokens=50_000, attempt="older", started_at=100),
        trial("inst-1", "treatment", tokens=10_000, attempt="newer", started_at=200),
    ]
    pairs, _, _ = assemble_pairs(trials)
    assert len(pairs) == 1
    assert pairs[0].treatment["metrics"]["total_tokens"] == 10_000
    provenance = pairs[0].provenance["treatment"]
    assert provenance["attempt"] == "newer"
    assert provenance["superseded"] == ["older"]


def test_attempt_selection_is_deterministic_regardless_of_store_order():
    forward = [
        trial("inst-1", "baseline"),
        trial("inst-1", "treatment", tokens=50_000, attempt="older", started_at=100),
        trial("inst-1", "treatment", tokens=10_000, attempt="newer", started_at=200),
    ]
    reversed_order = list(reversed(forward))
    assert assemble_pairs(forward)[0][0].treatment == assemble_pairs(reversed_order)[0][0].treatment


# --- Decision criteria file.


def test_criteria_required_keys(tmp_path):
    path = tmp_path / "criteria.json"
    path.write_text(json.dumps({"version": "criteria-v1", "primary_cost_metric": "total_tokens"}))
    with pytest.raises(ValueError, match="missing required keys"):
        Criteria.load(path)

    path.write_text(
        json.dumps(
            {
                "version": "criteria-v1",
                "primary_cost_metric": "total_tokens",
                "non_inferiority_margin": 0.1,
                "confidence_level": 0.95,
                "failure_policy": {"agent_failure": "outcome", "infra_failure": "exclude"},
            }
        )
    )
    criteria = Criteria.load(path)
    assert criteria.primary_cost_metric == "total_tokens"
    assert criteria.non_inferiority_margin == 0.1


# --- Intent-to-treat and the report.


def test_intent_to_treat_includes_zero_uptake():
    trials = paired_trials(n=10)
    # One treatment trial never touched c10r; it still counts.
    zero_uptake = next(t for t in trials if t["arm"] == "treatment")
    zero_uptake["metrics"]["uptake_count"] = 0

    result = analyze(trials, CRITERIA)
    assert result.cost.n_pairs == 10
    assert result.uptake["treatment_trials_with_uptake"] == 9
    assert result.uptake["treatment_trials"] == 10


def test_report_contents_complete():
    trials = paired_trials(n=10)
    trials.append(trial("inst-infra", "baseline"))
    trials.append(trial("inst-infra", "treatment", state=INFRA_FAILURE))
    build_manifest = [
        {"instance_id": f"inst-{i:02d}", "wall_clock_s": 60, "index_bytes": 40_000_000} for i in range(10)
    ]

    result = analyze(trials, CRITERIA, build_manifest_entries=build_manifest)
    report = render_report(result)

    for heading in (
        "## Decision criteria",
        "## Headline outcomes",
        "## Arm summaries",
        "## Paired deltas",
        "## Uptake and search displacement",
        "## Index amortization",
        "## Exclusions",
        "## Caveats",
    ):
        assert heading in report
    assert "criteria-v1" in report
    assert "inst-infra" in report and "no usable" in report and "infra-failure" in report
    assert "break-even" in report.lower()
    assert "Historical-fix file recovery" in report
    # Wall saving 150 s/episode against a 60 s index build: break-even 0.4 episodes.
    assert result.break_even["break_even_episodes"] == pytest.approx(0.4, abs=0.01)


def test_report_headline_not_uptake_conditioned():
    trials = paired_trials(n=10)
    for t in trials:
        if t["arm"] == "treatment":
            t["metrics"]["uptake_count"] = 0  # nobody used c10r at all

    result = analyze(trials, CRITERIA)
    report = render_report(result)
    assert result.cost.n_pairs == 10  # headline still counts every pair
    assert "regardless of c10r uptake" in report
    assert "0/10 trials used c10r" in report


def test_dev_instances_excluded_from_report():
    trials = paired_trials(n=10)
    trials.append(trial("inst-dev", "baseline", tokens=1))
    trials.append(trial("inst-dev", "treatment", tokens=1))
    frozen = {f"inst-{i:02d}" for i in range(10)}

    result = analyze(trials, CRITERIA, frozen_ids=frozen)
    report = render_report(result)
    assert result.cost.n_pairs == 10
    assert result.dev_excluded == 2
    assert "inst-dev" not in report
    assert "development-subset trials excluded from this report: 2" in report
