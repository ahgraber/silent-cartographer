# Tasks: internal-efficiency

Groups run in order.
Each measurement group is a checkpoint: its numbers decide whether the group after it is still worth its cost, and they are recorded in `discussion.md` before that group starts.

## 1. A build over unchanged inputs does no work

Requirement: _A build over unchanged inputs does no work_.

- [x] 1.1 Resolve the analyzer before analysis in `run_build`, and collect sources before the analyzer runs, so the currency check has both inputs and the version probe runs once.
- [x] 1.2 Add the currency check: compare the store's recorded content hash, analyzer provenance, and declared environment against the current ones through `GraphStore::freshness`, treating an absent, unreadable, metadata-less, or incompatible store as not current.
- [x] 1.3 Return a build outcome that distinguishes a rebuild from a skip, carrying the accounting either way — the freshly produced one, or the one the store already recorded.
- [x] 1.4 Add the `--force` flag to `build` and bypass the check when it is set.
- [x] 1.5 Render the outcome: a human line stating the index is already current and naming `--force`, and a machine projection carrying whether the store was rewritten.
- [x] 1.6 Bump `SURFACE_VERSION`, regenerate the surface snapshot, and record `force` in the closed flag vocabulary.
- [x] 1.7 Test: a build over an unchanged workspace analyzes nothing and leaves the store byte-identical (stub analyzer that fails any invocation other than `--version`).
- [x] 1.8 Test: an edited source, a changed analyzer version, and an absent index each drive the build to analyze.
- [x] 1.9 Test: `--force` analyzes a workspace whose index is already current.
- [x] 1.10 Test: a changed declared environment drives a Python build to analyze.
- [x] 1.11 Treat a store recorded for a different workspace identity or workspace root as not current, so the build runs, discloses the handoff, and re-records rather than skipping.
- [x] 1.12 Test: a build over a store recorded for another workspace analyzes and re-records, rather than reporting the index current.

## 2. Measure the currency check

- [x] 2.1 Measure cold build, unchanged rebuild, forced rebuild, and rebuild after a one-file edit, on one Python and one Rust dogfood repository.
- [x] 2.2 Record the figures in `discussion.md`.

## 3. One pass over each document

Requirement: _Deriving the graph costs one pass over each document_.

- [x] 3.1 Add `graph::prepared` with `PreparedCorpus` and `PreparedDocument`, moving `PreparedDocument` out of `join.rs` and adding its declarations keyed by name span.
- [x] 3.2 Prepare every document of the source corpus once, before the join, and expose the source text so consumers no longer need a separate source map.
- [x] 3.3 Take `&PreparedCorpus` in `join`, deleting its own preparation pass.
- [x] 3.4 Read tier content from the prepared document's declarations-by-name-span instead of parsing and walking `all_declarations` per symbol.
- [x] 3.5 Read definition parents, test classification, subtype edges, and clone keys from the prepared corpus instead of parsing per symbol, per definition, or per document.
- [x] 3.6 Read line indexes from the prepared document in `module_by_document` instead of building one per module symbol.
- [x] 3.7 Add the `#[cfg(test)]` parse counter and increment it in `SyntaxTree::parse`.
- [x] 3.8 Test: a symbol-dense document and a sparse document of equal size cost the same syntax work.
- [x] 3.9 Test: adding declarations to a document does not change the syntax work performed for it.
- [x] 3.10 Test: syntax work over a multi-document workspace is proportional to its documents.

## 4. Enclosure resolves by location

Requirement: _Reference occurrences carry enclosing-declaration attribution_ (MODIFIED).

- [x] 4.1 Build the definition-location map once per build from the join's aligned occurrences, and resolve enclosing declarations through it.
- [x] 4.2 Keep the `impl_item` path resolving through `type_by_name`, which no location map can serve.
- [x] 4.3 Refuse a location holding more than one persisted definition: attribute the occurrence as though no declaration enclosed it.
- [x] 4.4 Add the `#[cfg(test)]` comparison counter and increment it in the attribution lookup.
- [x] 4.5 Test: attribution work per occurrence is unchanged when the workspace persists many times as many definitions.
- [x] 4.6 Test: an occurrence whose nearest enclosing declaration's location holds two persisted definitions attributes to its module.
- [x] 4.7 Test: the three existing attribution behaviours — method, closure, module scope — still hold.

## 5. Measure one-pass derivation

- [x] 5.1 Re-measure cold build and rebuild after a one-file edit on the same two repositories.
- [x] 5.2 Compare stores built before and after groups 3 and 4 on all five dogfood repositories: symbols, occurrences, edges, discrepancies, corpus renders, and alignment accounting must match row for row, with both sides built from the same embedding code.
- [x] 5.3 Record the figures and the comparison result in `discussion.md`.

## 6. Re-derive only what changed — parked

Parked, not built, by user decision at the group 5 checkpoint (2026-08-20).
With one-pass derivation shipped, ingest is 4.5 s (Faker) / 3.3 s (ripgrep) of a rebuild whose floor is the whole-workspace indexer, so per-document re-derivation can recover at most a few seconds per edit while its blockers stand: workspace-global derivation inputs (identity collision ranks, cross-document test gating, `type_by_name`, duplicate marking) and derived rows with no single owning document.
Revisit only if the indexer's share of a rebuild shrinks.
The measurement and the blocker evidence are in `discussion.md`; the parked scope item is in `proposal.md`.

## 7. Carry the surface change into the MCP server

Runs after the Rust internals are settled, so the surface it pins against is final.

- [x] 7.1 Raise the MCP server's `SURFACE_VERSION` pin to the version the binary reports, so the startup check that refuses on a mismatch passes again.
- [x] 7.2 Decide whether the MCP `build` tool exposes the force option, and add it if a client needs a way past a store the currency check wrongly considers current.
- [x] 7.3 Correct the MCP `build` tool's cost description, which tells a client the call runs an indexer across the whole workspace and can take minutes — no longer true when the index is current.
- [x] 7.4 Run the MCP test suite against the built binary.

## Review remediation (2026-08-20)

External AI review, processed via receiving-feedback: five findings accepted, two accepted in part, one declined as a pre-existing condition this change narrowed (the analyzer-vs-snapshot race; recorded as a hardening candidate in `discussion.md`).

- [x] R.1 Back the currency check with one metadata read: extract `freshness_of` as a pure comparison over `IndexMetadata`, so ownership, freshness, and the returned accounting describe the same recorded build.
- [x] R.2 Treat metadata that cannot be read — a corrupt row in a recognized store — as not current, and test that a corrupt environment row drives the build to analyze rather than abort.
- [x] R.3 Narrow the one-pass requirement to what the counters prove: each analyzed document parsed at most once, parse work not growing with symbols; scenarios and test names aligned.
- [x] R.4 Correct both `Serves:` backlinks to the declared story `rebuild-costs-what-changed`.
- [x] R.5 Reconcile the proposal's out-of-scope wording with the delta contract: query answers are unchanged; `build`'s own outcome report is the mandated exception.
- [x] R.6 Test the outcome report: the human skip line names the index current and `--force`, and the machine projection carries `rebuilt` on both outcomes.
- [x] R.7 Compose the MCP `build` tool's displayed command from its effective arguments, and test that a forced call displays `c10r build --force`.
- [x] R.8 Test the MCP skip pass-through and force forwarding: an unchanged workspace reports `rebuilt: false` through the tool, and `force: true` drives a rebuild.
- [x] R.9 Correct the design's peak-memory risk: retention grew from the index's documents to every discovered source; the retention-restriction follow-up is recorded in `discussion.md`.

## Verify remediation (2026-08-20)

`sdd-verify` found no CRITICAL and two SUGGESTIONs, plus a documentation gap.
All three are addressed below.
Two working-tree changes it flagged as outside this change's scope stand by user decision: the `embed.rs` padding line belongs to `semantic-chunking` and must not travel in this change's commit train, and the `tests/test_uv_security_audit.py` removal is a manual edit that rides along.

- [x] V.1 Document the currency check in `README.md`: what a build compares before analyzing, what a skip reports, the `rebuilt` field, `--force`, and what drives a build rather than a skip.
  Correct the MCP section's build-cost prose, which described every build as potentially minutes long.
- [x] V.2 Assert the whole outcome of the handoff scenario, not only that the build did not skip: drive `build` through the built binary over a store recorded at another root, and check that it succeeds, discloses the handoff naming both roots on stderr, and leaves the store recording the new workspace root.
- [x] V.3 Carry the declaration list on `PreparedDocument` alongside the name-span index, so the join's syntax-only accounting reads the prepared list instead of walking the tree a second time per document.
- [x] V.4 Cover the second arm of the requirement's no-compatible-store clause, which only the absent-store arm exercised: give the requirement a scenario for a store at an unrecognized schema version, and test that a build over one analyzes and replaces it rather than reporting it current.

## 8. Close out

- [x] 8.1 Re-measure the full sequence on all five dogfood repositories and record the figures, peak heap included.
- [x] 8.2 Run the full test suite and every hook over the change.
- [x] 8.3 Remove the investigation instruments from `._scratch/` that the change no longer needs, keeping any the recorded measurements still depend on. (Resolved as: nothing removable — every instrument is depended on by this change's or `semantic-chunking`'s recorded measurements; see `discussion.md`.)
