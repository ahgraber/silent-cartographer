# c10r similar

Rank the indexed symbols most similar in content to a subject symbol.

```sh
c10r similar with_backoff
c10r similar --at src/lib.rs:1042
```

Use it before writing a helper, to check whether one already exists, or around an existing symbol, to assess duplication.

## Arguments

| Argument    | Meaning                                     |
| ----------- | ------------------------------------------- |
| `REFERENCE` | The subject symbol. Omit when using `--at`. |

## Options

| Flag                                              | Default     | Meaning                                                                                      |
| ------------------------------------------------- | ----------- | -------------------------------------------------------------------------------------------- |
| `--at <path:byte_offset>`                         | none        | Take the symbol enclosing this position as the subject. Cannot be combined with `REFERENCE`. |
| `--detail <location\|signature\|interface\|body>` | `signature` | Project content onto each row                                                                |
| `--max-lines <N>`                                 | `10`        | Cap the lines of content per row; `0` means unbounded                                        |

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--limit`, and `--cursor`.

## Clone markers

Every other indexed symbol is ranked by estimated similarity, using the same ranking [`search`](search.md) uses.
Two clone markers can appear on rows, and marked rows rank ahead of the estimate — exact before variant:

| Marker          | Means                                                                                                                                  |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `exact_clone`   | The same token sequence as the subject; the sources differ only in whitespace and comments                                             |
| `variant_clone` | The same token structure, with identifiers and literal values consistently substituted — a renamed copy, or one with changed constants |

The markers come from stored equivalence keys and are stated as fact.
Neither claims the two symbols behave identically.

The surrounding ranking carries the `"classification": "estimation"` label.
