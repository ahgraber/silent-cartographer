# c10r hooks

Install, remove, or run the repository hooks that keep the index tracking the checked-out commit.

```sh
c10r hooks install
c10r hooks uninstall
```

## Arguments

| Argument | Meaning                                                                                          |
| -------- | ------------------------------------------------------------------------------------------------ |
| `ACTION` | `install`, `uninstall`, or `run`. Required.                                                      |
| `EVENT`  | The refresh event this invocation fired for. Required by `run`; rejected for every other action. |

The installed hooks call `run`: `post-commit`, `post-rewrite`, `post-checkout`, and `post-merge` each invoke `c10r hooks run <event> -- "$@"`.
It is not meant to be typed by hand, though its output is always safe to read.

## Options

| Flag            | Meaning                                                                           |
| --------------- | --------------------------------------------------------------------------------- |
| `--overwrite`   | With `install`, delete an existing hook file instead of keeping it.               |
| `--root <PATH>` | The workspace root the refresh applies to. Required by `run`; rejected otherwise. |

An event, `--root`, or trailing arguments after `--` given to `install` or `uninstall` is a usage error naming `run` as the action they belong to; `--overwrite` given to `uninstall` or `run` is a usage error naming `install`.

`hooks` also takes the [common flags](common.md#shared-flags): `--db`, `--workspace`, `--json`, `--color`.

## What it installs

`install` writes an executable file for each of four events (`post-commit`, `post-rewrite`, `post-checkout`, `post-merge`) into the hooks directory git resolves for the current worktree (`.git/hooks`, a linked worktree's own hooks directory, or a relocated `core.hooksPath`).
Each file carries one `c10r hooks run` line per workspace installed into it:

```sh
#!/bin/sh
# c10r refresh hook — managed by `c10r hooks install` and `c10r hooks uninstall`
status=0
prior="$(dirname "$0")/post-commit.c10r-prior"
if [ -f "$prior" ] && [ -x "$prior" ]; then "$prior" "$@"; status=$?; fi
c10r hooks run post-commit --root='/path/to/project' --db='/path/to/project/.c10r/index.db' --workspace='project' -- "$@"
exit $status
```

`hooks run` decides, from the event's own rules, whether to build, and runs [`build`](build.md) when it does.
`build` already does nothing when the index matches the sources, so an event that moves `HEAD` without changing source files costs a hash check, not an analysis.

`install` and `uninstall` replace c10r's hook files outright; a file `install` writes is not a place to add your own commands, because a later `install` or `uninstall` overwrites or removes it.
A command you want to run on an event belongs in the kept hook, `<event>.c10r-prior`, which every generated script runs first.
See [existing hooks keep running](#existing-hooks-keep-running).

`install` reports the path it wrote for each event:

```text
wrote /path/to/project/.git/hooks/post-commit
wrote /path/to/project/.git/hooks/post-rewrite
wrote /path/to/project/.git/hooks/post-checkout
wrote /path/to/project/.git/hooks/post-merge
```

`install` never writes git configuration: it works only in the hooks directory git resolves, never through `core.hooksPath` or `hook.<name>.*` settings.

## When each event refreshes

| Event           | Refreshes                                                                                    | Does not refresh                                                                                                            |
| --------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `post-commit`   | An ordinary commit, and `git commit --amend`.                                                | A commit a rebase makes while replaying.                                                                                    |
| `post-rewrite`  | A rebase, once it finishes replaying commits.                                                | An amend, because `post-commit` already refreshed.                                                                          |
| `post-checkout` | A branch checkout or switch, and a rebase's starting checkout when it has nothing to replay. | A file checkout (`git checkout -- <file>`, `git restore`); a rebase's starting checkout when it has commits left to replay. |
| `post-merge`    | A merge or pull, fast-forward or with a merge commit.                                        | `git merge --squash`, which leaves `HEAD` unchanged.                                                                        |

### Rebases

With git's default backend, a rebase that has commits to replay fires a `post-checkout` when it starts, a `post-commit` for each commit it replays, and a `post-rewrite` when it finishes. c10r skips the starting `post-checkout` and every one of the per-commit `post-commit` events, and refreshes once, at the closing `post-rewrite`.
A rebase with nothing to replay fires only that starting `post-checkout`, with no `post-rewrite` to follow, so c10r refreshes there instead.
Either way, a default-backend rebase refreshes once, whatever the number of commits it replays.

A rebase with the apply backend (`--apply`, or `rebase.backend=apply`) fires no `post-commit` for its replayed commits, and its starting `post-checkout` looks exactly like an ordinary branch switch, so that refreshes; the closing `post-rewrite` then refreshes again.
That backend refreshes at most twice, whatever the number of commits it replays.

A rebase stopped on a conflict does not refresh, even across a commit made to resolve it, until the rebase completes.
Aborting a stopped rebase leaves the index at the commit it already matched; no refresh runs.

A cherry-pick or revert of several commits is not a rebase: each replayed commit fires `post-commit` and refreshes, the same as a series of ordinary commits.

## Several workspaces share one set of hooks

One set of hook files can refresh more than one workspace: each carries its own `c10r hooks run` line.
`install` adds a workspace to every event's file; installing a workspace that is already installed leaves every file byte-for-byte unchanged.
`hooks uninstall` removes a workspace from every file that names it.

Because `uninstall` removes by workspace identity, it needs the same `--db` and `--workspace` (or the same derived default) that `install` used.
With different values, `uninstall` names a different workspace, reports it as not installed, and changes nothing.

## Existing hooks keep running

When an event's path already holds a file `install` did not write, that file is kept: moved to `<event>.c10r-prior`, and run first, with the event's own arguments and standard input, from the hook `install` writes in its place.
The hook file exits with the kept hook's status, so a kept hook that fails still fails the git operation, as it did before c10r was installed.
A refresh never changes the hook's exit status, including when the refresh itself fails.
A kept file that is not executable is preserved but not run, because git would not have run it either.
A dangling symbolic link at an event's path makes `install` refuse, so no write follows the link.
A kept symbolic link is kept as a link, never dereferenced and copied.

`uninstall` reverses this once no workspace remains installed for an event: it deletes c10r's file and moves the kept hook back to the path it occupied.

## prek support

[prek](https://github.com/j178/prek) manages hooks by moving whatever it finds at an event's path to `<event>.legacy` and installing its own shim at the event's path, which runs `<event>.legacy` in turn.
When `install` finds a recognized prek shim at an event's path, it installs its own file at `<event>.legacy` instead of the event's path, so prek's shim and c10r's hook both run, once each, on every event.

Only the shim form written by prek 0.5.3 is recognized.
A shim from any other prek version, or any other hook manager (including pre-commit), is kept as an ordinary hook rather than placed under: prek's own later reinstall may then print its migration-mode notice, but no hook is lost.

`prek install -f` deletes `<event>.legacy` outright, taking c10r's hook with it when c10r's file was there. c10r cannot prevent that; [`doctor`](doctor.md#hooks) reports the missing hook and `c10r hooks install` restores it.

## What stays stale

`git reset --hard` and `git stash pop` fire no git hook at all, so neither refreshes the index.
A query against the resulting stale index carries a `refresh` field naming the command that catches it up; see [the `refresh` field](common.md#the-refresh-field).

## Refusals

`install` refuses, before changing any file, in three cases:

- The workspace root, index path, or workspace identity contains a line break, which the hook file's one-line-per-workspace format cannot carry.
  The refusal names which one.
- The present hook files for different events name different workspace sets.
  The refusal names each disagreeing file and the workspaces it names.
- A hook kept by an earlier install remains at `<event>.c10r-prior` with no c10r file for that event, and differs from the hook `install` would now keep in its place: a different file, or a symbolic link to a different target.
  The refusal names the `.c10r-prior` path and says to remove or rename it.

No refusal changes any file; resolve what the message names and run `install` again.

## Not yet handled

Linked worktrees share one hooks directory across every worktree of a repository, but a hook file names one worktree's root.
A commit in a second worktree rebuilds the first worktree's index (a no-op there) rather than its own.
A follow-up change addresses this.

Hook builds run in the foreground: a branch switch or merge that changes source files waits for the build to finish before the git operation returns.

## See also

- [`build`](build.md) — what a refresh actually runs
- [`doctor`](doctor.md#hooks) — check whether the hooks are installed and which workspaces they refresh
- [common: the `refresh` field](common.md#the-refresh-field) — the stale-answer command that fires when no hook does
