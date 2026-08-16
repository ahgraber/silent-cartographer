"""The entry point serves over a real stdio transport, on both eras.

Every other test drives the server in-process. That proves the tools but not the
thing a host actually launches: argument parsing, the startup checks, and the
stdio transport itself. This exercises that path end to end.
"""

from __future__ import annotations

from pathlib import Path
import sys

from conftest import ERAS
from fastmcp import Client
from fastmcp.client.transports import StdioTransport
import pytest


def entry_point(binary: str, root: Path) -> StdioTransport:
    """The server as a host would launch it: a child process speaking stdio."""
    return StdioTransport(
        command=sys.executable,
        args=["-m", "c10r_mcp", "--workspace", str(root)],
        env={"C10R_BINARY": binary},
        keep_alive=False,
    )


@pytest.mark.parametrize("era", ERAS)
async def test_the_entry_point_serves_over_stdio(c10r_binary: str, indexed_workspace: Path, era: str) -> None:
    """A launched process lists its tools and answers a query about its workspace."""
    async with Client(entry_point(c10r_binary, indexed_workspace), mode=era) as connected:
        listed = {tool.name for tool in await connected.list_tools()}
        result = await connected.call_tool("find", {"fragment": "double_value"}, raise_on_error=False)

    assert "find" in listed
    assert not result.is_error, result.content
    names = {row["name"] for row in result.structured_content["outcome"]["results"]}
    assert "double_value" in names
