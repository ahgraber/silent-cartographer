# Design: duplicate-identity

## Context

- The downstream stack is already contract-conformant for same-descriptor groups: `identity::project_all` disambiguates collision groups anchored to definition location (deterministic, encounter-order-independent), and the join (`graph/join.rs`) detects duplicated descriptors by a two-definition quorum and types refused occurrences `DuplicateAmbiguous`.
  The brainstorm §11 "rank duplicate groups by definition document path" follow-up is therefore already satisfied by `project_all`'s rank key; no identity-layer work is needed.
- The single starving point is `translate_index` (`src/semantic/rust_adapter.rs`): it accumulates occurrences into one `ExtractedSymbol` per SCIP symbol string, so same-descriptor twins are merged before identity projection or the join ever run.
  Measured on this repository (blast-radius dogfood, 2026-07-06): 6 twin groups — `crate/` ×6 (all crate roots) plus 5 same-named test-helper functions — carrying ~750 aligned reference occurrences.
- The baseline code-graph contract mandated unconditional duplicate-ambiguous refusal for twin references; the delta replaces it with locality attribution guarded by the ordinary alignment rules.
- Store schema is at version 5; the incompatible-store contract (build replaces, query refuses with guidance) already governs version bumps.

## Decisions

### Decision: Split in a backend-neutral normalization pass, not per adapter

**Chosen:** a normalization step applied to every backend's `ExtractedIndex` before identity projection: any in-workspace symbol carrying more than one definition occurrence is split into one symbol per definition occurrence (each twin keeping its own definition), and the group's non-definition occurrences move to a group-addressed collection on the index (descriptor + occurrences), leaving no reference exclusively assigned to a twin at extraction.

**Rationale:** the merge is a property of accumulate-by-descriptor-string translation, which any SCIP adapter (scip-python included) will share; normalizing at the model layer makes the semantic-engine delta contract hold at the port boundary for every current and future backend, with adapters unchanged.
The information needed to unmerge (role and location per occurrence) is fully preserved in the merged form, so a post-pass loses nothing.

**Alternatives considered:**

- Fix inside `translate_index` only: correct for Rust but re-derived per adapter; the Python adapter would need the same logic re-implemented and re-tested.
- Split at identity projection or in the join: smears group semantics across layers that currently receive already-shaped symbols; projection and join stay unchanged consumers instead.

### Decision: Locality attribution via two named rules, refusal default

**Chosen:** the join attributes a group's non-definition occurrence to a twin only when exactly one twin is selected by a locality rule, applied in evidence order:

1. **defining-document** — the occurrence's document is the definition document of exactly one twin.
2. **module-chain** — the occurrence's document belongs to the module tree of exactly one twin, where document→root association is derived from evidence already in the index: each module's defining document plus the module-declaration reference occurrences that link a module to the document declaring it, followed transitively to a crate root.

An occurrence no rule settles — document associated with no twin, or with more than one (e.g. a file compiled into two targets) — is typed `duplicate_ambiguous`.
A locality-selected occurrence still passes through the guarded join's alignment rules; locality selects the target, it never overrides a text refusal.

**Package-name resolution via build-target metadata (2026-07-07 dogfood amendments, two rounds):** an occurrence of a duplicated crate-root descriptor whose source token spells the package's own name (`use silent_cartographer::…` in a test or bin file) denotes the package's _library_ target no matter which document it sits in, so containing-document evidence points at the wrong twin — the self-dogfood measured 95/95 such attributions landing on the containing file's own crate root instead of the lib (first amendment: unconditional refusal).
Which twin is the library is not derivable from the index or from path conventions (rejected above), but it is an authoritative build-system fact: the Rust adapter obtains each package's library-target root document from `cargo metadata --no-deps` at analyze time and threads a package→library-root map (workspace-relative paths) through the extracted index; the join resolves a package-name token to the twin defined at that root under a third locality rule, `target_metadata` (second amendment).
When the metadata is unavailable (command fails, package absent, or no persisted twin at the named root) the occurrence falls back to the typed `duplicate_ambiguous` refusal — degradation is to honesty, never to guessing.
The comparison against the package name reuses the crate-root alignment rule's normalization (hyphen/underscore); the `crate`-keyword form (target-relative by construction) keeps ordinary locality attribution.
This shape carries to the Python adapter: absolute package imports have the same location-independent meaning, and that adapter supplies the analogous package-metadata facts.

**Rationale:** rule 1 alone recovers all same-file test-helper references and crate-root references in root documents; rule 2 recovers the dominant remainder (crate-root references throughout each target's module tree) from index-internal evidence, with no reliance on language-ecosystem path conventions.
Derivation gaps degrade to honest ambiguity, never to misattribution — the calibrated failure mode.

**Alternatives considered:**

- Refuse everything (baseline behavior): drops ~750 aligned references (98.1% → ~92% alignment) for no gain in honesty where evidence is decisive.
- Path-convention target mapping (`src/` vs `tests/*.rs`): Cargo-specific and brittle; contradicts deriving from index evidence and would not transfer to Python.

### Decision: Locality is provenance on the attribution, not a new alignment rule

**Chosen:** an attributed twin reference is counted under the alignment rule that accepted its text (exact, crate-root, …) exactly as today, and additionally records which locality rule selected the twin; the accounting identity (per-rule acceptances + refusals = total occurrences) is unchanged in shape.

**Rationale:** alignment rules answer "is this occurrence really this name at this spot"; locality answers "which twin owns it" — orthogonal questions, and conflating them would distort the per-rule acceptance counts that calibration monitoring relies on.

**Alternatives considered:**

- A `locality` alignment-rule bucket in the accounting: breaks comparability of rule counts across builds and double-encodes one acceptance.

### Decision: Duplicated-group disclosure is derived, not separately persisted

**Chosen:** duplicated-descriptor groups are computed from persisted symbols (group by identical descriptor, two-definition quorum) at query time for `status`/inspection; the build summary reports the group count alongside join-outcome counts.

**Rationale:** the store already persists each symbol's descriptor and definition span, so the groups are a query away; a dedicated table would be a second source of truth to keep consistent.

**Alternatives considered:**

- Persisting a groups table at build time: adds schema surface and invalidation duties for information the symbols table already determines.

### Decision: Crate-root module mapping reverts to the plain path

**Chosen:** with crate roots split, `module_by_doc` maps each root's defining document to that root's own symbol through the ordinary single-document path; the blast-radius multi-document workaround (every defining document of the merged crate symbol mapped to it) is removed.

**Rationale:** the workaround existed only because six crate roots were one merged symbol; after the split it is dead complexity, and module-scope references attribute to the correct per-target root, making `imports` edges per-target-precise (the dogfood's "several documents share the crate-root module symbol, dedup collapses them" caveat disappears).

**Alternatives considered:**

- Keeping the workaround defensively: contradicts the surgical-change rule and would mask regressions in the split.

### Decision: Schema version bumps to 6

**Chosen:** bump `SCHEMA_VERSION` 5 → 6.

**Rationale:** attribution provenance gains the locality dimension, and twin splitting changes persisted symbol rows and edge endpoints for previously-merged identities; stores written by 5 must be rebuilt, and the existing incompatible-store contract already gives builds replace semantics and queries a typed refusal.

**Alternatives considered:**

- No bump: a v5 store read by the new binary would silently carry merged twins — exactly the confident-wrong-answer class the product forbids.

### Decision: Builds wholly supersede (2026-07-07 dogfood amendment)

**Chosen:** `ingest` runs in one transaction that first deletes all derived rows (occurrences, edges, discrepancies, then symbols) and then inserts the new build; a failed build rolls back wholesale, leaving the prior build authoritative.
Consequence: one store holds exactly one build of one workspace; the symbol-identity baseline scenario that implied two workspaces sharing a store is reworded to per-workspace stores.

**Rationale:** the self-dogfood's second same-version build doubled every occurrence row — `symbols` deduped via INSERT OR REPLACE, `edges` via its unique index, only discrepancies were deleted, and occurrences accumulated; rows for entities gone from the sources also lingered.
Every prior change bumped the schema version, forcing store replacement and masking the defect; the single-row `index_metadata` already implied single-build semantics.

**Alternatives considered:**

- Unique index on occurrences: dedups doubles but still strands rows for vanished entities — dedup is not supersession.
- Replace the store file per build: loses the transactional failed-build-keeps-prior-state property the version-mismatch path does not need but rebuilds do.

### Decision: Dropped baseline scenario

The baseline scenario "Reference to a duplicated descriptor is typed ambiguous" (code-graph, requirement "Occurrences of duplicated descriptors are never arbitrarily attributed") is deliberately removed by the MODIFIED block: it asserted unconditional refusal, which this change replaces with locality-guarded attribution.
Its honest core survives as the two refusal scenarios (outside every territory; shared territory).

## Architecture

```text
backend adapter (rust today, python next)
        │  ExtractedIndex (may carry merged twins)
        ▼
normalization pass (semantic model layer)          ← NEW
        │  symbols: one per definition
        │  + group-addressed unattributed references
        ▼
identity projection (project_all)                   unchanged: ranks twins by definition site
        ▼
guarded join (graph/join.rs)
        │  per group-reference: locality rule → twin | duplicate_ambiguous   ← NEW
        │  then ordinary alignment rules; provenance = alignment rule + locality rule
        ▼
store (schema v6) ── edges follow attribution; module_by_doc plain path
        ▼
status / inspection ── duplicated groups derived from symbols; summary discloses count
```

## Risks

- **Over-splitting on multi-definition symbols that are not twins**: the split treats every extra definition occurrence as a distinct definition; a backend emitting several definition occurrences for one real symbol (none observed from rust-analyzer on this repo — the raw-index survey found only the 6 genuine twin groups) would be split into spurious twins.
  Mitigation: the dogfood re-checks the raw index's multi-definition population against the split output; if a counter-example family appears, the split criterion gains a guard and the semantic-engine delta gets amended before sync.
- **Module-chain derivation gaps**: documents the chain cannot associate (e.g. modules declared through macros) leave their crate-root references ambiguous.
  Acceptable by construction — refusal, not misattribution — and the residual count is visible in `duplicate_ambiguous` for the dogfood to judge.
- **Pinned-count churn**: existing tests asserting exact aligned/edge counts on this repository will shift (twins split, per-target imports edges appear, `duplicate_ambiguous` leaves zero).
  Mitigation: update pinned numbers with the dogfood evidence in hand, never by loosening assertions to ranges.
- **Shared-document targets**: a `src/` file compiled into both lib and bin would be associated with two roots and its crate-root references refused; on this repository the case is absent, but the shared-territory scenario pins the behavior so it stays typed, not arbitrary.
