# Proposal: rust-attribution-completions

## Intent

The python-attribution-completions dogfood left the Rust text-mismatch residual decomposed with counts and shapes (its `dogfood.md`; store queries over ripgrep/fd/self).
Six families carry real, checkable evidence the current rule inventory does not consume — together ≈1,200 of the 1,487 residual rows across the three Rust targets:

- **Indexing desugar** — `a[i]` desugars to the `index` method; rust-analyzer emits one occurrence per bracket token (`[` and `]`, always in pairs: ripgrep 414, self 190), neither spelling `index`.
- **Operator desugar gaps** — logic/arithmetic/comparison/deref methods outside the current closed correspondence: `not`@`!` (157 across targets), `sub`@`-` (25), `gt`@`>` (21), `add`@`+` (10), `deref`@`*` (2), plus same-trait siblings in the tails.
- **Range literals** — `a..b` / `a..=b` / `..` construct the `Range` / `RangeInclusive` / `RangeFrom` / `RangeTo` / `RangeFull` types; the occurrences are *type* references at the `..`/`..=` operator token (178 across targets), a closed shape↔type correspondence rather than a method desugar.
- **Tuple-field indexes** — `.0` field accesses refuse with expected `0`, found `0`: the text already matches, but a tuple index is not an identifier, so the default rule's name-node gate never sees it (107 across targets).
- **Path-keyword modules** — module occurrences at Rust's relative-path keywords: the `self` inside `use x::io::{self, …}` (the token means the module the path names), the path-start `self` of `use self::…` (the token means the containing module itself), and the `super` in `use super::…` (the token means the containing module's parent); ~55 harvested across targets, more once inline test modules are counted.
- **Trait-side `Self`** — trait references (`Matcher`, `Default`, `From<…>`) at `Self` tokens (~45, mostly ripgrep): the self-keyword rule compares only the enclosing implementation's *type*, never its *trait*.
  Raw emission shape uninspected; this family enters scope through a characterize-first gate.

The remaining residual is macro-expansion fallout (occurrences mapped to written source whose text is another construct entirely) and the identity-mismatch families the last dogfood verified unprovable — neither is ruled around.

## User Stories

### Story: operator-aware-references

As an AI agent tracing references and impact in Rust code, I want operator call sites — indexing, arithmetic, comparisons, ranges — attributed to the methods and types they desugar to, so that reference sets and blast-radius traces include the idiomatic call sites Rust code is mostly made of.

Ladders to the north star's precise-answer and blast-radius outcomes.

### Story: idiomatic-rust-evidence

As a developer whose Rust code uses tuple fields, `use` self-imports, and `super` paths, I want those sites attributed from the evidence each idiom actually carries, so that module- and field-level answers are complete and the accounting reflects caution only where evidence is genuinely absent.

Ladders to the north star's calibrated-confidence outcome.

## Scope

**In:**

- Extending the operator-desugar correspondence with the evidenced entries and their same-trait siblings where the desugar is equally closed (indexing at both bracket tokens; `Not`; the arithmetic five; `PartialOrd`'s four; `Neg`; `Deref`); the exact entry list is a design decision.
- A range-literal correspondence: a type occurrence resolving to one of the five range types is accepted at a range expression's operator token when the expression's shape (which ends are present, `..` vs `..=`) corresponds to that type — a closed shape↔type table.
- Tuple-field indexes accepted under the default rule: the name-node gate learns the tuple-index token so the already-matching text can be seen.
- Path-keyword module rules: a module occurrence at a `self` token inside a use-list is accepted when the enclosing path names that module; a module occurrence at a path-start `self` token is accepted when the resolved module is verifiably the containing module; a module occurrence at a `super` token is accepted when the resolved module is verifiably the containing module's parent — the verification source is a design decision.
- Trait-side `Self`, characterize-first: inspect the raw emission at a refusing `Self` token before design commits; extend the self-keyword rule to the enclosing implementation's trait only if the evidence is as clean as the type-side check.
- Accounting buckets and provenance for whatever new rules land; one schema bump.
- Dogfood: the standing five targets with per-family expected deltas; Python targets expected byte-stable (all new rules Rust-gated); scip-check disclosure gates; deviations investigated before recording.

**Out:**

- Renamed-crate roots (ripgrep's 52 `crate`@`printer`/`cli` rows) — needs Cargo dependency-rename evidence through the metadata boundary; recorded as a follow-up candidate, not implemented here.
- Macro-expansion fallout and the verified identity-mismatch residuals (`scip_types` crate-vs-module) — no rule may guess these.
- Any acceptance not backed by a closed, checkable correspondence.

## Approach

High-level direction; mechanism formalizes in design.md.

- The desugar and range tables stay closed correspondences dispatched inside the existing Rust-gated rule block; indexing must handle the two-occurrences-per-site pairing without double-counting constructs.
- Tuple-field indexes are a name-node recognition extension, not a new rule — acceptance rides `exact`.
- The use-list `self` check is pure syntax (the parent path segment's text names the module); the `super` check needs the document's own module relationship, whose source (module-span alignment vs. module-chain walk) the design settles.
- Trait-side `Self` reuses the existing impl-header syntax oracle if the characterization supports it.
- Sequencing: this delta modifies the Guarded positional join as amended by `python-attribution-completions`; that change syncs first.

## Open Questions

- What symbol shape rust-analyzer emits at trait-side `Self` tokens (the characterize-first gate decides whether the family lands or is recorded residual).
- Whether `super` verification can anchor on the module-span aligned occurrence of the containing document without new cross-document plumbing.
