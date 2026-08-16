"""An answer only counts as an answer to a question this server asked.

The client owns the elicitation handler, so a client determined to consent on the
user's behalf always can; that is a property of the protocol, not a hole here.
What must hold is narrower and testable: an `input_responses` payload this server
did not solicit — unsolicited, replayed from another tool or another workspace, or
carrying something other than a boolean true — never reads as consent, and the
operation does not happen.

The framework seals the request state on the wire, so a hand-written state is
refused before it reaches this server at all. These tests therefore obtain a real
sealed state from a genuine first round and misuse that, which is the strongest
position a misbehaving client can actually occupy.
"""

from __future__ import annotations

from pathlib import Path

from c10r_mcp.server import create_server
from conftest import (
    HOOK_PATH,
    MODERN_ERA,
    RecordingElicitation,
    client,
    decline_elicitation,
    init_git_repo,
    untouched,
    write_fixture_crate,
)
from mcp.shared.exceptions import MCPError
import mcp_types
import pytest

TOOL = "hooks_install"


@pytest.fixture
def worktree(tmp_path: Path) -> Path:
    """A git worktree with no commit hook and no index."""
    write_fixture_crate(tmp_path)
    init_git_repo(tmp_path)
    return tmp_path


def acceptance(key: str = TOOL, value: object = True) -> dict:
    """The response shape a genuine confirmation round produces."""
    return {key: mcp_types.ElicitResult(action="accept", content={"value": value})}


def watcher() -> RecordingElicitation:
    """An elicitation handler that records every prompt and always declines."""
    return RecordingElicitation(decline_elicitation)


async def ask_once(server, tool: str = TOOL, arguments: dict | None = None):
    """Run a genuine first round and return the server's ask, sealed state and all."""
    async with client(server, MODERN_ERA, watcher()) as connected:
        return await connected.session.call_tool(tool, arguments or {"acknowledge": True}, allow_input_required=True)


async def replay(server, *, input_responses, request_state, arguments=None):
    """Send a response and a state of our choosing, as a misbehaving client would."""
    async with client(server, MODERN_ERA, watcher()) as connected:
        return await connected.session.call_tool(
            TOOL,
            arguments or {"acknowledge": True},
            input_responses=input_responses,
            request_state=request_state,
            allow_input_required=True,
        )


def asked_again(result) -> bool:
    """Whether the server responded by asking rather than by acting."""
    return getattr(result, "result_type", None) == "input_required"


async def attempt(server, **kwargs):
    """Replay, tolerating a refusal from any layer.

    The framework rejects a state sealed for a different call before this server
    sees it, so a cross-call replay may be refused on the wire rather than answered
    with a fresh ask. Either is the guarantee holding; the caller asserts on what
    the workspace looks like afterwards, which is the part that matters.
    """
    try:
        return await replay(server, **kwargs)
    except MCPError as refused:
        return refused


async def test_an_unsolicited_response_does_not_authorize(c10r_binary: str, worktree: Path) -> None:
    """A fabricated acceptance on the first call is not an answer to anything."""
    server = create_server(c10r_binary, worktree)

    result = await replay(server, input_responses=acceptance(), request_state=None)

    assert untouched(worktree), "the operation ran without a prompt"
    assert asked_again(result), result


async def test_a_state_minted_for_another_tool_does_not_authorize(c10r_binary: str, worktree: Path) -> None:
    """Consent obtained for `build` cannot be spent on the hook installation."""
    server = create_server(c10r_binary, worktree)
    for_build = await ask_once(server, tool="build")
    assert for_build.request_state, "the first round minted no state to replay"

    result = await attempt(server, input_responses=acceptance(), request_state=for_build.request_state)

    assert untouched(worktree)
    assert isinstance(result, MCPError) or asked_again(result), result


async def test_a_state_minted_for_another_workspace_does_not_authorize(
    c10r_binary: str, worktree: Path, tmp_path_factory: pytest.TempPathFactory
) -> None:
    """Consent obtained about one root cannot be carried to another."""
    elsewhere = tmp_path_factory.mktemp("elsewhere")
    write_fixture_crate(elsewhere)
    init_git_repo(elsewhere)
    server = create_server(c10r_binary, worktree)
    for_elsewhere = await ask_once(server, arguments={"acknowledge": True, "root": str(elsewhere)})
    assert for_elsewhere.request_state

    result = await attempt(
        server,
        input_responses=acceptance(),
        request_state=for_elsewhere.request_state,
    )

    assert untouched(worktree)
    assert untouched(elsewhere)
    assert isinstance(result, MCPError) or asked_again(result), result


@pytest.mark.parametrize("value", ["false", "true", 1, 0, None, "yes"])
async def test_only_a_boolean_true_reads_as_consent(c10r_binary: str, worktree: Path, value: object) -> None:
    """A truthy stand-in for the boolean the schema asked for is not consent."""
    server = create_server(c10r_binary, worktree)
    asked = await ask_once(server)

    result = await replay(
        server,
        input_responses=acceptance(value=value),
        request_state=asked.request_state,
    )

    assert untouched(worktree), f"value={value!r} was treated as consent"
    assert result.content[0].text.startswith("Not performed"), result.content[0].text


@pytest.mark.parametrize("action", ["decline", "cancel"])
async def test_a_refusing_response_performs_nothing(c10r_binary: str, worktree: Path, action: str) -> None:
    """A well-formed refusal carrying valid state still changes nothing."""
    server = create_server(c10r_binary, worktree)
    asked = await ask_once(server)

    result = await replay(
        server,
        input_responses={TOOL: mcp_types.ElicitResult(action=action)},
        request_state=asked.request_state,
    )

    assert untouched(worktree)
    assert result.content[0].text.startswith("Not performed"), result.content[0].text


async def test_a_genuine_round_still_authorizes(c10r_binary: str, worktree: Path) -> None:
    """The tightened check does not break the confirmation it exists to protect."""
    server = create_server(c10r_binary, worktree)
    asked = await ask_once(server)

    result = await replay(server, input_responses=acceptance(), request_state=asked.request_state)

    assert (worktree / HOOK_PATH).is_file(), result
