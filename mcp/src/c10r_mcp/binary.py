"""Locating the installed `c10r` binary and running it as a child process.

Every invocation runs the binary with its working directory set to the resolved
workspace root and passes no path-bearing flags derived from that root. `c10r`
resolves its store, the store's recorded-root comparison, freshness, and git
seeding from the working directory, so setting it correctly scopes all of them
together.
"""

from __future__ import annotations

import asyncio
from collections.abc import Sequence
from dataclasses import dataclass
import os
from pathlib import Path
import shutil

BINARY_NAME = "c10r"
"""The command the server runs. Never bundled; the user installs it."""

BINARY_ENV_VAR = "C10R_BINARY"
"""An explicit path to the binary, overriding the `PATH` lookup."""

TERMINATE_GRACE_SECONDS = 5.0
"""How long a cancelled command may take to exit before it is killed outright."""


class BinaryUnavailableError(RuntimeError):
    """The `c10r` binary could not be found, or could not be run.

    Distinct from every index-state failure and from a surface-version
    difference: nothing about the workspace or the server can fix it.
    """


def locate_binary() -> str:
    """Return the absolute path to the `c10r` binary.

    An explicit `C10R_BINARY` wins over `PATH`. Either way the result is checked
    to be an executable file here rather than at first use, so an absent binary
    reports against the path that was actually consulted.

    The path is resolved absolute before it is checked. Every invocation runs
    with its working directory set to the caller's workspace, so a relative path
    would be verified against this process's directory and then executed against
    the workspace's — running whichever program happened to sit at that relative
    path inside the repository being indexed.
    """
    override = os.environ.get(BINARY_ENV_VAR)
    if override:
        candidate = Path(override).expanduser().resolve()
        if not candidate.is_file() or not os.access(candidate, os.X_OK):
            raise BinaryUnavailableError(
                f"{BINARY_ENV_VAR} names {override}, which is not an executable file. "
                f"Point it at a `{BINARY_NAME}` binary, or unset it to search PATH."
            )
        return str(candidate)

    found = shutil.which(BINARY_NAME)
    if found is None:
        raise BinaryUnavailableError(
            f"no `{BINARY_NAME}` binary on PATH. Install it, or set {BINARY_ENV_VAR} to the path of one."
        )
    return str(Path(found).resolve())


@dataclass(frozen=True, slots=True)
class Completed:
    """One finished `c10r` invocation, captured whole."""

    stdout: bytes
    """The machine-readable answer, as written."""

    stderr: str
    """The diagnostic, unaltered. Empty on a clean success."""

    exit_code: int
    """The outcome code from `c10r`'s exit taxonomy."""


async def run(binary: str, args: Sequence[str], cwd: Path) -> Completed:
    """Run `c10r` in `cwd` with `--json`, capturing both streams and the status."""
    try:
        process = await asyncio.create_subprocess_exec(
            binary,
            "--json",
            *args,
            cwd=str(cwd),
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    except OSError as exc:
        raise BinaryUnavailableError(f"could not run `{binary}`: {exc}") from exc

    try:
        stdout, stderr = await process.communicate()
    except asyncio.CancelledError:
        # A cancelled call must not leave the command running. `build` rewrites
        # the store, so an orphan keeps writing long after the caller believes it
        # stopped; the child is stopped and reaped before the cancellation
        # propagates, escalating only if it declines to leave.
        await _stop(process)
        raise

    return Completed(
        stdout=stdout,
        stderr=stderr.decode("utf-8", errors="replace"),
        exit_code=process.returncode if process.returncode is not None else 1,
    )


async def _stop(process: asyncio.subprocess.Process) -> None:
    """Terminate a running child, then kill it if it does not exit, and reap it."""
    if process.returncode is not None:
        return
    try:
        process.terminate()
        try:
            await asyncio.wait_for(asyncio.shield(process.wait()), TERMINATE_GRACE_SECONDS)
        except TimeoutError:
            process.kill()
            await asyncio.shield(process.wait())
    except ProcessLookupError:
        # It exited between the check and the signal; nothing left to stop.
        return
