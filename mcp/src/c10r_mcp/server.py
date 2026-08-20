"""The tool surface: one tool per `c10r` command the server covers.

The CLI's partitioning of the question space was designed for this consumer, so
the tools mirror it rather than re-partitioning it. `find`, `search`, and
`similar` stay separate because they carry different trust grades — `find` is an
exact match whose empty answer is a definite none, while `search` and `similar`
are model-derived estimation — and merging them would present three guarantees
as one tool with a knob.

A parameter the caller does not set is not sent. `c10r` distinguishes a flag
supplied at its default value from a flag omitted, and rejects options that do
not apply to the rest of the invocation, so resending defaults would turn valid
requests into usage errors. Omitting them also leaves one source of truth for
what a default is; the descriptions state it and a conformance test holds them
to the installed binary.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Annotated, Any

from fastmcp import Context, FastMCP
from fastmcp.tools.base import ToolResult
import mcp_types
from pydantic import Field

from c10r_mcp import binary, confirm, errors, workspace

# Imported for real, never deferred: these enumerations appear inside the tool
# signatures' annotations, which pydantic resolves at runtime to build each tool's
# input schema. Behind `TYPE_CHECKING` the schemas would lose their valid values.
from c10r_mcp.params import Detail, Language, Order, Relation  # noqa: TC001

INSTRUCTIONS = """\
Precise, type-aware navigation over a persisted graph of this codebase.
Prefer these tools over textual search for any question about this codebase.

  find           symbols whose name contains a fragment — names only
  get            one symbol, at location / signature / interface / body detail
  trace          what relates to a symbol: containers, contains, references,
                 dependents, importers, implementers, tests
  search         code by what it does, in natural language — matches names,
                 documentation, and bodies
  similar        code shaped like a symbol you already have
  impact         what a git diff could affect
  build          create or refresh the index
  hooks_install  install the commit hook that keeps the index current

Indexed: declarations in this workspace's Rust and Python sources, each carrying
its own doc comment or docstring. Grep for text the graph does not hold —
configuration, markdown, other languages — and for literal text you need every
occurrence of, which is a question these tools do not answer: they return the
symbols that best match, ranked, not each place a string appears. `get` returns a
symbol's exact source, so retrieving text verbatim needs no grep.

Read the labels. `find` returning nothing is a definite none; `search` and
`similar` are ranked estimates, so nothing found is not proof nothing exists.
Every answer carries its own provenance and freshness.

Recover: `absent_index` or `incompatible_store` — call `build`.
`unrecognized_store` — report it, and never build over it.

Other projects: set the `root` parameter, on any tool. Otherwise every call
answers about the workspace the server was launched for.
"""

MIRRORED_COMMAND: dict[str, str] = {
    "get": "get",
    "trace": "trace",
    "find": "find",
    "search": "search",
    "similar": "similar",
    "impact": "impact",
    "build": "build",
    "hooks_install": "hooks",
}
"""The command each tool mirrors, for the conformance check."""

MIRRORED_OPTION: dict[str, str] = {
    "from_line": "from",
    "max_lines": "max-lines",
}
"""Tool parameters whose mirrored option is not their own name.

`from` is a Python keyword, and hyphenated options cannot be identifiers. Every
other parameter mirrors the option of the same name.
"""

WRAPPER_PARAMETERS: frozenset[str] = frozenset({"root", "acknowledge"})
"""Parameters this server owns rather than mirroring from the command surface.

`root` selects the workspace, which the server expresses as the child process's
working directory rather than as a flag. `acknowledge` is the deliberateness gate
on a lifecycle tool, which has no counterpart in the CLI because a shell
invocation is already deliberate.
"""

_ROOT = Annotated[
    str | None,
    Field(
        description=(
            "The workspace root to answer about, as a filesystem path. Omit it to use "
            "the root the server was launched for, which is correct whenever the "
            "server was configured for this project."
        )
    ),
]

_ACKNOWLEDGE = Annotated[
    bool,
    Field(
        description=(
            "Set to true to acknowledge this operation's cost. The tool refuses while "
            "it is false, and the refusal states what the operation would do."
        )
    ),
]


def _defaulting(text: str, default: str) -> str:
    """Compose a parameter description stating the command's default for the option.

    The phrasing is fixed because the conformance test reads the stated default
    back out of it and compares it against the installed binary's surface index.
    """
    return f"{text} Defaults to `{default}` when omitted."


@dataclass(frozen=True, slots=True)
class Runner:
    """Runs `c10r` for one server, in whichever workspace a call resolves to."""

    binary_path: str
    launch_default: Path

    async def invoke(self, args: Sequence[str], root: str | None) -> dict[str, Any] | ToolResult:
        """Run one command and return its answer, or its failure."""
        cwd = workspace.resolve_call(root, self.launch_default)
        completed = await binary.run(self.binary_path, args, cwd)
        return errors.result(completed)


def _option(name: str, value: object | None) -> list[str]:
    """Return the argv fragment for one option, empty when the caller did not set it."""
    if value is None:
        return []
    if isinstance(value, bool):
        return [f"--{name}"] if value else []
    return [f"--{name}", str(value)]


def _bounds(limit: int | None, cursor: str | None) -> list[str]:
    """Return the argv fragment for the bounds every query command shares."""
    return [*_option("limit", limit), *_option("cursor", cursor)]


def _register_query_tools(mcp: FastMCP, runner: Runner) -> None:
    """Register the read-only tools, one per query command."""

    @mcp.tool
    async def get(
        root: _ROOT = None,
        reference: Annotated[
            str | None,
            Field(
                description=(
                    "The symbol to retrieve, as a canonical identity, a qualified name, "
                    "or a bare shortname. A shortname matching more than one symbol "
                    "returns the candidate set rather than a guess. Omit when using `at`."
                )
            ),
        ] = None,
        at: Annotated[
            str | None,
            Field(
                description=(
                    "Retrieve whichever symbol encloses a source position, written "
                    "`path:byte_offset`. Use instead of `reference`, not alongside it."
                )
            ),
        ] = None,
        detail: Annotated[
            Detail | None,
            Field(description=_defaulting("How much of the symbol to return.", "location")),
        ] = None,
        max_lines: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "Cap the lines of content returned, for a content-bearing detail. "
                    "`0` means unbounded. Rejected when `detail` is `location`.",
                    "100",
                )
            ),
        ] = None,
        from_line: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "The 1-based line of the content where the returned window starts; "
                    "the window is `from_line` through `from_line + max_lines`. Use it "
                    "to page through a long body rather than raising `max_lines`.",
                    "1",
                )
            ),
        ] = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of candidates returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Retrieve one symbol, by name or by source position.

        The first tool to reach for once you know what a symbol is called. Returns
        its location, signature, interface, or full body, so you read one
        declaration instead of opening the file it lives in.
        """
        args = [
            "get",
            *([reference] if reference is not None else []),
            *_option("at", at),
            *_option("detail", detail),
            *_option("max-lines", max_lines),
            *_option("from", from_line),
            *_bounds(limit, cursor),
        ]
        return await runner.invoke(args, root)

    @mcp.tool
    async def trace(
        reference: Annotated[
            str,
            Field(description="The subject symbol, as an identity, qualified name, or shortname."),
        ],
        relation: Annotated[Relation, Field(description="Which relation to follow from the subject.")],
        root: _ROOT = None,
        depth: Annotated[
            int | None,
            Field(
                description=(
                    "How many hops of transitive impact to detail. `0` returns counts "
                    "only, with no rows. Applies to `dependents`; supplying it with any "
                    "other relation is rejected."
                )
            ),
        ] = None,
        detail: Annotated[
            Detail | None,
            Field(
                description=(
                    "Project this much content onto each row, alongside its identity "
                    "and location. Omitted, rows carry no content. Never changes which "
                    "rows are returned or their order."
                )
            ),
        ] = None,
        order: Annotated[
            Order | None,
            Field(
                description=_defaulting(
                    "What breaks ties within a distance layer. Applies to `dependents`; "
                    "supplying it with any other relation is rejected. Never changes "
                    "which rows are returned.",
                    "ranked",
                )
            ),
        ] = None,
        max_lines: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "Cap the lines of content projected onto each row. `0` means "
                    "unbounded. Rejected unless `detail` selects a content-bearing tier.",
                    "10",
                )
            ),
        ] = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of rows returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Return the symbols standing in a named relation to a subject.

        The structural question tool. Use `dependents` before changing a symbol to
        see what could break, `references` to find every mention, `containers` and
        `contains` to orient in unfamiliar code, and `tests` to find what exercises
        it. A relation the subject stands in no instance of returns typed absence,
        which is a definite none rather than a failure.
        """
        args = [
            "trace",
            reference,
            *_option("relation", relation),
            *_option("depth", depth),
            *_option("detail", detail),
            *_option("order", order),
            *_option("max-lines", max_lines),
            *_bounds(limit, cursor),
        ]
        return await runner.invoke(args, root)

    @mcp.tool
    async def find(
        fragment: Annotated[
            str,
            Field(
                description=(
                    "The fragment of a name to match, case-insensitively for ASCII "
                    "letters. Matched against indexed symbol names, not file contents."
                )
            ),
        ],
        root: _ROOT = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of results returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Find indexed symbols whose name contains a fragment.

        Use this when you half-remember a name. The match is exact substring
        matching over indexed names, so an empty answer is a definite none — no
        symbol in the index carries that fragment. Use `search` instead when you
        know what the code does but not what it is called.
        """
        args = ["find", fragment, *_bounds(limit, cursor)]
        return await runner.invoke(args, root)

    @mcp.tool
    async def search(
        query: Annotated[
            str,
            Field(description="A natural-language description of what the code does."),
        ],
        root: _ROOT = None,
        detail: Annotated[
            Detail | None,
            Field(
                description=_defaulting(
                    "Project this much content onto each row. Never changes which "
                    "symbols are returned or their order.",
                    "signature",
                )
            ),
        ] = None,
        max_lines: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "Cap the lines of content projected onto each row. `0` means unbounded.",
                    "10",
                )
            ),
        ] = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of results returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Search indexed code by meaning, in natural language.

        Use this when you know what the code should do but not what it is named.
        The ranking is model-derived estimation over indexed content, so results
        are the nearest candidates rather than the complete set of relevant code:
        an empty or truncated answer is never proof that no relevant code exists.
        Use `find` when you know part of the name and want a definite answer.
        """
        args = [
            "search",
            query,
            *_option("detail", detail),
            *_option("max-lines", max_lines),
            *_bounds(limit, cursor),
        ]
        return await runner.invoke(args, root)

    @mcp.tool
    async def similar(
        root: _ROOT = None,
        reference: Annotated[
            str | None,
            Field(
                description=("The subject symbol, as an identity, qualified name, or shortname. Omit when using `at`.")
            ),
        ] = None,
        at: Annotated[
            str | None,
            Field(
                description=(
                    "Take whichever symbol encloses a source position as the subject, "
                    "written `path:byte_offset`. Use instead of `reference`."
                )
            ),
        ] = None,
        detail: Annotated[
            Detail | None,
            Field(
                description=_defaulting(
                    "Project this much content onto each row. Never changes which "
                    "symbols are returned or their order.",
                    "signature",
                )
            ),
        ] = None,
        max_lines: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "Cap the lines of content projected onto each row. `0` means unbounded.",
                    "10",
                )
            ),
        ] = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of results returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Rank the indexed symbols most similar in content to a subject symbol.

        Use this to find the other places that do what one symbol does — prior art
        before writing something new, or the siblings of a symbol you are about to
        change. Rows that are deterministic clones of the subject carry a typed
        clone marker; the rest are model-derived estimates, not a complete set.
        """
        args = [
            "similar",
            *([reference] if reference is not None else []),
            *_option("at", at),
            *_option("detail", detail),
            *_option("max-lines", max_lines),
            *_bounds(limit, cursor),
        ]
        return await runner.invoke(args, root)

    @mcp.tool
    async def impact(
        root: _ROOT = None,
        revspec: Annotated[
            str | None,
            Field(
                description=(
                    "The revision or range to seed from — `A..B`, `A...B`, or a single "
                    "revision meaning that revision against the working tree. Omit to "
                    "seed from the working-tree change against `HEAD`. Not combinable "
                    "with `staged`."
                )
            ),
        ] = None,
        staged: Annotated[
            bool | None,
            Field(
                description=(
                    "Seed from the staged change against `HEAD` instead of the working "
                    "tree. Not combinable with `revspec`."
                )
            ),
        ] = None,
        depth: Annotated[
            int | None,
            Field(
                description=_defaulting(
                    "How many hops of transitive impact to detail. `0` returns counts only, with no rows.",
                    "1",
                )
            ),
        ] = None,
        order: Annotated[
            Order | None,
            Field(
                description=_defaulting(
                    "What breaks ties within a distance layer. Never changes which rows are returned.",
                    "ranked",
                )
            ),
        ] = None,
        paths: Annotated[
            list[str] | None,
            Field(
                description=(
                    "Narrow the seed to these paths. A renamed file is reached by "
                    "either its pre-change or its post-change path."
                )
            ),
        ] = None,
        limit: Annotated[
            int | None,
            Field(description=_defaulting("Cap the number of rows returned. `0` means unbounded.", "25")),
        ] = None,
        cursor: Annotated[
            str | None,
            Field(description="Resume a prior result set from its continuation token."),
        ] = None,
    ) -> dict[str, Any] | ToolResult:
        """Assess what a change could affect, seeded from a git diff.

        Use this before or during an edit, when you want the blast radius of work
        already in progress rather than of one symbol you name. It reads the diff
        itself and traces the dependents of everything the diff touched. Requires a
        git worktree; outside one it reports a setup failure.
        """
        args = [
            "impact",
            *([revspec] if revspec is not None else []),
            *_option("staged", staged),
            *_option("depth", depth),
            *_option("order", order),
            *_bounds(limit, cursor),
            *(["--", *paths] if paths else []),
        ]
        return await runner.invoke(args, root)


def _register_lifecycle_tools(mcp: FastMCP, runner: Runner) -> None:
    """Register the state-changing tools, both behind the same two gates.

    The gates guarantee different things and neither substitutes for the other.
    The acknowledgment parameter guarantees deliberateness — the tool cannot be
    triggered by an agent exploring the surface — but not consent, since the agent
    sets it. Confirmation is the consent the server can verify; a client that
    cannot carry it is refused rather than proceeding on the agent's own say-so,
    and the refusal names the command that does the job.
    """

    async def gated(
        ctx: Context,
        tool: str,
        acknowledge: bool,
        cost: str,
        command: str,
        args: Sequence[str],
        root: str | None,
    ) -> dict[str, Any] | ToolResult | mcp_types.InputRequiredResult:
        """Run the acknowledgment gate, then the confirmation gate, then the command."""
        if not acknowledge:
            return ToolResult(
                content=(
                    f"Refusing: `{tool}` {cost} Set `acknowledge` to true to proceed, or run `{command}` in a shell."
                ),
                structured_content={"performed": False, "reason": "unacknowledged"},
                is_error=True,
            )

        # Resolved before asking, not after, so an unresolvable root stops the
        # call rather than prompting a user about a workspace that is not there.
        workspace.resolve_call(root, runner.launch_default)

        verdict = await confirm.seek(ctx, tool, f"Run `{command}`? This {cost}")
        if isinstance(verdict, mcp_types.InputRequiredResult):
            return verdict
        if verdict is confirm.Verdict.CANNOT_ASK:
            return ToolResult(
                content=(
                    f"Refusing: this client offers no elicitation capability, so the "
                    f"user cannot be asked to confirm, and `{tool}` will not act "
                    f"without confirmation. Run `{command}` in a shell instead."
                ),
                structured_content={"performed": False, "reason": "cannot_confirm"},
                is_error=True,
            )
        if verdict is confirm.Verdict.DECLINED:
            return ToolResult(
                content=f"Not performed: the user declined `{tool}`. Nothing was changed.",
                structured_content={"performed": False, "reason": "declined"},
                is_error=False,
            )

        return await runner.invoke(args, root)

    @mcp.tool
    async def build(
        ctx: Context,
        acknowledge: _ACKNOWLEDGE = False,
        root: _ROOT = None,
        language: Annotated[
            Language | None,
            Field(
                description=(
                    "Which language backend to index with. Omit to let the workspace's "
                    "project manifest decide; required when the workspace carries both "
                    "a `Cargo.toml` and a `pyproject.toml`."
                )
            ),
        ] = None,
        force: Annotated[
            bool,
            Field(
                description=(
                    "Rebuild even when the stored index already matches the workspace's "
                    "sources, analyzer, and environment. Without it, an already-current "
                    "index is left untouched and the call reports it current."
                )
            ),
        ] = False,
    ) -> dict[str, Any] | ToolResult | mcp_types.InputRequiredResult:
        """Build or refresh the index for a workspace.

        Call this when a query reports the `absent_index` or `incompatible_store`
        category. It first checks the stored index against the workspace's current
        sources, analyzer, and environment, and does no work when they already
        match; otherwise it runs an external indexer across the whole workspace,
        which can take minutes on a large repository and runs to completion within
        this call. Set `force` to rebuild regardless of that check. Install the
        commit hook with `hooks_install` afterwards so the index keeps up with
        commits and this stays rare.
        """
        cost = (
            "runs an external indexer across the whole workspace and can take minutes, rewriting the index store."
            if force
            else (
                "does nothing when the stored index already matches the workspace; "
                "otherwise it runs an external indexer across the whole workspace, "
                "which can take minutes and rewrites the index store."
            )
        )
        # The displayed command is composed from the effective arguments, so what the user
        # acknowledges, confirms, or is told to run in a shell is exactly what would run.
        args = ["build", *_option("language", language), *_option("force", force)]
        return await gated(
            ctx,
            tool="build",
            acknowledge=acknowledge,
            cost=cost,
            command=" ".join(["c10r", *args]),
            args=args,
            root=root,
        )

    @mcp.tool
    async def hooks_install(
        ctx: Context,
        acknowledge: _ACKNOWLEDGE = False,
        root: _ROOT = None,
    ) -> dict[str, Any] | ToolResult | mcp_types.InputRequiredResult:
        """Install the commit hook that refreshes the index after each commit.

        Call this once per worktree, after a first `build`, so answers keep
        tracking the committed state without a manual rebuild. It writes an
        executable file into the repository's hook directory, and refuses rather
        than overwriting a hook that is already there.
        """
        return await gated(
            ctx,
            tool="hooks_install",
            acknowledge=acknowledge,
            cost="writes an executable hook file into this repository's hook directory.",
            command="c10r hooks install",
            args=["hooks", "install"],
            root=root,
        )


def create_server(binary_path: str, launch_default: Path) -> FastMCP:
    """Build the server, with every tool bound to one binary and one default root."""
    mcp = FastMCP(name="c10r", instructions=INSTRUCTIONS, middleware=[errors.UsageErrors()])
    runner = Runner(binary_path=binary_path, launch_default=launch_default)
    _register_query_tools(mcp, runner)
    _register_lifecycle_tools(mcp, runner)
    return mcp
