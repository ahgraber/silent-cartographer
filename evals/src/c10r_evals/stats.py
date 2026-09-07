"""Paired statistics: Tango's score method for the paired difference in proportions, and Wilcoxon cost superiority.

Tango (1998), Statistics in Medicine 17:891-908.
For n pairs with discordant counts b (treatment hit, baseline miss) and c (treatment miss,
baseline hit), the score statistic for H0: p_treatment = p_baseline - delta is

    Z(b, c; n, delta) = (b - c + n*delta) / sqrt(n * (2*q21 - delta*(delta + 1)))

where q21 is the larger root of  2n*x^2 - (b + c + (2n - b + c)*delta)*x + c*delta*(delta + 1) = 0.
Confidence limits for lambda = p_treatment - p_baseline are the solutions of
Z(b, c; n, -lambda) = +/- z_alpha.
"""

import math
from dataclasses import dataclass

from scipy import stats as scipy_stats


def tango_z(b: int, c: int, n: int, delta: float) -> float:
    """Tango's efficient score statistic for H0: p_treatment = p_baseline - delta."""
    if n <= 0:
        raise ValueError("n must be positive")
    quad_a = 2.0 * n
    quad_b = -(b + c + (2.0 * n - b + c) * delta)
    quad_c = c * delta * (delta + 1.0)
    q21 = (math.sqrt(quad_b * quad_b - 4.0 * quad_a * quad_c) - quad_b) / (2.0 * quad_a)
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
class NonInferiorityResult:
    outcome: str  # "non-inferior" | "not-demonstrated"
    margin: float
    confidence_level: float
    z: float
    ci_lower: float
    ci_upper: float
    b: int
    c: int
    n_pairs: int


def non_inferiority(b: int, c: int, n: int, margin: float, confidence_level: float = 0.95) -> NonInferiorityResult:
    """One-sided non-inferiority decision: lower score bound above -margin.

    The reported interval is the (2*confidence_level - 1) two-sided CI whose lower
    bound corresponds to the one-sided test at 1 - confidence_level.
    """
    alpha = 1.0 - confidence_level
    z_stat = tango_z(b, c, n, margin)
    z_alpha = float(scipy_stats.norm.ppf(confidence_level))
    lower, upper = tango_ci(b, c, n, level=1.0 - 2.0 * alpha)
    outcome = "non-inferior" if z_stat > z_alpha else "not-demonstrated"
    return NonInferiorityResult(
        outcome=outcome,
        margin=margin,
        confidence_level=confidence_level,
        z=z_stat,
        ci_lower=lower,
        ci_upper=upper,
        b=b,
        c=c,
        n_pairs=n,
    )


@dataclass(frozen=True)
class CostSuperiorityResult:
    outcome: str  # "superior" | "not-demonstrated"
    metric: str
    p_value: float
    median_delta: float
    mean_delta: float
    n_pairs: int


def cost_superiority(deltas: list[float], metric: str, alpha: float = 0.05) -> CostSuperiorityResult:
    """Wilcoxon signed-rank on paired (treatment - baseline) deltas; superiority = negative shift."""
    n = len(deltas)
    if n == 0:
        return CostSuperiorityResult("not-demonstrated", metric, 1.0, 0.0, 0.0, 0)
    if all(d == 0 for d in deltas):
        return CostSuperiorityResult("not-demonstrated", metric, 1.0, 0.0, 0.0, n)
    result = scipy_stats.wilcoxon(deltas, alternative="less")
    median_delta = float(sorted(deltas)[n // 2] if n % 2 else sum(sorted(deltas)[n // 2 - 1 : n // 2 + 1]) / 2)
    mean_delta = sum(deltas) / n
    outcome = "superior" if result.pvalue < alpha and median_delta < 0 else "not-demonstrated"
    return CostSuperiorityResult(outcome, metric, float(result.pvalue), median_delta, mean_delta, n)
