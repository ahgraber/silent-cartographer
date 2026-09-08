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

## The index store

Every store is stamped at creation, and every command checks that stamp before touching the file.
A file that is not `c10r`'s own is left alone, and the refusal names both recoveries: rebuild if it is a stale `c10r` index, or fix `--db` if the file belongs to something else.

Queries never create a store.
A store built under an older schema version is `c10r`'s to replace: [`build`](build.md) rebuilds it in place, and a query refuses and tells you to rebuild.

Each store records the workspace root it was built from.
An answer read from a store describing a different workspace carries a `workspace_relation` marker naming that root, separate from the staleness flag.
