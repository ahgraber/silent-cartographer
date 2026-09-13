"""Paired statistics: Tango's score method for the paired difference in proportions, and a
percentile bootstrap for the mean paired log token ratio.

Tango (1998), Statistics in Medicine 17:891-908.
For n pairs with discordant counts b (treatment hit, baseline miss) and c (treatment miss,
baseline hit), the score statistic for H0: p_treatment = p_baseline - delta is

    Z(b, c; n, delta) = (b - c + n*delta) / sqrt(n * (2*q21 - delta*(delta + 1)))

where q21 is the larger root of  2n*x^2 - (b + c + (2n - b + c)*delta)*x + c*delta*(delta + 1) = 0.
Confidence limits for lambda = p_treatment - p_baseline are the solutions of
Z(b, c; n, -lambda) = +/- z_alpha.

Estimates report a two-sided interval at the declared `confidence_level`. Registered claims use
one-sided bounds from the interval at `2 * confidence_level - 1`; token bounds reuse the same
bootstrap draw.
"""

import math
import random
from dataclasses import dataclass

from scipy import stats as scipy_stats


def tango_z(b: int, c: int, n: int, delta: float) -> float:
    """Tango's efficient score statistic for H0: p_treatment = p_baseline - delta."""
    if n <= 0:
        raise ValueError("n must be positive")
    quad_a = 2.0 * n
    quad_b = -(b + c + (2.0 * n - b + c) * delta)
    quad_c = c * delta * (delta + 1.0)
    # The discriminant is analytically non-negative over the feasible delta range; near a root's
    # tangency point (e.g. small, lopsided b/c/n) floating-point cancellation alone can push the
    # computed value fractionally below zero, so it is clamped rather than let sqrt raise on noise.
    discriminant = max(0.0, quad_b * quad_b - 4.0 * quad_a * quad_c)
    q21 = (math.sqrt(discriminant) - quad_b) / (2.0 * quad_a)
    variance = n * (2.0 * q21 - delta * (delta + 1.0))
    if variance <= 0:
        # Degenerate at the boundary (e.g. b = c = 0 with delta = 0): no discordance, no evidence.
        return 0.0
    return (b - c + n * delta) / math.sqrt(variance)


def _solve_lambda(b: int, c: int, n: int, target_z: float) -> float:
    """Find lambda with Z(b, c; n, -lambda) = target_z by bisection; Z is decreasing in lambda."""
    lo, hi = -1.0 + 1e-9, 1.0 - 1e-9

    def g(lam: float) -> float:
        return tango_z(b, c, n, -lam) - target_z

    g_lo, g_hi = g(lo), g(hi)
    if g_lo < 0:
        return lo
    if g_hi > 0:
        return hi
    for _ in range(200):
        mid = (lo + hi) / 2.0
        if g(mid) > 0:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2.0


def tango_ci(b: int, c: int, n: int, level: float = 0.95) -> tuple[float, float]:
    """Two-sided score confidence interval for lambda = p_treatment - p_baseline."""
    alpha = (1.0 - level) / 2.0
    z_alpha = float(scipy_stats.norm.ppf(1.0 - alpha))
    lower = _solve_lambda(b, c, n, +z_alpha)
    upper = _solve_lambda(b, c, n, -z_alpha)
    return lower, upper


@dataclass(frozen=True)
class AccuracyEstimate:
    """The paired difference in any-gold-hit rate, lambda = p_treatment - p_baseline.

    `ci_lower`/`ci_upper` are the two-sided Tango score interval at the declared confidence level,
    reported as the headline estimate. `bound_lower`/`bound_upper` are the two-sided Tango score
    interval at `2 * confidence_level - 1`; each endpoint equals the one-sided bound at the
    declared confidence level, used to evaluate registered claims.
    """

    lambda_: float
    b: int
    c: int
    n_pairs: int
    ci_lower: float
    ci_upper: float
    bound_lower: float
    bound_upper: float


def accuracy_estimate(b: int, c: int, n: int, confidence_level: float = 0.95) -> AccuracyEstimate:
    """Paired hit-rate difference with its two-sided estimate interval and one-sided claim bounds.

    At `confidence_level = 0.95` these are the two-sided 95% interval and the two-sided 90%
    interval whose endpoints are the one-sided 95% claim bounds.
    """
    lambda_ = (b - c) / n if n else 0.0
    ci_lower, ci_upper = tango_ci(b, c, n, level=confidence_level)
    bound_lower, bound_upper = tango_ci(b, c, n, level=2.0 * confidence_level - 1.0)
    return AccuracyEstimate(
        lambda_=lambda_,
        b=b,
        c=c,
        n_pairs=n,
        ci_lower=ci_lower,
        ci_upper=ci_upper,
        bound_lower=bound_lower,
        bound_upper=bound_upper,
    )


@dataclass(frozen=True)
class AccuracyClaims:
    """Registered accuracy claims, each evaluated independently against one reference boundary."""

    superior: bool
    non_inferior: bool
    equivalent: bool
    harm: bool
    material_harm: bool


def accuracy_claims(estimate: AccuracyEstimate, boundary: float) -> AccuracyClaims:
    """Evaluate the five registered accuracy claims from the estimate's one-sided bounds.

    Equivalence is the two-one-sided-tests formulation: both one-sided bounds fall
    inside [-boundary, +boundary].
    """
    lower, upper = estimate.bound_lower, estimate.bound_upper
    return AccuracyClaims(
        superior=lower > 0.0,
        non_inferior=lower > -boundary,
        equivalent=lower > -boundary and upper < boundary,
        harm=upper < 0.0,
        material_harm=upper < -boundary,
    )


def _percentile(sorted_values: list[float], p: float) -> float:
    idx = min(len(sorted_values) - 1, int(p * len(sorted_values)))
    return sorted_values[idx]


@dataclass(frozen=True)
class TokenRatioEstimate:
    """The mean paired log token ratio, theta = mean(log(treatment / baseline)), and its ratio.

    `ci_lower`/`ci_upper` (and their `ratio_ci_*` exponentials) are the two-sided percentile-
    bootstrap interval at the declared confidence level, reported as the headline estimate.
    `bound_lower`/`bound_upper` (and their `ratio_bound_*` exponentials) are the one-sided
    bootstrap bounds at the same confidence level, used to evaluate registered claims, taken
    from the same bootstrap draw.
    """

    theta: float
    ratio: float
    ci_lower: float
    ci_upper: float
    ratio_ci_lower: float
    ratio_ci_upper: float
    bound_lower: float
    bound_upper: float
    ratio_bound_lower: float
    ratio_bound_upper: float
    n_pairs: int


def token_ratio_estimate(
    log_ratios: list[float],
    confidence_level: float = 0.95,
    resamples: int = 20000,
    seed: int = 0,
) -> TokenRatioEstimate:
    """Percentile bootstrap over instances for the mean paired log token ratio.

    One bootstrap draw supplies both the two-sided interval at `confidence_level` (percentiles
    `(1 - confidence_level) / 2` and `1 - (1 - confidence_level) / 2`) and the one-sided claim
    bounds at `confidence_level` (percentiles `1 - confidence_level` and `confidence_level`), so
    the two never disagree. At `confidence_level = 0.95` those are the 2.5th/97.5th and 5th/95th
    percentiles.
    """
    n = len(log_ratios)
    if n == 0:
        return TokenRatioEstimate(
            theta=0.0,
            ratio=1.0,
            ci_lower=0.0,
            ci_upper=0.0,
            ratio_ci_lower=1.0,
            ratio_ci_upper=1.0,
            bound_lower=0.0,
            bound_upper=0.0,
            ratio_bound_lower=1.0,
            ratio_bound_upper=1.0,
            n_pairs=0,
        )
    theta = sum(log_ratios) / n
    rng = random.Random(seed)
    means = sorted(sum(rng.choices(log_ratios, k=n)) / n for _ in range(resamples))
    two_sided_tail = (1.0 - confidence_level) / 2.0
    ci_lower, ci_upper = _percentile(means, two_sided_tail), _percentile(means, 1.0 - two_sided_tail)
    bound_lower, bound_upper = _percentile(means, 1.0 - confidence_level), _percentile(means, confidence_level)
    return TokenRatioEstimate(
        theta=theta,
        ratio=math.exp(theta),
        ci_lower=ci_lower,
        ci_upper=ci_upper,
        ratio_ci_lower=math.exp(ci_lower),
        ratio_ci_upper=math.exp(ci_upper),
        bound_lower=bound_lower,
        bound_upper=bound_upper,
        ratio_bound_lower=math.exp(bound_lower),
        ratio_bound_upper=math.exp(bound_upper),
        n_pairs=n,
    )


@dataclass(frozen=True)
class TokenUseClaims:
    """Registered token-use claims, each evaluated independently against one reference boundary.

    Equivalence is not registered because lower token use is a benefit, not a deviation to bound.
    """

    superior: bool
    non_inferior: bool
    harm: bool
    material_harm: bool


def token_use_claims(estimate: TokenRatioEstimate, boundary: float) -> TokenUseClaims:
    """Evaluate the four registered token-use claims from the one-sided bootstrap bounds."""
    lower, upper = estimate.ratio_bound_lower, estimate.ratio_bound_upper
    ratio_boundary = 1.0 + boundary
    return TokenUseClaims(
        superior=upper < 1.0,
        non_inferior=upper < ratio_boundary,
        harm=lower > 1.0,
        material_harm=lower > ratio_boundary,
    )
