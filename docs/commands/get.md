# c10r get

Retrieve one symbol at a chosen detail level, by name or by source position.

```sh
c10r get with_backoff
c10r get with_backoff --detail signature
c10r get with_backoff --detail body
c10r get --at src/lib.rs:42:9               # the symbol enclosing line 42, column 9
c10r get --at src/lib.rs:42                 # the declaration whose header is on line 42
```

## Arguments

| Argument    | Meaning                                       |
| ----------- | --------------------------------------------- |
| `REFERENCE` | The symbol reference. Omit when using `--at`. |

## Options

| Flag                                              | Default    | Meaning                                                                                 |
| ------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------- |
| `--detail <location\|signature\|interface\|body>` | `location` | How much of the symbol to return                                                        |
| `--at <path:line[:column]>`                       | none       | Return the symbol at this position. Cannot be combined with `REFERENCE`.                |
| `--max-lines <N>`                                 | `100`      | Cap the lines of content returned; `0` means unbounded                                  |
| `--from <LINE>`                                   | `1`        | 1-based line where the returned window starts; the window is `[from, from + max-lines)` |

`--max-lines` and `--from` apply only when `--detail` selects content (`signature`, `interface`, `body`).
The window's lines are numbered within the symbol's own text, not the file, and the disclosure line says so (`lines 1-20 of 40 in this symbol`).

`--at` reads lines and columns by default and byte offsets under `--position byte`; see [positions](common.md#positions) for the forms and the line rule.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--position`, `--limit`, and `--cursor`, and [naming a symbol](common.md#naming-a-symbol) for reference forms.

## See also

- [`find`](find.md) — locate a symbol when you only know part of its name
- [`search`](search.md) — locate code when you don't know the name at all
- [`trace`](trace.md) — what stands in a relation to this symbol
