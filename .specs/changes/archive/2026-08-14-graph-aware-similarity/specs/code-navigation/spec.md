# Delta for Code Navigation

## MODIFIED Requirements

### Requirement: Similar-code lookup

The system SHALL provide a CLI command `similar` that, given a subject symbol supplied as a symbol reference or a source position — resolved under the symbol-reference resolution contract, a position resolving to the symbol enclosing it — returns other corpus-contributing symbols ordered by estimated similarity to the subject, most similar first, where both a candidate's content and its dependency-graph relationships relative to the subject's serve as similarity evidence.
A candidate carrying no dependency relationships SHALL remain rankable on its content evidence alone, and a symbol SHALL NOT rank as similar on the breadth of its references alone — ubiquity is not similarity.
The subject SHALL NOT appear in its own answer.
Each result row SHALL identify its symbol and location and carry tier content at the same per-query detail axis as `search`, with signature as the default, and the chosen detail SHALL NOT alter which symbols are returned or their order.
An ambiguous subject reference SHALL yield the typed candidate set, and a corpus containing no symbol other than the subject SHALL yield a typed-empty answer.

> Previously: candidates were ordered by estimated similarity of their content alone; dependency-graph relationships played no part in the ranking.

Serves: rank-by-position

#### Scenario: Neighbors ranked with the subject excluded

- **GIVEN** an indexed function and several other corpus-contributing symbols
- **WHEN** `similar` is invoked for the function
- **THEN** other symbols are returned ordered most similar first, and the subject function is not among them

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

#### Scenario: A structurally adjacent candidate outranks its content peer

- **GIVEN** a subject and two candidates whose content is equally similar to the subject's, one sharing the subject's dependency relationships (the same callers and dependencies) and one wired elsewhere
- **WHEN** `similar` is invoked for the subject
- **THEN** the candidate sharing the subject's relationships precedes the other

#### Scenario: An isolated candidate is still ranked

- **GIVEN** a subject and a content-similar candidate carrying no dependency edges at all
- **WHEN** `similar` is invoked for the subject
- **THEN** the isolated candidate appears in the answer, ranked on its content evidence

#### Scenario: A ubiquitous symbol is not lifted by its ubiquity

- **GIVEN** a subject and a widely-referenced symbol whose content is unlike the subject's
- **WHEN** `similar` is invoked for the subject
- **THEN** the widely-referenced symbol does not precede the content-similar candidates

#### Scenario: A near-copy wired elsewhere keeps its content rank

- **GIVEN** a subject and a near-copy of it that shares none of the subject's dependency relationships (a parallel implementation wired into a different part of the workspace)
- **WHEN** `similar` is invoked for the subject
- **THEN** the near-copy precedes candidates that are structurally adjacent to the subject but dissimilar in content

## ADDED Requirements

### Requirement: Similarity-ranking version binds continuation tokens

Every `similar` continuation token SHALL be bound to the version of the similarity ranking in effect, and a token presented under a different similarity-ranking version SHALL be refused as a usage error naming the recovery — never resumed against a differently-ordered sequence.

Serves: rank-by-position

#### Scenario: A token from a different ranking version is refused

- **GIVEN** a continuation token issued for a `similar` answer under one similarity-ranking version
- **WHEN** the token is presented under a different similarity-ranking version
- **THEN** it is refused as a usage error naming the recovery, and no page is returned
