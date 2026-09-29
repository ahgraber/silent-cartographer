# Design: refresh-hooks

## Context

`hooks install` writes one `post-commit` file whose body is `c10r build <root> --db <db> --workspace <ws>`, with each value resolved to an absolute path and shell-quoted ([src/commands.rs](../../../src/commands.rs), `post_commit_hook_script`).
It resolves the hook path with `git rev-parse --git-path hooks/post-commit` ([src/git.rs](../../../src/git.rs)), so linked worktrees and relocated hook directories already work.
It refuses when any file exists at that path.

`hooks` takes a required positional `HookAction` value (`install`), and `root`, `--db`, and `--workspace` come from the invocation directory and the global flags ([src/main.rs](../../../src/main.rs)).
The manifest walk reports each subcommand's positional arguments and their enumerated values, one level deep.

`build` returns early when the store already describes the workspace under the same sources, analyzer, environment, and chunk parameters.
A hook that fires when nothing relevant changed costs a source hash, not an analysis.

The observed git and prek behavior this design relies on is recorded in the proposal's Approach section.
The facts that drive decisions below:

- git runs one file per hook event from the hooks directory.
- With the default rebase backend, the rebase-merge directory exists from a rebase's `post-checkout` through its `post-rewrite`.
- At that `post-checkout`, the to-do list is empty exactly when the rebase has nothing to replay.
- With the apply backend, nothing at the rebase's `post-checkout` distinguishes it from a branch switch.
- A squash merge fires `post-merge` with the squash flag `1` and does not move HEAD.
- A `post-checkout` hook that fails makes `git switch` and `git checkout` fail with exit status 1, whatever status the hook returned.
- prek's shim runs `<event>.legacy`, including one created after `prek install`, with git's arguments, and fails the git operation when it fails.
- `prek install` moves a foreign event file to `<event>.legacy`, replacing any file already there.
- `prek install -f` removes `<event>.legacy`.
  `prek uninstall` moves `<event>.legacy` back to the event path.
- A prek shim started with `PREK_RUNNING_LEGACY=1` prints an error and exits 0 without running hooks.

## Decisions

### Decision: RulesInTheBinaryChainingInTheScript

**Chosen:** Each hook file is a short `#!/bin/sh` script.
The script runs the kept hook, if any, then calls `c10r hooks run <event>` once per installed workspace.
`hooks run` decides whether to build and builds.

**Rationale:** The refresh rules (rebase in progress, to-do list empty, checkout flag, squash flag, `post-rewrite` argument) are the part most likely to change.
In the binary, they are covered by Rust tests, and upgrading c10r changes them without rewriting any hook file.
Chaining stays in the script so that the kept hook still runs when `c10r` is missing from `PATH`, or crashes.

**Alternatives considered:**

- All logic in the generated shell script: untestable, and every rule change requires users to reinstall.
- The hook file only does `exec c10r hooks run …`, with chaining in Rust: if `c10r` is not on `PATH`, the kept hook stops running, and a missing binary would silently disable another tool's hooks.

### Decision: HookFileLayout

**Chosen:** The generated file for event `E`:

```sh
#!/bin/sh
# c10r refresh hook — managed by `c10r hooks install` and `c10r hooks uninstall`
status=0
prior="$(dirname "$0")/E.c10r-prior"
if [ -f "$prior" ] && [ -x "$prior" ]; then "$prior" "$@"; status=$?; fi
c10r hooks run E --root='<root>' --db='<db>' --workspace='<ws>' -- "$@"
c10r hooks run E --root='<root2>' --db='<db2>' --workspace='<ws2>' -- "$@"
exit $status
```

The second line is the marker that identifies the file as c10r's.
The workspace lines are the only variable part.
One module renders and parses them, and a round-trip test covers it.

Values use the `--flag=value` form, so a value that starts with `-` is not read as a flag.
The kept hook always runs first when it is a regular file and executable (`-f` and `-x` follow a symbolic link; a directory at that path is skipped), with the script's own standard input, because it may read it (`post-rewrite` receives the rewritten-commit list on standard input) and `hooks run` never reads standard input.
The script exits with the kept hook's status, and the `hooks run` lines do not change it.
The kept hook sees `$0` as the `.c10r-prior` path, not its original path.

**Rationale:** The spec requires the kept hook's exit status to reach git, and requires a refresh never to change it.
Returning `$status` and ignoring the `hooks run` status gives both.
There is no rule to skip the kept hook: placement (below) guarantees c10r never keeps a copy of a hook that git already runs.

**Alternatives considered:**

- Skipping the kept hook when c10r runs as `E.legacy`: a user hook kept from `E.legacy` would never run.
- Skipping the kept hook when it is byte-identical to the file at `E`: identical bytes can do different work depending on `$0` or the environment (shown in the design debate).
- Running the kept hook after the refresh: the user would wait for a build before seeing the kept hook's output, and a hook reading standard input would lose it if `hooks run` consumed it.

### Decision: SupportedHookManagers

**Chosen:** A file is a recognized prek shim when its whole content matches a captured prek shim form.
The form is the generated text with two variable parts: the `# ID:` line and the `PREK=` path.
It includes line 2 (`# File generated by prek: …`) and the `exec "$PREK" hook-impl --hook-dir "$HERE" … --hook-type=<E> -- "$@"` line that performs the delegation to `E.legacy`.
Each supported form is stored as a test fixture captured from a named prek version.
A file that matches no stored form is not a recognized shim.
That includes a file with only the header, a modified shim, or a future shim form.

**Rationale:** Placement at `E.legacy` is correct only when the file at `E` runs `E.legacy`.
File presence and header lines do not establish that; a matched whole-file form does.
An unrecognized prek shim falls back to ordinary chaining.
There, the worst observed cost is prek's own `migration mode` error line on a later `prek install`, not a lost hook or a missing refresh.

**Alternatives considered:**

- Header-only recognition: a file that keeps the header but no longer delegates would receive c10r at a path git never runs (shown in the design debate).
- Recognizing pre-commit by its documented shim: its behavior with `E.legacy` is unobserved, so it waits for a later change.

### Decision: WorkspaceSetLivesInTheHookFiles

**Chosen:** The installed workspaces are the workspace lines in c10r's hook files.
`install` adds its workspace to each present c10r file that lacks it.
A c10r file missing for an event is rebuilt with the workspaces the present files name, but only when every present file names the same set.
When present files disagree, `install` refuses before writing anything and names the files.
The exception is files that differ only by the workspace being installed: removing that workspace from every file leaves one set, so `install` proceeds.
That state is what an installation that failed partway leaves, so a retry completes it, and a workspace removed from only some files is still refused.
`uninstall` removes its workspace from each present c10r file that names it, whether or not the files agree.
Each file is written to a temporary file in the hooks directory and renamed into place.

**Rationale:** No new state outside the hooks directory, and no git configuration.
Editing one workspace per file means a partial uninstall is finished by running uninstall again, and never undone by a later install.
Rebuilding a missing file from agreeing files repairs an event whose file another tool deleted.

**Alternatives considered:**

- Writing the union of all files into every file: a partial uninstall or a hand edit would put a removed workspace back into every file (raised in the design debate).
- A registry file in the git directory: a second source of truth that can disagree with the hook files, and a file in `.git` that is not part of any git mechanism.

### Decision: InstallPlacement

**Chosen:** For each event `E`, `install` acts on the first case that applies:

1. A c10r file exists at `E` or at `E.legacy`: update its workspace lines in place.
   If its content is already correct, do not write it.
2. The file at `E` is a recognized prek shim: the slot is `E.legacy`.
   Otherwise, the slot is `E`.
3. If `E.c10r-prior` exists (a kept hook with no c10r file), compare it with the file in the slot.
   Two regular files are identical when their bytes match.
   Two symbolic links are identical when their link targets are the same string.
   Any other pair is different.
   When they differ, refuse before changing any file, and name `E.c10r-prior`.
4. If a file is in the slot: with `--overwrite`, delete it; otherwise, move it to `E.c10r-prior`, replacing an identical one.
   A symbolic link is moved as a link.
5. Write c10r's file into the slot.

All four events' files are written to temporary files before any file in the hooks directory changes, and each temporary file is renamed into its slot last.
A write failure then changes nothing.
If the final rename fails after a kept hook moved, the kept hook is renamed back to its original path.
Uninstall restores a kept hook by renaming it over c10r's file in one step.

A slot at `E` leaves any `E.legacy` untouched.
The existing refusal for a dangling symbolic link at `E` applies, so no write follows a link out of the hooks directory.

**Rationale:** When prek owns `E`, c10r sits where prek runs it, so c10r never keeps a copy of prek's shim and prek never runs twice.
A later `prek install` finds its own shim at `E` and leaves `E.legacy` alone, so a user hook that c10r kept from `E.legacy` is not overwritten.
Any other file at `E` is chained, whatever else is in the directory.
The chaining matrix (`matrix.sh chain-v3.sh` in the debate directory) shows every tested order ending with the right hooks.

**Alternatives considered:**

- Always taking `E`: a later `prek install` moves c10r onto `E.legacy`, overwriting a user hook already there (observed).
- Taking `E.legacy` whenever both `E` and `E.legacy` exist: an unrelated hook at `E` never runs `E.legacy`, so no refresh runs (shown in the design debate).
- Replacing a different `E.c10r-prior` silently: it discards a hook c10r kept, which may be a user's.

### Decision: UninstallPlacement

**Chosen:** When workspaces remain, `uninstall` removes its workspace line from each c10r file.
When none remain, for each event, it deletes c10r's file and moves `E.c10r-prior`, if present, into the slot c10r's file occupied.

**Rationale:** Returning the kept hook to c10r's slot restores the chain as it was before c10r.
A hook kept from `E` returns to `E`.
A hook kept from `E.legacy` under prek returns to `E.legacy`, where prek runs it and `prek uninstall` later restores it to `E`.

### Decision: RefreshRules

**Chosen:** `hooks run <event> -- <git args>` builds when:

| Event           | Builds when                                                                                                     |
| --------------- | --------------------------------------------------------------------------------------------------------------- |
| `post-commit`   | no rebase is in progress                                                                                        |
| `post-rewrite`  | the first argument is `rebase`                                                                                  |
| `post-checkout` | the third argument (flag) is `1`, and either no rebase is in progress or the rebase's to-do list has no entries |
| `post-merge`    | the first argument (squash flag) is `0`                                                                         |

A rebase is in progress when the directory `git rev-parse --git-path rebase-merge` names exists.
A to-do list entry is a line of `git-rebase-todo` in that directory that is neither blank nor a `#` comment.
The build is the existing `run_build` with the line's root, `--db`, and `--workspace`, the language of the backend the existing index records (none when no index exists yet), and defaults for everything else.

`hooks run` always exits 0.
A build failure is written to standard error with the event and workspace named.

**Rationale:** `post-rewrite amend` is skipped because `post-commit` already built for the amend.
A squash merge is skipped because it leaves HEAD unchanged, and the commit that follows it builds.
Exiting 0 keeps a failed refresh from failing `git switch`, as the spec requires.
With the apply backend, the rebase's `post-checkout` looks like a branch switch and builds, and `post-rewrite rebase` builds again.
The user accepted this two-build bound.

**Fallback (agreed with the user):** if more rebase edge cases keep forcing rework of the to-do list rule, `post-checkout` skips during every rebase.
A rebase with nothing to replay then leaves the index out of date.
The spec's "A rebase with nothing to replay refreshes once" scenario would change with it.

**Alternatives considered:**

- Building at every `post-checkout`, including during a rebase: two full builds per default-backend rebase.
- Treating `rebase-apply` as a rebase in progress: with the apply backend, that directory does not exist yet at the rebase's `post-checkout`, so it would not prevent the first build.
- Using `GIT_REFLOG_ACTION`: it is empty at `post-checkout` for both backends (observed).

### Decision: HooksCommandShape

**Chosen:** `HookAction` gains `uninstall` and `run`.
`HooksArgs` gains an optional positional `event` (required by `run`, rejected otherwise), a `--overwrite` flag (accepted only with `install`), explicit `--root` (for `run`), and trailing arguments after `--` (for `run`).
A flag or positional given with the wrong action is a usage error that names the action it belongs to.
`run` is not hidden.
Its help text says the installed hooks call it.

**Rationale:** This keeps the positional-action shape `hooks` and `cache` already use, so the manifest walk and the completion generator report the new actions with no change to either.
Hiding `run` would hide a command that appears in every hook file a user may read.

**Alternatives considered:**

- Nested clap subcommands per action: the manifest walk is one level deep and would not report them.

### Decision: StaleRefreshCommand

**Chosen:** A stale answer carries a `refresh` field holding one shell command line.
It is `c10r build <root> --db <db> --workspace <ws>`, with values shell-quoted as the hook lines are.
`--language` names the backend the index records, so a workspace with both a `Cargo.toml` and a `pyproject.toml` rebuilds the same backend.
`--environment <path>` is added for the environment in effect for the query, when the backend resolves one.
`--chunk-size` and `--chunk-overlap` are added when the recorded values differ from the defaults.
When the index records no root, the field says that the root is unknown instead of naming one.
The human rendering prints the line after the stale marker.

**Rationale:** The spec requires that running the command makes the same query fresh.
Freshness compares the recorded environment with the one in effect for the query, so the rebuild must record the environment in effect, not the one recorded earlier.
The recorded chunk parameters are kept so the rebuild does not change the semantic-index identity.
Fresh answers omit the field, so the JSON shape of fresh answers does not change.

**Alternatives considered:**

- Pinning the recorded environment: when the active environment changed, the rebuild records the old one again and the query stays stale (raised in the design debate).
- Reusing impact's multi-step recovery recipe: that recipe builds a separate index at a base revision on purpose.
  A stale answer needs the live index rebuilt in place.

### Decision: CanonicalEnvironmentPath

**Chosen:** `resolve_environment` returns the canonical absolute path of the environment it resolves, for all three sources: an explicit path, `$VIRTUAL_ENV`, and a `.venv` or `venv` directory under the workspace root.
When canonicalization fails, it returns the path as resolved.

**Rationale:** Freshness compares the recorded environment path with the path resolved for a query as text.
The hooks build with an absolute root and record `/abs/root/.venv`.
A query runs with the root `.` and resolves `./.venv`.
The two name the same directory, but the text differs, so every answer from a hook-built Python index was marked stale because of the environment (observed with a `.venv` workspace and no activated environment).
A canonical path is the same for both.
It also makes the stale refresh command's `--environment` value absolute, so the command runs from any directory.

**Migration:** An index recorded with a relative environment path answers stale once after the upgrade.
One rebuild records the canonical path.
The store schema does not change.

**Alternatives considered:**

- Comparing paths by canonicalizing only at comparison time: the recorded value keeps a relative spelling that depends on the build's working directory, so the refresh command stays relative.
- Passing an absolute root to every query: queries take no root argument, and every call site would change.

### Decision: DoctorJsonShape

**Chosen:** `doctor --json` answers one object with an `indexers` array and a `hooks` object.
Before this change, it answered a bare array of indexers.

**Rationale:** The hook section is per worktree, not per indexer, so it cannot be an entry in the indexer array.
An object holds both sections and can take more sections without another break.
The break is accepted: `doctor` has no MCP tool, and the change is documented in `docs/commands/doctor.md`.

**Alternatives considered:**

- A separate `hooks status` action: a second command to learn, and `doctor` is where a user already looks when something is not working.

### Decision: StatusHumanRendering

**Chosen:** Without `--json`, `status` prints labeled lines in the style of `doctor`'s report, projected from the same JSON value.
The JSON answer is unchanged, except for the added `refresh` field on a stale index.
The field is the same command a stale query answer carries, built by the same function.

**Rationale:** The baseline output discipline requires the default rendering to be a human projection of the machine answer.
`status` printed the pretty-printed JSON instead.
A stale `status` is where a user looks first when the index stops refreshing, so it names the fix as query answers do.

**Alternatives considered:**

- Listing the stale files: the store records one hash over all sources, so naming files needs a per-file hash and a schema version bump.
  This is left to a separate change.

## Architecture

```text
git operation
   │  runs  <hooks>/E
   │        (or prek's shim at E, which runs <hooks>/E.legacy)
   ▼
generated script ── runs ──▶ <hooks>/E.c10r-prior   (kept hook, if executable)
   │                           exit status ──▶ script exit status ──▶ git
   │  for each workspace line
   ▼
c10r hooks run E ── rules (rebase dir, to-do list, flags, argument) ──▶ run_build
   │                                                                     (no-op when current)
   └── always exit 0; failures to standard error

c10r hooks install/uninstall ── reads/writes ──▶ <hooks>/E, E.legacy, E.c10r-prior   (E ∈ four events)
   └── recognizes prek shims against captured forms
c10r doctor ── reads ──▶ same files ──▶ per-event status + workspaces
query answer (stale) ── metadata + environment in effect ──▶ refresh command line
```

## Risks

- **git changes the rebase working files.**
  The rebase-in-progress check and the to-do list rule read files git does not document.
  Mitigation: integration tests run real rebases through the installed binary, so a git upgrade that breaks the rule fails a test.
  The agreed fallback removes the to-do list dependency.
- **prek changes its shim.**
  A new shim form is unrecognized until its fixture is added, so c10r chains to it.
  The observed cost is prek's own error line on a later `prek install`, not a lost hook.
  Mitigation: `doctor` reports the file at `E` as unrecognized.
- **Hook builds use a different environment than a manual build.**
  Hook lines pass no `--environment`, as the current hook does not, so a hook build resolves the environment from `$VIRTUAL_ENV` or the root.
  An index built by a hook in a shell without the project environment can differ from one built by hand.
  This behavior exists today and is unchanged.
- **Foreground builds on branch switches.**
  A switch that changes source files waits for a full analysis.
  Accepted by the user; background builds are out of scope.

## Verification Waivers

- **Requirement:** Existing hooks keep running (the scenarios naming prek) and Refresh-hook removal ("Removing while a supported hook manager is installed") **Reason:** prek is not a project dependency, and the test suite does not install it.
  Automated tests cover prek shim recognition against captured fixtures, and cover chaining with stand-in scripts.
  They do not run prek itself.
  **Manual evidence:** the chaining matrix from the design debate (`chain-v3.sh`, `matrix.sh`, `prek-late-legacy.sh`), rerun against the built binary during apply and captured with its output in `.specs/changes/refresh-hooks/evidence/prek.md`.
  **Recorded:** 2026-09-27
