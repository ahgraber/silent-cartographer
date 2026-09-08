# c10r find

Return the indexed symbols whose name contains the queried fragment.

```sh
c10r find retry
c10r find Client --limit 50
```

## Arguments

| Argument   | Meaning                               |
| ---------- | ------------------------------------- |
| `FRAGMENT` | The name fragment to match. Required. |

## Matching

Matching is case-insensitive, and case folding is ASCII-only: an ASCII letter matches regardless of case, while a non-ASCII character matches only exactly.

## Options

`find` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`, `--limit`, `--cursor`.

## See also

- [`search`](search.md) — find code by what it does, when you don't know the name
- [`get`](get.md) — retrieve one symbol once you have its name
