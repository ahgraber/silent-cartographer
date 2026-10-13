# Delta for Code Navigation Specification

## MODIFIED Requirements

### Requirement: Impact freshness across the straddle

> Previously: the rule carried a rationale about the index's internal structure; the behavior is unchanged.

Because the index is a point-in-time snapshot that may not match the change being asked about, the system SHALL label every impact answer as either exact — the index matches the change's pre-change state, meaning the workspace's discovered sources with the change reverted — or approximate — it does not.
A change whose pre-change side the system cannot reconstruct from the workspace SHALL be labeled approximate rather than assumed to match.
A changed region in a document the index does not hold SHALL be reported as unmappable rather than silently dropped; a region in a document the index holds, where the index has no declaration, SHALL NOT be reported unmappable, because an approximate answer already discloses that declarations may be missing.
A changed file whose pre-change content cannot be obtained at all SHALL be reported unmappable in whole, whether or not the index holds that document: no region within it can be located, so this is not the case above of a located region that no declaration overlaps.
An approximate answer SHALL carry a runnable recovery procedure that names the resolved base revision, the index location, and the workspace identity needed to produce an exact answer, so the caller is never left to reconstruct those by hand.
The procedure SHALL NOT modify the index the query was answered from, and following it SHALL yield an exact answer for the seed mode it was emitted for.

Serves: honest-diff-reach

#### Scenario: A change whose pre-change side cannot be reconstructed is approximate

- **GIVEN** a staged change to a file that also carries unstaged edits, so reverting the staged change alone does not reconstruct the file's pre-change content
- **WHEN** `impact` is invoked in staged mode
- **THEN** the answer is labeled approximate rather than exact

#### Scenario: The recovery procedure leaves the queried index untouched

- **GIVEN** an approximate impact answer whose recovery procedure has been followed to completion
- **WHEN** the index the original query was answered from is inspected
- **THEN** it is unchanged, rather than replaced by an index built at the change's base revision

#### Scenario: Following the recovery procedure yields an exact answer

- **GIVEN** an approximate impact answer for a revision range
- **WHEN** its recovery procedure is followed and the assessment re-run as the procedure directs
- **THEN** the re-run answer is labeled exact

#### Scenario: Matching index yields an exact answer

- **GIVEN** an index whose content matches the change's pre-change state
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled exact

#### Scenario: An untracked source file does not forfeit exactness

- **GIVEN** an index built while an untracked source file was present, with no edits since apart from the change under assessment
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled exact

#### Scenario: Mismatched index yields an approximate answer

- **GIVEN** an index whose content does not match the change's pre-change state
- **WHEN** `impact` is invoked
- **THEN** the answer is labeled approximate rather than presented as exact

#### Scenario: A changed symbol absent from the index is reported unmappable

- **GIVEN** an approximate run in which a changed region maps to no symbol present in the index
- **WHEN** `impact` is invoked
- **THEN** that region is reported as unmappable rather than omitted without trace

#### Scenario: A file whose pre-change content cannot be obtained is unmappable in whole

- **GIVEN** a change to a file the index holds a document for, whose pre-change content cannot be obtained
- **WHEN** `impact` is invoked
- **THEN** the whole file is reported unmappable, rather than its regions being dropped as merely overlapping no declaration

#### Scenario: An approximate answer carries a runnable recovery recipe

- **GIVEN** an approximate impact answer
- **WHEN** its disclosure is rendered
- **THEN** it includes a runnable recovery procedure naming the resolved base revision, the index location, and the workspace identity that together produce an exact answer

### Requirement: Change seed selection

> Previously: the rejection was stated through the internal record a continuation token is tied to; the behavior is unchanged.

The system SHALL seed the impact assessment from a git diff selected by a seed mode — the working-tree change (default), the staged change, or a revision range — optionally narrowed to a set of paths, and SHALL recognize a renamed file as the same file across the change, so that path narrowing reaches it by either its pre-change or post-change path.
The seed SHALL be taken from the change's pre-change side — content and path alike — so that a symbol removed or renamed by the change is still resolved to its prior identity and its dependents are still reported.
Because a revision range's dependents are drawn from the single current index rather than from either endpoint, an answer seeded from a range SHALL disclose which snapshot its dependents reflect.
A continuation token presented after the seeding change has moved SHALL be rejected rather than resumed against a different change's results.

Serves: scope-the-diff, diff-seeded-blast-radius

#### Scenario: A range answer discloses which snapshot its dependents reflect

- **GIVEN** a revision range whose seeds resolve against the range's pre-change side
- **WHEN** `impact` is invoked for that range
- **THEN** the answer states that the dependents reported are those the current index holds, not those of either endpoint

#### Scenario: A continuation token is refused after the change moves

- **GIVEN** a truncated impact answer and its continuation token
- **WHEN** the seeding change is altered and the token is presented again
- **THEN** it is rejected as a usage error rather than resumed against the altered change's results

#### Scenario: Working-tree change seeds the assessment

- **GIVEN** a workspace with uncommitted edits to an indexed symbol
- **WHEN** `impact` is invoked in its default mode
- **THEN** the seed is the symbol touched by the working-tree change and its dependents are returned

#### Scenario: Staged mode seeds only staged changes

- **GIVEN** a workspace with both staged and unstaged edits to different indexed symbols
- **WHEN** `impact` is invoked in staged mode
- **THEN** the seed is drawn only from the staged change, and the unstaged edit does not contribute to the seed

#### Scenario: Revision-range mode seeds from the range

- **GIVEN** two revisions between which an indexed symbol changed
- **WHEN** `impact` is invoked for that revision range
- **THEN** the seed is the symbol changed across the range and its dependents are returned

#### Scenario: Path narrowing restricts the seed

- **GIVEN** a change touching indexed symbols in two directories
- **WHEN** `impact` is invoked narrowed to one of those directories
- **THEN** only symbols under that path contribute to the seed

#### Scenario: A deleted symbol still resolves to its dependents

- **GIVEN** an index matching the change's pre-change state, and a change that deletes an indexed symbol other declarations depended on
- **WHEN** `impact` is invoked
- **THEN** the deleted symbol is resolved from the pre-change side and its dependents appear in the impact answer

#### Scenario: Path narrowing reaches a renamed file by its post-change path

- **GIVEN** an index matching the change's pre-change state, and a change that renames a file into a directory and edits an indexed symbol within it
- **WHEN** `impact` is invoked narrowed to the file's post-change directory
- **THEN** the edited symbol is still seeded from its pre-change side, rather than the narrowing reducing the change to an addition with no pre-change side

#### Scenario: A renamed file's changes still map

- **GIVEN** an index matching the change's pre-change state, and a change that renames a file and edits an indexed symbol within it
- **WHEN** `impact` is invoked
- **THEN** the edited symbol is resolved at its pre-change path and its dependents appear in the impact answer

### Requirement: Heuristic-grade answers are structurally labeled

> Previously: the rule stated the order of internal steps for an ambiguous or unresolved answer; the behavior is unchanged.

The system SHALL carry, in every `tests` answer — in the machine answer and in the human rendering alike — a structural marker identifying the answer as derived from convention-based test classification rather than from resolved semantic fact; each returned site SHALL carry, as provenance, the convention rule that classified its enclosing declaration; and the marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
An empty `tests` answer asserts only that no convention-classified reference site was found — never that nothing tests the subject — and SHALL carry the marker like any other `tests` answer.
An ambiguous-reference or unresolved-subject answer contains no classification-derived content, so it SHALL NOT carry the marker — the marker asserts a derivation, never merely the relation requested.
An answer of a relation derived only from resolved reference evidence SHALL NOT carry the heuristic-grade marker.

Serves: honest-heuristic-label

#### Scenario: Machine answer carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is returned as JSON
- **THEN** it carries a structural field marking the answer as convention-based classification

#### Scenario: Human render carries the marker

- **GIVEN** a `tests` query with one or more results
- **WHEN** the answer is rendered for a human
- **THEN** the rendering states that the results are convention-classified test code, not resolved semantic fact

#### Scenario: Each site carries its classification rule

- **GIVEN** a `tests` answer whose sites were classified under more than one convention rule
- **WHEN** the answer is returned as JSON
- **THEN** each site carries the convention rule that classified its enclosing declaration

#### Scenario: Resolved relations carry no heuristic marker

- **GIVEN** a `references` query against the same subject
- **WHEN** the answer is returned as JSON
- **THEN** it carries no heuristic-grade marker

#### Scenario: Unresolved subject carries no marker

- **GIVEN** a `tests` query whose subject resolves to no symbol
- **WHEN** the answer is returned as JSON
- **THEN** the typed absence carries no classification marker

#### Scenario: Ambiguous subject carries no marker

- **GIVEN** a `tests` query whose subject resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the ambiguous answer carries no classification marker

#### Scenario: Empty answer keeps the marker

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is returned as JSON
- **THEN** the empty answer carries the convention-based marker alongside its typed absence

#### Scenario: Empty human render states scoped absence

- **GIVEN** a `tests` query whose subject has only production references
- **WHEN** the answer is rendered for a human
- **THEN** the rendering presents the empty result as no convention-classified test reference found, not as proof that nothing tests the subject

#### Scenario: Heuristic marker composes with staleness

- **GIVEN** a `tests` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the heuristic-grade marker, each independently

### Requirement: Dependents order selector

> Previously: the rejection was stated through the internal record a continuation token is tied to; the behavior is unchanged.

The system SHALL support, on `dependents` traces and `impact` assessments, a per-query order selector offering exactly two orderings: `ranked` (the default) — distance first, then structural importance within each layer — and `unranked` — derived only from the answer's own stable structural keys: distance, then dependency kind, then canonical identity.
The selector SHALL be accepted only where its orderings are defined — a `dependents` trace or an `impact` assessment — and an order requested anywhere else SHALL be rejected as a usage error before any traversal.
The selector SHALL NOT alter which symbols the answer contains.
A continuation token SHALL be rejected, rather than resumed, when it is presented with a different ordering or a different ranking-model version than the one it was issued under.

Serves: unranked-order, ranked-blast-radius

#### Scenario: No selector yields the ranked ordering

- **GIVEN** an indexed symbol with dependents of differing codebase-wide importance at the same distance
- **WHEN** `trace` is invoked over the `dependents` relation with no order requested
- **THEN** the rows within a distance layer are ordered most important first

#### Scenario: Unranked ordering on request

- **GIVEN** an indexed symbol with several dependents
- **WHEN** `trace` is invoked over the `dependents` relation with the `unranked` ordering requested
- **THEN** the rows are ordered by distance, then dependency kind, then canonical identity

#### Scenario: Both orderings return the same answer set

- **GIVEN** an indexed symbol with several dependents
- **WHEN** the same `dependents` trace is invoked once under each ordering
- **THEN** both answers contain exactly the same symbols with the same distances and kinds

#### Scenario: A continuation token binds its ordering

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented with the `unranked` ordering requested
- **THEN** it is rejected as a usage error rather than resumed

#### Scenario: A continuation token does not outlive its ranking model

- **GIVEN** a truncated `dependents` answer under the `ranked` ordering and its continuation token
- **WHEN** the token is presented after the ranking model's version has changed
- **THEN** it is rejected as a usage error rather than resumed against a differently-ordered sequence

#### Scenario: An order requested outside its relations is refused

- **GIVEN** an indexed symbol referenced in several places
- **WHEN** `trace` is invoked over the `references` relation with an order requested
- **THEN** the invocation is rejected as a usage error before any traversal, naming where the selector applies

### Requirement: Search by meaning

> Previously: the ranking was described through internal signals and candidate pools; the behavior is unchanged.

The system SHALL provide a CLI command `search` that, given a natural-language query, returns corpus-contributing symbols ordered by estimated relevance to the query, most relevant first, where a symbol's name, its documentation, and its source content all serve as relevance evidence — including name words the source spells within one compound token — and sharing exact tokens with the query is not a precondition for a symbol to be returned.
A symbol whose passage is represented by several chunks SHALL be ranked by its best-matching chunk, so that content anywhere in a long symbol can surface it.
Among the candidates each ranking method considers, no symbol SHALL rank higher because more chunks represent it.
An answer MAY be drawn from a bounded set of candidates; every answer already states that its rows are candidates and not a complete list, and the bound never changes the order of the candidates the answer returns.
Each result row SHALL identify its symbol and location and carry content at a per-query detail level — location, signature, interface, or body — with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
Each corpus-contributing symbol SHALL appear at most once in a search answer, and a search against a store whose corpus is empty SHALL be a typed-empty answer, not a failure.

Serves: find-by-meaning, discriminating-long-symbol, honest-vector

#### Scenario: Documentation words match without the name

- **GIVEN** an indexed function whose documentation describes its behavior
- **WHEN** `search` is invoked with words drawn from that documentation, none of which appear in the function's name
- **THEN** the function is among the returned rows

#### Scenario: Name words match as natural language

- **GIVEN** an indexed function whose compound name joins several words into one source token
- **WHEN** `search` is invoked with those words as separate natural-language words
- **THEN** the function is among the returned rows

#### Scenario: Content late in a long symbol is findable

- **GIVEN** an indexed symbol whose passage exceeds the chunk size, with distinctive content near its end
- **WHEN** `search` is invoked with words drawn from that content
- **THEN** the symbol is among the returned rows

#### Scenario: Rows default to signature detail

- **GIVEN** an indexed workspace
- **WHEN** `search` is invoked with no detail requested
- **THEN** each returned row carries the signature tier of its symbol

#### Scenario: Detail does not change the result set

- **GIVEN** an indexed workspace and a query with several relevant symbols
- **WHEN** the same search is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

#### Scenario: A symbol appears at most once

- **GIVEN** an indexed function relevant to a query through both its name and its documentation
- **WHEN** `search` is invoked with that query
- **THEN** the function appears exactly once in the answer

#### Scenario: A multi-chunk symbol appears at most once

- **GIVEN** an indexed symbol whose passage is represented by several chunks, more than one of them relevant to a query
- **WHEN** `search` is invoked with that query
- **THEN** the symbol appears exactly once in the answer

#### Scenario: An empty corpus is typed absence

- **GIVEN** a built store whose corpus holds no passages
- **WHEN** `search` is invoked
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

### Requirement: Similar-code lookup

> Previously: the ranking was described through internal signals and candidate pools, and a scenario checked rows built from the subject's chunks; the behavior is unchanged.

The system SHALL provide a CLI command `similar` that, given a subject symbol supplied as a symbol reference or a source position — resolved under the symbol-reference resolution contract, a position resolving to the symbol enclosing it — returns other corpus-contributing symbols ordered by estimated similarity of their content to the subject's, most similar first.
Where either the subject's passage or a candidate's is represented by several chunks, similarity SHALL be their best-matching pair of chunks, so that two symbols sharing one region of content rank as similar.
Among the candidates each ranking method considers, neither SHALL rank higher because more chunks represent it.
An answer MAY be drawn from a bounded set of candidates; like a `search` answer, it states that its rows are candidates and not a complete list.
The subject SHALL NOT appear in its own answer.
Each result row SHALL identify its symbol and location and carry content at the same per-query detail axis as `search`, with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
An ambiguous subject reference SHALL yield the typed candidate set, and a corpus containing no symbol other than the subject SHALL yield a typed-empty answer.

Serves: assess-similarity, discriminating-long-symbol

#### Scenario: Neighbors ranked with the subject excluded

- **GIVEN** an indexed function and several other corpus-contributing symbols
- **WHEN** `similar` is invoked for the function
- **THEN** other symbols are returned ordered most similar first, and the subject function is not among them

#### Scenario: A multi-chunk subject excludes only itself

- **GIVEN** an indexed subject whose passage is represented by several chunks
- **WHEN** `similar` is invoked for that subject
- **THEN** no returned row is the subject

#### Scenario: Symbols sharing one region rank as similar

- **GIVEN** two indexed symbols whose passages exceed the chunk size and whose content coincides in one region only
- **WHEN** `similar` is invoked for one of them
- **THEN** the other is among the returned rows

#### Scenario: A position resolves the enclosing symbol as subject

- **GIVEN** a source position inside a method body
- **WHEN** `similar` is invoked for that position
- **THEN** the answer's subject is the method enclosing the position

#### Scenario: An ambiguous reference yields candidates

- **GIVEN** a short name shared by several indexed symbols
- **WHEN** `similar` is invoked with that name
- **THEN** a typed candidate set of the matching symbols is returned rather than an arbitrary one being chosen

#### Scenario: No other corpus symbol is typed absence

- **GIVEN** a built store whose corpus holds only the subject symbol
- **WHEN** `similar` is invoked for that symbol
- **THEN** an empty set is returned as a definite "none", distinct from an unavailable or failed answer

#### Scenario: Similar rows default to signature detail

- **GIVEN** an indexed workspace and a resolved subject
- **WHEN** `similar` is invoked with no detail requested
- **THEN** each returned row carries the signature tier of its symbol

#### Scenario: Similar detail does not change the result set

- **GIVEN** a subject with several corpus neighbors
- **WHEN** the same `similar` is invoked at two different detail levels
- **THEN** both answers return the same symbols in the same order

### Requirement: Clone answers are labeled as token equivalence

> Previously: the rule stated the order of internal steps for an ambiguous or unresolved answer; the behavior is unchanged.

Every `clones` answer, a typed-empty answer included, SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying its rows as deterministic token equivalence, never behavioral equivalence, and SHALL carry the guaranteed fragment length in effect.
A `clones` answer SHALL NOT carry the estimation marker or the semantic-index identity, and SHALL carry no similarity score.
The one exception is an ambiguous-reference or unresolved-subject answer, which SHALL NOT carry the token-equivalence marker.
The marker SHALL accompany, never replace, the provenance and freshness labeling every answer carries.
The machine answer and the human rendering SHALL both name every symbol not compared completely.

Serves: duplication-audit-in-one-run, shared-fragments

#### Scenario: The machine answer carries the marker and the length

- **GIVEN** a `clones` query with one or more rows
- **WHEN** the answer is returned as JSON
- **THEN** it carries the token-equivalence marker and the guaranteed fragment length, and no estimation marker

#### Scenario: The human render states token equivalence

- **GIVEN** a `clones` query with one or more rows
- **WHEN** the answer is rendered for a human
- **THEN** the rendering states that the rows are identical token structure, not proof of identical behavior

#### Scenario: An empty answer keeps the marker

- **GIVEN** a `clones` query against a corpus with no shared key
- **WHEN** the answer is returned as JSON
- **THEN** the typed-empty answer carries the token-equivalence marker

#### Scenario: An ambiguous subject carries no marker

- **GIVEN** a `clones` subject reference that resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the candidate-set answer carries no token-equivalence marker

#### Scenario: The marker composes with staleness

- **GIVEN** a `clones` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the token-equivalence marker

### Requirement: Semantic answers are structurally labeled

> Previously: the rule stated the order of internal steps for an ambiguous or unresolved answer; the behavior is unchanged.

Every `search` and `similar` answer — a typed-empty answer included — SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying the ranking as model-derived estimation rather than resolved semantic fact, and SHALL carry the store's recorded semantic-index identity as provenance.
The machine answer SHALL carry the recorded identity whole — it is the consumer's attribution of a ranking to the regime that produced it; the human rendering MAY present the identity in condensed form.
An ambiguous-reference or unresolved-subject answer contains no estimation-derived content, so it SHALL NOT carry the marker or the semantic-index provenance — the marker asserts a derivation, never merely the command invoked.
The human rendering and the commands' self-descriptions SHALL present the results as the nearest candidates the index holds — never as the complete set of relevant code, and an empty or truncated answer never as evidence that no relevant code exists.
A clone-certainty marker derives from deterministic equivalence and SHALL NOT be presented as part of the estimated ranking's heuristic grade.
The marker SHALL accompany, never replace, the provenance, freshness, and staleness labeling every answer carries.

Serves: find-by-meaning, assess-similarity, honest-vector

#### Scenario: Machine answer carries the marker and provenance

- **GIVEN** a `search` query with one or more results
- **WHEN** the answer is returned as JSON
- **THEN** it carries a structural field marking the ranking as model-derived estimation and the semantic-index identity as provenance

#### Scenario: Human render frames results as candidates

- **GIVEN** a `search` query with one or more results
- **WHEN** the answer is rendered for a human
- **THEN** the rendering presents the rows as the nearest candidates by estimated relevance, not as the complete set of matches

#### Scenario: An empty answer keeps the marker and scoped absence

- **GIVEN** a `search` against a built store whose corpus holds no passages
- **WHEN** the answer is returned
- **THEN** the typed-empty answer carries the marker, and the human rendering does not present the absence as proof that no relevant code exists

#### Scenario: The marker composes with staleness

- **GIVEN** a `similar` query against an index whose sources changed since indexing
- **WHEN** the answer is returned
- **THEN** it carries both the staleness flag and the estimation marker, each independently

#### Scenario: Clone markers are presented as fact

- **GIVEN** a `similar` answer containing a row with the exact-clone marker
- **WHEN** the answer is rendered for a human
- **THEN** the clone marker is presented as deterministic equivalence, distinct from the estimated ranking

#### Scenario: An ambiguous subject carries no marker

- **GIVEN** a `similar` subject reference that resolves ambiguously to several symbols
- **WHEN** the answer is returned as JSON
- **THEN** the candidate-set answer carries neither the estimation marker nor the semantic-index provenance

#### Scenario: An unresolved subject carries no marker

- **GIVEN** a `similar` subject reference that resolves to no symbol
- **WHEN** the answer is returned as JSON
- **THEN** the typed absence carries neither the estimation marker nor the semantic-index provenance
