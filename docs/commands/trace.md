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

## references

The answer's results are the sites the index resolved to the subject.
Two groups sit beside the results.
Neither group is counted in the results, in `page.returned`, or in `page.total`.

An indexer records no occurrence at a call it cannot resolve, so such a call is missing from the results.
The `unresolved_calls` group lists those calls whose called name equals the last component of the subject's name.
Matching is by name only, so each listed call may or may not call the subject.
Calls in a source the analyzer left out of its index are all listed, because the analyzer resolved none of them.

| Field       | Meaning                                            |
| ----------- | -------------------------------------------------- |
| `name`      | The called name matched                            |
| `total`     | How many matching calls exist                      |
| `truncated` | Whether `calls` lists fewer than `total`           |
| `calls`     | The calls, ordered by document path, then position |

Each call carries its `location`, the `receiver` text when the call has one, `receiver_shortened`, and `enclosing`, the declaration or module the call sits in.
The receiver is the text before the called name, such as `app` in `app.test_client()`.
It is stored as written, up to 200 characters.
A longer receiver is cut at 200 characters and `receiver_shortened` is true.
The human rendering joins a multi-line receiver onto one line and ends a cut receiver with `…`.

The `unconfirmed_uses` group lists the uses the indexer reported for the subject that the build refused to count.
A use is refused when the source text at its position is not the symbol's name, when the indexer reported it where no syntax node exists, or when it could mean any of several definitions that share one identity.

| Field       | Meaning                                           |
| ----------- | ------------------------------------------------- |
| `total`     | How many such uses exist                          |
| `truncated` | Whether `uses` lists fewer than `total`           |
| `uses`      | The uses, ordered by document path, then position |

Each use carries its `document_path`, its `location` when the position is known, and a `reason`: `duplicate_ambiguous`, `text_mismatch`, or `semantic_only`.
A `duplicate_ambiguous` use carries `candidates`, `candidates_total`, and `candidates_shortened`.
It lists at most 10 candidates.
Any other use carries `found_text`, the source text found at the position, and `found_text_shortened`.
Found text is stored up to 200 characters, the same bound as a receiver.
A longer found text is cut at 200 characters and `found_text_shortened` is true.
A use with no known position omits `location`, and the human rendering reads `(position unknown)`.

Both groups are capped at `--limit`.
A capped group sets `truncated`, carries no continuation token, and is identical on every page.
`--limit 0` lists every entry; the human rendering says so with `list all <N> with --limit 0`.
An answer with nothing to list carries a `total` of `0` and an empty list, so an absent group means the binary does not report it.
An ambiguous or absent subject carries neither group.

Limits of the `unresolved_calls` group:

- Calls inside the arguments of a Rust macro are not listed, because the macro's arguments are not parsed as expressions.
- Rust results depend on the build host.
  Code under a `#[cfg(...)]` condition that is false on the host has no occurrences, so its calls appear in the group, and the group differs between hosts.
- A common name such as `get` lists many unrelated calls.

## Empty answers

An empty answer of `references`, `dependents`, `importers`, or `tests`, and an `impact` answer whose seeds have no dependents, means that no resolved instance was recorded.
It does not mean that none exists.
Calls by frameworks, implicit calls such as constructors and operators, and lookups by string are not seen, and the human rendering says so.
For `tests`, the rendering reads `no convention-classified test reference found` and names the same unseen cases.
The JSON outcome stays `empty`.
For `references`, the rendering also states the totals of both groups.

An empty answer of `containers`, `contains`, or `implementers`, and an `impact` answer whose change touches no indexed declaration, is a definite none.
Each states a fact that the syntax tree establishes.

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
An empty answer means no resolved reference site matched a convention, not that nothing tests the symbol.
Test code that calls the symbol through a call the analyzer could not resolve, such as a call on an untyped fixture parameter, is not counted.

## See also

- [`impact`](impact.md) — the same dependents question, seeded from a git diff instead of a named symbol
