# Proposal: module-scope-rules

## Intent

The python-adapter dogfood (httpx2 v2.5.0) and the duplicate-identity dogfood (ripgrep) each left a large, precisely-characterized refusal population that the current rule inventory cannot accept even though the evidence for correct attribution is present in the build:

- **Python module references refuse en masse.**
  scip-python names every module symbol with an `__init__` descriptor terminal, so every `import httpx2` / `typing.…` token fails the default name-token rule — 4,428 refusals on httpx2, 79% of its entire refusal population, holding Python alignment at ~84.5% when ~97% is reachable.
  The evidence for acceptance is already in the descriptor: the token spells the module's own dotted name.
- **Same-document twins refuse all their references.**
  Twin definitions in one document (Python property getter/setter pairs and their `self` parameters — 69 references on httpx2; Rust macro-generated items — 45 on ripgrep) defeat the document-grained locality rules by construction, so every group reference lands `duplicate_ambiguous` even when the reference sits inside exactly one twin's own scope.

Both families were filed in brainstorm §11 with counts; this change resolves them with two evidence-gated rules and re-dogfoods to verify no new confident-wrong attributions appear.

## User Stories

### Story: python-module-impact

As an AI agent tracing dependencies in a Python codebase, I want import references to resolve to the module symbols they name, so that module-level impact questions ("who imports this?") are answered from aligned evidence instead of refusing the majority of real references.

Ladders to the north star's precise-answer and blast-radius outcomes.

### Story: twin-scope-precision

As a developer working with twinned symbols (property accessor pairs, macro-generated items), I want a reference inside exactly one twin's scope attributed to that twin, so that reference sets and impact traces are precise where the evidence is unambiguous, while genuinely shared references stay typed-ambiguous.

Ladders to the north star's calibrated-confidence outcome: attribute exactly where evidence discriminates, refuse where it does not.

## Scope

**In:**

- A Python kind-scoped alignment rule accepting a module occurrence whose source token spells the module's own name (the dotted namespace's terminal component), with rule provenance and its own acceptance bucket in the accounting.
- A scope-grained locality rule for duplicated-descriptor references: attribute a group reference to the twin whose declaration scope encloses it when exactly one twin's scope does; locality provenance recorded; everything else stays `duplicate_ambiguous`.
- Accounting/schema updates the two rules require (new per-rule count; SCHEMA_VERSION bump; `status` disclosure).
- Re-dogfood on httpx2 and ripgrep (regression against the recorded baselines) plus one additional Python target chosen for re-export/alias density, explicitly harvesting alias-family evidence (counts, shapes, sample sites) for the fast-follow change.

**Out:**

- Re-export alias rule family (`pytest.mark` → `MARK_GEN`, ripgrep facade re-exports) — fast-follow change; this change only gathers its evidence.
- Any loosening for scip-python's star-re-export misattribution (787 refs on httpx2) — a tool bug the guard correctly refuses; stays refused.
- Zero-width module definition markers — already handled by design (identity bookkeeping; the occurrence stays refused and counted).
- Rust module-reference behavior — the crate-root rule already covers Rust; no Rust alignment change.
- Live/overlay indexing, new query surfaces, mixed-language stores.

## Approach

High-level direction; mechanism formalizes in design.md.

- **Module-name rule (Python-scoped, mirroring the crate-root rule's shape):** a reference or definition occurrence resolving to a module descriptor is accepted when its source token equals the terminal dotted component of the module's namespace name (e.g. module `httpx2._client` accepted at a `_client` token; module `typing` at a `typing` token).
  Acceptance carries a named rule (`module_name`) as provenance and its own accounting bucket.
  Aligned module references at module scope then feed `imports` edges through the existing language-neutral contract — no edge-derivation change.
- **Scope-grained twin locality:** for a same-document (or any residual) twin group reference, walk the reference's enclosing-declaration chain outward; the innermost enclosing declaration that contains any twin definition decides — exactly one twin contained there attributes the reference to that twin (locality provenance `declaration_scope`), more than one or none leaves it `duplicate_ambiguous`.
  This slots into the existing locality-rule sequence after the document-grained rules and never bypasses the guarded join's alignment check.
- **Accounting and schema:** one new aligned-rule count (`module_name`) in `index_metadata` and `status`; SCHEMA_VERSION 7 → 8 with the existing replace-on-build migration contract.
- **Dogfood with alias harvesting:** rebuild httpx2 (expect text_mismatch to drop by ≈4,428 and duplicate_ambiguous toward 0 for scope-resolvable refs) and ripgrep (expect the 45 same-doc macro twins to resolve; facade aliases stay refused); add one alias-dense Python target; record per-family counts and sampled shapes for the re-export fast-follow.

## Open Questions

- Third dogfood target: which alias-dense Python repo (candidate criteria: heavy `from ._x import y` / `import x as y` re-export surface, post-cutoff preferred, installable venv).
