"""Each non-success outcome reaches its own category, carrying its own diagnostic.

The categories name mutually exclusive recoveries — build here, rebuild here, do
not build here at all — so two of them collapsing into one would send a caller to
the wrong remedy. The diagnostic is carried verbatim rather than reworded, so the
recovery instructions stay correct without this server knowing what they say.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
from typing import TYPE_CHECKING

from c10r_mcp.errors import Category
from c10r_mcp.server import create_server
from conftest import accept_elicitation, build_index, call, init_git_repo, write_fixture_crate
import pytest

if TYPE_CHECKING:
    from fastmcp import FastMCP

APPLICATION_ID = 0x63313072
"""The stamp `c10r` writes into a store it created, so it recognizes its own."""

STORE_PATH = Path(".c10r") / "index.db"


def error_of(result) -> dict:
    """The structured error a failed tool result carries."""
    assert result.is_error, result.structured_content
    return result.structured_content


@pytest.fixture
def unindexed_workspace(tmp_path: Path) -> Path:
    """A workspace nothing has been built in."""
    write_fixture_crate(tmp_path)
    return tmp_path


async def test_an_absent_index_names_its_recovery(c10r_binary: str, unindexed_workspace: Path) -> None:
    """No store at all is the absent-index category, and it names the rebuild."""
    server = create_server(c10r_binary, unindexed_workspace)

    error = error_of(await call(server, "find", {"fragment": "add"}))

    assert error["category"] == Category.ABSENT_INDEX
    assert error["exit_code"] == 3
    assert "build" in error["diagnostic"], error["diagnostic"]


async def test_an_incompatible_store_is_distinct_from_an_absent_index(c10r_binary: str, tmp_path: Path) -> None:
    """A store this binary created under another schema version is its own category."""
    write_fixture_crate(tmp_path)
    store = tmp_path / STORE_PATH
    store.parent.mkdir(parents=True)
    connection = sqlite3.connect(store)
    connection.executescript(
        f"CREATE TABLE placeholder (x); PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = 1;"
    )
    connection.close()
    server = create_server(c10r_binary, tmp_path)

    error = error_of(await call(server, "find", {"fragment": "add"}))

    assert error["category"] == Category.INCOMPATIBLE_STORE
    assert error["exit_code"] == 4
    assert error["category"] != Category.ABSENT_INDEX


async def test_an_unrecognized_store_is_distinct_from_both(c10r_binary: str, tmp_path: Path) -> None:
    """A file `c10r` cannot confirm is its own is refused as its own category."""
    write_fixture_crate(tmp_path)
    store = tmp_path / STORE_PATH
    store.parent.mkdir(parents=True)
    store.write_bytes(b"this file is not a c10r store")
    server = create_server(c10r_binary, tmp_path)

    error = error_of(await call(server, "find", {"fragment": "add"}))

    assert error["category"] == Category.UNRECOGNIZED_STORE
    assert error["exit_code"] == 6
    assert error["category"] not in {Category.ABSENT_INDEX, Category.INCOMPATIBLE_STORE}
    assert store.read_bytes() == b"this file is not a c10r store"


async def test_a_missing_language_indexer_is_distinct_from_every_index_state(
    c10r_binary: str, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An indexer the host does not carry is a setup failure, not an index state."""
    write_fixture_crate(tmp_path)
    init_git_repo(tmp_path)
    empty_path = tmp_path / "empty-bin"
    empty_path.mkdir()
    monkeypatch.setenv("PATH", str(empty_path))
    server = create_server(c10r_binary, tmp_path)

    error = error_of(
        await call(
            server,
            "build",
            {"acknowledge": True},
            elicitation_handler=accept_elicitation,
        )
    )

    assert error["category"] == Category.INDEXER_OR_SETUP
    assert error["exit_code"] == 5
    assert error["category"] not in {
        Category.ABSENT_INDEX,
        Category.INCOMPATIBLE_STORE,
        Category.UNRECOGNIZED_STORE,
    }


async def test_a_typed_empty_answer_is_not_an_error(
    c10r_binary: str, tmp_path_factory: pytest.TempPathFactory
) -> None:
    """A query resolving to no instances of its relation succeeds."""
    root = tmp_path_factory.mktemp("empty-relation")
    write_fixture_crate(root)
    init_git_repo(root)
    build_index(c10r_binary, root)
    server: FastMCP = create_server(c10r_binary, root)

    result = await call(server, "trace", {"reference": "unreferenced_leaf", "relation": "implementers"})

    assert not result.is_error, result.content
    assert result.structured_content["outcome"]["outcome"] == "empty"
