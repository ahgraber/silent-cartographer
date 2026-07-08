# Proposal: duplicate-identity

## Intent

The symbol-identity baseline already contracts that two distinct definitions sharing an identical resolved descriptor keep definition-anchored identities, but the contract is starved in practice: the Rust adapter accumulates extraction output by descriptor string, so same-descriptor twins are merged into one symbol before the identity layer can see them.
The 2026-07-06 blast-radius dogfood proved the scale on this repository: 6 twin groups (all crate roots and same-named test helpers) arrive fully evidenced in the raw semantic index and are silently collapsed, `duplicate_ambiguous` reads 0 forever, and every reference to a twin attributes to whichever definition was encountered first.
This change lifts duplicate handling above the merge so twins stay distinct, attributes their references by locality instead of surrendering them all to ambiguity, and makes the residual ambiguity honest and visible.
It must land before the Python adapter, whose indexer likely produces the same descriptor collisions.

## User Stories

### Story: twins-stay-distinct

As an agent or developer, I want two different definitions that happen to share a name to remain two different symbols with stable identities, so that an answer about one is never silently an answer about the other.

### Story: locality-keeps-precision

As an agent tracing usage or blast radius, I want a reference to a duplicated name attributed to the twin in whose territory the reference sits, so that honesty about duplicates does not needlessly degrade navigation and dependency answers that the evidence actually supports.

### Story: disclosed-ambiguity

As a developer calibrating trust in the graph, I want the store to disclose which names are duplicated and how many references could not be attributed to a single twin, so that I know exactly where answers are weaker before I lean on them.

## Scope

**In scope:**

- Semantic-engine extraction contract: a backend delivering multiple distinct definitions under one identical descriptor yields multiple distinct symbols, never a merged one.
- Symbol-identity: the definition-anchored disambiguation contract exercised end-to-end through real extraction (not only fixtures below the merge point), with twin ordering deterministic and meaningful (anchored to definition location, not input encounter order).
- Reference attribution: locality-based assignment of reference occurrences to twins; references no locality rule can settle become typed `duplicate_ambiguous` — counted, never guessed.
- Accounting and disclosure: `duplicate_ambiguous` participates in the status accounting identity (conservation holds), and duplicated-descriptor groups are visible to `status` consumers.
- Code-graph: edges derive from the corrected attribution, so per-twin dependency answers (including per-crate-root imports) are right; revisit the multi-document module mapping workaround that the merged crate root forced.

**Out of scope:**

- The multi-workspace management surface (registry, per-graph selection, cross-repo queries) — its own later change.
- The Python adapter — the next change, which depends on this one.
- Macro via-expansion attribution and other alignment-rule follow-ups from brainstorm §11.
- Fixing or working around the upstream descriptor scheme itself (rust-analyzer emits target-less descriptors; we handle, not patch).

## Approach

Mechanism parking lot — formalized in `design.md`:

- **Where the lift lives.**
  Prefer splitting/attribution in a backend-neutral layer (identity/join side of the extraction boundary) over per-adapter logic, so the Python adapter inherits it; the adapter's obligation shrinks to "do not merge".
- **Locality rule candidates**, strongest evidence first: a reference inside a twin's own defining document belongs to that twin; a reference inside a document that belongs to exactly one twin's territory (e.g. a test target rooted at one file; document sets reachable from one crate root's module tree) belongs to that twin; anything weaker is refused to `duplicate_ambiguous`.
  Territory derivation should come from evidence already in hand (defining documents, module containment) before reaching for external metadata — adopt-over-build applies.
- **Known hard case:** a document compiled into more than one target (e.g. a `src/` module declared by both lib and bin crate roots) may carry genuinely ambiguous crate-root references; that is exactly what the typed refusal is for.
- **Quantified stakes** (from the blast-radius dogfood): the naive alternative — keep twins, refuse every twin reference — flips ~750 currently-aligned references to ambiguous (98.1% → ~92% alignment) on this repository; locality attribution should recover the large majority, with the residual honestly counted.
- **Determinism:** rank twins by definition document path (deterministic, meaningful) rather than input order; identities remain stable across discovery order per the existing baseline contract.
- **Crate-root follow-on:** with crate roots un-merged, each root keeps its own defining document, which should simplify or retire the multi-document module mapping workaround added during blast-radius (imports edges per actual defining module).

## Open Questions

- How much duplicated-group detail `status` should disclose (count only, or named groups) — a small surface decision to settle in design.
