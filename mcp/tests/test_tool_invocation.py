"""Every query tool's parameters reach its mirrored command.

Each tool assembles its own argument list, so a mistake in one is invisible from
the others. `impact` is the least like the rest — it is the only tool that emits
a `--` separator before paths, and the only one with a bare flag — so its shapes
are exercised separately.

Each case is compared against the same command run in a shell, which is the same
standard the passthrough tests hold the answer to: the tool is correct when it
produces what the command produces, not when it produces something plausible.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

from c10r_mcp.server import create_server
from conftest import call, run_c10r
import pytest

if TYPE_CHECKING:
    from fastmcp import FastMCP


def direct(c10r_binary: str, args: list[str], root: Path) -> dict:
    """The answer the command produces when run in a shell."""
    completed = run_c10r(c10r_binary, args, root)
    assert completed.returncode == 0, completed.stderr.decode(errors="replace")
    return json.loads(completed.stdout)


QUERY_CASES = [
    pytest.param(
        "get",
        {"reference": "double_value", "detail": "signature"},
        ["get", "double_value", "--detail", "signature"],
        id="get",
    ),
    pytest.param(
        "trace",
        {"reference": "add_numbers", "relation": "references"},
        ["trace", "add_numbers", "--relation", "references"],
        id="trace",
    ),
    pytest.param(
        "find",
        {"fragment": "value", "limit": 5},
        ["find", "value", "--limit", "5"],
        id="find",
    ),
    pytest.param(
        "search",
        {"query": "adds two numbers together", "detail": "signature", "max_lines": 4, "limit": 3},
        ["search", "adds two numbers together", "--detail", "signature", "--max-lines", "4", "--limit", "3"],
        id="search",
    ),
    pytest.param(
        "similar",
        {"reference": "double_value", "detail": "location", "limit": 3},
        ["similar", "double_value", "--detail", "location", "--limit", "3"],
        id="similar",
    ),
]


@pytest.mark.parametrize(("tool", "arguments", "command"), QUERY_CASES)
async def test_a_query_tool_answers_as_its_command_does(
    server: FastMCP,
    c10r_binary: str,
    indexed_workspace: Path,
    tool: str,
    arguments: dict,
    command: list[str],
) -> None:
    """The tool's assembled invocation produces the command's own answer."""
    result = await call(server, tool, arguments)

    assert not result.is_error, result.content
    assert result.structured_content == direct(c10r_binary, command, indexed_workspace)


IMPACT_CASES = [
    pytest.param({}, ["impact"], id="working-tree"),
    pytest.param({"depth": 0}, ["impact", "--depth", "0"], id="aggregate-only"),
    pytest.param({"order": "unranked"}, ["impact", "--order", "unranked"], id="unranked"),
    pytest.param({"staged": True}, ["impact", "--staged"], id="staged"),
    pytest.param({"paths": ["src/lib.rs"]}, ["impact", "--", "src/lib.rs"], id="narrowed-to-paths"),
]


@pytest.mark.parametrize(("arguments", "command"), IMPACT_CASES)
async def test_impact_answers_as_its_command_does(
    c10r_binary: str, stale_workspace: Path, arguments: dict, command: list[str]
) -> None:
    """Each of `impact`'s shapes reaches the command, separator and bare flag alike."""
    server = create_server(c10r_binary, stale_workspace)

    result = await call(server, "impact", arguments)

    assert not result.is_error, result.content
    assert result.structured_content == direct(c10r_binary, command, stale_workspace)


async def test_the_staged_flag_changes_what_impact_seeds_from(c10r_binary: str, stale_workspace: Path) -> None:
    """A bare flag is not merely accepted; it selects a different seed.

    Comparing against the shell would pass even if the flag were dropped on both
    sides, so the answer's own account of what it seeded from is checked too.
    """
    server = create_server(c10r_binary, stale_workspace)

    working_tree = await call(server, "impact", {})
    staged = await call(server, "impact", {"staged": True})

    assert working_tree.structured_content["outcome"]["results"][0]["seed_mode"] == "working_tree"
    assert staged.structured_content["outcome"]["results"][0]["seed_mode"] == "staged"
