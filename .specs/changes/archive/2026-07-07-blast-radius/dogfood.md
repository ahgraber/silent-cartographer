# Dogfood & Ground Truth — blast-radius (2026-07-06)

Captured evidence for the Dogfood & Ground Truth task group, run against the post-review-remediation build.

## Build & status

Rebuilt this repository with the fixed binary (`rust-analyzer 1.95.0`, schema v5).
`status --json` captured at `.verify/status-post-fix.json`:
aligned=11970 (exact=10747, crate_root=288, operator_desugar=849, module_span=25, self_keyword=61), text_mismatch=228, semantic_only=0, duplicate_ambiguous=0, syntax_only=4 — accounting shape unchanged, conservation holds.
Edges after dedup: uses=6490, imports=449, contains=397, type_hierarchy=7.

## Ground truth: type_hierarchy — 7/7 exact

Hand enumeration of every written `impl Trait for Type` block in compiled code (`src/`, none in `tests/` — the four grep hits there are fixture string literals inside test functions, not code):

| Written impl | Derived edge |
| --- | --- |
| `impl From<DetailArg> for Detail` (cli.rs:59) | Detail → From ✓ (the formerly-dropped generic case) |
| `impl From<RelationArg> for Relation` (cli.rs:84) | Relation → From ✓ |
| `impl std::fmt::Display for CanonicalId` (identity.rs:140) | CanonicalId → Display ✓ (qualified-path case) |
| `impl From<Freshness> for FreshnessLabel` (output.rs:33) | FreshnessLabel → From ✓ |
| `impl Default for Capabilities` (capabilities.rs:66) | Capabilities → Default ✓ (external trait) |
| `impl SemanticEngine for FixtureEngine` (fixture.rs:51) | FixtureEngine → SemanticEngine ✓ |
| `impl SemanticEngine for RustAdapter` (rust_adapter.rs:53) | RustAdapter → SemanticEngine ✓ |

Matches: 7/7. Unexplained gaps: 0. Spurious edges: 0.
Designed skips confirmed: derive-generated implementations (no written impl block) and fixture-string impls (not code).

## Ground truth: dependents

Independent cross-check — direct `uses` dependents versus distinct enclosing declarations over the symbol's aligned reference occurrences (a derivation that does not read the edges table):

| Symbol | uses edges | occurrence-derived | imports edges | docs with module-scope refs |
| --- | --- | --- | --- | --- |
| CanonicalId | 36 | 36 ✓ | 6 | 7 |
| GraphStore | 70 | 70 ✓ | 5 | 7 |
| DetailArg | 3 | 3 ✓ | 0 | 0 |

The imports deltas are fully explained, not drops: several documents share the crate-root module symbol, and edge dedup collapses them (GraphStore: tests/operational.rs + tests/code_navigation.rs + tests/code_graph.rs → one edge from `crate`; every one of the 7 documents' modules is present among the edge sources).

Hand-read of `DetailArg` (small enough to verify exhaustively): graph reports GetArgs (field `detail: DetailArg`), `from` (the converter fn), and `Detail` (via the `impl From<DetailArg> for Detail` header) — 3/3; the mention in a graph/syntax.rs doc comment is correctly absent (comments are not code).

CLI smoke: `trace <GraphStore id> --relation dependents --depth 1` returns 74 detail rows + 5 beyond-bound aggregate groups, disclosure `beyond_bound`; at `--depth 25` disclosure is `ends_within_bound`, correct because the real network exhausts before hop 20 (the truncation case is pinned by the deep-chain unit tests).

## Duplicate-ambiguous investigation — hypothesis REFUTED

Hypothesis under test (brainstorm §11): duplicate_ambiguous stayed 0 because rust-analyzer drops sibling definitions from the emitted index.

Finding: **rust-analyzer drops nothing.**
The raw SCIP index contains 6 non-local descriptors with multiple definition occurrences, every twin fully present (SymbolInformation entries and definition occurrences alike):

- `crate/` — 6 definitions (all crate roots: main.rs, lib.rs, four tests/*.rs files)
- `ws().`, `client_id().`, `connect_id().`, `id_of().` — 2 each (same-named helper fns in tests/code_navigation.rs and tests/code_graph.rs)
- `sources().` — 3 (same-named helper across three test files)

The descriptor carries no target/crate component, so same-named top-level items in different targets collide byte-identically.

The actual reason duplicate_ambiguous is 0: `translate_index` (src/semantic/rust_adapter.rs:190) accumulates by SCIP symbol string — one `ExtractedSymbol` per descriptor, all occurrences pooled — so same-descriptor twins are **merged into one symbol before the identity layer's duplicate detection can see them**.
The six groups above land as one symbol each, definition location = first document encountered (e.g. `crate` → tests/semantic_engine.rs), and their references attribute to the merged identity.
The `#N` disambiguators that do appear (`graph#0` module / `graph#1` method, `Default#0/#1`, `params#0/#1`) come from the other collision path — distinct SCIP descriptors whose canonical forms collide — which works as contracted.

Consequence: the symbol-identity contract "True duplicates keep definition-anchored identities" is bypassed (not violated by the identity layer — starved by the adapter) for cross-target twins; its existing test evidence enters below the adapter merge, which is why the suite stays green.
Scope on this repo: 6 groups, all crate roots or test helpers; the crate-root merge is also what the multi-document module mapping (review fix 2) works around for imports edges.

**Follow-up filed** (brainstorm §11): move duplicate detection above (or into) the adapter accumulation so same-descriptor multiple-definition groups reach the definition-anchored disambiguation and duplicate-ambiguous accounting; natural home is the multi-workspace / identity change, and it should land before the Python adapter (scip-python may exhibit the same cross-target collisions).
