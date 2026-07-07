# Tasks: blast-radius

## Schema & Edge Model (contract-defining)

- [x] Rename the stored edge kind `calls` to `uses`: `EdgeKind::Calls` → `EdgeKind::Uses`, tag `'uses'`, schema comment updated to state the reference-grade meaning.
- [x] Drop the `edges.verified` column and `EdgeKind::is_verified`; all four persisted kinds are contracted by this change.
- [x] Add a uniqueness constraint on `edges (kind, src_id, dst_id)` and make edge insertion idempotent, so one relation instance persists regardless of how many occurrences produce it.
- [x] Bump `SCHEMA_VERSION` to 5.
- [x] Test: building the same fixture twice yields identical, deduplicated edge rows (unique per kind/src/dst), and the full existing suite passes under the rename (regression for the store-replacement transition is already covered by the calibration-hardening tests).

## Edge Derivation — uses and imports (write side)

- [x] Point the existing reference-derivation at the renamed kind: an aligned reference attributed to an enclosing declaration inserts a `uses` edge; an aligned reference attributed to the module inserts an `imports` edge (mechanically the current logic, under the new name and dedup).
- [x] Test (uses — call): a fixture function calling another yields a `uses` edge from caller to callee.
- [x] Test (uses — reference grade): a fixture function that only mentions a type in its body yields a `uses` edge from the function to the type.
- [x] Test (refusal guard): an occurrence refused by the join (e.g. a macro-fallout mismatch in the fixture) contributes no dependency edge.
- [x] Test (imports): a `use` statement in a fixture module yields an `imports` edge from the module to the named symbol.
- [x] Test (imports boundary): a reference inside a function yields no `imports` edge from the containing module.

## Edge Derivation — type_hierarchy (write side)

- [x] Change `SyntaxTree::trait_impls` to return the name-token spans of the trait and the type for each `impl Trait for Type` block — the identifier inside a generic type (`From<DetailArg>` → `From`'s token), the terminal segment of a qualified path (`fmt::Display` → `Display`'s token), and the bare identifier otherwise.
- [x] Derive `type_hierarchy` edges by resolving each name-token span through the aligned occurrence at exactly that location; when no aligned occurrence exists at the span, skip the edge (never fall back to name matching).
- [x] Test (plain impl): a fixture `impl WorkspaceTrait for WorkspaceType` yields an edge from the type to the trait.
- [x] Test (generic impl): a fixture `impl From<Arg> for WorkspaceType` yields an edge from the type to the `From` symbol — the previously dropped case.
- [x] Test (external trait): a fixture `impl Default for WorkspaceType` yields an edge from the type to the external trait's persisted symbol.
- [x] Test (skip on missing occurrence): an impl block whose trait name-token has no aligned occurrence yields no edge and no fabricated identity.

## Dependents Traversal (query core)

- [x] Implement the dependents computation as a recursive CTE over `edges`, walking `dst → src` across `uses`/`imports`/`type_hierarchy`, tracking hop depth, capped at the horizon constant (20), excluding the seed from its own results.
- [x] Group traversal rows per symbol at minimum depth, with the connecting edge kind taken from a shortest-depth hop under the fixed tie-break (kind order, then canonical identity), and order results by (depth, kind, identity).
- [x] Test (direct): a function using the seed appears at distance 1 with kind `uses`.
- [x] Test (transitive): a function two hops away appears at distance 2.
- [x] Test (via imports): a module importing the seed appears with kind `imports`.
- [x] Test (via type_hierarchy): a type implementing the seed trait appears with kind `type_hierarchy`.
- [x] Test (enclosure excluded): the seed's containing module is not reported as a dependent by containment alone.
- [x] Test (shortest distance): a symbol reaching the seed both directly and through an intermediate appears once, at distance 1.
- [x] Test (cycle): two mutually-using functions terminate, each reported at most once.
- [x] Test (determinism): repeated identical dependents queries return byte-identical ordering.

## CLI Surface

- [x] Add the `dependents` relation to `trace`, with a `--depth` flag defaulting to 1; supplying `--depth` with any other relation fails with a teaching error naming the flag, the relation, and the relations that accept it.
- [x] Render the dependents answer: detail rows (canonical identity, display name, edge kind, distance, location) up to the bound; beyond-bound aggregate counts by edge kind and depth; a horizon disclosure distinguishing reach-ends-within-bound, reach-beyond-bound, and aggregate-cut-at-horizon; all inside the existing calibrated envelope (provenance, freshness, typed absence, `--json`).
- [x] Write the `dependents` self-description as impact assessment — "what could break if this symbol changes" — including the reference-grade caveat, in the command help.
- [x] Add a deep-chain fixture (a chain of functions longer than the horizon) to exercise the horizon disclosure.
- [x] Test (CLI dependents): `trace <symbol> dependents` returns direct dependents with kind and distance labels.
- [x] Test (ends within bound): a subject whose network ends inside the requested depth reports no further reach.
- [x] Test (beyond bound): a subject with deeper dependents reports detail to the bound plus aggregate counts by kind and depth.
- [x] Test (horizon disclosure): on the deep-chain fixture, the answer states the aggregate itself is bounded rather than presenting it as total reach.
- [x] Test (typed absence): a symbol with no dependents returns a definite empty answer, distinct from failure.
- [x] Test (teaching error): `--depth` with `contains` fails with the teaching error.
- [x] Test (help framing): the command self-description presents `dependents` as impact assessment.

## Review Remediation (2026-07-05 code review)

- [x] Fix horizon disclosure to be computed independently of the detail/aggregate split, so a bound at or beyond the horizon still discloses the cut.
- [x] Test (boundary at horizon): `dependents_discloses_cut_when_depth_equals_horizon` — failed pre-fix, passes post-fix.
- [x] Test (bound beyond horizon): `dependents_discloses_cut_when_depth_exceeds_horizon` — failed pre-fix, passes post-fix.
- [x] Test (over-fire guard): `dependents_shallow_network_ends_within_bound_even_at_large_depth`.
- [x] Fix module-for-document mapping to cover every document holding an aligned module definition occurrence, so crate-root use statements produce `imports` edges.
- [x] Test (multi-document module): `shared_module_symbol_maps_to_every_document_it_defines` — failed pre-fix, passes post-fix; verified live (`src/main.rs` imports edges now present, crate-root imports 1 doc → all docs).
- [x] Deduplicate traversal rows inside the recursive query (`UNION`), bounding cost by graph size rather than path count.
- [x] Make `trace` with the dependents relation a typed teaching error instead of typed absence for library callers; test `trace_with_dependents_relation_errors_instead_of_empty`.
- [x] Replace the silent skip on a missing dependent symbol row with a typed store-corruption error naming the identity.
- [x] Pin `--depth 0` semantics (aggregate-only) with test `dependents_depth_zero_is_aggregate_only` and a help-text clause.
- [x] Document the one-token-per-location invariant (occurrence map) and the closed-edge-kind assumption (tie-break ordering) at their code sites.

## Dogfood & Ground Truth (captured evidence)

- [x] Rebuild the index on this repository with the new binary; capture `status` output (edge counts by kind, alignment accounting unchanged in shape).
- [x] Ground-truth `type_hierarchy`: hand-enumerate every `impl Trait for Type` block in this repository and compare against the derived edges; record the comparison (matches, designed skips, unexplained gaps) in the change record.
- [x] Ground-truth dependents: hand-check direct dependents for a small set of well-understood symbols (e.g. `CanonicalId`, `GraphStore`, one CLI arg type) against `trace ... dependents`; record the comparison.
- [x] Duplicate-ambiguous investigation: inspect the emitted SCIP index for descriptors with multiple definitions and determine whether rust-analyzer drops duplicate definitions upstream (the stayed-at-zero hypothesis); record the finding and, if the hypothesis is wrong, file the follow-up.
- [x] Dogfood evidence recorded in `dogfood.md` (2026-07-06 rerun against the remediated build); duplicate-detection follow-up filed in brainstorm §11 — hypothesis refuted, adapter merges twins before duplicate detection.
