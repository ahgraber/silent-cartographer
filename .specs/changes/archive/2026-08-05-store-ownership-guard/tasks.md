# Tasks: store-ownership-guard

## Recognizer and error foundations

- [x] Add the ownership constant (`application_id` = `0x63313072`) and a shared recognizer in `src/graph/store.rs` that reads the first 100 bytes with plain file I/O and returns a three-way verdict: absent (no file), unrecognized (short read, zero-length, wrong magic, wrong or missing `application_id`), or recognized.
- [x] Unit tests for the recognizer: absent path; zero-length file; non-SQLite file; SQLite file with no `application_id`; SQLite file with a foreign `application_id`; SQLite file with c10r's `application_id`; short/truncated file.
- [x] Add `StoreOpenError::UnrecognizedStore { path }` with the conditional two-branch message (rebuild branch for a genuine old index, fix-`--db` branch for an unrelated file; no unconditional delete instruction).
- [x] Unit test asserting the refusal message contains both recovery branches and no bare delete instruction.
- [x] Add `ExitCode::UnrecognizedStore = 6` in `src/exit.rs` and map `StoreOpenError::UnrecognizedStore` in `classify`; remove the `found == 0` → `NoIndex` special case (an unstamped file is now unrecognized before the version check runs).
- [x] Unit tests for `classify`: unrecognized-store error → code 6; recognized version-mismatch → code 4 (`Incompatible store is distinct` scenario); missing-index failure → code 3.

## Store lifecycle

- [x] Bump `SCHEMA_VERSION` to 13 and add the `workspace_root` column (canonicalized path, nullable — a root that cannot be recorded exactly is typed absence, which the `unknown` disclosure arm reads) to `index_metadata` in `src/graph/schema.rs`.
- [x] Rewrite `GraphStore::create`: build the store in a same-directory temp file opened with `create_new`, apply schema SQL, stamp `application_id` and `user_version`, commit, then rename onto the final path.
- [x] Unit tests for create: fresh create at an empty path yields a recognized v13 store; a pre-existing temp file is not reused (`create_new` refuses, build retries or errors cleanly); no partial file ever sits at the store path mid-create (temp name differs from final name).
- [x] Rewrite `GraphStore::open` as the read path: recognizer verdict first — absent refuses (no file created), unrecognized refuses with `UnrecognizedStore`, recognized proceeds to the version check — then opens with SQLite's read-only flag; open-time schema DDL is removed.
- [x] Unit tests for open: absent path refuses and creates nothing; foreign database refuses untouched; version-coincident foreign database refuses with nothing written into it (`Version-coincident foreign database refused by query` scenario); unmarked legacy store refuses with the rebuild branch; recognized current-version store opens and reads normally; recognized version-mismatched store refuses naming both versions (`Query refuses an incompatible store with guidance` scenario).
- [x] Rewrite `GraphStore::open_or_replace` as the build path: absent creates; unrecognized refuses with `UnrecognizedStore`; recognized version-mismatch removes store files (main plus sidecars) and recreates; recognized current-version opens read-write.
- [x] Unit tests for open_or_replace: foreign database with data refused byte-identical (`Foreign database refused by build`); version-coincident foreign database refused byte-identical (`Version-coincident foreign database refused by build`); non-database file refused with the typed error, not a storage error (`Non-database file refused`); recognized old-version store replaced and restamped at v13 (`Build replaces an incompatible store`); recognized current-version store opens without replacement (`Matching version operates normally`).
- [x] Integration test: store path resolving through a symlink to a foreign database — build refuses and the link target is byte-identical (`Redirected path to a foreign database refused`).

## Build and workspace identity

- [x] Record the canonicalized workspace root into `index_metadata.workspace_root` at every build, and surface it in `read_metadata`.
- [x] Unit test: a built store's metadata carries the canonicalized root of the workspace it was built from.
- [x] Implement the build handoff disclosure: before replacing or rebuilding a recognized _current-version_ store whose recorded root differs from the resolved root, emit a disclosure naming both roots on the diagnostic stream, then proceed and re-record.
- [x] Integration test: build over a current-version store recorded for a different root discloses both identities, succeeds, and the resulting store records the new root (`Build over a different workspace's store discloses and re-records`); build over a _version-mismatched_ recognized store proceeds without the disclosure guarantee (best-effort arm).

## Query surface

- [x] Route all query commands and `status` through the read-path open: a missing file yields the no-index failure (exit 3) naming `c10r build`, with nothing created at the path.
- [x] Integration tests: query against a missing path exits 3, names the build remedy, and leaves no file behind (`Query against a missing store creates nothing`); `status` likewise (`Status against a missing store creates nothing`).
- [x] Add the workspace comparison to the shared answer assembly: canonicalize the invocation root, compare with the recorded root, and emit a tri-state workspace field (`matched` carries no marker, `mismatched` carries the marker plus the recorded root, unevaluable comparison reports `unknown`) in the machine envelope and a corresponding human-render line.
- [x] Integration tests across the query command families (get, trace, find, impact, status): matching root → no marker (`Matching workspace carries no marker`); different root → marker present in JSON and human render (`Different workspace carries the marker`); different root with drifted sources → marker and staleness flag both present, independently (`Mismatch marker composes with staleness`); canonicalization failure → workspace reported `unknown`, not silently matched (`Unavailable comparison disclosed as unknown`).
  The unknown arm is reached through a store that records no root — what a build over a workspace path that cannot be represented exactly leaves behind — since a root that fails to canonicalize also fails source discovery, so that arm alone never reaches an assembled answer.
- [x] Integration test: ownership refusal from a query exits with code 6, distinct from codes 3 and 4 (`Ownership refusal is distinct`).

## cache and manifest

- [x] Replace `cache`'s `is_removable_index` header check with the shared recognizer (magic + `application_id`), keeping its existing refusal shape and exit behavior.
- [x] Integration tests: `cache` removes a recognized store (`Existing index removed`); refuses a foreign database carrying another application's version stamp, file intact (`A foreign versioned database is refused`); refuses a non-database file (`A non-index target is refused`); succeeds when nothing exists (`Nothing to remove is success`).
- [x] Extend `index_state` for `manifest`: distinguish absent (no file) from unrecognized (file present, recognizer refuses) in the reported index state, without failing the command; recognized stores keep the current shape.
- [x] Integration test: `manifest` against a foreign database succeeds and its index state reports the unrecognized condition, distinct from the absent-index shape (`Index state distinguishes unrecognized from absent`).
- [x] Update the `tests/surface_manifest.rs` fixture for the index-state change and decide the `SURFACE_VERSION` question: bump if the manifest's pinned structure (including exit-code enumeration, if present) changed; record the outcome in `design.md` if no bump is needed.
  Outcome: no bump, fixture unchanged — the snapshot pins the command/flag tree only, which this change does not touch; recorded in `design.md`.

## Remediation (verify 2026-08-05)

- [x] Add `StoreOpenError::UnreadableStore { path, cause }` in `src/graph/store.rs`, built by one helper that maps an `io::Error` to the message `cannot read {path}: {cause}` plus a hint for the two causes whose fix the OS words do not imply: `ErrorKind::IsADirectory` → `--db` wants the database file inside it (e.g. `{path}/index.db`); `ErrorKind::PermissionDenied` → check the file's owner and permissions.
  Every other cause carries the OS words alone, with no hint and no ownership claim.
- [x] Route the recognizer's `Err` through that helper in `GraphStore::open` and `GraphStore::open_or_replace`, and in `is_removable_index` (`src/commands.rs`) in place of its current `.with_context` wrapper.
- [x] Retry `ErrorKind::Interrupted` inside `recognize_store`'s read loop rather than propagating it — a signal mid-read is not a failed examination.
- [x] Map `StoreOpenError::UnreadableStore` and `StoreOpenError::StaleBuildFile` to `ExitCode::UnrecognizedStore` in `src/exit.rs::classify`, leaving `Storage`/`Io` on the generic code.
- [x] Report the third index state in `index_state` (`src/commands.rs`): the recognizer's `Err` arm yields `{"built": false, "store": "unreadable"}`, distinct from absent (`{"built": false}`) and unrecognized (`{"built": false, "store": "unrecognized"}`).
- [x] Shorten the `UnrecognizedStore` message to "{path} is not a c10r index store.
  If this is an old c10r index, remove it and run \`c10r build\`; otherwise point \`--db\` elsewhere." — dropping the self-referential clause about what c10r does or does not touch, while keeping both recoveries conditional and stating removal only inside the old-index branch.
- [x] Reword `StaleBuildFile` to "build file {path} already exists.
  If this is a leftover from an interrupted c10r build, removing it is safe; otherwise point \`--db\` elsewhere." — so it no longer asserts the file is c10r's own leftover.
- [x] Keep every refusal single-line: they reach the caller through `crate::render::sanitize`, which turns a literal newline into a replacement character, so a multi-line message renders as damage rather than as a break.
  Pin it with a unit test asserting each `StoreOpenError` variant's rendered message passes through `sanitize` unchanged.
- [x] Unit tests: a directory at the store path refuses through both `GraphStore::open` and `GraphStore::open_or_replace` with `UnreadableStore`, naming the path and the directory hint, asserting neither that the target is nor that it is not a c10r store (`Unexaminable path refused without an ownership claim`); a permission-denied file does the same, guarded to skip when the suite runs as root.
- [x] Unit test: the reworded `UnrecognizedStore` message still names the path, both recovery branches, and states removal only inside a conditional.
  If the existing `the_ownership_refusal_states_both_recoveries_and_deletes_nothing_unconditionally` probe for the literal `"if this is"` no longer matches the new wording, replace the probe with a structural check rather than bending the message to the assertion.
- [x] Unit test: `create_refuses_to_reuse_a_leftover_build_file` also asserts the reworded message makes no ownership claim and conditions removal (`An occupied build path is not overwritten`), and that the refusal classifies to exit code 6.
- [x] Integration tests: `cache` against a directory at `--db` refuses with the typed unreadable error naming the path, exits 6, and leaves the directory in place; `manifest` against the same path succeeds and reports `store: "unreadable"`, distinct from both other states (`Index state distinguishes an unexaminable path`).
- [x] Re-run the full suite through project tooling (`cargo fmt`, `cargo clippy`, `cargo test`) and re-run `sdd-verify`.

## Remediation (verify 2026-08-05, round 2)

- [x] Resolve `cache`'s split exit codes: refusing an unrecognized file exited 1 while refusing an unexaminable path exited 6.
  The Exit-code taxonomy requirement names "a target refused because the system cannot confirm it is a store the system created" as its own category, and reset's unrecognized-file refusal was that category landing on the generic code.
  Outcome: reset joins the ownership code.
  Added `Failure::UnrecognizedStore` (`src/exit.rs`), raised it from `run_cache` in place of the bare `anyhow!`, keeping reset's message shape (name the manual `rm`); a failure raised after recognition clears — a denied unlink — stays operational.
  Broadened the `Ownership refusal is distinct` scenario to name reset alongside build and query; recorded in `design.md` as _The ownership category belongs to the target, not to the command that met it_.
- [x] Repoint `cache_removal_os_error_names_the_path` at a genuine remove-time OS error.
  Its fixture — a directory at `--db` — was refused by `is_removable_index` before `std::fs::remove_file` was reached, so the `Removal error names the path` scenario had no regression test and the test duplicated `cache_refuses_a_path_it_cannot_examine` with weaker assertions.
  Now uses a recognized store whose parent directory is not writable, pinning exit 1 (the guard cleared; the unlink failed), guarded to return early when the process unlinks it anyway (root).
- [x] Resolve `manifest`'s reading of a recognized store that cannot be opened: it reported a recognized store at a mismatched schema version as an absent index, while every other command named the version mismatch and exited 4.
  Outcome: the index state gains a distinct reading (`{"built": false, "store": "incompatible"}`), added to the `Versioned structural surface index` delta clause with its own scenario, and recorded in `design.md` as _A recognized store at a schema version this binary does not read is its own index state_.
  Every other post-recognition read failure keeps the absent shape.
- [x] Correct this file's v13 task text: the `workspace_root` column is nullable, not `NOT NULL` — typed absence is what the `unknown` disclosure arm needs, as `design.md` records.
- [x] Refresh `proposal.md`'s status header: it read "Status: DRAFT" and "`tasks.md` deliberately not generated yet".

## Docs and close-out

- [x] Update README (and any exit-code table) with: the ownership guard behavior, exit code 6, the no-autocreate query behavior, the workspace-mismatch marker, and the one-time rebuild required for pre-v13 stores.
- [x] Run the full test suite through project tooling (`cargo fmt`, `cargo clippy`, `cargo test`) and confirm green.
- [x] Dogfood pass: rebuild one persistent clone under `~/.cache/silent-cartographer` (expect the one-time legacy refusal, then a clean rebuild), and spot-check a wrong-root query shows the mismatch marker.
