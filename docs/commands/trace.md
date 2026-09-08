# c10r trace

Return the symbols standing in a named relation to a subject symbol.

```sh
c10r trace with_backoff --relation references
c10r trace with_backoff --relation dependents --depth 2
c10r trace with_backoff --relation tests
c10r trace Client --relation contains --detail signature
```

## Arguments

| Argument    | Meaning                       |
| ----------- | ----------------------------- |
| `REFERENCE` | The subject symbol. Required. |

## Relations

`--relation` is required and takes one of:

| Relation       | Returns                                                                                            |
| -------------- | -------------------------------------------------------------------------------------------------- |
| `containers`   | The declaration that directly encloses the subject                                                 |
| `contains`     | The symbols the subject directly contains                                                          |
| `references`   | The sites that reference the subject                                                               |
| `dependents`   | Everything that depends on the subject, directly and transitively to `--depth`                     |
| `importers`    | The modules that import the subject                                                                |
| `implementers` | The types declaring the subject as a supertype — a trait's implementors, or a base type's subtypes |
| `tests`        | The reference sites whose enclosing declaration is test code                                       |

## Options

| Flag                                              | Default  | Meaning                                                                                                                       |
| ------------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `--relation <RELATION>`                           | required | See above                                                                                                                     |
| `--depth <N>`                                     | `1`      | `dependents` only: hops of transitive impact to detail. `0` is aggregate-only — no rows, everything counted in the aggregate. |
| `--order <ranked\|unranked>`                      | `ranked` | `dependents` only: row order within each distance layer                                                                       |
| `--detail <location\|signature\|interface\|body>` | none     | Project content onto each row. Omitted, rows carry identity and location only.                                                |
| `--max-lines <N>`                                 | `10`     | Cap the lines of content per row; `0` means unbounded                                                                         |

Passing `--depth` or `--order` with any relation other than `dependents` is an error.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--limit`, and `--cursor`.

## dependents

What could break if this symbol changes.
The relation is reference-grade, so any mention counts, not only a call: a type usage or a constant read is a dependent.

Rows are ordered by distance, then within each distance layer by how load-bearing the dependent is across the codebase (PageRank over the dependency edges).
Ranking never changes which symbols the answer contains, and no score is published.
`--order unranked` orders only by the answer's stable structural keys: distance, then dependency kind, then canonical identity.

## tests

The answer is the subject's reference sites filtered to those inside test code, so it includes shared test helpers as well as test cases.

Test code is recognized by language convention:

- Rust — `#[test]` attributes, `#[cfg(test)]` modules, `tests/` directories
- Python — `test_*.py`, `*_test.py`, `tests.py`, `conftest.py`, `tests/` directories

Every site names the rule that classified it, and the answer is labeled `"classification": "convention"`.
An empty answer means no site matched a convention, not that nothing tests the symbol.

## See also

- [`impact`](impact.md) — the same dependents question, seeded from a git diff instead of a named symbol
