"""One server instance serves both protocol eras, and the answer does not vary.

The `2026-07-28` revision removed the `initialize` handshake. That is a
protocol-level change rather than a transport-level one, so it reaches stdio even
though session identifiers never existed there. A client on either side of it
must reach the same tools and get the same answers.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from c10r_mcp.server import create_server
from conftest import (
    ERAS,
    HANDSHAKE_ERA,
    MODERN_ERA,
    accept_elicitation,
    call,
    client,
    init_git_repo,
    write_fixture_crate,
)
import pytest

if TYPE_CHECKING:
    from fastmcp import FastMCP


@pytest.mark.parametrize("era", ERAS)
async def test_a_client_of_either_era_lists_and_invokes_a_query_tool(server: FastMCP, era: str) -> None:
    """Both eras see the tools and get an answer from one."""
    async with client(server, era) as connected:
        listed = {tool.name for tool in await connected.list_tools()}
        result = await connected.call_tool("find", {"fragment": "double_value"}, raise_on_error=False)

    assert "find" in listed
    assert not result.is_error, result.content
    names = {row["name"] for row in result.structured_content["outcome"]["results"]}
    assert "double_value" in names


async def test_the_answer_does_not_vary_by_era(server: FastMCP) -> None:
    """The same query against the same server answers identically on both eras."""
    arguments = {"reference": "double_value", "relation": "references"}

    handshake = await call(server, "trace", arguments, era=HANDSHAKE_ERA)
    modern = await call(server, "trace", arguments, era=MODERN_ERA)

    assert not handshake.is_error, handshake.content
    assert handshake.structured_content == modern.structured_content


async def test_a_confirmed_build_answers_within_the_invocation(c10r_binary: str, tmp_path: Path) -> None:
    """No extension is needed to receive a build's answer; it completes in the call."""
    write_fixture_crate(tmp_path)
    init_git_repo(tmp_path)
    server = create_server(c10r_binary, tmp_path)

    result = await call(
        server,
        "build",
        {"acknowledge": True},
        elicitation_handler=accept_elicitation,
    )

    assert not result.is_error, result.content
    assert "aligned" in result.structured_content, result.structured_content
    assert (tmp_path / ".c10r").exists()
