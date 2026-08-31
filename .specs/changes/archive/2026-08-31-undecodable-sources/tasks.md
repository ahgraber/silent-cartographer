# Tasks: undecodable-sources

## Discovery guard

- [x] In `collect_dir` ([src/commands.rs:94](../../../src/commands.rs#L94)), match the `read_to_string` error instead of propagating it: on `std::io::ErrorKind::InvalidData`, skip the file; on every other kind, propagate with the existing context.
- [x] At that skip, emit a one-line diagnostic on standard error naming the workspace-relative path of the excluded file, routed through `crate::render::sanitize`.
- [x] Update the doc comments on `collect_sources`, `collect_rust_sources`, and `collect_python_sources` to state that a file whose bytes are not valid UTF-8 is excluded and reported.

## Discovery-level tests

- [x] Add a unit test in `src/commands.rs` that collects over a temporary workspace holding one CP-1251 source file and two decodable siblings, and asserts the result is exactly the two siblings and that collection returned `Ok`.
- [x] Add a unit test in `src/commands.rs` that collects over a temporary workspace holding a discovered source file made unreadable for a non-encoding reason, and asserts collection returns `Err` naming that file; gate the test on a runtime check that the read is actually denied, so it skips rather than fails when the suite runs with privileges that defeat the permission.

## Non-Unicode path guard

A file whose relative path is not valid Unicode is a separate failure mode from undecodable file _content_: the same lossy path-to-string conversion that renders a diagnostic also builds the document-path key everywhere downstream, so two distinct non-Unicode paths could otherwise collide onto the same key.
Excluded outright, the same shape as the content guard.

- [x] In `collect_dir` ([src/commands.rs](../../../src/commands.rs)), before the content read, gate on `rel_path.to_str()`: on `None`, emit a diagnostic (best-effort `to_string_lossy()` naming) and skip the file; on `Some`, proceed with the existing content-decode guard.
- [x] Update the doc comments on `collect_rust_sources` and `collect_python_sources` to state that a non-Unicode path is excluded and reported, alongside undecodable content.
- [x] Add a unit test in `src/commands.rs` that collects over a workspace holding one file with a non-Unicode name alongside a decodable sibling, and asserts the sibling alone is collected.
  Gated at runtime, not by `target_os`: it attempts the fixture write and, when the filesystem refuses a non-Unicode name outright (as macOS's does), prints why and returns rather than asserting on an unavailable condition — the same shape as the permission-probe test above it, and informed by how `../ai-zettelkasten`/`../blackwall` gate on the actual capability rather than the host OS.
  Confirmed live via `--nocapture` (`skipped a_non_unicode_path_is_excluded_not_fatal: ...`) that the test compiles and discloses its own limit here.
- [x] Add a scenario to the delta spec for this exclusion, and confirm it against the design's `ExcludeNonUnicodePaths` decision.
- [x] Record a Verification Waiver in `design.md` for the non-Unicode-path clause (no runnable evidence in this macOS, no-CI environment), backed by a manual code-trace record.
  See `design.md` § Verification Waivers and [manual-evidence-non-unicode-path.md](manual-evidence-non-unicode-path.md).
- [ ] **Follow-up, not yet done:** run `cargo test --lib commands::tests::a_non_unicode_path_is_excluded_not_fatal` on a Linux host (or Linux CI, once this repository has one) and record the result, closing the waiver above with real execution evidence.

## Excluded-file symbols report absent, not a hollow `Found`

An in-workspace symbol every one of whose occurrences names a document source discovery excluded had no source-derived content, but was still persisted as a queryable row — `get`/`find` returned it `Found` with every content field empty, a confidently-wrong answer shape a parallel review caught.
The fix omits the row itself; see the design's `OmitSymbolRowsWithNoSurvivingDocument` decision for why it is scoped to "no occurrence in a document the corpus holds," not the broader "no aligned definition."

- [x] In `ingest_with_params`'s symbol-row loop ([src/graph/mod.rs](../../../src/graph/mod.rs)), skip persisting a row for an in-workspace symbol when every one of its occurrences names a document absent from the prepared corpus (`PreparedCorpus::get`, the same lookup the join already applies per occurrence).
- [x] Strengthen the join-side test (below) with a `store.symbol(...)` assertion that the row is absent, and `QueryEngine::get`/`QueryEngine::find` assertions that both report absence rather than a contentless `Found`.
- [x] Confirm the pre-existing `semantic_only_occurrence_is_unaligned` test (a symbol whose document _is_ present but whose occurrence location is out of range) is unaffected — its row-persistence behavior must not change.
- [x] Re-run the full test suite to confirm no other test relies on a hollow row surviving for a wholly-document-absent symbol.

## Join-side evidence

- [x] Add a test in `tests/code_graph.rs` that ingests an `ExtractedIndex` whose documents include one absent from the sources passed to `ingest` (standing in for a file source discovery excluded), with occurrences in it, and asserts those occurrences are counted `semantic_only`, that the alignment accounting still sums to the total occurrences processed, that no row is persisted for the symbol, and that `get`/`find` report it absent.
  (This test ingests directly rather than driving `collect_rust_sources` over a real temp directory — the same idiom every other join test in this file uses; the discovery-level unit tests above are what exercise the real on-disk path.)

## Build and currency-check evidence

- [x] Add a test in `tests/operational.rs` that builds over a workspace holding an undecodable file, and asserts the build completes, `collect_rust_sources` excludes the file from what feeds the build, and a symbol defined in a decodable sibling is retrievable. (No live analyzer is available in this environment — `build_from_index` is the established seam every other retrievability test in this file uses in its place; what this test adds is driving the real `collect_rust_sources` discovery over an on-disk undecodable file rather than a fixture's already-collected sources.)
- [x] Add a test in `tests/operational.rs` that seeds a store through `build_from_index` over the collected sources of such a workspace, then runs `run_build` with the version-only stub, and asserts the outcome is already-current — evidence that the build side and the currency-check side exclude the same file.

## Process-level evidence

- [x] Add a test in `tests/operational_commands.rs` that drives the built binary's `build --json` over a workspace holding an undecodable file, and asserts the process exits with the success code, standard error names the excluded file, standard output parses as the `--json` build report and never carries the excluded file's name, and the report shows the build actually ran (`rebuilt: true`).

## Dogfood evidence

- [x] Clone the reproducer repository (a real repository shipping a fixture that is not valid UTF-8) into the dogfood clone directory at `~/.cache/silent-cartographer/`.
  Used [sphinx-doc/sphinx](https://github.com/sphinx-doc/sphinx) — `tests/roots/test-pycode/cp_1251_coded.py` is a genuine CP-1251 fixture, confirmed not valid UTF-8.
- [x] Run a working-tree build of `c10r` against that clone with a change-scoped `--db`, and capture the exit status, the diagnostic naming the excluded file, and the resulting total persisted symbol count.
- [x] Query the excluded file's own symbol (`find`, and `get` by qualified name) and capture that both report absent — the real-world confirmation that the excluded-file-symbols fix above holds outside the unit-test fixtures.
- [x] Record the captured dogfood output under `.specs/changes/undecodable-sources/`.
  See [dogfood-evidence.md](dogfood-evidence.md).

## Gate

- [x] Run the full test suite and report the result.
- [x] Run every `.pre-commit-config.yaml` hook over everything changed since `HEAD`, staged or not, including new files, and report the result.
