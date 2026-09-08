# c10r manifest

Print the command and flag structure, plus the current index state, as JSON.

```sh
c10r manifest
c10r manifest | jq '.commands[] | select(.name == "trace")'
```

Output is always JSON, whether or not `--json` is given.

## Options

`manifest` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## What it reports

Every command, its arguments and flags, the valid values of each, and the defaults — enough to drive the CLI without scraping help text.
The current index state travels in the same output.

The output carries a surface version, so a cached copy can be checked and invalidated.
The MCP server refuses to start when the binary's surface version is not the one it was built against.
