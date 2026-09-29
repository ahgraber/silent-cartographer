# Design: stale-files-and-line-positions

## Context

The build joins two sources of positions. tree-sitter reports byte offsets.
SCIP reports a line and a column in the position encoding each document declares (UTF-8, UTF-16, or UTF-32).
The build normalizes SCIP positions to byte offsets so the two sources compare in one exact unit, because a column-unit mismatch would silently bind an occurrence to the wrong node.
Byte offsets then became the output unit without a recorded decision.

The store holds each symbol's source text and the aggregate content hash of all analyzed files, but no per-file record and no copy of whole files.
The build is the only production code that writes the aggregate hash (`src/graph/mod.rs`, the index metadata write).
`status` is the only reader that needs per-file facts.

Three store tables hold locations: `symbols` (definition spans), `occurrences` (reference and definition name spans), and `join_discrepancies` (refused occurrences, whose span may be absent).
`impact` computes one more kind of location at query time: a changed region it could not map, derived from the pre-change text that git returns.

Source text is read as UTF-8; a file that is not valid UTF-8 is skipped by discovery.
So the text the build indexes is the file's bytes.

## Decisions

### Decision: PerFileFingerprints

**Chosen:** A new table `source_files(document_path TEXT PRIMARY KEY, content_hash TEXT NOT NULL)` holds one SHA-256 of each analyzed file's text.
The build writes it in the same transaction as the index metadata, from the same source list the aggregate hash reads.
The aggregate hash stays the freshness gate.
`status` hashes the files discovery finds now, with the recorded language, and compares the two sets by path: both present with different hashes is `changed`, only on disk is `added`, only in the store is `removed`.

**Rationale:** Both hashes derive from one source list in one transaction, so the file list is empty exactly when the aggregate hashes match; the spec's "stale for changed content exactly when at least one file differs" follows.
Keeping the aggregate hash leaves every freshness caller and the continuation-token identity unchanged.

**Alternatives considered:**

- Derive the aggregate hash from the per-file table: removes a redundancy, but changes the freshness gate every query uses for no user-visible gain.
- Store modification times: cheaper to compare, but a touched file with the same content would read as changed, and a checkout can preserve a changed file's mtime.

### Decision: BuildTimeLineColumn

**Chosen:** The build computes the start and end line and column of every location while it holds the file text, and stores four integer columns (`start_line`, `start_column`, `end_line`, `end_column`) beside the byte offsets in `symbols`, `occurrences`, and `join_discrepancies`.
The columns are nullable exactly where the byte offsets are.
The conversion is a new byte-offset-to-line-and-column converter that follows the editor convention below.
The existing `LineIndex` in `src/graph/range.rs` stays unchanged: it counts LF only, and it converts externally numbered lines into bytes for SCIP normalization and for `impact`'s git hunk mapping (`src/query/diff.rs`), whose line numbers follow that convention.
Byte offsets stay the internal unit; only the reported line and column use the editor convention.

**Rationale:** Each line and column comes from the same text as its byte offsets, so the two forms cannot disagree, and a stale answer stays self-consistent.
The store keeps no copy of the source.
Changing `LineIndex` itself would change guarded-join normalization and hunk mapping for files with a lone CR.

**Alternatives considered:**

- Compute at query time from the file on disk: once a file changes, lines disagree with byte offsets and point at the wrong code.
- Store each file's full text and compute at query time: the store grows by about the source size.
  Deferred, not rejected.
  Switching later costs a schema bump and a rebuild and no contract change, because the spec defines positions by the indexed text, not by how the store keeps them.
  Switching back after features come to depend on stored text would cost those features.

### Decision: LineAndColumnUnits

**Chosen:** Lines are numbered from 1 and end at LF, CRLF, or a lone CR.
Columns are numbered from 1 and count Unicode scalar values from the start of the line.
The end position is the position after the last character.
A byte offset between the CR and the LF of a CRLF maps to the position of the CR, the end of that line, since LSP makes positions inside a line ending unaddressable.

**Rationale:** LF, CRLF, and lone CR are the line endings LSP and common editors recognize, so c10r's line numbers match what an editor shows, and a `path:line` copied from an editor round-trips through `--at`.
In a file with a lone CR, c10r's reported lines differ from git's hunk lines; the reported lines follow the editor because the editor is where the reader follows them.
Character columns match editors for every character in the Basic Multilingual Plane.
An exclusive end matches the half-open byte span.

**Alternatives considered:**

- UTF-16 columns (the LSP default): exact for LSP clients, but a column no human counts, and still wrong for editors that count characters.
- Byte columns: need no text to compute, but land mid-character in an editor on any line with a multi-byte character before the location.
- LF-only lines, matching git and `LineIndex`: one convention in the code, but a `path:line` copied from an editor names the wrong line in a lone-CR file.

### Decision: LocationJsonShape

**Chosen:** Every location object in a machine answer gains four flat fields beside `span_start` and `span_end`: `start_line`, `start_column`, `end_line`, `end_column`.
This covers the result `Location`, the `impact` unmappable region, and the `status` discrepancy row and exemplar.
Where the byte offsets are `null`, the four fields are `null`.

**Rationale:** Flat fields match the existing flat `span_start`/`span_end`, and adding fields breaks no existing JSON consumer.

**Alternatives considered:**

- Nested `start: {line, column}` objects: tidier, but a second shape beside the flat byte fields in the same object.
- Fields present only under `--position line`: smaller answers, but the machine answer would depend on a rendering flag, and every MCP tool would need the flag.

### Decision: PositionFlag

**Chosen:** A global flag `--position line|byte`, default `line`, beside `--json` and `--color`.
It selects the human location format and how `--at` reads a position.
It does not change the machine answer.
The continuation-token identity (`PageIdentity`) gains the position unit only when the query reads an `--at` position; every other query's token is the same under either unit.
The MCP tools gain no parameter; their `at` descriptions name the line forms.

**Rationale:** A global flag reaches every command that prints a location, `status` included, without a per-command copy.
The unit must be bound for an `--at` query because `--at path:120` names a different position under each unit, so the same reference string would resume a different result set.
It must not be bound elsewhere, because the token is part of the machine answer, and the machine answer must not depend on the unit.
MCP answers are machine answers, which carry both units already.

**Alternatives considered:**

- A per-command flag: repeats one definition on seven commands.
- Leave the unit out of the token: resumes against the wrong subject when `--at` is used.
- Bind the unit for every query: a truncated `trace --json` answer would carry a different cursor under each unit.

### Decision: HumanStartOnly

**Chosen:** Under `--position line`, the human rendering names a location as `path:start_line:start_column`.
Under `--position byte`, it keeps `path:span_start-span_end`.

**Rationale:** `path:line:column` is the form editors and terminals open as a link; a range suffix breaks some link detectors.
The machine answer carries the whole range.

**Alternatives considered:**

- `path:12:5-14:2`: shows the end, but is not a link in common terminals.

### Decision: PositionParsing

**Chosen:** Under `--position line`, a position whose last two `:`-separated components are both integers is `path:line:column`; one trailing integer is `path:line`.
Under `--position byte`, the last component is a byte offset, as today.
A zero line or column is a usage error naming the accepted forms.

**Rationale:** Splitting from the right keeps paths that contain `:` usable.

**Alternatives considered:**

- Read the unit from the number of components (`path:N` always bytes): keeps old callers working, but a pasted `path:line` link would silently name a byte offset.

### Decision: PositionResolution

**Chosen:** A `path:line:column` position resolves by comparing `(line, column)` pairs against each symbol's stored start and end pairs, with the same enclosure rule the byte offset uses.
A `path:line` position selects the symbols in the document whose half-open span intersects `[(line, 1), (line + 1, 1))`, which excludes a span that ends at the line's column 1, then keeps those that contain no other selected symbol; one survivor is the answer, several are the typed candidate set.
The document's module is the answer when no narrower symbol covers the line; an empty document's zero-width module covers no line, so its answer is typed absence.
A column past the end of its line needs no special handling: `(L, oversized)` sorts after every stored position on line `L` and before `(L + 1, 1)`, so it selects the symbol that encloses the end of that line, which is the clamp LSP specifies and `position_to_byte` applies to SCIP positions.
A line past the end of the file sorts after every stored end, so no symbol encloses it.

**Rationale:** Within one text, `(line, column)` order and byte order agree on every position both can name, so the pair comparison returns the same symbol a byte offset at that position returns.
Neither rule needs the file text, which keeps the per-file storage decision reversible.

**Alternatives considered:**

- Resolve `path:line` at the first non-blank character: needs the file text.
- Resolve `path:line` at column 1: an indented method header resolves to the enclosing type.

### Decision: StatusFileList

**Chosen:** When content differs, the `status` report carries `changed_files: {changed, added, removed}` counts.
With `--detail files`, it also carries `files: [{document_path, change}]`, sorted by path, and a `page` block in the query answers' shape.
`status` gains `--limit` (default 25, `0` unbounded) and `--cursor`.
The token identity binds the command, the detail, the limit, the recorded index identity, and the aggregate hash of the source files on disk, so a token is refused when the source files' paths or content differ from the page it came from.
An edit that is later reverted leaves the hash, and so the list, unchanged, and the token stays valid.
One discovery pass, with the recorded language backend, feeds the freshness, the counts, the page slice, and the token hash, so one report describes one observation of the files.
The human rendering prints one `changed: path`, `added: path`, or `removed: path` line per file, sanitized, followed by the truncation disclosure and the next cursor.

The slice-and-token core of `src/query/page.rs` moves into one function over a `Vec`, which the two existing answer paths and `status` call.

**Rationale:** The user chose the paging used elsewhere in c10r.
The existing paging functions take a query `Answer`, which `status` does not produce, so the core is extracted rather than copied.
Binding the on-disk aggregate hash is what makes a later edit refuse the token.

**Alternatives considered:**

- The `--discrepancies`/`--all` summary-or-everything pattern: `status`'s other listing uses it, but the user chose query paging.
- Copy the paging functions for `status`: a near-duplicate of shared token logic.

### Decision: DetailFlagForm

**Chosen:** `status --detail` takes an optional value with one member, `files`; a bare `--detail` means `files`.

**Rationale:** The value form matches `--detail` on `get`, `trace`, and `similar` and leaves room for other kinds.
The bare form is the user's choice; the inconsistency with other commands, where `--detail` requires a value, is accepted.

### Decision: StaleReasons

**Chosen:** The `status` report carries `stale_reasons`, an array of the freshness values that apply (`stale_content`, `stale_version`, `stale_environment`), in that order, empty when fresh.
`freshness` keeps its current value, the first applicable reason in the same order.
Query answers are unchanged.
A store function returns every applicable reason; the existing single-value function is its first element.

**Rationale:** Reusing the freshness vocabulary keeps one term per concept, and keeping `freshness` breaks no consumer.
Deriving the single value from the list keeps the two from disagreeing.

### Decision: Versions

**Chosen:** `SCHEMA_VERSION` 16 → 17 for `source_files` and the location columns.
`SURFACE_VERSION` 11 → 12 for `--position`, `status --detail`, `status --limit`, and `status --cursor`; the MCP pin moves to 12.
The migration is the existing replace-or-refuse rule: a build replaces a v16 store, and a query refuses it with the rebuild command.
The changelog records two breaking changes: `--at` reads lines by default, and the human location format is `path:line:column`.

**Rationale:** The index is derived data, so rebuild is the migration, as for every earlier schema change.

### Decision: ContentWindowWording

**Chosen:** The `get` content window keeps its numbering.
Its disclosure text names the lines as lines of the symbol's text, for example `lines 1-20 of 40 in this symbol`.

**Rationale:** With file line numbers now in every location, an unlabeled `lines 1-20 of 40` reads as file lines.

## Architecture

```text
build ──reads──> source text (per file)
  │                 │
  │                 ├─ SHA-256 per file ──────────> source_files
  │                 ├─ aggregate hash ────────────> index_metadata.content_hash
  │                 └─ line-and-column converter (byte → editor line, column)
  │                         │
  └─ join ──> symbols / occurrences / join_discrepancies
                (span_start, span_end, start_line, start_column, end_line, end_column)

query ──> Location {span_*, start_*, end_*} ──> JSON (both units)
                                           └──> human: --position line → path:L:C
                                                       --position byte → path:S-E
--at path:L[:C] ──> (line, column) comparison against stored pairs ──> symbol

impact unmappable region ──> converter over git's pre-change text ──> both units (null when unreadable)

status ──> discovery now ──> SHA-256 per file ──compare──> source_files
          ├─ changed_files counts (when content differs)
          ├─ --detail files → files[] ──> shared page core (--limit / --cursor)
          └─ stale_reasons[] (all applicable) ; freshness = first
```

## Risks

- **A future location-bearing row without line and column.**
  Any new code that stores a location must fill the four columns.
  Mitigation: a build-level test compares every stored location's byte offsets and line and column against the indexed text, over the Rust and Python fixtures.
- **A path ending in `:<digits>`.**
  Under the line unit, `dir/a:12:5` reads as `dir/a`, line 12, column 5.
  Mitigation: document it; such file names are rare in source trees.
- **Old `--at` callers pass byte offsets.**
  They now name a line, and may resolve a different symbol without an error.
  Mitigation: the breaking change is in the changelog and the command docs; the MCP `at` descriptions name the new form.
- **Build cost.**
  One extra pass per file to index line starts, and one SHA-256 per file.
  Mitigation: the build already reads every file and hashes all of them; the five-repo timing check from earlier changes confirms no material slowdown.
- **Store size.**
  Four integers per symbol, occurrence, and discrepancy row, plus one row per file.
  Mitigation: measure on the largest dogfood store and record the growth.
- **Answer size for agents.**
  Four fields per location add an estimated 20 tokens each, about 500 tokens on a 25-row `trace`.
  All four stay: the end line is what an agent needs to read a whole definition by line range, and `get --detail body` does not cover reference, `trace`, `search`, or `impact` rows.
  Mitigation: measure representative `trace`, `find`, and `search` answers before and after with one tokenizer, and record the increase here.
- **The unreadable pre-change file region in `impact`.**
  It keeps the placeholder byte span `0-0`, which no text was read to derive, so its line and column are null.
- **Eval comparisons across versions.**
  The eval harness runs `c10r` through Bash, so the treatment agent sees the new output, and its token totals change.
  No eval parser reads locations.
  Paired runs under one binary stay comparable; comparing runs across c10r versions needs the binary's image digest in the analysis record, or separate sweeps.
- **Lone-CR files.**
  Reported lines follow the editor, while git's hunk lines and SCIP's lines count LF only.
  Mitigation: a test pins that SCIP normalization and hunk mapping keep their current byte results, and a scenario covers a definition after a lone CR.
