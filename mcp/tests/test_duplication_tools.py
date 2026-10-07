"""The `clones` tool, and the `subjects`, `mutual`, and `exclude` parameters of `similar`.

Each case runs against the real binary and the fixture crate's index. The fixture
holds two functions that differ only in a literal, so they are token-equivalent,
and a test that uses `double_value`, so `similar` carries a `tests_subject` row.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

from c10r_mcp.errors import Category
from conftest import call, run_c10r

if TYPE_CHECKING:
    from fastmcp import FastMCP

SUBJECTS = ["double_value", "add_numbers", "unreferenced_leaf"]


def labels(answer: dict) -> set[str]:
    """Every relation label carried by the rows of one `similar` answer."""
    return {relation["label"] for row in answer["outcome"]["results"] for relation in row["relations"]}


async def test_clones_returns_groups_graded_as_token_equivalence(server: FastMCP) -> None:
    result = await call(server, "clones")

    assert not result.is_error, result.content
    answer = result.structured_content
    assert answer["grade"] == "token_equivalence", answer
    assert answer["outcome"]["results"], answer


async def test_clones_matches_the_command(server: FastMCP, c10r_binary: str, indexed_workspace: Path) -> None:
    result = await call(server, "clones", {"limit": 1})

    completed = run_c10r(c10r_binary, ["clones", "--limit", "1"], indexed_workspace)
    assert result.structured_content == json.loads(completed.stdout)


async def test_similar_answers_every_subject_in_list_order(server: FastMCP) -> None:
    result = await call(server, "similar", {"subjects": SUBJECTS, "limit": 2})

    assert not result.is_error, result.content
    answers = result.structured_content["answers"]
    assert [entry["reference"] for entry in answers] == SUBJECTS, answers


async def test_exclude_removes_labeled_rows(server: FastMCP) -> None:
    before = await call(server, "similar", {"reference": "double_value"})
    after = await call(server, "similar", {"reference": "double_value", "exclude": ["tests_subject"]})

    assert "tests_subject" in labels(before.structured_content)
    assert "tests_subject" not in labels(after.structured_content)


async def test_mutual_returns_pairs(server: FastMCP) -> None:
    result = await call(server, "similar", {"mutual": True, "limit": 3})

    assert not result.is_error, result.content
    for pair in result.structured_content["outcome"]["results"]:
        assert {"lower", "upper"} <= set(pair), pair


async def test_an_out_of_set_exclude_value_is_rejected(server: FastMCP) -> None:
    result = await call(server, "similar", {"reference": "double_value", "exclude": ["sibling"]})

    assert result.is_error
    assert result.structured_content["category"] == Category.USAGE
    assert result.structured_content["exit_code"] is None
    assert "sibling" in result.structured_content["diagnostic"]


async def test_conflicting_similar_parameters_are_rejected_by_the_command(server: FastMCP) -> None:
    for arguments in (
        {"subjects": SUBJECTS, "reference": "double_value"},
        {"subjects": SUBJECTS, "mutual": True},
        {"mutual": True, "at": "src/lib.rs:2"},
    ):
        result = await call(server, "similar", arguments)

        assert result.is_error, arguments
        assert result.structured_content["category"] == Category.USAGE, arguments
        assert result.structured_content["exit_code"] == 2, arguments


async def test_a_subject_spanning_lines_is_rejected_before_any_command_runs(server: FastMCP) -> None:
    result = await call(server, "similar", {"subjects": ["double_value\nadd_numbers"]})

    assert result.is_error
    assert result.structured_content["category"] == Category.USAGE
    assert result.structured_content["exit_code"] is None
