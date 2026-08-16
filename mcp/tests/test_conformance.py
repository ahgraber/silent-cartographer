"""The tool schemas are held to the installed binary's own surface index.

The tools are hand-written, because a tool description is read by a model
choosing between tools while help text is read by a person who has already
chosen. This test recovers what generating them would have given for free: the
descriptions stay hand-written, but every factual claim in them is checked
against the binary rather than trusted.

It fails, rather than skips, when no binary is available. A skipped conformance
check reports as a pass, which is exactly the silent drift it exists to prevent.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
import re
from typing import Any

from c10r_mcp.server import MIRRORED_COMMAND, MIRRORED_OPTION, WRAPPER_PARAMETERS, create_server
from conftest import run_c10r
from fastmcp import Client, FastMCP
import pytest

STATED_DEFAULT = re.compile(r"Defaults to `([^`]+)` when omitted\.")


@pytest.fixture(scope="module")
def installed_surface(c10r_binary: str, tmp_path_factory: pytest.TempPathFactory) -> dict[str, Any]:
    """The installed binary's structural index, keyed by command name."""
    completed = run_c10r(c10r_binary, ["manifest"], tmp_path_factory.mktemp("manifest"))
    assert completed.returncode == 0, completed.stderr.decode(errors="replace")
    manifest = json.loads(completed.stdout)
    return {command["name"]: command for command in manifest["commands"]}


@pytest.fixture(scope="module")
def advertised_tools(c10r_binary: str, tmp_path_factory: pytest.TempPathFactory) -> dict[str, Any]:
    """Every tool's input schema, keyed by tool name, as a client would see it."""
    server: FastMCP = create_server(c10r_binary, tmp_path_factory.mktemp("schemas"))

    async def listed() -> dict[str, Any]:
        async with Client(server) as connected:
            return {tool.name: tool.input_schema for tool in await connected.list_tools()}

    return asyncio.run(listed())


def options_of(command: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Every positional argument and flag the surface index reports for a command."""
    return {entry["name"]: entry for entry in [*command["args"], *command["flags"]]}


def mirrored_option(parameter: str) -> str:
    """The command option a tool parameter mirrors."""
    return MIRRORED_OPTION.get(parameter, parameter)


def enum_values(schema: dict[str, Any]) -> list[str]:
    """The enumerated values a parameter accepts, across the optional-union shape."""
    if "enum" in schema:
        return list(schema["enum"])
    for arm in schema.get("anyOf", []):
        if "enum" in arm:
            return list(arm["enum"])
    return []


def parameters_of(schema: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """The mirrored parameters of a tool, excluding the ones this server owns."""
    return {name: value for name, value in schema.get("properties", {}).items() if name not in WRAPPER_PARAMETERS}


@pytest.mark.parametrize("tool", sorted(MIRRORED_COMMAND))
def test_every_parameter_names_a_real_option(
    tool: str, advertised_tools: dict[str, Any], installed_surface: dict[str, Any]
) -> None:
    """No tool offers a parameter the installed binary has no option for."""
    command = MIRRORED_COMMAND[tool]
    assert command in installed_surface, f"`{command}` is not a command of the installed binary"
    reported = options_of(installed_surface[command])

    for parameter in parameters_of(advertised_tools[tool]):
        assert mirrored_option(parameter) in reported, (
            f"tool `{tool}` offers `{parameter}`, which `c10r {command}` does not accept; "
            f"it accepts {sorted(reported)}"
        )


@pytest.mark.parametrize("tool", sorted(MIRRORED_COMMAND))
def test_every_accepted_value_is_in_the_reported_set(
    tool: str, advertised_tools: dict[str, Any], installed_surface: dict[str, Any]
) -> None:
    """No enumerated parameter accepts a value the installed binary would reject."""
    command = MIRRORED_COMMAND[tool]
    reported = options_of(installed_surface[command])

    for parameter, schema in parameters_of(advertised_tools[tool]).items():
        accepted = enum_values(schema)
        if not accepted:
            continue
        valid = reported[mirrored_option(parameter)].get("values", [])
        assert valid, (
            f"tool `{tool}` types `{parameter}` as an enumeration, but `c10r {command}` reports no valid set for it"
        )
        assert set(accepted) <= set(valid), (
            f"tool `{tool}` accepts {sorted(set(accepted) - set(valid))} for "
            f"`{parameter}`, which `c10r {command}` does not"
        )


@pytest.mark.parametrize("tool", sorted(MIRRORED_COMMAND))
def test_every_stated_default_matches_the_reported_default(
    tool: str, advertised_tools: dict[str, Any], installed_surface: dict[str, Any]
) -> None:
    """A default a description states is the default the installed binary applies."""
    command = MIRRORED_COMMAND[tool]
    reported = options_of(installed_surface[command])

    for parameter, schema in parameters_of(advertised_tools[tool]).items():
        stated = STATED_DEFAULT.search(schema.get("description", ""))
        if stated is None:
            continue
        option = reported[mirrored_option(parameter)]
        assert stated.group(1) == option.get("default"), (
            f"tool `{tool}` states `{parameter}` defaults to {stated.group(1)!r}, but "
            f"`c10r {command}` reports {option.get('default')!r}"
        )


REQUIRED_CONTROLS: dict[str, set[str]] = {
    "get": {"reference", "at", "detail", "max_lines", "from_line", "limit", "cursor"},
    "trace": {"reference", "relation", "depth", "detail", "order", "max_lines", "limit", "cursor"},
    "find": {"fragment", "limit", "cursor"},
    "search": {"query", "detail", "max_lines", "limit", "cursor"},
    "similar": {"reference", "at", "detail", "max_lines", "limit", "cursor"},
    "impact": {"revspec", "staged", "depth", "order", "paths", "limit", "cursor"},
}
"""The bounding and projection controls each query tool must keep advertising.

The other direction of the conformance check. Comparing advertised parameters
against the surface index proves nothing about a parameter that has been
*deleted*: it simply stops being iterated over. A caller governs result size
through these, so dropping one silently moves that job back onto post-processing
the answer — which is the thing exposing them exists to avoid.
"""


@pytest.mark.parametrize("tool", sorted(REQUIRED_CONTROLS))
def test_every_required_control_is_still_advertised(tool: str, advertised_tools: dict[str, Any]) -> None:
    """No bounding or projection control has quietly stopped being exposed."""
    advertised = set(parameters_of(advertised_tools[tool]))
    missing = REQUIRED_CONTROLS[tool] - advertised

    assert not missing, f"tool `{tool}` no longer advertises {sorted(missing)}"


@pytest.mark.parametrize("tool", sorted(REQUIRED_CONTROLS))
def test_every_required_control_is_a_real_option(tool: str, installed_surface: dict[str, Any]) -> None:
    """The required set is itself checked against the binary, so it cannot rot.

    Without this, a control renamed in the CLI could be listed as required here
    forever, and the check above would keep passing against a name that no longer
    exists.
    """
    reported = options_of(installed_surface[MIRRORED_COMMAND[tool]])
    unknown = {control for control in REQUIRED_CONTROLS[tool] if mirrored_option(control) not in reported}

    assert not unknown, f"`c10r {MIRRORED_COMMAND[tool]}` has no option for {sorted(unknown)}"


def test_at_least_one_default_is_actually_stated(advertised_tools: dict[str, Any]) -> None:
    """The default check would pass vacuously if no description stated one."""
    stated = [
        (tool, parameter)
        for tool, schema in advertised_tools.items()
        for parameter, value in parameters_of(schema).items()
        if STATED_DEFAULT.search(value.get("description", ""))
    ]

    assert len(stated) >= 10, stated


def test_the_conformance_check_requires_a_binary(c10r_binary: str) -> None:
    """The binary fixture fails rather than skipping, so this suite cannot pass empty."""
    assert Path(c10r_binary).is_file()
