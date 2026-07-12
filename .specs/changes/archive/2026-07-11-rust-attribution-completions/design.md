# Design: rust-attribution-completions

## Context

- All six families were decomposed from the python-attribution-completions dogfood's Rust residuals (`.specs/changes/archive/2026-07-10-python-attribution-completions/dogfood.md`; store queries over ripgrep/fd/self with per-family counts); this change consumes that harvest.
- The join's rule dispatch is language-gated with a per-span rule core (`evaluate_rules_at`), the desugar correspondence already exists as a closed table in `src/graph/join.rs`, the impl-header syntax oracle (`trait_impls()`) already yields each implementation's trait and self-type names, and the render surfaces are guarded by the bucket-sum test — new buckets cannot silently vanish from output.
- python-attribution-completions is synced and archived; this delta modifies the current baseline directly.
- Out-of-scope residuals stay characterized, not ruled: renamed-crate roots (52, needs Cargo rename evidence — follow-up candidate), macro-expansion fallout, and the `scip_types` identity mismatches (verified unprovable).

## Decisions

### Decision: Correspondence entries close by trait family, not by observed instance

**Chosen:** the desugar table extends with complete trait families wherever every member's desugar is equally closed — indexing (`index`/`index_mut` at index expressions), `Not`/`Neg` (unary `!`/`-`), the binary arithmetic five (`add`/`sub`/`mul`/`div`/`rem`), their compound-assignment forms (`add_assign`/… at `+=`/…), the `PartialOrd` four (`lt`/`le`/`gt`/`ge`), and `Deref`/`DerefMut` (unary `*`).
The implementer reconciles this list against the entries the table already carries, adding only what is missing.

**Rationale:** the dogfood observed `gt` but not `lt` only because of which comparisons these particular codebases wrote; the sibling methods desugar by the identical mechanism, so shipping observed-only entries manufactures the next residual round out of sampling noise.
The closure unit is the trait family because that is the unit the language defines the desugar on.

**Alternatives considered:**

- Observed-instances only: rejected as sampling noise dressed up as caution — the sibling evidence is closed by construction, not by observation.

### Decision: Index expressions accept each bracket occurrence independently

**Chosen:** rust-analyzer emits one `index` occurrence per bracket token (`[` and `]`, always pairs); the rule accepts an indexing-method occurrence when its location sits inside an index expression, so both occurrences of a site align — two occurrence rows for one construct, no dedup.

**Rationale:** occurrences are the accounting unit and both records are real claims by the analyzer about the same construct; collapsing them would misstate conservation.
Brackets are anonymous tokens, so the construct lookup at either bracket finds the index expression itself — no operator-token comparison exists for this shape, and the method-kind gate (`index`/`index_mut` only) carries the precision instead.

### Decision: Range literals get their own rule and bucket (`range_literal`)

**Chosen:** a new `AlignmentRule::RangeLiteral`: a reference occurrence resolving to a range type is accepted at a range expression's operator token when the expression's shape matches the type under a closed six-row table — start+end+`..` → `Range`, start-only → `RangeFrom`, end-only → `RangeTo`, neither → `RangeFull`, start+end+`..=` → `RangeInclusive`, end-only+`..=` → `RangeToInclusive` — comparing type names with generic arguments stripped (the self-keyword rule's existing convention).
`RangeToInclusive` rides along unobserved for family closure.

**Rationale:** these occurrences resolve to *types*, not methods — a different evidence kind than operator desugar, worth distinct provenance; the shape table is closed and checkable from the range expression's own children.

**Alternatives considered:**

- Riding the `operator_desugar` bucket: rejected — provenance would claim a method-desugar check that never ran.

### Decision: Tuple-field indexes ride the default rule through a position-gated name-node extension

**Chosen:** the name-node gate learns tuple-index tokens — an integer token is a name node **only when it is the field child of a field expression** (`x.0`); acceptance rides `exact` with no new rule or bucket.

**Rationale:** the text already equals the expected name (`0` == `0`); the only failure was the gate's identifier-shaped worldview.
The field-position condition is load-bearing: a blanket "integer literals are name nodes" would let any `0` literal in any expression satisfy a symbol named `0` — the gate keeps the evidence exactly as narrow as the idiom.

### Decision: Use-list `self` verifies against the enclosing path's terminal segment — pure syntax

**Chosen:** a new `AlignmentRule::UseListSelf`: a module-kind occurrence at a `self` token inside a use-list is accepted when the terminal segment of the enclosing use path spells the occurrence's expected module name (`use crate::walk::{self, …}` — the `self` token carries module `walk`, and the path's terminal segment spells `walk`).
Nested use-lists resolve against the nearest enclosing path segment.
An aliased self-import (`{self as foo}`) needs no special case: the `foo` tokens are the import-alias rule's business, and its binding-site verification composes with this rule's acceptance at the `self` target token.

**Rationale:** the evidence is entirely within one statement's syntax — the path spells the module the keyword means; no cross-document or map lookup involved.

### Decision: `super` verifies against the containing module — the doc-module map extended by inline `mod` nesting

**Chosen:** a new `AlignmentRule::SuperKeyword`: derive a Rust document→module map before the join (module-kind symbols whose definition occurrence spans its document — the same shape the module-span rule trusts), passed in alongside the existing Python map; a module occurrence at a `super` token is accepted when the expected module's descriptor is the ancestor of the CONTAINING module's chain at the depth the path's `super` chain states (one `super` → parent, `super::super` → grandparent).
The containing module is the document's own module extended by the inline `mod` block names enclosing the token — `use super::…` inside `mod tests { … }` resolves from `doc_module::tests`, the dominant production shape (55 of ripgrep's 65 accepted rows sit inside inline test modules).
The identity part of the remaining chain compares as descriptor segments; any part still inside the inline extension compares by name against the inline `mod` names — syntax facts of the very document, the same anchoring the module-chain locality uses.
Any shape that doesn't verify — no doc module, different package, depth beyond the chain, segment mismatch — refuses.
The crate root spells its own descriptor (`crate`) rather than an empty prefix, so a `super` chain resolving all the way to the crate root does not match ancestry and stays refused; no observed residual row takes that shape.

**Rationale:** "parent module" is a fact about identity structure, and Rust module descriptors carry the chain; comparing descriptor ancestry is the same identity-not-name discipline as every other rule, and the inline extension is required for correctness — a `super` inside an inline module resolves from that module, not from the file's.
The map derivation mirrors Python's `module_by_document` (different trigger shape, same role) and hands the join one more pre-derived fact rather than a new in-join discovery pass.

**Alternatives considered:**

- Walking syntax `mod` declarations alone (no identity map): only sees inline modules — file-per-module layouts, the common case, would never verify; rejected. The chosen hybrid anchors on the identity map and extends with inline syntax.

### Decision: Path-start `self` rides the self-name bucket

**Chosen:** a module-kind occurrence at a path-start `self` token (`use self::x;`, `self::helper()`) is accepted when the expected module IS the containing module — the same containing-module chain the `super` rule verifies against, at depth zero.
Acceptance rides `AlignmentRule::SelfName` (provenance `self_name`, the bucket Python's `__name__`/`__file__` acceptances use): the token means "this module" in both languages, and the verification source (the document's own module identity) is identical.
No new bucket, no schema impact; the rule is Rust-gated like its siblings, and the receiver/parameter/use-list shapes of `self` are structurally excluded by the syntax oracle.
Scope added mid-apply with user approval (2026-07-11) after review showed the harvest had lumped these rows in with use-list `self`.

**Rationale:** cross-language buckets have the `import_alias` precedent; the concept — a keyword denoting the containing module, verified against identity — is exactly the self-name rule's, so a separate bucket would split one evidence kind across two accounting rows.

### Decision: Trait-side `Self` extends the self-keyword rule to the observed impl-block emission

**Chosen:** the characterize-first inspection ran before any rule code and found a shape adjacent to the assumed one: at every refusing trait-`Self` site (ripgrep's `Matcher` ×36 / `FromStr` ×3, self's `From<…>`/`Default`/`SemanticEngine` rows), rust-analyzer emits a synthetic impl-block symbol — descriptors nesting `impl`, the self type, and the trait as the trailing component (e.g. ``impl#[`&'a M`][Matcher]``) — at exactly the `Self` token; the trait symbol itself never appears there, and the impl-block symbol carries no definition occurrence anywhere in the index.
With the user's approval the clause was amended to that shape: the self-keyword rule gains — a reference occurrence resolving to a trait, or to an implementation of one (the observed emission; its display name is the trait's), is accepted at a self-type keyword when the enclosing implementation (per the existing impl-header oracle) implements that trait, base names compared with generics stripped.
Acceptance rides the existing `self_keyword` bucket — same rule name, extended condition, no schema impact.
Attribution lands on the symbol the analyzer named — for the observed emission, an impl-block node with no definition location; enriching the trait's own reference set is not claimed by this rule.

**Rationale:** the emission is uniform across all refusing rows and closed — the display name the join compares is the trait's name in either shape, so one comparison covers both; the evidence source (impl-header names resolved through the syntax oracle) and the generics-stripped convention are the ones originally designed, applied to a better-verified shape.

### Decision: SCHEMA_VERSION 9 → 10

**Chosen:** three new accounting columns (`aligned_range_literal_count`, `aligned_use_list_self_count`, `aligned_super_keyword_count`); replace-on-build migration as always; both render surfaces updated in one task, with the existing bucket-sum guard test extended to the new fields.

### Decision: Dogfood expectations per family; Python targets are a byte-stability regression gate

**Chosen:** the standing five targets.
The propose-time estimates assumed the harvest's operator rows were desugar-table gaps; implementation-time investigation on the real indexes corrected them: the desugar table was already complete, every non-macro operator site already aligned, and the residual operator rows (`index` pairs, `not`, `sub`, `mul`) sit inside macro invocation bodies — tree-sitter sees an unparsed token tree there, so they are unverifiable and stay refused (correct behavior, sampled 30/30 macro-adjacent).
Corrected Rust expectations — ripgrep: `operator_desugar` unchanged (2912), `range_literal` ≈139, `exact` +≈89 (tuple; the ≈11 macro-interior tuple rows stay refused), `use_list_self` ≈30, `self_name` ≈33 (path-start `self`), `super_keyword` ≈65 (55 via inline test modules), `self_keyword` +≈36 (trait side); residual falls toward ≈658 — macro fallout (≈550), the 52 crate-rename rows, and ≈8 receiver-`self` rows carrying module symbols (upstream misattribution, characterized at dogfood).
fd and self: per-family deltas verified the same way at dogfood time against the archived before-numbers, with every deviation investigated before recording.
httpx2 and Flask MUST rebuild byte-identical (every new rule is Rust-gated) — any Python-side drift is a defect, not a deviation to explain.
scip-check disclosure gates on all five; deviations investigated before recording.

## Architecture

```text
join, per document (Rust pass 1 additions):
  default rule: name node now includes tuple-index tokens (field-position gated) → exact
  operator_desugar: table already complete (verified); index expressions accept at either bracket, per occurrence
  range_literal (new): range-type occurrence at range-expression operator, shape↔type table (6 rows)
  self_keyword: condition extended — type == impl self type  OR  trait == impl trait (observed impl-block emission)
  use_list_self (new): `self` in use-list (incl. `{self as w}`), enclosing path terminal spells the module;
                       accepted span = the `self` token, so alias pass 2 composes at the binding target
  self_name (extended to Rust): path-start `self` token, expected module == containing module
  super_keyword (new): `super` token, expected module == containing module's ancestor at chain depth
      └ containing module = doc→module map entry (Rust variant: whole-doc module definition spans,
        derived pre-join beside the Python map) + inline `mod` names enclosing the token

store: schema v10 (+3 buckets)    render: build line + status via the guarded bucket-sum path
```

## Risks

- **tree-sitter node shapes assumed, not verified** (range expression children, tuple-field position, use-list nesting): every syntax-oracle task starts by parsing a sample and dumping the tree — the convention that caught the `aliased_import` span shape last change.
- **Rust module descriptors hierarchical** for the `super` rule: verified by characterization on self's raw index (`graph/join/` nests under `graph/`; every file carries a whole-document module definition occurrence); non-nested shapes refuse regardless.
- **Bracket-pair counting** doubles per-site acceptances by design; the dogfood expectations state occurrence counts, not site counts, so conservation stays checkable.
- **Python byte-stability** is asserted, not hoped: the dogfood gate fails the change if httpx2/Flask move at all.
