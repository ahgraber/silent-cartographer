# c10r similar

Rank the indexed symbols most similar in content to a subject symbol.

```sh
c10r similar with_backoff
c10r similar --at src/lib.rs:42:9
c10r similar --at src/lib.rs:42
```

Use it before writing a helper, to check whether one already exists, or around an existing symbol, to assess duplication.

## Arguments

| Argument    | Meaning                                     |
| ----------- | ------------------------------------------- |
| `REFERENCE` | The subject symbol. Omit when using `--at`. |

## Options

| Flag                                              | Default     | Meaning                                                                               |
| ------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------- |
| `--at <path:line[:column]>`                       | none        | Take the symbol at this position as the subject. Cannot be combined with `REFERENCE`. |
| `--detail <location\|signature\|interface\|body>` | `signature` | Project content onto each row                                                         |
| `--max-lines <N>`                                 | `10`        | Cap the lines of content per row; `0` means unbounded                                 |
| `--exclude <label>`                               | none        | Omit rows carrying this relation label; repeat to omit several                        |

`--at` reads lines and columns by default and byte offsets under `--position byte`; see [positions](common.md#positions) for the forms and the line rule.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--position`, `--limit`, and `--cursor`.

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

## Relation labels

Each row carries `relations`, the graph relations that hold between the row's symbol and the subject.
The list is always present and is empty when no label holds.
A label states a stored graph fact only.
It never claims the row duplicates, overrides, or implements the subject, and it never changes which rows appear or their order.

| Label             | Means                                                                                                                                                    |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `uses_subject`    | The row's symbol references the subject                                                                                                                  |
| `used_by_subject` | The subject references the row's symbol                                                                                                                  |
| `tests_subject`   | The row's symbol is classified test code and references the subject; the label carries the convention `rule` that classified it                          |
| `type_family`     | The row's declaring type and the subject's differ, and one declares the other as a supertype or both declare a common supertype defined in the workspace |

A type's declaring type is the type itself, and a member's declaring type is the type that directly encloses it.
Two members of one type never carry `type_family`.
The label compares types only: an inherent method of a type that also implements a workspace trait carries it against that trait's methods.
The human rendering shows each label after the row, with the rule for `tests_subject`.

`--exclude` removes every row that carries any of the given labels, keeping the remaining rows in their order.
`--limit` and `--cursor` then count only the remaining rows, and a cursor is accepted only under the exclusions it was issued with.
The JSON answer names the exclusions in `excluded_relations`, sorted, and omits the field when none is given.
