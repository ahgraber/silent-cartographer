# c10r impact

Assess what a change could affect: the dependents of every symbol the change touched, seeded from a git diff rather than from a symbol you name.

```sh
c10r impact                  # working tree against HEAD
c10r impact --staged         # staged changes only
c10r impact v1.2..HEAD       # a revision range
c10r impact -- src/some/dir  # narrowed to a subtree
```

## Arguments

| Argument   | Default                     | Meaning                                                                                                                 |
| ---------- | --------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `REVSPEC`  | working tree against `HEAD` | `A..B`, `A...B`, or a single revision (that revision against the working tree, as `git diff <commit>` does)             |
| `PATHS...` | whole diff                  | After `--`, narrow the seed to these paths. A renamed file is reached by either its pre-change or its post-change path. |

## Options

| Flag                         | Default  | Meaning                                                                                                    |
| ---------------------------- | -------- | ---------------------------------------------------------------------------------------------------------- |
| `--staged`                   | off      | Seed from the staged change against `HEAD` instead of the working tree                                     |
| `--depth <N>`                | `1`      | Hops of transitive impact to detail. `0` is aggregate-only — no rows, everything counted in the aggregate. |
| `--order <ranked\|unranked>` | `ranked` | Row order within each distance layer; see [`trace`](trace.md#dependents)                                   |

See [common flags](common.md#shared-flags) for `--db`, `--workspace`, `--json`, `--color`, `--limit`, and `--cursor`.

## The answer

Returns the declarations the diff touched, then their dependents to the depth bound, with deeper reach aggregated rather than dropped.

Each answer carries an `exactness` grade:

| Grade         | Means                                                                                 |
| ------------- | ------------------------------------------------------------------------------------- |
| `exact`       | The index matches the change's pre-change state, so the reach shown is the real reach |
| `approximate` | Something drifted since the index was built                                           |

An `approximate` answer carries a runnable recovery recipe: shell commands that rebuild the index at the change's base revision in a throwaway worktree and re-run the same query, with every value filled in.

```sh
c10r --json impact | jq -r '.outcome.results[0].recovery.steps[]'
```

## See also

- [`trace --relation dependents`](trace.md#dependents) — the same question, seeded from a symbol you name
- [`build`](build.md) — a current index is what makes an answer `exact`
