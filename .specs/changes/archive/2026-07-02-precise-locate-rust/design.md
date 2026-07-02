# Design: precise-locate-rust

## Context

This is the project's first change, so it stands up structure later changes assume: the baseline `specs/` contract floor, the Cargo crate, and the persistent store.
It is a vertical slice — the thinnest end-to-end path that delivers a user-facing outcome (precise locate for Rust) while forcing the foundational decisions (identity, the two-oracle join, the storage substrate, the engine port) to be made and verified against a real query rather than in the abstract.

Constraints inherited from the north star and the design brainstorm (`._scratch/silent-cartographer-design-brainstorm.md`):

- **Calibration over coverage** — the product dies if it lies with confidence; an honest "stale" or "ambiguous" keeps a consumer, a confident-wrong answer loses one permanently.
- **One identity, two oracles** — a single stable identity; the syntax tree owns structure/enclosure, the semantic index owns cross-file identity/resolution/types.
- **Adopt over build** — reuse `tree-sitter-rust` and `rust-analyzer scip`; c10r builds the unification, the graph, and the surface.
- **Storage is SQLite-core** — the vector extension and FTS5 are deferred; the schema reserves room for them but may take an additive migration when the semantic pillar lands.
- **Clean-room wall** — the design is reasoned independently of the prior tool; the verbs `get`, `build`, `status` are carried over 1:1 under explicit user authorization, and `trace` plus its relation taxonomy are independently designed.

## Decisions

### Decision: Identity as a normalized projection of the SCIP descriptor

**Chosen:** The canonical identity is a deterministic, workspace-namespaced projection of each symbol's SCIP descriptor, of the conceptual form `<workspace>::<module_path>::<qualname>[disambiguator]`.
A disambiguator is appended only when the projection would otherwise collide with another symbol in the same workspace (parse-don't-validate: respond to an observed collision, not a hypothetical one).
Because Rust has no function overloading, Rust projections do not collide and carry no `@arity` suffix; the suffix slot is reintroduced per-language when an overloading language (e.g. C#) lands.

**Rationale:** SCIP symbol strings are already globally unique by construction, so leaning on the descriptor gives stability and determinism for free; normalizing to our own form keeps the identity engine-neutral so a future Python or LSP backend produces comparable keys.
Collision-only disambiguation avoids carrying dead suffixes on languages that do not need them.

**Alternatives considered:**

- Adopt the raw SCIP symbol string verbatim as identity: rejected — it is engine-specific and would not normalize across future backends.
- Always append `@arity` for cross-language uniformity: rejected — vacuous on Rust, and arity is too weak a disambiguator for type-overloading languages anyway (the real disambiguator is the SCIP descriptor's own).

### Decision: Three tiers of reference — identity, name, shortname

**Chosen:** A symbol is addressable at three tiers along a user-facingness axis that runs inversely to uniqueness:

```text
            uniqueness  user-facing  example                         role
identity    exact       low          <ws>::crate::net::Client::connect  machine round-trip key, join key, JSON
name        usually 1   medium       crate::net::Client::connect        addressable path a human/agent types
shortname   low         high         connect                            display label, half-remembered lookup
```

Input resolution accepts any tier and resolves _up_ toward identity; an ambiguous lower tier (typically a shortname) yields a typed candidate set rather than an arbitrary choice.
Every result renders each symbol with its identity (source of truth) and a human-readable name; the human view prefers the name/shortname, the JSON always carries identity.

**Rationale:** This is the "machine-readable answer is source of truth, human view is a render" principle made concrete, and it keeps the trust contract honest — ambiguity is surfaced, never silently resolved.
It also separates two axes that were entangled: _how you refer to a symbol_ (this tier model) versus _how much of it you retrieve_ (the `get` detail knob).

**Alternatives considered:**

- Single addressing form (require the qualified name): rejected — humans work from shortnames they half-remember; agents want exact identity round-trips.
  One form serves neither end well.
- Silently pick the first match on ambiguity: rejected — a confident wrong target is precisely the calibration failure the product cannot afford.

### Decision: The two-oracle join

**Chosen:** Per file, `tree-sitter-rust` provides structure and enclosure (always fresh, error-tolerant, no build) and `rust-analyzer scip` provides cross-file identity, resolution, and types (snapshot, build-gated).
They are joined by mapping each SCIP occurrence's range — normalized via the document's declared position encoding — onto the tree-sitter byte offsets, matching by range containment to the syntactic name node, and asserting the source bytes at the matched range equal the symbol's expected name token — the terminal segment of its descriptor, since a SCIP range covers the name token, not the qualified path.
A mismatch is surfaced and the occurrence is not persisted as an aligned attribution.
The whole index is gated on a content hash of the analyzed sources.

**Rationale:** The join is the project's load-bearing risk; the text-equality guard turns the silent failure mode (coordinate/encoding drift) into a loud one, and the content-hash gate prevents joining a fresh tree against a stale index.

**Alternatives considered:**

- Collapse to SCIP only: rejected — most indexers omit enclosure information, and enclosure is exactly what structural queries need.
- Collapse to tree-sitter only: rejected — no cross-file identity or types.
- Stack-graphs (single-source syntax+resolution): rejected — archived/dead upstream.

### Decision: SemanticEngine port at the intersection, with feature detection

**Chosen:** The port's mandatory contract is the intersection every backend (batch SCIP now, live LSP later) can satisfy: produce symbols with resolved descriptors and role-classified occurrences carrying mappable ranges, plus analyzer provenance.
Capabilities only some backends offer — enclosure information, base-index eligibility, live incremental updates — are exposed as queryable feature detection, never assumed.
A backend conformance suite encodes the mandatory contract; the Rust adapter is the first implementation to pass it.

**Rationale:** This is the Liskov substitution discipline for backends — any conformant backend is interchangeable without callers observing a behavioral change — and the conformance suite makes the otherwise-abstract substitutability claim testable with a single backend today.

**Alternatives considered:**

- Define the port at the union of SCIP and LSP capabilities: rejected — only some backends would satisfy it, breaking substitutability.

### Decision: SQLite-core schema with reserved extension points

**Chosen:** A single embedded SQLite store holds a symbols table (canonical identity, kind, span), an occurrences table (symbol, range, role), and an edges table tagged by relation type.
Ingest populates all four base relations (`contains`, `calls`, `imports`, `type_hierarchy`); only `contains` and occurrence-derived `references` are contracted and verified this change.
Provenance (analyzer name + version) and the source content hash are stored as index metadata; either differing from what is in effect marks results stale.
Schema shape reserves room for vector columns and an FTS5 index without dictating their form.

**Rationale:** One store covers graph traversal (recursive CTEs), keyword (FTS5 later), and vectors (sqlite-vec later) without a second system; populating all edges now is cheap plumbing that saves rework, while withholding their contracts keeps every verified requirement tied to a story.

**Alternatives considered:**

- Persist only `contains`: rejected by the user in favor of populating all four now.
- Commit the vector column shape now: rejected — the shape is unknown until the semantic pillar is designed; reserving it concretely risks the wrong shape.

### Decision: CLI surface — two query meta-operations plus an operational pair

**Chosen:** `get` retrieves a symbol (referenced by name, shortname, position, or identity) at a detail level (location / signature / body); `trace` returns symbols in a named relation to a subject, over an open taxonomy named by a shared-root directional convention (`containers`/`contains`, later `callers`/`callees`, `importers`/`imports`).
This change ships `containers`, `contains`, and `references`.
`build` (re)builds the index; `status` reports provenance and freshness.
Every answer carries the output contract (identity + name, provenance, freshness, typed absence, deterministic order, `--json`).

**Rationale:** Collapsing per-query verbs onto two axes (detail, relation) matches the §6b query contract and lets future relations and depth>1 (blast radius) extend `trace` rather than adding commands.
The shared-root convention optimizes for recognizability over grammatical uniformity (`callees`, `supertypes`, `containers` are all recognizable; forced uniformity would invent non-words).

**Alternatives considered:**

- A flat verb per query (`def`, `refs`, `type-uses`, `enclosing`, `children`): rejected — it hard-codes a closed relation set and ignores the detail and relation axes.
- Base relation + `--reverse` flag for direction: rejected — a modal flag is less discoverable than explicit directed names.

### Decision: Dependency edges populated but uncontracted

**Chosen:** `calls`, `imports`, and `type_hierarchy` edges are written during ingest but carry an "unverified until proposal 2" marker in the store and are exposed by no query this change.

**Rationale:** Their correctness contracts require enclosure attribution (the meatiest join surface) and are only observable once a query traverses them; deferring the contracts to proposal 2 keeps this change reviewable while saving the population plumbing.
The marker prevents a future consumer from trusting them before their contract exists.

### Decision: Caller attribution contracted at the write-site

**Chosen:** Every aligned reference occurrence records the nearest enclosing persisted declaration in the fresh syntax tree — the module when no narrower declaration encloses it, the declaring function when the occurrence sits inside a closure (closures are not persisted symbols).
The attribution is contracted and tested this change; the `calls` edges built from it remain uncontracted-for-reading until proposal 2.

**Rationale:** A `calls` edge is exactly this composition — a reference occurrence (semantic oracle) crossed with the enclosure chain (syntax oracle) — and the population code is written this change, so leaving the composition unspecified would let proposal 2 inherit whatever the code happened to do.
Contracting the write-site keeps this crossing deliberate without pulling caller/callee queries into scope.

**Alternatives considered:**

- Defer the attribution contract wholly to proposal 2: rejected — the code exists this change and would be written spec-blind.
- Contract `callers`/`callees` queries now: rejected — scope creep; the read-side belongs to proposal 2 with blast radius.

### Decision: Ingest policy for local and external symbols

**Chosen:** File-local symbols (parameters, let-bindings — SCIP symbols with no global descriptor) are excluded from the persisted base; they cannot be referenced cross-file, so the base graph loses nothing, and they remain overlay territory for later changes.
External symbols (resolved descriptors with no in-workspace definition — standard-library and third-party items) are persisted as a distinct symbol class carrying identity and reference occurrences but no definition span; retrieving one yields a typed "external — no source in this workspace" rather than an error.

**Rationale:** The canonical identity is a pure function of the resolved descriptor, and a total function needs a defined domain — locals and externals are the two descriptor shapes the projection would otherwise meet undefined.
Keeping externals as first-class reference targets preserves the callee identity that future `calls` edges into third-party code will need.

**Alternatives considered:**

- Fabricate canonical identities for locals: rejected — they have no module path or qualname to project and no cross-file meaning.
- Drop external references entirely: rejected — it silently narrows `references` results and discards the callee end of future third-party call edges.

## Architecture

```text
  Rust sources
       │
       ├────────────────────────┐
       ▼                        ▼
 tree-sitter-rust         rust-analyzer scip
 (structure, enclosure,   (identity, resolution,
  spans; always fresh)     types; snapshot)
       │                        │
       └──────────┬─────────────┘
                  ▼
          Guarded positional join
   (encoding-normalized range containment,
    text-equality guard, content-hash gate)
                  │
                  ▼   ── via SemanticEngine port (Rust adapter) ──
          SQLite-core graph store
   (symbols · occurrences · edges[contains|calls|imports|type_hierarchy]
    · spans · provenance · content-hash · alignment counts)
                  │
                  ▼
            Query engine
   (reference resolution: shortname|name|position|identity → identity;
    get[detail], trace[relation])
                  │
                  ▼
        CLI  (get · trace · build · status)
   output: {identity, name, provenance, freshness, stale, typed-absence}  + --json
```

## Risks

- **Encoding/coordinate drift between the oracles** — the classic silent misalignment.
  Mitigated by the text-equality guard (loud failure) and content-hash gating, both directly tested, including a non-ASCII case.
- **Guard miscalibration** — a guard that compares the wrong string (the qualified name instead of the name token) fails pervasively, gets demoted to a warning under implementation pressure, and dies looking like a fix.
  Mitigated by fixing the comparison target (the descriptor's terminal name segment) in the contract and tasks up front.
- **Macro-expanded occurrences inflate the unaligned bucket** — rust-analyzer indexes expanded code that has no node in the written source, so a visible semantic-only rate on real Rust is expected and benign.
  Mitigated by the join-alignment accounting, which makes that baseline measurable instead of alarming, so the join is not "fixed" into real misattribution.
- **`rust-analyzer scip` build cost and memory** — indexing is build-gated and can be memory-heavy on large crates.
  Mitigated by scoping v1 to on-demand indexing and dogfooding on this repository first.
- **Substitutability is unexercised with one backend** — the port's interchangeability claim cannot be fully demonstrated until a second backend (Python, proposal 2+) exists.
  Mitigated by encoding the mandatory contract in a conformance suite the Rust adapter passes, so the contract is concrete even with one implementor.
- **Ambiguous reference resolution** — shortname collisions could frustrate users if candidate sets are large.
  Mitigated by returning typed candidates (never a silent pick) and letting callers narrow by qualified name or identity.

## Verification Waivers

None — every SHALL in the delta specs maps to runnable evidence (see `tasks.md`).
The substitutability concern is covered by the backend conformance suite rather than waived.
