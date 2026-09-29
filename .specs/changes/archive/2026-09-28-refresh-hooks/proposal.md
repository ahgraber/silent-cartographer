# Proposal: refresh-hooks

## Intent

`c10r hooks install` writes one `post-commit` hook that runs `c10r build` after every commit.
This has three problems.

A rebase runs `post-commit` once for every commit it replays.
On a large Python project, each run is a full scip-python analysis, so a rebase of ten commits waits for ten builds.

The index refreshes only when a commit is made.
A branch switch, a merge, or a pull moves HEAD without a commit, so the index stays out of date until the next commit.

Git runs one hook file per event from the hooks directory.
Hook managers such as prek and pre-commit own those files too.
Today `hooks install` refuses when the file exists, so a repository that uses a hook manager cannot install the c10r hook.
In this repository, prek owns `post-checkout`, `post-merge`, and `post-rewrite`, and c10r owns `post-commit`.

## User Stories

### Story: rebase-builds-once

As a developer on a large project, I want a rebase to refresh the index once, when the rebase finishes, so that a rebase does not wait for one full build per replayed commit.
A rebase with the non-default apply backend may refresh twice: once before it replays commits and once after.

> Ladders to north-star outcome 5 (**Calibrated trust**): the refresh discipline stays affordable enough to leave installed, so answers keep their freshness.

### Story: index-follows-head

As a developer or agent, I want the index refreshed when a branch switch, merge, or pull moves HEAD, so that answers describe the checked-out code without a manual `c10r build`.

> Ladders to north-star outcome 2 (**Blast radius before change**): the diff-seeded assessment is exact only when the index matches the committed state.
> Ladders to north-star outcome 5 (**Calibrated trust**): fewer answers are stale.

### Story: hooks-coexist

As a developer who uses prek, pre-commit, or several c10r workspaces in one repository, I want c10r's hooks and the other hooks in the same event to all run, so that I do not choose between tools or wire one tool into another.

> Ladders to the guiding principle **Adopt over build**: c10r works beside the hook managers a repository already uses.

### Story: missing-refresh-is-visible

As a developer or agent, I want c10r to tell me when its hook is not installed or the index is out of date, and which command fixes it, so that an index that stopped refreshing does not mislead me.

> Ladders to north-star outcome 4 (**Honest under edit**) and the guiding principle **Calibration over coverage**: a stale answer states the remedy as well as the staleness.

## Scope

**In scope** (two capabilities — `command-surface`, then `code-navigation`):

- Hook events.
  c10r installs hooks for `post-commit`, `post-rewrite`, `post-checkout`, and `post-merge`.
- Rebase and amend rules.
  A rebase with the default backend refreshes the index once.
  A rebase with the apply backend (`--apply`, or `rebase.backend=apply`) refreshes it at most twice.
  An amend refreshes it once.
- Branch switches and merges.
  A branch switch, merge, or pull that moves HEAD refreshes the index.
  A file checkout (`git checkout -- <file>`, `git restore`) does not.
- Chaining.
  When a hook file already exists, `hooks install` keeps it and runs it from c10r's hook.
  It no longer refuses.
  A hook that existed before c10r and prek keeps running, and is back in place after both are removed, whatever the order of installs and removals.
- prek as a supported hook manager.
  When prek's shim owns an event, c10r installs where prek runs it (`<event>.legacy`), so prek's hooks and c10r's refresh each run once.
  Only prek shim forms that a test captures are recognized.
  Any other file is kept as an ordinary hook.
- Several workspaces.
  One c10r hook refreshes every workspace installed into it.
  Installing a workspace that is already installed changes nothing.
- `hooks install --overwrite`.
  It deletes a hook file that installation would otherwise keep.
- `hooks uninstall`.
  It removes a workspace from c10r's hooks.
  When no workspace remains, it removes c10r's hook files and puts each kept hook back where c10r's file was.
- `doctor` reports, per hook event, whether c10r's hook will run and which workspaces it refreshes.
  This covers the case where another tool's forced reinstall (`prek install -f`) deleted c10r's hook.
- A stale answer names the `c10r build` command that refreshes the index (`code-navigation`).
  This is the only signal after operations that fire no hook: `git reset --hard` and `git stash pop`.
- A stale `status` names the same refresh command, and `status` without `--json` prints human text instead of JSON (`code-navigation`, `command-surface`).
- An index built by the hooks answers fresh (`code-navigation`).
  The hooks build with an absolute root, and a query uses the invocation directory, so a Python environment found through `.venv` was recorded and resolved as two different spellings of one path, and every answer was marked stale.
  The environment path is recorded and resolved as one canonical absolute path.
- The surface version bump and the matching MCP pin.
- README documentation for the hook behavior.

**Out of scope:**

- Linked worktrees.
  All worktrees of a repository share one hooks directory, but c10r's hook names one worktree's root.
  A commit in a second worktree rebuilds the first worktree (a no-op) and never refreshes its own index.
  The current hook already has this defect.
  A follow-up change must solve it.
  It is not an open question to leave unresolved.
- Background builds.
  Hooks build in the foreground, as the current hook does.
  A branch switch that changes source files waits for the build.
  A background build is safe only if two builds cannot write one store at the same time, and that is not established.
- Writing git configuration. c10r does not use `core.hooksPath` or config-defined hooks (`hook.<name>.*`).
  It works only in the hooks directory that git resolves.
- Integration through a hook manager's configuration (for example, a prek local hook).
- Preventing another tool's forced reinstall from deleting c10r's hook.
  `prek install -f` replaces the file on purpose. c10r detects and reports the loss (through `doctor` and stale answers) but cannot prevent it.
- MCP tools for `hooks uninstall` or `--overwrite`.
  Both remove files, and the MCP surface already leaves out the command that removes the index store.
- Hook managers other than prek, including pre-commit.
  Their shims are kept as ordinary hooks.
  If such a manager is reinstalled after c10r, its hooks may run twice unless the manager guards against that itself. pre-commit becomes supported in a later change, once its shim form and its handling of `<event>.legacy` are observed.
- husky.
  husky sets `core.hooksPath` and may regenerate the directory it points to.
  c10r installs into whatever hooks directory git resolves, but this change makes no promise about husky.
- Cherry-picks and reverts of several commits.
  Each replayed commit fires `post-commit`, so each one refreshes, as the current hook does.
  The rebase story does not cover them.
- Native support for workspaces with several languages.
  A workspace with both a `Cargo.toml` and a `pyproject.toml` builds one backend, chosen with `--language`.
  The hooks and the stale refresh command reuse the backend the index records.
  A later change should build every detected language and make `--language` a filter.
- Preserving hand edits to c10r's hook files.
  Installation and removal replace the file, and `doctor` reports an edited file as modified.
  A user's own commands belong in the kept hook, which c10r's file runs first.
- A user-written `<event>.legacy` file.
  prek replaces it on install, with or without c10r present, so this change makes no promise to preserve it.

## Approach

Observed behavior, git 2.54 and prek, from scratch repositories:

| Operation                                                 | Hooks fired                                                                                  |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `git switch`, `git checkout <branch>`, `git worktree add` | `post-checkout`, branch flag `1`                                                             |
| `git checkout -- <file>`, `git restore`                   | `post-checkout`, file flag `0`                                                               |
| `git pull` (fast-forward or merge), `git merge`           | `post-merge`                                                                                 |
| `git pull --rebase` with no local commits                 | `post-merge`                                                                                 |
| A rebase that replays commits (default backend)           | `post-checkout` at the start, `post-commit` per commit, `post-rewrite rebase` at the end     |
| A rebase with nothing to replay (default backend)         | `post-checkout` only                                                                         |
| A rebase that replays commits (apply backend)             | `post-checkout` at the start, `post-applypatch` per commit, `post-rewrite rebase` at the end |
| A rebase with nothing to replay (apply backend)           | `post-checkout` only                                                                         |
| `git commit --amend`                                      | `post-commit`, then `post-rewrite amend`                                                     |
| `git cherry-pick A..B`, `git revert A..B`                 | `post-commit` per commit                                                                     |
| `git merge --squash`                                      | `post-merge 1` (squash flag), HEAD unchanged                                                 |
| `git reset --hard`, `git stash pop`                       | none                                                                                         |

With the default backend, the rebase-merge directory (`git rev-parse --git-path rebase-merge`) exists from the rebase's `post-checkout` through its `post-rewrite`.
At the rebase's `post-checkout`, the rebase's to-do list (`git-rebase-todo` in that directory) is empty when there is nothing to replay and has entries otherwise. git's own `contrib/completion/git-prompt.sh` tests for this directory and reads its `head-name`, `msgnum`, and `end` files.
It does not read the to-do list, so the to-do list rule depends on a file no shipped git tool reads.

With the apply backend, no rebase directory exists at the rebase's `post-checkout`, and `GIT_REFLOG_ACTION` is empty there for both backends.
Nothing available at that `post-checkout` distinguishes an apply-backend rebase from a branch switch, so it refreshes, and `post-rewrite rebase` refreshes again.
The user accepted two refreshes for that backend.

The refresh rules per event:

- `post-commit`: build, unless a rebase is in progress.
- `post-rewrite`: build when the argument is `rebase`.
  Do nothing for `amend`, because `post-commit` already built.
- `post-checkout`: do nothing for a file checkout (flag `0`).
  During a rebase, build only when the to-do list is empty.
  Otherwise, build.
- `post-merge`: build, unless the squash flag is `1`.
  A squash merge does not move HEAD, and the commit that follows it fires `post-commit`.

`build` already does nothing when the index matches the sources, so an event that moves HEAD without changing source files costs only a hash check.

The rules live in a `c10r hooks run <event>` subcommand that the hook file calls, so they are tested in Rust, not in generated shell.
The hook file holds one `hooks run` line per installed workspace, with the resolved root, `--db`, and `--workspace`, as the current hook does.
A marker line identifies the file as c10r's, so `install`, `uninstall`, and `doctor` recognize it at the event path or at `<event>.legacy`.

prek's observed behavior:

- `prek install` moves a foreign hook at the event path to `<event>.legacy` and runs it.
  If `<event>.legacy` already exists, prek replaces it.
- prek's shim runs `<event>.legacy` even when that file appears after `prek install`, passes git's arguments to it, and fails the git operation when it fails.
- prek sets `PREK_RUNNING_LEGACY=1` when it runs a legacy hook.
  A prek shim started with that variable prints `error: prek's Git shim is installed in migration mode` and exits 0 without running hooks.
- `prek install -f` removes `<event>.legacy` and replaces the file at the event path.
- `prek uninstall` moves `<event>.legacy` back to the event path.
- prek's shim identifies itself on line 2 with `# File generated by prek: …` and delegates through `exec "$PREK" hook-impl --hook-dir "$HERE" … -- "$@"`.

Chaining, as settled in a design debate with a second agent (`._scratch/refresh-hooks-debate/`):

- When the event path holds a recognized prek shim, c10r installs at `<event>.legacy`.
  A foreign `<event>.legacy` is kept as c10r's prior hook. c10r never chains to a prek shim, so prek never runs twice.
- When the event path holds any other file, c10r moves it to `<event>.c10r-prior` and takes the event path.
  Any `<event>.legacy` is left alone.
- c10r's hook always runs its kept hook first.
- Uninstall puts the kept hook back where c10r's file was.

Two earlier rules failed in that debate.
Always taking the event path let a later `prek install` overwrite a user hook sitting at `<event>.legacy`.
Inferring delegation from file names or identical bytes installed c10r where git never runs it.
The scripts `chain-v3.sh` and `matrix.sh` in that directory reproduce the final rule across fifteen install and uninstall orders.

If new rebase edge cases keep requiring rework of the to-do-list rule, fall back to skipping `post-checkout` during every rebase.
A rebase with nothing to replay then leaves the index out of date, and stale answers report it.
This fallback was agreed with the user before this proposal.
