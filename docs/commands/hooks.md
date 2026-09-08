# c10r hooks

Install the repository hook that keeps the index current.

```sh
c10r hooks install
```

## Arguments

| Argument | Meaning                                                                      |
| -------- | ---------------------------------------------------------------------------- |
| `ACTION` | `install` — install the post-commit hook that refreshes the index. Required. |

## Options

`hooks` takes only the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## What it installs

A post-commit hook that reruns [`c10r build`](build.md), so the index tracks `HEAD`.

The path is resolved through git, so the hook lands in a linked worktree's hooks directory or a relocated `core.hooksPath`.

If a hook already exists at that path, `c10r` refuses rather than overwriting it.
Add the `c10r build` line to your existing hook by hand.
