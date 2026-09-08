# c10r cache

Inspect or remove the stored index at the `--db` path.

```sh
c10r cache dir
c10r cache size
c10r cache clear
c10r cache size --db /path/to/index.db
```

## Arguments

| Argument | Meaning                                                    |
| -------- | ---------------------------------------------------------- |
| `ACTION` | `clear`, `dir`, or `size`. Required — see the table below. |

`c10r cache` on its own is a usage error naming the three actions, and it removes nothing.

| Action  | Answers                           |
| ------- | --------------------------------- |
| `clear` | Remove the stored index.          |
| `dir`   | Which directory holds the store?  |
| `size`  | How much disk does the store use? |

## Options

`cache` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## clear

Removes the store, along with the `-wal` and `-shm` sidecar files SQLite leaves beside it.

A file without `c10r`'s ownership stamp is left untouched, and the command exits 6; see [the index store](common.md#the-index-store).
A store recorded under an older schema version is still `c10r`'s, so `clear` removes it.
Removing an index that is not there is a success, not a failure.

If `--db` is a symbolic link, `clear` removes the link and the store it names stays on disk, because unlinking a link never reaches the file behind it.
The command warns on standard error and says so in its answer, and the `--json` report carries `"symlink": true`:

```text
warning: index.db is a symbolic link, so c10r removed the link and left the index it names in place
removed the symbolic link at index.db; the index it names is still there
```

`clear` is the only command that deletes an index, and the MCP server does not expose it.

## dir

Reports the directory that holds the store — the parent of the `--db` path.

The path is reported without canonicalization.
`c10r` does not resolve it against the workspace root or expand it to an absolute path, so the answer matches what every other command means by `--db`.
A `--db` naming a bare filename reports `.`.

A path that is not valid Unicode is rendered lossily, as it is everywhere else `c10r` reports a path, so the reported text does not always round-trip to the bytes you passed.

`dir` reads no file, so it answers whether or not a store exists at the path.

Under `--json` the answer carries the store path alongside the directory:

```json
{
  "dir": ".c10r",
  "db": ".c10r/index.db"
}
```

## size

Reports the size of the files [`clear`](#clear) would remove: the store file plus any `-wal` and `-shm` sidecars.

The figure is the size the filesystem reports, not the blocks it allocates.
The two agree for an ordinary store.

```json
{
  "state": "recognized",
  "path": ".c10r/index.db",
  "bytes": 4404019
}
```

`size` reports a byte count only for a store `c10r` owns.
For anything else it names what it found and still exits 0, because a target `c10r` does not own is never opened:

| `state`        | Meaning                                                                                      | `bytes` |
| -------------- | -------------------------------------------------------------------------------------------- | ------- |
| `recognized`   | A store `c10r` owns, at the schema version this build reads.                                 | present |
| `incompatible` | A store `c10r` owns, at a schema version this build does not read. `clear` still removes it. | present |
| `absent`       | No file at the path.                                                                         | omitted |
| `unrecognized` | A file `c10r` did not create. Left untouched.                                                | omitted |
| `unreadable`   | A path whose contents cannot be examined, such as a directory.                               | omitted |

`bytes` is also omitted when a store `c10r` owns is removed between being classified and being measured.
The state still reports what the classification found, and the missing figure says the measurement did not land.

A store at rest carries no sidecars, so `size` normally reports the store file alone.
When a `-wal` sidecar is present — during a concurrent build, or after one is interrupted — classifying the store opens it, and SQLite extends the `-shm` sidecar as a result.
The reported total then includes that extension.
[`status`](status.md) and [`manifest`](manifest.md) open the store the same way.

## See also

- [`build`](build.md) — rebuild after clearing
- [`status`](status.md) — what the index knows, rather than what it costs
