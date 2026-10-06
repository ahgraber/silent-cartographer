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

## Files that change during a build

Analysis can take long enough for the workspace to change under it.
After writing the store, `build` discovers the sources a second time and compares them with the files it indexed.
A file edited, created, or deleted in between is reported, and the build still exits with success: the store describes the text that was indexed, and [`status`](status.md) reports it stale for those files.

Under `--json`, the answer always carries `"changed_during_build"`, a list of `{"document_path", "change"}` objects sorted by path, where `change` is `changed`, `added`, or `removed`.
The list is empty when nothing moved, and when the index was already current and nothing was analyzed.

```json
{
  "changed_during_build": [
    {
      "change": "changed",
      "document_path": "src/b.rs"
    },
    {
      "change": "added",
      "document_path": "src/f.rs"
    }
  ]
}
```

The human answer prints one line per file after the accounting line:

```text
built: aligned=0 (...) text_mismatch=0 semantic_only=0 duplicate_ambiguous=0 syntax_only=0
changed during build: src/b.rs
added during build: src/f.rs
```

The list is not paged.
An edit that is reverted before the second pass is not reported, and the index then matches the files again.

## Chunk parameters

The values a build ran with are stored with the index and reported by [`status`](status.md), and the currency check compares them.
Changing either re-derives every vector on the next build.

## See also

- [`hooks`](hooks.md) — refresh the index automatically when git changes the checked-out commit
- [`doctor`](doctor.md) — check that the language indexers are installed before building
