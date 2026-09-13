"""Registered analysis criteria loaded from a replayable JSON input."""

import json
from dataclasses import dataclass
from pathlib import Path

CRITERIA_REQUIRED_KEYS = (
    "version",
    "primary_cost_metric",
    "accuracy_reference_boundary",
    "token_use_reference_boundary",
    "confidence_level",
    "failure_policy",
)


@dataclass(frozen=True)
class Criteria:
    """Pre-declared analysis criteria; required input for any reported analysis.

    The analysis reports paired estimates for both axes and evaluates each registered claim
    independently against its reference boundary. It issues no combined pass/fail or adoption
    outcome across the two axes.
    """

    version: str
    primary_cost_metric: str
    accuracy_reference_boundary: float  # percentage points of hit rate, so 0.025 is 2.5 points
    token_use_reference_boundary: float  # proportional, so 0.10 is a ratio boundary of 1.10
    confidence_level: float
    failure_policy: dict
    # The pre-registered run size. `planned_pairs` is what the report is measured against, so a
    # short report says so; `attempts_per_cell` is what `run` holds the frozen sweep to.
    planned_pairs: int | None = None
    attempts_per_cell: int | None = None

    @classmethod
    def load(cls, path: Path) -> "Criteria":
        raw = json.loads(path.read_text())
        missing = [key for key in CRITERIA_REQUIRED_KEYS if key not in raw]
        if missing:
            raise ValueError(f"decision-criteria file {path} is missing required keys: {missing}")
        planned_pairs = raw.get("planned_pairs")
        attempts_per_cell = raw.get("attempts_per_cell")
        return cls(
            version=raw["version"],
            primary_cost_metric=raw["primary_cost_metric"],
            accuracy_reference_boundary=float(raw["accuracy_reference_boundary"]),
            token_use_reference_boundary=float(raw["token_use_reference_boundary"]),
            confidence_level=float(raw["confidence_level"]),
            failure_policy=raw["failure_policy"],
            planned_pairs=None if planned_pairs is None else int(planned_pairs),
            attempts_per_cell=None if attempts_per_cell is None else int(attempts_per_cell),
        )
