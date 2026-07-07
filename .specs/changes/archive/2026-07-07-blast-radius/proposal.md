# Proposal: blast-radius

## Intent

The graph already stores the links that connect symbols to what they depend on — which declarations use which symbols, which modules import which symbols, and which types implement which traits — but those links were written without correctness contracts, and no query can read them.
The store's most valuable relationships are dead weight, and north-star outcome 2 (blast radius before change) is unserved: an agent about to modify a symbol cannot ask what depends on it.
This change contracts the dependency links' correctness (fixing a known recording gap for trait implementations with generic parameters along the way), and exposes one traversal query that answers the impact question: "if I change this symbol, what could break?"

## User Stories

### Story: impact-before-change

As an agent or developer about to modify a symbol, I want to see everything that depends on it — directly and transitively, to a depth I control — so that I can assess the impact of the change before making it instead of silently breaking distant code.

The query surface and its documentation MUST present this explicitly as impact assessment: `trace <symbol> dependents --depth N` is the "what could this change break" question, not merely a graph walk.

_Ladders to north-star outcome 2 (Blast radius before change)._

### Story: honest-horizon

As an agent or developer reading an impact answer, I want the answer to state how much further the dependency network extends beyond the depth I asked for, so that I never mistake "the query stopped looking here" for "the impact ends here."

_Ladders to north-star outcomes 2 (Blast radius) and 5 (Calibrated trust)._

### Story: trustworthy-dependency-edges

As an agent or developer relying on an impact answer, I want every stored dependency link written under a stated, verified rule — with honest naming (a link meaning "uses" is not called "calls") and without silently dropped trait implementations — so that the answer is complete and calibrated enough to lean on.

_Ladders to north-star outcomes 2 (Blast radius) and 5 (Calibrated trust)._

## Scope

**In scope:**

- **code-graph:** correctness contracts for the three dependency link kinds, until now populated but uncontracted:
  - `uses` — an enclosing declaration depends on a symbol its body references (reference-grade by design: mentioning a type or constant counts, not only function calls; renamed from the uncontracted `calls` kind to say what it means);
  - `imports` — a module depends on a symbol referenced at module scope;
  - `type_hierarchy` — a type implements a trait, including trait names carrying generic parameters (today silently dropped — the recording gap this change closes).
- **code-graph:** the dependents traversal contract — reverse traversal over the three dependency kinds; enclosure (`contains`) supplies attribution and never propagates impact; results are depth-bounded with per-result connection kind and hop distance.
- **code-navigation:** the `trace <symbol> dependents --depth N` query — detailed results up to the requested depth, each identifying the symbol, the kind of link that connected it, and its distance from the seed; beyond the requested depth, aggregate counts of further reach (by link kind and depth, up to a fixed internal horizon); the existing calibrated output contract (provenance, freshness, typed absence, deterministic order, `--json`) applies.
- **Verification rider:** ground-truth the duplicate-ambiguous accounting bucket, which has stayed at zero across both dogfood builds despite the semantic indexer's known duplicate emissions (hypothesis: the duplicated definitions are dropped from the emitted index upstream).

**Out of scope:**

- Splitting call-shaped links from reference-shaped links (`calls` vs `uses` classification) — deferred; additive later, per the edge-model decision.
- The forward direction (`dependencies` — what a symbol itself depends on) — no v1 story ladders to it; a symmetric addition later.
- Per-kind relation names (`importers`, `subtypes`, `callers`, …) — each result already labels its connection kind; sugar names belong to the deferred interface-taxonomy change.
- A diff-seeded blast-radius command (seeding from git changes) — excluded by the no-temporal-anchoring decision (2026-07-01); composes externally as diff → touched symbols → this query.
- The `is-tested` derived view — drags in the indexing-scope-includes-tests decision; its own change.
- The MCP surface, pagination/cursor model, and any bounded-output machinery beyond the depth bound and horizon aggregate — deferred per brainstorm §11.

## Approach

_Mechanism sandbox — formalized in `design.md`._

- Rename the stored link kind `calls` → `uses`; no baseline contract names it yet, so this is a fresh contract, not a migration of one.
  Schema version bumps; the store-replacement contract from calibration-hardening makes rebuild the migration.
- `type_hierarchy` evidence source is a design decision to resolve: either fix the syntax-based matcher to compare trait names with generic parameters stripped, or switch evidence to the semantic index's own implementation relationships if rust-analyzer's SCIP output carries them (to be checked against our emitted index during design, not assumed).
- Traversal as a recursive query in SQLite over the three dependency kinds in reverse, tracking hop distance; detail rows cut at `--depth`; traversal continues to a fixed internal horizon whose beyond-depth reach is aggregated by kind and depth.
  The horizon is a provisional design constant, like the discrepancy-listing group cap, until the pagination change lands.
- Deduplication within a traversal (a symbol reachable by several paths) reports the shortest hop distance; ordering deterministic.
- Dogfood verification on this repository: hand-checked ground truth for direct dependents of a small symbol set, plus the duplicate-ambiguous investigation.

## Open Questions

- None blocking; the `type_hierarchy` evidence-source choice is deliberately deferred to `design.md` after inspecting the emitted SCIP data.
