# c10r status

Report the index's provenance, freshness, and join-alignment counts.

```sh
c10r status
c10r status --discrepancies
c10r status --duplicates
```

## Options

| Flag              | Default | Meaning                                                                                                                                              |
| ----------------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--discrepancies` | off     | Add the join-discrepancy detail: a bounded summary of the non-aligned occurrences behind the counts, grouped by outcome kind and expected name token |
| `--all`           | off     | With `--discrepancies`, return every persisted row instead of the bounded summary                                                                    |
| `--duplicates`    | off     | Add the per-group detail for duplicated descriptors: each group's shared descriptor and the definitions sharing it                                   |

The duplicated-descriptor group count is always in the summary; `--duplicates` adds the groups themselves.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, and `--color`.

## What it reports

Provenance and freshness: which analyzer produced the index, and how far behind the workspace it is.

Join-alignment counts: how much of the syntactic structure was matched to resolved symbols during the last build.

The chunk parameters the build ran with.

## See also

- [`build`](build.md) — refresh the index these counts describe
- [`doctor`](doctor.md) — whether the indexers themselves are healthy
