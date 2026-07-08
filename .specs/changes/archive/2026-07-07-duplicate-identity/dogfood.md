# Dogfood & Ground Truth — duplicate-identity (2026-07-07)

Two rounds: this repository (below), then two external Rust projects for generalizability (fd, ripgrep — recorded in the External section).
The dogfood surfaced two defects fixed and spec-amended mid-run (F4, F5), and its F4 refusals were then upgraded to authoritative resolution on user direction (F6: package-name tokens resolve to the library twin via `cargo metadata`); all numbers below are from fresh stores built with the final binary.

## Build & status (fresh store, schema v6)

`aligned=14833 (exact=13345 crate_root=319 operator_desugar=1082 module_span=25 self_keyword=62) text_mismatch=285 semantic_only=0 duplicate_ambiguous=0 syntax_only=4` — conservation holds.
A second build over the same store reproduces every count identically (rebuild idempotence, F5's contract).

## Twin groups: 6/6, no over-splitting

`status --duplicates` discloses exactly the six predicted groups — `crate` ×6 (all crate roots), `sources` ×3, `ws`/`client_id`/`connect_id`/`id_of` ×2 (test helpers) — and the raw SCIP index contains exactly the same six multi-definition descriptors (17 definitions total, verified with the scip-check tool).
The design's top risk (a backend emitting several definition occurrences for one real symbol, which the split would wrongly twin) did not materialize: multi-definition ⇒ genuine twins on this repository.
The canonical-collision groups (`graph#0/#1`, `Default#0/#1`, `params#0/#1` — distinct descriptors whose projections collide) are correctly absent from the disclosure (F2 fix).

## Locality recovery and the residual

Twin reference population on a fresh build: 276.

| Disposition | Count | Judgment |
| --- | --- | --- |
| `defining_document` attribution (rule `exact`) | 143 | test-helper calls inside their own defining files — all correct by hand-check construction |
| `module_chain` attribution (rule `crate_root`) | 37 | `crate::` keyword tokens in `src/` modules, chained through `mod` declarations to the lib root — correct |
| `target_metadata` attribution (rule `crate_root`) | 97 | `silent_cartographer` package-name tokens in test/bin files (including the 2 in the shared `tests/support/mod.rs`): resolved to the lib twin (`crate#0`, `src/lib.rs`) named by `cargo metadata` (F6) — all land on the library, never on the containing file's own root |
| refused `duplicate_ambiguous` | 0 | every twin reference on this repository is attributable with evidence |

The F4 interim state (95 package-name refusals) and the F6 upgrade are both preserved as unit-test scenarios; the shared-territory refusal stays pinned by its fixture test even though no instance survives on this repository (the two shared-document occurrences were package-name tokens, which target metadata settles first).
The historical "~750 aligned references at the six twins" recorded in the blast-radius dogfood was inflated by the then-latent store-accumulation defect (F5): that session rebuilt over same-version stores, so occurrence rows from successive builds were being summed.
~277 is the true population.

## Defects found by this dogfood (both fixed and spec-amended before sync)

- **F4 — package-name misattribution.** Before the fix, all 95 `silent_cartographer` package-name tokens were locality-attributed to the containing file's own crate root (95/95 wrong: a package-name token denotes the package's library target regardless of location).
  First fixed as an unconditional refusal carve-out; then upgraded on user direction (**F6**) to authoritative resolution — the Rust adapter reads each package's library-target root from `cargo metadata --no-deps`, and the join attributes package-name tokens to the twin defined there under the third locality rule `target_metadata`, refusing only when the metadata is unavailable or names no persisted twin.
  The code-graph delta carries the final contract (scenarios "Package-name reference resolves to the library target" and "Package-name reference without target metadata is typed ambiguous").
- **F5 — builds accumulated rows.** `ingest` never cleared prior derived rows; a same-version rebuild doubled every occurrence row (138 doubled spans measured), and rows for vanished entities lingered.
  Latent in every prior release (each change bumped the schema version, forcing store replacement).
  Fixed with whole-build supersession in one transaction; the code-graph delta gained "Builds wholly supersede prior derived state", and the symbol-identity scenario that implied two workspaces sharing one store was reworded to per-workspace stores.

## Per-target imports (blast-radius caveat resolved)

`GraphStore` importers now include three distinct per-target crate roots (`crate#2`, `crate#3`, `crate#4`) plus `commands`, `query`, `query::resolve`, and the `graph` module — the blast-radius dogfood's "several documents share the crate-root module symbol, dedup collapses them" caveat is gone.

## Pinned counts

No existing test pins whole-repository accounting numbers (the suite is fixture-based), so no pins needed updating; this note satisfies the task.

## Observation (out of scope, recorded for §11)

rust-analyzer emits multiple identical occurrences (same span, same symbol, same role) at desugared-operator sites — 292 such span groups on this repository, all `operator_desugar`, stable across rebuilds.
The store records the raw emission faithfully; whether to collapse them is a future calibration question, not a defect of this change.

## External projects (generalizability, 2026-07-07)

Both cloned at depth 1 into session scratch; indexed with the final binary.

### fd (sharkdp/fd — single bin crate + integration tests)

`aligned=7446 (exact=6702 crate_root=242 operator_desugar=444 module_span=24 self_keyword=34) text_mismatch=88 semantic_only=0 duplicate_ambiguous=0 syntax_only=20` — 98.6% aligned, identical before and after F6.
1 twin group (`crate` ×2: `src/main.rs` + `tests/tests.rs`), matching the raw index's multi-definition population exactly.
Locality: 12 `defining_document` + 41 `module_chain`, zero refusals — fd has no lib target, so no package-name tokens exist and every `crate::` reference chains cleanly to the bin root.

### ripgrep (BurntSushi/ripgrep — 13-crate workspace)

`aligned=49951 (exact=45422 crate_root=1514 operator_desugar=2912 module_span=98 self_keyword=5) text_mismatch=1093 semantic_only=0 duplicate_ambiguous=77 syntax_only=111` — 97.3% aligned.
12 twin groups, matching the raw index's 12 multi-definition descriptors exactly (no over-splitting), and exercising families the self-repo never produced:

- crate roots across bench/example/build-script/test targets of one package (`globset`, `grep`, `grep-matcher`, `grep-searcher`, `ignore` ×4, `ripgrep` ×3 including `build.rs`);
- a same-named function across two targets of one package (`main` in `build.rs` and `crates/core/main.rs`);
- a same-named const across two integration-test files (`IGNORE_FILE`);
- **same-document twins** (`HAYSTACK` ×6 and `SHERLOCK` ×2 in `tests/feature.rs`, `MyError` ×2, `glue::tests::SHERLOCK` ×2 — all genuinely multi-definition in the raw index, macro/cfg-generated).

Locality at scale: 394 `module_chain` + 148 `target_metadata` + 15 `defining_document` + 2 `exact` attributions.
The `target_metadata` rule resolved cross-package references correctly (spot-checked: `ignore`'s use of `globset` attributes to `crates/globset/src/lib.rs`; a bench file's package-name token attributes to its package's lib, not the bench root).
The 77 refusals decompose entirely into honest categories: 35 `SHERLOCK` + 6 `HAYSTACK` + 4 `MyError` (same-document twins, which document-grained locality can never separate), and 32 facade re-export aliases (`grep::matcher::…` puts a `matcher` token on the `grep-matcher` crate root — the token spells neither the package nor the descriptor name; these were text-mismatch refusals before this change, so no attribution was lost).

### Generalizability verdict

The split criterion (multiple definition occurrences ⇒ twins) held on all three repositories with zero counter-examples; disclosure matched raw ground truth 6/6, 1/1, 12/12; locality attribution scaled to a 13-crate workspace and to cross-package references; every surviving refusal is a case where attribution genuinely is not evidenced.
**Follow-ups filed (brainstorm §11):** same-document twins (macro/cfg-generated) need a scope-grained locality rule; facade re-export aliases join the existing use-tree-alias refusal family.
