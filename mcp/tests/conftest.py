"""Shared fixtures: the binary under test, and workspaces it can answer about.

Every fixture here rests on the real `c10r` binary and a real index built by it.
Nothing is stubbed: the whole point of this server is that it does not
reinterpret the CLI, so a test against a stub would prove nothing about the
thing being verified.
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
from typing import TYPE_CHECKING

from c10r_mcp.binary import BINARY_ENV_VAR, BINARY_NAME
from c10r_mcp.server import create_server
from fastmcp import Client, FastMCP
from fastmcp.client.elicitation import ElicitResult
import pytest

if TYPE_CHECKING:
    from fastmcp.client.client import CallToolResult

REPO_ROOT = Path(__file__).resolve().parents[2]

HANDSHAKE_ERA = "legacy"
MODERN_ERA = "2026-07-28"
ERAS = (HANDSHAKE_ERA, MODERN_ERA)

STORE_DIR = ".c10r"
"""The directory `c10r` keeps a workspace's index store in."""

HOOK_PATH = Path(".git") / "hooks" / "post-commit"
"""Where the commit hook lands in a worktree."""

FIXTURE_CARGO_TOML = """\
[package]
name = "fixture-crate"
version = "0.1.0"
edition = "2021"
"""

FIXTURE_LIB_RS = """\
/// Adds two numbers together.
pub fn add_numbers(left: u64, right: u64) -> u64 {
    left + right
}

/// Doubles a value by adding it to itself.
pub fn double_value(value: u64) -> u64 {
    add_numbers(value, value)
}

/// Carries a terminal-control escape sequence in its body, to prove content
/// round-trips byte-exactly rather than being sanitized in transit.
pub fn escaped_banner() -> &'static str {
    "\\x1b[31mred\\x1b[0m"
}

/// Stands in no relation to anything else, so tracing it yields typed absence.
pub fn unreferenced_leaf() -> u64 {
    7
}

pub mod alpha {
    /// Shares its shortname with `beta::shared_name`, so the bare name denotes
    /// more than one symbol and resolving it yields a candidate set.
    pub fn shared_name() -> u64 {
        1
    }
}

pub mod beta {
    /// Shares its shortname with `alpha::shared_name`.
    pub fn shared_name() -> u64 {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises `double_value`, so the heuristic-graded `tests` relation is
    /// non-empty and carries its classification label.
    #[test]
    fn doubling_adds_a_value_to_itself() {
        assert_eq!(double_value(3), 6);
    }
}
"""


@pytest.fixture(scope="session")
def c10r_binary() -> str:
    """The `c10r` binary under test.

    Fails rather than skips when none is present. A skipped conformance check
    reports as a pass, which is exactly the silent drift these tests exist to
    prevent.
    """
    override = os.environ.get(BINARY_ENV_VAR)
    if override:
        candidate = Path(override)
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
        pytest.fail(f"{BINARY_ENV_VAR} names {override}, which is not an executable file")

    for profile in ("release", "debug"):
        built = REPO_ROOT / "target" / profile / BINARY_NAME
        if built.is_file() and os.access(built, os.X_OK):
            return str(built)

    found = shutil.which(BINARY_NAME)
    if found:
        return found

    pytest.fail(
        f"no `{BINARY_NAME}` binary to test against. Build one with "
        f"`cargo build --release` in {REPO_ROOT}, or set {BINARY_ENV_VAR}."
    )


def run_c10r(binary: str, args: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    """Run `c10r` directly, the way a shell would, for comparison against a tool."""
    return subprocess.run(  # noqa: S603
        [binary, "--json", *args],
        cwd=cwd,
        capture_output=True,
        check=False,
    )


def write_fixture_crate(root: Path) -> None:
    """Lay down the small Rust crate every indexed workspace is built from."""
    (root / "src").mkdir(parents=True, exist_ok=True)
    (root / "Cargo.toml").write_text(FIXTURE_CARGO_TOML)
    (root / "src" / "lib.rs").write_text(FIXTURE_LIB_RS)


def init_git_repo(root: Path) -> None:
    """Make `root` a git worktree with one commit, so diff-seeded tools can run."""
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "fixture",
        "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
        "GIT_COMMITTER_NAME": "fixture",
        "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
    }
    # `commit.gpgsign` is disabled explicitly: the ambient user config may sign
    # commits with a key this fixture has no business reaching for.
    git = ["git", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]
    subprocess.run([*git, "init", "-q"], cwd=root, check=True, env=env)  # noqa: S603
    subprocess.run([*git, "add", "-A"], cwd=root, check=True, env=env)  # noqa: S603
    subprocess.run(  # noqa: S603
        [*git, "commit", "-q", "-m", "fixture"], cwd=root, check=True, env=env
    )


def build_index(binary: str, root: Path) -> None:
    """Build a real index in `root`, failing loudly if the indexer could not run."""
    built = run_c10r(binary, ["build"], root)
    if built.returncode != 0:
        pytest.fail(
            f"`c10r build` failed in {root} with exit {built.returncode}: {built.stderr.decode(errors='replace')}"
        )


def untouched(root: Path) -> bool:
    """Whether neither the store nor the commit hook has come into being."""
    return not (root / STORE_DIR).exists() and not (root / HOOK_PATH).exists()


@pytest.fixture(scope="session")
def indexed_workspace(c10r_binary: str, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A git worktree holding the fixture crate, with an index built over it."""
    root = tmp_path_factory.mktemp("indexed")
    write_fixture_crate(root)
    init_git_repo(root)
    build_index(c10r_binary, root)
    return root


@pytest.fixture(scope="session")
def other_indexed_workspace(c10r_binary: str, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """A second indexed workspace, holding a symbol the first one does not.

    Two roots are two stores with no coordination between them, which is what
    makes cross-workspace isolation structural rather than engineered.
    """
    root = tmp_path_factory.mktemp("other")
    (root / "src").mkdir(parents=True)
    (root / "Cargo.toml").write_text(FIXTURE_CARGO_TOML.replace("fixture-crate", "other-crate"))
    (root / "src" / "lib.rs").write_text(
        "/// Belongs to the second workspace alone.\npub fn only_in_other() -> u8 {\n    1\n}\n"
    )
    init_git_repo(root)
    build_index(c10r_binary, root)
    return root


@pytest.fixture(scope="session")
def stale_workspace(c10r_binary: str, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """An indexed workspace whose sources changed after the index was built."""
    root = tmp_path_factory.mktemp("stale")
    write_fixture_crate(root)
    init_git_repo(root)
    build_index(c10r_binary, root)
    with (root / "src" / "lib.rs").open("a") as source:
        source.write("\n/// Written after the index was built.\npub fn late_arrival() -> u64 {\n    1\n}\n")
    return root


@pytest.fixture(scope="session")
def relocated_workspace(indexed_workspace: Path, tmp_path_factory: pytest.TempPathFactory) -> Path:
    """An indexed workspace copied elsewhere, so its store records another root.

    The store carries the workspace root it was built for, so a copy queried in
    place is a store describing somewhere else — the condition the answer's
    workspace-relationship disclosure exists to make visible.
    """
    destination = tmp_path_factory.mktemp("relocated") / "workspace"
    shutil.copytree(indexed_workspace, destination, ignore=shutil.ignore_patterns("target"))
    return destination


@pytest.fixture
def server(c10r_binary: str, indexed_workspace: Path) -> FastMCP:
    """A server whose launch-time default is the indexed workspace."""
    return create_server(c10r_binary, indexed_workspace)


async def accept_elicitation(message, response_type, params, context) -> ElicitResult:
    """An elicitation handler that confirms."""
    return ElicitResult(action="accept", content={"value": True})


async def decline_elicitation(message, response_type, params, context) -> ElicitResult:
    """An elicitation handler that declines."""
    return ElicitResult(action="decline")


class RecordingElicitation:
    """An elicitation handler that records every prompt and delegates the answer.

    The recorded prompts are the evidence: that the user was asked at all, and
    what they were asked.
    """

    def __init__(self, answer=accept_elicitation) -> None:
        self.prompts: list[str] = []
        self.answer = answer

    async def __call__(self, message, response_type, params, context) -> ElicitResult:
        self.prompts.append(message)
        return await self.answer(message, response_type, params, context)


def client(server: FastMCP, era: str = MODERN_ERA, elicitation_handler=None) -> Client:
    """A client pinned to one protocol era, optionally able to be asked."""
    return Client(server, mode=era, elicitation_handler=elicitation_handler)


async def call(
    server: FastMCP,
    tool: str,
    arguments: dict | None = None,
    *,
    era: str = MODERN_ERA,
    elicitation_handler=None,
) -> CallToolResult:
    """Invoke one tool through a client and hand back the raw result.

    Errors are returned rather than raised, so a test can read the category and
    the diagnostic a failure carries instead of only its message.
    """
    async with client(server, era, elicitation_handler) as connected:
        return await connected.call_tool(tool, arguments or {}, raise_on_error=False)
