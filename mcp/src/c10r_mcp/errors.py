"""Turning a `c10r` outcome into a tool result.

`c10r` signals its outcome through a closed exit-code taxonomy and writes the
human-readable specifics to standard error. The exit code supplies the category
a caller branches on; the text supplies the recovery. Both are carried through:
the category as a field the caller can switch on, the diagnostic verbatim, so
the recovery instructions stay correct without this wrapper knowing what they
say.

A failure is reported as a tool result flagged in error rather than by raising,
because a raised error reaches the client as a string and the category would
have to be parsed back out of it.
"""

from __future__ import annotations

from enum import StrEnum
import json
from typing import TYPE_CHECKING, Any

from fastmcp.exceptions import ToolError, ValidationError
from fastmcp.server.middleware import Middleware
from fastmcp.tools.base import ToolResult

if TYPE_CHECKING:
    from c10r_mcp.binary import Completed

SUCCESS = 0
"""`c10r`'s success code. Covers a typed-empty answer, which is not a failure."""


class Category(StrEnum):
    """The outcome categories `c10r`'s exit taxonomy distinguishes.

    Each names a distinct recovery, so two of them must never collapse into one:
    an absent index invites a build, an incompatible store invites a rebuild, and
    an unrecognized store invites neither.
    """

    GENERAL = "general"
    USAGE = "usage"
    ABSENT_INDEX = "absent_index"
    INCOMPATIBLE_STORE = "incompatible_store"
    INDEXER_OR_SETUP = "indexer_or_setup"
    UNRECOGNIZED_STORE = "unrecognized_store"
    UNKNOWN = "unknown"
    """An exit code outside the taxonomy this server was written against."""


_BY_EXIT_CODE: dict[int, Category] = {
    1: Category.GENERAL,
    2: Category.USAGE,
    3: Category.ABSENT_INDEX,
    4: Category.INCOMPATIBLE_STORE,
    5: Category.INDEXER_OR_SETUP,
    6: Category.UNRECOGNIZED_STORE,
}


def categorize(exit_code: int) -> Category:
    """Return the category an exit code falls in."""
    return _BY_EXIT_CODE.get(exit_code, Category.UNKNOWN)


def usage(diagnostic: str) -> ToolResult:
    """Build a tool result reporting a rejected invocation.

    Carries no exit code, because no command ran: the invocation was refused
    before one could be started.
    """
    return ToolResult(
        content=diagnostic,
        structured_content={
            "category": str(Category.USAGE),
            "exit_code": None,
            "diagnostic": diagnostic,
        },
        is_error=True,
    )


def failure(completed: Completed) -> ToolResult:
    """Build a tool result reporting a non-success outcome.

    Carries the category, the raw exit code, and the diagnostic exactly as
    `c10r` wrote it.
    """
    return ToolResult(
        content=completed.stderr,
        structured_content={
            "category": str(categorize(completed.exit_code)),
            "exit_code": completed.exit_code,
            "diagnostic": completed.stderr,
        },
        is_error=True,
    )


def answer(completed: Completed) -> dict[str, Any]:
    """Return the structured answer a successful invocation produced.

    Parsed and returned with no field added, removed, renamed, or reordered —
    JSON object order survives the round trip, and every scalar is carried as
    written.
    """
    try:
        parsed = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise ToolError(f"`c10r` exited successfully but its answer did not parse as JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ToolError(f"`c10r` exited successfully but its answer was {type(parsed).__name__}, not a JSON object")
    return parsed


def result(completed: Completed) -> dict[str, Any] | ToolResult:
    """Return the tool result for a finished invocation, success or failure."""
    if completed.exit_code == SUCCESS:
        return answer(completed)
    return failure(completed)


class UsageErrors(Middleware):
    """Give a schema rejection the shape a command's own usage rejection has.

    A rejected invocation has two producing paths: the schema layer refuses an
    out-of-set enumerated value before any command runs, and `c10r` refuses a
    combination no schema can express. Both are the same thing to a caller —
    fix the arguments and retry — so both arrive as the usage category rather
    than one structured result and one block of framework prose.

    The framework's own message already names the accepted values, so it is
    carried as the diagnostic rather than reworded.
    """

    async def on_call_tool(self, context: Any, call_next: Any) -> Any:
        """Report a rejected invocation as a usage error rather than as raw prose."""
        try:
            return await call_next(context)
        except ValidationError as rejected:
            return usage(str(rejected))
