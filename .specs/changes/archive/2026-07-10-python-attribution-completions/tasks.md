# Tasks: python-attribution-completions

Standing rules for every task, restated so no task needs outside context:

- Never read, grep, or index anything under `/Users/mithras/_code/_worktrees/silent-cartographer/old-c10r-worktree/` (clean-room wall).
- Test-first for every contract scenario: write the named failing test, then make it pass.
  Never weaken an existing test; the two sanctioned existing-test changes are spelled out below where they occur.
- Every failure direction is refusal: when in doubt, an occurrence stays refused — never attributed by guess.
- Finish every group with `cargo fmt`, `cargo clippy --all-targets`, `cargo test --all-targets` all green before starting the next group.
- macOS link gotcha: prefix cargo commands with `LIBRARY_PATH="$(xcrun --show-sdk-path)/usr/lib"`.
- No git commits.
- scip-python 0.6.6 and python3 are installed; fixture regeneration tasks run for real.

## Syntax Oracle: Alias Bindings & Dotted Constructs

Definition of done: the join can ask any document for its alias bindings and, at any span, for the enclosing dotted-construct chain.

- [x] Add `alias_bindings()` to `SyntaxTree` in `src/graph/syntax.rs` returning per-document alias declarations, each `(alias_name_span, target_token_span)`.
  Python forms: `import a.b as c` (target token span = the dotted name `a.b`'s full span; alias span = `c`) and `from m import n as c` (target token span = `n`; alias span = `c`), both read from tree-sitter's `aliased_import` nodes.
  Rust forms: `use path as name;` and `pub use path as name;` (target token span = the terminal segment of `path`; alias span = `name`), from `use_as_clause` nodes.
- [x] Test `python_alias_bindings_enumerate`: a fixture source with both Python forms yields exactly the declared bindings with correct spans; a plain `import x` yields none.
- [x] Test `rust_use_as_bindings_enumerate`: a fixture source with `use a::b as c;` and `pub use d as e;` yields both; a plain `use a::b;` yields none.
- [x] Add `enclosing_dotted_constructs(span)` to `SyntaxTree` (Python): the chain of `attribute`/`dotted_name` node spans enclosing the span, innermost first; empty for a span outside any dotted construct.
- [x] Test `python_dotted_construct_chain_at_token`: in `h2.connection.H2Connection(...)`, the chain at the `h2` token contains the `h2.connection` span (and the wider `h2.connection.H2Connection` span); a bare identifier yields an empty chain.

## Raw-Shape Inspection & Fixture Extension

Definition of done: the raw scip-python emission shapes for `__name__` and alias sites are recorded; the committed fixture carries both idioms.

- [x] INSPECT FIRST (do not code the self-name rule before this): on the Flask clone at `/private/tmp/claude-501/-Users-mithras--code-silent-cartographer/57ecf74a-9eb1-48ce-91c9-fa19b61801e6/scratchpad/flask` (or a minimal probe project), run scip-python and examine the raw symbol emitted at a `Flask(__name__)`-style site.
  The module-scope-rules dogfood showed these refusals with `expected_name` = the module's dotted name (e.g. `tests.test_blueprints`), NOT `__init__` — so the symbol's descriptor shape differs from the `__init__`-terminal shape `classify_module_kinds` recognizes.
  Record the exact descriptor shape in a comment in the code and, if it is a module in different clothing, extend `classify_module_kinds` (and its test) to classify it; if it is not module-kind at all, adjust the self-name rule's equality to compare against the document's module by its dotted name instead, and say so in your report.
- [x] Extend `tests/fixtures/python-conformance/`: add to `pkg/consumer.py` an aliased import used at least once (e.g. `from pkg.shapes import Widget as W` plus a `W(...)` use) and a module-self-name line (e.g. `MODULE_NAME = __name__`); regenerate the committed `index.scip` once via the `#[ignore]` generator; refresh `SCIP-PYTHON-VERSION` if changed.
  Verify from the regenerated index which occurrences scip-python actually emits at the `W` use and at `__name__`, and record them in your report — subsequent test tasks say "prefer fixture, fall back to synthetic" based on what exists.

## Pass-1 Rules: Dotted Completion, Self-Name, Module-Marker

Definition of done: the three module-evidence rules accept per the delta scenarios; everything else keeps refusing; accounting fields exist in `JoinAccounting` (persistence is the schema group).

- [x] Dotted completion inside the module-name rule in `src/graph/join.rs`: when the span-level checks fail for a module-kind occurrence, retry the trailing-component-run condition against each span in `enclosing_dotted_constructs(span)`, innermost first; acceptance stays rule `module_name`.
- [x] Test `module_reference_accepted_through_enclosing_dotted_construct` (scenario: Module reference accepted through its enclosing dotted construct): synthetic index — module `pkg.sub` occurrence whose span covers only the `pkg` token inside source text `pkg.sub`, aligned under `module_name`; a prefix token with no enclosing dotted construct spelling the module stays refused (second assertion).
- [x] Self-name rule (`AlignmentRule::SelfName`, provenance `self_name`, `aligned_self_name` accounting field): Python-only; token text exactly `__name__` or `__file__`; accept iff the occurrence's resolved symbol equals the containing document's module per the document→module map (derive the map before `join` and pass it in as a parameter — it is already computed from index symbols in `ingest`; hoist, do not duplicate).
- [x] Test `self_name_token_accepted_for_own_module` (scenario: Module self-name token accepted for its own module): fixture if the regenerated index carries the `__name__` occurrence, else synthetic; assert rule provenance `self_name`.
- [x] Test `self_name_token_for_foreign_module_stays_refused` (scenario: Self-name token for a foreign module stays refused): synthetic — a `__name__` token whose occurrence resolves to a different module.
- [x] Module-marker rule (`AlignmentRule::ModuleMarker`, provenance `module_marker`, `aligned_module_marker` field): Python-only; definition-role occurrence of a module-kind symbol whose range is the empty span at byte 0 of its document.
- [x] Test `module_origin_marker_accepted_as_definition` (scenario: Module origin marker accepted as its definition): fixture-driven — the committed index's zero-width markers align under `module_marker`.
- [x] Test `zero_width_non_module_stays_refused` (scenario: Zero-width occurrence of a non-module stays refused): synthetic — a class-kind symbol with an empty-span origin occurrence.
- [x] SANCTIONED existing-test change: `python_occurrence_outside_every_rule_stays_refused` currently pins the zero-width marker as its refusal sample; the marker rule legitimately flips that exact occurrence (delta scenario "Module origin marker accepted as its definition").
  Retarget it to a sample no rule covers — synthetic occurrence of symbol A at a token spelling unrelated symbol B's name (the star-re-export misattribution shape) — keeping the typed-discrepancy and zero-aligned-rows assertions.
- [x] Marker-rule ripple check: aligned module definitions now enter `def_name_span` with empty spans — pin that `get` on a module returns the document at offset 0 (test `module_definition_location_is_document_origin`) and that no `contains` edge or enclosure is fabricated from an empty span (extend an existing containment test or add an assertion; state which).
- [x] Extend `AliasBinding` in `src/graph/syntax.rs` with `binding_span` (the whole `aliased_import`/`use_as_clause` node span) and extend the two enumeration tests to assert it.
- [x] Binding-site narrowing in the pass-1 rule dispatch: when an occurrence's span equals one of its document's alias bindings' `binding_span`s, re-evaluate the rule set at that binding's target token span; acceptance keeps the rule the narrowed evidence satisfies as provenance (no new bucket).
- [x] Test `binding_site_occurrence_accepted_at_target_token` (scenario: Binding-site occurrence accepted at its binding's target token): fixture-driven — the regenerated index's occurrence spanning `Widget as W` aligns with default-rule provenance.
- [x] Test `binding_site_with_foreign_target_stays_refused` (scenario: Binding-site occurrence with a foreign target stays refused): synthetic — an occurrence spanning a binding whose target token spells a different symbol's name.

## Pass 2: Import-Alias Rule

Definition of done: refused tokens spelling a document-local alias of their resolved symbol align under `import_alias`; every verification failure stays refused.

- [x] Second join pass in `src/graph/join.rs` per the design: after a document's first pass completes, for each first-pass refusal whose span sits on an identifier, look up the document's `alias_bindings()`; a binding is usable iff a first-pass aligned occurrence whose symbol equals the refused occurrence's symbol sits at the binding's target token span or at the binding's whole `binding_span` (the narrowed binding-site acceptance); accept iff the refused token's text equals the binding's alias name (`AlignmentRule::ImportAlias`, provenance `import_alias`, `aligned_import_alias` field).
  Remove accepted occurrences from the refusal buckets before accounting is finalized (conservation must hold).
- [x] Test `alias_token_accepted_under_document_binding` (scenario: Alias token accepted under its document's binding): fixture if the regenerated index carries the aliased-use occurrence, else synthetic (source with `from m import n as c` + a `c` use; index with both occurrences resolving to the same symbol).
- [x] Test `alias_bound_to_different_symbol_stays_refused` (scenario: Alias bound to a different symbol stays refused): synthetic — the `c` use's occurrence resolves to a different symbol than the binding site's.
- [x] Test `alias_binding_outside_document_is_not_evidence` (scenario: Alias binding outside the containing document is not evidence): synthetic — binding in document 1, refused token in document 2.
- [x] Test `alias_of_alias_stays_refused` (design risk pin): a binding whose own target token only aligns via the alias pass contributes nothing; both occurrences stay refused.
- [x] Rust leg, gated per the design: wire `alias_bindings()` for Rust documents through the same pass (it is language-neutral once bindings exist); test `rust_use_alias_accepted_under_document_binding` (synthetic: `use a::b as c;` + a `c` token whose occurrence resolves to `b`'s symbol).
  If twin/facade binding sites do not verify to a unique aligned occurrence, that is the gate working — do not force; the dogfood records what lands.

## Schema v9 & Render Surfaces

Definition of done: three new buckets persist, render everywhere, and conserve; v8 stores replace on build and refuse on query.

- [x] Bump `SCHEMA_VERSION` to 9; add `aligned_self_name_count`, `aligned_module_marker_count`, `aligned_import_alias_count` columns; thread through `IndexMetadata`/`write_metadata`/`read_metadata`.
- [x] Render both surfaces in the same task (the module-scope-rules D1 lesson): `run_status` JSON/plain and the `build` accounting line.
  Extract the build line's formatting into a testable function (it is currently an inline `println!` in `src/main.rs`); test `accounting_line_renders_every_bucket` asserts the rendered line's buckets sum to its `aligned=` total using a `JoinAccounting` with every field non-zero.
- [x] Test `new_rule_counts_ride_metadata`: metadata round-trip preserves all three new counts.
- [x] Extend the accounting-conservation test with non-zero `self_name`/`module_marker`/`import_alias` terms (each new bucket is a write-site of the conserved sum).
- [x] Confirm `build_replaces_an_incompatible_store` and `query_refuses_an_incompatible_store_with_guidance` pass against v9 unchanged.

## Dogfood, Regression & Upstream Issue

Definition of done: five targets rebuilt against stated per-family expectations, every deviation investigated before recording, the assignment-bound-alias verification recorded either way, and the upstream issue drafted.

- [x] Rebuild httpx2 (`v2.5.0`, clone in session scratchpad) — expectations: `module_marker` +119; `module_name` grows by the subset of the 201 prefix tokens whose enclosing dotted construct qualifies; `pytest.mark` (248) expected residual pending the verification below; star-re-export misattributions (787) unchanged.
- [x] Assignment-bound alias verification: inspect the raw httpx2 index for any `pytest`-package symbol binding `mark` to `MARK_GEN` (a symbol or relationship carrying the binding); record the finding in `dogfood.md` — if checkable evidence exists, say what a rule would need and file it as follow-up scope, do not implement here.
- [x] Rebuild Flask (`3.1.3`, clone in session scratchpad) — expectations: `self_name` ≈188, `module_marker` ≈83, `import_alias` ≈11, dotted completion recovers a subset of ~33 prefix tokens; `os.path` misattributions (~65) unchanged.
- [x] Rebuild ripgrep, fd, self — expectations: fd byte-stable; ripgrep `duplicate_ambiguous` −32 iff the Rust alias leg verified its facade bindings, else byte-stable; self recovers its ~35 `use … as` text-mismatch refusals iff the Rust leg landed, else stable; `semantic_only` 0 everywhere.
- [x] scip-check disclosure gate on every rebuilt target (stop on any mismatch).
- [x] Spot checks by hand: 2 `self_name`, 2 `import_alias`, 2 dotted-completion attributions read at source.
- [x] Draft the upstream scip-python issue (star re-export representative-symbol misattribution): minimal repro (an all-star `__init__.py` package), observed vs expected symbols, version 0.6.6; save the draft into `dogfood.md` for the user to file — do not post it anywhere.
- [x] Record everything in `.specs/changes/python-attribution-completions/dogfood.md` (pins, tool versions, before/after per family, deviations + investigations, the alias-verification finding, the issue draft); update brainstorm §11 (families resolved with residuals; assignment-bound finding).
