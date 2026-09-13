"""Run both arms of a sweep, bringing every cell up to a target attempt count.

Running one arm to completion and then the other would confound the arm with everything that
changes over the hours between them: endpoint load, provider-side drift, and time-varying
failure rates. Pairing issues does not remove that, because the two halves of a pair would sit
at opposite ends of the run. Instead the arms alternate in blocks, so each issue's two arms run
close together and the arm is spread evenly across the run rather than concentrated in half of it.

Which arm leads is inverted from block to block. A fixed leader would put one arm first in every
pair and confound the arm with position within the pair; inverting also keeps the same issue from
running in both arms at once, which matters because Pier runs several trials concurrently.

Pier applies its attempt count uniformly to whatever instances one invocation selects, so an arm
short by different amounts on different instances cannot be topped up in a single call. Within a
block, instances are grouped by how many attempts they lack and one invocation is issued per group.

The last invocation replaces this process rather than running underneath it. A frozen sweep runs
for tens of hours, and a Python parent that caught the interrupt and exited first would leave Pier
orphaned mid-sweep.
"""

import logging
import os
import shlex
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

from c10r_evals.resume import arm_status, dataset_path
from c10r_evals.runtime import log_fields

logger = logging.getLogger(__name__)

ARMS = ("baseline", "treatment")

# Pier copies these into the trial container, and the container reads them ahead of the proxy
# settings the rendered config supplies. A value here sends the sweep to the wrong endpoint and
# bills it to the wrong account, so the run refuses rather than producing unusable trials.
HOST_CREDENTIAL_VARS = ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN")


class RunRefused(Exception):
    """The run would not measure what it claims to, so it does not start."""


DEFAULT_BLOCK_SIZE = 10


@dataclass(frozen=True)
class RunGroup:
    """One Pier invocation: the instances it selects and the attempts it adds to each."""

    arm: str
    attempts: int
    instances: tuple[str, ...] | None  # None selects the arm's whole task tree
    argv: tuple[str, ...]
    block: int = 0

    def describe(self) -> str:
        scope = "all instances" if self.instances is None else f"{len(self.instances)} instance(s)"
        return f"block {self.block} {self.arm}: {self.attempts} attempt(s) on {scope}"


def pier_executable() -> Path:
    """Pier from the environment this process runs in, so the sweep is not re-resolved by PATH."""
    return Path(sys.executable).parent / "pier"


def build_pier_argv(
    pier: Path,
    config: Path,
    dataset: Path,
    instances: Sequence[str] | None,
    attempts: int,
) -> tuple[str, ...]:
    """The exact Pier invocation for one group.

    `instances` is None for a pass over the arm's whole tree. A filtered pass names the tree as
    well, because Pier refuses a local dataset filter without an explicit dataset.
    """
    argv = [str(pier), "run", "-c", str(config)]
    if instances is not None:
        argv += ["-p", str(dataset)]
        for instance in instances:
            argv += ["-i", instance]
    if attempts > 1:
        argv += ["-k", str(attempts)]
    return tuple(argv)


def check_credentials(environ: dict[str, str] | None = None) -> None:
    """Refuse a run whose trials would reach an endpoint other than the evaluation proxy."""
    env = os.environ if environ is None else environ
    leaked = [name for name in HOST_CREDENTIAL_VARS if env.get(name)]
    if leaked:
        raise RunRefused(
            f"unset {', '.join(leaked)} before running. Pier copies host credentials into the trial "
            "container, where they take precedence over the proxy the config names, so the sweep "
            "would run against the wrong endpoint and bill the wrong account."
        )
    if not env.get("EVAL_PROXY_TOKEN"):
        raise RunRefused(
            "EVAL_PROXY_TOKEN is empty. The rendered configs carry it unresolved and the container "
            "reads it at run time; see .env.example."
        )


def arm_config(config_dir: Path, arm: str) -> Path:
    """The config for one arm, refusing before anything tries to read it."""
    config = config_dir / f"{arm}.yaml"
    if not config.is_file():
        raise RunRefused(f"{config} does not exist; render the arm configs first")
    return config


def check_subset(config: Path, subset: str) -> None:
    """Refuse a config whose task tree is not the subset the caller named."""
    dataset = dataset_path(config)
    actual = dataset.parent.name
    if actual != subset:
        raise RunRefused(
            f"{config} runs {dataset}, which is the '{actual}' subset, not '{subset}'. "
            "Render the config for the subset you meant, or name the config that already runs it."
        )


def check_attempts(subset: str, attempts: int, planned_attempts: int | None) -> None:
    """Hold the frozen run to its pre-registered attempt count.

    Extra attempts do not change a reported pair, because one attempt represents a cell, but they
    change what was run against what was declared.
    """
    if subset != "frozen" or planned_attempts is None or attempts <= planned_attempts:
        return
    raise RunRefused(
        f"the frozen run is pre-registered at {planned_attempts} attempt(s) per cell and this asks "
        f"for {attempts}. Pass --over-attempts to run more anyway."
    )


def plan_arm(
    arm: str,
    config: Path,
    attempts: int,
    pier: Path,
    only: Sequence[str] | None = None,
    block: int = 0,
) -> list[RunGroup]:
    """Group an arm's outstanding work by how many attempts each instance lacks.

    `only` restricts the plan to one block of instances; the rest of the arm is planned by the
    other calls that cover the remaining blocks.
    """
    status = arm_status(config, attempts)
    dataset = dataset_path(config)
    wanted = status.deficits if only is None else {i: d for i, d in status.deficits.items() if i in set(only)}

    by_deficit: dict[int, list[str]] = {}
    for instance, deficit in sorted(wanted.items()):
        by_deficit.setdefault(deficit, []).append(instance)

    groups = []
    for deficit, instances in sorted(by_deficit.items()):
        whole_tree = only is None and len(instances) == status.planned
        selected = None if whole_tree else tuple(instances)
        groups.append(
            RunGroup(
                arm=arm,
                attempts=deficit,
                instances=selected,
                argv=build_pier_argv(pier, config, dataset, selected, deficit),
                block=block,
            )
        )
    return groups


def blocks_of(instances: Sequence[str], block_size: int) -> list[tuple[str, ...]]:
    """Split a stable instance order into consecutive blocks."""
    if block_size < 1:
        raise RunRefused(f"--block-size must be at least 1, got {block_size}")
    ordered = sorted(instances)
    return [tuple(ordered[i : i + block_size]) for i in range(0, len(ordered), block_size)]


def plan(
    config_dir: Path,
    arms: Sequence[str],
    attempts: int,
    pier: Path | None = None,
    block_size: int = DEFAULT_BLOCK_SIZE,
) -> list[RunGroup]:
    """Every Pier invocation the requested arms still need, with the arms alternating in blocks.

    One arm alone runs as it always would; alternating needs two arms to alternate between.
    """
    resolved = pier or pier_executable()
    configs = {arm: arm_config(config_dir, arm) for arm in arms}
    if len(arms) == 1:
        arm = arms[0]
        return plan_arm(arm, configs[arm], attempts, resolved)

    outstanding = {arm: set(arm_status(configs[arm], attempts).deficits) for arm in arms}
    covered = sorted(set().union(*outstanding.values()))
    groups: list[RunGroup] = []
    for index, block in enumerate(blocks_of(covered, block_size)):
        # Invert the leader each block so neither arm is systematically first within a pair.
        ordered_arms = arms if index % 2 == 0 else tuple(reversed(arms))
        for arm in ordered_arms:
            selected = [i for i in block if i in outstanding[arm]]
            if selected:
                groups += plan_arm(arm, configs[arm], attempts, resolved, only=selected, block=index)
    return groups


def execute(groups: list[RunGroup]) -> int:
    """Run every group, replacing this process with the last one."""
    for group in groups[:-1]:
        log_fields(logger, logging.INFO, "pier_group_start", arm=group.arm, attempts=group.attempts)
        completed = subprocess.run(list(group.argv), check=False)
        if completed.returncode != 0:
            log_fields(
                logger,
                logging.ERROR,
                "pier_group_failed",
                arm=group.arm,
                exit_code=completed.returncode,
            )
            return completed.returncode
    last = groups[-1]
    log_fields(logger, logging.INFO, "pier_group_exec", arm=last.arm, attempts=last.attempts)
    os.execvp(last.argv[0], list(last.argv))


def render_plan(groups: list[RunGroup]) -> str:
    """The planned invocations, one per line, exactly as they would be run."""
    return "\n".join(f"# {group.describe()}\n{shlex.join(group.argv)}" for group in groups)
