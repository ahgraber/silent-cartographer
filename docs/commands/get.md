# c10r get

Retrieve one symbol at a chosen detail level, by name or by source position.

```sh
c10r get with_backoff
c10r get with_backoff --detail signature
c10r get with_backoff --detail body
c10r get --at src/lib.rs:1042              # the symbol enclosing that position
```

## Arguments

| Argument    | Meaning                                       |
| ----------- | --------------------------------------------- |
| `REFERENCE` | The symbol reference. Omit when using `--at`. |

## Options

| Flag                                              | Default    | Meaning                                                                                 |
| ------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------- |
| `--detail <location\|signature\|interface\|body>` | `location` | How much of the symbol to return                                                        |
| `--at <path:byte_offset>`                         | none       | Return the symbol enclosing this position. Cannot be combined with `REFERENCE`.         |
| `--max-lines <N>`                                 | `100`      | Cap the lines of content returned; `0` means unbounded                                  |
| `--from <LINE>`                                   | `1`        | 1-based line where the returned window starts; the window is `[from, from + max-lines)` |

`--max-lines` and `--from` apply only when `--detail` selects content (`signature`, `interface`, `body`).

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--limit`, and `--cursor`, and [naming a symbol](common.md#naming-a-symbol) for reference forms.

## See also

- [`find`](find.md) — locate a symbol when you only know part of its name
- [`search`](search.md) — locate code when you don't know the name at all
- [`trace`](trace.md) — what stands in a relation to this symbol
