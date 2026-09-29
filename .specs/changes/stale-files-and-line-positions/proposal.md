# Proposal: stale-files-and-line-positions

## Intent

Every location c10r reports is a pair of byte offsets, such as `jobs.py:1249-1264`.
Editors and terminals do not open a byte offset, so a reader cannot follow a location as a `path:line` link.
A reader who sees such a location can also mistake the offsets for line numbers, because nothing labels the unit.
`get --at` and `similar --at` take a byte offset too, so a `path:line` copied from an editor names the wrong position.

`status` reports `stale_content` when a source file changed since the build, but it does not name the file.
A reader cannot tell whether the files they care about changed, so they cannot decide whether a rebuild matters to their question.
`status` also reports only the first staleness reason it finds.
When the content and the analyzer version both changed, it reports only the content change, so a short list of changed files would hide that the whole index is stale.

Both problems need the store to record facts per source file, so both changes share one schema version bump.

## User Stories

### Story: clickable-locations

As a developer or agent, I want every location reported as a line and column, so that I can open it directly in an editor and pass it back to c10r without converting units.

> Ladders to north-star outcome 1 (**Precise locate**): a location a reader can follow is the end of the search, not the start of another one.
> Ladders to the guiding principle **Agent-native, human-rendered**: the human rendering shows the unit people and editors use, and the machine answer carries both units.

### Story: status-names-what-changed

As a developer or agent, I want `status` to name the files that changed since the index was built and every reason the index is stale, so that I can tell whether a rebuild matters to my question before I run one.

> Ladders to north-star outcome 5 (**Calibrated trust**): the staleness label names its cause, so the reader decides how far to lean on the index.
> Ladders to north-star outcome 4 (**Honest under edit**): a partial account of staleness does not pass for a complete one.

## Scope

**In scope** (two capabilities, `code-graph` then `code-navigation`, plus the MCP server's wording and pin):

- The store records, per source file, a content fingerprint of the file as the build read it (`code-graph`).
- The store records a line and column for every location it records, computed from the same text as the location's byte offsets (`code-graph`).
- The schema version bump.
  The existing replace-or-refuse rule is the migration: a build replaces an older store, and a query refuses it with the rebuild command.
- Every location in every answer carries its line and column beside its byte offsets in the machine answer (`code-navigation`).
  This covers `get`, `trace`, `find`, `search`, `similar`, and `impact` results, the regions `impact` could not map, and the discrepancy rows of `status`.
- A position flag (`--position line|byte`) selects the unit of the human rendering and of `--at` (`code-navigation`).
  The default is `line`.
  With `line`, a location renders as `path:line:column`.
- `get --at` and `similar --at` accept `path:line:column` and `path:line` by default, and `path:byte_offset` with `--position byte` (`code-navigation`).
  This breaks callers that pass a byte offset without the flag.
- `status --detail files` (and a bare `--detail`) names each source file that differs from the indexed content as changed, added, or removed (`code-navigation`).
  The list pages with `--limit` and `--cursor`, as query answers do.
- `status` reports every staleness reason that applies, not only the first (`code-navigation`).
- The surface version bump for the new flags.
- The MCP server: the matching surface pin, and `at` parameter descriptions that name the line and column form.
- Command documentation for the location format, `--position`, `--at`, and `status --detail files`.

**Out of scope:**

- Listing changed files in a stale query answer.
  A stale answer keeps its freshness label and refresh command; `status --detail files` names the files.
- Naming the answers a change affects.
  The file list names files whose content changed.
  A change in one file can change answers about symbols in an unchanged file, and this change does not claim otherwise.
- Storing the full text of each source file.
- Columns counted in UTF-16 units or in bytes.
  Columns count Unicode characters.
- A position parameter on the MCP tools.
  MCP answers are machine answers, which carry both units, and MCP `at` takes the default line form.
- Changing the `get` content window (`--from`, `--max-lines`).
  Its lines count within the symbol's text, not within the file; its disclosure text says so.
- Paging for the other `status` listings (`--discrepancies`, `--all`).

## Approach

**Per-file fingerprints.**
The build already reads each source file to compute the whole-index content hash.
It records one fingerprint per file in a new per-file table, in the same write that records the whole-index hash.
`status` computes the same fingerprints for the files on disk and compares the two sets: a path in both with different fingerprints is changed, a path only on disk is added, and a path only in the store is removed.
The whole-index hash stays the freshness gate.
Both derive from the same file list and text, so `status` reports stale content exactly when the file list is not empty.

**Line and column at build time.**
The build computes each location's start and end line and column while it holds the file text, and stores them beside the byte offsets.
The store keeps no copy of the source.
Lines and columns then describe the indexed text, as the byte offsets do, so a stale answer stays self-consistent and stays labeled stale.
Computing them at query time from the file on disk was rejected: once a file changes, the line numbers would disagree with the byte offsets and point at the wrong code.
Storing the full file text was deferred.
Switching to it later costs a schema bump and a rebuild, and no contract change, provided the spec does not depend on how the store keeps positions.

**Position resolution.**
`--at path:line:column` resolves to the innermost symbol that encloses that position, as a byte offset does today.
`--at path:line` resolves to the innermost symbol that covers any part of that line; when several symbols qualify, the answer is the typed candidate set.
Both rules use only stored lines and columns.

**Status reasons.**
`freshness` keeps its current single value for compatibility.
A new list carries every reason that applies.

**Surfaces.**
The continuation token already binds every query parameter, so `--position` joins it automatically.
The surface version rises for `--position`, `status --detail`, and the `status` paging flags; the MCP pin rises with it.

## Open Questions

- Whether the human rendering shows only the start (`path:line:column`) or the whole range.
  The start alone is the form editors open; `design.md` decides.
- The JSON field names and shape for line and column.
  `design.md` decides, keeping the existing `span_start` and `span_end` fields.
