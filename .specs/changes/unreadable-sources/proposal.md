# Proposal: unreadable-sources

> **Placeholder.**
> This proposal records the intent, stories, and scope of the change.
> The delta specs, `design.md`, and `tasks.md` are not written yet.
> Each open question below needs an explicit decision before the change is implemented.

## Intent

Source discovery walks the workspace for every command: `build`, `status`, and the currency check each query performs.
When the walk meets a directory or an entry it cannot read, for example a directory owned by another user, the invocation fails.
One such directory blocks every command until the user removes it or changes its permissions.

The baseline requirement "Source discovery excludes undecodable files" makes this failure the contract for files: a discovered file that cannot be read for any reason other than its encoding SHALL fail the invocation, and the build names the file.
The contract does not cover a directory that cannot be listed, or an entry whose file type cannot be read.
When the file type cannot be read, the error names no path, so the user cannot tell which entry to fix.

## User Stories

### Story: unreadable-entry-is-named

As a developer or agent whose workspace holds a directory or entry c10r cannot read, I want the failure or the exclusion to name that path, so that I can fix the permission or exclude the path without searching for it.

> Ladders to north-star outcome 5 (**Calibrated trust**): a gap in what c10r read is named, never implied.

## Scope

**In scope** (one capability, `code-graph`):

- Every discovery failure on a directory or an entry names its path.
- A decision on whether an unreadable directory or entry fails the invocation or is excluded and disclosed (see Open Questions).
- Tests for the `read_dir` and `file_type` error arms of discovery that are not NotFound, which have no test today.

**Out of scope:**

- Files that are not valid UTF-8.
  `undecodable-sources` already excludes and discloses them.
- Files or directories that vanish during the walk.
  `stale-files-and-line-positions` treats them as absent.
- A configured list of excluded paths.
  `c10r-settings` is the place for persistent project settings.

## Approach

All errors come from one walk, `collect_dir` in `src/commands.rs`, so one change there reaches every caller.
The `read_dir` arm already names the directory in its error context; the `file_type` arm propagates the error without the path.

## Open Questions

- Should an unreadable directory or entry fail the invocation, as an unreadable file does today, or be excluded and disclosed, as an undecodable file is?
  `undecodable-sources` chose to fail on every read error other than encoding, because "those are operational failures that must stay loud".
  Excluding would reverse that decision for directories and entries, and possibly for files too.
- If excluded: the files indexed from an excluded directory disappear from the discovered set.
  `status --detail files` and the end-of-build check would then name them `removed`.
  Do they need a separate classification, such as `unreadable`, so that a deleted file and a file c10r cannot read now stay distinct?
  Adding that value after agents depend on the `change` field is a contract change.
- If excluded: does the currency check treat an index as stale when a directory it once read becomes unreadable?
