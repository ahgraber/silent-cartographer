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

| Flag                                              | Default     | Meaning                                                                                                                                        |
| ------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `--at <path:line[:column]>`                       | none        | Take the symbol at this position as the subject. Cannot be combined with `REFERENCE`.                                                          |
| `--subjects-from <PATH\|->`                       | none        | Answer for each reference in a list read from a file or standard input; see below. Cannot be combined with `REFERENCE`, `--at`, or `--cursor`. |
| `--detail <location\|signature\|interface\|body>` | `signature` | Project content onto each row                                                                                                                  |
| `--max-lines <N>`                                 | `10`        | Cap the lines of content per row; `0` means unbounded                                                                                          |
| `--exclude <label>`                               | none        | Omit rows carrying this relation label; repeat to omit several                                                                                 |

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

## Many subjects in one invocation

`--subjects-from <PATH|->` answers for every reference in a list, on one store connection.
Pass a file path, or `-` to read standard input.
It cannot be combined with `REFERENCE`, `--at`, or `--cursor`.

```sh
c10r similar --subjects-from subjects.txt --json
c10r similar --subjects-from - --limit 5 < subjects.txt
```

The list holds one reference per line, in any form `REFERENCE` accepts.
Surrounding whitespace is trimmed and blank lines are skipped.
A list that cannot be read, is not valid UTF-8, or is `-` while standard input is a terminal fails with exit code 2 and a message naming the source.

Each subject gets the answer a single `c10r similar <reference>` returns under the same `--detail`, `--max-lines`, `--exclude`, and `--limit`, including its typed outcome, rows, markers, labels, truncation disclosure, and continuation token.
The JSON answer is an `answers` array in list order, each entry holding the `reference` as written and its `answer`, the object a single `--json` invocation emits.
The human rendering prints each reference as a `#` heading followed by its single-subject rendering.

An ambiguous reference yields its candidate set, and an unresolved reference yields the typed absence.
Neither fails the invocation, which exits 0 whenever the list was read.
An empty list yields `{"answers": []}`, or `empty: no subjects were given` in the human rendering, as a definite none.
A per-subject continuation token resumes a single-subject `c10r similar <reference> --cursor <token>` query under the same controls.
