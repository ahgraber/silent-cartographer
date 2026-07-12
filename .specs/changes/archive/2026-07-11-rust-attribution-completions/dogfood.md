# Dogfood record: rust-attribution-completions

## Characterization verdicts (recorded before any rule code)

### Trait-side `Self` emission — PROCEED under an amended clause

Inspected on the ripgrep clone's raw index (`index.scip`, rust-analyzer emission) at refusing sites found via the store's `join_discrepancies` (expected `Matcher`, found `Self`; `crates/matcher/src/lib.rs` bytes 41307–41311 and siblings).

- The occurrence's symbol is **not** the trait: it is a synthetic impl-block symbol whose descriptors nest `impl`, the self type, and the trait as the trailing component — verbatim: `` rust-analyzer cargo grep-matcher 0.1.8 impl#[`&'a M`][Matcher] ``.
- The span is exactly the `Self` token (4 bytes) in qualified paths such as `Self::Error` / `Self::Captures` inside `impl<'a, M: Matcher> Matcher for &'a M`.
- The store's display name for that symbol is the trailing descriptor's name — the trait's name — which is why the refusal reads expected `Matcher`.
- Generics appear in the expected name where the trait is generic (self's `From<DetailArg>`, `From<Freshness>`, `From<LanguageArg>`, `From<RelationArg>` rows), so the comparison must strip generic arguments.
- The impl-block symbol carries **no definition occurrence anywhere in the index** (verified at the impl header line: only the trait-name references and local type parameters are emitted there); it sits in the store as kind `other`, class `external`, with no span.
- The shape is uniform across every trait-`Self` refusal surveyed: ripgrep `Matcher` ×36, `FromStr` ×3, `DecimalFormatter` ×1, `Default` ×1; self `Default` ×2, `SemanticEngine` ×2, `From<…>` ×4.

Because the shape differs from the one the gate assumed (the trait symbol itself) but is uniform, closed, and verifiable against the impl-header oracle, the user approved amending the delta's trait clause and the design decision to the observed shape (2026-07-10) rather than recording the family residual.
Consequences the amendment accepts: attribution lands on the impl-block node (no definition location — countable, not navigable), and the trait's own reference set is not enriched.

### Rust module descriptor nesting — PROCEED as designed

Inspected on self's raw index (`/Users/mithras/_code/silent-cartographer/index.scip`).

- Module descriptors nest hierarchically: `src/semantic/scip.rs` → `semantic/scip/`, `src/graph/join.rs` → `graph/join/`, `src/graph/mod.rs` → `graph/`, `src/lib.rs` → `crate/`.
- Every sampled file carries a whole-document module definition occurrence (definition role, range `[0, 0, <last-line>, 0]`) — the same shape the module-span rule already accepts.
- Nuance: the crate root's descriptor is `crate/` (its own named descriptor, not an empty prefix), so a `super` chain resolving to the crate root will not match descriptor-prefix ancestry and stays refused; no observed residual row takes that shape.

## Pins and tool versions

Five standing targets rebuilt with the schema-v10 binary (this change's working tree; suite 255 green, clippy/fmt clean).
Pins: httpx2 `9b7ee8e` (v2.5.0), Flask `22d9247` (3.1.3), ripgrep `48b0c795`, fd `5a5852e1`, self = working tree on this change; scip-python 0.6.6; rust-analyzer 1.95.0 / rustc 1.95.0 via `rust-toolchain.toml`.
Baselines: the four external targets from `.specs/changes/archive/2026-07-10-python-attribution-completions/dogfood.md`; self is not byte-comparable (the repo carries this change's own diff).
Every store written fresh under schema v10 (replace-on-build verified by the v9→v10 open).

## Final accounting (all five targets)

| target | aligned | exact | od | crate_root | module_span | self_keyword | self_name | import_alias | range_literal | use_list_self | super_keyword | text_mismatch | dup_ambiguous | syntax_only | aligned % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| httpx2 | 35249 | 30770 | 0 | 0 | 0 | 0 | 5 | 1 | 0 | 0 | 0 | 1102 | 0 | 0 | 97.0% |
| Flask | 16320 | 14414 | 0 | 0 | 0 | 0 | 188 | 18 | 0 | 0 | 0 | 68 | 6 | 2 | 99.5% |
| ripgrep | 50391 | 45516 | 2912 | 1514 | 98 | 45 | 33 | 39 | 139 | 30 | 65 | 658 | 72 | 111 | 98.6% |
| fd | 7505 | 6706 | 444 | 242 | 24 | 42 | 7 | 0 | 21 | 5 | 14 | 29 | 0 | 20 | 99.6% |
| self | 21331 | 18942 | 1799 | 429 | 28 | 73 | 0 | 2 | 30 | 0 | 28 | 299 | 0 | 4 | 98.6% |

(httpx2/Flask module_name and module_marker buckets omitted from the table for width: httpx2 4354/119, Flask 1617/83 — both byte-identical to the archived record.)
Aligned % = aligned / (aligned + text_mismatch + duplicate_ambiguous), matching prior records.
Conservation holds on every target: ripgrep +396 aligned / −396 text_mismatch, fd +59/−59; httpx2 and Flask ±0.

## HARD GATE: Python byte-stability — PASSED

httpx2 and Flask rebuilt **byte-identical** to the archived accounting in every pre-existing bucket, with all three new buckets exactly zero (every new rule is Rust-gated).

## Per-family results

### ripgrep (vs 49995 / 1054 / 72 / 111)

- `range_literal` **139 — exactly the harvest count**; spot-checked at source (`Range` at `0..10`, at `start..end`).
- `super_keyword` 65: the 10 file-module rows plus 55 recovered through the inline-module containment fix (`use super::…` inside `mod tests { … }` blocks across the crates); spot-checked (`glob` at `use super::Token::*;` inside glob.rs's tests module). Zero `super` refusals remain.
- `self_name` 33 (path-start `self`, scope added mid-apply): spot-checked (`human` at `use self::ParseSizeErrorKind::*;` in human.rs). More than the harvest's 22 — the harvest only counted two crates.
- `use_list_self` 30 (harvest said ≈22; the extra rows are additional `{self, …}` sites the residual decomposition's LIMIT hid).
- `self_keyword` +40 of the surveyed 41 trait-`Self` rows (36 `Matcher`, 3 `FromStr`, 1 `Default`); spot-checked at `Self::Error`/`Self::Captures` in `impl Matcher for &'a M`. The one non-convert: `DecimalFormatter` at `Self::MAX_U64_LEN` inside the struct's **own body** (an array-length in a field declaration) — no enclosing impl exists there, so the rule correctly refuses; a distinct idiom (`Self` within a type's own declaration body), one row, recorded residual.
- `exact` +89: tuple-field indexes; spot-checked (`0` at `base_lits.0.is_empty()`). The other ≈11 harvested tuple rows are macro-interior and stay refused.
- `operator_desugar` **unchanged (2912) — the propose-time estimate (+≈540) was wrong, investigated to root cause**: the desugar table was already complete at baseline, so every non-macro operator site already aligned before this change. The harvest's `index` (207+207), `not` (98), `sub` (25), `mul` (10) rows all sit inside macro invocation bodies (`assert!`, `assert_eq!`, …) — 30/30 sampled macro-adjacent. rust-analyzer expands macros and emits occurrences inside them; tree-sitter sees an unparsed token tree, so no construct exists to verify against. Unverifiable, correctly refused, permanent macro-fallout residual.
- Residual 658: macro fallout (≈550), the 52 out-of-scope crate-rename rows (`crate`@printer 31, @cli 21), 8 receiver-`self` rows (below), assorted tails.
- 8 rows expected `walk`/`util`, found `self` at **method-receiver positions** (`self.follow_link`, `self.dir.join(…)`): rust-analyzer attributes a module symbol to a value receiver token — an upstream misattribution shape; no oracle can verify a receiver as a module reference, correctly refused, recorded residual.

### fd (vs 7446 / 88 / 0 / 20)

- +59 aligned / −59 refused: `range_literal` 21, `super_keyword` 14, `self_name` 7, `use_list_self` 5, tuple `exact` +4 (exactly the harvest count), trait-side `self_keyword` +8 (inferred from the archived aggregate: 242+444+24+34 = 744 held for the four pre-existing Rust buckets).
- `operator_desugar` unchanged — fd's `not` 26 rows are macro fallout, same investigation as ripgrep.
- Residual 29 (expectation was ≈30): macro fallout plus tails.

### self (vs 19581 / 345 / 0 / 4, not byte-comparable — repo carries this change)

- `range_literal` 30, `super_keyword` 28 (≈22 expected — repo growth), `self_keyword` 73 (trait side landed), `import_alias` 2 unchanged, `use_list_self`/`self_name` 0 (no such idioms in tree), Python-only buckets 0, `semantic_only` 0, `duplicate_ambiguous` 0 — all invariants hold.
- Residual 299: macro fallout (index 196, not 36, gt 24, add 11, bitand 1, lt 1 — 25/25 sampled macro-adjacent, self's test files are assert-heavy), the 26 `scip_types` identity-mismatch rows (verified unprovable last change), one comment-mapped `add` (macro fallout onto a comment), and **one new grammar-ambiguity row**: a `RangeTo` occurrence at the `..` of `source[..*i]` — tree-sitter-rust parses `..*i` as an empty range multiplied by `i` (a `binary_expression` wrapping an empty `range_expression`) rather than RangeTo of a dereference, so the shape reads RangeFull and mismatches. Same class as the `..=b` expression-position quirk noted in the syntax oracle; correctly refused, recorded residual.

## Disclosure gates (raw multi-def symbols vs disclosed twin groups)

| target | raw multi-def symbols | disclosed twin groups |
| --- | --- | --- |
| httpx2 | 16 | 16 |
| Flask | 31 | 31 |
| ripgrep | 12 | 12 |
| fd | 1 | 1 |
| self | 11 | 11 |

All five match (self grew 10→11 with the repo's own diff; parity is the gate). PASSED.

## Spot checks (read at source)

- `range_literal`: `Range` at `for _ in 0..10` (util.rs), `Range` at `&doc[start..end]` (mod.rs) — both genuine range constructions.
- `use_list_self`: `hyperlink` at the `self` of `hyperlink::{self, HyperlinkConfig}` in summary.rs and standard.rs — the path terminal spells the module.
- `super_keyword`: `glob` at `use super::Token::*;` and `use super::{Glob, GlobBuilder, Token};` in glob.rs — inside the tests module, parent resolves through the inline chain.
- `self_name` (path-start): `human` at `use self::ParseSizeErrorKind::*;` (human.rs), `wtr` at `use self::StandardStreamKind::*;` (wtr.rs) — each document's own module.
- trait-`Self`: `Matcher` at `Self::Error` and `Self::Captures` inside `impl<'a, M: Matcher> Matcher for &'a M` (crates/matcher/src/lib.rs).
- tuple-index: `0` at `base_lits.0.is_empty()` (globset lib.rs).

## Session notes

- The propose-time dogfood expectations misread the harvest: the operator rows were macro fallout, not desugar-table gaps (the table needed nothing). Recorded above and corrected in design.md.
- Two mid-apply amendments landed with user approval: the trait-`Self` clause re-shaped to the observed impl-block emission, and path-start `self` added to scope (riding the `self_name` bucket).
- One implementation gap caught by dogfooding real rows: the `super` rule originally anchored on the document module alone and missed inline `mod` nesting; fixed with the containing-module extension (+55 rows in ripgrep alone).
- Follow-up candidates (unchanged): renamed-crate roots (52 rows, needs Cargo rename metadata); `Self` within a type's own declaration body (1 row); upstream: receiver-token module attributions (8 rows), tree-sitter-rust range-grammar ambiguities (`..=b` expression position, `..*expr`).
