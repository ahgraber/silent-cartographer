"""Markdown report rendering for a completed paired analysis."""

from c10r_evals.analysis import PairedAnalysis

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


def render_report(result: PairedAnalysis) -> str:
    criteria = result.criteria
    lines: list[str] = []
    add = lines.append

    add("# eval-localization paired report")
    add("")
    add("## Decision criteria")
    add("")
    add(f"- criteria version: `{criteria.version}`")
    add(f"- primary cost metric: `{criteria.primary_cost_metric}`")
    add(f"- accuracy non-inferiority margin: {criteria.non_inferiority_margin} (any-gold-file hit rate)")
    add(f"- confidence level (one-sided): {criteria.confidence_level}")
    add(f"- failure policy: {criteria.failure_policy}")
    add("")

    add("## Headline outcomes (intent-to-treat)")
    add("")
    add(
        f"- cost superiority on `{result.cost.metric}`: **{result.cost.outcome}** "
        f"(Wilcoxon one-sided p = {_fmt(result.cost.p_value, 4)}, "
        f"median delta = {_fmt(result.cost.median_delta, 1)}, mean delta = {_fmt(result.cost.mean_delta, 1)}, "
        f"n = {result.cost.n_pairs} pairs)"
    )
    acc = result.accuracy
    add(
        f"- accuracy non-inferiority: **{acc.outcome}** "
        f"(paired hit-rate difference CI [{_fmt(acc.ci_lower)}, {_fmt(acc.ci_upper)}], "
        f"margin -{acc.margin}, score z = {_fmt(acc.z, 2)}, "
        f"discordant pairs b = {acc.b}, c = {acc.c}, n = {acc.n_pairs})"
    )
    add("- every pair counts regardless of c10r uptake; agent failures score as outcomes.")
    add("")

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

    add("## Secondary cost metrics")
    add("")
    for sec in result.secondary_costs:
        add(
            f"- `{sec.metric}`: {sec.outcome} (p = {_fmt(sec.p_value, 4)}, median delta = {_fmt(sec.median_delta, 1)})"
        )
    if not result.secondary_costs:
        add("- none recorded")
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

    add("## Uptake and search displacement")
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

    add("## Index amortization")
    add("")
    break_even = result.break_even
    if not break_even.get("available"):
        add(f"- not computable: {break_even.get('reason')}")
    else:
        add(
            f"- mean index build: {_fmt(break_even['mean_index_wall_clock_s'], 1)} s, {_fmt(break_even['mean_index_bytes'] / 1_000_000, 1)} MB"
        )
        add(f"- per-episode wall-clock saving: {_fmt(break_even['per_episode_wall_saving_s'], 1)} s")
        if break_even.get("break_even_episodes") is not None:
            add(f"- break-even: {_fmt(break_even['break_even_episodes'], 1)} episodes amortize one index build")
        else:
            add("- break-even: never (no per-episode wall-clock saving observed)")
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
    return "\n".join(lines)
