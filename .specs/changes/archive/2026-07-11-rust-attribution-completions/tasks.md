# Tasks: rust-attribution-completions

Standing rules for every task, restated so no task needs outside context:

- Never read, grep, or index anything under `/Users/mithras/_code/_worktrees/silent-cartographer/` (clean-room wall).
- Test-first for every contract scenario: write the named failing test, then make it pass.
  Never weaken an existing test; the one sanctioned existing-test change is spelled out below where it occurs.
- Every failure direction is refusal: when in doubt, an occurrence stays refused — never attributed by guess.
- Finish every group with `cargo fmt`, `cargo clippy --all-targets`, `cargo test --all-targets` all green before starting the next group.
- macOS link gotcha: prefix cargo commands with `LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib"`.
- No git commits.
- Verify every tree-sitter node kind and field name by parsing a sample and dumping the tree programmatically before trusting it (delete the probe afterward) — this convention caught the `aliased_import` span shape last change.

## Raw-Shape Characterization

Definition of done: the two gated families have recorded verdicts; no rule code for either exists yet.

- [x] CHARACTERIZE FIRST (gates the trait-`Self` tasks): on the ripgrep clone's raw index (`/private/tmp/claude-501/-Users-mithras--code-silent-cartographer/57ecf74a-9eb1-48ce-91c9-fa19b61801e6/scratchpad/ripgrep/index.scip`, scip-check tool in the sibling `scip-check/` dir), inspect a refusing trait-`Self` site (expected `Matcher`, found `Self` — find one via the store's `join_discrepancies`).
  Record: the occurrence's exact symbol (is it the trait itself?), its span (exactly the `Self` token?), and whether generics appear in the expected name (self's `From<DetailArg>` rows say yes).
  If the occurrence is the trait symbol at the `Self` token, the trait-side tasks below proceed; anything murkier → record the family residual, skip those tasks, and flag that the delta's trait clause needs amending before sync.
  **Verdict (recorded in dogfood.md): the emission is a synthetic impl-block symbol (``impl#[`&'a M`][Matcher]``) at exactly the `Self` token, trailing descriptor = the trait, generics present in the expected name; user approved amending the delta/design to this shape — the trait-side tasks proceed against the amended clause.**
- [x] CHARACTERIZE FIRST (gates the `super` tasks): on self's raw index (`/Users/mithras/_code/silent-cartographer/index.scip`), confirm Rust module symbols carry hierarchical descriptors (does module `semantic::scip`'s descriptor nest `semantic` then `scip`?) and confirm the whole-document module definition occurrence exists per file (the shape the module-span rule accepts).
  Record both; if descriptors are not nested, the `super` rule cannot anchor on prefix ancestry — record residual, skip its tasks, flag the delta clause.
  **Verdict (recorded in dogfood.md): confirmed — `semantic/scip/` nests `semantic` then `scip`, `graph/join/` nests under `graph/`, and every sampled file carries a whole-document module definition occurrence; crate root spells `crate/` (its own descriptor, not an empty prefix), so crate-root `super` chains stay refused. The `super` tasks proceed.**

## Syntax Oracles

Definition of done: the join can ask, at any span, for the index/range/use-list/super facts the new rules condition on.

- [x] Extend name-node recognition in `src/graph/syntax.rs`: an integer token is a name node iff it is the field child of a field expression (tuple-field index `x.0`); a bare integer literal anywhere else is not.
  Test `tuple_index_is_name_node_only_in_field_position`: `x.0` yields the token; a standalone `0` literal and a `0` in an array length do not.
- [x] Add `range_shape(span)` to `SyntaxTree` (Rust): for a span at a range expression's operator token, return which ends are present and whether the operator is inclusive (`..=`); `None` off range operators.
  Test `range_shape_reads_all_six_shapes`: `a..b`, `a..`, `..b`, `..`, `a..=b`, `..=b` each yield their shape; a non-range span yields `None`.
- [x] Add `use_list_self_context(span)` to `SyntaxTree` (Rust): for a span at a `self` token inside a use-list, return the terminal segment span of the nearest enclosing use path; `None` elsewhere (including `self` as a receiver or path-start `self::`).
  Test `use_list_self_resolves_enclosing_path_terminal`: `use a::walk::{self, x};` at the `self` token yields the `walk` span; nested `use a::{b::{self}};` yields `b`; a method's `self` parameter yields `None`.
- [x] Add `super_chain_depth(span)` to `SyntaxTree` (Rust): for a span at a `super` token in a path, return 1 + the number of `super` segments preceding it in the same path; `None` off `super` tokens.
  Test `super_chain_depth_counts_position`: `use super::x` yields 1; the second token of `super::super::y` yields 2.

## Pass-1 Rules

Definition of done: the delta's Rust scenarios accept and refuse as written; new accounting fields exist in `JoinAccounting` (persistence is the schema group); every rule is Rust-gated.

- [x] Reconcile the desugar correspondence in `src/graph/join.rs` against the full family list — indexing (`index`/`index_mut` at index expressions), `Not` (`not` at `!`), `Neg` (`neg` at unary `-`), arithmetic (`add`/`sub`/`mul`/`div`/`rem` at their binary operators), compound assignment (`add_assign`/`sub_assign`/`mul_assign`/`div_assign`/`rem_assign` at `+=`-family operators), `PartialOrd` (`lt`/`le`/`gt`/`ge` at `<`/`<=`/`>`/`>=`), `Deref`/`DerefMut` (`deref`/`deref_mut` at unary `*`) — adding only what the table lacks; list the final table in your report.
  Index expressions have no operator token: accept an `index`/`index_mut` occurrence when its span sits inside an index expression, each occurrence independently.
- [x] Test `indexing_occurrences_accepted_at_both_brackets` (scenario: Indexing occurrences accepted at both bracket tokens): synthetic — two `index` occurrences at `[` and `]` of one `a[i]`, both aligned under `operator_desugar`; a third `index` occurrence at a non-index location stays refused.
- [x] Test `extended_operators_accepted_under_desugar_rule`: one occurrence each for `not`@`!`, `gt`@`>`, `add`@`+`, `deref`@`*`, all aligned under `operator_desugar`; confirm the pre-existing `Method outside the correspondence stays refused` test still passes unchanged.
- [x] Tuple-field default acceptance: no join change beyond the name-node extension; test `tuple_field_index_accepted_under_default_rule` (scenario: Tuple-field index accepted under the default rule): synthetic — a field occurrence named `0` at the `0` token of `x.0`, aligned under `exact`.
- [x] Range-literal rule (`AlignmentRule::RangeLiteral`, provenance `range_literal`, `aligned_range_literal` field): Rust-only; a reference occurrence resolving to a range type accepted at a range operator token when `range_shape` matches the type under the six-row table, generic arguments stripped.
- [x] Test `range_literal_accepted_under_shape_correspondence` (scenario: Range literal accepted under its shape correspondence): synthetic — `Range` at `a..b` and `RangeFrom` at `a..`, both aligned under `range_literal`.
- [x] Test `range_occurrence_with_mismatched_shape_stays_refused` (scenario: Range occurrence with a mismatched shape stays refused): synthetic — `RangeInclusive` at a plain `a..b` operator.
- [x] Use-list-self rule (`AlignmentRule::UseListSelf`, provenance `use_list_self`, `aligned_use_list_self` field): Rust-only; a module-kind occurrence at a use-list `self` token accepted when the enclosing path terminal (per `use_list_self_context`) spells the expected module name.
- [x] Test `use_list_self_token_accepted_for_path_module` (scenario: Use-list self token accepted for the path's module): synthetic — module `walk` occurrence at the `self` in `use crate::walk::{self};`.
- [x] Test `use_list_self_for_different_module_stays_refused` (scenario: Use-list self token for a different module stays refused): synthetic — module `other` occurrence at the same token.
- [x] Rust document→module map: extend the pre-join derivation so Rust documents map to their module symbol via the whole-document module definition occurrence (the Python zero-width variant stays untouched); one derivation function, dispatched by language, passed into `join` through the existing parameter.
  Extend an existing map test or add `rust_doc_module_derived_from_whole_document_definition` (state which in the report).
- [x] Super-keyword rule (`AlignmentRule::SuperKeyword`, provenance `super_keyword`, `aligned_super_keyword` field): Rust-only; a module-kind occurrence at a `super` token accepted when the expected module's descriptor equals the document's own module's descriptor ancestor at depth `super_chain_depth(span)`; no doc module, non-nested descriptors, or depth mismatch → refuse.
  Gated on the characterization verdict.
- [x] Test `super_token_accepted_for_parent_module` (scenario: Super token accepted for the parent module): synthetic — doc module `a::b::c`, `super` occurrence resolving to `a::b`, aligned under `super_keyword`; a `super::super` token resolving to `a` also accepted (depth 2).
- [x] Test `super_token_for_non_parent_module_stays_refused` (scenario: Super token for a non-parent module stays refused): synthetic — the same doc, a `super` occurrence resolving to sibling `a::d`.
- [x] Trait-side self-keyword extension (characterization verdict: proceed, amended shape): the self-keyword rule also accepts a reference occurrence resolving to a trait — or to an implementation of one (the observed emission: an impl-block symbol whose display name is the trait's) — at a self-type keyword when the enclosing implementation's trait (per the existing impl-header oracle) has the same base name, generics stripped; acceptance rides `self_keyword`.
- [x] Test `trait_reference_accepted_at_self_keyword` (scenario: Trait reference accepted at a self-type keyword within its implementation): synthetic — an occurrence whose expected name is `From<X>` at `Self` inside `impl From<X> for Y`.
- [x] SANCTIONED existing-test change: the existing foreign-implementation self-keyword refusal test pins "expected type differs from the impl's self type → refused", a universal the trait clause narrows (delta scenario "Self keyword in a foreign implementation stays refused" now requires matching neither the self type nor the trait).
  Extend its setup so the expected symbol matches neither, keeping its refusal assertions; do not delete it.

- [x] EMERGED (aliased self-import write-site): the use-list-self rule accepts at the `self` token itself so the import-alias pass verifies aliased self-imports (`{self as w}`) at the binding target; the `use_list_self_context` oracle looks through a `use_as_clause`.
  Test `aliased_use_list_self_composes_with_import_alias` covers the composed path; the oracle test gained the aliased case.
- [x] EMERGED (inline-module containment, found on real ripgrep rows): the super rule resolves from the CONTAINING module — the doc module extended by inline `mod` names enclosing the token — not the document module alone (`use super::…` inside `mod tests` was refusing).
  Test `super_inside_inline_module_resolves_from_the_inline_chain`.
- [x] EMERGED (scope added with user approval 2026-07-11): path-start `self` rule — a module occurrence at a path-start `self` token (`use self::x;`, `self::helper()`) accepted when the expected module IS the containing module; rides the `self_name` bucket (cross-language, like `import_alias`); oracle `path_start_self(span)`.
  Tests `path_start_self_recognized_only_at_path_start` (oracle), `path_start_self_token_accepted_for_containing_module`, `path_start_self_for_foreign_module_stays_refused`.
- [x] INVESTIGATED (desugar reconciliation finding): the correspondence table was already complete — every family the proposal listed was present at baseline, and every non-macro operator site already aligned; the harvest's `index`/`not`/`sub`/`mul` residual rows sit inside macro invocation bodies (token trees to tree-sitter, unverifiable, correctly refused; 30/30 sampled macro-adjacent).
  Final table: branch→?; eq/ne/lt/le/gt/ge; add/sub/mul/div/rem (+_assign); bitand/bitor/bitxor/shl/shr (+_assign); neg; not; index/index_mut; deref/deref_mut; call/call_mut/call_once; into_iter/next.

## Schema v10 & Render Surfaces

Definition of done: three new buckets persist, render everywhere, and conserve; v9 stores replace on build and refuse on query.

- [x] Bump `SCHEMA_VERSION` to 10; add `aligned_range_literal_count`, `aligned_use_list_self_count`, `aligned_super_keyword_count` columns; thread through `IndexMetadata`/`write_metadata`/`read_metadata`.
- [x] Extend both render surfaces in the same task: `build_accounting_line` and `run_status`'s alignment object gain the three buckets; extend `accounting_line_renders_every_bucket`'s all-nonzero `JoinAccounting` so the bucket-sum guard covers the new fields (it fails on any unrendered bucket by construction).
- [x] Extend `new_rule_counts_ride_metadata` (or add a v10 sibling; state which) so the three new counts round-trip non-zero.
- [x] Extend the accounting-conservation test with non-zero `range_literal`/`use_list_self`/`super_keyword` terms (each new bucket is a write-site of the conserved sum).
- [x] Confirm `build_replaces_an_incompatible_store` and `query_refuses_an_incompatible_store_with_guidance` pass against v10 unchanged.

## Dogfood, Regression & Record

Definition of done: five targets rebuilt against stated per-family expectations, Python byte-stability enforced as a gate, every deviation investigated before recording.

- [x] Rebuild ripgrep — expectations: `operator_desugar` +≈540 (index pairs 414, `not` 98, `sub` 25, comparison/arithmetic tails), `range_literal` ≈139, `exact` +≈100 (tuple indexes), `use_list_self` ≈22, `self_keyword` +≈36 iff the trait gate landed; `text_mismatch` falls toward ≈250 (macro fallout + the 52 out-of-scope crate-rename rows); `duplicate_ambiguous` 72 and `syntax_only` 111 unchanged.
- [x] Rebuild fd — expectations: `not` 26, ranges 19, tuple 4, use-list self ≈8 recovered; residual toward ≈30; everything else stable.
- [x] Rebuild self — expectations: index 190, `not` 33, `gt` 21, `add` 10, `deref` 2, ranges 20, `super` ≈22, tuple 3 recovered; residual toward ≈45 plus the 27 verified-unprovable `scip_types` rows; `import_alias` 2 unchanged.
- [x] Rebuild httpx2 and Flask — HARD GATE: byte-identical accounting to the archived python-attribution-completions dogfood numbers (every new rule is Rust-gated); any drift is a defect to fix, not a deviation to record.
- [x] scip-check disclosure gate on every rebuilt target (stop on any mismatch).
- [x] Spot checks by hand: 2 `range_literal`, 2 `use_list_self`, 2 `super_keyword` (or trait-`Self` if that family landed), and 1 tuple-index attribution read at source.
- [x] Record everything in `.specs/changes/rust-attribution-completions/dogfood.md` (pins, tool versions, before/after per family, both characterization verdicts, deviations + investigations); update brainstorm §11 (Rust families resolved with residuals; renamed-crate follow-up restated).
