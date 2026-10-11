# c10r status

Report the index's provenance, freshness, and join-alignment counts.

```sh
c10r status
c10r status --discrepancies
c10r status --duplicates
```

## Options

| Flag               | Default | Meaning                                                                                                                                              |
| ------------------ | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `--discrepancies`  | off     | Add the join-discrepancy detail: a bounded summary of the non-aligned occurrences behind the counts, grouped by outcome kind and expected name token |
| `--all`            | off     | With `--discrepancies`, return every persisted row instead of the bounded summary                                                                    |
| `--duplicates`     | off     | Add the per-group detail for duplicated descriptors: each group's shared descriptor and the definitions sharing it                                   |
| `--detail[=files]` | off     | Name the source files that differ from the index, one per row. A bare `--detail` means `files`.                                                      |
| `--limit <N>`      | `25`    | With `--detail`, cap on files returned per page; `0` means unbounded, with no page block                                                             |
| `--cursor <TOKEN>` | none    | With `--detail`, resume a truncated file list from its continuation token                                                                            |

The duplicated-descriptor group count is always in the summary; `--duplicates` adds the groups themselves.

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, and `--position`.

## What it reports

Provenance and freshness: which analyzer produced the index, and how far behind the workspace it is.

Join-alignment counts: how much of the syntactic structure was matched to resolved symbols during the last build.

The chunk parameters the build ran with.

## Why an index is stale

`stale_reasons` lists every reason the index is behind, in the order `stale_content`, `stale_version`, `stale_environment`: the sources changed, the analyzer's version changed, or the interpreter environment changed.
For a Python index, the analyzer is the recorded indexer, and `status` compares the recorded version with the installed version of that indexer.
It is empty for a fresh index.
`freshness` carries the first reason.

When the sources changed, `changed_files` counts the source files that differ from what the build indexed:

```json
{
  "changed_files": {
    "changed": 2,
    "added": 1,
    "removed": 1
  }
}
```

A file counts as `changed` when its text differs from the text indexed, `added` when discovery finds it and the index does not hold it, and `removed` when the index holds it and discovery no longer finds it.
Files that are not source files, such as a README, are never counted, and editing one leaves the index fresh.

`--detail files` adds `files`, the differing files sorted by path as `{"document_path", "change"}` objects, paged by `--limit` and `--cursor`.
A continuation token is refused once the index or the files on disk have changed since it was issued.

## Output

Without `--json`, `status` prints one labeled line per fact, projected from the same report `--json` emits.

```text
workspace: my-repo
analyzer: rust-analyzer 1.85.0
freshness: stale_content
refresh: c10r build '/work/my-repo' --db='/work/my-repo/.c10r/index.db' --workspace='my-repo' --language=rust
aligned: exact=120 total=120
text_mismatch: 0
semantic_only: 3
duplicate_ambiguous: 0
syntax_only: 1
duplicated descriptor groups: 0
semantic index: <model> corpus definition 2 chunk size 512 overlap 64
```

With `--detail`, the human output also names each file on a `changed:`, `added:`, or `removed:` line, and discloses truncation with the cursor that resumes the list.
For an index with two edited files, one new file, and one deleted file, `status --detail --limit 3` prints:

```text
workspace: demo
analyzer: rust-analyzer 1.85.0
freshness: stale_content
stale reasons: stale_content
refresh: c10r build '/work/demo' --db='/work/demo/.c10r/index.db' --workspace='demo' --language=rust
changed files: 2 changed, 1 added, 1 removed
changed: src/a.rs
changed: src/b.rs
removed: src/d.rs
page 0: 3 of 4 files (truncated)
  resume with --cursor <TOKEN>
aligned: crate_root=0 exact=0 ...
```

The `aligned:` line here is shortened, and the remaining lines of the report follow it.
`<TOKEN>` stands for the opaque continuation token the command prints.

When the index is stale, `--json` carries the same command in a `refresh` field, and the human output shows it on a `refresh:` line.
A fresh index has neither.
Running the command brings the index back to fresh.

## See also

- [`build`](build.md) — refresh the index these counts describe
- [`doctor`](doctor.md) — whether the indexers themselves are healthy
