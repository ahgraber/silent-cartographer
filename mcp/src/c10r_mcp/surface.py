"""The startup gate holding this server to the surface it was written against.

`c10r manifest` emits a versioned structural index of the command surface, and
its surface version changes whenever a command's structure changes. The server
compares that version against the one recorded here and refuses to serve on any
difference.

A difference does not prove a covered tool is affected, so refusing is stricter
than strictly necessary. It is chosen anyway: the alternative requires deciding
per tool whether a structural change touched it, and being wrong there produces
exactly the failure this gate exists to prevent — a tool offering an option the
binary no longer has.

The refusal is deliberately distinct from an index-state failure. A version
difference is fixed by aligning versions; an absent or incompatible index is
fixed by rebuilding, arrives per call, and carries `c10r`'s own diagnostic.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from c10r_mcp import binary

SURFACE_VERSION = 10
"""The command-surface version these tools were written against.

Raise this together with the tool definitions whenever `c10r`'s surface version
moves, having re-checked the affected tools against the new surface.
"""


class SurfaceMismatchError(RuntimeError):
    """The installed binary's command surface is not the one this server knows."""


async def read_manifest(binary_path: str, cwd: Path) -> dict[str, Any]:
    """Read the installed binary's structural surface index.

    `manifest` answers whether or not an index has been built, so this reports
    nothing about the workspace's index state.
    """
    completed = await binary.run(binary_path, ["manifest"], cwd)
    if completed.exit_code != 0:
        raise binary.BinaryUnavailableError(
            f"`{binary_path} manifest` exited {completed.exit_code}: {completed.stderr}"
        )
    try:
        manifest = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise binary.BinaryUnavailableError(f"`{binary_path} manifest` did not emit JSON: {exc}") from exc
    if not isinstance(manifest, dict):
        raise binary.BinaryUnavailableError(
            f"`{binary_path} manifest` emitted {type(manifest).__name__} rather than a "
            f"JSON object; this does not look like a `c10r` binary."
        )
    return manifest


async def check(binary_path: str, cwd: Path) -> int:
    """Confirm the installed surface is the recorded one; return its version.

    Raises `SurfaceMismatchError` on any difference, naming both versions and the
    remediation.
    """
    manifest = await read_manifest(binary_path, cwd)
    reported = manifest.get("surface_version")
    if not isinstance(reported, int) or isinstance(reported, bool):
        raise binary.BinaryUnavailableError(
            f"`{binary_path} manifest` reported a surface version of {reported!r}, "
            f"which is not a version number; this does not look like a `c10r` binary."
        )
    if reported != SURFACE_VERSION:
        raise SurfaceMismatchError(
            f"`{binary_path}` reports command-surface version {reported}; this server "
            f"was written against version {SURFACE_VERSION}. The two must match. "
            f"Upgrade the server package to one built against version {reported}, or "
            f"install a `c10r` whose surface version is {SURFACE_VERSION}. "
            f"This is a version-alignment failure, not an index-state failure: "
            f"rebuilding the index will not resolve it."
        )
    return reported
