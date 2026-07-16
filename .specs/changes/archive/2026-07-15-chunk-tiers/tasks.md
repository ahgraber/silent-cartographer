# Tasks: chunk-tiers

## Schema & Store

- [x] Bump `SCHEMA_VERSION` 10 → 11 in `src/graph/schema.rs`; add nullable `signature_text` and `interface_text` TEXT columns to the `symbols` table DDL.
- [x] Extend `SymbolRow` and the symbol insert/select paths in `src/graph/store.rs` with the two tier fields.
- [x] Test: a symbol row inserted with tier content round-trips both columns unchanged, and a row inserted with NULL tiers reads back as absent (store-level unit test).
- [x] Test: existing incompatible-store tests pass against v11 (build replaces a v10 store; query against a v10 store refuses with versions and recovery action).

## Tier Extraction (syntax oracle, pure helpers)

- [x] Implement Rust signature extraction in `src/graph/syntax.rs`: item text from node start to its body delimiter; items without a distinct body return the full declaration; attributes stay in the signature.
- [x] Implement Rust documentation extraction: the contiguous outer doc-comment run (`///`, `/** */`) immediately above an item, tolerating attributes between docs and item; the leading inner doc run (`//!`, `/*! */`) for a document.
- [x] Implement Python signature extraction: `def`/`class` header from node start through the header-ending `:`, decorators included.
- [x] Implement Python documentation extraction: the docstring (first body statement when a string expression); the module docstring (first document statement when a string expression).
- [x] Implement interface composition: documentation and signature joined in source order; equals signature when no documentation; unknown node kinds fall back to signature = full declaration.
- [x] Test: documented Rust function yields header-without-body signature and docs+signature interface (scenario: Documented Rust function tiers).
- [x] Test: Rust `;`-terminated and body-less items (const, static, type alias, bodiless trait fn, unit struct) yield signature = full declaration (scenario: Declaration without a distinct body).
- [x] Test: Rust struct/enum/trait headers cut at their `{`; generics and where-clauses stay in the signature.
- [x] Test: doc run with `#[derive(...)]` between docs and item is still associated with the item.
- [x] Test: Rust document-leading `//!` run extracted as module documentation (scenario: Rust module interface carries its module documentation).
- [x] Test: Python function with multi-line parameter list and decorator yields header-through-`:` signature including the decorator; interface carries header plus docstring (scenario: Python function tiers carry the docstring).
- [x] Test: Python class docstring and module docstring extraction (scenario: Python module interface carries its module docstring).
- [x] Test: undocumented item's interface equals its signature (scenario: Undocumented symbol falls back to signature) — fallback write-site.
- [x] Test: an unrecognized declaration kind degrades to signature = full declaration, interface = signature, never an error — total-extraction fallback write-site.
- [x] Cut the Python signature at the header-ending `:` token itself (a comment between header and body sits outside the `body` field and must not leak into the signature).
- [x] Recognize docstrings behind leading comments (shebang lines included) and implicitly concatenated string literals; tests for the shebang-module, comment-before-docstring, and concatenated-string cases.

## Build Persistence Wiring

- [x] Populate `signature_text`/`interface_text` in `src/graph/mod.rs` where `definition_span` resolves each symbol's declaration, for both language backends.
- [x] Add the explicit module branch at persistence: module-kind symbols get a whole-document span and text in both languages; module signature = qualified name; module interface = module documentation, falling back to the name.
- [x] Test: built Rust workspace persists tiers for a documented function, an undocumented item, and a const (canonical persistence write-site, per-kind partitions).
- [x] Test: built Python fixture persists tiers for a docstring-bearing function (Python persistence write-site).
- [x] Test: Rust module body equals its document byte-for-byte after a build (scenario: Rust module body equals its document).
- [x] Test: Python module body equals its document byte-for-byte after a build — the zero-width-marker case (scenario: Python module body equals its document) — module-branch write-site.
- [x] Test: external symbols persist NULL tiers (no definition span → no tier content).
- [x] Test: a declaration-less in-workspace symbol (a struct field; enum-variant constants and derive-synthesized methods take the same path) persists its name token as body, signature, and interface alike — name-span fallback write-site.
- [x] Test: rebuilding unchanged sources is idempotent including the tier columns (extends the builds-supersede evidence to the new columns).
- [x] Compose the module interface tier as signature followed by module documentation (uniform interface contract; signature alone when no documentation exists); update the module-tier tests.
- [x] Test: an inline `mod` declaration keeps its declaration span and text, never the whole document (scenario: Inline module declarations keep their declaration spans).

## Query Surface: get

- [x] Add `Detail::Interface` in `src/query/mod.rs` and `DetailArg::Interface` in `src/cli.rs`.
- [x] Serve `Detail::Signature` from persisted `signature_text` and `Detail::Interface` from `interface_text`; remove the query-time `signature_of` helper.
- [x] Test: `get` at interface detail returns docs+signature without the body (scenario: Retrieve interface by name).
- [x] Test: `get` at interface detail on an undocumented symbol returns the signature (scenario: Interface without documentation falls back to signature).
- [x] Test: `get` at interface detail on a module returns the module documentation without the whole document (scenario: Retrieve module interface).
- [x] Test: `get` at body detail on a module returns the whole document (scenario: Retrieve module body returns the whole document).
- [x] Test: `get` at interface detail on a Python function returns header plus docstring (scenario: Retrieve Python interface).
- [x] Test: existing `get` scenarios (location, signature, body, by-position, Python body/position) pass against the persisted-tier source of truth.
- [x] Test: `get --json` at interface detail carries the tier content within the calibrated output contract fields.

## Query Surface: trace

- [x] Add optional `--detail` to `TraceArgs` in `src/cli.rs` (reusing the detail enum), default absent.
- [x] Implement per-row tier projection in the trace paths of `src/query/mod.rs`: symbol-denoting rows project their own tiers; reference-site rows project the tiers of their attributed enclosing declaration; membership and order untouched.
- [x] Test: trace without `--detail` returns today's row shape with no tier content (scenario: Default trace rows carry no tier content).
- [x] Test: trace references at signature detail carries each row's tier content (scenario: Trace at signature detail).
- [x] Test: a reference site inside a method projects the method's signature (scenario: Reference sites project their enclosing declaration) — site-row write-site.
- [x] Test: trace dependents at interface detail carries interface content alongside kind and distance (scenario: Trace dependents at interface detail) — dependents-path write-site.
- [x] Test: the same trace at two detail levels returns identical symbols in identical order (scenario: Detail does not change the result set).
- [x] Test: trace `--json` with detail carries per-row content; dependents beyond-bound aggregate rows carry no tier content.
- [x] Select the module a NULL-attributed site projects by the widest module definition occurrence in the document, not by row span/identity order (a re-export-defined module ties the file module's whole-document span).
- [x] Test: a module-scope site projects the file module's tiers over a same-document re-export module whose identity sorts first — NULL-attribution write-site.
- [x] Test: `containers` at body detail carries the container's own full body (symbol-row projection arm, body tier).
- [x] Test: explicit location detail carries no content field (the default projection, requested explicitly).
- [x] Strengthen the signature-detail trace test to assert each row's exact attributed tier, not just presence.

## Validation & Dogfood

- [x] Run `cargo fmt`, `cargo clippy --all-targets`, `cargo test` through project tooling; all green with no new warnings.
- [x] Dogfood on self: build, spot-check module interface (`//!` files), a documented fn, a const; record store-size growth vs the prior schema.
- [x] Dogfood on ripgrep and fd: alignment counts and twin-group disclosure unchanged from the recorded baseline; sample tier content at source.
- [x] Dogfood on httpx2 and Flask: alignment counts and disclosure gates unchanged; document the expected module-row delta (whole-document spans) and verify non-module symbol rows are otherwise unchanged.
- [x] Write `dogfood.md` recording tier sampling, the module-row delta, store growth, and any node kinds that hit the total-extraction fallback.
