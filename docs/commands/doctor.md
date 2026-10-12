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

The exit code is decided from indexer readiness alone: 0 when both indexers are ready, 5 otherwise.

## Hooks

`doctor` also reports, for the git worktree the invocation directory is in, whether [`hooks`](hooks.md) will run for each refresh event and which workspaces they refresh.
Hook status never changes the exit code.

Each event is reported in one of four states:

| State                          | Meaning                                                                                                                                                                                                                                          |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| installed                      | A refresh hook runs for this event, naming the workspaces it refreshes, byte-for-byte as `hooks install` would write it.                                                                                                                         |
| modified                       | The file git runs for this event names workspaces, but its content differs from what `hooks install` would write, most commonly a command added by hand, so the workspaces are not reported as refreshed.                                        |
| present, activation unverified | A refresh hook exists at the event's legacy path, but the file at the event's own path is not one this build recognizes as reaching it, so whether the legacy file ever runs cannot be confirmed.                                                |
| not installed                  | No refresh hook runs for this event: none is present, or one is but is not executable, with the fix: the install command, or, when a kept hook remains that would collide with installing, that hook's path and the need to remove or rename it. |

`installed` and `modified` both require the file git actually runs to be executable: the file at the event's own path, or, under a recognized hook-manager shim, both the shim and the legacy file it delegates to.
A c10r file that exists but is not executable is reported `not installed`, since git skips it the same as if it were absent.

```text
rust-analyzer  present      rust-analyzer 1.95.0 (59807616 2026-04-14)
scip-python    present      0.6.6
hooks:
  post-commit  installed  /path/to/project/.git/hooks/post-commit  workspaces: project
  post-rewrite  modified  /path/to/project/.git/hooks/post-rewrite  workspaces named: project  installation and removal replace this file; put commands of your own in post-rewrite.c10r-prior
  post-checkout  installed  /path/to/project/.git/hooks/post-checkout  workspaces: project
  post-merge  not installed  fix: cd '/path/to/project' && c10r hooks install --db='/path/to/project/.c10r/index.db' --workspace='project'
```

When the hook files for different events name different workspace sets, `doctor` adds a line stating that the hook files disagree, alongside each event's own reported workspaces.

Outside a git worktree, or when the worktree's hooks directory cannot be resolved, the report states that hook status does not apply and names why:

```text
hooks: not applicable (/home/me/scratch is not inside a git worktree, so c10r cannot read its git history or install hooks for it. Run the command from inside a git repository.)
```

### JSON shape

Under `--json`, the answer is one object with an `indexers` array and a `hooks` object:

```json
{
  "indexers": [
    {
      "name": "rust-analyzer",
      "presence": "present",
      "version": "rust-analyzer 1.95.0 (59807616 2026-04-14)"
    },
    {
      "name": "scip-python",
      "presence": "present",
      "version": "0.6.6"
    }
  ],
  "hooks": {
    "events": [
      {
        "event": "post-commit",
        "state": "installed",
        "path": "/path/to/project/.git/hooks/post-commit",
        "workspaces": [
          "project"
        ]
      },
      {
        "event": "post-rewrite",
        "state": "modified",
        "path": "/path/to/project/.git/hooks/post-rewrite",
        "workspaces_named": [
          "project"
        ],
        "note": "installation and removal replace this file; put commands of your own in post-rewrite.c10r-prior"
      },
      {
        "event": "post-checkout",
        "state": "installed",
        "path": "/path/to/project/.git/hooks/post-checkout",
        "workspaces": [
          "project"
        ]
      },
      {
        "event": "post-merge",
        "state": "not_installed",
        "fix": "cd '/path/to/project' && c10r hooks install --db='/path/to/project/.c10r/index.db' --workspace='project'"
      }
    ],
    "disagreement": false
  }
}
```

A `modified` event carries `workspaces_named` instead of `workspaces`, naming the workspaces its file names without asserting that they are refreshed, because an edit can stop the refresh lines from running.
It also carries a `note` stating that install and removal replace the file and naming the kept hook (`<event>.c10r-prior`) as where your own commands belong.
An `unverified` event carries `workspaces_named` in the same way.

Outside a git worktree, `hooks` carries only a `reason`:

```json
{
  "reason": "/home/me/scratch is not inside a git worktree, so c10r cannot read its git history or install hooks for it. Run the command from inside a git repository."
}
```

## See also

- [`hooks`](hooks.md) — install, remove, or run the refresh hooks `doctor` reports on
