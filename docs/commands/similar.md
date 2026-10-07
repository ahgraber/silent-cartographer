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

| Flag                                              | Default     | Meaning                                                                                                                                                    |
| ------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--at <path:line[:column]>`                       | none        | Take the symbol at this position as the subject. Cannot be combined with `REFERENCE`.                                                                      |
| `--subjects-from <PATH\|->`                       | none        | Answer for each reference in a list read from a file or standard input; see below. Cannot be combined with `REFERENCE`, `--at`, or `--cursor`.             |
| `--mutual`                                        | off         | List the pairs of symbols that are each the first row of the other's answer; see below. Cannot be combined with `REFERENCE`, `--at`, or `--subjects-from`. |
| `--detail <location\|signature\|interface\|body>` | `signature` | Project content onto each row                                                                                                                              |
| `--max-lines <N>`                                 | `10`        | Cap the lines of content per row; `0` means unbounded                                                                                                      |
| `--exclude <label>`                               | none        | Omit rows carrying this relation label; repeat to omit several                                                                                             |

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

## Mutual nearest pairs

`--mutual` lists every pair of indexed symbols in which each symbol is the first row of the other's answer, under the same `--exclude`.
A first row is the first row a single `c10r similar <reference>` returns, so excluding a label can change which pairs exist: a test and the subject it calls stop being a pair under `--exclude tests_subject`.

```sh
c10r similar --mutual --limit 20
c10r similar --mutual --limit 0 --exclude tests_subject --json
```

Each pair appears once, and pairs are ordered by the canonical identity of the lower member.
A pair row holds both members under `lower` and `upper`, each with its symbol, location, and content at `--detail`, and the clone marker that holds between them.
A member's `relations` are the labels it carries as the other member's first row.
A pair carries no score, and the answer carries the `"classification": "estimation"` label like every `similar` answer.
A corpus with no pair yields a typed-empty answer.

`--limit` counts pairs.
The walk visits symbols in identity order and stops after one pair more than the limit, so a page costs only the symbols it reaches and the answer cannot state a total.
Its `page` block omits `total`, and the human rendering reads `page 0: 10 results (truncated)`.
The continuation token records the last visited symbol and is bound to the exclusions, `--detail`, `--limit`, `--max-lines`, and the index it was issued under.
`--cursor` resumes after that symbol and repeats no pair.
`--limit 0` walks the whole corpus once and returns every pair.

Each symbol the walk touches costs one single-subject ranking, reused when the walk meets it again, and a resumed walk repeats the rankings of the partners it reaches.
A full listing therefore costs at most one ranking per corpus symbol, and each ranking grows with the corpus, so the listing grows quadratically.
On the ripgrep store (3,354 symbols) a full listing took 98 seconds and returned 817 pairs, and `--exclude tests_subject` took 73 seconds and returned 811.
On the httpx2 store (2,240 symbols) it took 32 seconds and returned 688 pairs, and 684 under `--exclude tests_subject`.
A page of 10 pairs took 1.2 seconds on ripgrep and 0.7 seconds on httpx2.
Page through the answer, or stop early, rather than listing a large corpus in one run.
