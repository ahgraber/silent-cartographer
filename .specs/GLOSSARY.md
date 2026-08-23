# Glossary — silent-cartographer (`c10r`)

> Terms of art used by the baseline `specs/`, the north star, and the code.
> This is a reference for what a term denotes, not a contract: the specs define behavior, and on any conflict they win.
> One term per concept and one concept per term — a word that has to mean two things in `c10r` is a naming defect, not an entry.

## Scope and conventions

- An entry earns its place by being a term whose everyday meaning is not enough to read the specs correctly.
- Each entry says what the term denotes, not how it is implemented.
- Where a term is easily confused with a neighbor, the entry says which is which.
- Where a term is borrowed from an outside system and means something different there, the entry says so.

## Terms

### North Star & Product Framing

**Blast radius**
: The set of symbols a change to a subject symbol could break — its dependents, reached through the `dependents`/`impact` traversal.
North-star outcome 2 ("blast radius before change") names this the reason to see dependents before altering a symbol.

**Calibration**
: The discipline of never presenting an answer more confidently than its accuracy warrants.
The guiding principle "calibration over coverage" and north-star outcome 5 ("calibrated trust") tie this to choosing precision versus freshness per query and to every answer carrying its own provenance and staleness.

**One identity, two oracles**
: The guiding principle that a single stable canonical symbol identity is authored by two sources of authority by competence: the syntax tree owns structure and enclosure, the semantic index owns cross-file identity, resolution, and types.
Disagreement between the two is represented, never silently resolved.

**Syntax tree**
: The other of the two oracles the graph unifies: the always-fresh structural source, owning structure and enclosure, as opposed to the semantic index's identity/resolution/types.

**Semantic index**
: One of the two oracles the graph unifies: the source of cross-file identity, resolution, and types, as opposed to the syntax tree's structure and enclosure.
Not to be confused with "semantic-index identity" (the retrieval-side provenance token — embedding model, corpus definition version, chunk parameters — carried by `search`/`similar` answers); the two share a name but denote different subsystems.

**Provenance**
: The recorded origin of an answer or an attribution, required wherever the specs use the word: analyzer identity and version (and any environment fact a backend declares material) for a query result; the alignment rule that accepted an occurrence; the locality evidence that resolved a duplicate; the semantic-index identity behind a ranking.
Distinct from freshness/staleness — provenance says where an answer came from, staleness says whether that origin still matches current sources — though the two compose on the same answer.

**Staleness**
: The typed state marking whether a persisted result still matches its underlying sources and recorded provenance (analyzer identity/version, declared environment facts).
A result is fresh while none of those has changed and stale the moment any of them has; north-star outcome 4 ("honest under edit") relies on this distinction never being silently dropped.

### Symbol Identity

**Canonical identity**
: The deterministic, workspace-namespaced key assigned to every indexed symbol — a pure function of the symbol's resolved semantic descriptor, stable across re-indexing regardless of discovery order.
It is the join key between the two oracles and the round-trip key exposed to consumers; distinct from a descriptor, which is not guaranteed unique.

**Descriptor**
: The resolved semantic identifier a backend assigns to a symbol, prior to canonicalization.
Unlike a canonical identity, a descriptor is not guaranteed unique within a workspace — two distinct definitions can share one ("duplicated descriptor").

**Disambiguator**
: A value introduced into a canonical identity only where the resolved descriptor would otherwise collide with another symbol's, anchored to the colliding symbols' definition locations so identity stays stable across discovery order.

**Workspace identity**
: The identity of the workspace a build or store is indexed under, which qualifies every canonical identity so that identical descriptors from two different workspaces never collide.
Derived deterministically from the workspace root's directory name unless a caller supplies one explicitly, which always overrides the derived default.

### Code Graph — Join & Persistence

**Aligned attribution**
: The persisted record that a semantic occurrence has been reconciled with the syntactic construct at its location under an alignment rule; carries that rule as provenance.
An occurrence not persisted as an aligned attribution falls into one of the typed refusal outcomes instead.

**Current** _(index currency)_
: The state in which a stored index already describes the workspace being built exactly — same workspace identity and root, same sources, analyzer, declared environment, and chunk parameters.
A build against a current index analyzes nothing and leaves the store unchanged, though the system always offers an explicit way to build anyway.

**Discrepancy**
: A persisted record of a non-aligned join outcome — its location, outcome kind, expected symbol name, and the source text actually found there — wholly superseded by each subsequent build.

**Enclosure**
: The structural relation (`contains`/`containers`) between a symbol and the construct that directly encloses it.
Enclosure is persisted and traceable but is never itself a channel of dependence — `dependents` never propagates through it, only through the `uses`/`imports`/`type_hierarchy` edges.

**Guarded positional join**
: The mechanism that attributes each semantic occurrence to the syntactic construct at its source location, persisting the attribution as aligned only when an alignment rule's exact expectation is satisfied.
An occurrence satisfying no rule is never persisted as a confident attribution — it is recorded as a typed refusal instead.

**Name token**
: The terminal segment of a resolved descriptor — as opposed to its qualified path — that the default alignment rule compares against source text.
A tuple-field index token counts as a name token for this purpose.

**Occurrence**
: A role-classified (definition or reference), source-mapped mention of a symbol, as extracted by a semantic backend.
Distinct from a symbol (the persisted identified entity an occurrence is of) and from an attribution (the join's verdict on where an occurrence's source text lands relative to syntax).

**Store** _(index)_
: The persisted database artifact holding one build's complete graph — symbols, occurrences, edges, discrepancies, corpus, and provenance — wholly replaced by each successful build.
Used near-interchangeably with "index" throughout the specs (e.g. "index store", "the index is built") for the same artifact; not to be confused with the "surface index," which describes the CLI's own commands and flags rather than workspace content.

**Tier**
: One of three fixed levels of content the system persists for every symbol whose definition span is persisted: the signature tier (declaration without body), the interface tier (signature plus the symbol's own documentation, falling back to the signature when there is none), and the body tier (the definition span's source text).
The first two are derived from the source; the body tier is the source verbatim.
Distinct from the query-time detail axis, which offers these three plus `location` — a level carrying no content at all.

**Twin**
: A term used in the code-graph spec's scenario titles for one member of a duplicated-descriptor group — interchangeable with "duplicate" for an individual definition sharing a descriptor with others.

### Semantic Corpus & Retrieval

> The semantic index names three levels: a **document** is a source file, a **passage** is what one corpus-contributing symbol contributes, and a **chunk** is the bounded run of a passage handed to the embedding model.

**Bounded pool**
: The (possibly incomplete) candidate set a ranking signal in `search` or `similar` may draw from.
Part of the "candidates, not completeness" framing every semantic answer carries; a pool never changes how the candidates within it are ranked relative to one another.

**Chunk**
: A bounded slice of a passage's content — never exceeding the recorded chunk size, the passage's identifying header included — that exactly one vector representation derives from.
A passage within the chunk size is one chunk; a longer passage is covered by several, divided at structural boundaries (declarations/statements in code, sentences/paragraphs in prose) wherever the content offers them, and only at an arbitrary point when it offers none.

**Chunk parameters**
: The chunk _size_ and the _overlap_ between adjacent chunks in effect for a build, recorded as part of the build's semantic-index identity.
A change to either parameter is one of the inputs that makes a stored index no longer current, triggering a rebuild.

**Clone-equivalence key**
: One of two deterministic keys persisted for every corpus-contributing symbol with no other corpus-contributing symbol inside it (a leaf): a formatting-insensitive key (shared exactly when token sequences match after whitespace and comments are disregarded) and a substitution-insensitive key (shared exactly when token sequences also match under a consistent one-to-one substitution of identifiers and literals).
A container symbol carries no equivalence key at all.

**Corpus-contributing symbol**
: An in-workspace symbol eligible to appear in the semantic corpus.
A symbol whose every persisted content tier is exactly its own name token contributes nothing — such content carries nothing beyond the symbol's identity, and a name-only passage would displace content-bearing candidates in every answer.

**Exact-clone marker / Variant-clone marker**
: Typed markers a `similar` result row can carry, derived from deterministic clone-equivalence keys rather than from estimated similarity: exact-clone for a candidate sharing the subject's formatting-insensitive key, variant-clone for one sharing only the substitution-insensitive key (identical token structure, names/literals substituted — never a claim of behavioral equivalence).
Marked rows precede unmarked ones, and exact-clone rows precede variant-clone rows; within a certainty class, estimated-similarity order still applies.

**Heuristic-grade marker**
: The structural label every `tests`, `search`, and `similar` answer carries — even when typed-empty — identifying its content as convention-based classification or model-derived estimation rather than resolved semantic fact.
It accompanies, never replaces, the ordinary provenance/freshness/staleness labeling; an answer that terminates before classification or ranking is derived (ambiguous reference, unresolved subject) carries no marker at all.

**Lexical representation**
: The non-vector, text-search representation persisted per passage, derived from the passage's whole render regardless of how many chunks or vector representations that passage has.

**Passage**
: The semantic corpus's per-symbol unit: exactly one per corpus-contributing symbol, derived from the symbol's own content if it is a leaf or from its interface tier if it encloses other corpus-contributing symbols.
A passage may be represented by more than one chunk/vector when its content exceeds the chunk size, but it is still exactly one passage.

**Semantic corpus**
: The build-time-derived collection of passages over corpus-contributing symbols that backs `search` and `similar`.
Derivation is deterministic: identical build inputs yield identical passages.

**Semantic-index identity**
: The recorded combination — embedding model identity, corpus definition version, and chunk parameters — that produced a build's semantic representations, carried as provenance on every `search`/`similar` answer.
Not to be confused with "semantic index" the north-star oracle (cross-file identity/resolution/types); this identity concerns only the retrieval/embedding subsystem.

**Vector representation**
: An embedding derived from exactly one chunk, guaranteed independent of the batch it was embedded alongside — identical whether that chunk is embedded alone or with any other chunks, in any order.

### Code Navigation — Query Surface

**Continuation token**
: An opaque, deterministic token issued on a truncated result set that resumes exactly the remaining results.
Bound to the exact query identity it was issued for — including, where applicable, the ordering selector, the ranking model version, and (for `impact`) the seeding change — and rejected rather than silently resumed if presented against different parameters.

**Dependents**
: The relation and traversal answering "what could break if this symbol changes": symbols reaching a seed via `uses`/`imports`/`type_hierarchy` edges, directly or transitively, never through enclosure, each reported once at its shortest distance in hops.
The traversal's cost is bounded by the reachable set, not by the depth bound requested.

**Detail level**
: The per-query axis — `location`, `signature`, `interface`, or `body` — at which a result's content is projected, never altering which results are returned or their order.
For a reference-site result, detail projects the content of the declaration the site is attributed to, not the reference site itself; for a module symbol, body denotes the whole defining document rather than an in-document declaration span.
Three of its four levels name persisted tiers; `location` names none.

**Exact / Approximate** _(impact freshness)_
: The two labels every `impact` answer carries: exact when the index matches the change's pre-change state exactly, approximate otherwise — including whenever that pre-change state cannot be reconstructed from the workspace at all.
An approximate answer always carries a runnable, non-mutating recovery procedure toward an exact one.

**Impact**
: The `impact` command's answer: the union of `dependents` for a set of seed symbols derived from a git diff, with no forward/callee-direction reach — reverse-reachability only.
A change touching no indexed symbol is a typed-empty successful answer, not a failure.

**Relation kind**
: One of the seven named relations `trace` supports, following a shared-root directional convention: `containers`, `contains`, `references`, `dependents`, `importers`, `implementers`, and `tests`.
`tests` is presented in the command's self-description as convention-based classification, and `dependents` as impact assessment, distinct from the other, resolved-fact relations.

**Seed symbol / Seed mode**
: A seed symbol is one of the symbols an `impact` assessment starts from, resolved from a git diff's pre-change side (so a deleted or renamed symbol still resolves).
Seed mode selects which diff feeds seeding: the working-tree change (default), the staged change, or a revision range — the last of which draws its dependents from the single current index rather than either endpoint, which the answer discloses.

### MCP Surface

**Elicitation capability**
: The client-offered mechanism a lifecycle tool uses to obtain the caller's confirmed consent before performing its operation.
When the connected client offers none, the tool refuses outright rather than issuing a request the client cannot answer.

**Lifecycle tool**
: An MCP tool that performs a state-changing operation (`build`, `hooks install`), as opposed to the query tools that mirror read-only commands one to one.
Requires both an explicit acknowledgment parameter and confirmed consent via elicitation before it acts; a declined or uncancellable confirmation leaves every store, file, and repository unchanged.

**Workspace root** _(MCP)_
: The single workspace every tool invocation resolves to: the root explicitly named on the call, else the server's configured launch-time default, else the directory the server process itself resolves to.
A configured root that fails to resolve to an existing directory is refused at startup rather than silently replaced by a fallback.
