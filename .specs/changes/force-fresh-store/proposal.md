# Proposal: force-fresh-store

## Intent

`c10r build --force` builds even when the stored index is already current.
It does not start from an empty store.
It opens the existing store file, deletes every derived row, and writes the new rows into the same tables.

A user runs `--force` when they distrust the index.
Three things survive a forced build today that such a user expects to be gone:

- The store's table layout.
  The tables are created only when a store is first created.
  A store written by a development build at the same schema version but with a different layout keeps that layout, and the forced build fails when it writes a column the store lacks.
- The store's size.
  SQLite does not shrink a file when rows are deleted, so a store stays at its largest size until it is recreated.
- Anything else wrong with the file that the build does not rewrite.

Every build already recomputes every row, so a forced build that starts from an empty file loses no data and costs no extra time.
Starting over must not cost the user their index when the forced build fails: today a failed build leaves the prior index intact, and a forced build must keep that guarantee.

Two related gaps exist.
A forced build is the one build a person should choose deliberately.
The MCP `build` tool already asks the user to confirm every build, but on the command line any caller, including an agent in a shell, can run `--force` without asking.
And a store path that is a symbolic link needs a defined outcome when the store is replaced.

## User Stories

### Story: force-starts-over

As a developer who distrusts the index, I want `build --force` to build the index from an empty store, so that nothing from the old store, its layout, its leftovers, or its size, survives the rebuild.

> Ladders to north-star outcome 5 (**Calibrated trust**): when a user doubts the index, the remedy must produce an index whose only source is the current build.

### Story: a-failed-force-keeps-the-index

As a developer or agent, I want a forced build that fails to leave my previous index in place and readable, so that starting over never leaves me with no index at all.

> Ladders to north-star outcome 4 (**Honest under edit**) and to the principle **Calibration over coverage**: a failed rebuild must not turn a usable, labeled index into none.

### Story: a-forced-build-is-a-choice

As a developer, I want every forced build to require my confirmation, so that an agent cannot discard and rebuild my index on its own judgment.

> Ladders to north-star outcome 5 (**Calibrated trust**): distrusting the index is a judgment a person makes, and the agent asks the person instead of acting.

## Scope

**In scope:**

- code-graph: a forced build writes a complete new store, with every table created fresh, beside the existing store, and replaces the existing store with it only when the build succeeds.
  A forced build that fails, or is stopped, leaves the existing store unchanged and readable.
- code-graph: before it analyzes anything, a forced build over an existing store checks that there is free space for a second copy of the store plus a margin, and fails early, naming the space required and available, when there is not.
- code-graph: when the store path is a symbolic link, a forced build replaces the store the link names and keeps the link, and the build report names the replaced store.
- code-graph: a build removes leftover temporary stores that an earlier forced build left behind when it was stopped, but only files that the build recognizes as its own temporary stores and whose build is no longer running, and the build report names each file it removed.
- command-surface: every forced build requires confirmation: an interactive prompt when the build runs at a terminal, or an explicit confirmation flag (`--yes`).
  A forced build with no terminal and no confirmation flag is refused with a usage error that names the flag.
- mcp-surface: the MCP `build` tool keeps `force`.
  The confirmation it already asks the user for states, for a forced build, that the build replaces the index, and a confirmed forced build runs with the command line's confirmation flag.
- Documentation: `docs/commands/build.md`, the `build` help text, and the MCP `build` tool description.

**Out of scope:**

- Non-forced builds.
  They keep rebuilding in place inside one transaction, and keep skipping work when the index is current.
- Detecting a same-version layout mismatch without `--force`.
  The schema version remains the signal for an automatic replacement.
- The `Index reset` command and its handling of a symbolic link, which removes the link and keeps the store it names.
- Any confirmation for a non-forced build.

## Approach

The store module already creates an empty store in a temporary file beside the final path and renames it into place.
A forced build extends that: it creates the temporary store, runs the whole build into it, and renames it over the existing store only after the build commits.
On a build error, it removes the temporary store before it returns.

The final path is resolved through a symbolic link before the temporary path is derived, so the temporary store and the rename land beside the store the link names.
The rename replaces a file in the same directory, so it is atomic on the platforms c10r supports.
A query already reading the old store keeps reading it through its open file handle.

The temporary store's name carries the build's process ID.
Before a build starts, it lists files beside the store that match the temporary-store pattern, keeps those whose process ID is no longer running, checks each with the store ownership guard, and removes the ones the guard recognizes.

The confirmation is a command-layer check that runs before any work.
The prompt follows the build prompt that `retained-utility` proposes: it asks only when standard input and standard error are both attached to a terminal, a refresh hook never asks, and an empty answer or the end of input counts as no. The flag is `--yes`, accepted only together with `--force`.
Every forced build names, in its report, the store it replaced.

The MCP server runs the command line without a terminal, so after the user confirms a forced build in the existing dialog, the server passes `--yes`.
The `build` tool's parameters do not change.
Adding `--yes` changes the CLI surface, so the command-surface version changes, and the `c10r-mcp` package needs a release for its surface pin.

## Resolved Questions

- 2026-10-20: A forced build through a symbolic link replaces the store the link names, keeps the link, and discloses the replaced store.
  Decided by the user.
- 2026-10-20: Every forced build requires confirmation, at a terminal by a prompt and elsewhere by `--yes`; a forced build with neither is refused.
  Decided by the user.
- 2026-10-20: The MCP `build` tool keeps `force`, because its existing confirmation dialog already asks the user before every build; the dialog states that a forced build replaces the index.
  Decided by the user, reversing an earlier decision to remove `force` once the dialog was found.
- 2026-10-20: A build removes leftover temporary stores from a stopped forced build, only when it recognizes them as its own and their build is no longer running, and reports each removal.
  Decided by the user.
- 2026-10-20: The confirmation flag is `--yes`.
  Decided by the user.
- 2026-10-20: A forced build over an existing store first checks for free space of the store's size plus 10%, and fails before analysis when there is less.
  Decided by the user.
