"""Obtaining the caller's confirmation before a lifecycle operation.

The two protocol eras ask differently and are strictly gated against each other:
the handshake era pushes a request down the session's back-channel mid-execution,
while `2026-07-28` removed that channel, so a tool asks by returning a request
and is re-invoked with the answer attached. Using either mechanism on the other
era raises rather than degrading, so a server serving both implements both and
selects at request time. Confining that branch here keeps it out of every tool.

The modern path re-runs the tool from the top on each round, so nothing is held
between rounds except the request state, which the framework seals on the way out
and verifies on the way back. That state is what makes an answer an answer:
without it, any `input_responses` a caller happened to send would read as a
completed round and the tool would act on a confirmation it never requested.

The two layers divide cleanly. The framework owns integrity and request binding —
it refuses a state it did not seal, and one sealed for a different method, tool,
or argument set, which is what stops an answer being carried between tools or
workspaces. This module owns the narrower question the framework cannot answer:
whether the state present is one *it* minted while asking, so an unsolicited
response is never mistaken for a reply.

What none of it can guarantee: the client owns the elicitation handler, so a
client determined to consent on the user's behalf always can. The guarantee is
that the server asked, in this exchange — never that a stray or replayed answer
passes for one.
"""

from __future__ import annotations

from enum import StrEnum
from typing import TYPE_CHECKING

import mcp_types
from mcp_types.version import MODERN_PROTOCOL_VERSIONS

if TYPE_CHECKING:
    from fastmcp.server.context import Context

_CONFIRMATION_FIELD = "value"
"""The single boolean field the confirmation form carries."""

_STATE_PREFIX = "c10r-confirm"
"""Marks state this module minted, so a foreign state never parses as a verdict."""


class Verdict(StrEnum):
    """What the caller said, or that they could not be asked."""

    CONFIRMED = "confirmed"
    DECLINED = "declined"
    """Declined, cancelled, or unanswerable; all leave everything unchanged."""

    CANNOT_ASK = "cannot_ask"
    """The client offers no elicitation capability, so consent is unobtainable."""


def _is_modern(ctx: Context) -> bool:
    """Whether the negotiated era removed the server-initiated back-channel."""
    request_context = ctx.request_context
    return request_context is not None and request_context.protocol_version in MODERN_PROTOCOL_VERSIONS


def _offers_elicitation(ctx: Context) -> bool:
    """Whether the connected client declared an elicitation capability."""
    capabilities = ctx.session.client_capabilities
    return capabilities is not None and capabilities.elicitation is not None


def _expected_state(key: str) -> str:
    """Return the request state a confirmation round for this operation carries.

    Identifies the round as one this module opened. It does not need to carry the
    tool's arguments: the framework seals the state against the request's own
    method, target, and argument digest, so a state minted for one call is
    already refused when echoed on another.
    """
    return f"{_STATE_PREFIX}:{key}"


def _ask(message: str, key: str) -> mcp_types.InputRequiredResult:
    """Build the modern era's ask: a request the client answers by re-invoking."""
    return mcp_types.InputRequiredResult(
        input_requests={
            key: mcp_types.ElicitRequest(
                params=mcp_types.ElicitRequestFormParams(
                    message=message,
                    requested_schema={
                        "type": "object",
                        "properties": {
                            _CONFIRMATION_FIELD: {
                                "type": "boolean",
                                "title": "Proceed",
                                "description": message,
                            }
                        },
                        "required": [_CONFIRMATION_FIELD],
                    },
                )
            )
        },
        request_state=_expected_state(key),
    )


def _reads_as_accepted(answer: mcp_types.InputResponse | None) -> bool:
    """Whether one response is an acceptance, judged strictly.

    The response content is client-supplied and only advisory-typed by the
    requested schema, so the confirmation field must be the boolean `True`
    itself. A truthy stand-in — the string `"false"`, say — is not consent.
    """
    if answer is None or getattr(answer, "action", None) != "accept":
        return False
    content = getattr(answer, "content", None) or {}
    return content.get(_CONFIRMATION_FIELD) is True


async def seek(ctx: Context, key: str, message: str) -> Verdict | mcp_types.InputRequiredResult:
    """Ask the caller to confirm, through the negotiated era's own mechanism.

    Returns a verdict, or — on `2026-07-28`, when nothing has been asked yet —
    the request the tool must return so the client can answer it. `key` names the
    request within the round; use the tool's own name so two lifecycle tools
    never read each other's answer.
    """
    if not _offers_elicitation(ctx):
        return Verdict.CANNOT_ASK

    if _is_modern(ctx):
        # Only a round this module opened can close one. An `input_responses`
        # payload arriving without the state minted while asking answers no
        # question that was put, so the round starts over rather than counting.
        if ctx.request_state != _expected_state(key):
            return _ask(message, key)
        return Verdict.CONFIRMED if _reads_as_accepted((ctx.input_responses or {}).get(key)) else Verdict.DECLINED

    accepted = await ctx.elicit(message, bool)
    if accepted.action != "accept":
        return Verdict.DECLINED
    return Verdict.CONFIRMED if accepted.data is True else Verdict.DECLINED
