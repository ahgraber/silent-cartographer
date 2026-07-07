# Design: blast-radius

## Context

The structural spine (precise-locate-rust, calibration-hardening) already writes dependency edges during a build, but under placeholder semantics: the kind now renamed `uses` was stored as `calls`, every dependency edge carries a `verified = 0` marker meaning "populated but uncontracted," and no query reads them.
This change gives those edges contracts and a read path.
Constraints inherited from the spine: the join only produces aligned occurrences under named rules (edges must derive exclusively from those), the store is derived and replayable (schema migration = rebuild, per the store-replacement contract), and the calibrated output contract governs every CLI answer.

An empirical result fixed one open question: rust-analyzer's emitted SCIP index for this repository contains **zero** `SymbolInformation.relationships` entries (checked 2026-07-05 against `index.scip`, 2,180 document symbols).
The semantic indexer offers no implementation-relationship evidence for Rust, so trait-implementation edges must come from our own syntax analysis joined to SCIP identities.

## Decisions

### Decision: Rename `calls` to `uses` and retire the `verified` column

**Chosen:** the stored edge kind `calls` becomes `uses`; the `edges.verified` column is dropped; `SCHEMA_VERSION` bumps 4 → 5.

**Rationale:** the edge is reference-grade (61% of its instances in the live index point at non-callable symbols — types, constants, macros), and the product's naming must say what the data means.
The `verified` column existed solely to mark the uncontracted kinds; after this change all four persisted kinds are contracted, so the marker is dead weight.
The store-replacement contract makes the migration a rebuild — no data migration code.

**Alternatives considered:**

- Keep `calls` and document the reference grade: rejected — a name that requires a footnote to not mislead is the confident-lie failure in miniature.
- Flip `verified` to 1 instead of dropping it: rejected — a column with one possible value carries no information.

### Decision: Deduplicated dependency edges

**Chosen:** an edge is a relation instance, unique on `(kind, src, dst)`; repeated references from the same declaration to the same symbol produce one edge.
Occurrences continue to carry the individual sites.

**Rationale:** the live index holds 8,562 `uses` rows for far fewer distinct relations; traversal semantics are set-shaped (a dependent either is or is not reached), and the per-site detail already lives in the occurrences table.
Uniqueness is enforced by the schema (unique index), so every write path — initial build and any future incremental path — inherits it.

### Decision: Trait-implementation edges from name-token occurrences, not name matching

**Chosen:** `trait_impls` in the syntax layer returns the **name-token spans** of the trait and type in each `impl Trait for Type` block (the identifier inside `From<DetailArg>` or the terminal segment of `fmt::Display`), instead of the nodes' raw text.
Edge derivation then resolves each span through the aligned occurrence at exactly that location, yielding the canonical identity SCIP assigned there.

**Rationale:** this closes the generic-parameter gap by construction — the occurrence sits on the name token, so `From<DetailArg>`, `fmt::Display`, and `Bar<T>` all resolve without any string surgery — and it eliminates the ambiguity of display-name lookup (two symbols sharing a name cannot be confused; the occurrence carries the exact identity).
It is also authority-by-competence: syntax attests "this is an implementation block," the semantic index attests which trait and type are named.

**Alternatives considered:**

- SCIP `relationships` with `is_implementation` (the semantically cleanest source): rejected on evidence — rust-analyzer emits none.
- Normalize the raw text (strip `<...>`, take the last `::` segment) and match by display name: rejected — collisions between same-named symbols would be resolved arbitrarily, exactly the guess-instead-of-refuse behavior the calibration principle forbids.

**Accepted limitation (documented, not fixed here):** derive-generated implementations (`#[derive(Debug)]`) produce no `impl` block in written syntax, so they yield no `type_hierarchy` edge.
Hand-written implementations of workspace traits — the case that matters for in-workspace impact — are covered.
If an aligned occurrence is absent at a name-token span (e.g. the occurrence was refused), the edge is skipped, never guessed.

### Decision: Module attribution covers every defining document

**Chosen:** the module-for-a-document mapping that anchors `imports` edges is built from every aligned module definition occurrence, so a module symbol defined in several documents anchors module-scope references in each of them.

**Rationale:** rust-analyzer emits one shared module symbol for every crate root — the bin target, the lib target, and each integration-test file — each root carrying its own definition occurrence in its own document.
Mapping only one document per symbol silently dropped every other crate root's use statements (found in review; `src/main.rs` had zero imports edges).
The shared identity across crate roots is an upstream duplicate-descriptor behavior; its identity-level handling stays with the dogfood duplicate-ambiguous investigation task.

### Decision: Traversal as a recursive SQL query with depth tracking

**Chosen:** dependents are computed by a recursive CTE over the `edges` table, walking `dst → src` across the three dependency kinds, tracking hop depth, capped at the internal horizon; the recursion deduplicates its rows (`UNION`), so a symbol re-reachable along many paths is expanded once per distinct depth and kind and cost is bounded by graph size, not path count; results are grouped per symbol taking the minimum depth, with the connecting edge kind taken from a shortest-depth hop (ties broken by fixed kind order, then identity order, for determinism).
The seed itself is never reported as its own dependent, so cycles terminate at the cap and self-loops (a recursive function using itself) are inert.
Detail rows are those at depth ≤ the requested bound, ordered by (depth, kind, canonical identity); rows beyond the bound aggregate to (kind, depth, count).

**Rationale:** the graph lives in SQLite; a recursive CTE keeps traversal in the store with no new machinery (the Phase-0 substrate decision anticipated exactly this).
Minimum-depth grouping implements the shortest-distance contract; the fixed tie-break preserves the deterministic-ordering clause of the calibrated output contract.

**Alternatives considered:**

- Loading edges into memory and walking in Rust: workable, but duplicates what the store does natively and adds a second traversal implementation to keep consistent later (daemon, MCP).

### Decision: Internal horizon and depth default

**Chosen:** the traversal horizon is a fixed design constant of **20 hops**; `--depth` defaults to **1** when omitted.

**Rationale:** the horizon bounds the aggregate the same way the discrepancy-listing group cap bounds that summary — a provisional constant until the pagination change lands; 20 exceeds any plausible real dependency chain while keeping the recursive query trivially cheap at this graph's size.
A depth default of 1 makes the bare query the safe, cheap "direct dependents" answer, while the beyond-bound aggregate immediately shows how much deeper the network runs — the honest horizon itself teaches the user to reach for `--depth`.
A depth of 0 is a valid bound — aggregate-only, no detailed rows — documented in the command help and pinned by test.
The horizon-cut disclosure is computed independently of the requested depth: a dependent at the horizon means deeper reach may exist unexplored, and that must be disclosed even when the requested bound equals or exceeds the horizon (found in review — the original implementation could report "ends within bound" for a truncated walk).

**Alternatives considered:**

- No default (require `--depth`): friction without a calibration payoff, since the aggregate already prevents a shallow answer from masquerading as the whole story.

### Decision: `--depth` is dependents-only, enforced by a teaching error

**Chosen:** supplying `--depth` with any relation other than `dependents` fails with a typed error naming the flag, the relation, and the relations that accept it.

**Rationale:** the agent-native error contract (teach, don't just refuse) already governs the CLI; silently ignoring a meaningless flag would misteach the surface.

## Architecture

```text
build:
  rust-analyzer scip ──► occurrences ──► guarded join ──► aligned occurrences
                                                              │
                       enclosing-declaration attribution ─────┤
                                                              ▼
                                              edge derivation (this change)
                                                uses:    reference attributed to a declaration
                                                imports: reference attributed to the module
                                                type_hierarchy: impl-block name-token spans
                                                                joined to aligned occurrences
                                                              ▼
                                              edges (kind, src, dst) UNIQUE, schema v5

query:
  trace <symbol> dependents --depth N
        │ resolve symbol reference (existing tiers)
        ▼
  recursive CTE, reverse over {uses, imports, type_hierarchy}, horizon 20
        │ min-depth per symbol, deterministic tie-break
        ▼
  detail rows (depth ≤ N: identity, name, kind, distance, location)
  + beyond-bound aggregate (kind × depth counts, N < depth ≤ 20)
  + horizon disclosure (whether reach continued at hop 20)
  + calibrated envelope (provenance, freshness, typed absence, --json)
```

## Risks

- **Reference-grade `uses` inflates the radius** (spurious dependents add review effort): accepted by the edge-model decision; the command's self-description states the grade so consumers calibrate.
  The call-shaped split remains additive later.
- **Terminal-kind ambiguity on equal-depth paths** could break deterministic ordering: mitigated by the fixed tie-break order in the traversal decision, exercised by a determinism test.
- **Occurrence-based trait-impl derivation silently skips when the name-token occurrence is missing**: the skip is the designed refusal (never guess), but a systematic absence would under-report — the dogfood ground-truth task compares derived `type_hierarchy` edges against a hand-enumerated list of this repository's impl blocks to catch that.
- **Schema bump invalidates existing stores**: by design; the store-replacement contract (build replaces, queries teach) was shipped for exactly this, and its tests already cover the transition.

## Dogfood findings (this repository, 2026-07-05)

Rebuilt with the new binary against `silent-cartographer` itself (schema v5).

### Build accounting and edge counts

Join alignment (unchanged in shape): `aligned=11605` (exact 10434, crate_root 278, operator_desugar 807, module_span 25, self_keyword 61); `text_mismatch=224`, `semantic_only=0`, `duplicate_ambiguous=0`, `syntax_only=4`; freshness `fresh`.

Derived edges by kind: `contains=395`, `imports=392`, `type_hierarchy=7`, `uses=6296`. Total rows `7090`, distinct `(kind, src, dst)` also `7090` — **deduplication confirmed live** (every persisted edge is a distinct relation; the pre-dedup `uses` population reported at design time was larger).

### `type_hierarchy` ground truth — exact 1:1, no gaps, no fabrications

Every hand-enumerated `impl Trait for Type` block in `src/` mapped to exactly one derived edge, and vice versa (7 ↔ 7):

| impl block | derived edge |
| --- | --- |
| `impl std::fmt::Display for CanonicalId` | CanonicalId → `core::fmt::Display` (qualified path → terminal `Display`) |
| `impl From<Freshness> for FreshnessLabel` | FreshnessLabel → `core::convert::From` (generic — previously dropped) |
| `impl From<DetailArg> for Detail` | Detail → `From` (generic) |
| `impl From<RelationArg> for Relation` | Relation → `From` (generic) |
| `impl Default for Capabilities` | Capabilities → `core::default::Default` |
| `impl SemanticEngine for FixtureEngine` | FixtureEngine → `SemanticEngine` (workspace trait) |
| `impl SemanticEngine for RustAdapter` | RustAdapter → `SemanticEngine` |

The three generic `From<…>` cases — silently dropped by the prior name-matching derivation — are now recorded. The four `impl … for …` matches under `tests/` are all inside test string literals, not compiled trait impls, so they correctly produce no edges.

### `dependents` ground truth

- `CanonicalId`: 39 direct dependents (34 `uses`, 5 `imports`), reach extends beyond depth 1.
- `GraphStore`: 72 direct dependents (the query/command handlers plus every test that opens a store, and `commands`/`graph`/`query`/`resolve` module imports).
- `RelationArg` (CLI arg type): exactly 3 direct — `Relation`, `from`, `TraceArgs` — matching grep precisely (the `From<RelationArg> for Relation` impl body and the `TraceArgs.relation` field).

### Duplicate-ambiguous investigation — hypothesis refuted, follow-up filed

The stayed-at-zero `duplicate_ambiguous` count is **not** because rust-analyzer drops duplicate definitions upstream. Inspecting the emitted `index.scip`: colliding same-descriptor definitions (e.g. the three test-helper `sources()` functions rust-analyzer logs as "Duplicate symbol") are emitted under a **single shared SCIP symbol string** carrying multiple definition occurrences (`sources().` → 3 definition occurrences, one symbol), not as multiple `SymbolInformation` entries. `translate_index` groups occurrences by symbol string, so the collision collapses into one `ExtractedSymbol`. The duplicate-ambiguous guard fires only on ≥2 **distinct** persisted symbols sharing a descriptor, so it never triggers on this shape.

**Follow-up (new change):** colliding same-descriptor definitions currently merge into one canonical identity, with the first definition occurrence winning the span and the other definition sites collapsing onto it — a latent identity-collision gap distinct from the reference-side duplicate-ambiguous case. The guard should also detect one-symbol/multiple-definition-occurrence collisions, or the translation should preserve the distinct definition sites. Out of scope here (the blast-radius edges consume identities as produced); tracked for its own change.
