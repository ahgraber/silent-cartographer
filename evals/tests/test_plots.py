"""The four registered plots: each expected file renders non-empty, and reference boundaries
are actually present in the plot's layer specification, not merely absent-of-error rendering."""

import statistics
import warnings
from dataclasses import replace

import pytest
from test_analysis import CRITERIA, trial

from c10r_evals.analysis import analyze
from c10r_evals.plots import (
    PLOT_FILENAMES,
    build_accuracy_plot,
    build_token_ratio_plot,
    build_token_summary_plot,
    build_turn_size_plot,
    render_plots,
)


def _layers_of(plot, geom_name: str):
    return [layer for layer in plot.layers if type(layer.geom).__name__ == geom_name]


def _varied_step_trials(n: int = 8) -> list[dict]:
    """Paired trials whose `total_steps` actually varies across instances.

    `paired_trials` fixes every trial's `total_steps` at 12, which collapses plot 3's connected
    pairs to flat lines and plot 4's iso-total-token curves to single points each — a fixture
    artifact, not a real trajectory's shape, and one that hides a broken iso-curve implementation
    from the test suite. Real trajectories vary in length, so plot fixtures should too.
    """
    trials = []
    for i in range(n):
        instance = f"inst-{i:02d}"
        steps = 5 + 3 * i
        trials.append(trial(instance, "baseline", tokens=30_000 + i * 500, steps=steps))
        trials.append(trial(instance, "treatment", tokens=18_000 + i * 400, uptake=3, search=2, steps=steps))
    return trials


def _float_metric_trials(n: int = 6) -> list[dict]:
    """Paired trials whose metrics are floats, as the experiment store returns them.

    MLflow records every metric as a float, so a trial read back from the store carries
    `total_steps` as e.g. `16.0`. Fixtures that use ints hide any plot arithmetic that
    requires an integer.
    """
    trials = _varied_step_trials(n)
    for t in trials:
        t["metrics"] = {k: float(v) for k, v in t["metrics"].items()}
    return trials


def test_registered_plots_render(tmp_path):
    result = analyze(_varied_step_trials(n=8), CRITERIA)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        paths = render_plots(result, tmp_path)
    one_observation_warnings = [w for w in caught if "one observation" in str(w.message).lower()]
    assert not one_observation_warnings, (
        "varied step counts must give plot 4's iso-curves more than one point per level; "
        f"got: {[str(w.message) for w in one_observation_warnings]}"
    )

    assert set(paths) == set(PLOT_FILENAMES)
    for name, path in paths.items():
        assert path.name == PLOT_FILENAMES[name]
        assert path.is_file()
        assert path.stat().st_size > 0


def test_turn_size_plot_draws_iso_token_curves():
    """Plot 4's contract includes curves for equal `total_tokens` values; assert they actually
    exist as multi-point lines, not merely that rendering did not raise."""
    result = analyze(_varied_step_trials(n=8), CRITERIA)
    plot = build_turn_size_plot(result)

    lines = _layers_of(plot, "geom_line")
    assert lines, "plot 4 must draw at least one iso-total-token curve layer"
    iso_data = lines[0].geom.data
    assert "level" in iso_data.columns
    points_per_level = iso_data.groupby("level").size()
    assert len(points_per_level) >= 1
    assert (points_per_level >= 2).all(), "each iso-total-token curve must span more than one point"


def test_plots_render_from_float_metrics_as_the_store_returns_them(tmp_path):
    """Every metric read back from the experiment store is a float, including `total_steps`."""
    result = analyze(_float_metric_trials(), CRITERIA)

    written = render_plots(result, tmp_path)

    assert set(written) == set(PLOT_FILENAMES)
    for path in written.values():
        assert path.stat().st_size > 0


def test_boundaries_drawn_when_interval_crosses(tmp_path):
    # A handful of pairs makes a wide confidence interval; assert the crossing actually holds
    # before checking that the boundary is still drawn, so the scenario is not vacuous.
    trials = _varied_step_trials(n=8)
    trials[1]["metrics"]["any_gold_hit"] = 0  # one baseline-only hit: enough discordance to widen the CI
    result = analyze(trials, CRITERIA)
    boundary = result.criteria.accuracy_reference_boundary
    ratio_boundary = 1.0 + result.criteria.token_use_reference_boundary
    assert result.accuracy.ci_lower < boundary < result.accuracy.ci_upper or (
        result.accuracy.ci_lower < -boundary < result.accuracy.ci_upper
    )

    accuracy_plot = build_accuracy_plot(result)
    rects = _layers_of(accuracy_plot, "geom_rect")
    assert rects, "accuracy plot must draw the shaded reference-boundary region"
    boundary_data = rects[0].geom.data
    assert boundary_data["ymin"].iloc[0] == -boundary
    assert boundary_data["ymax"].iloc[0] == boundary
    (accuracy_plot).save(tmp_path / "accuracy.png", width=7, height=5, dpi=80, verbose=False)

    token_plot = build_token_ratio_plot(result)
    vlines = _layers_of(token_plot, "geom_vline")
    xintercepts = sorted(layer.geom.data["xintercept"].iloc[0] for layer in vlines)
    assert 1.00 in xintercepts
    assert ratio_boundary in xintercepts
    token_plot.save(tmp_path / "token.png", width=7, height=5, dpi=80, verbose=False)


def test_boundaries_drawn_regardless_of_criteria_values():
    """The reference lines track the declared criteria, not a hardcoded literal."""
    custom = replace(CRITERIA, accuracy_reference_boundary=0.2, token_use_reference_boundary=0.33)
    result = analyze(_varied_step_trials(n=6), custom)

    rects = _layers_of(build_accuracy_plot(result), "geom_rect")
    boundary_data = rects[0].geom.data
    assert boundary_data["ymin"].iloc[0] == -0.2
    assert boundary_data["ymax"].iloc[0] == 0.2

    vlines = _layers_of(build_token_ratio_plot(result), "geom_vline")
    xintercepts = sorted(layer.geom.data["xintercept"].iloc[0] for layer in vlines)
    assert xintercepts == sorted([1.00, 1.33])


def test_accuracy_plot_shows_paired_outcome_counts_and_hit_rate_interval():
    result = analyze(_varied_step_trials(n=8), CRITERIA)
    plot = build_accuracy_plot(result)

    cols = _layers_of(plot, "geom_col")
    assert cols, "plot 1 must draw the four paired outcome counts as bars"
    counts = dict(zip(cols[0].geom.data["outcome"], cols[0].geom.data["count"], strict=True))
    assert counts == result.paired_outcomes

    pointranges = _layers_of(plot, "geom_pointrange")
    assert pointranges, "plot 1 must draw the hit-rate difference with its interval"
    estimate = pointranges[0].geom.data
    assert estimate["lambda_"].iloc[0] == pytest.approx(result.accuracy.lambda_)
    assert estimate["ci_lower"].iloc[0] == pytest.approx(result.accuracy.ci_lower)
    assert estimate["ci_upper"].iloc[0] == pytest.approx(result.accuracy.ci_upper)

    hlines = _layers_of(plot, "geom_hline")
    assert hlines, "plot 1 must draw the no-difference line at 0"
    assert hlines[0].geom.mapping["yintercept"] == 0


def test_token_ratio_plot_shows_histogram_rug_box_median_and_mean():
    result = analyze(_varied_step_trials(n=8), CRITERIA)
    plot = build_token_ratio_plot(result)

    assert _layers_of(plot, "geom_histogram"), "plot 2 must draw the ratio histogram"
    assert _layers_of(plot, "geom_rug"), "plot 2 must draw the observation rug"

    metric = result.criteria.primary_cost_metric
    ratios = [pair.treatment["metrics"][metric] / pair.baseline["metrics"][metric] for pair in result.pairs]
    q1, q3 = statistics.quantiles(ratios, n=4)[0], statistics.quantiles(ratios, n=4)[2]

    boxes = _layers_of(plot, "geom_rect")
    assert len(boxes) == 1, "plot 2 must draw exactly one box for the box-and-whisker summary"
    box_data = boxes[0].geom.data
    assert box_data["xmin"].iloc[0] == pytest.approx(q1)
    assert box_data["xmax"].iloc[0] == pytest.approx(q3)

    segments = _layers_of(plot, "geom_segment")
    whiskers = [s for s in segments if s.geom.aes_params.get("color") is None]
    assert whiskers, "plot 2 must draw the box-and-whisker's whiskers"

    medians = [s for s in segments if s.geom.aes_params.get("color") == "blue"]
    assert medians, "plot 2 must draw the median"
    assert medians[0].geom.data["x"].iloc[0] == pytest.approx(statistics.median(ratios))

    mean_interval_segments = [s for s in segments if s.geom.aes_params.get("color") == "red"]
    assert mean_interval_segments, "plot 2 must draw the geometric mean's interval"
    mean_ci_data = mean_interval_segments[0].geom.data
    assert mean_ci_data["x"].iloc[0] == pytest.approx(result.token_use.ratio_ci_lower)
    assert mean_ci_data["xend"].iloc[0] == pytest.approx(result.token_use.ratio_ci_upper)

    mean_points = [p for p in _layers_of(plot, "geom_point") if p.geom.aes_params.get("color") == "red"]
    assert mean_points, "plot 2 must draw the geometric mean marker"
    assert mean_points[0].geom.data["x"].iloc[0] == pytest.approx(result.token_use.ratio)


def test_token_summary_plot_shows_arm_boxplots_paired_lines_and_log_scales():
    result = analyze(_varied_step_trials(n=8), CRITERIA)
    plot = build_token_summary_plot(result)

    assert set(plot.data["measure"].unique()) == {"total_tokens", "avg_tokens_per_turn"}
    assert set(plot.data["arm"].astype(str).unique()) == {"baseline", "treatment"}

    boxplots = _layers_of(plot, "geom_boxplot")
    assert boxplots, "plot 3 must draw arm-level box-and-whisker plots"
    assert dict(boxplots[0].mapping) == {"x": "arm", "y": "value"}

    lines = _layers_of(plot, "geom_line")
    assert lines, "plot 3 must connect paired observations across arms"
    assert dict(lines[0].mapping).get("group") == "instance_id"

    log_scales = [s for s in plot.scales if getattr(s, "trans", None) == "log10"]
    assert log_scales, "plot 3 must plot both measures on a log scale"


def test_turn_size_plot_distinguishes_hit_and_miss_and_facets_by_arm():
    trials = _varied_step_trials(n=8)
    trials[1]["metrics"]["any_gold_hit"] = 0  # Exercise both marker states.
    result = analyze(trials, CRITERIA)
    plot = build_turn_size_plot(result)

    points = _layers_of(plot, "geom_point")
    assert points, "plot 4 must plot the turn observations"
    mapping = dict(points[0].mapping)
    assert mapping.get("color") == "hit", "hit/miss must be visually distinguished by color"
    assert mapping.get("shape") == "hit", "hit/miss must be visually distinguished by shape"
    assert set(points[0].geom.data["hit"]) == {True, False}

    assert plot.facet.vars == ["arm"], "plot 4 must use separate panels per arm"
