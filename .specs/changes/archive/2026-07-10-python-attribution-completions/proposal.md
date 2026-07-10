# Proposal: python-attribution-completions

## Intent

The module-scope-rules dogfood left the Python refusal residual fully characterized (its `dogfood.md` and brainstorm §11 carry counts, span shapes, and sample sites per family).
Four families carry real, checkable evidence the current rule inventory does not consume:

- **Import-alias references** — a token spelling a local alias (`from jinja2 import Environment as BaseEnvironment`; Flask ~11 refs) refuses even though the binding statement sits in the same document; the assignment-bound variant (`pytest.mark` → `MARK_GEN`, httpx2 248 refs) binds in third-party code and may or may not be evidence-reachable — the design decides, and whatever stays refused is documented residual.
- **Module self-name idiom** — `Flask(__name__)` / `__file__` sites (Flask 188 refs) resolve to the containing document's own module, a fact the store already derives; the token spells the module's runtime name rather than its spelled name.
- **Prefix-token module spans** — scip-python puts a submodule occurrence's range on the leading token (`h2` carrying `h2.connection`; httpx2 201, Flask ~33), but the enclosing dotted construct's text spells the module in full — syntax evidence the module-name rule's span-level gate cannot see.
- **Zero-width module markers** — the module definition markers (httpx2 119, Flask ~83) that the document→module bookkeeping already trusts, still counted as refusals: accounting noise rather than caution.

The fifth family — scip-python's representative-symbol misattribution through star re-exports (httpx2 787, Flask ~65) — is wrong SCIP data and is explicitly not ruled around; this change drafts the upstream issue instead.

## User Stories

### Story: alias-aware-references

As an AI agent tracing references in codebases that alias their imports, I want reference sites spelled by a local alias attributed to the aliased symbol, so that reference sets and impact traces include alias sites instead of refusing an idiom most real codebases use.

Ladders to the north star's precise-answer and blast-radius outcomes.

### Story: idiomatic-module-evidence

As a developer whose code uses Python's module idioms — `__name__` at app construction, dotted module paths, package markers — I want those sites attributed from the evidence each idiom actually carries, so that module-level answers are complete and the accounting reflects caution only where evidence is genuinely absent.

Ladders to the north star's calibrated-confidence outcome.

## Scope

**In:**

- An import-alias alignment rule: a token spelling a name the containing document binds to the occurrence's resolved symbol via an import-alias statement is accepted, with the binding as provenance; the recognized binding forms per language are a design decision (Python `import … as` / `from … import … as` first; Rust `use … as` / facade `pub use` as the design allows — ripgrep's 32 facade refusals are the Rust instance).
- Assignment-bound aliases (`pytest.mark`): included only as far as checkable evidence reaches; the design settles whether any exists, and the residual is recorded, not forced.
- A module self-name rule: a module occurrence at a `__name__` or `__file__` token is accepted when the resolved module is the containing document's own module.
- Dotted-expression completion of the module-name rule: a module occurrence whose span sits within a dotted construct is accepted when that construct's text (after any leading relative-import dots) equals a trailing component-run of the module's dotted namespace name.
- A module-marker rule: scip-python's zero-width origin definition marker is accepted as the module's definition attribution — the same index shape the document→module bookkeeping already trusts.
- Accounting buckets and provenance values for each new rule; one schema bump (v8 → v9).
- Drafting the upstream scip-python issue for the star-re-export misattribution (filing it is the user's action, not the assistant's).
- Dogfood: the standing five targets (httpx2 v2.5.0, Flask 3.1.3, ripgrep, fd, self) with stated expected deltas per family and the scip-check disclosure gate; every deviation investigated before recording.

**Out:**

- Any acceptance of scip-python's representative-symbol misattributions (star re-exports, `os.path`) — wrong SCIP data stays refused regardless of what any new rule could match.
- Forking or patching scip-python.
- New query surfaces, live indexing, mixed-language stores.

## Approach

High-level direction; mechanism formalizes in design.md.

- **Alias rule:** the syntax layer contributes a per-document alias table (alias name → aliased target's name evidence) from import-alias statements; the join accepts a refused-by-text token when the alias table maps its text to the expected symbol's name under the existing rule conditions.
  Rust facade `pub use` feeds the same table shape if the design brings it in; assignment-bound aliases enter only if the semantic index itself supplies a checkable binding.
- **Self-name rule:** condition on the existing document→module derivation — the occurrence's resolved module must equal the containing document's module; token text `__name__`/`__file__` is the trigger, the equality is the evidence.
- **Dotted completion:** widen the module-name rule's structural gate from the span to the smallest enclosing dotted construct, comparing the construct text under the unchanged trailing-component-run condition.
- **Marker rule:** accept the zero-width origin definition when the symbol is module-kind and its document matches — the acceptance mirrors the bookkeeping that already consumes the shape.
- **Sequencing:** this change's deltas modify requirements as amended by `module-scope-rules`; sync order is module-scope-rules first, then this change.

## Open Questions

- Whether any checkable evidence exists for assignment-bound third-party aliases (`pytest.mark`), or the family is recorded residual for this round.
- Whether Rust `pub use` facade aliases land in this change's alias rule or stay a recorded residual (they surface through the twin-locality path, not plain text mismatch — the design must reconcile the two paths).
