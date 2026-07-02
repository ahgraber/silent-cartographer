# Tasks: precise-locate-rust

## Scaffold

- [x] Create the Cargo crate (binary + library) with the pinned toolchain, `clap` (derive) for the CLI skeleton, and the SQLite dependency.
- [x] Stand up the baseline `specs/` directory so `sdd-sync` has a contract floor to merge into.
- [x] Add the relation/role domain terms (`callers`, `callees`, `containers`, `importers`) to `.typos.toml` so specs and code stop tripping the spell-check.

## symbol-identity

- [x] Define a `CanonicalId` newtype and the projection from a SCIP descriptor to the conceptual `<workspace>::<module_path>::<qualname>[disambiguator]` form.
- [x] Implement workspace namespacing as a mandatory prefix on every projection.
- [x] Implement collision-only disambiguation: detect a within-workspace projection collision and append a disambiguator only then; emit no suffix when none is needed.
- [x] Test (determinism): re-projecting unchanged sources yields byte-identical identities. _(Deterministic canonical identity)_
- [x] Test (order independence): two runs visiting files in different orders produce the same identity per symbol. _(Deterministic canonical identity)_
- [x] Test (namespacing): identical descriptors from two workspaces project to distinct identities. _(Workspace-namespaced identity)_
- [x] Test (two-workspace ingest): the same descriptor ingested under two workspaces persists two distinct symbols and queries do not conflate them. _(Workspace-namespaced identity — persisted path; verify remediation 2026-07-02)_
- [x] Test (uniqueness): a corpus of distinct symbols yields no duplicate identities. _(Identity uniqueness within a workspace)_
- [x] Test (no vacuous suffix): Rust symbols, which do not collide, carry no disambiguator suffix while uniqueness still holds. _(Identity uniqueness within a workspace)_

## semantic-engine

- [x] Define the `SemanticEngine` port trait at the mandatory intersection: produce symbols with resolved descriptors and role-classified occurrences carrying mappable ranges, plus analyzer provenance.
- [x] Define queryable feature detection for optional capabilities (enclosure information, base-index eligibility, live updates).
- [x] Implement the Rust adapter wrapping `rust-analyzer scip` and `tree-sitter-rust` behind the port.
- [x] Build the backend conformance suite that encodes the mandatory extraction contract.
- [x] Test (definition extraction): a declaration yields the symbol with a definition-role occurrence whose range maps to the declaration. _(Mandatory extraction contract)_
- [x] Test (reference extraction): a use site yields a reference-role occurrence with a mapping range. _(Mandatory extraction contract)_
- [x] Test (conformance gate): a backend that violates a contract clause is reported non-conformant and not treated as usable. _(Mandatory extraction contract)_
- [x] Test (provenance): an ingested index records the adapter's analyzer name and version, retrievable as provenance. _(Backend provenance reporting)_
- [x] Test (capability declared): a backend declaring enclosure support is detected present and relied upon. _(Optional capabilities are queryable)_
- [x] Test (capability undeclared): a backend not declaring base-index eligibility is not used as a base index and its absence is explicit. _(Optional capabilities are queryable)_
- [x] Test (exemplar conformance): a checked-in SCIP index fixture run through the Rust adapter's translation passes the backend conformance suite. _(Mandatory extraction contract — exemplar write-site; verify remediation 2026-07-02)_
- [x] Test (capability live-updates): the live-updates capability is queryable and its absence on a non-declaring backend is explicit. _(Optional capabilities are queryable — live-updates arm; verify remediation 2026-07-02)_

## code-graph

- [x] Define the SQLite schema: symbols (identity, kind, span), occurrences (symbol, range, role), edges (type-tagged), and index metadata (analyzer provenance, source content hash); reserve room for vector columns and an FTS5 index without committing their shape.
- [x] Implement range normalization via each document's declared position encoding onto tree-sitter byte offsets.
- [x] Implement the join: range-containment match to the syntactic name node, with the text-equality guard asserting the matched bytes equal the expected name token (the descriptor's terminal segment, not the qualified path).
- [x] Implement the content-hash gate that refuses to join a tree against a non-matching index.
- [x] Implement persistence of symbols, occurrences, `contains` edges, and spans.
- [x] Populate `calls`, `imports`, and `type_hierarchy` edges during ingest, each carrying the "unverified until proposal 2" marker.
- [x] Implement enclosing-declaration attribution: every aligned reference occurrence records the nearest enclosing persisted declaration, with the module as the outermost attribution.
- [x] Implement the local/external ingest policy: exclude file-local symbols from the persisted base; persist external symbols as a distinct class (identity and reference occurrences, no definition span).
- [x] Implement join-alignment accounting: persist per-build counts of aligned, text-mismatch, semantic-only, and syntax-only outcomes.
- [x] Implement enclosure persistence and the enclosing/children retrievals.
- [x] Implement staleness evaluation against both the source content hash and the recorded analyzer version.
- [x] Test (join aligned): an occurrence whose location spells the symbol name is persisted as an aligned attribution. _(Guarded positional join — canonical write-site)_
- [x] Test (join text mismatch): an occurrence whose location does not spell the expected name is not persisted as aligned and the mismatch is surfaced, including a non-ASCII drift case. _(Guarded positional join — mismatch branch)_
- [x] Test (join semantic-only): a SCIP occurrence with no syntactic construct at its location is recorded as unaligned, not misattributed. _(Guarded positional join — SCIP-only branch)_
- [x] Test (join syntax-only): a syntactic construct the backend did not resolve is retained structurally without a fabricated identity. _(Guarded positional join — AST-only branch)_
- [x] Test (round-trip): a persisted symbol returns the same identity, occurrence set, and exact span text. _(Lossless symbol persistence)_
- [x] Test (enclosure): a method's enclosing type is returned, and a module's direct contents are exactly its children. _(Enclosure is persisted)_
- [x] Test (fresh): unchanged sources and analyzer report results as fresh. _(Staleness reflects underlying change — fresh branch)_
- [x] Test (stale on content): changed source content marks derived results stale. _(Staleness reflects underlying change — content write-site)_
- [x] Test (stale on version): a differing analyzer version marks results stale and flags reindex. _(Staleness reflects underlying change — version write-site)_
- [x] Test (attribution method): an aligned reference inside a method body is attributed to that method. _(Reference occurrences carry enclosing-declaration attribution)_
- [x] Test (attribution closure): a reference inside a closure is attributed to the declaring function. _(Reference occurrences carry enclosing-declaration attribution — closure branch)_
- [x] Test (attribution module-level): a module-scope reference is attributed to the module, never a fabricated declaration. _(Reference occurrences carry enclosing-declaration attribution — module branch)_
- [x] Test (locals excluded): a parameter or let-binding does not appear in the persisted symbol table. _(design.md ingest policy — local symbols)_
- [x] Test (external class): a reference to a third-party symbol persists under the external class with no fabricated definition span. _(design.md ingest policy — external symbols)_
- [x] Test (accounting recorded): a build records all four join-outcome counts. _(Join alignment accounting)_
- [x] Test (accounting conserves): aligned + text-mismatch + semantic-only equals the total semantic occurrences processed. _(Join alignment accounting — conservation)_

## code-navigation

- [x] Implement reference resolution mapping a shortname, qualified name, or canonical identity to the symbol(s) it denotes, returning a typed candidate set on ambiguity.
- [x] Implement `get` with the detail axis (location / signature / body) and by-position entry that resolves to the enclosing symbol.
- [x] Implement `trace` over the `containers`, `contains`, and `references` relations, with a type subject's `references` returning its type-occurrences.
- [x] Implement the output contract: each result carries identity + human-readable name, provenance, freshness, typed absence, deterministic ordering, and a `--json` rendering.
- [x] Wire `get` and `trace` into the CLI.
- [x] Test (resolve qualified): a qualified name denoting one symbol resolves uniquely. _(Symbol reference resolution — canonical path)_
- [x] Test (resolve ambiguous): a shared shortname returns a typed candidate set, not an arbitrary pick. _(Symbol reference resolution — ambiguity branch)_
- [x] Test (resolve identity): a canonical identity round-trips to exactly one symbol. _(Symbol reference resolution — identity branch)_
- [x] Test (get location): `get` at location detail returns the definition file and position. _(Symbol retrieval at a chosen detail)_
- [x] Test (get body): `get` at body detail returns the span text byte-for-byte. _(Symbol retrieval at a chosen detail)_
- [x] Test (get signature): `get` at signature detail returns the signature without the body. _(Symbol retrieval at a chosen detail)_
- [x] Test (get by position): `get` for a position returns the enclosing symbol. _(Symbol retrieval at a chosen detail — position write-site)_
- [x] Test (trace contains): `trace` over `contains` returns exactly the direct members. _(Relationship trace — contains write-site)_
- [x] Test (trace containers): `trace` over `containers` for a method returns its enclosing type. _(Relationship trace — containers write-site)_
- [x] Test (trace references): `trace` over `references` returns every reference site with none omitted. _(Relationship trace — references write-site)_
- [x] Test (trace type references): `trace` over `references` for a type returns its use sites. _(Relationship trace — type-subject write-site)_
- [x] Test (trace empty): a subject with no instances of a relation returns typed absence. _(Relationship trace — empty branch)_
- [x] Test (output provenance + freshness): a fresh result carries provenance and is marked fresh. _(Calibrated output contract — get write-site)_
- [x] Test (output identity + name): each returned symbol carries both its canonical identity and a human-readable name. _(Calibrated output contract)_
- [x] Test (output stale): a result over changed sources is marked stale, exercised through both `get` and `trace`. _(Calibrated output contract — both query write-sites)_
- [x] Test (output determinism): repeated identical queries return locations in the same order. _(Calibrated output contract)_
- [x] Test (output ambiguous): `get` with a shortname shared by several symbols returns the typed ambiguous candidate outcome, asserted through the JSON rendering. _(Calibrated output contract — ambiguous write-site; Symbol retrieval at a chosen detail — ambiguous branch; verify remediation 2026-07-02)_
- [x] Test (output absent): `get` for a reference denoting no symbol returns the typed absent outcome, distinct from both an empty relation and a failure, asserted through the JSON rendering. _(Calibrated output contract — absent write-site; verify remediation 2026-07-02)_

## Operational

- [x] Implement the `build` command that (re)builds the index for a workspace through the ingest path.
- [x] Implement the `status` command that reports the index's analyzer provenance, freshness/staleness, and the join-alignment counts.
- [x] Add the git-hook reindex doorbell that triggers `build`.
- [x] Test (build): `build` over a fixture workspace produces a queryable index. _(exercises the join + persistence path end-to-end)_
- [x] Test (status): `status` reports the recorded provenance, the current freshness state, and the join-alignment counts. _(Backend provenance reporting; Staleness reflects underlying change; Join alignment accounting — CLI surface)_
