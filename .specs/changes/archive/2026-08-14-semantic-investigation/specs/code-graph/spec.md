# Delta for Code Graph

## ADDED Requirements

### Requirement: Semantic corpus

The system SHALL derive, at build time, a semantic corpus over the in-workspace symbols whose tier content is persisted: a symbol that contains no other corpus-contributing symbol contributes one entry derived from its own source content together with its identity, and a symbol that contains other corpus-contributing symbols contributes one entry derived from its interface tier together with its identity.
A symbol whose every persisted content tier is exactly its own name token SHALL contribute no corpus entry: such content carries nothing beyond the symbol's identity, and name-only entries would displace content-bearing candidates in every answer.
Containment counts contributing symbols only, so a symbol enclosing nothing but name-only symbols is a leaf: no nested entry exists whose content its body could double-count.
No corpus entry SHALL derive from the full body of a symbol that contains other corpus-contributing symbols, and every corpus-contributing symbol SHALL appear in the corpus exactly once.
Corpus derivation SHALL be deterministic: identical build inputs yield identical corpus entries.

Serves: find-by-meaning, assess-similarity

#### Scenario: Leaf declaration contributes its own content

- **GIVEN** an indexed function that contains no other corpus-contributing symbol
- **WHEN** the index is built
- **THEN** the corpus holds exactly one entry for the function, derived from its own source content and identity

#### Scenario: Container contributes its interface only

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type's corpus entry derives from its interface tier, and no corpus entry carries the methods' bodies through the type

#### Scenario: Module entry excludes member bodies

- **GIVEN** an indexed module whose document opens with module documentation and contains several declarations
- **WHEN** the index is built
- **THEN** the module's corpus entry derives from its interface tier, not from the whole document

#### Scenario: Every contributor appears exactly once

- **GIVEN** an indexed workspace whose modules, types, and members are all corpus-eligible
- **WHEN** the index is built
- **THEN** the corpus holds exactly one entry per contributing symbol

#### Scenario: Corpus derivation is deterministic

- **GIVEN** the same sources built into the same store twice
- **WHEN** the corpus entries are retrieved after each build
- **THEN** the two corpora are identical

#### Scenario: A name-only symbol contributes nothing

- **GIVEN** an indexed symbol (a function parameter) whose persisted content tiers all hold only its own name token
- **WHEN** the index is built
- **THEN** the corpus holds no entry for that symbol

### Requirement: Semantic representations persisted per build

For every corpus entry, the system SHALL persist with the build a vector representation and a lexical representation derived from that entry's content, wholly superseded by each subsequent build; a build that fails SHALL leave the prior build's representations intact and authoritative; and a rebuild over unchanged sources SHALL yield representations identical to the prior build's.

Serves: find-by-meaning, assess-similarity

#### Scenario: Every corpus entry is represented

- **GIVEN** a completed build with a non-empty corpus
- **WHEN** the semantic representations are retrieved
- **THEN** each corpus entry carries both a vector representation and a lexical representation

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again
- **THEN** the persisted vector and lexical representations are identical to the first build's

#### Scenario: Representations of vanished symbols do not linger

- **GIVEN** a store built from sources containing a symbol
- **WHEN** the sources no longer contain that symbol and the store is rebuilt
- **THEN** no representation for the vanished symbol remains retrievable

#### Scenario: An edited symbol is re-represented

- **GIVEN** a store built from a set of sources
- **WHEN** a corpus-contributing symbol's source content is changed and the store is rebuilt
- **THEN** that symbol's persisted representations derive from the new content

#### Scenario: A failed build leaves prior representations authoritative

- **GIVEN** a store holding a completed build's representations
- **WHEN** a subsequent build fails before completing
- **THEN** the prior build's representations remain retrievable, unchanged

### Requirement: Clone-equivalence keys

For every corpus-contributing symbol that contains no other corpus-contributing symbol, the system SHALL persist two equivalence keys over the symbol's source: a formatting-insensitive key that two symbols share exactly when their token sequences are identical after comments and whitespace are disregarded, and a substitution-insensitive key that two symbols share exactly when their token sequences are additionally identical under a consistent one-to-one substitution of identifiers and literal values.
A symbol that contains other corpus-contributing symbols SHALL carry no equivalence key.
Keys SHALL be persisted with the build and wholly superseded by each subsequent build.

Serves: assess-similarity

#### Scenario: Formatting variants share the formatting-insensitive key

- **GIVEN** two indexed functions whose sources differ only in whitespace and comments
- **WHEN** the index is built
- **THEN** the two functions carry the same formatting-insensitive key

#### Scenario: A consistently renamed copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently renames the other's identifiers
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: A literal-substituted copy shares only the substitution-insensitive key

- **GIVEN** two indexed functions identical except that one consistently replaces the other's literal values
- **WHEN** the index is built
- **THEN** the two functions carry the same substitution-insensitive key and different formatting-insensitive keys

#### Scenario: An inconsistent renaming shares neither key

- **GIVEN** two indexed functions alike except that one merges two distinct identifiers of the other into a single name
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: Unrelated code shares neither key

- **GIVEN** two indexed functions with different token sequences under every normalization
- **WHEN** the index is built
- **THEN** the two functions share neither equivalence key

#### Scenario: A container carries no equivalence key

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type carries no equivalence key

#### Scenario: Keys are superseded by each build

- **GIVEN** an indexed leaf whose source then changes
- **WHEN** the store is rebuilt
- **THEN** the leaf's persisted keys are those of the new source and the prior build's keys are gone

### Requirement: Semantic-index identity recorded

The system SHALL record, with every build it persists, the semantic-index identity in effect — the embedding model identity and the corpus definition version — and SHALL make it retrievable alongside the store's provenance.

Serves: find-by-meaning, assess-similarity

#### Scenario: Identity recorded and retrievable

- **GIVEN** a completed build
- **WHEN** the store's provenance is retrieved
- **THEN** it carries the embedding model identity and the corpus definition version that produced the build's semantic representations
