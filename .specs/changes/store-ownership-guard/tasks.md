# Tasks: store-ownership-guard

## Recognizer and error foundations

- [ ] Add the ownership constant (`application_id` = `0x63313072`) and a shared recognizer in `src/graph/store.rs` that reads the first 100 bytes with plain file I/O and returns a three-way verdict: absent (no file), unrecognized (short read, zero-length, wrong magic, wrong or missing `application_id`), or recognized.
- [ ] Unit tests for the recognizer: absent path; zero-length file; non-SQLite file; SQLite file with no `application_id`; SQLite file with a foreign `application_id`; SQLite file with c10r's `application_id`; short/truncated file.
- [ ] Add `StoreOpenError::UnrecognizedStore { path }` with the conditional two-branch message (rebuild branch for a genuine old index, fix-`--db` branch for an unrelated file; no unconditional delete instruction).
- [ ] Unit test asserting the refusal message contains both recovery branches and no bare delete instruction.
- [ ] Add `ExitCode::UnrecognizedStore = 6` in `src/exit.rs` and map `StoreOpenError::UnrecognizedStore` in `classify`; remove the `found == 0` → `NoIndex` special case (an unstamped file is now unrecognized before the version check runs).
- [ ] Unit tests for `classify`: unrecognized-store error → code 6; recognized version-mismatch → code 4 (`Incompatible store is distinct` scenario); missing-index failure → code 3.

## Store lifecycle

- [ ] Bump `SCHEMA_VERSION` to 13 and add the `workspace_root` column (canonicalized path, NOT NULL) to `index_metadata` in `src/graph/schema.rs`.
- [ ] Rewrite `GraphStore::create`: build the store in a same-directory temp file opened with `create_new`, apply schema SQL, stamp `application_id` and `user_version`, commit, then rename onto the final path.
- [ ] Unit tests for create: fresh create at an empty path yields a recognized v13 store; a pre-existing temp file is not reused (`create_new` refuses, build retries or errors cleanly); no partial file ever sits at the store path mid-create (temp name differs from final name).
- [ ] Rewrite `GraphStore::open` as the read path: recognizer verdict first — absent refuses (no file created), unrecognized refuses with `UnrecognizedStore`, recognized proceeds to the version check — then opens with SQLite's read-only flag; open-time schema DDL is removed.
- [ ] Unit tests for open: absent path refuses and creates nothing; foreign database refuses untouched; version-coincident foreign database refuses with nothing written into it (`Version-coincident foreign database refused by query` scenario); unmarked legacy store refuses with the rebuild branch; recognized current-version store opens and reads normally; recognized version-mismatched store refuses naming both versions (`Query refuses an incompatible store with guidance` scenario).
- [ ] Rewrite `GraphStore::open_or_replace` as the build path: absent creates; unrecognized refuses with `UnrecognizedStore`; recognized version-mismatch removes store files (main plus sidecars) and recreates; recognized current-version opens read-write.
- [ ] Unit tests for open_or_replace: foreign database with data refused byte-identical (`Foreign database refused by build`); version-coincident foreign database refused byte-identical (`Version-coincident foreign database refused by build`); non-database file refused with the typed error, not a storage error (`Non-database file refused`); recognized old-version store replaced and restamped at v13 (`Build replaces an incompatible store`); recognized current-version store opens without replacement (`Matching version operates normally`).
- [ ] Integration test: store path resolving through a symlink to a foreign database — build refuses and the link target is byte-identical (`Redirected path to a foreign database refused`).

## Build and workspace identity

- [ ] Record the canonicalized workspace root into `index_metadata.workspace_root` at every build, and surface it in `read_metadata`.
- [ ] Unit test: a built store's metadata carries the canonicalized root of the workspace it was built from.
- [ ] Implement the build handoff disclosure: before replacing or rebuilding a recognized _current-version_ store whose recorded root differs from the resolved root, emit a disclosure naming both roots on the diagnostic stream, then proceed and re-record.
- [ ] Integration test: build over a current-version store recorded for a different root discloses both identities, succeeds, and the resulting store records the new root (`Build over a different workspace's store discloses and re-records`); build over a _version-mismatched_ recognized store proceeds without the disclosure guarantee (best-effort arm).

## Query surface

- [ ] Route all query commands and `status` through the read-path open: a missing file yields the no-index failure (exit 3) naming `c10r build`, with nothing created at the path.
- [ ] Integration tests: query against a missing path exits 3, names the build remedy, and leaves no file behind (`Query against a missing store creates nothing`); `status` likewise (`Status against a missing store creates nothing`).
- [ ] Add the workspace comparison to the shared answer assembly: canonicalize the invocation root, compare with the recorded root, and emit a tri-state workspace field (`matched` carries no marker, `mismatched` carries the marker plus the recorded root, unevaluable comparison reports `unknown`) in the machine envelope and a corresponding human-render line.
- [ ] Integration tests across the query command families (get, trace, find, impact, status): matching root → no marker (`Matching workspace carries no marker`); different root → marker present in JSON and human render (`Different workspace carries the marker`); different root with drifted sources → marker and staleness flag both present, independently (`Mismatch marker composes with staleness`); canonicalization failure → workspace reported `unknown`, not silently matched (`Unavailable comparison disclosed as unknown`).
- [ ] Integration test: ownership refusal from a query exits with code 6, distinct from codes 3 and 4 (`Ownership refusal is distinct`).

## cache and manifest

- [ ] Replace `cache`'s `is_removable_index` header check with the shared recognizer (magic + `application_id`), keeping its existing refusal shape and exit behavior.
- [ ] Integration tests: `cache` removes a recognized store (`Existing index removed`); refuses a foreign database carrying another application's version stamp, file intact (`A foreign versioned database is refused`); refuses a non-database file (`A non-index target is refused`); succeeds when nothing exists (`Nothing to remove is success`).
- [ ] Extend `index_state` for `manifest`: distinguish absent (no file) from unrecognized (file present, recognizer refuses) in the reported index state, without failing the command; recognized stores keep the current shape.
- [ ] Integration test: `manifest` against a foreign database succeeds and its index state reports the unrecognized condition, distinct from the absent-index shape (`Index state distinguishes unrecognized from absent`).
- [ ] Update the `tests/surface_manifest.rs` fixture for the index-state change and decide the `SURFACE_VERSION` question: bump if the manifest's pinned structure (including exit-code enumeration, if present) changed; record the outcome in `design.md` if no bump is needed.

## Docs and close-out

- [ ] Update README (and any exit-code table) with: the ownership guard behavior, exit code 6, the no-autocreate query behavior, the workspace-mismatch marker, and the one-time rebuild required for pre-v13 stores.
- [ ] Run the full test suite through project tooling (`cargo fmt`, `cargo clippy`, `cargo test`) and confirm green.
- [ ] Dogfood pass: rebuild one persistent clone under `~/.cache/silent-cartographer` (expect the one-time legacy refusal, then a clean rebuild), and spot-check a wrong-root query shows the mismatch marker.
