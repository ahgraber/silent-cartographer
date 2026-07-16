# Proposal: chunk-tiers

## Intent

Agents retrieving code through c10r today face an all-or-nothing choice: a symbol's location, its signature, or its entire body.
There is no tier that answers "what is this and what does it document itself as?" — the cheapest question an agent asks when triaging candidates — and trace results carry no content at all, forcing a follow-up `get` per row.
This change establishes the chunk substrate: every persisted symbol becomes a retrievable unit with a fixed set of content tiers, persisted at build time, so retrieval depth is a per-query choice and the future find-by-intent pillar has a corpus to index without re-extraction.

## User Stories

### Story: fetch-at-depth

As an agent that has located a symbol, I want to retrieve it at a chosen depth — location, signature, signature-plus-documentation, or full body — so that I spend context only on what the task needs.

### Story: triage-trace-results

As an agent tracing a symbol's relations, I want each result row to carry content at a detail level I choose per query, so that I can judge candidate relevance without a follow-up lookup per row.

### Story: orient-by-summary

As a developer or agent orienting in an unfamiliar codebase, I want a module's or type's own documentation and signature without its full body, so that I can build a mental map at low token cost.

### Story: search-ready-substrate

As the product owner preparing the find-by-intent pillar, I want every symbol's tier content persisted uniformly at build time, so that a later search change can index it directly instead of re-extracting it.

## Scope

**In scope:**

- A per-symbol tier model on the persisted graph: every in-workspace symbol carries signature and interface (signature + its own documentation) content alongside its existing full-body span, extracted during the build's syntax pass and persisted with the build (code-graph).
- Cross-language module bodies: a Python module's persisted span becomes its whole document, matching Rust's existing behavior, so modules are chunks with all tiers in both languages (code-graph).
- Tier fallback semantics: a symbol with no documentation serves its signature as its interface tier; absence is never an error (code-graph, code-navigation).
- `get` gains the interface detail level, completing the axis location | signature | interface | body (code-navigation).
- `trace` gains an opt-in detail knob projecting each result row at a chosen tier; the default output shape is unchanged (code-navigation).
- Schema version bump; chunk tier rows obey the builds-wholly-supersede contract.

**Out of scope:**

- Any search machinery — FTS tables, identifier splitting, embeddings, fuzzy find (deferred to the semantic-lite/find change; design.md records a binding corpus constraint for it).
- Output bounding — `--max-chars`, `--limit`, `--cursor` (deferred to the CLI-alignment change; until then a module body returns a whole file).
- Per-chunk test-classification (`is_test`) — belongs to the deferred is-tested view.
- Diff-seeded queries (temporal anchoring stays a non-goal).
- Any config file; no new defaults requiring configuration.

## Approach

Chunks are derived from the aligned graph, never extracted in parallel: a symbol's persisted definition span is its body tier, and the same syntax-oracle pass that anchors the guarded join yields the signature and documentation sub-spans (Rust: item header up to the body delimiter, preceding `///` block, or the file-level `//!` block for modules; Python: the `def`/`class` header, the docstring statement, or the module docstring).
Tier content is persisted per symbol at build time (columns on the symbols table or span offsets into the stored body — design decides copies vs offsets), superseded wholly by each build, behind a schema version bump.
Python module symbols adopt whole-document definition spans, replacing the current empty span persisted from the zero-width origin marker.
`trace` detail projection reads the same persisted rows; no query-time parsing anywhere.
The prior tool's validated shape (declaration chunks × fixed content levels × signature-first economy) is adopted; its construction (parallel tree-sitter extraction string-joined to the graph, orphan re-homing, join-outcome taxonomy) is deliberately not, because the aligned graph already provides the join.

## Open Questions

- None — remaining choices (tier storage as copies vs offsets; signature extraction for header-less items like consts and fields) are design-level and resolved in design.md.
