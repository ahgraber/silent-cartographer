"""The console entry point: check, then serve over stdio.

Three things are settled before a single tool is advertised, because each of them
makes every answer wrong rather than one answer wrong: the binary can be run, its
command surface is the one these tools were written against, and the launch-time
workspace root resolves.
"""

from __future__ import annotations

import argparse
import asyncio
from pathlib import Path
import sys

from setproctitle import setproctitle

from c10r_mcp import binary, surface, workspace
from c10r_mcp.server import create_server

PROCESS_TITLE = "c10r-mcp"


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="c10r-mcp",
        description="Serve the c10r code-graph CLI as MCP tools over stdio.",
    )
    parser.add_argument(
        "--workspace",
        metavar="PATH",
        default=None,
        help=(
            "The workspace root to answer about when a call names none. Defaults to "
            f"${workspace.WORKSPACE_ENV_VAR}, then to the server's own working "
            "directory. A path that is not an existing directory is refused rather "
            "than replaced by a fallback."
        ),
    )
    return parser.parse_args(argv)


async def _prepare(configured: str | None) -> tuple[str, Path]:
    """Resolve and check everything serving depends on, or raise."""
    launch_default = workspace.resolve_launch_default(configured)
    binary_path = binary.locate_binary()
    await surface.check(binary_path, launch_default)
    return binary_path, launch_default


def main(argv: list[str] | None = None) -> int:
    """Run the stdio server, or report why it will not start."""
    setproctitle(PROCESS_TITLE)
    args = _parse_args(argv)

    try:
        binary_path, launch_default = asyncio.run(_prepare(args.workspace))
    except (
        workspace.WorkspaceRefusedError,
        binary.BinaryUnavailableError,
        surface.SurfaceMismatchError,
    ) as refusal:
        print(f"c10r-mcp: refusing to serve: {refusal}", file=sys.stderr)
        return 1

    # No banner: on stdio the server's only job is the protocol, and a startup
    # banner is noise in whatever log the host points standard error at.
    create_server(binary_path, launch_default).run(transport="stdio", show_banner=False)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
