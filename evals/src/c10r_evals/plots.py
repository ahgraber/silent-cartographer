"""The four registered plots for a paired analysis, rendered with plotnine.

A headless, deterministic backend is forced before plotnine's own matplotlib import, so
rendering never depends on a display and never varies between machines.
"""

import statistics
from pathlib import Path

import matplotlib

matplotlib.use("Agg")

import pandas as pd
from plotnine import (
    aes,
    facet_wrap,
    geom_boxplot,
    geom_col,
    geom_histogram,
    geom_hline,
    geom_line,
    geom_point,
    geom_pointrange,
    geom_rect,
    geom_rug,
    geom_segment,
    geom_vline,
    ggplot,
    labs,
    scale_x_log10,
    scale_y_log10,
)

from c10r_evals.analysis import PairedAnalysis

PLOT_FILENAMES = {
    "paired-accuracy": "paired-accuracy.png",
    "token-ratio-distribution": "token-ratio-distribution.png",
    "token-summaries": "token-summaries.png",
    "turn-size": "turn-size.png",
}


def _avg_tokens_per_turn(metrics: dict) -> float | None:
    """`total_tokens / total_steps`, the plots' definition of average tokens per turn."""
    tokens = metrics.get("total_tokens")
    steps = metrics.get("total_steps")
    if tokens is None or not steps:
        return None
    return tokens / steps


def build_accuracy_plot(result: PairedAnalysis) -> ggplot:
    """Paired localization accuracy: the four paired outcome counts beside the hit-rate estimate.

    plotnine has no single geom that overlays a bar chart of counts and a point-range-with-band
    estimate in one coordinate system, so the two displays are composed as facets of one plot:
    each layer carries its own small data frame tagged with the facet it belongs to, and
    `facet_wrap` with free scales renders each in its own panel while keeping both under one
    figure and one legend/theme.
    """
    outcomes = result.paired_outcomes
    outcomes_df = pd.DataFrame(
        {
            "panel": "paired outcomes",
            "outcome": list(outcomes.keys()),
            "count": list(outcomes.values()),
        }
    )
    acc = result.accuracy
    boundary = result.criteria.accuracy_reference_boundary
    estimate_df = pd.DataFrame(
        {
            "panel": ["hit-rate difference"],
            "x": ["treatment - baseline"],
            "lambda_": [acc.lambda_],
            "ci_lower": [acc.ci_lower],
            "ci_upper": [acc.ci_upper],
        }
    )
    boundary_df = pd.DataFrame({"panel": ["hit-rate difference"], "ymin": [-boundary], "ymax": [boundary]})

    return (
        ggplot()
        + geom_col(data=outcomes_df, mapping=aes(x="outcome", y="count"))
        + geom_rect(
            data=boundary_df,
            mapping=aes(xmin=-float("inf"), xmax=float("inf"), ymin="ymin", ymax="ymax"),
            inherit_aes=False,
            fill="#66c2a5",
            alpha=0.25,
        )
        + geom_hline(data=estimate_df, mapping=aes(yintercept=0), linetype="dashed")
        + geom_pointrange(data=estimate_df, mapping=aes(x="x", y="lambda_", ymin="ci_lower", ymax="ci_upper"))
        + facet_wrap("~panel", scales="free")
        + labs(x="", y="value", title="Paired localization accuracy")
    )


def build_token_ratio_plot(result: PairedAnalysis) -> ggplot:
    """Paired token-ratio distribution: histogram, rug, box-and-whisker, geometric mean, median.

    The histogram's bins are equal-width in `log10(ratio)` because `scale_x_log10` transforms the
    x data before `geom_histogram` computes its bins, which is the standard way an equal-width
    log-scale histogram is built rather than binning the linear ratio and relabeling the axis.
    """
    metric = result.criteria.primary_cost_metric
    ratio_boundary = 1.0 + result.criteria.token_use_reference_boundary
    ratios = [
        pair.treatment["metrics"][metric] / pair.baseline["metrics"][metric]
        for pair in result.pairs
        if pair.baseline["metrics"].get(metric, 0) > 0 and pair.treatment["metrics"].get(metric, 0) > 0
    ]
    df = pd.DataFrame({"ratio": ratios or [1.0]})

    token = result.token_use
    median = statistics.median(ratios) if ratios else 1.0
    if len(ratios) >= 4:
        q1, q3 = statistics.quantiles(ratios, n=4)[0], statistics.quantiles(ratios, n=4)[2]
    else:
        q1 = q3 = median
    iqr = q3 - q1
    in_fence = [r for r in ratios if q1 - 1.5 * iqr <= r <= q3 + 1.5 * iqr] or (ratios or [median])
    lo_whisker, hi_whisker = min(in_fence), max(in_fence)

    box_y, box_half = -1.0, 0.4
    box_df = pd.DataFrame({"xmin": [q1], "xmax": [q3], "ymin": [box_y - box_half], "ymax": [box_y + box_half]})
    whisker_df = pd.DataFrame(
        {"x": [lo_whisker, q3], "xend": [q1, hi_whisker], "y": [box_y, box_y], "yend": [box_y, box_y]}
    )
    median_line_df = pd.DataFrame(
        {"x": [median], "xend": [median], "y": [box_y - box_half], "yend": [box_y + box_half]}
    )
    mean_y = box_y + box_half + 0.3
    mean_ci_df = pd.DataFrame(
        {"x": [token.ratio_ci_lower], "xend": [token.ratio_ci_upper], "y": [mean_y], "yend": [mean_y]}
    )
    mean_point_df = pd.DataFrame({"x": [token.ratio], "y": [mean_y]})

    return (
        ggplot(df, aes(x="ratio"))
        + geom_histogram(bins=20, fill="#8da0cb", color="white")
        + geom_rug(sides="b")
        + geom_rect(
            data=box_df,
            mapping=aes(xmin="xmin", xmax="xmax", ymin="ymin", ymax="ymax"),
            inherit_aes=False,
            fill="white",
            color="black",
        )
        + geom_segment(data=whisker_df, mapping=aes(x="x", xend="xend", y="y", yend="yend"), inherit_aes=False)
        + geom_segment(
            data=median_line_df, mapping=aes(x="x", xend="xend", y="y", yend="yend"), inherit_aes=False, color="blue"
        )
        + geom_segment(
            data=mean_ci_df, mapping=aes(x="x", xend="xend", y="y", yend="yend"), inherit_aes=False, color="red"
        )
        + geom_point(data=mean_point_df, mapping=aes(x="x", y="y"), inherit_aes=False, color="red")
        + geom_vline(xintercept=1.00, linetype="dashed")
        + geom_vline(xintercept=ratio_boundary, linetype="dotted")
        + scale_x_log10()
        + labs(x="paired token ratio (log scale)", y="count", title="Paired token-ratio distribution")
    )


def build_token_summary_plot(result: PairedAnalysis) -> ggplot:
    """Comparative token summaries: arm-level box-and-whisker for total_tokens and avg tokens/turn."""
    rows = []
    for pair in result.pairs:
        for arm, trial in (("baseline", pair.baseline), ("treatment", pair.treatment)):
            total = trial["metrics"].get("total_tokens")
            avg = _avg_tokens_per_turn(trial["metrics"])
            if total is not None and total > 0:
                rows.append({"instance_id": pair.instance_id, "arm": arm, "measure": "total_tokens", "value": total})
            if avg is not None and avg > 0:
                rows.append(
                    {"instance_id": pair.instance_id, "arm": arm, "measure": "avg_tokens_per_turn", "value": avg}
                )
    df = pd.DataFrame(rows)
    df["arm"] = pd.Categorical(df["arm"], categories=["baseline", "treatment"], ordered=True)

    return (
        ggplot(df, aes(x="arm", y="value"))
        + geom_line(aes(group="instance_id"), alpha=0.3)
        + geom_boxplot(aes(x="arm", y="value"), alpha=0.5, outlier_alpha=0)
        + geom_point(alpha=0.5)
        + facet_wrap("~measure", scales="free_y")
        + scale_y_log10()
        + labs(x="", y="value (log scale)", title="Comparative token summaries")
    )


def build_turn_size_plot(result: PairedAnalysis) -> ggplot:
    """Turn count against average turn size: arm panels, hit/miss markers, iso-total-token curves."""
    rows = []
    for pair in result.pairs:
        for arm, trial in (("baseline", pair.baseline), ("treatment", pair.treatment)):
            steps = trial["metrics"].get("total_steps")
            avg = _avg_tokens_per_turn(trial["metrics"])
            if steps and avg is not None:
                rows.append(
                    {
                        "instance_id": pair.instance_id,
                        "arm": arm,
                        "steps": steps,
                        "avg_tokens_per_turn": avg,
                        "hit": bool(trial["metrics"].get("any_gold_hit", 0)),
                    }
                )
    df = pd.DataFrame(rows)

    plot = ggplot()
    if not df.empty:
        totals = sorted(row["steps"] * row["avg_tokens_per_turn"] for row in rows)
        levels = sorted({totals[0], totals[len(totals) // 2], totals[-1]})
        # The experiment store returns every metric as a float, so a turn count arrives as e.g.
        # 16.0; the curve is sampled at whole turns, which is the only scale a turn count takes.
        step_min = max(1, int(min(row["steps"] for row in rows)))
        step_max = int(max(row["steps"] for row in rows))
        iso_rows = [
            {"steps": s, "avg_tokens_per_turn": level / s, "level": level}
            for level in levels
            for s in range(step_min, step_max + 1)
        ]
        plot = plot + geom_line(
            data=pd.DataFrame(iso_rows),
            mapping=aes(x="steps", y="avg_tokens_per_turn", group="level"),
            inherit_aes=False,
            linetype="dotted",
            alpha=0.5,
        )
    plot = (
        plot
        + geom_point(data=df, mapping=aes(x="steps", y="avg_tokens_per_turn", color="hit", shape="hit"))
        + facet_wrap("~arm")
        + labs(x="turns used", y="average tokens per turn", title="Turn count and average turn size")
    )
    return plot


def render_plots(result: PairedAnalysis, out_dir: Path) -> dict:
    """Render and save all four registered plots; returns their output paths by name."""
    out_dir.mkdir(parents=True, exist_ok=True)
    builders = {
        "paired-accuracy": build_accuracy_plot,
        "token-ratio-distribution": build_token_ratio_plot,
        "token-summaries": build_token_summary_plot,
        "turn-size": build_turn_size_plot,
    }
    paths = {}
    for name, builder in builders.items():
        path = out_dir / PLOT_FILENAMES[name]
        builder(result).save(path, width=7, height=5, dpi=100, verbose=False)
        paths[name] = path
    return paths
