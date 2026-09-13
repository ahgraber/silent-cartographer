"""Report how much of an arm still needs to run.

Completed episodes and agent failures are scorable; infrastructure failures are not.
Work is measured against a target number of scorable attempts per instance.
"""

import logging
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

import yaml

from c10r_evals.runtime import log_fields
from c10r_evals.trial_evidence import AGENT_FAILURE, COMPLETED, classify_terminal_state, instance_id_from_result

logger = logging.getLogger(__name__)

USABLE_STATES = {COMPLETED, AGENT_FAILURE}


class ArmConfigError(Exception):
    """The arm config does not name the one dataset this arm runs over."""


def dataset_path(config_path: Path) -> Path:
    """The task tree the arm runs, as named by its Pier config."""
    config = yaml.safe_load(config_path.read_text())
    datasets = config.get("datasets") or []
    paths = [d.get("path") for d in datasets if isinstance(d, dict) and d.get("path")]
    if len(paths) != 1:
        raise ArmConfigError(f"{config_path} names {len(paths)} dataset paths; expected exactly one")
    return Path(paths[0])


def sweep_dir(config_path: Path) -> Path:
    """The directory holding this arm's sweep metadata and job tree."""
    config = yaml.safe_load(config_path.read_text())
    jobs_dir = config.get("jobs_dir")
    if not jobs_dir:
        raise ArmConfigError(f"{config_path} does not name a jobs_dir")
    return Path(jobs_dir).parent


def planned_instances(dataset_root: Path) -> set[str]:
    """Instance ids the arm's task tree contains."""
    if not dataset_root.is_dir():
        raise ArmConfigError(f"task tree {dataset_root} does not exist; generate the tasks first")
    return {p.name for p in dataset_root.iterdir() if p.is_dir()}


def scored_attempts(sweep: Path) -> Counter[str]:
    """How many scorable attempts each instance has, across every job directory.

    A resumed or topped-up sweep writes a new job directory each time, so attempts for one
    instance are spread across them. Trial directory names are truncated by Pier, so the
    instance id comes from the trial's own record rather than from the directory name.
    """
    jobs = sweep / "jobs"
    if not jobs.is_dir():
        return Counter()
    counts: Counter[str] = Counter()
    for job in sorted(p for p in jobs.iterdir() if p.is_dir()):
        for trial in sorted(p for p in job.iterdir() if p.is_dir()):
            if not (trial / "result.json").is_file():
                continue
            if classify_terminal_state(trial) not in USABLE_STATES:
                continue
            counts[_instance_of(trial)] += 1
    counts.pop("", None)
    return counts


def scored_instances(sweep: Path) -> set[str]:
    """Instances with at least one scorable attempt."""
    return set(scored_attempts(sweep))


def _instance_of(trial_dir: Path) -> str:
    """Identify a trial's instance exactly as the importer does."""
    import json

    record = json.loads((trial_dir / "result.json").read_text())
    return instance_id_from_result(record, trial_dir.name)


@dataclass(frozen=True)
class ArmStatus:
    """How much of one arm is done, measured against a target of `attempts` per instance."""

    config: Path
    attempts: int
    planned_ids: tuple[str, ...]
    deficits: dict[str, int]  # instance -> scorable attempts still owed, only where non-zero
    attempt_counts: Counter[str]  # instance -> scorable attempts recorded so far

    @property
    def planned(self) -> int:
        return len(self.planned_ids)

    @property
    def pending(self) -> list[str]:
        """Instances still owed at least one attempt, in a stable order."""
        return sorted(self.deficits)

    @property
    def owed(self) -> int:
        """Total attempts still to run across the arm."""
        return sum(self.deficits.values())

    @property
    def distribution(self) -> dict[int, int]:
        """How many planned instances sit at each attempt count, including zero.

        A ragged arm shows more than one entry, which is what an interrupted top-up looks like.
        """
        return dict(sorted(Counter(self.attempt_counts.get(i, 0) for i in self.planned_ids).items()))

    @property
    def above_target(self) -> dict[str, int]:
        """Instances already holding more scorable attempts than the target asks for.

        Nothing runs for these and nothing is discarded, but a target below what an arm holds
        usually means the wrong config was named, so the caller says so rather than reporting
        an arm complete without comment.
        """
        return {i: self.attempt_counts[i] for i in self.planned_ids if self.attempt_counts.get(i, 0) > self.attempts}

    @property
    def complete(self) -> bool:
        return not self.deficits


def arm_status(config_path: Path, attempts: int = 1) -> ArmStatus:
    """Measure one arm against a target of `attempts` scorable attempts per planned instance."""
    if attempts < 1:
        raise ValueError(f"attempts must be at least 1, got {attempts}")
    planned = planned_instances(dataset_path(config_path))
    counts = scored_attempts(sweep_dir(config_path))
    deficits = {i: attempts - counts.get(i, 0) for i in planned if counts.get(i, 0) < attempts}
    status = ArmStatus(
        config=config_path,
        attempts=attempts,
        planned_ids=tuple(sorted(planned)),
        deficits=deficits,
        attempt_counts=counts,
    )
    log_fields(
        logger,
        logging.INFO,
        "arm_status_computed",
        config=str(config_path),
        attempts=attempts,
        planned=status.planned,
        pending=len(deficits),
        owed=status.owed,
        distribution=status.distribution,
    )
    if status.above_target:
        held = sorted(set(status.above_target.values()))
        log_fields(
            logger,
            logging.WARNING,
            "arm_holds_more_attempts_than_requested",
            config=str(config_path),
            attempts=attempts,
            instances_above_target=len(status.above_target),
            attempts_held=held,
        )
    return status


def pending_instances(config_path: Path, attempts: int = 1) -> list[str]:
    """Instances of this arm owed a scorable attempt, in a stable order."""
    return arm_status(config_path, attempts).pending
