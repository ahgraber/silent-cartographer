# Design: chunk-tiers

## Context

The store already materializes each in-workspace symbol's definition span and its exact text (`symbols.span_start/span_end/span_text`), so the body tier exists today; this change adds the two cheaper tiers and makes module bodies uniform across languages.
The build already runs a full syntax pass per document (`graph::definition_span` walks `SyntaxTree::all_declarations` to find each symbol's declaration), so tier extraction rides an existing crossing — no new oracle contact, no parallel extraction.
The prior tool's chunking was harvested under a user-granted crossing (`._scratch/c10r-chunking-harvest.md`); its product shape is adopted, its construction (parallel tree-sitter extraction string-joined to the graph, orphan re-homing, a join-outcome taxonomy for chunk↔node mismatches) is rejected because the aligned graph already provides the join.
Output bounding is deferred to the CLI-alignment change; until it lands, a module body response is a whole file by accepted decision.

## Decisions

### Decision: BoundaryOracleIsTreeSitter

**Chosen:** chunk locations come from the semantic oracle (the symbol set is SCIP's), and chunk _extents_ — where a declaration starts and ends, where its header stops, where its documentation sits — come from tree-sitter, in the same build pass that anchors the guarded join.

**Rationale:** tree-sitter is already load-bearing (every aligned occurrence is anchored to a tree-sitter construct; `definition_span` already walks its declarations), it is error-tolerant (boundaries stay computable on broken, mid-edit code — the honest-under-edit outcome), and it is uniform across current and rollout languages.
Any other extent source would introduce a third span coordinate system to reconcile against the two the guarded join already unifies.
The division of authority is the project spine: identity from semantics, extent from syntax.

**Alternatives considered:**

- SCIP `enclosing_range`: gives name-token spans, not declaration extents; the field is absent from most indexers, and the contract floor forbids mandatory behavior on a capability only some backends provide; also build-gated and stale — the wrong freshness tier for structure.
- LSP `documentSymbol`: accurate ranges but needs a resident server per language and abuses an interactive protocol for batch extraction; LSP is the deferred _semantic_ overlay, not a structure source.
- Native compiler ASTs (`syn`, Python `ast`): precise but not error-tolerant, per-language bespoke (Python's requires a Python runtime inside a Rust binary), and a third span system.
- Textual heuristics (regex, brace counting, indentation): break on strings/comments/macros; grep-grade unreliability the product exists to beat.
- Fixed/sliding windows or embedding-based splitting: slicing for unstructured text; a window is not a symbol, so it breaks the chunk↔identity equality that makes retrieval exact.

**Known limits:** grammar quirks are real (dogfoods hit tree-sitter-rust range-expression ambiguities), and doc-comment association is a convention implemented on top of the tree, not grammar-given — both absorbed by the total-extraction fallback and sampled by the five-target dogfood.

### Decision: TierStorageAsMaterializedColumns

**Chosen:** two nullable TEXT columns on `symbols` — `signature_text`, `interface_text` — populated at build; the body tier remains `span_text`.

**Rationale:** the interface tier is not always a slice of the stored body — Rust doc comments sit _outside_ the item node's span (`decl.full_span` excludes the preceding `///` run), so offsets into `span_text` cannot represent interface content.
Materialized copies keep the read path a plain column read, and give the future search change directly indexable columns.
The duplication is bounded: signatures and doc blocks are small relative to bodies.

**Alternatives considered:**

- Offsets into `span_text`: cannot express Rust interface content (doc text precedes the span); saves little.
- A separate `chunk_tiers` table: 1:1 with symbols — a join for nothing.
- Query-time extraction: violates the snapshot model (a query would parse current files against a stored index) and pays a parse per query.

### Decision: TierExtractionRules

**Chosen:** per-language, kind-scoped extraction in the build's syntax pass:

- **Rust signature:** the item's text from the start of its node to its body delimiter — a function's header up to the block `{`, a struct/enum/trait header up to its `{`; items terminated without a distinct body (`;`-terminated consts, statics, type aliases, bodiless trait fns, unit/tuple structs) take the full declaration, per the spec's no-distinct-body clause.
  Attributes on the item are part of the declaration form and stay in the signature.
- **Rust documentation:** the contiguous run of outer doc comments (`///`, `/** */`) immediately above the item (attributes may sit between the docs and the item); for a file module, the leading inner doc run (`//!`, `/*! */`) at document start.
- **Python signature:** the `def`/`class` header from node start through the header-ending `:`, decorators included (they are caller-relevant declaration form).
- **Python documentation:** the docstring — the body's first statement when it is a string expression; for a module, the document's first statement when it is a string expression.
- **Interface tier:** documentation and signature joined in source order (Rust docs precede the header; Python docstring follows it); equal to the signature when no documentation exists.
- **Extraction never fails a build:** any kind the rules do not recognize falls back to signature = full declaration, interface = signature.

**Rationale:** these are the two shipped languages' native documentation forms; the fallback clause keeps tier extraction total so an exotic node kind degrades to honest, coarser content instead of an error or an empty field.

**Alternatives considered:**

- Excluding decorators/attributes from signatures: they carry caller-relevant contract (`#[must_use]`, `@property`) — excluding them makes the tier less honest for triage.
- Treating `#[doc = "…"]` attributes as documentation: rare in hand-written code; deferred until dogfood shows it matters.

### Decision: ModuleSignatureIsItsName

**Chosen:** a module's signature tier is its declaration form as the graph knows it — the module's qualified name — not a slice of document text; its interface tier is its signature followed by its module documentation, keeping the uniform interface contract (signature together with documentation) and the uniform fallback (the signature alone when no documentation exists); its body is the whole document.

**Rationale:** a file module has no in-document declaration header, and the current query-time fallback (cut the whole file at the first `{`) produces garbage.
The qualified name is honest, derived metadata — the one thing a module's "signature" can truthfully be.

**Alternatives considered:**

- Synthesizing `mod name;` / `import name` source forms: fabricates bytes that exist nowhere in the workspace.
- Refusing the signature tier for modules: a kind-specific hole in a four-tier contract, for no gain.

### Decision: PythonModuleWholeDocumentSpan

**Chosen:** module-kind symbols get an explicit whole-document definition span at persistence in both languages — `(0, document.len())`, text = the document — replacing the current implicit behavior (Rust: whole-doc via the name-span fallback; Python: the zero-width origin marker persisting an empty span).

**Rationale:** one explicit module branch replaces two accidental, divergent outcomes; the module-marker rule's identity bookkeeping (`module_by_document`) is untouched — only the persisted span of the module symbol changes.

**Alternatives considered:**

- Widening the Python marker occurrence itself: the occurrence's zero-width range is faithful backend output and feeds the join accounting; persistence is the right layer for the span decision.

### Decision: TraceDetailAsRowProjection

**Chosen:** `trace --detail <location|signature|interface|body>` projects tier content onto each result row as an additional content field; omitted, the output is today's shape; the flag never changes result membership or order.
Rows that denote symbols (`containers`, `contains`, `dependents`) project the symbol's own tiers; rows that denote reference sites (`references`) project the tiers of the declaration the site is attributed to — the enclosing-declaration attribution the join already persists.
A site attributed to the module/file itself (a NULL attribution) projects the document's module symbol, selected by the widest module definition occurrence in that document: a file module's definition occurrence spans the whole document while an inline `mod` block's or a re-export-defined module's sits on a name token, so the file module wins even when persisted spans tie (a re-export-defined module persists the same whole-document span as the file module it is declared in, and identity order alone would pick between them arbitrarily).
`get --detail` gains `interface`, served from the persisted column; the query-time `signature_of` helper is replaced by the persisted `signature_text` for `get --detail signature`.

**Rationale:** projection-not-machinery is the brainstorm's own framing of the granularity knob ("an enum on the result"); replacing `signature_of` removes a second, weaker signature definition so both commands serve one persisted truth.

**Alternatives considered:**

- Signature-first default content on every trace row (the prior tool's behavior): changes the current output contract and grows every response; rejected in exploration.

### Decision: SearchCorpusConstraint (binding on future changes)

**Chosen:** recorded constraint for the deferred find/semantic-lite and embedding changes: any search or embedding corpus over chunk content MUST be defined over non-nested content — leaf declaration bodies plus container interface tiers, or each symbol's exclusive text (its span minus its children's spans) — and MUST NOT index nested full bodies.

**Rationale:** enclosure duplicates content at every level (a type's body contains its methods; a module's body is the whole file); indexing nested bodies double-counts lexical statistics, returns duplicate hits, and doubles embedding cost.
This change deliberately keeps container bodies retrievable (uniform contract, real fetch-whole-module value), so the duplication concern must be discharged where it actually lives: the corpus definition.

**Alternatives considered:**

- Demoting containers from chunks to protect search: does not protect it (class bodies still nest method bodies) and amputates module docs from the future corpus.

## Architecture

```text
build:
  adapter (SCIP) ──► identity ──► guarded join ──► persistence
                                       │                │
                       syntax oracle ──┤                ├── symbols (+ signature_text, interface_text)  [SCHEMA_VERSION 10 → 11]
                                       │                │      body tier = span_text (unchanged)
                       tier extraction ┴────────────────┘      module span = whole document (both languages)

query:
  get  --detail location|signature|interface|body ──► column read (+ fallbacks resolved at build, not query)
  trace --detail …  ──► same traversal, per-row tier projection (membership/order untouched)
```

## Risks

- **tree-sitter node-kind coverage** (signature boundary unknown for an item kind): the total-extraction fallback (signature = full declaration) degrades gracefully; dogfood on the five standard targets samples the real distribution.
- **Python header edge cases** (multi-line parameter lists, async, decorated classes): covered by dedicated extraction tests; header ends at the `:` closing the header, which tree-sitter exposes structurally rather than textually.
- **Store growth** from two new text columns: bounded by signature/doc size; observed growth reported in the dogfood record.
- **Behavior change in `get --detail signature`** (persisted tier replaces the query-time `{`/`;` heuristic): existing scenarios re-run against the new source of truth; divergences will surface in tests as improvements (the heuristic mis-handled bodies containing `;` before `{` only in edge cases the tier extraction handles structurally).
- **Python store rows change** (module spans): the httpx2/Flask byte-identical rebuild gate from prior changes does NOT apply to this change by design; the dogfood record documents the expected delta (module rows only) and verifies non-module rows are unchanged.
- **Name-token tiers for declaration-less symbols**: symbols whose definitions match no persisted declaration kind (derive-synthesized methods anchored at a type name, enum-variant and associated constants, type parameters) persist their name token as body, signature, and interface alike — the same name-span definition they persisted before this change; tiers mirror it rather than fabricate content. Widening the persisted-declaration kind set would give some of these real declaration tiers, but that set feeds enclosure and attribution, so it is a follow-up change, not tier machinery.
- **Human (non-JSON) rendering is Debug-derived and not a stability surface**: `render` formats answers with `{:#?}`, so the trace row's struct-variant change alters the human dump (it now prints `content: None` on undetailed rows). JSON is the stable machine surface (`content` is skip-if-none there); real human rendering is owned by the deferred CLI-alignment change.
- **`trace --detail body` multiplies bodies per row**: every reference site clones its attributed declaration's full body, and a module-scope site clones a whole document — references × body-size output with no bound until the CLI-alignment change lands `--max-chars`/`--limit` (output bounding is out of scope here by accepted proposal decision; this bullet makes the deferred exposure explicit for that change).
