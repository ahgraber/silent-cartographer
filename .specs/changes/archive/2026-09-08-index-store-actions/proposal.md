# Proposal: index-store-actions

## Intent

`c10r cache` deletes the index store.
The command name is a noun, so it reads as a request to show the cache, but running it removes the store.
No other command works this way.
`hooks` is also a noun, and it requires `hooks install` before it writes anything.

Two other questions about the same store have no command at all: where the store is, and how much disk it uses.
Today a user answers them by reconstructing the path from the `--db` default and running `du` on the result.

## User Stories

### Story: deletion-is-named

As a developer or agent, I want the destructive action spelled out in the invocation, so that an incomplete command line never deletes an index I meant to inspect.

> Ladders to the guiding principle **Safety by structure, not by caveat**: the command shape makes the destructive path opt-in.

### Story: locate-and-measure-the-store

As a developer or agent, I want to ask `c10r` where its index store is and how much disk it uses, so that I can decide whether to clear it without reconstructing the path from flag defaults.

> Ladders to **Agent-native, human-rendered**: an agent deciding whether to clear the store reads the location and the size as fields of a structured answer.
> Ladders to north-star outcome 5 (**Calibrated trust**) through the rule for an unowned target: a byte count for a file `c10r` does not own would describe the wrong file, so `size` reports the target's state instead.

## Scope

**In scope** (one capability — `command-surface`):

- `cache` takes a required action.
  Naming the command alone is a usage error that lists the valid actions and removes nothing.
- `cache clear` — the current removal.
  Behavior, ownership refusal, and exit codes are unchanged.
- `cache dir` — the directory holding the store at the `--db` path, reported whether or not a store exists there.
  The machine answer also carries the store path.
- `cache size` — the on-disk bytes of a store `c10r` owns, counting the files `cache clear` deletes.
  For a target `c10r` does not own, the answer carries the target's state and no byte count, and the command succeeds.
- The surface version bump and the matching MCP pin, because the command structure changed.
- Documentation for the three actions, and the existing tests moved onto `cache clear`.

**Out of scope:**

- Exposing `cache dir` or `cache size` as MCP tools.
  Both are read-only, so there is a case for it, but that decision belongs to the MCP tool surface and carries its own tool-schema work.
  The README's claim that no tool on that surface removes an index store stays true either way.
- A second cache.
  The index store at `--db` is the only artifact `c10r` writes; the embedding model is compiled into the binary.
  A second on-disk artifact would add a row to `dir` and `size`, not a command.
- Reporting what the store contains: symbol counts, freshness, or workspace identity.
  `status` and `manifest` report those already.
- A deprecation window in which bare `c10r cache` still clears.
  The surface is pre-1.0 and carries no deprecation obligation.
- Canonicalizing or creating the reported directory.
  `dir` reports the path as `--db` gave it.
  No other command resolves `--db` against the workspace root.

## Approach

`hooks` already has the shape `cache` needs: a required positional action backed by a `ValueEnum`.
The manifest walk and the completion generator both derive it from the clap tree, so `cache` takes the same shape with three values and needs no per-command work in either.

`clear` keeps its current handler.

`dir` computes the parent of the `--db` path.
It renders `.` when the path is a bare filename, and it reads no file, so it answers before a build and for a path that does not exist.

`size` is the action with a trust boundary.
Its total is the bytes `clear` would free: the store file plus the `-wal` and `-shm` sidecars that `clear` removes with it.
The ownership recognizer that every store-touching path calls decides whether the target is `c10r`'s.
It separates a foreign file, an unexaminable path, and a store written under an unreadable schema version, so `size` can report which one it found instead of reporting zero bytes for all three.
An unowned target gets a state and a success exit, because refusing would block a read that changes nothing.
A store written under an unreadable schema version is still `c10r`'s and is still what `clear` removes, so it gets a byte count as well as a state.

## Open Questions

None.
The three surface decisions — the required action, what `dir` prints, and what `size` counts and reports for an unowned target — were settled with the user before this proposal.
