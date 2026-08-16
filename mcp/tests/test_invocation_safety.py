"""The child process is located and stopped safely.

Both properties here follow from the same decision: every invocation runs with
its working directory set to the caller's workspace. That makes the binary path
and the process lifetime the server's problem rather than the shell's.
"""

from __future__ import annotations

import asyncio
from pathlib import Path
import subprocess

from c10r_mcp import binary
from conftest import write_fixture_crate
import pytest


def test_a_binary_from_the_environment_resolves_absolute(
    tmp_path: Path, c10r_binary: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A relative override is made absolute before it is ever run.

    An invocation runs with its working directory set to the workspace, so a
    relative path would be checked here and resolved there — running whichever
    program sat at that path inside the repository being indexed.
    """
    (tmp_path / "tools").mkdir()
    (tmp_path / "tools" / "c10r").symlink_to(c10r_binary)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv(binary.BINARY_ENV_VAR, "tools/c10r")

    located = binary.locate_binary()

    assert Path(located).is_absolute(), located


def test_a_binary_from_path_resolves_absolute(
    tmp_path: Path, c10r_binary: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The same holds for a binary discovered through a relative PATH entry."""
    (tmp_path / "bin").mkdir()
    (tmp_path / "bin" / "c10r").symlink_to(c10r_binary)
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv(binary.BINARY_ENV_VAR, raising=False)
    monkeypatch.setenv("PATH", "bin")

    located = binary.locate_binary()

    assert Path(located).is_absolute(), located


def test_the_located_binary_answers_from_another_working_directory(
    tmp_path: Path, c10r_binary: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A decoy at the same relative path inside the workspace is never run."""
    (tmp_path / "tools").mkdir()
    (tmp_path / "tools" / "c10r").symlink_to(c10r_binary)
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv(binary.BINARY_ENV_VAR, "tools/c10r")
    located = binary.locate_binary()

    workspace = tmp_path / "workspace"
    (workspace / "tools").mkdir(parents=True)
    decoy = workspace / "tools" / "c10r"
    decoy.write_text("#!/bin/sh\necho DECOY\nexit 0\n")
    decoy.chmod(0o755)

    completed = subprocess.run(  # noqa: S603
        [located, "--json", "manifest"], cwd=workspace, capture_output=True, check=False
    )

    assert completed.returncode == 0, completed.stderr.decode(errors="replace")
    assert b"DECOY" not in completed.stdout
    assert b"surface_version" in completed.stdout


async def test_a_cancelled_invocation_stops_the_command(tmp_path: Path, c10r_binary: str) -> None:
    """A cancelled build does not keep rewriting the store after the caller stops.

    Without cleanup the child outlives the cancelled call: the store appears
    seconds later, written by a process the caller believes it stopped.
    """
    write_fixture_crate(tmp_path)
    store = tmp_path / ".c10r" / "index.db"

    call = asyncio.create_task(binary.run(c10r_binary, ["build"], tmp_path))
    await asyncio.sleep(0.3)
    assert not store.exists(), "the build finished before it could be cancelled"

    call.cancel()
    with pytest.raises(asyncio.CancelledError):
        await call

    await asyncio.sleep(3.0)
    assert not store.exists(), "the cancelled build went on to write the store"
