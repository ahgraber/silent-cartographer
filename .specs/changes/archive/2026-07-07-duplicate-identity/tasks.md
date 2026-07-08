# Tasks: duplicate-identity

## Normalization Pass (semantic model)

- [x] Extend the engine-neutral model with a group-addressed collection for unattributed occurrences: descriptor plus the non-definition occurrences of a duplicated descriptor (`src/semantic/model.rs`).
- [x] Implement the normalization pass: split every in-workspace symbol carrying more than one definition occurrence into one symbol per definition (each keeping its own definition occurrence), moving the group's non-definition occurrences into the group-addressed collection.
- [x] Wire the pass at the port boundary so every backend's `ExtractedIndex` is normalized before identity projection consumes it (rust adapter and fixture engine both flow through it).
- [x] Test: two same-descriptor definitions yield two distinct symbols, each anchored at its own definition location (scenario: Twin definitions yield distinct symbols).
- [x] Test: the group's reference occurrences land in the group-addressed collection and on no single twin (scenario: References to a duplicated descriptor survive extraction unassigned).
- [x] Test: a single-definition descriptor passes through unchanged, definition and references intact (scenario: Unique descriptors are unaffected).
- [x] Test: an external symbol (reference-only, no definition occurrence) is never split.
- [x] Integration test: normalized twins reach `project_all` and receive distinct definition-anchored identities ordered by definition document path.

## Locality Attribution (join)

- [x] Build the document→twin association for a duplicated group: the defining-document rule (a document that is the definition document of exactly one twin).
- [x] Build the module-chain association: document→crate-root derivation from module definition documents plus module-declaration reference occurrences, followed transitively to a root.
- [x] Process group-addressed references in the join: locality rules select a unique twin, then the ordinary alignment rules run; acceptance records both the alignment rule and the locality rule as provenance; no unique twin → typed `duplicate_ambiguous` discrepancy.
- [x] Test: reference in a twin's defining document attributes to that twin with `defining_document` locality provenance (scenario: Reference in one duplicate's territory is attributed to it).
- [x] Test: reference in a document reachable only through one twin's module chain attributes to that twin with `module_chain` provenance (same scenario, second rule).
- [x] Test: locality-selected occurrence whose source text satisfies no alignment rule is refused as text-mismatch, not attributed (scenario: Locality does not bypass the guarded join).
- [x] Test: reference in a document associated with no twin is typed `duplicate_ambiguous` (scenario: Reference outside every duplicate's territory is typed ambiguous).
- [x] Test: reference in a document associated with more than one twin is typed `duplicate_ambiguous` (scenario: Reference in shared territory is typed ambiguous).
- [x] Test: a unique-descriptor symbol's references still flow through the ordinary guarded join untouched by locality (scenario: Unduplicated descriptors are unaffected).
- [x] Implement the package-name carve-out: an occurrence of a duplicated crate-root descriptor whose source token spells the package name (crate-root rule normalization) is refused to `duplicate_ambiguous`, never attributed by locality (2026-07-07 dogfood amendment).
- [x] Test: package-name reference among duplicated crate roots is typed ambiguous while a `crate`-keyword reference in the same document still attributes (scenario: Package-name reference among duplicated crate roots is typed ambiguous).
- [x] Test: accounting conservation holds with locality-attributed references counted under their alignment rules and refusals under `duplicate_ambiguous` (existing conservation contract, new input shape).
- [x] Rust adapter: obtain the package→library-root map from `cargo metadata --no-deps` at analyze time (workspace-relative paths), thread it through the extracted index, degrade to an empty map on any failure (2026-07-07 second amendment).
- [x] Join: `target_metadata` locality rule — a package-name token in a duplicated group attributes to the twin defined at that package's library root; no metadata or no matching twin → `duplicate_ambiguous` as before.
- [x] Test: package-name reference attributes to the library twin with `target_metadata` provenance, never the containing document's own root (scenario: Package-name reference resolves to the library target).
- [x] Test: with no target metadata available, the package-name reference stays `duplicate_ambiguous` (scenario: Package-name reference without target metadata is typed ambiguous).

## Store & Schema

- [x] Bump `SCHEMA_VERSION` to 6 and persist locality provenance on aligned attributions (`src/graph/schema.rs`, `src/graph/store.rs`).
- [x] Remove the multi-document crate-root module mapping workaround; `module_by_doc` maps each root's defining document to that root's own symbol (`src/graph/mod.rs`).
- [x] Test: locality provenance round-trips through the store with the attribution.
- [x] Test: module-scope references in different targets produce `imports` edges from their own per-target crate roots (the blast-radius dedup-collapse caveat is gone).
- [x] Confirm the incompatible-store tests pass against the new version (build replaces a v5 store; query refuses it with guidance).

- [x] Fix (discovered by dogfood, pre-existing latent defect): a build over an existing same-version store accumulates rows — `occurrences` are never cleared, and `symbols`/`edges` rows for entities gone from the sources linger; ingest SHALL wholly supersede all derived tables per build in one transaction.
- [x] Test: building twice over the same store with unchanged sources yields byte-identical occurrence, symbol, and edge row sets (rebuild idempotence).

## Disclosure (status)

- [x] Implement the derived duplicated-groups query: group persisted symbols by identical descriptor under the two-definition quorum, returning each group's descriptor and definitions.
- [x] Surface the group count in the build/status summary alongside the join-outcome counts, and expose the group detail retrievably.
- [x] Test: a store with twins returns each group with its shared descriptor and member definitions (scenario: Duplicated groups are retrievable).
- [x] Test: a store with no duplicated descriptors returns a definite empty set, distinct from failure (scenario: No duplicates is typed absence).
- [x] Test: the status JSON carries the duplicated-group count.

## Dogfood & Ground Truth

- [x] Rebuild this repository's index; verify the 6 known twin groups (`crate/` ×6; `ws`, `client_id`, `connect_id`, `id_of`, `sources`) appear split and disclosed.
- [x] Cross-check the raw-index multi-definition population against the split output to confirm no over-splitting (the design's top risk); if a non-twin multi-definition family appears, amend the semantic-engine delta before sync.
- [x] Verify locality recovery: the bulk of the ~750 formerly-merged references attribute under a locality rule; record the residual `duplicate_ambiguous` count and judge it.
- [x] Spot-check per-target `imports` edges for a symbol from the blast-radius dogfood table (e.g. `GraphStore`).
- [x] Update pinned accounting counts in existing tests from the dogfood evidence; record the run in `dogfood.md`.
