# Common to every command

Flags and behavior shared by the whole command surface.

## Shared flags

| Flag                            | Default                             | Meaning                                                    |
| ------------------------------- | ----------------------------------- | ---------------------------------------------------------- |
| `--db <PATH>`                   | `.c10r/index.db`                    | Path to the SQLite index                                   |
| `--workspace <NAME>`            | the workspace root's directory name | The workspace identity that symbols are namespaced under   |
| `--json`                        | off                                 | Print the structured answer instead of the human rendering |
| `--color <auto\|always\|never>` | `auto`                              | `auto` styles only when standard output is a terminal      |

Result-bearing commands (`get`, `trace`, `find`, `search`, `similar`, `impact`) also take:

| Flag               | Default | Meaning                                                       |
| ------------------ | ------- | ------------------------------------------------------------- |
| `--limit <N>`      | `25`    | Cap on rows returned; `0` means unbounded, with no page block |
| `--cursor <TOKEN>` | none    | Resume a truncated result set from its continuation token     |

## Naming a symbol

`get`, `trace`, and `similar` take a symbol reference in any of three forms:

- a short name — `connect`
- a qualified name — `Client::connect`
- the full canonical identity, which every answer carries

An ambiguous reference returns the candidate set to choose from.

`get` and `similar` also accept `--at path:byte_offset` instead of a reference, which resolves to the symbol enclosing that position.

## Content detail

Commands that can return source content share one `--detail` axis:

| Level       | Returns                                           |
| ----------- | ------------------------------------------------- |
| `location`  | The definition file and position                  |
| `signature` | The signature, without the body                   |
| `interface` | The signature plus the symbol's own documentation |
| `body`      | The full source body                              |

`--detail` never changes which rows are returned or their order — only what each row shows.
Defaults differ per command; see each page.

`--max-lines` caps the lines of content returned (`0` means unbounded) and applies only to a content-bearing detail.
Its default also differs per command.

## Reading an answer

Provenance, freshness, typed absence, and heuristic labels work the same way for every command; see [reading an answer](../../README.md#reading-an-answer).

## The `refresh` field

Every stale answer carries a `refresh` field: a runnable command that rebuilds the index the answer came from.
A fresh answer carries no `refresh` field at all, rather than an empty one.

In the human rendering, the command appears on its own `refresh:` line directly under the header line that marks the answer stale:

```text
scip-python 0.6.6 · stale (content changed)
refresh: c10r build '/path/to/project' --db='/path/to/project/.c10r/index.db' --workspace='project' --language=python
```

Under `--json`, the same text is the `refresh` field:

```json
{
  "freshness": "stale_content",
  "stale": true,
  "refresh": "c10r build '/path/to/project' --db='/path/to/project/.c10r/index.db' --workspace='project' --language=python"
}
```

The command is `c10r build`, given the workspace root, `--db`, `--workspace`, and `--language` recorded for the index.
`--db` is absolute, so the command runs from any directory.
`--language` names the recorded backend, so a workspace with both a `Cargo.toml` and a `pyproject.toml` rebuilds the same backend.
It adds `--environment` when the query resolved one for the interpreter environment in effect, and `--chunk-size`/`--chunk-overlap` when the index was built with values other than the defaults, so a rebuild does not change the semantic-index identity.
Running it makes the same query answer fresh again, as long as the sources, analyzer, and environment stay as they were when the command was built.

When the index does not record a workspace root, the field cannot name one to build from.
It is then a shell comment instead of a runnable command, so pasting it into a shell runs nothing:

```text
# workspace root not recorded; run from the workspace root: c10r build . --db='/path/to/project/.c10r/index.db' --workspace='project' --language=python
```

## The index store

Every store is stamped at creation, and every command checks that stamp before touching the file.
A file that is not `c10r`'s own is left alone, and the refusal names both recoveries: rebuild if it is a stale `c10r` index, or fix `--db` if the file belongs to something else.

Queries never create a store.
A store built under an older schema version is `c10r`'s to replace: [`build`](build.md) rebuilds it in place, and a query refuses and tells you to rebuild.

Each store records the workspace root it was built from.
An answer read from a store describing a different workspace carries a `workspace_relation` marker naming that root, separate from the staleness flag.
