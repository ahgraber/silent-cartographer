"""Resolving the workspace root every invocation answers about.

Precedence is the root named on the invocation, then the root the server was
explicitly configured with at launch, then the directory the server process
itself resolves to. A configured root that is not an existing directory refuses
at startup rather than falling back, because a server quietly answering about
its own launch directory instead of the project it was configured for is a
correct answer about the wrong workspace.
"""

from __future__ import annotations

import os
from pathlib import Path

from fastmcp.exceptions import ToolError

WORKSPACE_ENV_VAR = "C10R_WORKSPACE"
"""The launch-time workspace root, when no explicit argument is given."""


class WorkspaceRefusedError(RuntimeError):
    """A configured workspace root did not resolve to an existing directory."""


def resolve_launch_default(configured: str | None = None) -> Path:
    """Resolve the root every invocation that names none will answer about.

    `configured` is the explicit launch argument; absent that, the environment
    variable; absent both, the directory the server process resolves to.
    """
    named = configured if configured is not None else os.environ.get(WORKSPACE_ENV_VAR)
    if named is None:
        return Path.cwd().resolve()

    root = Path(named).expanduser()
    if not root.is_dir():
        raise WorkspaceRefusedError(
            f"configured workspace root {named} is not an existing directory. "
            f"Point it at a directory, or leave it unset to use the server's own "
            f"working directory."
        )
    return root.resolve()


def resolve_call(named: str | None, launch_default: Path) -> Path:
    """Resolve the root one invocation answers about.

    A root named on the invocation overrides the launch-time default. It is
    checked here so a mistyped path is a stated refusal rather than an opaque
    failure to start the child process.
    """
    if named is None:
        return launch_default

    root = Path(named).expanduser()
    if not root.is_dir():
        raise ToolError(f"workspace root {named} is not an existing directory")
    return root.resolve()
