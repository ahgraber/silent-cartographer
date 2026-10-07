# c10r clones

List the deterministic copies in the index: groups of symbols with identical token structure, or the copies of one symbol.

```sh
c10r clones
c10r clones with_backoff
c10r clones --at src/lib.rs:42:9
```

Use it to audit duplication across the whole corpus, or to list every copy of a symbol before changing it.
Unlike [`similar`](similar.md), which estimates, every row here follows from the stored equivalence keys.

## Arguments

| Argument    | Meaning                                                                         |
| ----------- | ------------------------------------------------------------------------------- |
| `REFERENCE` | The subject symbol. Omit to list the clone groups of the corpus, or use `--at`. |

## Options

| Flag                        | Default | Meaning                                                                               |
| --------------------------- | ------- | ------------------------------------------------------------------------------------- |
| `--at <path:line[:column]>` | none    | Take the symbol at this position as the subject. Cannot be combined with `REFERENCE`. |

`clones` projects no content onto its rows; read a row's symbol with [`get`](get.md).
`--at` reads lines and columns by default and byte offsets under `--position byte`; see [positions](common.md#positions) for the forms and the line rule.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--position`, `--limit`, and `--cursor`.

## Token equivalence

Every answer that reaches the clone evidence carries `"grade": "token_equivalence"` and a `guaranteed_fragment_length`, and the human rendering states both.
The grade says the rows have identical token structure, not that the symbols behave identically.
Two symbols are token equivalent when they differ only in whitespace, comments, and documentation (`exact_clone`), or also in identifiers and literal values substituted consistently one for one (`variant_clone`).

`guaranteed_fragment_length` is the number of tokens, taken from the parameters the index was built under, at which a shared run of tokens is guaranteed to be found.
Fragments that this command will report later are at least this long, and a shorter shared run may be missed.
An answer carries no estimation label, no semantic-index identity, and no score.
An ambiguous or unresolved reference ends before the clone evidence is read, so its answer carries neither field.

## Clone groups

With no subject, each row is a group of two or more symbols that share a substitution-insensitive key.
Every symbol that shares such a key with another appears in exactly one group.

| Field         | Meaning                                                                                              |
| ------------- | ---------------------------------------------------------------------------------------------------- |
| `certainty`   | `exact_clone` when every member shares one formatting-insensitive key, `variant_clone` otherwise     |
| `token_count` | The length of the token sequence the members share                                                   |
| `members`     | The symbols with their locations, ordered by canonical identity                                      |
| `exact_set`   | On each member, a number from 1; members with the same number are `exact_clone` copies of each other |

`exact_set` numbers the sets in the order their first member appears, so a `variant_clone` group shows which of its members are exact copies.

Groups are ordered by `token_count` descending, then by member count descending, then by the lowest member's canonical identity, so substantial copies precede the many small groups of accessors and stubs.
`--limit` counts groups, and the answer carries the `total` number of groups.
A corpus in which no two symbols share a key yields a typed-empty answer, which still carries the grade.

## Clones of a subject

With a subject, each row is another symbol that shares the subject's substitution-insensitive key as a whole.
A row carries its `kind` (`whole_symbol`), its `certainty` (`exact_clone` when the symbol also shares the subject's formatting-insensitive key, `variant_clone` otherwise), and the symbol with its location.
Rows are ordered `exact_clone` first, then by canonical identity, and the subject never appears in its own answer.
The same command will also report the token runs the subject shares with other symbols, as rows of kind `fragment`.

| Outcome          | Means                                                                                                       |
| ---------------- | ----------------------------------------------------------------------------------------------------------- |
| `found`          | The subject has clones                                                                                      |
| `empty`          | The subject has comparable content and no clone                                                             |
| `not_comparable` | The subject has no comparable content, as a type that contains methods has none; no comparison was possible |
| `ambiguous`      | The reference names several symbols; the answer lists the candidates                                        |
| `absent`         | Nothing resolved                                                                                            |

`not_comparable` is not `empty`: an empty answer says the comparison ran and found no clone, while `not_comparable` says there was nothing to compare.
Only `clones` produces it.

## Paging

The answer is computed whole and then paged, so a page's `total` is exact.
A continuation token is bound to the query that issued it, with or without a subject, and to the index it was drawn from.
