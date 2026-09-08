# c10r search

Find code by meaning: describe what the code does, and get the nearest candidate symbols.

```sh
c10r search "retry a request with exponential backoff"
c10r search "parse a config file into typed settings" --detail body
```

## Arguments

| Argument | Meaning                                                         |
| -------- | --------------------------------------------------------------- |
| `QUERY`  | A natural-language description of what the code does. Required. |

## Options

| Flag                                              | Default     | Meaning                                               |
| ------------------------------------------------- | ----------- | ----------------------------------------------------- |
| `--detail <location\|signature\|interface\|body>` | `signature` | Project content onto each row                         |
| `--max-lines <N>`                                 | `10`        | Cap the lines of content per row; `0` means unbounded |

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--limit`, and `--cursor`.

## What counts as evidence

A symbol's name, its documentation, and its source all count.
Names are split into words, so `withBackoff` matches "with backoff".

Shared words are not required: the ranking combines a semantic-embedding signal with a lexical one, so paraphrases and exact identifiers both work.

## Reading the result

Every answer is labeled `"classification": "estimation"` and carries the semantic index's identity as provenance.

Rows are the nearest candidates the index holds, most relevant first — not the complete set of relevant code.
An empty answer is not proof that none exists.
No relevance score is published; the order is the ranking.

## See also

- [`similar`](similar.md) — the same ranking, with a symbol as the query instead of a description
- [`find`](find.md) — match on the name instead of the meaning
