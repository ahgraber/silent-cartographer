"""Paired analysis: pair assembly under the failure policy, decision criteria, and outcome computation.

Consumes trial records in the shape `telemetry.query_trials` returns, so the analysis
never re-parses raw trajectories.
"""

import json
import math
import random
import statistics
from dataclasses import dataclass, field
from pathlib import Path

from c10r_evals.criteria import Criteria
from c10r_evals.stats import (
    AccuracyClaims,
    AccuracyEstimate,
    TokenRatioEstimate,
    TokenUseClaims,
    accuracy_claims,
    accuracy_estimate,
    token_ratio_estimate,
    token_use_claims,
)
from c10r_evals.telemetry import AGENT_FAILURE, COMPLETED, INFRA_FAILURE

USABLE_STATES = {COMPLETED, AGENT_FAILURE}
TERMINAL_STATES = (COMPLETED, AGENT_FAILURE, INFRA_FAILURE)

SECONDARY_COST_METRICS = ("total_cost_usd", "total_steps", "wall_clock_s")

# Fixed operational thresholds for the cache-engagement signal (see `_cache_engagement_summary`):
# a trajectory of at least this many turns is "several", and a cached share at or below this
# fraction of prompt tokens is "near-zero". Both are stated constants, not derived from any run's
# data, so the signal reads the same way across runs.
CACHE_SIGNAL_TURN_THRESHOLD = 5
CACHE_SIGNAL_NEAR_ZERO_SHARE = 0.01


class NoPairedInstances(Exception):
    """No instance has a scorable trial in both arms, so no paired estimate is defined."""


@dataclass(frozen=True)
class PriceSchedule:
    """A named, dated USD-per-million-token price table for deriving per-trajectory financial cost.

    Every priced category must be present for cost computation. Price schedules load from dated
    external JSON; no default table is bundled.
    """

    name: str
    pricing_date: str
    prices_usd_per_million_tokens: dict

    @classmethod
    def load(cls, path: Path) -> "PriceSchedule":
        raw = json.loads(path.read_text())
        required = ("name", "pricing_date", "prices_usd_per_million_tokens")
        missing = [key for key in required if key not in raw]
        if missing:
            raise ValueError(f"price schedule {path} is missing required keys: {missing}")
        return cls(
            name=raw["name"],
            pricing_date=raw["pricing_date"],
            prices_usd_per_million_tokens={k: float(v) for k, v in raw["prices_usd_per_million_tokens"].items()},
        )


def trajectory_cost(metrics: dict, schedule: PriceSchedule) -> float | None:
    """Per-trajectory financial cost under `schedule`, or `None` when a priced category is unrecorded."""
    total = 0.0
    for category, price_per_million in schedule.prices_usd_per_million_tokens.items():
        if category not in metrics:
            return None
        total += metrics[category] * price_per_million / 1_000_000.0
    return total


@dataclass(frozen=True)
class FinancialCostSummary:
    """Arm-level per-trajectory financial cost under one named, dated price schedule.

    `*_unavailable` counts trajectories whose cost could not be computed because a required
    token category was unrecorded; those trajectories are excluded from `*_mean_cost_usd`
    rather than imputed as zero.
    """

    schedule_name: str
    pricing_date: str
    baseline_mean_cost_usd: float | None
    treatment_mean_cost_usd: float | None
    baseline_unavailable: int
    treatment_unavailable: int
    n_trials: dict


def _financial_cost_summary(pairs: list["Pair"], schedule: PriceSchedule) -> FinancialCostSummary:
    def arm_costs(trials: list[dict]) -> tuple[float | None, int]:
        costs = [trajectory_cost(t["metrics"], schedule) for t in trials]
        available = [c for c in costs if c is not None]
        return (statistics.fmean(available) if available else None), (len(costs) - len(available))

    baseline_trials = [p.baseline for p in pairs]
    treatment_trials = [p.treatment for p in pairs]
    baseline_mean, baseline_unavailable = arm_costs(baseline_trials)
    treatment_mean, treatment_unavailable = arm_costs(treatment_trials)
    return FinancialCostSummary(
        schedule_name=schedule.name,
        pricing_date=schedule.pricing_date,
        baseline_mean_cost_usd=baseline_mean,
        treatment_mean_cost_usd=treatment_mean,
        baseline_unavailable=baseline_unavailable,
        treatment_unavailable=treatment_unavailable,
        n_trials={"baseline": len(baseline_trials), "treatment": len(treatment_trials)},
    )


@dataclass(frozen=True)
class Pair:
    instance_id: str
    baseline: dict
    treatment: dict
    # Which recorded attempt each arm's number came from, and which repeats it replaced,
    # so a pair stays traceable when the instruction set or the endpoint is iterated on.
    provenance: dict = field(default_factory=dict)

    def delta(self, metric: str) -> float | None:
        base = self.baseline["metrics"].get(metric)
        treat = self.treatment["metrics"].get(metric)
        if base is None or treat is None:
            return None
        return float(treat) - float(base)


TASK_LEVEL_LABEL = "descriptive; supports no formal claim"


@dataclass(frozen=True)
class TaskLevelRegression:
    """A continuous, unbinned task characteristic's association with both paired estimates.

    Reports an ordinary-least-squares slope of the per-pair accuracy delta (and, separately, the
    per-pair log token ratio) on the covariate, with a percentile-bootstrap interval over pairs.
    `log_covariate` records whether the covariate is log-transformed.
    """

    covariate: str
    log_covariate: bool
    n_pairs: int
    accuracy_slope: float | None
    accuracy_slope_ci: tuple[float, float] | None
    token_slope: float | None
    token_slope_ci: tuple[float, float] | None
    label: str = TASK_LEVEL_LABEL


@dataclass(frozen=True)
class TaskLevelSplit:
    """Headline accuracy and token-ratio estimators recomputed for each boolean characteristic level."""

    covariate: str
    groups: dict
    label: str = TASK_LEVEL_LABEL


def _percentile(sorted_values: list[float], p: float) -> float:
    idx = min(len(sorted_values) - 1, max(0, int(p * len(sorted_values))))
    return sorted_values[idx]


def _regression_slope_ci(
    xs: list[float], ys: list[float], *, resamples: int = 2000, seed: int = 0
) -> tuple[float, tuple[float, float] | None] | None:
    """OLS slope of `ys` on `xs`, with a two-sided 95% percentile-bootstrap interval over pairs."""
    n = len(xs)
    if n < 3 or len(set(xs)) < 2:
        return None
    slope, _intercept = statistics.linear_regression(xs, ys)
    rng = random.Random(seed)
    slopes = []
    for _ in range(resamples):
        idx = [rng.randrange(n) for _ in range(n)]
        sx = [xs[i] for i in idx]
        if len(set(sx)) < 2:
            continue
        sy = [ys[i] for i in idx]
        boot_slope, _ = statistics.linear_regression(sx, sy)
        slopes.append(boot_slope)
    if not slopes:
        return slope, None
    slopes.sort()
    return slope, (_percentile(slopes, 0.025), _percentile(slopes, 0.975))


def _pair_covariate(pair: "Pair", key: str, arm: str = "baseline") -> float | None:
    trial = pair.baseline if arm == "baseline" else pair.treatment
    value = trial["metrics"].get(key)
    return None if value is None else float(value)


def _accuracy_delta(pair: "Pair") -> float:
    return float(pair.treatment["metrics"].get("any_gold_hit", 0)) - float(
        pair.baseline["metrics"].get("any_gold_hit", 0)
    )


def _token_log_ratio(pair: "Pair", metric: str) -> float | None:
    base = pair.baseline["metrics"].get(metric)
    treat = pair.treatment["metrics"].get(metric)
    if base and treat and base > 0 and treat > 0:
        return math.log(treat / base)
    return None


def _task_level_continuous(
    pairs: list["Pair"], covariate_key: str, cost_metric: str, *, log_covariate: bool, arm: str = "baseline"
) -> TaskLevelRegression:
    xs_acc, ys_acc, xs_tok, ys_tok = [], [], [], []
    n_pairs = 0
    for pair in pairs:
        raw = _pair_covariate(pair, covariate_key, arm)
        if raw is None or (log_covariate and raw <= 0):
            continue
        n_pairs += 1
        x = math.log(raw) if log_covariate else raw
        xs_acc.append(x)
        ys_acc.append(_accuracy_delta(pair))
        ratio = _token_log_ratio(pair, cost_metric)
        if ratio is not None:
            xs_tok.append(x)
            ys_tok.append(ratio)
    accuracy_result = _regression_slope_ci(xs_acc, ys_acc)
    token_result = _regression_slope_ci(xs_tok, ys_tok)
    return TaskLevelRegression(
        covariate=covariate_key,
        log_covariate=log_covariate,
        n_pairs=n_pairs,
        accuracy_slope=accuracy_result[0] if accuracy_result else None,
        accuracy_slope_ci=accuracy_result[1] if accuracy_result else None,
        token_slope=token_result[0] if token_result else None,
        token_slope_ci=token_result[1] if token_result else None,
    )


def _discordant_counts(pairs: list["Pair"]) -> tuple[int, int]:
    """(b, c): pairs where treatment hit and baseline missed, and the reverse."""
    b = sum(
        1
        for p in pairs
        if p.treatment["metrics"].get("any_gold_hit", 0) > p.baseline["metrics"].get("any_gold_hit", 0)
    )
    c = sum(
        1
        for p in pairs
        if p.treatment["metrics"].get("any_gold_hit", 0) < p.baseline["metrics"].get("any_gold_hit", 0)
    )
    return b, c


def _task_level_split(
    pairs: list["Pair"], covariate_key: str, cost_metric: str, confidence_level: float
) -> TaskLevelSplit:
    # A pair whose covariate was never recorded belongs in neither group; only `bool()`-ing a
    # present value decides which one, so it is not silently folded into the `False` group.
    by_flag: dict[bool, list[Pair]] = {True: [], False: []}
    for pair in pairs:
        value = _pair_covariate(pair, covariate_key)
        if value is not None:
            by_flag[bool(value)].append(pair)

    groups: dict = {}
    for flag, subset in by_flag.items():
        if not subset:
            groups[flag] = {"n_pairs": 0, "accuracy": None, "token_use": None}
            continue
        b, c = _discordant_counts(subset)
        groups[flag] = {
            "n_pairs": len(subset),
            "accuracy": accuracy_estimate(b, c, len(subset), confidence_level=confidence_level),
            "token_use": token_ratio_estimate(_log_ratios(subset, cost_metric), confidence_level=confidence_level),
        }
    return TaskLevelSplit(covariate=covariate_key, groups=groups)


@dataclass
class PairedAnalysis:
    pairs: list[Pair]
    exclusions: list[dict]
    dev_excluded: int
    accuracy: AccuracyEstimate
    accuracy_claims: AccuracyClaims
    token_use: TokenRatioEstimate
    token_use_claims: TokenUseClaims
    token_ratio_median: float | None
    token_ratio_of_totals: float | None
    paired_outcomes: dict
    arm_summaries: dict
    uptake: dict
    cache_engagement: dict
    failure_summary: dict
    criteria: Criteria
    task_level: dict = field(default_factory=dict)
    price_schedule: PriceSchedule | None = None
    financial_cost: FinancialCostSummary | None = None
    excluded_metrics_pairs: dict = field(default_factory=dict)


def _attempt_order(trial: dict) -> tuple:
    """Sort key placing the most recent attempt last; falls back to the attempt name."""
    return (trial.get("started_at") or 0, str(trial.get("attempt") or ""))


def select_attempt(trials: list[dict]) -> tuple[dict | None, list[dict]]:
    """Choose the attempt that represents an (instance, arm), and return the rest.

    The latest usable attempt wins, which is what a bounded rerun means: a rerun exists
    because the attempt before it did not reach a state the analysis can score. An
    instance is only lost when no attempt reached one.
    """
    usable = sorted((t for t in trials if t["terminal_state"] in USABLE_STATES), key=_attempt_order)
    if not usable:
        return None, []
    return usable[-1], usable[:-1]


class MixedModelError(Exception):
    """The trials span more than one model, so they do not describe one comparison."""


def assemble_pairs(trials: list[dict], frozen_ids: set[str] | None = None) -> tuple[list[Pair], list[dict], int]:
    """Join trials per instance across arms; agent failures pair as outcomes, infra failures exclude.

    Raises `MixedModelError` when trials span multiple models.
    """
    models = {t["params"].get("model") for t in trials if t.get("params")}
    models.discard(None)
    if len(models) > 1:
        raise MixedModelError(
            f"trials span {len(models)} models ({', '.join(sorted(models))}); "
            "select one with the analysis `--model` option"
        )
    dev_excluded = 0
    by_cell: dict[str, dict[str, list[dict]]] = {}
    seen_states: dict[str, set[str]] = {}
    for trial in trials:
        instance = trial["instance_id"]
        if frozen_ids is not None and instance not in frozen_ids:
            dev_excluded += 1
            continue
        by_cell.setdefault(instance, {}).setdefault(trial["arm"], []).append(trial)
        seen_states.setdefault(instance, set()).add(trial["terminal_state"])

    pairs: list[Pair] = []
    exclusions: list[dict] = []
    for instance in sorted(by_cell):
        arms = by_cell[instance]
        chosen: dict[str, dict] = {}
        superseded: dict[str, list[dict]] = {}
        for arm in ("baseline", "treatment"):
            selected, rest = select_attempt(arms.get(arm, []))
            if selected is not None:
                chosen[arm] = selected
                superseded[arm] = rest

        missing = [arm for arm in ("baseline", "treatment") if arm not in chosen]
        if missing:
            unusable = sorted(s for s in seen_states[instance] if s not in USABLE_STATES)
            reason = f"no usable {' and '.join(missing)} trial"
            if unusable:
                reason += f" ({', '.join(unusable)})"
            exclusions.append({"instance_id": instance, "reason": reason})
            continue

        pairs.append(
            Pair(
                instance,
                chosen["baseline"],
                chosen["treatment"],
                provenance={
                    arm: {
                        "attempt": chosen[arm].get("attempt"),
                        "trial_key": chosen[arm].get("trial_key"),
                        "instruction_set_version": chosen[arm].get("instruction_set_version"),
                        "superseded": [t.get("attempt") for t in superseded.get(arm, [])],
                    }
                    for arm in ("baseline", "treatment")
                },
            )
        )
    return pairs, exclusions, dev_excluded


def _arm_summary(trials: list[dict], metrics: tuple[str, ...]) -> dict:
    summary: dict = {"n_trials": len(trials)}
    for metric in metrics:
        values = [t["metrics"][metric] for t in trials if metric in t["metrics"]]
        if values:
            summary[metric] = {"mean": statistics.fmean(values), "median": statistics.median(values)}
    return summary


def _uptake_summary(pairs: list[Pair]) -> dict:
    treatment = [p.treatment for p in pairs]
    baseline = [p.baseline for p in pairs]

    def counts(trials: list[dict], key: str) -> list[float]:
        return [t["metrics"].get(key, 0.0) for t in trials]

    treatment_uptake = counts(treatment, "uptake_count")
    with_uptake = sum(1 for u in treatment_uptake if u > 0)
    baseline_search = counts(baseline, "search_count")
    treatment_search = counts(treatment, "search_count")
    return {
        "treatment_mean_uptake": statistics.fmean(treatment_uptake) if treatment_uptake else 0.0,
        "treatment_trials_with_uptake": with_uptake,
        "treatment_trials": len(treatment),
        "baseline_mean_search": statistics.fmean(baseline_search) if baseline_search else 0.0,
        "treatment_mean_search": statistics.fmean(treatment_search) if treatment_search else 0.0,
        "search_displacement": (statistics.fmean(baseline_search) - statistics.fmean(treatment_search))
        if baseline_search and treatment_search
        else 0.0,
    }


def _cache_engagement_arm(trials: list[dict]) -> dict:
    """Cache engagement for one arm's trials.

    An unreported `total_cached_tokens` category is excluded from cached-share and near-zero
    summaries and counted as `cache_category_absent`.
    """
    shares = []
    absent = 0
    near_zero_multiturn = 0
    for trial in trials:
        metrics = trial["metrics"]
        if "total_cached_tokens" not in metrics:
            absent += 1
            continue
        prompt_tokens = metrics.get("total_prompt_tokens")
        if prompt_tokens is None or prompt_tokens <= 0:
            continue
        share = metrics["total_cached_tokens"] / prompt_tokens
        shares.append(share)
        turns = metrics.get("total_steps", 0)
        if turns >= CACHE_SIGNAL_TURN_THRESHOLD and share <= CACHE_SIGNAL_NEAR_ZERO_SHARE:
            near_zero_multiturn += 1
    return {
        "n_trials": len(trials),
        "cache_category_absent": absent,
        "reporting_trials": len(shares),
        "median_cached_share": statistics.median(shares) if shares else None,
        "near_zero_multiturn_trials": near_zero_multiturn,
    }


def _cache_engagement_summary(pairs: list[Pair]) -> dict:
    """Return diagnostic cache-engagement summaries for both arms."""
    return {
        "baseline": _cache_engagement_arm([p.baseline for p in pairs]),
        "treatment": _cache_engagement_arm([p.treatment for p in pairs]),
    }


def _log_ratios(pairs: list[Pair], metric: str) -> list[float]:
    """Per-instance log(treatment / baseline) for a positive cost metric."""
    ratios = []
    for pair in pairs:
        base = pair.baseline["metrics"].get(metric)
        treat = pair.treatment["metrics"].get(metric)
        if base and treat and base > 0 and treat > 0:
            ratios.append(math.log(treat / base))
    return ratios


def _paired_outcomes(pairs: list[Pair]) -> dict:
    """The four paired hit/miss outcome counts behind the accuracy estimate."""
    outcomes = {"both_hit": 0, "treatment_only": 0, "baseline_only": 0, "neither_hit": 0}
    for pair in pairs:
        treatment_hit = bool(pair.treatment["metrics"].get("any_gold_hit", 0))
        baseline_hit = bool(pair.baseline["metrics"].get("any_gold_hit", 0))
        if treatment_hit and baseline_hit:
            outcomes["both_hit"] += 1
        elif treatment_hit:
            outcomes["treatment_only"] += 1
        elif baseline_hit:
            outcomes["baseline_only"] += 1
        else:
            outcomes["neither_hit"] += 1
    return outcomes


def _token_ratio_summary(pairs: list[Pair], metric: str) -> dict:
    """The median paired ratio and the ratio of total tokens, alongside the mean paired log ratio.

    Each answers a different question: the median describes the middle issue, and the ratio of
    totals describes the sampled workload's billable volume, where large trajectories dominate.
    """
    ratios = []
    treatment_total = 0.0
    baseline_total = 0.0
    have_totals = False
    for pair in pairs:
        base = pair.baseline["metrics"].get(metric)
        treat = pair.treatment["metrics"].get(metric)
        if base and treat and base > 0 and treat > 0:
            ratios.append(treat / base)
        if base is not None and treat is not None:
            baseline_total += base
            treatment_total += treat
            have_totals = True
    return {
        "median_ratio": statistics.median(ratios) if ratios else None,
        "ratio_of_totals": (treatment_total / baseline_total) if have_totals and baseline_total > 0 else None,
    }


def _failure_summary(trials: list[dict], frozen_ids: set[str] | None) -> dict:
    """Per-arm counts of each terminal state, from the latest attempt of every (instance, arm) cell.

    Every attempt counts here, including one that never reached a usable state, so an
    infra-failure that the retry policy did not recover still shows up beside the exclusion
    list it produced.
    """
    by_cell: dict[tuple[str, str], list[dict]] = {}
    for t in trials:
        instance = t["instance_id"]
        if frozen_ids is not None and instance not in frozen_ids:
            continue
        by_cell.setdefault((instance, t["arm"]), []).append(t)

    counts = {"baseline": dict.fromkeys(TERMINAL_STATES, 0), "treatment": dict.fromkeys(TERMINAL_STATES, 0)}
    for (_instance, arm), cell_trials in by_cell.items():
        latest = max(cell_trials, key=_attempt_order)
        state = latest["terminal_state"]
        counts[arm][state] = counts[arm].get(state, 0) + 1
    return counts


def trajectory_level_output(result: PairedAnalysis) -> dict:
    """Per-trajectory measures and per-instance pairings behind every reported estimate.

    A reader can recompute the paired estimates directly from this structure, or apply a
    different reference boundary, without rerunning the experiment. Emitted as one JSON
    document: a `trajectories` row per selected trial carries its full recorded metrics under
    an open-ended key set, and a `pairings` row per paired instance names the trial key each
    arm contributed, joining back to `trajectories`.
    """
    trajectories = []
    pairings = []
    for pair in result.pairs:
        for arm, trial in (("baseline", pair.baseline), ("treatment", pair.treatment)):
            cost = (
                trajectory_cost(trial["metrics"], result.price_schedule) if result.price_schedule is not None else None
            )
            trajectories.append(
                {
                    "instance_id": pair.instance_id,
                    "arm": arm,
                    "terminal_state": trial["terminal_state"],
                    "attempt": trial.get("attempt"),
                    "trial_key": trial.get("trial_key"),
                    "metrics": trial["metrics"],
                    "financial_cost_usd": cost,
                }
            )
        pairings.append(
            {
                "instance_id": pair.instance_id,
                "baseline_trial_key": pair.baseline.get("trial_key"),
                "treatment_trial_key": pair.treatment.get("trial_key"),
            }
        )
    return {"trajectories": trajectories, "pairings": pairings}


def analyze(
    trials: list[dict],
    criteria: Criteria,
    frozen_ids: set[str] | None = None,
    price_schedule: PriceSchedule | None = None,
) -> PairedAnalysis:
    pairs, exclusions, dev_excluded = assemble_pairs(trials, frozen_ids)
    if not pairs:
        # Every estimate is defined over paired instances, so without one there is nothing to
        # report. Saying so here keeps the cause visible; the alternative surfaces as an
        # arithmetic error from inside the interval solver, which names neither the filters
        # applied nor the trials they discarded.
        raise NoPairedInstances(
            f"no instance has a scorable trial in both arms: {len(trials)} trial(s) considered, "
            f"{len(exclusions)} excluded, {dev_excluded} outside the reported subset"
        )

    metric = criteria.primary_cost_metric
    token_use = token_ratio_estimate(_log_ratios(pairs, metric), confidence_level=criteria.confidence_level)
    token_claims = token_use_claims(token_use, criteria.token_use_reference_boundary)

    b, c = _discordant_counts(pairs)
    accuracy = accuracy_estimate(b, c, len(pairs), confidence_level=criteria.confidence_level)
    acc_claims = accuracy_claims(accuracy, criteria.accuracy_reference_boundary)

    task_level = {
        "source_file_count": _task_level_continuous(
            pairs, "source_file_count", metric, log_covariate=True, arm="baseline"
        ),
        "source_file_cue": _task_level_split(pairs, "source_file_cue", metric, criteria.confidence_level),
        "baseline_trajectory_length": _task_level_continuous(
            pairs, "total_steps", metric, log_covariate=False, arm="baseline"
        ),
    }
    financial_cost = _financial_cost_summary(pairs, price_schedule) if price_schedule is not None else None

    metric_names = (
        criteria.primary_cost_metric,
        *SECONDARY_COST_METRICS,
        "reward",
        "precision",
        "recall",
        "f1",
        "any_gold_hit",
        "uptake_count",
        "search_count",
    )
    arm_summaries = {
        "baseline": _arm_summary([p.baseline for p in pairs], metric_names),
        "treatment": _arm_summary([p.treatment for p in pairs], metric_names),
    }
    token_distribution = _token_ratio_summary(pairs, metric)

    return PairedAnalysis(
        pairs=pairs,
        exclusions=exclusions,
        dev_excluded=dev_excluded,
        accuracy=accuracy,
        accuracy_claims=acc_claims,
        token_use=token_use,
        token_use_claims=token_claims,
        token_ratio_median=token_distribution["median_ratio"],
        token_ratio_of_totals=token_distribution["ratio_of_totals"],
        paired_outcomes=_paired_outcomes(pairs),
        arm_summaries=arm_summaries,
        uptake=_uptake_summary(pairs),
        cache_engagement=_cache_engagement_summary(pairs),
        failure_summary=_failure_summary(trials, frozen_ids),
        criteria=criteria,
        task_level=task_level,
        price_schedule=price_schedule,
        financial_cost=financial_cost,
    )
