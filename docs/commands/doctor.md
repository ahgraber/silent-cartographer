# c10r doctor

Report whether each required language indexer is present, and its version.

```sh
c10r doctor
c10r doctor --json
```

Run it before a first [`build`](build.md), or when a build fails with exit code 5.

## Options

`doctor` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## The indexers it checks

| Backend | Requires                                                                |
| ------- | ----------------------------------------------------------------------- |
| Rust    | `rust-analyzer` on `PATH`, or the path given to `build --rust-analyzer` |
| Python  | `scip-python` (`npm install -g @sourcegraph/scip-python`)               |
