"""Paired analysis: pair assembly under the failure policy, decision criteria, and outcome computation.

Consumes trial records in the shape `telemetry.query_trials` returns, so the analysis
never re-parses raw trajectories.
"""

import json
import statistics
from dataclasses import dataclass, field
from pathlib import Path

from c10r_evals.stats import (
    CostSuperiorityResult,
    NonInferiorityResult,
    cost_superiority,
    non_inferiority,
)
from c10r_evals.telemetry import AGENT_FAILURE, COMPLETED

USABLE_STATES = {COMPLETED, AGENT_FAILURE}

CRITERIA_REQUIRED_KEYS = (
    "version",
    "primary_cost_metric",
    "non_inferiority_margin",
    "confidence_level",
    "failure_policy",
)

SECONDARY_COST_METRICS = ("total_cost_usd", "total_steps", "wall_clock_s")


@dataclass(frozen=True)
class Criteria:
    """Pre-declared decision criteria; required input for any reported analysis."""

    version: str
    primary_cost_metric: str
    non_inferiority_margin: float
    confidence_level: float
    failure_policy: dict

    @classmethod
    def load(cls, path: Path) -> "Criteria":
        raw = json.loads(path.read_text())
        missing = [key for key in CRITERIA_REQUIRED_KEYS if key not in raw]
        if missing:
            raise ValueError(f"decision-criteria file {path} is missing required keys: {missing}")
        return cls(
            version=raw["version"],
            primary_cost_metric=raw["primary_cost_metric"],
            non_inferiority_margin=float(raw["non_inferiority_margin"]),
            confidence_level=float(raw["confidence_level"]),
            failure_policy=raw["failure_policy"],
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


@dataclass
class PairedAnalysis:
    pairs: list[Pair]
    exclusions: list[dict]
    dev_excluded: int
    cost: CostSuperiorityResult
    secondary_costs: list[CostSuperiorityResult]
    accuracy: NonInferiorityResult
    arm_summaries: dict
    uptake: dict
    break_even: dict
    criteria: Criteria
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


def assemble_pairs(trials: list[dict], frozen_ids: set[str] | None = None) -> tuple[list[Pair], list[dict], int]:
    """Join trials per instance across arms; agent failures pair as outcomes, infra failures exclude."""
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


def _break_even(pairs: list[Pair], build_manifest_entries: list[dict] | None) -> dict:
    """Episodes needed for per-episode wall-clock savings to cover the one-time index build cost."""
    if not build_manifest_entries:
        return {"available": False, "reason": "no build manifest"}
    index_wall = statistics.fmean(e["wall_clock_s"] for e in build_manifest_entries)
    index_bytes = statistics.fmean(e["index_bytes"] for e in build_manifest_entries)
    wall_deltas = [d for d in (p.delta("wall_clock_s") for p in pairs) if d is not None]
    if not wall_deltas:
        return {
            "available": False,
            "reason": "no paired wall-clock measurements",
            "mean_index_wall_clock_s": index_wall,
            "mean_index_bytes": index_bytes,
        }
    per_episode_saving = -statistics.fmean(wall_deltas)  # positive when treatment is faster
    result = {
        "available": True,
        "mean_index_wall_clock_s": index_wall,
        "mean_index_bytes": index_bytes,
        "per_episode_wall_saving_s": per_episode_saving,
    }
    if per_episode_saving > 0:
        result["break_even_episodes"] = index_wall / per_episode_saving
    else:
        result["break_even_episodes"] = None  # no per-episode saving: the index never amortizes
    return result


def analyze(
    trials: list[dict],
    criteria: Criteria,
    frozen_ids: set[str] | None = None,
    build_manifest_entries: list[dict] | None = None,
) -> PairedAnalysis:
    pairs, exclusions, dev_excluded = assemble_pairs(trials, frozen_ids)

    primary_deltas = [d for d in (p.delta(criteria.primary_cost_metric) for p in pairs) if d is not None]
    cost = cost_superiority(primary_deltas, criteria.primary_cost_metric)
    secondary = []
    for metric in SECONDARY_COST_METRICS:
        deltas = [d for d in (p.delta(metric) for p in pairs) if d is not None]
        if deltas:
            secondary.append(cost_superiority(deltas, metric))

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
    accuracy = non_inferiority(b, c, len(pairs), criteria.non_inferiority_margin, criteria.confidence_level)

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

    return PairedAnalysis(
        pairs=pairs,
        exclusions=exclusions,
        dev_excluded=dev_excluded,
        cost=cost,
        secondary_costs=secondary,
        accuracy=accuracy,
        arm_summaries=arm_summaries,
        uptake=_uptake_summary(pairs),
        break_even=_break_even(pairs, build_manifest_entries),
        criteria=criteria,
    )
