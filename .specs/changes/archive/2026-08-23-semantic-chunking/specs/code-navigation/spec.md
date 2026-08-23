# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Search by meaning

The system SHALL provide a CLI command `search` that, given a natural-language query, returns corpus-contributing symbols ordered by estimated relevance to the query, most relevant first, where a symbol's name, its documentation, and its source content all serve as relevance evidence — including name words the source spells within one compound token — and sharing exact tokens with the query is not a precondition for a symbol to be returned.
A symbol whose passage is represented by several chunks SHALL be ranked by its best-matching one, so that content anywhere in a long symbol can surface it and, among the candidates a signal considers, no symbol is advantaged by the number of chunks representing it.
A signal MAY draw its candidates from a bounded pool; a bounded pool is part of the candidates-not-completeness framing every answer carries, and it never changes how the candidates within it are ranked.
Each result row SHALL identify its symbol and location and carry content at a per-query detail level — location, signature, interface, or body — with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
Each corpus-contributing symbol SHALL appear at most once in a search answer, and a search against a store whose corpus is empty SHALL be a typed-empty answer, not a failure.

> Previously: relevance was scored against the single vector representing each symbol, so no best-of rule was needed and the at-most-once guarantee could not be reached through multiple representations.

Serves: discriminating-long-symbol, honest-vector

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

The system SHALL provide a CLI command `similar` that, given a subject symbol supplied as a symbol reference or a source position — resolved under the symbol-reference resolution contract, a position resolving to the symbol enclosing it — returns other corpus-contributing symbols ordered by estimated similarity of their content to the subject's, most similar first.
Where either the subject's passage or a candidate's is represented by several chunks, similarity SHALL be their best-matching pair, so that two symbols sharing one region of content rank as similar and, among the candidates a signal considers, neither is advantaged by the number of chunks representing it.
A signal MAY draw its candidates from a bounded pool, under the same candidates-not-completeness framing `search` carries.
The subject SHALL NOT appear in its own answer.
Each result row SHALL identify its symbol and location and carry content at the same per-query detail axis as `search`, with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
An ambiguous subject reference SHALL yield the typed candidate set, and a corpus containing no symbol other than the subject SHALL yield a typed-empty answer.

> Previously: similarity compared the single vector of the subject against the single vector of each candidate.

Serves: discriminating-long-symbol

#### Scenario: Neighbors ranked with the subject excluded

- **GIVEN** an indexed function and several other corpus-contributing symbols
- **WHEN** `similar` is invoked for the function
- **THEN** other symbols are returned ordered most similar first, and the subject function is not among them

#### Scenario: A multi-chunk subject excludes only itself

- **GIVEN** an indexed subject whose passage is represented by several chunks
- **WHEN** `similar` is invoked for that subject
- **THEN** no returned row is the subject, and the subject's own chunks contribute no row of their own

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

### Requirement: Semantic answers are structurally labeled

Every `search` and `similar` answer — a typed-empty answer included — SHALL carry, in the machine answer and the human rendering alike, a structural marker identifying the ranking as model-derived estimation rather than resolved semantic fact, and SHALL carry the store's recorded semantic-index identity as provenance.
The machine answer SHALL carry the recorded identity whole — it is the consumer's attribution of a ranking to the regime that produced it; the human rendering MAY present the identity in condensed form.
An ambiguous-reference or unresolved-subject answer terminates before any ranking is derived and contains no estimation-derived content, so it SHALL NOT carry the marker or the semantic-index provenance — the marker asserts a derivation, never merely the command invoked.
The human rendering and the commands' self-descriptions SHALL present the results as the nearest candidates the index holds — never as the complete set of relevant code, and an empty or truncated answer never as evidence that no relevant code exists.
A clone-certainty marker derives from deterministic equivalence and SHALL NOT be presented as part of the estimated ranking's heuristic grade.
The marker SHALL accompany, never replace, the provenance, freshness, and staleness labeling every answer carries.

> Previously: the recorded identity had no operator-variable part, so the machine answer's two identity fields were the whole identity and no surface split was needed; the empty-answer scenario named the corpus's units as entries.

Serves: honest-vector

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
