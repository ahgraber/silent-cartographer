"""The server refuses to serve a command surface it was not written against.

A surface-version difference does not prove a covered tool is affected, so
refusing is stricter than strictly necessary. It is chosen anyway: a loud startup
refusal carrying two version numbers is cheap to act on, and a subtly wrong tool
schema is not. The refusal is deliberately unlike an index-state failure, whose
remedy is a rebuild and which arrives per call.
"""

from __future__ import annotations

import json
from pathlib import Path
import stat

from c10r_mcp import surface
from c10r_mcp.__main__ import main
from c10r_mcp.binary import BINARY_ENV_VAR, BinaryUnavailableError, locate_binary
from c10r_mcp.server import create_server
from conftest import client
import pytest

STUB_TEMPLATE = """\
#!/bin/sh
if [ "$1" = "--json" ]; then shift; fi
if [ "$1" = "manifest" ]; then
  cat <<'JSON'
{json_payload}
JSON
  exit 0
fi
exit 1
"""


def write_stub_binary(path: Path, surface_version: int) -> Path:
    """A stand-in `c10r` reporting one chosen surface version and nothing else."""
    payload = json.dumps({"surface_version": surface_version, "help": "stub", "commands": [], "index": {}})
    path.write_text(STUB_TEMPLATE.format(json_payload=payload))
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return path


async def test_a_matching_surface_version_serves_its_tools(c10r_binary: str, indexed_workspace: Path) -> None:
    """The installed binary agrees with the recorded version, and the tools appear."""
    reported = await surface.check(c10r_binary, indexed_workspace)

    assert reported == surface.SURFACE_VERSION
    server = create_server(c10r_binary, indexed_workspace)
    async with client(server) as connected:
        assert {tool.name for tool in await connected.list_tools()}


async def test_a_differing_surface_version_refuses_and_names_both(tmp_path: Path, indexed_workspace: Path) -> None:
    """A different reported version stops the server, with both numbers and the remedy."""
    stub = write_stub_binary(tmp_path / "c10r", surface.SURFACE_VERSION + 1)

    with pytest.raises(surface.SurfaceMismatchError) as refusal:
        await surface.check(str(stub), indexed_workspace)

    message = str(refusal.value)
    assert str(surface.SURFACE_VERSION) in message, message
    assert str(surface.SURFACE_VERSION + 1) in message, message
    assert "version-alignment" in message, message
    assert "rebuilding the index will not resolve it" in message, message


def test_a_differing_surface_version_stops_the_entry_point(
    tmp_path: Path,
    indexed_workspace: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The refusal reaches the process boundary rather than being logged and ignored."""
    stub = write_stub_binary(tmp_path / "c10r", surface.SURFACE_VERSION + 1)
    monkeypatch.setenv(BINARY_ENV_VAR, str(stub))

    status = main(["--workspace", str(indexed_workspace)])

    assert status == 1
    refusal = capsys.readouterr().err
    assert "refusing to serve" in refusal, refusal
    assert str(surface.SURFACE_VERSION) in refusal, refusal


@pytest.mark.parametrize(
    ("emitted", "described"),
    [
        ("[]", "list"),
        ("null", "NoneType"),
        ('"seven"', "str"),
    ],
)
async def test_a_manifest_that_is_not_an_object_is_refused(
    tmp_path: Path, indexed_workspace: Path, emitted: str, described: str
) -> None:
    """Valid JSON that is not a surface index refuses rather than crashing."""
    stub = tmp_path / "c10r"
    stub.write_text(f"#!/bin/sh\nprintf '%s' '{emitted}'\n")
    stub.chmod(0o755)

    with pytest.raises(BinaryUnavailableError, match=described):
        await surface.check(str(stub), indexed_workspace)


async def test_a_manifest_without_a_version_number_is_refused(tmp_path: Path, indexed_workspace: Path) -> None:
    """A surface version that is not a number is refused, not compared."""
    stub = write_stub_binary(tmp_path / "c10r", surface.SURFACE_VERSION)
    stub.write_text(
        stub.read_text().replace(f'"surface_version": {surface.SURFACE_VERSION}', '"surface_version": "7"')
    )

    with pytest.raises(BinaryUnavailableError, match="not a version number"):
        await surface.check(str(stub), indexed_workspace)


def test_an_absent_binary_is_a_distinct_startup_failure(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """No binary at all names the binary, and is not a surface-version difference."""
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.delenv(BINARY_ENV_VAR, raising=False)

    with pytest.raises(BinaryUnavailableError, match="c10r") as failure:
        locate_binary()

    assert not isinstance(failure.value, surface.SurfaceMismatchError)


def test_an_unrunnable_binary_is_a_distinct_startup_failure(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A named path that cannot be executed reports against that path."""
    not_executable = tmp_path / "c10r"
    not_executable.write_text("not a program")
    monkeypatch.setenv(BINARY_ENV_VAR, str(not_executable))

    with pytest.raises(BinaryUnavailableError, match=str(not_executable)):
        locate_binary()
