# c10r-mcp

An MCP server exposing the `c10r` code-graph CLI as tools, over stdio.

It runs the `c10r` binary you installed as a child process and returns its structured answer unmodified.
It bundles no binary and changes nothing about the CLI.

See the [repository README](../README.md#mcp-server) for installation, client configuration, and the consent model.

## Layout

| Module         | Responsibility                                                             |
| -------------- | -------------------------------------------------------------------------- |
| `binary.py`    | Locating `c10r`, and running it in a resolved workspace                    |
| `errors.py`    | Turning an exit code and a diagnostic into a typed tool result             |
| `workspace.py` | Resolving which workspace one call answers about                           |
| `surface.py`   | The startup gate holding the tools to the installed command surface        |
| `params.py`    | The closed value sets the tool schemas carry                               |
| `confirm.py`   | Obtaining confirmation through the negotiated protocol era's own mechanism |
| `server.py`    | The tools themselves, and the server instructions                          |
| `__main__.py`  | The console entry point: check, then serve                                 |

## Development

From this directory:

```sh
uv sync
uv run pytest
```

From anywhere else, pass the directory rather than the project, so pytest starts here and reads the configuration in `pyproject.toml`:

```sh
uv run --directory mcp pytest
```

The tests need a real `c10r` binary.
They look at `C10R_BINARY`, then `target/release/c10r` and `target/debug/c10r` in the repository, then `PATH`, and fail — never skip — when none is found.

`SURFACE_VERSION` in `surface.py` records the command surface the tools were written against.
When `c10r`'s own surface version moves, re-check the affected tools against the new surface and raise it in the same change; the conformance tests in `tests/test_conformance.py` check every parameter, accepted value, and stated default against `c10r manifest`.
