# Proposal: precise-locate-rust

## Intent

Coding agents and the developers beside them burn context grepping unfamiliar Rust code and cannot trust unranked text matches, yet c10r's structural spine does not exist yet — there is no graph to query.
This change stands up that spine end-to-end on the smallest real slice: a persisted, unified syntax+semantic graph for **Rust**, keyed on a stable canonical identity, answering exact definition / reference / type-occurrence and enclosure queries over a CLI.
It dogfoods on this repository, and it proves the architecture's load-bearing risk — joining the syntax tree to the semantic index without silent misalignment — on one language before breadth is attempted.

## User Stories

### Story: locate-symbol

As an agent or developer, I want to find a Rust symbol's definition, references, and
type-occurrences exactly, so that effort goes to the task instead of the search.

_Ladders to north-star outcome 1 (Precise locate)._

### Story: understand-structure

As a developer or agent, I want to ask what encloses a position and what a scope contains, so that I
can orient in unfamiliar Rust code.

_Ladders to north-star outcome 3 (Structural understanding)._

### Story: trust-the-answer

As a consumer, I want every answer to carry its provenance and staleness, so that I calibrate how far
to lean on it rather than being misled by a confident wrong answer.

_Ladders to north-star outcome 5 (Calibrated trust)._

## Scope

**In scope:**

- A canonical, **workspace-namespaced**, deterministic symbol identity, validated against Rust's SCIP
  descriptors.
- The `SemanticEngine` **port contract floor** — the intersection every backend must satisfy, plus
  optional queryable capabilities via feature detection — with a Rust adapter
  (`rust-analyzer scip` + `tree-sitter-rust`) as the exemplar implementation.
- The AST↔SCIP **join**: range-containment after position-encoding normalization, text-equality
  guarded, content-hash gated.
- A persistent **SQLite-core** store; schema for symbols, occurrences, `contains` edges, and source
  spans, shaped to extend (future vectors, additional edge types) without redesign.
- Ingest **populates** all four base edge types (`contains`, `calls`, `imports`, `type_hierarchy`);
  this proposal **contracts and verifies** only what the queries below observe, plus two **write-site contracts**: every aligned reference occurrence records its enclosing declaration (the caller-attribution crossing `calls` edges will need), and each build records join-alignment accounting (aligned / text-mismatch / semantic-only / syntax-only counts).
- Two query meta-operations: `get` — retrieve a symbol (identified by name or source position) at a chosen detail level (location, signature, or full body), folding definition-lookup and enclosure-by-position into one operation along the detail axis; and `trace` — return the symbols standing in a given relation to a subject, over an open relation taxonomy named by a shared-root directional convention (`containers`/`contains`, later `callers`/`callees`, `importers`/`imports`).
  This change supports `containers`/`contains` (enclosure, upward and downward) and `references` (a type subject's references are its type-occurrences, so type-occurrence is not a distinct relation).
- The operational pair `build` (build or refresh the index) and `status` (report provenance, freshness, and the join-alignment counts); these verb names are carried over 1:1 from the prior tool's CLI under user-directed authorization, as is `get`.
- An on-demand **CLI** with a structured output contract: provenance, freshness/staleness, typed absence, deterministic ordering, `--json`; git-hook reindex doorbell.
- The Cargo crate scaffold and the baseline `specs/` contract floor for the capabilities above.

**Out of scope:**

- Blast-radius / impact — the transitive closure over dependency edges, and the correctness contracts
  for `calls` / `imports` / `type_hierarchy` (their queries observe them) — **proposal 2**.
- Any language other than Rust.
- Semantic / embedding search; the `sqlite-vec` vector columns and `FTS5` index (reserved by schema
  shape, not built) — its own change.
- The **MCP** surface — a later change (the CLI is the v1 renderer).
- Resident daemon and live structural / semantic overlays — later changes.
- The multi-repo **management surface** (workspace registry, cross-repo queries); only identity
  **namespacing** is in scope, so the surface is never designed out.

## Approach

_Mechanism sandbox — formalized in `design.md`._

- Two oracles per file: `tree-sitter-rust` for structure/enclosure (always fresh, error-tolerant) and `rust-analyzer scip` for cross-file identity/resolution/types (snapshot, build-gated).
  Behind the `SemanticEngine` port so Python and live-LSP backends slot in later.
- Identity as a normalized, workspace-namespaced projection of each SCIP symbol's descriptor, with a disambiguator appended only on an observed within-workspace collision (omitted for Rust, which does not overload).
  Resolution accepts three reference tiers along a user-facingness axis — identity (exact, machine), name (qualified, addressable), shortname (display) — and ambiguity returns typed candidates.
- Join: normalize SCIP occurrence ranges via each document's `position_encoding` to tree-sitter byte offsets, match by range containment to the AST name node, assert the bytes at the matched range equal the symbol's name token — the terminal segment of its descriptor (loud failure on drift) — and gate the whole index on a content hash of the analyzed sources.
  Aligned reference occurrences additionally record their nearest enclosing persisted declaration (the module as the outermost attribution), and the build accumulates the four join-outcome counts.
- SQLite schema: a symbols table (canonical id, kind, span), an occurrences table (symbol, range, role), a `contains` edge table; the dependency edges populated into the same edge table, tagged by type.
  Provenance = analyzer name+version as graph metadata; a version change marks the graph stale.
- CLI surface: two query meta-operations (`get`, `trace`) plus the operational pair (`build`, `status`).
  `get` carries a detail-level argument (location / signature / body — the §6b granularity knob); `trace` carries a relation argument over an open taxonomy, so later relations (calls, imports, inherits) and depth>1 (blast radius) extend the same verb rather than adding verbs.
  Every answer carries `{provenance, freshness, stale}` and renders empty results as typed absence, not "unavailable".

## Open Questions

- The signature detail tier for symbol kinds whose declaration is not distinct from their body (e.g. a unit struct) — whether such a `get` at signature detail falls back to the location or the body tier.
  A presentation edge to settle during implementation, not a contract change.
