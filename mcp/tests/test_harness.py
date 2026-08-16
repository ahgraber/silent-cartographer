"""The harness proves itself before anything rests on it.

If the fixture workspace were not really indexed, every later test asserting an
answer would pass or fail for reasons unrelated to the server.
"""

from __future__ import annotations

import json
from pathlib import Path

from conftest import run_c10r


def test_the_fixture_workspace_answers_a_query_through_the_binary(c10r_binary: str, indexed_workspace: Path) -> None:
    """The located binary answers a real query against the built fixture index."""
    completed = run_c10r(c10r_binary, ["find", "double_value"], indexed_workspace)

    assert completed.returncode == 0, completed.stderr.decode(errors="replace")
    answer = json.loads(completed.stdout)
    assert answer["outcome"]["outcome"] == "found", answer
    names = [row["name"] for row in answer["outcome"]["results"]]
    assert "double_value" in names, answer
