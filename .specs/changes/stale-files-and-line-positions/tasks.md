# Tasks: stale-files-and-line-positions

## Line and column converter

- [ ] Add a byte-offset-to-line-and-column converter in [src/graph/range.rs](../../../src/graph/range.rs), separate from `LineIndex`, per design.md § LineAndColumnUnits: lines from 1 ending at LF, CRLF, or lone CR; columns from 1 counting Unicode scalar values; a byte offset between the CR and LF of a CRLF maps to the CR's position.
  Leave `LineIndex` and `position_to_byte` unchanged.
- [ ] Unit tests for the converter: ASCII line; a multi-byte character (`é`) before the offset; a character outside the Basic Multilingual Plane before the offset; LF, CRLF, and lone-CR files with an offset on line 5; an offset between CR and LF; offset 0; an offset at end of file; an exclusive end after a closing brace.
- [ ] Regression test: for a fixture with a lone CR and one with CRLF, SCIP range normalization and `impact`'s hunk-to-byte mapping return the byte offsets they return before this change.

## Store schema

- [ ] In [src/graph/schema.rs](../../../src/graph/schema.rs), add the `source_files(document_path, content_hash)` table and the four nullable columns `start_line`, `start_column`, `end_line`, `end_column` to `symbols`, `occurrences`, and `join_discrepancies`; bump `SCHEMA_VERSION` to 17 and add the migration note to its doc comment.
- [ ] Add the line and column fields to `SymbolRow`, `OccurrenceRow`, and `DiscrepancyRow` in [src/graph/store.rs](../../../src/graph/store.rs), and write and read them in `insert_symbol`, the occurrence insert, the discrepancy insert, and each row mapper.
- [ ] Add store functions to write the per-file fingerprints in the build's transaction and to read them all.
- [ ] Update the store tests that stamp or compare `SCHEMA_VERSION` only where they assert the literal version.

## Build writes

- [ ] In the build (`src/graph/mod.rs`, ingest), compute each symbol's, occurrence's, and discrepancy's line and column with the converter over the same source text that produced its byte offsets; leave them null where the byte span is null.
- [ ] In the same write as the index metadata, write one `source_files` row per analyzed file with the SHA-256 of its text, from the source list the aggregate hash reads.
- [ ] Test (write-site: definitions): after building a Rust fixture and a Python fixture, every stored symbol's line and column, converted back through the fixture text, equals its byte offsets.
- [ ] Test (write-site: reference occurrences): the same check over every stored occurrence, including one preceded on its line by `é` and one preceded by an emoji.
- [ ] Test (write-site: discrepancies): a refused occurrence with normalizable coordinates stores line and column; one without stores neither.
- [ ] Test: a fixture with LF, CRLF, and lone-CR files records a definition on line 5 of each at line 5; a location ending between CR and LF is recorded at the CR's position.
- [ ] Test: a definition span `fn target() {}` on line 3 records start `3:1` and end `3:15`.
- [ ] Test: the `source_files` rows equal the analyzed file list, and the whole-index hash still matches the aggregate over the same files.

## Freshness reasons

- [ ] Add a store function returning every applicable freshness reason in the order content, version, environment, and make `freshness_of` return its first element (design.md § StaleReasons).
- [ ] Unit tests: content and version changed yields both; only environment changed yields only environment; fresh yields none; the single value equals the first reason in each case.

## Location in answers

- [ ] Add `start_line`, `start_column`, `end_line`, `end_column` to `Location` in [src/query/output.rs](../../../src/query/output.rs) and fill them from the stored columns at every `Location` construction in [src/query/mod.rs](../../../src/query/mod.rs) and [src/query/search.rs](../../../src/query/search.rs).
- [ ] Add the four fields to `UnmappableRegion` in [src/query/impact.rs](../../../src/query/impact.rs): compute them with the converter over the pre-change text; set them null for the unreadable-file region.
- [ ] Add the four fields to the `status` discrepancy rows and exemplars in `run_status`.
- [ ] Test: `find --json` returns a location whose line and column and byte offsets denote the same position.
- [ ] Test: each of `get`, `trace`, `search`, `similar`, and `impact` JSON results carries the four fields (one assertion per command in one test over one fixture).
- [ ] Test: `impact --json` on a change to a file the index does not hold reports the region's line and column in the pre-change text; on an unreadable pre-change file the four fields are null.
- [ ] Test: `status --discrepancies --all --json` rows carry line and column.
- [ ] Test: after inserting lines above an indexed definition, a query answer is stale and reports the indexed line, not the edited one.

## Position flag and human rendering

- [ ] Add the global `--position line|byte` flag (default `line`) to [src/cli.rs](../../../src/cli.rs) beside `--json` and `--color`, and thread the unit to the human renderers.
- [ ] In [src/render.rs](../../../src/render.rs), render a location as `path:start_line:start_column` under `line` and as `path:span_start-span_end` under `byte` (design.md § HumanStartOnly); do the same for `status_location` in [src/commands.rs](../../../src/commands.rs).
- [ ] Change the `get` content-window disclosure text to name the lines as lines of the symbol's text (design.md § ContentWindowWording).
- [ ] Test: `get` at location detail renders `path:12:5` by default and byte offsets with `--position byte`.
- [ ] Test: a truncated `trace --json` answer, including its cursor, is identical under both units.
- [ ] Update existing tests that assert the byte location format in human output or pass `--at path:byte_offset`: add `--position byte` where the test is about bytes, or assert the new format where it is about the default; record each changed test in the task note.

## Source positions

- [ ] Replace `parse_position` in [src/commands.rs](../../../src/commands.rs) with a parser for the selected unit per design.md § PositionParsing, refusing a zero line or column with a usage error that names the accepted forms.
- [ ] Add store lookups for `(line, column)` enclosure and for half-open line intersection with the no-nested-survivor rule (design.md § PositionResolution), returning one symbol, a candidate set, or absence.
- [ ] Wire both into `get --at` and `similar --at`, and bind the position unit into `PageIdentity` only when `--at` is read.
- [ ] Tests: `path:line:column` inside a method body returns the method; `path:line` on a method's header line returns the method; a body line returns the function; two sibling declarations on one line return the candidate set; a span ending at the line's column 1 is not selected; `similar --at path:line` on a Python method takes the method.
- [ ] Tests: an oversized column on a middle line of a multi-line function returns the function, and on a line where one declaration ends and another starts returns the symbol enclosing the line end; a line past end of file is typed absence; an empty document is typed absence.
- [ ] Tests: `--position byte` with `path:byte_offset` returns the method; `get --at path:0` fails with the usage error; a stale position reads the indexed text and the answer is stale.
- [ ] Test: a cursor from a paged `--at` query under one unit is refused under the other.

## Paging core

- [ ] Extract the slice-and-token core of [src/query/page.rs](../../../src/query/page.rs) into one function over a `Vec`, and make `apply_pagination` and `apply_report_pagination` call it (design.md § StatusFileList).
- [ ] Run the existing paging tests unchanged to confirm the extraction changes no answer.

## Status file list

- [ ] Add `--detail[=files]` (bare `--detail` means `files`), `--limit` (default 25), and `--cursor` to `StatusArgs` (design.md § DetailFlagForm).
- [ ] In `run_status`, run one recorded-backend discovery pass that feeds freshness, `stale_reasons`, the `changed_files` counts, and the file list; classify each path as changed, added, or removed against `source_files`.
- [ ] Emit `stale_reasons` always and `changed_files` when content differs; with the file detail, emit `files` sorted by path, paged through the shared core with a token identity that binds the command, detail, limit, recorded index identity, and on-disk aggregate hash.
- [ ] Render the reasons, counts, and one `changed:`/`added:`/`removed:` line per file, sanitized, with the truncation disclosure and next cursor, in the human `status` output.
- [ ] Tests: an edited file is named changed; a new file added; a deleted file removed; each with stale content reported.
- [ ] Tests: counts without `--detail` (two changed, one added, none removed, no file named); bare `--detail` equals `--detail files`; a fresh index names no file; an edited README is not named and the index stays fresh.
- [ ] Test: the human rendering names an edited file as changed and a deleted file as removed.
- [ ] Tests: a list longer than `--limit 2` truncates and discloses, and its cursor returns the next files; after another file's content changes, that cursor is refused.
- [ ] Tests: `status` names both content and analyzer reasons when both changed, only the environment when only it changed, and none when fresh, in JSON and human output.

## Surfaces and versions

- [ ] Bump `SURFACE_VERSION` to 12 in [src/manifest.rs](../../../src/manifest.rs) and confirm the surface index lists `--position` and the new `status` flags with their values and defaults.
- [ ] In [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py), move the pin to 12; in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), change the `get` and `similar` `at` descriptions to the `path:line:column` and `path:line` forms.
- [ ] Run the MCP test suite, including the schema-against-surface tests.

## Documentation

- [ ] Update [docs/commands/get.md](../../../docs/commands/get.md) and [docs/commands/similar.md](../../../docs/commands/similar.md) for the `--at` forms; add `--position` to [docs/commands/common.md](../../../docs/commands/common.md) with the line and column definition and the `:<digits>` path caveat.
- [ ] Update [docs/commands/status.md](../../../docs/commands/status.md) for `stale_reasons`, `changed_files`, `--detail files`, `--limit`, and `--cursor`, with a human output example.
- [ ] Update location examples in the other command docs and the README from `path:start-end` to `path:line:column`, and document the four JSON fields.

## Measurements

- [ ] Measure store size and build time on the dogfood clones under `~/.cache/silent-cartographer` before and after, and record the results in design.md § Risks.
- [ ] Measure the token count of representative `trace`, `find`, and `search` JSON answers before and after with one tokenizer, and record the increase in design.md § Risks.

## Close-out

- [ ] Run the full Rust and MCP test suites and every pre-commit hook on the changed files, and report scope with counts.
