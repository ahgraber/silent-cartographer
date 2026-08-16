"""A lifecycle operation happens behind verified consent, or not at all.

Two gates, guaranteeing different things. The acknowledgment parameter
guarantees deliberateness — the tool cannot be triggered by an agent exploring
the surface — but not consent, since the agent sets it. Confirmation is the
consent the server can verify. A client that cannot carry a confirmation is
refused rather than proceeding on the agent's own say-so, and because this
server's whole mechanism is running the CLI, the refusal always has a real
command to name.
"""

from __future__ import annotations

from pathlib import Path

from c10r_mcp.server import create_server
from conftest import (
    ERAS,
    HANDSHAKE_ERA,
    HOOK_PATH,
    MODERN_ERA,
    STORE_DIR,
    RecordingElicitation,
    accept_elicitation,
    call,
    decline_elicitation,
    init_git_repo,
    untouched,
    write_fixture_crate,
)
import pytest


@pytest.fixture
def fresh_workspace(tmp_path: Path) -> Path:
    """A git worktree with no index and no commit hook."""
    write_fixture_crate(tmp_path)
    init_git_repo(tmp_path)
    return tmp_path


@pytest.fixture
def fresh_server(c10r_binary: str, fresh_workspace: Path):
    """A server whose default root is that untouched worktree."""
    return create_server(c10r_binary, fresh_workspace)


async def test_an_unacknowledged_build_changes_nothing(fresh_server, fresh_workspace: Path) -> None:
    """The refusal states the cost, and no store is created."""
    result = await call(fresh_server, "build", {}, elicitation_handler=accept_elicitation)

    assert result.is_error
    assert untouched(fresh_workspace)
    message = " ".join(block.text for block in result.content)
    assert "acknowledge" in message, message
    assert "minutes" in message, message
    assert "c10r build" in message, message


async def test_an_unacknowledged_hook_installation_writes_nothing(fresh_server, fresh_workspace: Path) -> None:
    """The refusal states what would be written, and nothing is."""
    result = await call(fresh_server, "hooks_install", {}, elicitation_handler=accept_elicitation)

    assert result.is_error
    assert untouched(fresh_workspace)
    message = " ".join(block.text for block in result.content)
    assert "hook" in message, message
    assert "c10r hooks install" in message, message


async def test_an_unacknowledged_invocation_asks_the_client_nothing(fresh_server, fresh_workspace: Path) -> None:
    """The deliberateness gate is checked first, so no prompt reaches the user."""
    handler = RecordingElicitation()

    result = await call(fresh_server, "build", {}, elicitation_handler=handler)

    assert result.is_error
    assert handler.prompts == [], handler.prompts
    assert untouched(fresh_workspace)


@pytest.mark.parametrize("era", ERAS)
async def test_a_confirmed_build_is_performed(fresh_server, fresh_workspace: Path, era: str) -> None:
    """Acknowledged and confirmed, the index is built and the answer comes back."""
    result = await call(
        fresh_server,
        "build",
        {"acknowledge": True},
        era=era,
        elicitation_handler=accept_elicitation,
    )

    assert not result.is_error, result.content
    assert (fresh_workspace / STORE_DIR).exists()
    assert "aligned" in result.structured_content, result.structured_content


@pytest.mark.parametrize("era", ERAS)
async def test_a_confirmed_hook_installation_is_performed(fresh_server, fresh_workspace: Path, era: str) -> None:
    """Acknowledged and confirmed, the hook is written and the answer comes back."""
    result = await call(
        fresh_server,
        "hooks_install",
        {"acknowledge": True},
        era=era,
        elicitation_handler=accept_elicitation,
    )

    assert not result.is_error, result.content
    hook = fresh_workspace / HOOK_PATH
    assert hook.is_file()
    assert hook.stat().st_mode & 0o111, oct(hook.stat().st_mode)


@pytest.mark.parametrize("era", ERAS)
async def test_a_declined_confirmation_performs_nothing(fresh_server, fresh_workspace: Path, era: str) -> None:
    """Declining leaves every store, file, and repository as it was."""
    result = await call(
        fresh_server,
        "build",
        {"acknowledge": True},
        era=era,
        elicitation_handler=decline_elicitation,
    )

    assert not result.is_error, result.content
    assert result.structured_content == {"performed": False, "reason": "declined"}
    assert untouched(fresh_workspace)


@pytest.mark.parametrize("era", ERAS)
@pytest.mark.parametrize("tool", ["build", "hooks_install"])
async def test_a_client_that_cannot_be_asked_is_refused_with_its_remedy(
    fresh_server, fresh_workspace: Path, era: str, tool: str
) -> None:
    """No elicitation capability means no operation, and a command to run instead."""
    result = await call(fresh_server, tool, {"acknowledge": True}, era=era)

    assert result.is_error
    assert result.structured_content == {"performed": False, "reason": "cannot_confirm"}
    assert untouched(fresh_workspace)
    message = " ".join(block.text for block in result.content)
    assert "cannot be asked to confirm" in message, message
    assert ("c10r build" if tool == "build" else "c10r hooks install") in message, message


async def test_the_two_lifecycle_tools_do_not_read_each_other_s_confirmation(
    fresh_server, fresh_workspace: Path
) -> None:
    """Each tool's request is keyed by its own name, across a modern round trip."""
    handler = RecordingElicitation()

    await call(
        fresh_server,
        "hooks_install",
        {"acknowledge": True},
        era=MODERN_ERA,
        elicitation_handler=handler,
    )

    assert len(handler.prompts) == 1, handler.prompts
    assert "c10r hooks install" in handler.prompts[0], handler.prompts
    assert "c10r build" not in handler.prompts[0], handler.prompts


async def test_the_handshake_era_asks_through_the_session(fresh_server, fresh_workspace: Path) -> None:
    """The handshake era's mechanism is used, and it reaches the client."""
    handler = RecordingElicitation()

    result = await call(
        fresh_server,
        "hooks_install",
        {"acknowledge": True},
        era=HANDSHAKE_ERA,
        elicitation_handler=handler,
    )

    assert handler.prompts, "the handshake-era client was never asked"
    assert not result.is_error, result.content
    assert (fresh_workspace / HOOK_PATH).is_file()
