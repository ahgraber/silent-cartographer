# c10r build

Build or refresh the index for a workspace.

```sh
c10r build              # index the current directory
c10r build ../other     # index another workspace root
c10r build --force      # build regardless of the currency check
```

## Arguments

| Argument | Default | Meaning                     |
| -------- | ------- | --------------------------- |
| `ROOT`   | `.`     | The workspace root to index |

## Options

| Flag                        | Default                             | Meaning                                                                             |
| --------------------------- | ----------------------------------- | ----------------------------------------------------------------------------------- |
| `--language <rust\|python>` | from the project manifest           | Which backend to build with                                                         |
| `--rust-analyzer <PATH>`    | `rust-analyzer`                     | Path to the `rust-analyzer` executable                                              |
| `--environment <PATH>`      | `$VIRTUAL_ENV`, then `.venv`/`venv` | An explicit interpreter environment; meaningful for the Python backend              |
| `--force`                   | off                                 | Build even when the stored index already describes the workspace                    |
| `--chunk-size <TOKENS>`     | `512`                               | Bound on the whole text embedded per chunk, the passage header included             |
| `--chunk-overlap <TOKENS>`  | `64`                                | Overlap between adjacent chunks of one passage; must be smaller than the chunk size |

With no `--language`, the workspace's manifest decides: `Cargo.toml` selects rust, `pyproject.toml` selects python.
If both are present, `--language` is required.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, and `--color`.

## Rebuilding is lazy

`build` compares the stored index against the workspace's sources, analyzer, declared environment, and chunk parameters.
If all match, it skips the code analysis, leaves the store untouched, and reports the index already current.
A commit that changed no indexed source returns in a fraction of a second.

Under `--json`, the answer carries `"rebuilt"`: `true` if the workspace was analyzed, `false` if the index was already current.

Anything the check cannot confirm causes a build rather than a skip — an absent index, an older schema version, a store recorded for a different workspace, or unreadable metadata.
Use `--force` when you distrust the check itself.

## Chunk parameters

The values a build ran with are stored with the index and reported by [`status`](status.md), and the currency check compares them.
Changing either re-derives every vector on the next build.

## See also

- [`hooks`](hooks.md) — rebuild automatically after every commit
- [`doctor`](doctor.md) — check that the language indexers are installed before building
