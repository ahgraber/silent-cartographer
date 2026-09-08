# Design: index-store-actions

## Context

`cache` is the only command that deletes anything, and it deletes when run with no action.
`hooks` already uses the shape this change gives `cache`: a required positional action backed by a `ValueEnum`, not a nested clap subcommand.

Three existing pieces decide most of this design:

- `manifest` derives the structural surface by walking the clap tree one level of subcommands deep ([src/manifest.rs](../../../src/manifest.rs)).
  It lists each command's positional arguments and their enumerated values.
- The shell completion script is generated from the same tree.
- `recognize_store` ([src/graph/store.rs](../../../src/graph/store.rs)) decides whether a file at the store path is `c10r`'s.
  Every store-touching path calls it.

## Decisions

### Decision: ActionIsARequiredPositional

**Chosen:** `cache` takes a required positional `ValueEnum` with the values `clear`, `dir`, and `size`, the same shape `hooks` uses.

**Rationale:** the manifest walk reports each subcommand's positional arguments with their enumerated values.
A positional action therefore appears in the structural index as `cache` with a required argument listing the three actions, and the completion generator picks it up from the same tree.
Clap rejects a missing required positional before dispatch, so no handler guard is needed to stop a bare invocation from deleting anything.

**Alternatives considered:**

- A nested clap subcommand (`Cache(CacheCommand)` with three subcommands): the manifest walk would report `cache` with no arguments and would not list the three actions.
  Fixing that requires teaching the walk to recurse, which changes the surface-index contract and gains nothing over the positional.
- A `--clear` flag on the bare command: the bare invocation stays valid, and the destructive path becomes a flag rather than a named action.

### Decision: SharedStoreClassifier

**Chosen:** extract the classification `index_state` performs — absent, unrecognized, unexaminable, incompatible, recognized — into one function in [src/commands.rs](../../../src/commands.rs), and call it from both `index_state` and `cache size`.

**Rationale:** `cache size` reports the same distinctions the structural surface index reports, and the spec now requires that.
Two separate classifications over the same file would diverge.
The recognized case pays one extra store open: the classifier checks the schema version, then `read_index_state` opens the store to read metadata.
Opening an existing SQLite file is cheap, and `manifest` is not on a hot path.

**Alternatives considered:**

- A private classifier inside the `cache size` handler: a smaller diff, but it puts a second decision about the same file in the binary, and the two would diverge when either one changes.
- Reporting an incompatible store as recognized: cheaper, because `size` would not open the store at all.
  It hides the one state whose remedy differs.
  A store the binary cannot read still occupies disk and is still what `clear` removes.

### Decision: SizeCoversWhatClearRemoves

**Chosen:** the total covers the store file plus its `-wal` and `-shm` sidecars, which is the set `run_cache` unlinks.

**Rationale:** the definition is checkable, and it pairs the two actions: `size` reports the size of the files `clear` removes.
The sidecars are usually absent and small.
A store left behind by an interrupted concurrent build can carry a WAL as large as the store itself, which is when an operator asks.

The figure is the size the filesystem reports for each file, not the blocks it allocates.
The two agree for an ordinary SQLite store.
A sparse or compressed file would report more than deleting it frees, which is why the contract is the size of those files rather than the disk that removing them returns.

When any part of the total cannot be established, the action reports no figure at all.
Zero is a size no store has, and a partial total understates what is at the path.
An absent sidecar is the exception: no sidecar is the store's resting state, so it contributes nothing rather than withholding the answer.

**Alternatives considered:**

- The store file alone: the number then excludes disk the index actually uses, and it disagrees with `clear` in the case where the difference is largest.
- Every file in the store's directory: `--db` can point anywhere, so that directory can hold unrelated files.

### Decision: UnownedTargetReportsStateAsSuccess

**Chosen:** for an absent, unrecognized, or unexaminable target, `size` reports the state, omits the byte count, and exits with the success code.

**Rationale:** the ownership guard stops `c10r` from writing to a file it does not own.
A target that fails the ownership test is never opened, so `size` cannot write to it, and a refusal would extend the guard past its purpose.
Reporting the foreign file's byte count is the outcome to avoid: the number is correct for a file that is not the index.

**Alternatives considered:**

- Refusing with the unrecognized-store exit code, as `clear` does: consistent within the command, but it leaves "how much disk is this using" unanswerable for the operator who does not yet know what the file is.
- Reporting the byte count alongside the state: a caller reading the byte field of `cache size --json` would get a number for a file `c10r` does not own.
  Omitting the field forces the caller to read the state.

### Decision: DirIsPathArithmetic

**Chosen:** `dir` reports the parent of the `--db` path as given, renders `.` for a bare filename, and reads no file.

**Rationale:** no other command canonicalizes `--db` or resolves it against the workspace root.
Resolving it here would make `cache dir` and `status` print different paths for the same store.
Reading no file also lets `dir` answer before a build, for a path that does not exist, and for a path that cannot be examined.

**Alternatives considered:**

- Canonicalizing the path: fails when the store does not exist, which is one of the two cases the requirement covers.
- Creating the directory: `build` is the command that creates the store directory.

### Decision: SurfaceVersionAndMcpPinMoveTogether

**Chosen:** bump `SURFACE_VERSION` from 9 to 10, bump the MCP server's pin, and regenerate the manifest snapshot fixture, all in this change.

**Rationale:** the "Versioned structural surface index" requirement already obliges the version to change when the command structure changes, and the MCP requirement already obliges the server to refuse a surface it was not built against.
Moving the two constants satisfies both, so neither needs a spec delta.
Splitting them across changes would ship a server that refuses every binary carrying this change.

## Architecture

```text
c10r cache <action>
        |
        +-- clear -> run_cache(db)            unchanged: recognize -> unlink store + sidecars
        |
        +-- dir   -> cache_dir(db)            parent of --db as given; no filesystem contact
        |
        +-- size  -> cache_size(db)
                        |
                        +-- store_state(db) ------+  (shared with index_state, used by `manifest`)
                        |     Absent / Unrecognized / Unreadable / Incompatible / Recognized
                        |
                        +-- Recognized | Incompatible -> sum bytes of db, db-wal, db-shm
                        +-- otherwise                 -> state only, no byte count
                                                          both exit 0
```

The machine answer reuses the state spellings `index_state` emits: `unrecognized`, `unreadable`, and `incompatible`.
It adds `recognized` and `absent`, which `index_state` expresses through its `built` field instead.

## Risks

- **A script or a habit that runs bare `c10r cache` breaks.**
  The failure is a usage error listing the three actions, and nothing is deleted.
  The README table, `docs/commands/cache.md`, and the completion script all change here.
- **`cache size` opens a store to classify it, and opening is not free of effect.**
  The connection is read-only, and a target that fails the ownership test is never opened at all.
  A store that already carries a `-wal` sidecar is the exception worth naming: SQLite extends the `-shm` sidecar to 32768 bytes when it opens such a database, so the reported total includes an extension the command itself caused.
  `status` and `manifest` open the store the same way and have the same effect today.
  A store at rest carries no sidecars, so the common case reports the store's own bytes and creates nothing.
- **An unexaminable path reaches `size`.**
  It becomes a reported state rather than a propagated error.
- **The manifest snapshot fixture and `SURFACE_VERSION` diverge.**
  The snapshot test fails on any structural change, and its failure message gives the regeneration command.
  This change follows it rather than adding new machinery.

## Verification Overrides

- **Finding:** CRITICAL — failing test suite with no recorded override.
  `tests/semantic_engine.rs::python_live_tool_matches_committed_fixture_shape` fails, so `cargo test --release --no-fail-fast` reports one failure alongside 756 passes.
  **Stage:** verify **Reason:** the test builds a Python environment with `python3 -m venv`, and the sandbox on the development machine denies `ensurepip`, so the command cannot run there.
  The failure is environmental and pre-existing: it reproduces identically on the commit this change is built from, and no file this change touches is on its path.
  Blocking the change on it would block every change made on this machine.
  **Constraints:** the test is not to be deleted, skipped, marked `#[ignore]`, or altered to pass.
  The override records that this change proceeded past the failure; it does not excuse the test.
  **Follow-up task:** "Run `cargo test --release --test semantic_engine python_live_tool_matches_committed_fixture_shape` where `python3 -m venv` can create an environment, and record the result." — see `tasks.md` § Overridden verify finding.
  **Approved by:** user **Recorded:** 2026-09-07
