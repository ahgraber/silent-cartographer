# Tasks: stale-files-and-line-positions

## Line and column converter

- [x] Add a byte-offset-to-line-and-column converter in [src/graph/range.rs](../../../src/graph/range.rs), separate from `LineIndex`, per design.md § LineAndColumnUnits: lines from 1 ending at LF, CRLF, or lone CR; columns from 1 counting Unicode scalar values; a byte offset between the CR and LF of a CRLF maps to the CR's position.
  Leave `LineIndex` and `position_to_byte` unchanged.
- [x] Unit tests for the converter: ASCII line; a multi-byte character (`é`) before the offset; a character outside the Basic Multilingual Plane before the offset; LF, CRLF, and lone-CR files with an offset on line 5; an offset between CR and LF; offset 0; an offset at end of file; an exclusive end after a closing brace.
- [x] Regression test: for a fixture with a lone CR and one with CRLF, SCIP range normalization and `impact`'s hunk-to-byte mapping return the byte offsets they return before this change.

## Store schema

- [x] In [src/graph/schema.rs](../../../src/graph/schema.rs), add the `source_files(document_path, content_hash)` table and the four nullable columns `start_line`, `start_column`, `end_line`, `end_column` to `symbols`, `occurrences`, and `join_discrepancies`; keep `SCHEMA_VERSION` at 17, which the Python kind and module display-name fix already set, and extend its migration note (design.md § Versions).
- [x] Add the line and column fields to `SymbolRow`, `OccurrenceRow`, and `DiscrepancyRow` in [src/graph/store.rs](../../../src/graph/store.rs), and write and read them in `insert_symbol`, the occurrence insert, the discrepancy insert, and each row mapper.
- [x] Add store functions to write the per-file fingerprints in the build's transaction and to read them all.
- [x] Update the store tests that stamp or compare `SCHEMA_VERSION` only where they assert the literal version.
- [x] Add the nullable `header_end`, `header_end_line`, and `header_end_column` columns to `symbols`, extend the version 17 migration note, and write and read them through `SymbolRow`, `insert_symbol`, and the symbol row mapper (design.md § HeaderEnd).
- [x] The aggregate content hash length-prefixes each path and text instead of separating them with NUL bytes, so a file whose text contains a NUL cannot make one file set hash like another; the version 17 migration note records it.

## Build writes

- [x] In the build (`src/graph/mod.rs`, ingest), compute each symbol's, occurrence's, and discrepancy's line and column with the converter over the same source text that produced its byte offsets; leave them null where the byte span is null.
- [x] In the same write as the index metadata, write one `source_files` row per analyzed file with the SHA-256 of its text, from the source list the aggregate hash reads.
- [x] Test (write-site: definitions): after building a Rust fixture and a Python fixture, every stored symbol's line and column, converted back through the fixture text, equals its byte offsets.
- [x] Test (write-site: reference occurrences): the same check over every stored occurrence, including one preceded on its line by `é` and one preceded by an emoji.
- [x] Test (write-site: discrepancies): a refused occurrence with normalizable coordinates stores line and column; one without stores neither.
- [x] Test: a fixture with LF, CRLF, and lone-CR files records a definition on line 5 of each at line 5; a location ending between CR and LF is recorded at the CR's position.
- [x] Test: a definition span `fn target() {}` on line 3 records start `3:1` and end `3:15`.
- [x] Test: the `source_files` rows equal the analyzed file list, and the whole-index hash still matches the aggregate over the same files.
- [x] In the build, record each symbol's header end: the offset where the syntax tree's signature tier is cut at the body delimiter (exposed from `declaration_tiers`), the span end for a declaration without a distinct body or a symbol stored with its name span, and null for a document's own module; convert it with the same converter.
- [x] Tests, on every supported language: a reference preceded on its line by `é` and one preceded by an emoji store start columns that count each character once, and a refused occurrence with normalizable coordinates stores line and column while one without stores neither.
- [x] Tests, on every supported language: a function's header end is where its body begins; a decorated definition's header and a multi-line parameter list's header cover every line of the declaration form; a constant, a field, and a parameter have a header end equal to their span end; a document's own module has no header end and an inline module has one; every stored header end's line and column denote its byte offset in the fixture text.
- [x] Test, on every supported language: a symbol with no definition span, such as an external symbol, has no header end.
- [x] Test: a declaration whose syntax node cannot be found again gets its span's end as its header end.
- [x] Test, on every supported language: a type declaration's header ends at the delimiter that opens its body (a Rust struct before its `{`, a Python class after its `:`), at the expected byte offset, line, and column.

## Freshness reasons

- [x] Add a store function returning every applicable freshness reason in the order content, version, environment, and make `freshness_of` return its first element (design.md § StaleReasons).
- [x] Unit tests: content and version changed yields both; only environment changed yields only environment; fresh yields none; the single value equals the first reason in each case.

## Location in answers

- [x] Add `start_line`, `start_column`, `end_line`, `end_column` to `Location` in [src/query/output.rs](../../../src/query/output.rs) and fill them from the stored columns at every `Location` construction in [src/query/mod.rs](../../../src/query/mod.rs) and [src/query/search.rs](../../../src/query/search.rs).
- [x] Add the four fields to `UnmappableRegion` in [src/query/impact.rs](../../../src/query/impact.rs): compute them with the converter over the pre-change text; set them null for the unreadable-file region.
- [x] Add the four fields to the `status` discrepancy rows and exemplars in `run_status`.
- [x] Test: `find --json` returns a location whose line and column and byte offsets denote the same position.
- [x] Test: each of `get`, `trace`, `search`, `similar`, and `impact` JSON results carries the four fields (one assertion per command in one test over one fixture).
- [x] Test: `impact --json` on a change to a file the index does not hold reports the region's line and column in the pre-change text; on an unreadable pre-change file the four fields are null.
- [x] Test: the unmappable-region line and column in the pre-change text also holds for a Python repository.
- [x] Test: `status --discrepancies --all --json` rows carry line and column.
- [x] Test: after inserting lines above an indexed definition, a query answer is stale and reports the indexed line, not the edited one.
- [x] Tests, on every supported language: `find` locations, the locations in `get`, `trace`, `search`, `similar`, and `impact` answers, and a stale answer's reported line carry line and column that agree with the byte offsets.
- [x] Test, on every supported language: the rows of the `trace` relations `contains`, `containers`, `importers`, and `implementers` carry line and column that agree with their byte offsets.
- [x] Test, on every supported language: every seed, every dependent row, and every unmappable region of an `impact` answer carries a location with all four line and column fields, and a row without one fails the test.
- [x] Test, on every supported language: `status --discrepancies --all` rows and summary exemplars carry line and column, and the human row renders them as `path:line:column`.

## Position flag and human rendering

- [x] Add the global `--position line|byte` flag (default `line`) to [src/cli.rs](../../../src/cli.rs) beside `--json` and `--color`, and thread the unit to the human renderers.
- [x] In [src/render.rs](../../../src/render.rs), render a location as `path:start_line:start_column` under `line` and as `path:span_start-span_end` under `byte` (design.md § HumanStartOnly); do the same for `status_location` in [src/commands.rs](../../../src/commands.rs).
- [x] Change the `get` content-window disclosure text to name the lines as lines of the symbol's text (design.md § ContentWindowWording).
- [x] Test: `get` at location detail renders `path:12:5` by default and byte offsets with `--position byte`.
- [x] Test, on every supported language: `find`, `trace`, `search`, `similar`, and `impact` render a location as `path:line:column` by default, and `find` renders the byte span under `--position byte`.
- [x] Test: a truncated `trace --json` answer, including its cursor, is identical under both units.
- [x] Test, on every supported language: `find`, `trace`, `search`, `similar`, and `impact` each render a location as `path:start-end` under `--position byte`.
- [x] Test, on every supported language: the `--json` answer of `get`, `trace`, `find`, `search`, `similar`, and `impact` is the same under `--position line` and `--position byte`.
- [x] Tests: under the default line unit, an `impact` unmappable region renders as `path:line:column` and under `--position byte` as `path:start-end`; a `status --discrepancies --all` row renders as `path:line:column`.
- [x] Update existing tests that assert the byte location format in human output or pass `--at path:byte_offset`: add `--position byte` where the test is about bytes, or assert the new format where it is about the default; record each changed test in the task note.
  Changed tests:
  - `clap_tree_matches_the_recorded_flag_vocabulary` (tests/command_surface.rs): `position` added to the recorded global flag set.
  - `malformed_at_position_is_a_usage_error_naming_the_expected_form` (tests/command_surface.rs): `--position byte` added; the test is about `path:byte_offset`.
  - `the_human_status_report_carries_every_value_of_the_machine_report` (tests/code_navigation.rs): the human report is rendered under `--position byte`, the unit that names both ends of a span, since the line unit names only the start.
  - `trace_contains_symbol_rows_carry_definition_location` (tests/code_navigation.rs): rendered under the byte unit; the test is about the byte span of a row stored without a line and column.
  - `every_query_identity_field_affects_the_hash` (src/query/page.rs): the new `position_unit` field joins the identity literal and the variation list.
  - Every caller of `run_get`, `run_trace`, `run_find`, `run_search`, `run_similar`, `run_impact`, `run_status`, and `to_human` passes the line unit, and every caller of `get_by_position` and `similar_by_position` wraps its byte offset in `SourcePosition::Byte`; no assertion changed.

## Source positions

- [x] Replace `parse_position` in [src/commands.rs](../../../src/commands.rs) with a parser for the selected unit per design.md § PositionParsing, refusing a zero line or column with a usage error that names the accepted forms.
- [x] Add store lookups for `(line, column)` enclosure and for half-open line intersection with the no-nested-survivor rule (design.md § PositionResolution), returning one symbol, a candidate set, or absence.
- [x] Replace the line lookup with the header rule (design.md § PositionResolution): select symbols whose header intersects the line, keep those no other selected symbol contains, and when none is selected resolve `(line, 1)` through the `(line, column)` enclosure lookup.
- [x] Wire both into `get --at` and `similar --at`, and bind the position unit into `PageIdentity` only when `--at` is read.
- [x] Tests: `path:line:column` inside a method body returns the method; `path:line` on a method's header line returns the method; a body line returns the function; two sibling declarations on one line return the candidate set; a span ending at the line's column 1 is not selected; `similar --at path:line` on a Python method takes the method.
- [x] Tests for the header rule, each scenario on every supported language: a method header line that declares parameters returns the method; every line of a decorated header and of a multi-line parameter list returns the declaration; a one-line type with members returns the type; a line declaring one member of a multi-line type returns the member; a body line returns the function; two sibling declarations on one line return the candidate set; a span ending at the line's column 1 is not selected.
  Rework the existing line-position tests above to the header rule and give each a case per supported language.
- [x] Tests: an oversized column on a middle line of a multi-line function returns the function, and on a line where one declaration ends and another starts returns the symbol enclosing the line end; a line past end of file is typed absence; an empty document is typed absence.
- [x] Test: the span-ending-at-column-1 scenario also runs over a Python document, with rows stored directly because a Python header ends after the colon that closes its line.
- [x] Tests: `--position byte` with `path:byte_offset` returns the method; `get --at path:0` fails with the usage error; a stale position reads the indexed text and the answer is stale.
- [x] Test: a cursor from a paged `--at` query under one unit is refused under the other.
- [x] Tests, on every supported language: a non-zero `path:line:column` given to `get --at` inside a method body returns the method; the oversized column, line past end of file, empty document, byte unit, and stale position scenarios hold (the line where one declaration ends and another starts is Rust only, because a Python declaration always begins on its own line).
- [x] Test, on every supported language: `similar --at path:line`, parsed from the command line, takes the method declared on that line and returns the answer `similar` gives for the method by name.
- [x] Test: a zero line or column in `similar --at` is a usage error naming the accepted forms, as it is for `get --at`.
- [x] Test, on every supported language: a continuation token bound to a `get --at` query under the line unit is accepted under that unit and refused under the byte unit; a `get --at` answer never spans pages, so the token is minted over the same query identity rather than read from an answer.

## Paging core

- [x] Extract the slice-and-token core of [src/query/page.rs](../../../src/query/page.rs) into one function over a `Vec`, and make `apply_pagination` and `apply_report_pagination` call it (design.md § StatusFileList).
- [x] Run the existing paging tests unchanged to confirm the extraction changes no answer.

## Status file list

- [x] Add `--detail[=files]` (bare `--detail` means `files`), `--limit` (default 25), and `--cursor` to `StatusArgs` (design.md § DetailFlagForm).
- [x] In `run_status`, run one recorded-backend discovery pass that feeds freshness, `stale_reasons`, the `changed_files` counts, and the file list; classify each path as changed, added, or removed against `source_files`.
- [x] Emit `stale_reasons` always and `changed_files` when content differs; with the file detail, emit `files` sorted by path, paged through the shared core with a token identity that binds the command, detail, limit, recorded index identity, and on-disk aggregate hash.
- [x] Render the reasons, counts, and one `changed:`/`added:`/`removed:` line per file, sanitized, with the truncation disclosure and next cursor, in the human `status` output.
- [x] Tests: an edited file is named changed; a new file added; a deleted file removed; each with stale content reported.
- [x] Tests: counts without `--detail` (two changed, one added, none removed, no file named); bare `--detail` equals `--detail files`; a fresh index names no file; an edited README is not named and the index stays fresh.
- [x] Test: the human rendering names an edited file as changed and a deleted file as removed.
- [x] Test, on every supported language: the human `status` output prints the `changed files:` count line when content differs.
- [x] Tests: a list longer than `--limit 2` truncates and discloses, and its cursor returns the next files; after another file's content changes, that cursor is refused.
- [x] Test, on every supported language: editing the content of a file already in the list, with no file added or removed, makes the earlier cursor refused.
- [x] Tests: `status` names both content and analyzer reasons when both changed, only the environment when only it changed, and none when fresh, in JSON and human output.
- [x] `status` reads every stored value its report uses (metadata, source fingerprints, duplicated groups, index identity, semantic-index identity, discrepancies) inside one read snapshot taken before discovery, so a build committing during the analyzer probe cannot mix its values into the report.

## End-of-build check

- [x] In `run_build` ([src/commands.rs](../../../src/commands.rs)), after the store write, run discovery again with the same language and compare it with the indexed source list through the `status` comparison; carry the result in `BuildOutcome` (design.md § BuildEndRecheck).
- [x] Add `changed_during_build` to `build_outcome_json`, always present, and one `<change> during build: path` line per file to `build_outcome_line`, sanitized.
- [x] Tests, using a stub analyzer that edits the fixture while it runs: an edited file is named changed and `status` then reports stale content; a created file is named added; a deleted file is named removed; the build exits with success in each case.
- [x] Tests: an undisturbed build carries an empty list and prints no file line; an already-current build carries an empty list.
- [x] Test, on every supported language: a source file that becomes unreadable while the analyzer runs fails the build with an error naming the file, and the store holds the new build's metadata and file fingerprints.
- [x] Discovery treats a file or directory that vanishes while it is read, including a dangling symlink, as absent, so the second pass after the store write succeeds and names a vanished file as removed; every other read error still fails.
- [x] Test: a subdirectory that vanished before discovery lists it yields no sources and no error, while a missing workspace root still fails.

## Surfaces and versions

- [x] Bump `SURFACE_VERSION` to 12 in [src/manifest.rs](../../../src/manifest.rs) and confirm the surface index lists `--position` and the new `status` flags with their values and defaults.
- [x] In [mcp/src/c10r_mcp/surface.py](../../../mcp/src/c10r_mcp/surface.py), move the pin to 12; in [mcp/src/c10r_mcp/server.py](../../../mcp/src/c10r_mcp/server.py), change the `get` and `similar` `at` descriptions to the `path:line:column` and `path:line` forms.
- [x] Run the MCP test suite, including the schema-against-surface tests.

## Documentation

- [x] Update [docs/commands/get.md](../../../docs/commands/get.md) and [docs/commands/similar.md](../../../docs/commands/similar.md) for the `--at` forms; add `--position` to [docs/commands/common.md](../../../docs/commands/common.md) with the line and column definition and the `:<digits>` path caveat.
- [x] Update [docs/commands/build.md](../../../docs/commands/build.md) for `changed_during_build` and the human lines.
- [x] Update [docs/commands/status.md](../../../docs/commands/status.md) for `stale_reasons`, `changed_files`, `--detail files`, `--limit`, and `--cursor`, with a human output example.
- [x] Update location examples in the other command docs and the README from `path:start-end` to `path:line:column`, and document the four JSON fields.

## Measurements

- [x] Measure store size and build time on the dogfood clones under `~/.cache/silent-cartographer` before and after, and record the results in design.md § Risks.
- [x] Measure the token count of representative `trace`, `find`, and `search` JSON answers before and after with one tokenizer, and record the increase in design.md § Risks.

## Close-out

- [x] Run the full Rust and MCP test suites and every pre-commit hook on the changed files, and report scope with counts.
