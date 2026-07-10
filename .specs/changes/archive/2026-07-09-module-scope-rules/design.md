# Design: module-scope-rules

## Context

- Both rules land in machinery that already exists: the guarded join's kind-scoped rule dispatch (four Rust rules, language-scoped since python-adapter) and the locality-rule sequence over twin groups (defining-document, module-chain, target-metadata, from duplicate-identity).
  This change adds one rule to each sequence; no new subsystem.
- The evidence base is the recorded dogfoods: httpx2 v2.5.0 (4,428 module-name refusals; 69 same-document twin refusals across 16 groups) and ripgrep (45 same-document macro-twin refusals), plus clean baselines on self and fd to regress against.
- The re-export alias family is explicitly a fast-follow: this change must leave the dogfood with enough recorded alias evidence (counts, shapes, sample sites per target) to design that rule without re-running discovery.

## Decisions

### Decision: Python modules are classified as module-kind at the adapter boundary

**Chosen:** the Python adapter's enrichment classifies a symbol as `SymbolKind::Module` when its descriptor carries scip-python's module shape — a meta-suffixed `__init__` terminal following the module's namespace segment — so every downstream consumer (the module-name rule, module-to-document bookkeeping, `get`/`status` display) keys off the one `module` kind vocabulary instead of re-deriving the shape.

**Rationale:** scip-python does not report a module kind in `SymbolInformation`, so Python modules currently land as `other` and the document→module mapping keys off the zero-width definition marker as a second, parallel convention.
Normalizing the kind once at the boundary keeps the backend-specific naming convention in the adapter — where backend conventions belong — and lets the join stay convention-free.

**Alternatives considered:**

- Recognizing the `__init__` shape inside the join at rule-evaluation time: leaks a scip-python convention into language-neutral code and leaves `get` reporting modules as `other`.
- Upstreaming a kind fix to scip-python: right long-term, useless for a version-pinned tool today.

### Decision: Module-name rule — trailing component-runs of the dotted name, module-kind occurrences only

**Chosen (as amended by the 2026-07-09 dogfood):** the rule accepts an occurrence when (a) the resolved symbol is module-kind, and (b) the occurrence's span text, after stripping any leading relative-import dots, equals a trailing component-run of the module's dotted namespace name at a component boundary — the full name (`anyio.abc`), the bare terminal component (`_client`), and the relative form (`._api`, `.transports.default`) are all instances of the one condition.
For a bare-identifier span the identifier name-node gate applies as in the default rule; for dotted and relative spans — which no single identifier node contains — the structural gate is the span text itself, which must be exactly leading dots plus a dotted identifier path (no other characters), read from the source bytes.
Rule provenance value: `module_name`; its own acceptance bucket in the accounting.
The rule is role-agnostic: scip-python's zero-width module definition markers never match (empty text), so they keep refusing without a special case.

**Rationale:** the httpx2 dogfood decomposed the module-reference family into four span shapes scip-python emits: bare terminal identifiers (3,800 — the originally-spec'd form), relative-import spans that include the leading dots (253), full dotted-path spans (106), and prefix-token spans where the range covers only a leading component of a submodule's name (269).
The first three all carry the same clean evidence — the token spells the module's own name — and are unified by the trailing-component-run condition; the fourth is a genuine range quirk (an `h2` token carrying the `h2.connection` symbol) where the text does not name the module, and it stays refused.
Accepting leading components would attribute one token to a module it does not name — the confident-wrong shape the guard exists to refuse.
External modules (`typing`, `pytest`) are covered by the same clause: the rule conditions on symbol kind and name evidence, not workspace membership, exactly as the crate-root rule does.

**Alternatives considered:**

- Terminal-identifier-only (the pre-amendment shape): left 359 clean-evidence references refused for no calibration reason; the follow-up would have reopened the same rule.
- Accepting leading/prefix components: over-attribution (see above).
- Restricting to workspace modules: `import typing` sites would keep refusing for no calibration reason, and module-level dependents on external modules is real signal.

### Decision: Kind-scoped rules are language-gated in dispatch (dogfood amendment, 2026-07-09)

**Chosen:** the rule dispatch enforces the spec's language scoping literally — the four Rust rules evaluate only for Rust documents, the module-name rule only for Python documents.

**Rationale:** the first httpx2 rebuild showed the un-gated Rust module-span rule accepting 8 Python occurrences — zero-width module markers on empty `__init__.py` files, where a zero-width span vacuously equals the whole (empty) document.
The acceptances were semantically harmless but spec-nonconformant, and un-gated rules are a standing cross-language leak hazard as rule inventories grow per language.

**Alternatives considered:**

- Language-neutralizing the module-span rule instead: defensible semantics, but it widens a contract mid-change for 8 degenerate occurrences on empty files — not worth the spec surface.

### Decision: Scope-grained locality — innermost twin-bearing enclosing declaration decides

**Chosen:** for a group reference in a document associated with more than one twin, walk the reference's enclosing-declaration chain innermost-outward (the chain the syntax oracle already provides); the first declaration whose full span contains at least one twin's definition location is the deciding scope.
Exactly one twin inside it → attribute to that twin with locality provenance `declaration_scope`; more than one, or chain exhausted → `duplicate_ambiguous` unchanged.
The rule runs after the document-grained rules in the existing locality sequence and never bypasses the guarded join's alignment check.

**Rationale:** this is the containment relation the store already trusts for enclosure, applied to twins: a `self` reference inside a property getter's body finds the getter's `function_definition` as its innermost twin-bearing scope (the getter's `self` twin is defined in its parameter list) and attributes correctly; the setter's references symmetrically.
Twins at document top level (ripgrep's un-wrapped macro statics) find no twin-bearing *declaration* and stay refused — the document itself is not a deciding scope, because every same-document twin lives there and it discriminates nothing.
The refusal direction of every failure mode is preserved: span drift, macro-mapped ranges, and multi-twin scopes all fall to `duplicate_ambiguous`, never to an arbitrary pick.

**Alternatives considered:**

- Nearest-preceding-definition semantics (attribute to the last twin defined before the reference): mimics runtime rebinding order but fabricates confidence for forward references and macro-reordered code; rejected as guess-shaped.
- Distance-based (closest twin by byte offset): pure heuristic, no scope evidence; rejected.

### Decision: SCHEMA_VERSION 7 → 8

**Chosen:** bump for the new `aligned_module_name_count` accounting column on `index_metadata`; migration is the existing replace-on-build / typed-refusal-on-query contract.

**Rationale:** the accounting contract requires per-rule counts to be retrievable and to conserve the occurrence total; a v7 store read by the new binary would silently miss a bucket.
Locality provenance needs no schema change — `occurrences.locality` is TEXT and gains the value `declaration_scope`.

**Alternatives considered:**

- Folding module-name acceptances into the exact bucket: destroys the per-rule provenance the accounting contract and future rule-calibration work depend on.

### Decision: Dogfood = three-target regression plus alias harvest

**Chosen:** rebuild and record httpx2 v2.5.0 and ripgrep against their archived baselines, self as the clean-Rust regression, and add Flask (alias-dense `from .x import y as y` re-export surface) as the third target; pin exact tags in `dogfood.md`.
Expected deltas stated up front: httpx2 `text_mismatch` drops by ≈4,428 (module family) and `duplicate_ambiguous` 69 → ≈8 (the 12 property `self`-param groups resolve by scope; the 4 class-attribute groups only partially — references outside both twins' scopes stay ambiguous); ripgrep's 45 same-document macro twins resolve only where the macro wraps its expansion in a declaration; self and fd byte-stable.
Every deviation from an expected delta is investigated before recording.
The alias harvest is a first-class dogfood task: for each target, categorize residual `text_mismatch` families with counts and sampled sites, explicitly tagging alias-shaped ones (token spells a runtime alias of the expected symbol) — the fast-follow's design input.

**Rationale:** the two prior dogfoods produced the baselines that make regression meaningful; Flask is chosen for exactly the family this change defers, so the fast-follow starts from data, not discovery.

**Alternatives considered:**

- httpx2 only: loses the Rust-side scope-rule evidence (ripgrep) and gathers no alias data beyond pytest.
- Skipping expected-delta statements: a dogfood that cannot say what it expects cannot detect over-acceptance — the number moving is not the same as the number being right.

## Architecture

```text
guarded join (per occurrence)
  ├─ default rule (name-token equality)            [unchanged]
  ├─ Rust kind-scoped rules ×4                      [unchanged]
  └─ Python kind-scoped rule ×1  ◄── NEW module_name
       symbol.kind == Module (set by adapter enrichment)
       + name-node gate + token == terminal dotted component

twin-group locality (per group reference, in sequence)
  ├─ defining_document   [unchanged]
  ├─ module_chain        [unchanged]
  ├─ target_metadata     [unchanged]
  └─ declaration_scope  ◄── NEW
       enclosing-declaration chain (syntax oracle, already parsed)
       innermost declaration containing ≥1 twin def decides:
       exactly 1 → attribute (locality = declaration_scope)
       else      → duplicate_ambiguous (unchanged default)

store: schema v8 (aligned_module_name_count on index_metadata)
status: renders the new rule bucket; accounting conservation unchanged
imports edges: no derivation change — newly-aligned module refs at
module scope flow through the existing contract (module→module edges)
```

## Risks

- **Over-acceptance by the module-name rule**: the token must already resolve to that module in the semantic index — the rule reconciles a naming convention, it does not guess resolution; the same trust shape as the crate-root rule, which held at 0 misattributions across three Rust dogfoods.
  The dogfood's expected-delta discipline is the check: aligned-count growth must equal the module-family refusal count it consumes.
- **Twin definition spans vs syntax spans drift** (macro-mapped ranges, decorated definitions): containment failure falls to refusal, never misattribution; ripgrep is the stress test.
- **Kind reclassification ripples**: flipping Python modules from `other` to `module` kind changes `get`/`status` display and the module-bookkeeping predicates; the existing Python fixture tests pin the consumer-visible behavior and the conformance fixture pins the shape the classification keys on.
- **Flask may not index under scip-python 0.6.6 / Python 3.14**: probe before committing to it; fall back to another alias-dense target (pydantic, requests) and record the substitution in `dogfood.md`.
- **Class-attribute twins resolve only partially**: expected and stated in the dogfood plan; the residual is honest ambiguity (a bare `count` reference in a sibling method genuinely does not say which definition it means), not a defect.
