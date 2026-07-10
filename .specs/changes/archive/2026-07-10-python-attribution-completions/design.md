# Design: python-attribution-completions

## Context

- All four families were characterized by the module-scope-rules dogfood (`.specs/changes/module-scope-rules/dogfood.md`, brainstorm §11) with counts, span shapes, and sample sites; this change consumes that harvest, it does not re-discover.
- The join's rule dispatch is language-gated and per-rule accounted (module-scope-rules); the document→module map derives from index symbols before the join runs, so join-time rules may consume it.
- Sequencing: the delta modifies the Guarded positional join as amended by module-scope-rules (feat-committed, not yet synced); sync order is module-scope-rules first.
- Rust has its own recorded alias instances: ripgrep's 32 facade `pub use` refusals (twin-locality path) and self's ~35 `use … as` refusals (filed 2026-07-04) — the alias rule's mechanism decides whether they land now or stay recorded.

## Decisions

### Decision: Alias bindings are identity-verified through the binding site's own aligned occurrence

**Chosen:** an alias binding is usable evidence only when the binding statement's target token carries an aligned occurrence of the very symbol the refused occurrence resolves to.
The syntax layer contributes per-document alias declarations (alias name span + target token span) from each language's declared forms; the join, in a second pass over first-pass refusals, accepts a refused reference token when its document declares an alias of that spelling whose target token span holds a first-pass aligned occurrence with the same symbol identity.

**Rationale:** comparing names would re-introduce name-matching (two distinct `Environment` symbols would collide); anchoring on the aligned occurrence at the binding site makes the alias check an identity equation, the same trick `type_hierarchy` derivation uses for impl headers and base classes.
It also handles Rust facades for free in principle: `pub use grep_matcher as matcher;` has its `grep_matcher` token resolved by the target-metadata locality to the library twin, so the binding pins the exact twin — no new twin logic.

**Alternatives considered:**

- Text-level alias→target-name table: name-matching by another door; rejected.
- Resolving aliases through the semantic index's own symbol for the alias name: scip-python emits no such intermediate symbol at reference sites — it resolves straight through to the target, which is exactly why the sites refuse today.

### Decision: Two-pass join — alias evaluation runs over first-pass refusals

**Chosen:** the existing single pass stays untouched; occurrences it refuses are retried once against the document's alias bindings after the whole document's first pass completes (bindings need the first pass's aligned occurrences to verify against).
Acceptance in the second pass records rule `import_alias`; anything still refused lands in the discrepancy buckets exactly as today.

**Rationale:** the binding-site occurrence must be aligned before it can verify anything, and binding statements lexically precede uses in the overwhelming case but not always — a per-document barrier is the simplest correct ordering.
The pass is per-document and bounded by the refusal count, so cost is negligible.

**Alternatives considered:**

- Interleaving alias lookup in the first pass with lazy verification: order-dependent results; rejected.

### Decision: Binding-site occurrences align through target-token narrowing

**Chosen:** scip-python emits the occurrence at a from-import alias binding with a span covering the whole `target as alias` text (fixture: one occurrence spanning `Widget as W`, carrying the target's symbol), which no span-level rule can accept; when an occurrence's span equals an alias binding's whole binding span, the join re-evaluates it at the binding's target token span, accepting under whichever rule the narrowed evidence satisfies (default name-token equality, or the module rules for module-kind symbols) with that rule as provenance.
The alias pass's verification condition — "the target token span holds a first-pass aligned occurrence" — is thereby defined precisely: an aligned occurrence whose span is the binding's target token span or the binding's whole binding span.

**Rationale:** the wide span is a carrier quirk of the same family as the prefix-token module spans — the target token inside it genuinely spells the symbol's name; narrowing is the mirror image of the dotted-construct widening, and it keeps the alias pass's identity verification exactly as designed (Rust binding sites, whose occurrences sit on the target token itself, satisfy the same condition through span equality).
Pinning "holds" to the binding's own two spans keeps an incidental containing occurrence — a whole-document module span, for instance — from verifying a binding it is not part of.
Without the narrowing, every Python binding-site occurrence refuses on text and the alias rule could never verify a Python binding; the emission shape was discovered by fixture inspection during implementation.

**Alternatives considered:**

- Verifying bindings against raw (unaligned) occurrences at the target span plus an inline text check: duplicates rule logic inside the alias pass and leaves every binding-site occurrence counted as a refusal — accounting noise the narrowing removes.

### Decision: Alias binding forms per language — Python import-as now, Rust use-as attempted, assignment-bound recorded

**Chosen:** Python declares `import a.b as c` and `from m import n as c`; Rust declares `use path as name;` including `pub use` re-exports.
The Rust leg is attempted under an explicit gate: if the binding site's token does not resolve to a unique aligned occurrence (twin or otherwise), the binding contributes nothing and the sites stay refused — recorded, not forced.
Assignment-bound third-party aliases (`pytest.mark` → `MARK_GEN`, 248 refs) are expected to stay residual: the binding lives in unindexed third-party code; the implementer verifies against the raw index (does any `pytest/mark.` symbol with a relationship to `MARK_GEN` exist?) before concluding, and records the finding either way.

**Rationale:** both Python forms are unambiguous single-statement bindings; Rust's `use … as` is the same shape and its instances are already counted (32 + ~35).
The assignment-bound family fails the change's own evidence bar unless the index carries the binding — the verification step turns "we believe it's unreachable" into a recorded fact.

**Alternatives considered:**

- Python-only this round: leaves two counted Rust families on the table when the mechanism is shared; the gate bounds the risk.
- Chasing `pytest.mark` through typeshed/stub knowledge: hardcoded third-party knowledge; rejected outright.

### Decision: Self-name rule conditions on the document→module map

**Chosen:** a module occurrence at a token spelling exactly `__name__` or `__file__` is accepted iff the occurrence's resolved symbol equals the containing document's own module per the document→module derivation (the zero-width-marker map, computed before the join and passed in).
Rule provenance `self_name`.

**Rationale:** the equality is a pure identity check between two facts the build already holds; `Flask(__name__)` sites (188 on Flask) resolve to the containing module by construction.
A `__name__` token resolving to any other module — or any non-module symbol — fails the equality and stays refused.

**Alternatives considered:**

- Accepting `__name__` by token alone: would accept scip-python misattributions at those tokens; the equality is the whole guard.

### Decision: Module-marker rule mirrors the bookkeeping it duplicates

**Chosen:** a definition-role occurrence of a module-kind symbol whose range is the empty span at byte 0 of its document is accepted with rule provenance `module_marker`; the document→module bookkeeping keeps its own derivation (no dependency inversion — the rule and the map both read the same index shape).

**Rationale:** the store already trusts this shape for `imports`-edge sourcing; counting it as a refusal is noise that misstates calibration.
Aligned module definitions also give `get` a definition location for modules (the document at offset 0) — a small consumer win.

**Alternatives considered:**

- Deriving the map from the aligned rows instead: creates a join→ingest ordering knot for zero benefit.

### Decision: Dotted-expression completion lives inside the module-name rule

**Chosen:** when the module-name rule's span-level checks fail, the rule retries with the text of the smallest enclosing dotted construct (Python `attribute`/`dotted_name` nodes) under the unchanged trailing-component-run condition; acceptance stays rule `module_name` (same bucket — same evidence family, wider carrier).

**Rationale:** the prefix-token spans are a range quirk on occurrences whose site does spell the module in full; the construct text is the same evidence the rule already accepts, read from the construct the parser sees.
A prefix token with no enclosing dotted construct spelling the module stays refused.

**Alternatives considered:**

- A separate rule + bucket: provenance precision without an evidence distinction — the accepted text satisfies the identical condition.

### Decision: SCHEMA_VERSION 8 → 9

**Chosen:** three new accounting columns (`aligned_self_name_count`, `aligned_module_marker_count`, `aligned_import_alias_count`); replace-on-build migration as always.
Both render sites (the `build` output line and `status`) gain the buckets in the same task — the D1 lesson from module-scope-rules.

### Decision: Dogfood expectations per family, upstream issue drafted not filed

**Chosen:** the standing five targets, with stated expectations — httpx2: marker +119, dotted-completion recovers a subset of the 201 prefix tokens (only those with a qualifying enclosing construct), `pytest.mark` 248 expected residual (subject to the raw-index verification); Flask: self-name ≈188, marker ≈83, alias ≈11, prefix subset of ~33; ripgrep: −32 duplicate-ambiguous iff the Rust alias leg lands, else byte-stable; self: ~35 use-alias text-mismatch recovered iff the Rust leg lands; fd: byte-stable.
Deviations investigated before recording; scip-check disclosure gate on every target.
The scip-python star-re-export issue is drafted into the dogfood record for the user to file; nothing is sent anywhere by the assistant.

## Architecture

```text
join, per document:
  pass 1 (existing): default + language-gated kind-scoped rules
    Python additions:  module_name (span → else smallest enclosing dotted construct)
                       self_name   (token __name__/__file__ ∧ symbol == doc's module)
                       module_marker (def role ∧ module kind ∧ empty span at origin)
  barrier: document's aligned occurrences known
  pass 2 (new): import_alias over pass-1 refusals
    syntax alias declarations (per language forms)
      Python: import…as / from…import…as     Rust: use…as / pub use…as
    binding usable iff target-token span holds a pass-1 aligned occurrence
    accept refused token iff spelling == alias ∧ binding's symbol == occurrence's symbol

store: schema v9 (+3 buckets)   render: build line + status together
document→module map: unchanged derivation, now also passed into join (self_name)
```

## Risks

- **Alias second pass ordering**: a binding whose target token itself only aligns via the alias pass (alias-of-alias) stays refused — accepted limitation, refusal-direction.
- **Rust alias leg friction**: `use` trees with nested groups/globs complicate binding extraction; the gate (unique aligned occurrence at the binding token or nothing) keeps failure refusal-shaped, and the leg may be recorded residual without blocking the change.
- **Marker-rule ingest ripples**: aligned module definitions enter `def_name_span` with empty spans; `get` on modules and `contains`/parent derivation paths must be checked (fixture tests pin them) so an empty span never fabricates enclosure.
- **Self-name false positives require symbol equality to fail first** — none observed in the harvest; the dogfood's per-family expected deltas are the check.
- **httpx2/Flask numbers assume unchanged clones** (same tags pinned); rebuilds use the recorded pins.
