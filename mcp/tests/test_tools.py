"""The advertised surface, and where an invalid invocation is caught.

A closed value set is typed as an enumeration so the schema rejects an out-of-set
value before any command runs, which prevents the error rather than reporting it.
A constraint spanning more than one parameter cannot be expressed in a schema, so
`c10r` owns it — which means a usage error has two producing paths, and each needs
its own evidence.
"""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

from c10r_mcp.errors import Category
from c10r_mcp.server import create_server
from conftest import HANDSHAKE_ERA, call, client
import pytest

if TYPE_CHECKING:
    from fastmcp import FastMCP

QUERY_TOOLS = {"get", "trace", "find", "search", "similar", "impact"}
LIFECYCLE_TOOLS = {"build", "hooks_install"}

INDEX_STATE_CATEGORIES = (
    Category.ABSENT_INDEX,
    Category.INCOMPATIBLE_STORE,
    Category.UNRECOGNIZED_STORE,
)
"""The categories describing the state of the store, each with its own recovery.

`unrecognized_store` is the one that must not be omitted: its recovery is the
opposite of the other two — report the file rather than build over it — so
instructions naming only the first two would point a caller at the single remedy
the ownership guard exists to withhold.
"""

COVERED_LANGUAGES = ("Rust", "Python")
"""The languages the graph indexes, which bound what these tools can answer."""


async def advertised(server: FastMCP) -> set[str]:
    """The tool names a client sees."""
    async with client(server) as connected:
        return {tool.name for tool in await connected.list_tools()}


async def test_the_tool_list_names_the_mirrored_commands(server: FastMCP) -> None:
    """One tool per covered query command, plus the two lifecycle tools, and nothing else."""
    assert await advertised(server) == QUERY_TOOLS | LIFECYCLE_TOOLS


async def test_no_tool_removes_the_stored_index(server: FastMCP) -> None:
    """The command that deletes the store is deliberately absent from the surface."""
    names = await advertised(server)

    assert "cache" not in names
    assert not {name for name in names if "reset" in name or "cache" in name}


async def test_a_bounded_invocation_discloses_its_truncation(server: FastMCP) -> None:
    """A cap returns no more than the cap, and says the result set was cut short."""
    result = await call(server, "find", {"fragment": "e", "limit": 2})

    assert not result.is_error, result.content
    answer = result.structured_content
    assert len(answer["outcome"]["results"]) <= 2, answer
    assert answer["page"]["truncated"] is True, answer


async def test_an_out_of_set_enum_value_is_rejected_before_any_command_runs(
    indexed_workspace: Path,
) -> None:
    """The schema catches an invalid enumerated value and names the accepted set.

    The server is pointed at a binary that does not exist, so reaching the command
    at all would fail differently — the rejection proves nothing ran.
    """
    server = create_server("/nonexistent/c10r", indexed_workspace)

    result = await call(server, "trace", {"reference": "double_value", "relation": "callers"})

    assert result.is_error
    assert result.structured_content["category"] == Category.USAGE, result.structured_content
    assert result.structured_content["exit_code"] is None, "no command ran, so there is no exit code"
    diagnostic = result.structured_content["diagnostic"]
    assert "callers" in diagnostic, diagnostic
    for accepted in ("containers", "contains", "references", "dependents"):
        assert accepted in diagnostic, diagnostic


async def test_both_usage_paths_report_the_same_category(server: FastMCP) -> None:
    """A schema rejection and a command rejection are one failure class to a caller.

    They are produced by different layers — the schema refuses an out-of-set value
    before anything runs, `c10r` refuses a combination no schema can express — so
    a caller that had to tell them apart would be branching on the wrapper's
    internals rather than on what went wrong.
    """
    by_schema = await call(server, "trace", {"reference": "x", "relation": "callers"})
    by_command = await call(server, "trace", {"reference": "double_value", "relation": "containers", "depth": 1})

    assert by_schema.structured_content["category"] == Category.USAGE
    assert by_command.structured_content["category"] == Category.USAGE
    assert by_schema.is_error
    assert by_command.is_error


async def test_an_option_that_does_not_apply_is_a_usage_error_from_the_command(
    server: FastMCP,
) -> None:
    """A cross-parameter constraint is caught by `c10r`, which names the valid form."""
    result = await call(
        server,
        "trace",
        {"reference": "double_value", "relation": "containers", "depth": 1},
    )

    assert result.is_error
    assert result.structured_content["category"] == Category.USAGE
    assert result.structured_content["exit_code"] == 2
    diagnostic = result.structured_content["diagnostic"]
    assert "--depth" in diagnostic, diagnostic
    assert "dependents" in diagnostic, diagnostic


@pytest.mark.parametrize("era", [HANDSHAKE_ERA, "auto"])
async def test_the_server_instructions_reach_a_client_on_either_era(server: FastMCP, era: str) -> None:
    """Instructions are served on the handshake era and through modern discovery.

    `auto` is how a modern client really connects: it probes `server/discover`,
    which is where the `2026-07-28` era carries instructions now that there is no
    handshake to carry them.
    """
    async with client(server, era) as connected:
        instructions = connected.instructions

    assert instructions, f"no instructions reached a {era} client"
    assert "grep" in instructions or "textual search" in instructions, instructions
    for tool in QUERY_TOOLS | LIFECYCLE_TOOLS:
        assert tool in instructions, f"the instructions never mention `{tool}`"
    for category in INDEX_STATE_CATEGORIES:
        assert category in instructions, f"the instructions name no recovery for `{category}`"
    for language in COVERED_LANGUAGES:
        assert language in instructions, f"the instructions never say the graph covers {language}"


@pytest.mark.parametrize("tool", sorted(QUERY_TOOLS | LIFECYCLE_TOOLS))
async def test_every_tool_carries_a_description(server: FastMCP, tool: str) -> None:
    """A tool with no description cannot be chosen between by a model reading the list."""
    async with client(server) as connected:
        described = {t.name: t.description for t in await connected.list_tools()}

    assert described[tool], f"{tool} advertises no description"
