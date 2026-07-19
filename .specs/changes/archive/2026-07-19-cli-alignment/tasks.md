# Tasks: cli-alignment

Groups run in build-dependency order: the CLI contract foundation first, then human rendering, then the bounding that answers carry, then the query commands that emit bounded answers, then the operational commands, and finally `manifest`, which indexes the now-settled surface.

## CLI contract foundation

- [x] Define an `Outcome`/exit-code mapping (`0` success, `1` generic failure, `2` usage, `3` no-index, `4` incompatible store, `5` indexer/setup) applied once at the top of `main`, replacing ad-hoc exits.
- [x] Route every answer to stdout and every diagnostic to stderr across the command handlers; ensure `--json` writes the serialized answer to stdout only.
- [x] Establish the closed canonical flag set on the clap definitions (`--json`, `--detail`, `--relation`, `--depth`, `--limit`, `--max-chars`, `--cursor`, `--color`, global `--db`/`--workspace`) with no aliases.
- [x] Ensure enum-valued flags (`--detail`, `--relation`, `--color`) reject an out-of-set value with a diagnostic listing the accepted values, and that argument validation runs before any side effect.
- [x] Confirm non-interactive operation: no command reads interactive input; a non-terminal environment is treated as headless.
- [x] Test: each exit-code category maps correctly — success on a non-empty answer, success on a typed-empty answer, usage on a bad flag, no-index on an unbuilt directory, incompatible-store on a wrong-`user_version` store, indexer/setup on a missing indexer.
- [x] Test: an answer lands on stdout and its accompanying diagnostic lands on stderr.
- [x] Test: walking the built clap `Command` tree yields exactly the recorded flag vocabulary, and known banned aliases (`--format`, `--output`, `--top-k`, `--no-color`) are undefined.
- [x] Test: an unknown `--relation` value is rejected with a message enumerating the valid relations; an invalid argument to a mutating command changes no state.
- [x] Test: a command invoked with stdin/stdout not attached to a terminal runs to completion without prompting.

## Human rendering

- [x] Add a render module that projects each answer type (`Answer<SymbolDetail>`, `Answer<TraceItem>`, `Answer<DependentsReport>`, and the `find` answer) to human text from the same serialized value the JSON path uses; remove the `{:#?}` rendering.
- [x] Make rendering detail-aware: `location` → one line `path:start-end`; row answers → header line plus one line per result; content details (`signature`/`interface`/`body`) → the tier text as a multi-line source block.
- [x] Add the `--color=<auto|always|never>` gate using `std::io::IsTerminal`: `auto` styles only when stdout is a terminal, and styling is suppressed under `--json` and when stdout is redirected.
- [x] Test: a multi-line body renders as multi-line source text, not an escaped single-line scalar.
- [x] Test: the same query rendered with `--json` and with the default human render presents the same results in the same order.
- [x] Test: piped (non-terminal) output carries no color/styling control sequences, and `--json` output carries none.

## Output bounding

- [x] Add a shared pagination step applied to every result-bearing answer: cap the result set to `--limit`, and when truncated, disclose truncation and emit an opaque continuation token encoding `(parameter-identity-hash, page-index)` bound to the index content-hash.
- [x] Add a shared content-bound step applied at the single tier-text projection point: cap each result's content to `--max-chars` and disclose per-result content truncation, independently of `--limit`.
- [x] Implement continuation resume: decode the token, recompute the parameter+index hash, reject a mismatch as a usage error naming the recovery, reject an out-of-range page naming the valid range, and otherwise return the next page over the deterministic order.
- [x] Test: a query exceeding `--limit` returns at most that many results and discloses result-set truncation.
- [x] Test: resuming with the continuation token returns the next page deterministically from where the prior page ended.
- [x] Test: a result whose content exceeds `--max-chars` is capped with per-result content truncation disclosed (exercised through a `get` body and through a `trace` content row).
- [x] Test: a token presented with different query parameters, and a token presented after the index is rebuilt, are each rejected as usage errors rather than mis-paged.

## Query commands

- [x] Add a store query selecting symbols whose `display_name` contains a fragment case-insensitively (`LIKE '%…%'` with metacharacters escaped), ordered by `canonical_id`.
- [x] Add the `find` command wiring that query through resolution-independent search, returning a bounded answer, with a no-match result as typed absence.
- [x] Add `Relation::Importers` and `Relation::Implementers`, reverse-walking the `imports` and `type_hierarchy` edges by destination, and expose them as `--relation` values on `trace`.
- [x] Test: `find` returns every symbol whose name contains the fragment, matches case-insensitively, treats a no-match as typed absence, and honors `--limit`.
- [x] Test: `trace --relation importers` returns exactly the modules importing the subject; `trace --relation implementers` returns exactly the types declaring the subject as a supertype.
- [x] Test: the pre-existing `trace` relations (`containers`, `contains`, `references`, `dependents`) and the typed-empty relation behavior are unchanged.

## Operational commands

- [x] Add the `doctor` command: probe each required indexer (`rust-analyzer`, `scip-python`) for presence and version, print a fixed-width report with an install hint for any absent one, and exit `5` when a required indexer is missing.
- [x] Add the `cache` command: remove the index at the discovered `--db` path, report the path, treat an already-absent index as success, and map an OS removal error to exit `1` naming the path.
- [x] Test: `doctor` reports each indexer present with its version and exits `0` when all are installed; reports an absent indexer with an install hint and exits `5` when one is missing.
- [x] Test: `cache` removes an existing index and reports the path; is success when there is nothing to remove; and on a removal OS error reports a failure naming the path.

## Introspection

- [x] Add a `surface_version` constant and the `manifest` command: walk the clap `Command` tree for commands, valid arg/flag values, and defaults, and emit them as JSON alongside `surface_version` and the current index state.
- [x] Test: `manifest` enumerates the commands with their valid arg/flag values and defaults, and carries a `surface_version`.
- [x] Test: a snapshot of the derived surface structure fails when the command/flag structure changes without a `surface_version` bump (drift is a checked invariant).

## Bounding defaults and windowing

- [x] Default `--limit` to 25 on `get`/`trace`/`find` at the command layer (engine stays bounding-agnostic); make `--limit 0` the explicit unbounded sentinel (remove the ranged ≥1 parser); attach a page block only for a partial view (truncated or a resumed page).
- [x] Rename `--max-chars` to `--max-lines` and re-implement the single content-cap point to cap by lines; per-command defaults (10 on `trace`, 100 on `get`); `0` = unbounded; remove the flag from `find`; `dependents` inherits trace's default.
- [x] Add `--from` (get-only) windowing: return the lines `[from, from + max-lines)`; disclose the window via an additive `content_lines` block (start/end/total) plus `content_truncated`; a `--from` past the end yields empty content with the total disclosed, not an error; include `from` in `get`'s page-identity hash.
- [x] Add the modal teaching errors: an explicitly typed `--max-lines`/`--from` with no content-bearing detail is a usage error naming the accepting details, on `get` and `trace`, using clap's `ValueSource` so defaults stay dormant.
- [x] Cap an `Ambiguous` candidate list at the effective limit, disclosing the total via an additive `candidates_total` field and a "…and N more — narrow the reference" human line; no cursor for candidates.
- [x] Regenerate the surface snapshot for the new flags/defaults and add `--max-chars` to the vocabulary test's banned-alias set.
- [x] Test: a default-bounded query over a >25 result set returns 25 with truncation disclosed and a resuming cursor; `--limit 0` returns the whole set with no page block.
- [x] Test: per-command `--max-lines` defaults bind (a trace row capped at 10 lines; a `get` body capped at 100 lines) with no flag typed.
- [x] Test: `--from` windows `get` content with position/total disclosed, including the past-the-end empty-content case and the next-window hint.
- [x] Test: an explicit `--max-lines`/`--from` on a non-content detail is a usage error naming the accepting details, on `get` and `trace`; `find` rejects `--max-lines` as an unknown argument.
- [x] Test: an ambiguous reference over more candidates than the limit returns a capped candidate list with the total disclosed.
- [x] Test (re-pinned): the content-bound unit tests are line-based (a UTF-8 line test replaces the multibyte-character-boundary test, which line-splitting makes moot); the `--limit 0` process test asserts unbounded behavior; the `--max-chars` process tests assert `--max-lines` semantics.

## Row locations and output hardening

- [x] Add a control-character sanitizer to the render module and route every structural human-output interpolation through it — the header (analyzer name/version), row lines (canonical id, display name, kind, document path), ambiguity candidate lines, `FindItem` rows, `DependentsReport` rows — plus the `doctor` (tool name/version/hint) and `cache` (path) human reports in `src/commands.rs`; leave `--json` output and content-bearing tier text (signature/interface/body) verbatim.
- [x] Add `location: Option<Location>` to `TraceItem::Symbol`, populated via the existing `location_of(row)` at its four construction sites in `trace()` (`contains`/`containers`/`importers`/`implementers`); render it as ` at <path>:<start>-<end>` on the human row when present.
- [x] Extend `find`'s doc comment in `src/cli.rs` (a second paragraph, so `manifest`'s derived `about` text is unaffected) to state that matching is case-insensitive for ASCII letters and exact for non-ASCII characters.
- [x] Test: a symbol whose `display_name`/`document_path`/tier text carry an ANSI escape and a raw newline, inserted via the store API — the human render of `get`, `find`, and a `trace --relation contains` row carries no escape and no forged row; the same query under `--json` preserves the hostile name and path byte-exactly; a `get --detail body` over the same symbol still renders the escape and newline verbatim in the content block.
- [x] Test: a `doctor` PATH-stub tool whose `--version` output carries an ANSI escape is reported present with no escape in the human report, while `--json` preserves the hostile version byte-exactly.
- [x] Test: a `trace --relation contains` row over an in-workspace member carries its document path and span in JSON and renders `at <path>:<start>-<end>` in the human row; a member with no persisted span (external) carries no `location` key and no `at` suffix, in both views.

## Shell completions

- [x] Add the `clap_complete` dependency (version-aligned with clap 4).
- [x] Add `Command::Completions(CompletionsArgs)` to `src/cli.rs`, a required positional `shell: clap_complete::Shell`.
- [x] Dispatch `completions` in `src/main.rs`: generate the script from `Cli::command()` to standard output; reject an explicit `--json` as a usage error naming why, before anything is written.
- [x] Add a "Shell completions" section to `README.md` with zsh and bash install one-liners.
- [x] Regenerate the surface snapshot fixture for the new `completions` command.
- [x] Test: `completions zsh` exits `0`, emits a non-empty script on standard output with no diagnostic, and the script names the current top-level commands.
- [x] Test: `completions bash` exits `0` and emits a script distinct from the zsh script.
- [x] Test: `completions nosuchshell` exits `2` with a diagnostic listing the accepted shells.
- [x] Test: `--json completions zsh` (explicit) exits `2` with a diagnostic naming `--json` and `completions`.
- [x] Test: `completions zsh` succeeds in an empty directory with no index, creating none.

## Boundary hardening and dependents bounding

- [x] Page a `dependents` report's detailed rows under the effective `--limit` through a sibling of the shared pagination step (`apply_dependents_pagination`, one token encode/decode path), repeating the depth-bound/horizon/disclosure/beyond-bound summary on every page; render the paged rows, the summary, and the standard page line in the human dependents section.
- [x] Guard `cache` removal behind an index-store header check: refuse (exit `1`, naming the manual `rm` alternative) unless the target carries the SQLite magic and a nonzero `user_version` stamp; keep an empty file removable as a failed-create artifact; build the WAL/SHM sidecar paths byte-preservingly from the primary's os-string.
- [x] Extend sanitization to content blocks in the human render: tier text passes the same control/bidi class map with `\n`, `\t`, and `\r` exempt, applied to the value before any styling composition; `--json` stays byte-exact.
- [x] Route the stderr diagnostic paths in `src/main.rs` (the error printer and doctor's missing-indexer line) through `render::sanitize`.
- [x] Use saturating arithmetic in `window_content`'s end computation so a `--max-lines` near `usize::MAX` cannot overflow with a `--from` past line 1.
- [x] Require build metadata in `open_query_store`: a schema-stamped store with no recorded metadata is the no-index outcome (exit `3`) with a message naming `c10r build`; stamp minimal metadata in the directly-built test fixture stores via a shared `tests/support` helper.
- [x] Reject `--from 0` at parse time with a ranged (`1..`) clap parser, removing the runtime coercion to line 1.
- [x] Add `values` to `ArgManifest` so a `ValueEnum`-backed positional (e.g. `completions`' `shell`) enumerates its valid values as flags already do; regenerate the surface snapshot (structure-only change, `SURFACE_VERSION` unshipped at 2).
- [x] Guard the `UPDATE_SURFACE_SNAPSHOT` regeneration path: refuse to overwrite the fixture when the structure changed but the recorded `surface_version` equals the current one, via a pure `regen_allowed` decision function.
- [x] Test: an explicit `--limit` caps a dependents report's detailed rows and the cursor resumes the next rows exactly, with the aggregate and horizon disclosure present on every page; the default limit binds over a >25-direct-dependent subject.
- [x] Test (re-pinned): `cache` refuses a plain text file at the `--db` path, leaving it intact and naming the `rm` alternative — the prior pin asserted removal of any file.
- [x] Test: `cache` removes a valid stamped store and a wrong-version stamped store (recovery case); refuses a zero-`user_version` SQLite file; removes an empty file as success.
- [x] Test (re-pinned): a hostile body renders with its terminal controls as replacement characters (and byte-exact under `--json`) — the prior pin asserted the controls rendered verbatim; a CRLF body renders with no replacement characters.
- [x] Test: `--from 2 --max-lines 18446744073709551615` returns the full remaining window with exit `0`; `--from 0` exits `2` naming the 1-based constraint.
- [x] Test: a schema-stamped, metadata-less store answers a query with exit `3`; a stderr diagnostic embedding a hostile `--db` path carries no raw escape byte.
- [x] Test: the `shell` positional lists bash/elvish/fish/powershell/zsh in the manifest; `regen_allowed` refuses only a structural change at an unchanged version.

## Bounded external-tool probes

- [x] Add a shared std-only bounded-probe helper: spawn with piped stdout/stderr, drain each pipe on its own thread into a 64 KiB-capped buffer (read past the cap, discard the excess), poll `try_wait` against a deadline parameter, and on expiry kill+reap the direct child; abandon the reader threads on the timeout path (never join — a descendant can hold the pipe open), join them only on the success path; return a typed completed/timed-out outcome.
- [x] Route the four short-lived probes through the helper at the design-sourced 10s deadline: `rust-analyzer --version` (adapter construction), `cargo metadata --no-deps` (library roots, timeout degrades to the existing empty map), `scip-python --version` (version discovery), and the resolved interpreter's `--version` (environment facts); leave the two long-running analysis runs (`rust-analyzer scip`, `scip-python index`) unbounded.
- [x] Add a present-but-unresponsive indexer state to `doctor`: distinguish present/absent/unresponsive with a clean serde shape, give unresponsive an investigation hint distinct from the install hint, render the third state in the human report, and exit with the indexer/setup-failure code when any indexer is absent or unresponsive.
- [x] Disclose a query-path probe timeout: in the freshness read, a probe timeout keeps the recorded-provenance / absent-environment fallback (machine answer unchanged) but emits one sanitized stderr diagnostic; a missing tool stays silent as before.
- [x] Test (helper): the completed and timed-out outcomes with short deadlines, a descendant holding the pipe open does not block the probe, and a flood past the cap is bounded.
- [x] Test (doctor): a hanging indexer stub is reported unresponsive with the investigation hint and exits with the setup-failure code within a bounded window; a stub whose backgrounded grandchild holds the probe pipe open does not hang doctor; a stub flooding its version output past the cap still reports present.
- [x] Test (query): a `get` against a built index with a hanging analyzer on `PATH` answers from recorded state on stdout with the timeout disclosed on stderr, within a bounded window.

## Verification remediation

Closes the 2026-07-19 verify pass's CRITICAL/WARNING write-site partition gaps (human-render sanitization under-exercised, five narrow branches, one alias-literal and one exit-code gap) — tests only, no production change.

- [x] Test (Source-faithful content rendering): `tests/human_render.rs::references_rows_sanitize_hostile_fields_in_human_output` — a `trace --relation references` row over a hostile symbol renders sanitized at the default (no-content) detail with no forged row, and at `--detail signature` neutralizes all three control classes (ANSI escape, C1 CSI, bidirectional override) while `--json` round-trips byte-exactly; `build_hostile_db` extended with a reference occurrence and `HOSTILE_BODY` extended to carry all three control classes.
- [x] Test (Source-faithful content rendering): `tests/human_render.rs::ambiguous_candidate_lines_sanitize_hostile_names` — two symbols sharing a hostile shortname resolve ambiguously with the human candidate list sanitized and no forged candidate row; new sibling builder `build_ambiguous_hostile_db`.
- [x] Test (Source-faithful content rendering): `tests/human_render.rs::dependents_rows_render_sanitized_in_human_output` — a hostile dependent's human `dependents` rendering (summary and detail row) carries no raw terminal control and no forged row; new sibling builder `build_hostile_dependents_db`.
- [x] Test (Index reset): `tests/operational_commands.rs::cache_human_success_line_names_the_removed_path` — `cache` without `--json` reports the removal naming the (sanitized) path on stdout.
- [x] Test (Indexer readiness report): extended `tests/operational_commands.rs::doctor_reports_a_hanging_indexer_unresponsive_within_a_bounded_time` with a human-render invocation (same stubs, no second setup) asserting the unresponsive status word and investigation hint, exit `5`.
- [x] Test (Indexer readiness report): `tests/operational_commands.rs::doctor_reports_a_hanging_scip_python_unresponsive_within_a_bounded_time` — the scip-python arm of the unresponsive-indexer timeout, sibling of the rust-analyzer case.
- [x] Test (Bounded and resumable answers): `src/query/page.rs::tests::{out_of_range_page_names_the_valid_range, dependents_out_of_range_page_names_the_valid_range}` — unit tests (the CLI cannot honestly mint a token past the last page) calling `apply_pagination`/`apply_dependents_pagination` with a token encoded via the module's own `encode_token` naming a page past the last, asserting the valid-range diagnostic.
- [x] Test (Bounded and resumable answers): `tests/output_bounding.rs::dependents_detail_row_content_is_capped_and_disclosed` — a `trace --relation dependents --detail body --max-lines N` row is capped with its truncation disclosed; `build_dependents_db` extended with multi-line dependent bodies.
- [x] Test (Bounded and resumable answers): `tests/output_bounding.rs::page_block_is_suppressed_when_a_positive_limit_is_not_exhausted` — a `--limit` larger than the result set carries no `page` block, distinct from the `--limit 0` early return.
- [x] Test (Rejections name valid alternatives): `tests/command_surface.rs::malformed_at_position_is_a_usage_error_naming_the_expected_form` — `get --at` with a missing colon and with a non-numeric offset are each a usage error naming the expected form.
- [x] Test (Exit-code taxonomy): `tests/command_surface.rs::unstamped_sqlite_store_is_the_no_index_code` — a valid SQLite file with `user_version = 0` at `--db` is the no-index code (exit `3`) observed at the process level.
- [x] Test (Closed flag vocabulary): `tests/command_surface.rs::banned_format_alias_is_rejected_by_the_binary` — the banned `--format` alias run against the built binary is a usage error, not just absent from the clap-tree walk.
- [x] Test (Relationship trace): `tests/code_navigation.rs::trace_implementers_with_none_is_typed_absence` — the `implementers` typed-empty branch, sibling of the existing `importers` case.
