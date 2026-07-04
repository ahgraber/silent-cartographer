# Proposal: calibration-hardening

## Intent

The first live index of this repository exposed three calibration gaps in the shipped structural spine.
True-duplicate symbols from the semantic indexer (a known rust-analyzer defect) are disambiguated by discovery order, so distinct definitions can swap identities between runs and their references cannot be trusted.
The join records only aggregate outcome counts, so a 10.7% text-mismatch rate cannot be decomposed into benign macro fallout versus real drift — the exact confident-ignorance the product exists to prevent.
And the default workspace identity is the literal string `workspace`, which silently defeats the day-one namespacing insurance the moment a second codebase is indexed.
The second live rebuild (with the discrepancy listing) added a fourth gap: ~12.6% of semantic occurrences are refused by the name-token guard even though they are structurally *true* resolutions — operator desugaring (`?`→`branch`, `[`→`index`), external crate roots (`use std::` resolving to a `crate/` descriptor), whole-file module spans — so `references` (and soon `callers`) silently under-report, the false-negative calibration failure the north star forbids.
This change closes all four before blast-radius contracts are built on top of the affected edges.

## User Stories

### Story: stable-identity-under-duplicates

As an agent or developer querying the graph, I want distinct definitions to keep distinct, stable, meaningful identities even when the semantic indexer emits duplicate symbols for them, so that queries never conflate two definitions or shuffle their identities between runs.

_Ladders to north-star outcomes 1 (Precise locate) and 5 (Calibrated trust)._

### Story: diagnosable-join

As a human or agent operating c10r, I want every occurrence the join refused to attribute to be inspectable after a build — what was expected, what was found, where — so that I can distinguish benign macro fallout from real misalignment instead of guessing from a single count.

_Ladders to north-star outcome 5 (Calibrated trust)._

### Story: recoverable-index-state

As a user upgrading c10r, I want an existing index the new binary cannot use to be replaced or refused with clear guidance, so that a bad stored state is one rebuild away from fixed instead of a mid-operation storage error.

_Ladders to north-star outcome 5 (Calibrated trust); realizes the "trivial recovery when stored state goes bad" candidate story from the 2026-07-01 clean-room review (brainstorm §11)._
_Added 2026-07-02 after the live rebuild hit exactly this failure._

### Story: rule-typed-alignment

As an agent or developer querying references and, soon, callers, I want occurrences whose truth is not name-shaped — operator desugaring, crate roots, whole-file modules — attributed under named, deterministic alignment rules, so that queries stop silently under-reporting and the alignment figure is one I can lean on rather than explain away.

_Ladders to north-star outcomes 1 (Precise locate), 2 (Blast radius — callers feed it), and 5 (Calibrated trust)._
_Added 2026-07-02 after the second live rebuild decomposed the refusal set._

### Story: workspace-scoped-by-default

As a user running c10r across several codebases, I want each graph namespaced by its own workspace identity without passing a flag, so that symbols from different codebases never collide even when I never thought about namespacing.

_Ladders to north-star outcome 5 (Calibrated trust) and the multi-codebase addressing decision (brainstorm §11)._

## Scope

**In scope:**

- **symbol-identity:** duplicate-descriptor disambiguation becomes deterministic and definition-anchored — ranked by where each twin is defined, not by discovery order (MODIFIED: Identity uniqueness within a workspace).
- **symbol-identity:** the default workspace identity is derived deterministically from the workspace root's name; an explicit `--workspace` overrides; collision policing between same-named roots is assigned to the future multi-workspace management surface, not this default (ADDED).
- **code-graph:** reference occurrences of a duplicated descriptor are never silently attributed to one twin — the ambiguity is typed and counted, per the calibration principle (ADDED).
- **code-graph:** join discrepancies (text-mismatch, semantic-only, and duplicate-ambiguous outcomes) persist with inspectable detail — expected name, found source text, location — retrievable after the build through the CLI; the default listing is a bounded, grouped summary computed over the full set with typed truncation, and an explicit switch returns everything (ADDED).
- The accounting conservation contract extends to cover any new outcome category exactly.
- **code-graph:** the store records its schema version and validates it at open — `build` replaces an incompatible store (the index is derived, replayable data; rebuild is the migration), while queries refuse with a typed teaching error naming both versions and the recovery action (ADDED).
- **code-graph:** alignment becomes rule-typed — name-token equality is the default rule, joined by four kind-scoped exact rules (crate-root package name, desugared-operator constructs over a closed correspondence, whole-document module definitions, and self-type keywords cross-checked against the enclosing implementation); every acceptance carries its rule as provenance, anything matching no rule stays refused, and the accounting gains per-rule buckets (MODIFIED: Guarded positional join; MODIFIED: Join alignment accounting).

**Out of scope:**

- Correctness contracts and queries for `calls` / `imports` / `type_hierarchy`, the depends-on closure, and blast radius — the next change.
- Automatic categorization heuristics over discrepancies (module-span vs. derive-fallout classifiers) — the raw detail this change persists is the substrate; classification can layer on later if reading the detail proves insufficient.
- The multi-workspace management surface and multi-row index metadata — deferred per brainstorm §11; this change only fixes the default identity.
- Any upstream fix or workaround for rust-analyzer's duplicate emission itself — we harden against it, we do not patch it.
- Attribution of macro-expansion fallout to its invocation site (`via-expansion` provenance) — the honest fix for the derive-generated residual is its own design decision; that residual stays a typed refusal this change.
- Any fuzzy or heuristic matching — every alignment rule is an exact, deterministic check against a kind-correct expectation.

## Approach

_Mechanism sandbox — formalized in `design.md`._

- Rank each duplicate-descriptor group by (definition document path, definition range) instead of input index; the disambiguator suffix stays, but its assignment becomes stable under any discovery order and meaningful (path-ordered).
  Definition occurrences attach to their own twin by co-location with the ranked definition.
- Non-definition occurrences of a duplicated descriptor are genuinely unattributable between twins from SCIP data alone; they are recorded as a typed `duplicate-ambiguous` join outcome — a new accounting bucket — rather than being assigned to an arbitrary twin.
  The conservation law extends: aligned + text-mismatch + semantic-only + duplicate-ambiguous = total semantic occurrences.
- Discrepancy detail persists to a per-build table (document, range, outcome kind, expected name token, found source text truncated to a bounded length), replaced on each build; `status` gains a discrepancy listing mode.
  The default listing aggregates over persisted fields (GROUP BY outcome kind and expected token, ordered by descending count, one exemplar location per group) so the bounded view gists the whole set; this is plain aggregation, not the deferred semantic classifiers.
  `--all` returns raw rows; the group cap is a provisional design constant until the interface-layer pagination change lands.
- Alignment rules: the join dispatches each occurrence to a named rule — default name-token equality; crate-root (descriptor terminal `crate` accepts the descriptor's own package name or the `crate` keyword); operator-desugar (a closed method→construct table, enumerated in design.md; the construct at the occurrence's location is resolved via the syntax tree because rust-analyzer's operator spans can sit adjacent to the sigil); module-span (module definition whose range is the whole document).
  The self-keyword rule (added 2026-07-04 from the second dogfood's ~50 `Self` refusals) accepts a type reference at a `Self` token only when the enclosing impl's self-type base name equals the expected base name, generics stripped — the impl-header cross-check keeps it exact where token presence alone would not be.
  Acceptances persist a rule tag; accounting buckets go per-rule; the expected live effect is ~970 refusals dropping to a mostly macro-fallout residual (~200), verified by the user's rebuilds.
- Default workspace identity (RESOLVED 2026-07-02): the workspace root's directory name, verbatim — readable, portable across clones and machines, deterministic from the root path alone (no environment probing).
  Remote-derived codebase identity and worktree kinship are deliberately deferred to the multi-workspace management change (see brainstorm §11), where "same codebase, different checkout" can be modeled as a relationship instead of crammed into one string; collisions between same-named roots are policed there, at registration time.
