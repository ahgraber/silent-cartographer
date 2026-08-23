# Delta for Code Graph

## ADDED Requirements

### Requirement: Bounded chunk scope

Every vector representation SHALL derive from a chunk within the recorded chunk size, and the chunks of one passage SHALL together cover that passage's whole content — no part of a passage's content is left unrepresented.
The chunk size SHALL bound the whole text embedded, the passage header carried on each chunk included.

Where the content offers structural boundaries — declarations and statements in code, paragraphs and sentences in prose — a chunk SHALL begin and end at one.
Only content offering no such boundary within the chunk size SHALL be divided at an arbitrary point, and a chunk SHALL never be empty.

When the recorded overlap is non-zero, each chunk after the first SHALL additionally carry the trailing content of its predecessor up to the overlap, taken as whole boundary-delimited units wherever the content offers them.

Serves: discriminating-long-symbol, nothing-unrepresented

#### Scenario: Content within the chunk size is one chunk

- **GIVEN** a passage whose content does not exceed the chunk size in effect
- **WHEN** the index is built
- **THEN** the passage carries exactly one vector representation, derived from its whole content

#### Scenario: Content exceeding the chunk size is covered by several chunks

- **GIVEN** a passage whose content exceeds the chunk size in effect
- **WHEN** the index is built
- **THEN** the passage carries more than one vector representation, and every part of its content lies within at least one chunk

#### Scenario: No chunk exceeds the chunk size

- **GIVEN** a passage whose content far exceeds the chunk size in effect
- **WHEN** the index is built
- **THEN** every chunk embedded is within the chunk size, its passage header included

#### Scenario: Chunks divide at structural boundaries

- **GIVEN** a passage whose content exceeds the chunk size and consists of several declarations
- **WHEN** the index is built
- **THEN** no chunk begins or ends part-way through a declaration

#### Scenario: Prose divides at prose boundaries

- **GIVEN** a passage whose content exceeds the chunk size and is documentation prose the syntax oracle reports as one indivisible construct
- **WHEN** the index is built
- **THEN** no chunk begins or ends part-way through a sentence

#### Scenario: Content offering no boundary is still bounded and covered

- **GIVEN** a passage whose content exceeds the chunk size and offers no structural boundary within it, such as a single literal
- **WHEN** the index is built
- **THEN** each chunk is within the chunk size and the chunks together cover the whole literal

#### Scenario: Overlap carries whole units

- **GIVEN** a passage whose content exceeds the chunk size, built with a non-zero overlap
- **WHEN** the index is built
- **THEN** each chunk after the first begins with trailing units of its predecessor, none of them partial

#### Scenario: No overlap means no shared content

- **GIVEN** a passage whose content exceeds the chunk size, built with an overlap of none
- **WHEN** the index is built
- **THEN** no content appears in more than one chunk

### Requirement: Vector independence from batch composition

A vector representation SHALL be a function of the chunk it represents alone: the vector persisted for a given chunk SHALL be identical whether that chunk is embedded by itself or alongside any other chunks, and SHALL NOT depend on the size, order, or content of the group it was embedded with.

Serves: honest-vector

#### Scenario: Grouping does not change a vector

- **GIVEN** a chunk that a build embeds alongside other chunks
- **WHEN** the same chunk is embedded by itself
- **THEN** the two vectors are identical

#### Scenario: A large neighbor does not change a small chunk's vector

- **GIVEN** a corpus holding both a very short chunk and a chunk orders of magnitude longer
- **WHEN** the index is built
- **THEN** the short chunk's vector is identical to the vector it carries in a corpus holding it alone

#### Scenario: The order chunks are embedded in does not change a vector

- **GIVEN** a group of chunks differing widely in length, embedded together in one order
- **WHEN** the same group is embedded together in a different order
- **THEN** every chunk carries the same vector under both orders

### Requirement: Chunk parameters recorded and honored

The system SHALL record, with every build it persists, the chunk parameters in effect — the chunk size and the overlap between adjacent chunks — and every persisted vector SHALL derive from a chunk taken under the parameters recorded with that same build.

Serves: honest-vector

#### Scenario: Parameters recorded with the build

- **GIVEN** a build run with explicit chunk parameters
- **WHEN** the store's recorded parameters are retrieved
- **THEN** they are the values the build ran with

#### Scenario: Changed parameters re-derive every vector

- **GIVEN** a store built at one chunk size
- **WHEN** the same sources are built into that store at a different chunk size
- **THEN** every persisted vector derives from the new chunk size, and none is retained from the prior build

## MODIFIED Requirements

### Requirement: Per-symbol tier content

The system SHALL persist, for every in-workspace symbol whose definition span is persisted, content at three fixed tiers over the symbol's own source: a signature tier — the declaration form without its body — an interface tier — the signature together with the symbol's own documentation — and a body tier — the definition span's source text.
The signature and interface tiers are derived from the source; the body tier is the source, verbatim.
A symbol carrying no documentation SHALL have an interface tier equal to its signature tier, and a declaration with no distinct body SHALL have a signature tier equal to its full declaration.
A module defined by a document rather than an in-document declaration carries no declaration form in source, so its signature tier SHALL be its qualified name.
Tier content SHALL be extracted from the same build's fresh syntax tree that anchors the guarded join, and SHALL be persisted with the build, wholly superseded by each subsequent build.

> Previously: the requirement named two tiers, signature and interface.
> The definition span's own text was persisted under this same extraction rule, and the corpus-eligibility predicate already read it as one of the symbol's tiers, but it was not named as a tier here.

This requirement advances no user story of its own.
It rides the change's vocabulary work: naming the third tier is what makes "tier" denote one thing, and the glossary cannot record the term correctly while the requirement that defines it names two of three.

#### Scenario: Documented Rust function tiers

- **GIVEN** an indexed Rust function carrying a doc comment and a body
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's declaration without its body, and the interface tier carries both the doc comment and the signature and not the body

#### Scenario: The body tier is the definition span verbatim

- **GIVEN** an indexed symbol whose definition span is persisted
- **WHEN** its body tier is retrieved
- **THEN** it is the source text of that span, unaltered

#### Scenario: Undocumented symbol falls back to signature

- **GIVEN** an indexed symbol with no documentation of its own
- **WHEN** its interface tier is retrieved
- **THEN** it equals the symbol's signature tier

#### Scenario: Python function tiers carry the docstring

- **GIVEN** an indexed Python function with a docstring
- **WHEN** its tier content is retrieved
- **THEN** the signature tier is the function's header, and the interface tier carries the header together with the docstring

#### Scenario: Rust module interface carries its module documentation

- **GIVEN** an indexed Rust module whose document opens with inner doc comments
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module documentation

#### Scenario: Python module interface carries its module docstring

- **GIVEN** an indexed Python module whose document opens with a module docstring
- **WHEN** the module's interface tier is retrieved
- **THEN** it carries the module docstring

#### Scenario: Declaration without a distinct body

- **GIVEN** an indexed declaration with no body distinct from its declaration form
- **WHEN** its signature tier is retrieved
- **THEN** it equals the full declaration

### Requirement: Semantic corpus

The system SHALL derive, at build time, a semantic corpus of passages over the in-workspace symbols whose tier content is persisted: a symbol that contains no other corpus-contributing symbol contributes one passage derived from its own source content together with its identity, and a symbol that contains other corpus-contributing symbols contributes one passage derived from its interface tier together with its identity.
A symbol whose every persisted content tier is exactly its own name token SHALL contribute no passage: such content carries nothing beyond the symbol's identity, and name-only passages would displace content-bearing candidates in every answer.
Containment counts contributing symbols only, so a symbol enclosing nothing but name-only symbols is a leaf: no nested passage exists whose content its body could double-count.
No passage SHALL derive from the full body of a symbol that contains other corpus-contributing symbols, and every corpus-contributing symbol SHALL appear in the corpus exactly once.
Corpus derivation SHALL be deterministic: identical build inputs yield identical passages.

> Previously: identical in substance; the corpus's unit was named a "corpus entry".

<!-- modified-removes: Module entry excludes member bodies -->

Serves: discriminating-long-symbol

#### Scenario: Leaf declaration contributes its own content

- **GIVEN** an indexed function that contains no other corpus-contributing symbol
- **WHEN** the index is built
- **THEN** the corpus holds exactly one passage for the function, derived from its own source content and identity

#### Scenario: Container contributes its interface only

- **GIVEN** an indexed type containing several methods
- **WHEN** the index is built
- **THEN** the type's passage derives from its interface tier, and no passage carries the methods' bodies through the type

#### Scenario: Module passage excludes member bodies

- **GIVEN** an indexed module whose document opens with module documentation and contains several declarations
- **WHEN** the index is built
- **THEN** the module's passage derives from its interface tier, not from the whole document

#### Scenario: Every contributor appears exactly once

- **GIVEN** an indexed workspace whose modules, types, and members are all corpus-eligible
- **WHEN** the index is built
- **THEN** the corpus holds exactly one passage per contributing symbol

#### Scenario: Corpus derivation is deterministic

- **GIVEN** the same sources built into the same store twice
- **WHEN** the passages are retrieved after each build
- **THEN** the two corpora are identical

#### Scenario: A name-only symbol contributes nothing

- **GIVEN** an indexed symbol (a function parameter) whose persisted content tiers all hold only its own name token
- **WHEN** the index is built
- **THEN** the corpus holds no passage for that symbol

### Requirement: Semantic representations persisted per build

For every passage, the system SHALL persist with the build one or more vector representations and one lexical representation derived from that passage's content, wholly superseded by each subsequent build; a build that fails SHALL leave the prior build's representations intact and authoritative; and a rebuild over unchanged sources under unchanged parameters SHALL yield representations identical to the prior build's.
Each of a passage's vector representations SHALL derive from one chunk — the passage's identifying header together with a bounded run of its content — so that content drawn from anywhere in the passage is represented as content of that symbol; the lexical representation SHALL derive from the passage's whole render.

> Previously: every "corpus entry" carried exactly one vector representation, derived from the entry's whole content.

<!-- modified-removes: Every corpus entry is represented -->

Serves: discriminating-long-symbol, honest-vector

#### Scenario: Every passage is represented

- **GIVEN** a completed build with a non-empty corpus
- **WHEN** the semantic representations are retrieved
- **THEN** each passage carries at least one vector representation and exactly one lexical representation

#### Scenario: Every chunk carries its passage's header

- **GIVEN** a passage whose content exceeds the chunk size in effect
- **WHEN** its vector representations are retrieved
- **THEN** each derives from a chunk carrying the passage's identifying header

#### Scenario: Rebuild over unchanged sources is idempotent

- **GIVEN** a store built from a set of sources
- **WHEN** the same sources are built into the same store again under the same parameters
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

### Requirement: Semantic-index identity recorded

The system SHALL record, with every build it persists, the semantic-index identity in effect — the embedding model identity, the corpus definition version, and the chunk parameters the build ran under — and SHALL make it retrievable alongside the store's provenance.

> Previously: the recorded identity carried the embedding model identity and the corpus definition version only.

Serves: honest-vector

#### Scenario: Identity recorded and retrievable

- **GIVEN** a completed build
- **WHEN** the store's provenance is retrieved
- **THEN** it carries the embedding model identity, the corpus definition version, and the chunk parameters that produced the build's semantic representations

#### Scenario: Two stores built under different parameters are distinguishable

- **GIVEN** two stores built from the same sources under different chunk parameters
- **WHEN** each store's provenance is retrieved
- **THEN** their recorded semantic-index identities differ

### Requirement: A build over unchanged inputs does no work

Before analyzing a workspace, the system SHALL determine whether the stored index already describes that workspace: the same workspace identity and root, and the workspace's current sources, analyzer, declared environment, and chunk parameters.
When it does, the system SHALL analyze nothing, leave the store unchanged, report the index as already current, and succeed.
When any of those inputs differs, or no compatible store exists, the system SHALL build.
The system SHALL offer an explicit way to build even when the stored index is already current.

> Previously: the inputs compared were the workspace identity and root, the sources, the analyzer, and the declared environment; the chunk parameters were not among them.

Serves: honest-vector

#### Scenario: Rebuilding an unchanged workspace analyzes nothing

- **GIVEN** a workspace whose index was just built
- **WHEN** the workspace is built again with no source, analyzer, environment, or parameter change
- **THEN** no analysis runs, the store is unchanged, and the index is reported as already current

#### Scenario: An edited source rebuilds

- **GIVEN** a workspace whose index was just built
- **WHEN** one source file is edited and the workspace is built again
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed analyzer rebuilds

- **GIVEN** a workspace whose index was built by one analyzer version
- **WHEN** the workspace is built again with a different analyzer version
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed environment rebuilds

- **GIVEN** a workspace whose index records a declared interpreter environment
- **WHEN** the workspace is built again against a different environment
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed chunk parameter rebuilds

- **GIVEN** a workspace whose index was built under one set of chunk parameters
- **WHEN** the workspace is built again, unchanged in every other input, under a different chunk size or overlap
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A store recorded for another workspace builds

- **GIVEN** a store whose recorded workspace identity or root differs from the one being built
- **WHEN** the workspace is built over that store
- **THEN** the workspace is analyzed, the handoff is disclosed, and the store records the workspace it now describes

#### Scenario: An absent index builds

- **GIVEN** a workspace with no index
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is written

#### Scenario: A store built under an unrecognized schema version builds

- **GIVEN** a workspace whose store the system created under a schema version it does not recognize
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is replaced

#### Scenario: An explicit rebuild ignores currency

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built with the explicit rebuild option
- **THEN** the workspace is analyzed and the store is rebuilt
