"""Every invocation resolves to exactly one workspace, and answers from its store.

Working directory is the whole of workspace scoping: `c10r` resolves its store,
the store's recorded-root comparison, freshness, and git seeding from it, so
setting it correctly makes all of them correct together. Isolation between
workspaces is then structural — two roots are two files — rather than engineered.
"""

from __future__ import annotations

import asyncio
from pathlib import Path

from c10r_mcp.__main__ import main
from c10r_mcp.server import create_server
from c10r_mcp.workspace import WORKSPACE_ENV_VAR, WorkspaceRefusedError, resolve_launch_default
from conftest import call, client
import pytest


def names_found(result) -> set[str]:
    """The symbol names a `find` answer carries."""
    assert not result.is_error, result.content
    outcome = result.structured_content["outcome"]
    return {row["name"] for row in outcome.get("results", [])}


async def test_a_named_root_overrides_the_default(
    c10r_binary: str, indexed_workspace: Path, other_indexed_workspace: Path
) -> None:
    """Naming a root on the invocation answers from that root's store."""
    server = create_server(c10r_binary, indexed_workspace)

    result = await call(
        server,
        "find",
        {"fragment": "only_in_other", "root": str(other_indexed_workspace)},
    )

    assert "only_in_other" in names_found(result)


async def test_an_unnamed_invocation_uses_the_configured_default(
    c10r_binary: str, other_indexed_workspace: Path
) -> None:
    """An explicitly configured server answers from the root it was configured for."""
    server = create_server(c10r_binary, other_indexed_workspace)

    result = await call(server, "find", {"fragment": "only_in_other"})

    assert "only_in_other" in names_found(result)


async def test_an_unconfigured_server_falls_back_to_its_own_directory(
    c10r_binary: str,
    indexed_workspace: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """With nothing configured, the server answers about where it is running."""
    monkeypatch.delenv(WORKSPACE_ENV_VAR, raising=False)
    monkeypatch.chdir(indexed_workspace)
    server = create_server(c10r_binary, resolve_launch_default(None))

    result = await call(server, "find", {"fragment": "double_value"})

    assert "double_value" in names_found(result)


async def test_the_environment_variable_supplies_the_launch_default(
    c10r_binary: str, other_indexed_workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """With no explicit argument, the environment variable names the workspace.

    The middle arm of the precedence chain, and a documented public setting. The
    fallback test reaches the process directory by deleting this variable, which
    assumes it is honored without ever showing that it is.
    """
    monkeypatch.setenv(WORKSPACE_ENV_VAR, str(other_indexed_workspace))
    server = create_server(c10r_binary, resolve_launch_default(None))

    result = await call(server, "find", {"fragment": "only_in_other"})

    assert "only_in_other" in names_found(result)


def test_an_explicit_root_outranks_the_environment_variable(
    tmp_path: Path, indexed_workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A configured root wins over the variable, rather than the other way round."""
    monkeypatch.setenv(WORKSPACE_ENV_VAR, str(tmp_path))

    assert resolve_launch_default(str(indexed_workspace)) == indexed_workspace.resolve()


def test_an_unresolvable_root_from_the_environment_is_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """A root named by the variable is checked exactly as an explicit one is."""
    missing = tmp_path / "no-such-project"
    monkeypatch.setenv(WORKSPACE_ENV_VAR, str(missing))

    with pytest.raises(WorkspaceRefusedError, match=str(missing)):
        resolve_launch_default(None)


def test_an_unresolvable_configured_root_is_refused_at_startup(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], monkeypatch: pytest.MonkeyPatch
) -> None:
    """A configured root that does not exist stops the server rather than falling back."""
    missing = tmp_path / "no-such-project"
    monkeypatch.delenv(WORKSPACE_ENV_VAR, raising=False)

    status = main(["--workspace", str(missing)])

    assert status == 1
    refusal = capsys.readouterr().err
    assert str(missing) in refusal, refusal
    assert "not an existing directory" in refusal, refusal


def test_the_launch_default_refuses_rather_than_falling_back(tmp_path: Path) -> None:
    """The refusal is raised where the root is resolved, not deferred to a call."""
    missing = tmp_path / "absent"

    with pytest.raises(WorkspaceRefusedError, match=str(missing)):
        resolve_launch_default(str(missing))


async def test_concurrent_invocations_across_projects_do_not_cross(
    c10r_binary: str, indexed_workspace: Path, other_indexed_workspace: Path
) -> None:
    """Two roots answered at once each return only their own symbols."""
    server = create_server(c10r_binary, indexed_workspace)

    async with client(server) as connected:
        here, there = await asyncio.gather(
            connected.call_tool("find", {"fragment": "shared_name", "root": str(indexed_workspace)}),
            connected.call_tool("find", {"fragment": "only_in_other", "root": str(other_indexed_workspace)}),
        )

    assert "shared_name" in names_found(here)
    assert "only_in_other" not in names_found(here)
    assert "only_in_other" in names_found(there)
    assert "shared_name" not in names_found(there)


async def test_a_store_describing_another_workspace_is_disclosed(c10r_binary: str, relocated_workspace: Path) -> None:
    """An answer off a store built elsewhere says so rather than reading as matched."""
    server = create_server(c10r_binary, relocated_workspace)

    result = await call(server, "find", {"fragment": "double_value"})

    assert not result.is_error, result.content
    relation = result.structured_content.get("workspace_relation")
    assert relation is not None, result.structured_content
    assert relation["state"] == "mismatched", relation
    assert relation["recorded_root"], relation
