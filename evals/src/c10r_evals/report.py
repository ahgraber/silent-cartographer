"""Markdown report rendering for a completed paired analysis."""

from c10r_evals.analysis import CACHE_SIGNAL_NEAR_ZERO_SHARE, CACHE_SIGNAL_TURN_THRESHOLD, PairedAnalysis
from c10r_evals.telemetry import AGENT_FAILURE, COMPLETED, INFRA_FAILURE

FIGURE_TITLES = {
    "paired-accuracy": "Paired localization accuracy",
    "token-ratio-distribution": "Paired token-ratio distribution",
    "token-summaries": "Comparative token summaries",
    "turn-size": "Turn count and average turn size",
}

CAVEATS = """\
- **Historical-fix file recovery, not localization correctness.** Accuracy grades recovery of the
  files the historical fix changed; a valid alternative localization scores as a miss, and this
  proxy error is not guaranteed symmetric across arms.
- **Contamination attenuates.** Models may localize memorized repositories from memory, which
  shrinks the measurable treatment effect; observed effects read as conservative.
- **The estimand is the treatment bundle.** Results attribute effects to c10r plus its instruction
  prompt, never to the tool alone.
"""


def _fmt(value: float | None, digits: int = 3) -> str:
    if value is None:
        return "n/a"
    return f"{value:.{digits}f}"


def _fmt_slope(slope: float | None, ci: tuple[float, float] | None, digits: int = 4) -> str:
    if slope is None:
        return "n/a (fewer than 3 pairs, or no variation in the covariate)"
    if ci is None:
        return f"{_fmt(slope, digits)} (bootstrap interval unavailable)"
    return f"{_fmt(slope, digits)} (95% bootstrap interval [{_fmt(ci[0], digits)}, {_fmt(ci[1], digits)}])"


def _fmt_cost(value: float | None) -> str:
    return "unavailable" if value is None else f"${value:.4f}"


def _append_decision_criteria(lines: list[str], result: PairedAnalysis) -> None:
    criteria = result.criteria
    add = lines.append
    add("## Decision criteria")
    add("")
    add(f"- criteria version: `{criteria.version}`")
    add(f"- primary cost metric: `{criteria.primary_cost_metric}`")
    add(
        f"- accuracy reference boundary: {criteria.accuracy_reference_boundary} "
        "(percentage points of any-gold-file hit rate)"
    )
    add(
        f"- token-use reference boundary: {criteria.token_use_reference_boundary} "
        "(proportional, so a ratio boundary of 1 + this value)"
    )
    add(f"- confidence level (one-sided): {criteria.confidence_level}")
    if criteria.attempts_per_cell is not None:
        add(f"- attempts per cell: {criteria.attempts_per_cell}")
    if criteria.planned_pairs is not None:
        reported = len(result.pairs)
        shortfall = criteria.planned_pairs - reported
        line = f"- planned pairs: {criteria.planned_pairs}; this report covers {reported}"
        if shortfall > 0:
            line += f", **{shortfall} short of the pre-registered plan**"
        add(line)
    add(f"- failure policy: {criteria.failure_policy}")
    add("")
    add(
        "The report estimates the accuracy and token-use effects and evaluates the registered claims "
        "independently beside them. It issues no combined verdict across the two axes."
    )
    add("")


def _append_primary_results(lines: list[str], result: PairedAnalysis) -> None:
    criteria = result.criteria
    add = lines.append
    add("## Arm summaries")
    add("")
    add("| metric | baseline mean | baseline median | treatment mean | treatment median |")
    add("| --- | --- | --- | --- | --- |")
    baseline_summary = result.arm_summaries["baseline"]
    treatment_summary = result.arm_summaries["treatment"]
    metrics = [k for k in baseline_summary if k != "n_trials"]
    for metric in metrics:
        base = baseline_summary.get(metric)
        treat = treatment_summary.get(metric)
        add(
            f"| {metric} | {_fmt(base['mean'] if base else None)} | {_fmt(base['median'] if base else None)} "
            f"| {_fmt(treat['mean'] if treat else None)} | {_fmt(treat['median'] if treat else None)} |"
        )
    add("")

    add("## Paired effect estimates (intent-to-treat)")
    add("")
    acc = result.accuracy
    add(
        f"- accuracy: paired hit-rate difference lambda = {_fmt(acc.lambda_)} (treatment - baseline), "
        f"two-sided 95% score interval [{_fmt(acc.ci_lower)}, {_fmt(acc.ci_upper)}], "
        f"discordant pairs b = {acc.b}, c = {acc.c}, n = {acc.n_pairs}"
    )
    token = result.token_use
    add(
        f"- token use on `{criteria.primary_cost_metric}`: geometric mean paired ratio = {_fmt(token.ratio, 2)}x "
        f"(mean paired log ratio theta = {_fmt(token.theta)}), "
        f"two-sided 95% bootstrap interval [{_fmt(token.ratio_ci_lower, 2)}x, {_fmt(token.ratio_ci_upper, 2)}x], "
        f"n = {token.n_pairs} pairs"
    )
    add("- every pair counts regardless of c10r uptake; agent failures score as outcomes.")
    add("")
    add("Both estimates are reported whether or not any registered claim below is supported.")
    add("")

    add("## Paired accuracy outcome table")
    add("")
    outcomes = result.paired_outcomes
    add("| outcome | count |")
    add("| --- | --- |")
    add(f"| both hit | {outcomes['both_hit']} |")
    add(f"| treatment only hit | {outcomes['treatment_only']} |")
    add(f"| baseline only hit | {outcomes['baseline_only']} |")
    add(f"| neither hit | {outcomes['neither_hit']} |")
    add("")

    add("## Token ratio distribution")
    add("")
    add("Three summaries of the same paired ratios, each answering a different question.")
    add("")
    add("| summary | value | question answered |")
    add("| --- | --- | --- |")
    add(
        f"| mean paired log ratio (geometric mean) | {_fmt(token.ratio, 2)}x | "
        "proportional change per trajectory, equal weight per issue |"
    )
    add(f"| median paired ratio | {_fmt(result.token_ratio_median, 2)}x | what happened on the middle issue |")
    add(
        f"| ratio of total tokens | {_fmt(result.token_ratio_of_totals, 2)}x | "
        "billable volume for this sampled workload, large trajectories weigh more |"
    )
    add("")

    add("## Paired deltas")
    add("")
    add(f"| instance | {criteria.primary_cost_metric} delta | hit delta | baseline state | treatment state |")
    add("| --- | --- | --- | --- | --- |")
    for pair in result.pairs:
        hit_delta = pair.treatment["metrics"].get("any_gold_hit", 0) - pair.baseline["metrics"].get("any_gold_hit", 0)
        add(
            f"| {pair.instance_id} | {_fmt(pair.delta(criteria.primary_cost_metric), 0)} | {hit_delta:+.0f} "
            f"| {pair.baseline['terminal_state']} | {pair.treatment['terminal_state']} |"
        )
    add("")


def _append_supporting_measures(lines: list[str], result: PairedAnalysis) -> None:
    add = lines.append
    add("## Supporting measures")
    add("")
    add(
        "Exploratory: uptake, search displacement, and the failure summary below describe "
        "observed behavior. They support no registered claim."
    )
    add("")

    add("### Uptake and search displacement")
    add("")
    uptake = result.uptake
    add(
        f"- treatment uptake: mean {_fmt(uptake['treatment_mean_uptake'], 2)} c10r calls/trial; "
        f"{uptake['treatment_trials_with_uptake']}/{uptake['treatment_trials']} trials used c10r at least once"
    )
    add(
        f"- search calls: baseline mean {_fmt(uptake['baseline_mean_search'], 2)}, "
        f"treatment mean {_fmt(uptake['treatment_mean_search'], 2)}, "
        f"displacement {_fmt(uptake['search_displacement'], 2)}"
    )
    add("")

    add("### Cache engagement")
    add("")
    add(
        f"Diagnostic, not a claim: flags a trial of {CACHE_SIGNAL_TURN_THRESHOLD}+ turns that reported "
        f"a cached share of prompt tokens at or below {CACHE_SIGNAL_NEAR_ZERO_SHARE:.0%}, the signal that "
        "a frozen run has silently lost caching. Caching does not change `total_tokens`, so it carries no "
        "registered claim."
    )
    add("")
    add("| arm | median cached share | flagged multi-turn near-zero trials | cache category absent |")
    add("| --- | --- | --- | --- |")
    for arm in ("baseline", "treatment"):
        cache = result.cache_engagement[arm]
        median_share = cache["median_cached_share"]
        median_str = "n/a" if median_share is None else f"{median_share:.1%}"
        add(
            f"| {arm} | {median_str} | {cache['near_zero_multiturn_trials']}/{cache['reporting_trials']} "
            f"| {cache['cache_category_absent']}/{cache['n_trials']} |"
        )
    add("")

    add("### Failure summary")
    add("")
    add("Terminal-state counts from the latest attempt of every (instance, arm) cell in this report.")
    add("")
    add("| arm | completed | agent-failure | infra-failure |")
    add("| --- | --- | --- | --- |")
    for arm in ("baseline", "treatment"):
        counts = result.failure_summary[arm]
        add(f"| {arm} | {counts[COMPLETED]} | {counts[AGENT_FAILURE]} | {counts[INFRA_FAILURE]} |")
    add("")

    add("## Exclusions")
    add("")
    if result.exclusions:
        for exclusion in result.exclusions:
            add(f"- {exclusion['instance_id']}: {exclusion['reason']}")
    else:
        add("- none")
    add(f"- development-subset trials excluded from this report: {result.dev_excluded}")
    add("")


def _append_task_level_analyses(lines: list[str], result: PairedAnalysis) -> None:
    add = lines.append
    add("## Task-level analyses")
    add("")
    add(
        "Descriptive only: dividing the pairs by a task characteristic reduces precision, so no "
        "registered claim rides on any result below."
    )
    add("")

    count_result = result.task_level["source_file_count"]
    add(f"### Repository source-file count ({count_result.label})")
    add("")
    add(
        f"- n = {count_result.n_pairs} pairs; the covariate is `log(source_file_count)`, kept "
        "continuous with no threshold derived from the results"
    )
    add(
        f"- accuracy-delta slope per log-unit: {_fmt_slope(count_result.accuracy_slope, count_result.accuracy_slope_ci)}"
    )
    add(f"- log-token-ratio slope per log-unit: {_fmt_slope(count_result.token_slope, count_result.token_slope_ci)}")
    add("")

    cue_result = result.task_level["source_file_cue"]
    add(f"### Source-file cue ({cue_result.label})")
    add("")
    add("| cue | n pairs | accuracy lambda | accuracy 95% CI | token ratio | token 95% CI |")
    add("| --- | --- | --- | --- | --- | --- |")
    for flag in (True, False):
        group = cue_result.groups[flag]
        acc, tok = group["accuracy"], group["token_use"]
        if acc is None:
            add(f"| {flag} | 0 | n/a | n/a | n/a | n/a |")
            continue
        add(
            f"| {flag} | {group['n_pairs']} | {_fmt(acc.lambda_)} | [{_fmt(acc.ci_lower)}, {_fmt(acc.ci_upper)}] | "
            f"{_fmt(tok.ratio, 2)}x | [{_fmt(tok.ratio_ci_lower, 2)}x, {_fmt(tok.ratio_ci_upper, 2)}x] |"
        )
    add("")

    length_result = result.task_level["baseline_trajectory_length"]
    add(f"### Baseline trajectory length ({length_result.label})")
    add("")
    add(f"- n = {length_result.n_pairs} pairs; the covariate is the baseline arm's `total_steps`, continuous")
    add(
        f"- accuracy-delta slope per step: {_fmt_slope(length_result.accuracy_slope, length_result.accuracy_slope_ci)}"
    )
    add(f"- log-token-ratio slope per step: {_fmt_slope(length_result.token_slope, length_result.token_slope_ci)}")
    add("")


def _append_financial_cost(lines: list[str], result: PairedAnalysis) -> None:
    add = lines.append
    add("## Financial cost")
    add("")
    if result.financial_cost is None:
        add("- no price schedule supplied; financial cost not derived.")
    else:
        fc = result.financial_cost
        add(f"- price schedule: `{fc.schedule_name}` (pricing date: {fc.pricing_date})")
        add(
            f"- baseline mean per-trajectory cost: {_fmt_cost(fc.baseline_mean_cost_usd)} "
            f"({fc.baseline_unavailable}/{fc.n_trials['baseline']} trials unavailable)"
        )
        add(
            f"- treatment mean per-trajectory cost: {_fmt_cost(fc.treatment_mean_cost_usd)} "
            f"({fc.treatment_unavailable}/{fc.n_trials['treatment']} trials unavailable)"
        )
    add("")


def _append_registered_claims(lines: list[str], result: PairedAnalysis) -> None:
    criteria = result.criteria
    add = lines.append
    add("## Registered claims")
    add("")
    add(
        "Each claim is evaluated independently against its pre-registered reference boundary. "
        "Claims can overlap, and the accuracy and token-use results are never combined into one verdict."
    )
    add("")
    add("| axis | claim | rule | supported |")
    add("| --- | --- | --- | --- |")
    ac = result.accuracy_claims
    boundary = criteria.accuracy_reference_boundary
    add(f"| accuracy | superior | lower score bound above 0 | {ac.superior} |")
    add(f"| accuracy | non-inferior | lower score bound above -{boundary} | {ac.non_inferior} |")
    add(f"| accuracy | equivalent | 90% score interval inside [-{boundary}, +{boundary}] | {ac.equivalent} |")
    add(f"| accuracy | harm | upper score bound below 0 | {ac.harm} |")
    add(f"| accuracy | material harm | upper score bound below -{boundary} | {ac.material_harm} |")
    tc = result.token_use_claims
    ratio_boundary = 1.0 + criteria.token_use_reference_boundary
    add(f"| token use | superior | upper bootstrap ratio bound below 1.00 | {tc.superior} |")
    add(f"| token use | non-inferior | upper bootstrap ratio bound below {ratio_boundary:.2f} | {tc.non_inferior} |")
    add(f"| token use | harm | lower bootstrap ratio bound above 1.00 | {tc.harm} |")
    add(f"| token use | material harm | lower bootstrap ratio bound above {ratio_boundary:.2f} | {tc.material_harm} |")
    add("")


def _default_plot_links() -> dict[str, str]:
    """Return links for plots beside the report."""
    # Avoid importing matplotlib and plotnine for report-only callers.
    from c10r_evals.plots import PLOT_FILENAMES

    return dict(PLOT_FILENAMES)


def _append_figures(lines: list[str], plot_links: dict[str, str]) -> None:
    add = lines.append
    add("## Figures")
    add("")
    for name, title in FIGURE_TITLES.items():
        add(f"### {title}")
        add("")
        add(f"![{title}]({plot_links[name]})")
        add("")


def _append_output_notes(lines: list[str], result: PairedAnalysis) -> None:
    add = lines.append
    add("## Trajectory-level output")
    add("")
    add(
        "Per-trajectory measures and per-instance pairings needed to reproduce the estimates above, or to "
        "apply a different reference boundary, are emitted alongside this report."
    )
    add("")

    repeats = [
        (pair.instance_id, arm, entry)
        for pair in result.pairs
        for arm, entry in sorted(pair.provenance.items())
        if entry.get("superseded")
    ]
    if repeats:
        add("## Superseded attempts")
        add("")
        add("Each pair below had more than one usable attempt; the latest was used.")
        add("")
        for instance_id, arm, entry in repeats:
            replaced = ", ".join(str(a) for a in entry["superseded"])
            add(f"- {instance_id} ({arm}): used {entry['attempt']}, replacing {replaced}")
        add("")

    add("## Caveats")
    add("")
    add(CAVEATS)


def render_report(result: PairedAnalysis, plot_links: dict[str, str] | None = None) -> str:
    """Render the report with plot links, defaulting to files beside the report."""
    lines = ["# eval-localization paired report", ""]
    _append_decision_criteria(lines, result)
    _append_primary_results(lines, result)
    _append_supporting_measures(lines, result)
    _append_task_level_analyses(lines, result)
    _append_financial_cost(lines, result)
    _append_figures(lines, plot_links or _default_plot_links())
    _append_registered_claims(lines, result)
    _append_output_notes(lines, result)
    return "\n".join(lines)
