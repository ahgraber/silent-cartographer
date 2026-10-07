# Common to every command

Flags and behavior shared by the whole command surface.

## Shared flags

| Flag                            | Default                             | Meaning                                                                  |
| ------------------------------- | ----------------------------------- | ------------------------------------------------------------------------ |
| `--db <PATH>`                   | `.c10r/index.db`                    | Path to the SQLite index                                                 |
| `--workspace <NAME>`            | the workspace root's directory name | The workspace identity that symbols are namespaced under                 |
| `--json`                        | off                                 | Print the structured answer instead of the human rendering               |
| `--color <auto\|always\|never>` | `auto`                              | `auto` styles only when standard output is a terminal                    |
| `--position <line\|byte>`       | `line`                              | The unit of printed locations and of `--at`; see [positions](#positions) |

Result-bearing commands (`get`, `trace`, `find`, `search`, `similar`, `clones`, `impact`) also take:

| Flag               | Default | Meaning                                                       |
| ------------------ | ------- | ------------------------------------------------------------- |
| `--limit <N>`      | `25`    | Cap on rows returned; `0` means unbounded, with no page block |
| `--cursor <TOKEN>` | none    | Resume a truncated result set from its continuation token     |

## Naming a symbol

`get`, `trace`, `similar`, and `clones` take a symbol reference in any of three forms:

- a short name — `connect`
- a qualified name — `Client::connect`
- the full canonical identity, which every answer carries

An ambiguous reference returns the candidate set to choose from.

`get`, `similar`, and `clones` also accept `--at` instead of a reference, which resolves to a symbol by source position; see [positions](#positions).

## Positions

Every location in an answer is a file path plus a position in that file's text.
`--position` selects the unit the human rendering prints and the unit `--at` reads.
It does not change the `--json` answer, which carries both units.

Under the default `--position line`, a location prints as `path:line:column`, the form editors and terminals open as a link:

```text
src/client.rs:12:5
```

Lines are numbered from 1 and end at LF, CRLF, or a lone CR, so the numbers match what an editor shows.
Columns are numbered from 1 and count Unicode characters from the start of the line.
A location's end is exclusive: it is the position after the last character.
The human rendering prints only the start; `--json` carries the end.

Under `--position byte`, a location prints as its byte span, `path:start-end`, and `--at` reads a byte offset:

```text
src/client.rs:240-318
```

Every location object in a `--json` answer carries both units: `span_start` and `span_end` are byte offsets, and `start_line`, `start_column`, `end_line`, and `end_column` are the line and column of the same two positions.
Where a location has no byte offsets, the four line and column fields are `null` too.
Lines and columns describe the text that was indexed, so in a stale answer they can differ from the file as it is now.

### Reading `--at`

Under `--position line`:

| Form               | Resolves to                                                                                                                                                                |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `path:line:column` | The innermost symbol whose span contains that position                                                                                                                     |
| `path:line`        | The declaration whose header is on that line, else the innermost symbol enclosing the start of the line; several sibling declarations on the line return the candidate set |

A header runs from the start of a declaration to where its body begins, so it covers decorators and every line of a multi-line parameter list.
A line inside a function body has no header, and resolves to the function.
A column past the end of its line resolves as the end of that line does.
A zero line or column is a usage error that names the accepted forms.

Under `--position byte`, `--at path:byte_offset` resolves to the innermost symbol enclosing that offset.

The position is split from the right, so a path that contains `:` stays usable.
A path whose own name ends in `:<digits>` is the exception: under `--position line`, `dir/a:12:5` reads as `dir/a`, line 12, column 5.

A continuation token from a paged `--at` query is bound to the unit it was issued under and is refused under the other.

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
