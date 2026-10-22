# Design: force-fresh-store

## Context

- `run_build` in [src/commands.rs](../../../src/commands.rs) checks `force` only to skip the currency check.
  It then opens the store with `GraphStore::open_or_replace`, which reuses a recognized store at the binary's schema version as it is, and `ingest_with_params` rebuilds every row inside one transaction (`begin_build`).
- `GraphStore::create` in [src/graph/store.rs](../../../src/graph/store.rs) already writes a new store into `<store>.c10r-tmp-<pid>` beside the final path, applies `SCHEMA_SQL` and both stamps, and renames the file into place.
  It refuses with `StaleBuildFile` when that exact temporary path already exists.
- `recognize_or_refuse` decides whether a file is a store the system created, from the file header, before any connection opens.
- The MCP `build` tool in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py) runs an acknowledgment gate and then asks the user through elicitation.
  It composes the displayed command, the shell remedy, and the arguments it runs from one argument list.
- `retained-utility` proposes a build prompt with the same terminal rule and a `confirm` closure passed to `run_build`; it is not built yet.
- The command-surface version is `SURFACE_VERSION` in [src/manifest.rs](../../../src/manifest.rs), currently 14, and the MCP package pins it.

## Decisions

### Decision: A forced build writes a new store beside the old one and renames it into place

**Chosen:** A forced build does not open the existing store for writing.
`GraphStore` gains a constructor that creates an empty store at the temporary path, with `SCHEMA_SQL` and both stamps applied, and returns it with that path.
`run_build` ingests into it as it ingests into any store, closes the connection, and renames the temporary file over the final path.
The rename happens only after `ingest_with_params` has committed.
A guard value owns the temporary path from creation until the rename and removes the file when it is dropped, so an error or a panic between creation and rename leaves no temporary store behind.
The `-wal` and `-shm` side files of the old store are removed after the rename, as `remove_store_files` removes them today.

**Rationale:** The rename replaces a file in the same directory, so a reader sees either the old store or the new one, never a mix.
A query that already holds the old store open keeps reading it through its file handle until it closes.
Every build already recomputes every row, so the forced build costs the same time it costs today.

**Alternatives considered:**

- Delete the existing store, then build at the final path: a failed forced build would leave no index, against the story `a-failed-force-keeps-the-index`.
- Drop and recreate the tables inside the existing file: the file would keep its largest size, and anything else in the file would survive.

### Decision: A forced build checks free space before it analyzes

**Chosen:** After the ownership check and before analysis, a forced build over an existing store reads the store file's size and the free space of the file system that holds it, with `libc::statvfs` on the store's directory (`f_bavail × f_frsize`, the space available to an unprivileged process).
The build needs the store size plus 10%, rounded up to a whole byte.
When the free space is smaller, the build fails with the general failure code and the message `not enough free space to build a new index beside the existing one: needs <n> bytes (the index size plus 10%), <m> bytes free on the file system holding <path>. Free space, or remove the index with `c10r cache clear` and build again.` No check runs when no store exists at the store path, because then only one store is ever written.
`run_build` reads the free bytes through a function value that `main` sets to the `statvfs` reader, so tests substitute a small value without filling a disk.
`docs/commands/build.md` documents the margin.

**Rationale:** The old and the new store exist together until the rename, and the new store's size is unknown until it is built.
The old store's size is the best estimate available before analysis; the margin covers a workspace that grew since it was built.
Failing before analysis saves the minutes a large analysis takes, and the old store is never at risk either way.
The estimate can refuse a build that would have fit, for example when the old store was left large by a bigger workspace; the message names `c10r cache clear`, the command that removes the index, as the way out.

**Alternatives considered:**

- No check: safe for the index, because a failed build keeps it, but the failure arrives after the analysis.

### Decision: A forced build checks ownership of the existing file first

**Chosen:** Before it creates the temporary store, a forced build runs `recognize_or_refuse` on the final path.
An absent path proceeds; a recognized store proceeds; an unrecognized file is refused with the ownership error, as a non-forced build refuses it today.

**Rationale:** The rename would otherwise overwrite a file the system did not create, which `Store ownership recognition` forbids.

### Decision: A symbolic link at the store path is resolved before the temporary path is derived

**Chosen:** When the final path is a symbolic link, the build follows the link chain with `read_link` until it reaches a path that is not a link, and uses that path as the final path.
A link whose target does not exist yet resolves to that target, and the forced build creates the store there.
The answer names the resolved path as the replaced store.

**Rationale:** Renaming onto the link itself would replace the link with a plain file and leave the store it named stale.
`read_link` resolves only the links at the final path, so the store is written in the directory the user chose, not in a canonicalized parent.

### Decision: Leftover temporary stores are removed by every build that proceeds

**Chosen:** Before the analyzer is resolved and before the currency check, every build that proceeds lists the files in the store's directory whose name is the store's file name followed by `.c10r-tmp-` and a decimal process ID.
A forced build does this only after its ownership check, free-space check, and confirmation, because the requirement "A forced build requires confirmation" states that a refused forced build removes no file.
The store's directory is the directory of the store path after symbolic-link resolution, on every build, because a forced build creates its temporary store there.
For each, it skips the file when that process is still running, then runs `recognize_or_refuse`, and removes the file and its `-journal`, `-wal`, and `-shm` side files only when the file is recognized.
A process is still running when `libc::kill(pid, 0)` succeeds or fails with `EPERM`.
`libc` becomes a direct dependency; it is already in `Cargo.lock` through other crates.
The build's answer lists each removed path.

**Rationale:** Running before the currency check means an already-current build also reclaims the space.
A reused process ID can only keep a leftover, never remove a live store, so the error is on the safe side.
The ownership check keeps the cleanup away from any file the system did not create, whatever its name.

**Alternatives considered:**

- An advisory lock held on the temporary file: SQLite holds its own locks on the same file, and how the two lock kinds interact differs between macOS and Linux.

### Decision: Confirmation is a closure passed to the build, decided before any work

**Chosen:** `run_build` gains a `confirm` parameter, a closure that receives the question and returns whether the user agreed.
`main` builds it from `ask_confirmation` in `commands.rs`, which takes the two terminal flags and the input and output streams, so tests drive it without a terminal: when standard input and standard error are both terminals, it writes the question to standard error and reads one line from standard input; any answer other than `y` or `yes`, ignoring case and surrounding space, is no, and end of input is no. Otherwise the closure answers no without asking.
The hook runner passes a closure that never asks.
`run_build` calls it only for a forced build without `--yes`, after the link resolution, the ownership check, and the free-space check, and before the analyzer is resolved, so the question is asked only for a build that can run; a no becomes `Failure::Usage` with a message that names `--yes`.
`--yes` is a `build` flag declared with clap's `requires = "force"`, so `--yes` alone is a usage error before `run_build` runs.
The question reads: `--force replaces the stored index at <path> with a new one built from scratch. Continue? [y/N]`.

**Rationale:** Tests drive both answers through the closure without a terminal.
This is the same shape `retained-utility` designs for its structural-build prompt, so the two prompts share one closure when both are built.
Whichever change lands first adds the parameter; the second reuses it.

### Decision: The build answer reports the replaced store and removed leftovers

**Chosen:** `BuildOutcome` gains `replaced_store: Option<PathBuf>` and `removed_temporary_stores: Vec<PathBuf>`.
The JSON answer carries `replaced_store` only when a forced build replaced a store, and `removed_temporary_stores` only when it is not empty.
The human rendering adds `replaced the index at <path>` and one `removed a leftover temporary store: <path>` line per removal.

**Rationale:** Fields that appear only when they apply leave every existing answer byte-identical, so callers that read today's answer are unaffected.

### Decision: The MCP build tool confirms a forced build and passes `--yes` only to the command it runs

**Chosen:** For `force`, the cost text in the confirmation says: `replaces the stored index with a new one built from scratch by running an external indexer across the whole workspace, which can take minutes.` The displayed command and the shell remedy stay `c10r build --force`, so a user who runs it at a terminal is asked there.
After the user confirms, the server appends `--yes` to the arguments it runs, because the command line it starts has no terminal.
The shared consent helper takes a list of arguments that it appends only to the command it runs, never to the displayed command or the remedy.
The tool's parameters do not change.

**Rationale:** The dialog is the user's confirmation, and `--yes` records it for the command line.
Showing `--yes` in the remedy would teach the user a command that skips the question.

### Decision: The command-surface version moves to 15

**Chosen:** `SURFACE_VERSION` becomes 15 for the new `--yes` flag, the surface manifest snapshot is regenerated, and the MCP package's pinned version moves with it.

**Rationale:** The surface version tracks commands and flags, and the MCP server refuses a binary whose surface it was not built against.

## Architecture

```text
c10r build --force [--yes]
  main
    confirm closure: terminal? ask on stderr, read stdin : answer no
  run_build
    validate arguments                       (--yes without --force rejected by clap)
    resolve symbolic link at the store path
    forced: recognize_or_refuse(final path)   unrecognized -> ownership error
            free space >= store size + 10%?   no -> general failure naming both sizes
            not --yes? confirm(question)      no -> usage error naming --yes
    remove leftover temporary stores          (pattern, process ended, recognized)
    resolve analyzer, collect sources
    forced: analyze
            read the old store's workspace     (handoff disclosure)
            GraphStore::create_pending at <final>.c10r-tmp-<pid>   (guard removes it on any exit)
            ingest_with_params -> commit
            close, rename temporary over final, remove old side files
            answer: replaced_store, removed_temporary_stores
    not forced: currency check, open_or_replace, ingest in place (unchanged)

MCP build(force=true, acknowledge=true)
  acknowledgment gate -> elicitation ("replaces the stored index ...")
  confirmed -> run `c10r build --force --yes`
```

## Risks

- **Disk space during a forced build:** the old store and the new one exist together until the rename.
  The free-space check catches most shortfalls before analysis; a new store that outgrows the margin still fails during writing, and that failure leaves the old store in place and removes the partial new one.
- **A stopped build that dies while creating its temporary store:** if it stops before the stamps are written, the file is not recognized and the cleanup keeps it.
  Such a file is small, because no rows have been written yet.
- **Process ID reuse:** an unrelated live process with a leftover's process ID keeps that leftover until the process exits.
- **Untested path: ingestion fails after the temporary store exists.**
  No test reaches this failure through `run_build`, because the stub analyzers always produce an index that ingests.
  A store unit test covers the guard that removes an unpublished temporary store, and the build passes the ingestion error up before the rename.
  The user accepted this gap on 2026-10-21 rather than add a fault-injection point or a hand-built invalid index.
- **Two prompts in one build:** if `retained-utility` lands first, a forced build of a workspace whose indexer then fails could ask twice; the second question belongs to that change's flow and is asked only after the first was answered yes.
