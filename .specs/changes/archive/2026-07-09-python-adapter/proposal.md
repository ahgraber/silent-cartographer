# Proposal: python-adapter

> **Status: delta specs authored 2026-07-07** (after `duplicate-identity` synced into the baseline); design and tasks pending.
> Scope decisions settled with the user the same day: single-language per store, environment honesty as typed failure + provenance (doctor command deferred to the interface change), dogfood target pinned to `pydantic/httpx2`.

## Intent

The north star's v1 horizon is trustworthy exact navigation and blast radius for Rust **and Python**; today only Rust exists.
The semantic-engine contract was deliberately written as a backend-neutral intersection with a conformance suite as the gate, and this change cashes that in: a Python backend built from the only production Python SCIP indexer (scip-python, on Pyright) paired with tree-sitter-python as the syntax oracle, delivering the same four pillars Rust has — calibrated identity, guarded syntax/semantic alignment, the dependency graph, and navigation queries — from the batch index alone.
A second, differently-shaped language is also the first real test that the backend contract, the identity scheme, and the calibration discipline generalize beyond the language they were grown on.

## User Stories

### Story: python-precise-locate

As an agent or developer working a Python codebase, I want a symbol's definition and references located exactly, so that effort goes to the task, not the search.

### Story: python-blast-radius

As an agent about to alter a Python symbol, I want its dependents — callers, importers, subclasses — so that I do not silently break distant code.

### Story: python-calibrated-answers

As an agent or developer, I want Python answers to carry the same calibration as Rust answers — typed refusals, alignment accounting, provenance, honest staleness — so that I can trust c10r on Python exactly as far as the evidence supports and no further.

## Scope

**In scope:**

- A Python semantic backend satisfying the existing semantic-engine contract: batch index via scip-python, provenance reporting (indexer identity and version, plus the Python environment the index resolved against), capability declaration, and a conformance-suite pass.
- A Python syntax oracle (tree-sitter-python) and the guarded positional join for Python, including whatever Python-specific alignment-rule families dogfooding surfaces — grown the same way Rust's were, with refusal as the default.
- Python identities through the existing workspace-namespaced canonical identity, inheriting duplicate handling from `duplicate-identity`.
- Dependency edges (`uses`, `imports`, `contains`, `type_hierarchy` — class inheritance in Python terms) and full navigation parity: locate, trace, dependents on Python stores.
- Operational honesty at the new external boundary: detecting the indexer toolchain, failing with a typed, actionable error when it is absent or the project's environment cannot be resolved.

**Out of scope:**

- A live LSP precision overlay (north star: beyond v1).
- Adapters for ty or pyrefly — excluded from the base-index role until either ships SCIP output; no LSP-only adapter.
- Cross-engine base+overlay identity reconciliation.
- Any other language, and any semantic/intent search work.

## Approach

**Settled direction** (from `._scratch/python-engine-port-adapter-analysis.md`, 2026-06-28):

- scip-python is the only production Python SCIP emitter; it is invoked as an external tool the way rust-analyzer is, and its Node.js runtime is an accepted operational dependency of Python support.
- SCIP stays the port for the base index; engines without a SCIP indexer cannot serve as a base.
  This is the waiting strategy for ty/pyrefly, not a rejection of them.
- tree-sitter-python mirrors tree-sitter-rust on the syntax side.

**Preliminary spec guidance** (for the deferred deltas — written against the expected post-`duplicate-identity` baseline):

- **semantic-engine:** the mandatory contract must not strengthen — contract-floor rule; Python conformance is a second implementation passing the same suite, so the likely delta is MODIFIED wording naming a second exemplar plus per-backend conformance scenarios, not new mandatory clauses.
  Capabilities Python may lack (e.g. enclosure information) stay feature-detected, never assumed.
- **symbol-identity:** the identity contract is descriptor-pure and language-agnostic, so expect no contract change; Python descriptor normalization hazards (re-exports through `__init__`, aliased imports, `<locals>` names, overloads) are design-level handling that only becomes spec if a genuinely new contract emerges.
- **code-graph:** existing edge requirements were worded during a Rust-only era; deltas should restate anything Rust-flavored at language-neutral altitude and add Python scenarios per partition (language is a semantic partition of these contracts' input space — e.g. `type_hierarchy` covers both a Rust trait implementation and a Python class base).
- **code-navigation:** contracts should already be language-neutral; expect scenario additions only.

**Preliminary design guidance:**

- Index quality depends on the Python environment scip-python resolves against (virtualenv, installed third-party packages); the environment used is part of provenance, and an unresolvable environment is a typed failure, not a degraded silent index.
- Expect scip-python descriptor collisions analogous to rust-analyzer's; the `duplicate-identity` machinery is inherited, not re-derived — this is why that change lands first.
- Alignment-rule discovery is an explicit dogfood loop in the plan (Rust needed `crate_root`, `operator_desugar`, `self_keyword`, `module_span`; Python candidates to probe: `self`/`cls`, decorators, properties, dunder methods, re-export aliasing), with each family added as a named, tested rule and everything else refused.
- First-party versus third-party symbols: keep parity with however the Rust store treats external-package descriptors; do not invent a new policy inside this change.

## Settled Questions (2026-07-07)

- **Dogfood target:** `https://github.com/pydantic/httpx2`, pinned at a release tag chosen at implementation time; a second, larger repo may join later the way ripgrep did for Rust.
- **Mixed-language repositories:** out of scope — one store holds one language's build (auto-detect with explicit override; a Rust+Python repo indexes each language into its own store); the mixed-store unification (per-backend provenance/freshness) is its own later change.
- **Environment readiness:** typed failure + environment provenance in this change; a proactive doctor-style readiness command is deferred to the interface change — it will reuse the same detection primitives the typed-failure path builds, so this is not rework.
- **ty / pyrefly SCIP output:** confirmed wishlist-only, nothing shipped (user, 2026-07-07); scip-python remains the sole base indexer.

## Open Questions

- scip-python maintenance risk (Sourcegraph abandonment was rated medium-likelihood/high-impact in the research); decide whether pinning a fork is part of this change or accepted risk.
