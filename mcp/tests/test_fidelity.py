"""The answer a tool returns is the answer the command produced.

Provenance, freshness, typed absence, candidate sets, and heuristic-grade labels
are the product, not decoration. A wrapper that reshapes the payload can drop
them, so each is compared against the command run directly rather than against a
description of what it should look like.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

from c10r_mcp.server import create_server
from conftest import call, run_c10r

if TYPE_CHECKING:
    from fastmcp import FastMCP


def direct(c10r_binary: str, args: list[str], root: Path) -> dict:
    """The answer the command produces when run in a shell."""
    completed = run_c10r(c10r_binary, args, root)
    assert completed.returncode == 0, completed.stderr.decode(errors="replace")
    return json.loads(completed.stdout)


async def test_a_result_bearing_answer_is_unaltered(
    server: FastMCP, c10r_binary: str, indexed_workspace: Path
) -> None:
    """A tool's answer equals the mirrored command's, field for field."""
    result = await call(server, "find", {"fragment": "double_value"})

    assert not result.is_error, result.content
    assert result.structured_content == direct(c10r_binary, ["find", "double_value"], indexed_workspace)


def field_order(value: object) -> object:
    """The shape of a value reduced to its key order, at every depth.

    Dictionaries compare equal regardless of insertion order, so an answer whose
    fields were reordered passes an `==` check untouched. This projects the order
    itself into something that does not.
    """
    if isinstance(value, dict):
        return [(key, field_order(nested)) for key, nested in value.items()]
    if isinstance(value, list):
        return [field_order(item) for item in value]
    return None


async def test_no_field_is_reordered(server: FastMCP, c10r_binary: str, indexed_workspace: Path) -> None:
    """The answer's fields arrive in the order the command emitted them.

    The remaining clause of the passthrough contract: equality alone cannot see a
    reordering, so the key order is compared on its own.
    """
    arguments = {"reference": "double_value", "relation": "references"}
    result = await call(server, "trace", arguments)
    command = direct(c10r_binary, ["trace", "double_value", "--relation", "references"], indexed_workspace)

    assert not result.is_error, result.content
    assert list(result.structured_content) == list(command), "top-level fields were reordered"
    assert field_order(result.structured_content) == field_order(command)


async def test_a_typed_empty_answer_survives_as_typed_absence(
    server: FastMCP, c10r_binary: str, indexed_workspace: Path
) -> None:
    """Absence arrives as the command's typed empty, not as results or an error."""
    arguments = {"reference": "unreferenced_leaf", "relation": "implementers"}
    result = await call(server, "trace", arguments)

    assert not result.is_error, result.content
    answer = result.structured_content
    assert answer["outcome"]["outcome"] == "empty", answer
    assert "results" not in answer["outcome"], answer
    assert answer == direct(
        c10r_binary,
        ["trace", "unreferenced_leaf", "--relation", "implementers"],
        indexed_workspace,
    )


async def test_an_ambiguous_reference_returns_its_candidate_set(
    server: FastMCP, c10r_binary: str, indexed_workspace: Path
) -> None:
    """A shortname denoting several symbols yields candidates, not a guess."""
    result = await call(server, "get", {"reference": "shared_name"})

    assert not result.is_error, result.content
    answer = result.structured_content
    assert answer["outcome"]["outcome"] == "ambiguous", answer
    assert len(answer["outcome"]["candidates"]) > 1, answer
    assert answer == direct(c10r_binary, ["get", "shared_name"], indexed_workspace)


async def test_calibration_labels_reach_the_caller(c10r_binary: str, stale_workspace: Path) -> None:
    """A heuristic-grade answer off a changed index keeps every label it carries."""
    server = create_server(c10r_binary, stale_workspace)
    arguments = {"reference": "double_value", "relation": "tests"}

    result = await call(server, "trace", arguments)

    assert not result.is_error, result.content
    answer = result.structured_content
    assert answer["classification"] == "convention", answer
    assert answer["freshness"] == "stale_content", answer
    assert answer["stale"] is True, answer
    assert answer["provenance"]["analyzer_name"], answer
    assert answer == direct(
        c10r_binary,
        ["trace", "double_value", "--relation", "tests"],
        stale_workspace,
    )


async def test_body_content_round_trips_byte_exactly(
    server: FastMCP, c10r_binary: str, indexed_workspace: Path
) -> None:
    """A body embedding a terminal-control sequence arrives exactly as written."""
    arguments = {"reference": "escaped_banner", "detail": "body"}
    result = await call(server, "get", arguments)

    assert not result.is_error, result.content
    body = result.structured_content["outcome"]["results"][0]["payload"]["body"]
    expected = direct(c10r_binary, ["get", "escaped_banner", "--detail", "body"], indexed_workspace)["outcome"][
        "results"
    ][0]["payload"]["body"]
    assert body == expected
    assert "\\x1b[31m" in body, body
