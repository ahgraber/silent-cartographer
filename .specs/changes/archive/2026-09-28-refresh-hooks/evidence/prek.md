# Evidence: refresh hooks beside prek

This file is the manual evidence that the verification waiver in `design.md` names for the scenarios that need prek itself.

Recorded: 2026-09-27.
Versions: git 2.54.0, prek 0.5.3 (b7eb60271 2026-09-13), and c10r built from the `refresh-hooks` branch with `cargo build --release`.

## Chaining matrix

`chain-v3.sh` runs one order of steps in a new repository and makes a commit after each step.
The steps are:

- `H`: write a user hook at `post-commit`.
- `prek`: run `prek install`.
- `prek-f`: run `prek install -f`.
- `prek-un`: run `prek uninstall`.
- `c10r`: run `c10r hooks install` with the built binary.
- `c10r-un`: run `c10r hooks uninstall` with the built binary.

Each line shows the hooks that ran on the commit (`H`, and `prek` for prek's own hook), the number of c10r refreshes (build reports in the commit output), and the files in the hooks directory.
A line ends with `[error output: …]` when the commit output contains prek's migration-mode error or a failed refresh.

Command, from this directory:

```sh
PATH="<worktree>/target/release:$PATH" sh matrix.sh chain-v3.sh
```

Results:

- In every order, `H` runs on every commit while it is installed.
- prek's hook runs once per commit while prek is installed.
- c10r refreshes once per commit while its hook is reachable.
- No commit output contains prek's migration-mode error or a failed refresh.
- After both tools are removed, the hooks directory holds only `H` at `post-commit`, or nothing when `H` was never written.
- `prek install -f` deletes c10r's `post-commit.legacy`.
  On the next commit, neither the refresh nor `H` runs.
  `H` is kept as `post-commit.c10r-prior`.
  The next `c10r hooks install` restores both.

Output:

```text
=== prek c10r prek c10r-un prek-un
prek     runs: prek         c10r refreshes: 0  files: post-commit
c10r     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
prek     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
c10r-un  runs: prek         c10r refreshes: 0  files: post-commit
prek-un  runs: (none)       c10r refreshes: 0  files:
=== H prek c10r prek c10r-un prek-un
H        runs: H            c10r refreshes: 0  files: post-commit
prek     runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
c10r-un  runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
prek-un  runs: H            c10r refreshes: 0  files: post-commit
=== H prek c10r prek prek-un c10r-un
H        runs: H            c10r refreshes: 0  files: post-commit
prek     runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek-un  runs: H            c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
c10r-un  runs: H            c10r refreshes: 0  files: post-commit
=== H c10r prek c10r-un prek-un
H        runs: H            c10r refreshes: 0  files: post-commit
c10r     runs: H            c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
prek     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
c10r-un  runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
prek-un  runs: H            c10r refreshes: 0  files: post-commit
=== H c10r prek prek-un c10r-un
H        runs: H            c10r refreshes: 0  files: post-commit
c10r     runs: H            c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
prek     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek-un  runs: H            c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
c10r-un  runs: H            c10r refreshes: 0  files: post-commit
=== H prek c10r c10r-un prek-un
H        runs: H            c10r refreshes: 0  files: post-commit
prek     runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
c10r-un  runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
prek-un  runs: H            c10r refreshes: 0  files: post-commit
=== c10r prek c10r-un prek-un
c10r     runs: (none)       c10r refreshes: 1  files: post-checkout post-commit post-merge post-rewrite
prek     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
c10r-un  runs: prek         c10r refreshes: 0  files: post-commit
prek-un  runs: (none)       c10r refreshes: 0  files:
=== c10r prek prek-un c10r-un
c10r     runs: (none)       c10r refreshes: 1  files: post-checkout post-commit post-merge post-rewrite
prek     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
prek-un  runs: (none)       c10r refreshes: 1  files: post-checkout post-commit post-merge post-rewrite
c10r-un  runs: (none)       c10r refreshes: 0  files:
=== H prek c10r prek-f c10r c10r-un prek-un
H        runs: H            c10r refreshes: 0  files: post-commit
prek     runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek-f   runs: prek         c10r refreshes: 0  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
c10r-un  runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
prek-un  runs: H            c10r refreshes: 0  files: post-commit
=== prek c10r prek prek-f c10r c10r-un prek-un
prek     runs: prek         c10r refreshes: 0  files: post-commit
c10r     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
prek     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
prek-f   runs: prek         c10r refreshes: 0  files: post-checkout post-commit post-merge post-rewrite
c10r     runs: prek         c10r refreshes: 1  files: post-checkout post-commit post-commit.legacy post-merge post-rewrite
c10r-un  runs: prek         c10r refreshes: 0  files: post-commit
prek-un  runs: (none)       c10r refreshes: 0  files:
=== H c10r prek prek-f c10r c10r-un prek-un
H        runs: H            c10r refreshes: 0  files: post-commit
c10r     runs: H            c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
prek     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
prek-f   runs: prek         c10r refreshes: 0  files: post-checkout post-commit post-commit.c10r-prior post-merge post-rewrite
c10r     runs: H prek       c10r refreshes: 1  files: post-checkout post-commit post-commit.c10r-prior post-commit.legacy post-merge post-rewrite
c10r-un  runs: H prek       c10r refreshes: 0  files: post-commit post-commit.legacy
prek-un  runs: H            c10r refreshes: 0  files: post-commit
```

## A legacy hook created after prek install

`prek-late-legacy.sh` installs prek, then writes `post-commit.legacy` and `post-checkout.legacy` hooks that record their arguments and exit 3. prek runs both, passes git's arguments, and the exit status 3 fails `git switch` (`switch exit 1`).
A c10r file at `<event>.legacy` is therefore run by prek, and a kept hook's failure still reaches git.

Command: `sh prek-late-legacy.sh`

```text
--- prek shim header
#!/bin/sh
# File generated by prek: https://github.com/j178/prek
# ID: 182c10f181da4464a3eec51b83331688
commit exit 0
--- post-commit with a .legacy created after prek install
late-legacy args=
prek
  out: log......................................................................Passed
log......................................................................Passed
switch exit 1
--- post-checkout: does the legacy exit status (3) reach git?
late-legacy args=e481f7283eecb1a8b3bcdbfa36e2ab26c9b27301 e481f7283eecb1a8b3bcdbfa36e2ab26c9b27301 1
prek
```

## Rebase state at each hook

`reflog-action.sh` records, for each hook, whether the `rebase-merge` or `rebase-apply` directory exists, `GIT_REFLOG_ACTION`, and the arguments.
With the default backend, the `rebase-merge` directory exists at the rebase's `post-checkout`.
With the apply backend, the rebase's `post-checkout` sees no rebase directory and an empty `GIT_REFLOG_ACTION`, the same as a plain switch.

Command: `sh reflog-action.sh`

```text
--- merge backend, 2 commits to replay
post-checkout [merge-dir] action='' args=<sha> <sha> 1
post-commit [merge-dir] action='' args=
post-commit [merge-dir] action='' args=
post-rewrite [merge-dir] action='' args=rebase
--- apply backend, 2 commits to replay
post-checkout [idle] action='' args=<sha> <sha> 1
post-applypatch [apply-dir] action='rebase (pick)' args=
post-applypatch [apply-dir] action='rebase (pick)' args=
post-rewrite [apply-dir] action='rebase (pick)' args=rebase
--- merge backend, nothing to replay
post-checkout [merge-dir] action='' args=<sha> <sha> 1
--- apply backend, nothing to replay
post-checkout [idle] action='' args=<sha> <sha> 1
--- plain switch
post-checkout [idle] action='' args=<sha> <sha> 1
--- switch -c at same commit
post-checkout [idle] action='' args=<sha> <sha> 1
--- switch --detach
post-checkout [idle] action='' args=<sha> <sha> 1
```
