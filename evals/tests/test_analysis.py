"""Paired analysis: Tango reference values, estimates, registered claims, pair assembly, report."""

import json
import math
import statistics
from dataclasses import replace
from pathlib import Path

import pytest

from c10r_evals.analysis import (
    CACHE_SIGNAL_NEAR_ZERO_SHARE,
    CACHE_SIGNAL_TURN_THRESHOLD,
    TASK_LEVEL_LABEL,
    Criteria,
    MixedModelError,
    NoPairedInstances,
    Pair,
    PriceSchedule,
    _cache_engagement_summary,
    _paired_outcomes,
    _token_ratio_summary,
    analyze,
    assemble_pairs,
    trajectory_cost,
    trajectory_level_output,
)
from c10r_evals.report import FIGURE_TITLES, render_report
from c10r_evals.split import FROZEN_SIZE
from c10r_evals.stats import (
    accuracy_claims,
    accuracy_estimate,
    tango_ci,
    tango_z,
    token_ratio_estimate,
    token_use_claims,
)
from c10r_evals.telemetry import AGENT_FAILURE, COMPLETED, INFRA_FAILURE

CRITERIA = Criteria(
    version="criteria-v1",
    primary_cost_metric="total_tokens",
    accuracy_reference_boundary=0.1,
    token_use_reference_boundary=0.14,
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
    steps: int = 12,
) -> dict:
    metrics = {"uptake_count": uptake, "search_count": search}
    if state != INFRA_FAILURE:
        metrics.update(
            {
                "total_tokens": tokens,
                "total_cost_usd": tokens / 250_000,
                "total_steps": steps,
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


def test_tango_z_handles_near_zero_discriminant_without_raising():
    """A small, lopsided (b, c, n) can push the score statistic's discriminant fractionally
    below zero through floating-point cancellation alone; it must clamp, not raise."""
    z = tango_z(b=0, c=4, n=6, delta=0.5)
    assert math.isfinite(z)
    accuracy_estimate(b=0, c=4, n=6, confidence_level=0.95)

    # McNemar special case (delta = 0): Z = (b - c)/sqrt(b + c) = 6/sqrt(12).
    assert tango_z(b=9, c=3, n=32, delta=0.0) == pytest.approx(1.732, abs=0.001)


def test_accuracy_estimate_two_sided_interval():
    """The default confidence_level=0.95 reuses tango_ci at 95%; its 90% interval sources the claim bounds."""
    estimate = accuracy_estimate(b=9, c=3, n=32)
    assert estimate.lambda_ == pytest.approx((9 - 3) / 32)
    lower_95, upper_95 = tango_ci(b=9, c=3, n=32, level=0.95)
    assert estimate.ci_lower == pytest.approx(lower_95)
    assert estimate.ci_upper == pytest.approx(upper_95)
    lower_90, upper_90 = tango_ci(b=9, c=3, n=32, level=0.90)
    assert estimate.bound_lower == pytest.approx(lower_90)
    assert estimate.bound_upper == pytest.approx(upper_90)


def test_accuracy_estimate_confidence_level_changes_the_interval():
    """A declared confidence level other than 0.95 must actually change the computed interval."""
    narrower = accuracy_estimate(b=9, c=3, n=32, confidence_level=0.80)
    wider = accuracy_estimate(b=9, c=3, n=32, confidence_level=0.99)
    default = accuracy_estimate(b=9, c=3, n=32)
    assert narrower.ci_upper - narrower.ci_lower < default.ci_upper - default.ci_lower
    assert wider.ci_upper - wider.ci_lower > default.ci_upper - default.ci_lower
    lower_60, upper_60 = tango_ci(b=9, c=3, n=32, level=0.60)  # 2*0.80 - 1
    assert narrower.bound_lower == pytest.approx(lower_60)
    assert narrower.bound_upper == pytest.approx(upper_60)


def test_token_ratio_estimate_confidence_level_changes_the_interval():
    """A declared confidence level other than 0.95 must actually change the computed interval."""
    log_ratios = [math.log(1.1)] * 150 + [math.log(0.7)] * 50
    default = token_ratio_estimate(log_ratios, resamples=5000, seed=1)
    narrower = token_ratio_estimate(log_ratios, confidence_level=0.80, resamples=5000, seed=1)
    assert narrower.ci_upper - narrower.ci_lower < default.ci_upper - default.ci_lower
    assert narrower.bound_upper - narrower.bound_lower < default.bound_upper - default.bound_lower


def test_token_ratio_estimate_two_sided_interval():
    """The two-sided 95% interval (2.5/97.5 pct) and the one-sided 95% bounds (5/95 pct) share one bootstrap draw."""
    log_ratios = [math.log(1.1)] * 150 + [math.log(0.7)] * 50
    estimate = token_ratio_estimate(log_ratios, resamples=5000, seed=1)
    assert estimate.theta == pytest.approx(sum(log_ratios) / len(log_ratios))
    assert estimate.ratio == pytest.approx(math.exp(estimate.theta))
    assert estimate.ci_lower < estimate.bound_lower < estimate.theta < estimate.bound_upper < estimate.ci_upper
    assert estimate.ratio_ci_lower == pytest.approx(math.exp(estimate.ci_lower))
    assert estimate.ratio_bound_upper == pytest.approx(math.exp(estimate.bound_upper))


# --- Registered claims: each rule reads bounds from one interval per axis, never a separate resample.


def test_claim_outcome_directions():
    good = accuracy_estimate(b=40, c=10, n=200)
    good_claims = accuracy_claims(good, boundary=0.5)
    assert good_claims.superior
    assert good_claims.non_inferior
    assert not good_claims.harm
    assert not good_claims.material_harm

    bad = accuracy_estimate(b=0, c=40, n=200)
    bad_claims = accuracy_claims(bad, boundary=0.05)
    assert bad_claims.harm
    assert bad_claims.material_harm
    assert not bad_claims.superior
    assert not bad_claims.non_inferior

    cheap = token_ratio_estimate([math.log(0.5)] * 200)
    cheap_claims = token_use_claims(cheap, boundary=0.1)
    assert cheap_claims.superior
    assert cheap_claims.non_inferior
    assert not cheap_claims.harm

    dear = token_ratio_estimate([math.log(1.5)] * 200)
    dear_claims = token_use_claims(dear, boundary=0.1)
    assert dear_claims.harm
    assert dear_claims.material_harm
    assert not dear_claims.superior


def test_equivalence_requires_interval_inside_boundaries():
    """Equivalence is the two-one-sided-tests formulation: both 90% endpoints inside [-boundary, +boundary]."""
    tight = accuracy_estimate(b=5, c=5, n=2000)  # lambda = 0, tight discordance rate
    assert accuracy_claims(tight, boundary=0.05).equivalent

    wide = accuracy_estimate(b=1, c=1, n=10)  # too few pairs for a narrow interval
    assert not accuracy_claims(wide, boundary=0.05).equivalent


def test_overlapping_claims_both_reported():
    """Accuracy can be shown worse while also shown non-inferior: both claims hold at once."""
    estimate = accuracy_estimate(b=2, c=20, n=200)
    claims = accuracy_claims(estimate, boundary=0.15)
    assert claims.harm
    assert claims.non_inferior


def test_no_combined_verdict():
    """No reported field states a single pass, fail, or adoption outcome across the two axes.

    The report may say in prose that it issues no combined verdict; it must not carry a field
    that itself is such a verdict, e.g. a single "adopt"/"recommend" outcome mixing both axes.
    """
    trials = paired_trials(n=10)
    result = analyze(trials, CRITERIA)
    report = render_report(result)

    assert not hasattr(result, "outcome")
    assert not hasattr(result, "verdict")
    assert not hasattr(result, "adopt")
    assert not hasattr(result, "recommendation")
    forbidden = ("adopt c10r", "recommend", "overall outcome", "overall verdict")
    for term in forbidden:
        assert term not in report.lower()


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


def test_two_models_refuse_to_pair():
    """A second model names the same (instance, arm) cells; pairing them would read one as a rerun of the other."""
    a = [trial("inst-1", "baseline"), trial("inst-1", "treatment")]
    b = [
        trial("inst-1", "baseline", attempt="b1", started_at=999),
        trial("inst-1", "treatment", attempt="b2", started_at=999),
    ]
    for t in a:
        t["params"] = {"model": "model-A"}
    for t in b:
        t["params"] = {"model": "model-B"}

    assert len(assemble_pairs(a)[0]) == 1
    assert len(assemble_pairs(b)[0]) == 1
    with pytest.raises(MixedModelError, match="model-A"):
        assemble_pairs(a + b)


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
                "accuracy_reference_boundary": 0.1,
                "token_use_reference_boundary": 0.14,
                "confidence_level": 0.95,
                "failure_policy": {"agent_failure": "outcome", "infra_failure": "exclude"},
            }
        )
    )
    criteria = Criteria.load(path)
    assert criteria.primary_cost_metric == "total_tokens"
    assert criteria.accuracy_reference_boundary == 0.1
    assert criteria.token_use_reference_boundary == 0.14
    # Absent run-size fields are optional; the report and `run` check for None rather than guess.
    assert criteria.planned_pairs is None
    assert criteria.attempts_per_cell is None


def test_criteria_loads_the_planned_run_size():
    """The pre-registered pair count and the frozen subset the run draws must be the same number."""
    criteria = Criteria.load(Path(__file__).resolve().parents[1] / "criteria.json")
    assert criteria.planned_pairs == FROZEN_SIZE
    assert criteria.attempts_per_cell == 1


# --- Intent-to-treat and the report.


def test_intent_to_treat_includes_zero_uptake():
    trials = paired_trials(n=10)
    # One treatment trial never touched c10r; it still counts.
    zero_uptake = next(t for t in trials if t["arm"] == "treatment")
    zero_uptake["metrics"]["uptake_count"] = 0

    result = analyze(trials, CRITERIA)
    assert result.token_use.n_pairs == 10
    assert result.uptake["treatment_trials_with_uptake"] == 9
    assert result.uptake["treatment_trials"] == 10


def test_report_contents_complete():
    trials = paired_trials(n=10)
    trials.append(trial("inst-infra", "baseline"))
    trials.append(trial("inst-infra", "treatment", state=INFRA_FAILURE))

    result = analyze(trials, CRITERIA)
    report = render_report(result)

    for heading in (
        "## Decision criteria",
        "## Arm summaries",
        "## Paired effect estimates",
        "## Paired accuracy outcome table",
        "## Token ratio distribution",
        "## Paired deltas",
        "## Supporting measures",
        "## Exclusions",
        "## Figures",
        "## Registered claims",
        "## Caveats",
    ):
        assert heading in report
    assert "criteria-v1" in report
    assert "inst-infra" in report and "no usable" in report and "infra-failure" in report
    assert "Historical-fix file recovery" in report


def test_report_links_every_registered_plot():
    from c10r_evals.plots import PLOT_FILENAMES

    report = render_report(analyze(paired_trials(n=10), CRITERIA))

    assert set(FIGURE_TITLES) == set(PLOT_FILENAMES)
    for name, title in FIGURE_TITLES.items():
        assert f"![{title}]({PLOT_FILENAMES[name]})" in report


def test_report_links_plots_written_outside_the_report_directory():
    links = {name: f"figures/{name}.png" for name in FIGURE_TITLES}

    report = render_report(analyze(paired_trials(n=10), CRITERIA), links)

    for name, title in FIGURE_TITLES.items():
        assert f"![{title}](figures/{name}.png)" in report


def test_report_states_the_shortfall_against_the_planned_pair_count():
    criteria = replace(CRITERIA, planned_pairs=200, attempts_per_cell=1)

    short = render_report(analyze(paired_trials(n=10), criteria))
    assert "planned pairs: 200; this report covers 10" in short
    assert "190 short of the pre-registered plan" in short
    assert "attempts per cell: 1" in short

    full = render_report(analyze(paired_trials(n=10), replace(CRITERIA, planned_pairs=10)))
    assert "planned pairs: 10; this report covers 10" in full
    assert "short of the pre-registered plan" not in full


def test_report_headline_not_uptake_conditioned():
    trials = paired_trials(n=10)
    for t in trials:
        if t["arm"] == "treatment":
            t["metrics"]["uptake_count"] = 0  # nobody used c10r at all

    result = analyze(trials, CRITERIA)
    report = render_report(result)
    assert result.token_use.n_pairs == 10  # headline still counts every pair
    assert "regardless of c10r uptake" in report
    assert "0/10 trials used c10r" in report


# --- Cache engagement: an operational diagnostic, not a registered result.


def test_cache_engagement_appears_in_report():
    result = analyze(paired_trials(n=5), CRITERIA)
    report = render_report(result)
    assert "### Cache engagement" in report
    assert f"{CACHE_SIGNAL_TURN_THRESHOLD}+ turns" in report
    assert f"{CACHE_SIGNAL_NEAR_ZERO_SHARE:.0%}" in report


def test_cache_engagement_flags_multiturn_near_zero_trial():
    """A trajectory that ran several turns yet reported near-zero cache reads is the signal
    the task exists to surface; a short trajectory with the same zero share is not flagged."""
    flagged = _pair(
        "flagged",
        baseline_metrics={"total_tokens": 1},
        treatment_metrics={
            "total_tokens": 1,
            "total_steps": CACHE_SIGNAL_TURN_THRESHOLD,
            "total_prompt_tokens": 1000.0,
            "total_cached_tokens": 0.0,
        },
    )
    too_short = _pair(
        "too-short",
        baseline_metrics={"total_tokens": 1},
        treatment_metrics={
            "total_tokens": 1,
            "total_steps": CACHE_SIGNAL_TURN_THRESHOLD - 1,
            "total_prompt_tokens": 1000.0,
            "total_cached_tokens": 0.0,
        },
    )
    engaged = _pair(
        "engaged",
        baseline_metrics={"total_tokens": 1},
        treatment_metrics={
            "total_tokens": 1,
            "total_steps": CACHE_SIGNAL_TURN_THRESHOLD,
            "total_prompt_tokens": 1000.0,
            "total_cached_tokens": 500.0,
        },
    )

    summary = _cache_engagement_summary([flagged, too_short, engaged])["treatment"]
    assert summary["near_zero_multiturn_trials"] == 1
    assert summary["reporting_trials"] == 3
    assert summary["cache_category_absent"] == 0
    assert summary["median_cached_share"] == pytest.approx(0.0)


def test_cache_engagement_absent_category_distinct_from_zero():
    """`total_cached_tokens` missing means the trajectory never reported the category, which is
    not the same claim as caching having run and returned nothing; it must not count toward the
    near-zero flag or the median cached share."""
    absent = _pair(
        "absent",
        baseline_metrics={"total_tokens": 1},
        treatment_metrics={"total_tokens": 1, "total_steps": CACHE_SIGNAL_TURN_THRESHOLD + 5},
    )
    zero = _pair(
        "zero",
        baseline_metrics={"total_tokens": 1},
        treatment_metrics={
            "total_tokens": 1,
            "total_steps": CACHE_SIGNAL_TURN_THRESHOLD + 5,
            "total_prompt_tokens": 1000.0,
            "total_cached_tokens": 0.0,
        },
    )

    summary = _cache_engagement_summary([absent, zero])["treatment"]
    assert summary["n_trials"] == 2
    assert summary["cache_category_absent"] == 1
    assert summary["reporting_trials"] == 1
    assert summary["near_zero_multiturn_trials"] == 1  # only the reporting trial is flagged
    assert summary["median_cached_share"] == pytest.approx(0.0)  # computed over the one reporting trial only


def test_no_paired_instances_names_the_cause():
    """Filtering to a subset that holds no trials is an ordinary operator state, not an arithmetic
    error. It happens whenever the analysis runs before the reported subset has any results."""
    trials = paired_trials(n=4)

    with pytest.raises(NoPairedInstances) as raised:
        analyze(trials, CRITERIA, frozen_ids={"inst-not-present"})

    message = str(raised.value)
    assert "both arms" in message
    assert "8 trial(s) considered" in message


def test_dev_instances_excluded_from_report():
    trials = paired_trials(n=10)
    trials.append(trial("inst-dev", "baseline", tokens=1))
    trials.append(trial("inst-dev", "treatment", tokens=1))
    frozen = {f"inst-{i:02d}" for i in range(10)}

    result = analyze(trials, CRITERIA, frozen_ids=frozen)
    report = render_report(result)
    assert result.token_use.n_pairs == 10
    assert result.dev_excluded == 2
    assert "inst-dev" not in report
    assert "development-subset trials excluded from this report: 2" in report


# --- Paired accuracy outcome table.


def test_paired_outcome_counts_sum_to_pairs():
    pairs = [
        _pair(
            "both-hit",
            baseline_metrics={"total_tokens": 1, "any_gold_hit": 1},
            treatment_metrics={"total_tokens": 1, "any_gold_hit": 1},
        ),
        _pair(
            "treatment-only",
            baseline_metrics={"total_tokens": 1, "any_gold_hit": 0},
            treatment_metrics={"total_tokens": 1, "any_gold_hit": 1},
        ),
        _pair(
            "baseline-only",
            baseline_metrics={"total_tokens": 1, "any_gold_hit": 1},
            treatment_metrics={"total_tokens": 1, "any_gold_hit": 0},
        ),
        _pair(
            "neither",
            baseline_metrics={"total_tokens": 1, "any_gold_hit": 0},
            treatment_metrics={"total_tokens": 1, "any_gold_hit": 0},
        ),
    ]
    outcomes = _paired_outcomes(pairs)
    assert outcomes == {"both_hit": 1, "treatment_only": 1, "baseline_only": 1, "neither_hit": 1}
    assert sum(outcomes.values()) == len(pairs)


def test_paired_outcome_counts_present_in_report():
    result = analyze(paired_trials(n=6), CRITERIA)
    report = render_report(result)
    outcomes = result.paired_outcomes
    assert sum(outcomes.values()) == len(result.pairs)
    assert "both hit" in report.lower()
    assert "treatment only" in report.lower()
    assert "baseline only" in report.lower()
    assert "neither" in report.lower()


# --- Token ratio distribution: all three summaries reported side by side.


def test_token_ratio_distribution_reports_all_three_summaries():
    pairs = [
        _pair("a", baseline_metrics={"total_tokens": 100}, treatment_metrics={"total_tokens": 130}),
        _pair("b", baseline_metrics={"total_tokens": 100}, treatment_metrics={"total_tokens": 200}),
        _pair("c", baseline_metrics={"total_tokens": 100}, treatment_metrics={"total_tokens": 50}),
    ]
    distribution = _token_ratio_summary(pairs, "total_tokens")
    assert distribution["median_ratio"] == pytest.approx(1.3)
    assert distribution["ratio_of_totals"] == pytest.approx(380 / 300)


def test_token_ratio_distribution_present_in_report():
    result = analyze(paired_trials(n=6), CRITERIA)
    report = render_report(result)
    assert result.token_ratio_median is not None
    assert result.token_ratio_of_totals is not None
    assert "median paired ratio" in report.lower()
    assert "ratio of total tokens" in report.lower()
    assert "mean paired log ratio" in report.lower()


# --- Failure summary and exploratory labelling.


def test_failure_summary_present():
    trials = paired_trials(n=4)
    trials.append(trial("inst-infra", "baseline"))
    trials.append(trial("inst-infra", "treatment", state=INFRA_FAILURE))
    failed = trial("inst-agent-fail", "treatment", state=AGENT_FAILURE, hit=0)
    trials.append(trial("inst-agent-fail", "baseline"))
    trials.append(failed)

    result = analyze(trials, CRITERIA)
    summary = result.failure_summary
    assert summary["treatment"][COMPLETED] == 4
    assert summary["treatment"][AGENT_FAILURE] == 1
    assert summary["treatment"][INFRA_FAILURE] == 1
    assert summary["baseline"][COMPLETED] == 6

    report = render_report(result)
    assert "Failure summary" in report
    assert "completed" in report.lower() and "agent-failure" in report.lower() and "infra-failure" in report.lower()


def test_exploratory_results_labelled():
    result = analyze(paired_trials(n=6), CRITERIA)
    report = render_report(result)
    assert "exploratory" in report.lower()
    # The exploratory label sits with the supporting measures, not inside the registered claims.
    claims_start = report.index("## Registered claims")
    claims_end = report.index("## Caveats")
    assert "exploratory" not in report[claims_start:claims_end].lower()


# --- Report ordering: estimates before formal claims.


def test_estimates_rendered_before_claims():
    result = analyze(paired_trials(n=6), CRITERIA)
    report = render_report(result)
    claims_index = report.index("## Registered claims")
    assert report.index("## Arm summaries") < claims_index
    assert report.index("## Paired effect estimates") < claims_index
    assert report.index("## Paired accuracy outcome table") < claims_index
    assert report.index("## Token ratio distribution") < claims_index


# --- Reproducible trajectory-level output.


def test_trajectory_level_output_reproduces_estimates():
    result = analyze(paired_trials(n=8), CRITERIA)
    emitted = trajectory_level_output(result)

    assert len(emitted["pairings"]) == len(result.pairs)
    by_key = {row["trial_key"]: row for row in emitted["trajectories"]}

    log_ratios = []
    for pairing in emitted["pairings"]:
        baseline_row = by_key[pairing["baseline_trial_key"]]
        treatment_row = by_key[pairing["treatment_trial_key"]]
        base_tokens = baseline_row["metrics"]["total_tokens"]
        treat_tokens = treatment_row["metrics"]["total_tokens"]
        log_ratios.append(math.log(treat_tokens / base_tokens))

    recomputed_theta = sum(log_ratios) / len(log_ratios)
    assert recomputed_theta == pytest.approx(result.token_use.theta)

    recomputed_hits_b = sum(
        1
        for pairing in emitted["pairings"]
        if by_key[pairing["treatment_trial_key"]]["metrics"]["any_gold_hit"]
        > by_key[pairing["baseline_trial_key"]]["metrics"]["any_gold_hit"]
    )
    recomputed_hits_c = sum(
        1
        for pairing in emitted["pairings"]
        if by_key[pairing["treatment_trial_key"]]["metrics"]["any_gold_hit"]
        < by_key[pairing["baseline_trial_key"]]["metrics"]["any_gold_hit"]
    )
    recomputed_lambda = (recomputed_hits_b - recomputed_hits_c) / len(emitted["pairings"])
    assert recomputed_lambda == pytest.approx(result.accuracy.lambda_)


def _pair(instance_id: str, baseline_metrics: dict, treatment_metrics: dict) -> Pair:
    return Pair(
        instance_id,
        {"metrics": baseline_metrics, "terminal_state": COMPLETED},
        {"metrics": treatment_metrics, "terminal_state": COMPLETED},
    )


# --- Registered task-level analyses.


def _task_level_trials(n: int = 12) -> list[dict]:
    trials = []
    for i in range(n):
        instance = f"inst-{i:02d}"
        count = 10.0 * (i + 1)  # continuous, strictly increasing: 10..120
        cue = i % 2 == 0
        steps = 5 + i  # continuous, strictly increasing baseline trajectory length
        base = trial(instance, "baseline", tokens=30_000, hit=1 if i % 3 else 0, steps=steps)
        treat = trial(instance, "treatment", tokens=15_000 + i * 500, hit=1 if i % 2 else 0, steps=steps)
        for t in (base, treat):
            t["metrics"]["source_file_count"] = count
            t["metrics"]["source_file_cue"] = 1.0 if cue else 0.0
        trials.extend([base, treat])
    return trials


def test_task_level_estimates_present():
    result = analyze(_task_level_trials(), CRITERIA)
    task_level = result.task_level
    assert set(task_level) == {"source_file_count", "source_file_cue", "baseline_trajectory_length"}

    # Repository source-file count stays continuous, on a log scale, with no bin or threshold:
    # the estimate is a regression slope over the raw per-pair values, not a group comparison.
    count_result = task_level["source_file_count"]
    assert count_result.log_covariate is True
    assert count_result.n_pairs == 12
    assert count_result.accuracy_slope is not None
    assert count_result.accuracy_slope_ci is not None
    assert count_result.token_slope is not None
    assert count_result.token_slope_ci is not None

    # The source-file cue is a genuine two-group split: both paired estimates recomputed per group.
    cue_result = task_level["source_file_cue"]
    assert set(cue_result.groups) == {True, False}
    for group in cue_result.groups.values():
        assert group["n_pairs"] > 0
        assert group["accuracy"].n_pairs == group["n_pairs"]
        assert group["token_use"].n_pairs <= group["n_pairs"]

    # Baseline trajectory length also stays continuous; no log scale is required of it.
    length_result = task_level["baseline_trajectory_length"]
    assert length_result.log_covariate is False
    assert length_result.n_pairs == 12
    assert length_result.accuracy_slope is not None


def test_task_level_split_ignores_pairs_missing_the_characteristic():
    """A pair whose covariate was never recorded is not silently folded into the `False` group."""
    trials = paired_trials(n=4)  # the plain fixture never sets source_file_cue
    result = analyze(trials, CRITERIA)
    cue_result = result.task_level["source_file_cue"]
    assert cue_result.groups[True]["n_pairs"] == 0
    assert cue_result.groups[False]["n_pairs"] == 0
    assert cue_result.groups[True]["accuracy"] is None


def test_task_level_results_labelled():
    result = analyze(_task_level_trials(), CRITERIA)
    for key in ("source_file_count", "source_file_cue", "baseline_trajectory_length"):
        assert result.task_level[key].label == TASK_LEVEL_LABEL

    report = render_report(result)
    assert "## Task-level analyses" in report
    section_start = report.index("## Task-level analyses")
    claims_start = report.index("## Registered claims")
    assert section_start < claims_start
    section = report[section_start:claims_start]
    assert "descriptive" in section.lower()
    assert "no formal claim" in section.lower()
    # The label sits with the task-level section, not folded into the registered claims.
    assert "descriptive" not in report[claims_start : report.index("## Trajectory-level output")].lower()


# --- Financial cost under a named price schedule.


def test_cost_names_schedule_and_date():
    schedule = PriceSchedule(
        name="test-schedule",
        pricing_date="2026-09-01",
        prices_usd_per_million_tokens={"total_prompt_tokens": 3.0, "total_completion_tokens": 15.0},
    )
    trials = paired_trials(n=3)
    for t in trials:
        t["metrics"]["total_prompt_tokens"] = t["metrics"]["total_tokens"] * 0.8
        t["metrics"]["total_completion_tokens"] = t["metrics"]["total_tokens"] * 0.2

    result = analyze(trials, CRITERIA, price_schedule=schedule)
    fc = result.financial_cost
    assert fc.schedule_name == "test-schedule"
    assert fc.pricing_date == "2026-09-01"
    assert fc.baseline_unavailable == 0
    assert fc.treatment_unavailable == 0
    assert fc.baseline_mean_cost_usd == pytest.approx(
        statistics.fmean(
            t["metrics"]["total_prompt_tokens"] * 3.0 / 1_000_000
            + t["metrics"]["total_completion_tokens"] * 15.0 / 1_000_000
            for t in trials
            if t["arm"] == "baseline"
        )
    )

    report = render_report(result)
    assert "test-schedule" in report
    assert "2026-09-01" in report


def test_missing_category_marks_unavailable():
    schedule = PriceSchedule(
        name="test-schedule",
        pricing_date="2026-09-01",
        prices_usd_per_million_tokens={"total_prompt_tokens": 3.0, "total_reasoning_tokens": 20.0},
    )
    # A category the schedule prices but the trial never reported is absent, not zero.
    assert trajectory_cost({"total_prompt_tokens": 1000.0}, schedule) is None
    assert trajectory_cost({"total_prompt_tokens": 1000.0, "total_reasoning_tokens": 0.0}, schedule) == pytest.approx(
        1000.0 * 3.0 / 1_000_000
    )

    trials = [trial("inst-1", "baseline", tokens=30_000), trial("inst-1", "treatment", tokens=15_000)]
    for t in trials:
        t["metrics"]["total_prompt_tokens"] = t["metrics"]["total_tokens"] * 0.8
        # total_reasoning_tokens intentionally left unset, as an unreporting harness would.

    result = analyze(trials, CRITERIA, price_schedule=schedule)
    fc = result.financial_cost
    assert fc.baseline_mean_cost_usd is None
    assert fc.treatment_mean_cost_usd is None
    assert fc.baseline_unavailable == 1
    assert fc.treatment_unavailable == 1

    report = render_report(result)
    assert "unavailable" in report.lower()


def test_financial_cost_absent_without_schedule():
    result = analyze(paired_trials(n=3), CRITERIA)
    assert result.financial_cost is None
    assert result.price_schedule is None
    assert "no price schedule" in render_report(result).lower()
