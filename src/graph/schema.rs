//! The SQLite-core schema.
//!
//! One embedded store holds the symbols, their occurrences, type-tagged edges, the semantic-corpus
//! representations (render text, embedding vectors, lexical index, clone-equivalence keys), and
//! per-index metadata (analyzer provenance, source content hash, join-alignment accounting,
//! semantic-index identity).

/// The current schema version. Bumped on any schema-affecting change under the reproducibility
/// policy. Stamped into each store's `PRAGMA user_version` at creation and validated at open,
/// before any table access; the `index_metadata.schema_version` column carries it as provenance.
///
/// Version is not ownership: whose file this is lives in `PRAGMA application_id`
/// ([`crate::graph::store::APPLICATION_ID`]), which no migration disturbs.
///
/// The semantic-index identity (the embedding model, the corpus definition) folds into this
/// version: the model is compiled into the binary, so an identity change can only arrive in a new
/// binary, and bumping this version makes every stale store refuse wholesale — no separate
/// semantic-compatibility gate exists.
///
/// A change to the alignment-rule vocabulary bumps this version like any other schema-affecting
/// change. Which rules the join applies decides which occurrences align, so a new rule yields a
/// different graph from the same sources; a store built under the old vocabulary is stale, not
/// merely missing a count. The keyed `aligned_counts` column spares that change any DDL and any
/// per-rule read/write edit — it does not spare it the version bump.
///
/// Migration: version 21 adds the `overrides` edge kind (a method to each method it overrides);
/// a store built earlier has none, so its dependents answers would omit overrides;
/// version 20 adds `unresolved_calls` (calls whose called name no semantic occurrence
/// covers), `discrepancy_symbols` (the symbols a refused reference occurrence may denote), and
/// `symbols.duplicate_descriptor` (the kind-aware descriptor duplicated twins group on);
/// version 19 changes the stored identity form: a Rust declaration outside the package's
/// library carries a unit marker after the package (`package+bin.tool`, `package+shared`), a
/// Python module path is one segment per dotted component, and a Python package initializer has no
/// `__init__` segment (module names read from the segments joined with `.`); the build-target
/// inventory and the digest of each manifest it was read from are recorded in
/// `index_metadata.targets`;
/// version 18 leaves documentation out of the spelled token sequence the clone keys read
/// (a documented Python leaf's stored keys change) and adds the shared-fragment evidence —
/// `clone_fingerprints`, `symbols.clone_token_count`, an index on `symbols.clone_substitution_key`,
/// and the fragment parameters in `index_metadata`;
/// version 17 stores a callable whose parent is not a type as `function` rather than
/// `method`, and names a Python module by its module name rather than `__init__` (both stored
/// columns, and both inputs to the passage render, so the corpus definition version moved with it)
/// and records each location's line and column beside its byte offsets (`symbols`, `occurrences`,
/// `join_discrepancies`), each analyzed file's content hash (`source_files`), and where each symbol's
/// header ends (`symbols`), and computes the aggregate content hash with each path and text
/// length-prefixed;
/// version 16 replaced the twelve `aligned_*_count` columns with a single keyed
/// `aligned_counts` JSON object, retiring the per-rule column, read, and write edits a new rule
/// used to require;
/// version 15 reshaped `semantic_vectors` to one row per chunk (carrying its passage's
/// identity and ordinal) and added the chunk parameters to `index_metadata`; version 14 added the
/// semantic corpus (`semantic_corpus`, `semantic_lexical`, `semantic_vectors`), the clone-key
/// columns on `symbols`, and the semantic-index identity in `index_metadata`. The index is derived,
/// replayable data, so rebuild is the migration — a build replaces an older store of its own
/// wholesale and a query refuses it with recovery guidance (the replace-or-refuse contract).
pub const SCHEMA_VERSION: i64 = 21;

/// The DDL that creates the full schema. Idempotent via `IF NOT EXISTS`.
pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

-- One row per indexed workspace build. Holds provenance, the content-hash gate, and the
-- join-alignment accounting: `aligned_counts` is a JSON object mapping each alignment rule's stored
-- tag to its acceptance count, alongside the refusal counts. The rule vocabulary is keyed rather
-- than columnar so a rule joining it costs no DDL and no per-rule read/write edit; it still bumps
-- the schema version, because a new rule changes which occurrences align and so changes the graph.
-- Every rule the writing binary knows is emitted, zero-valued ones included, and the read requires
-- exactly that key set: a missing or unknown key means the store was built under a different
-- vocabulary, which is a different graph, not a differing count.
-- `environment` is the backend's declared interpreter-environment facts as JSON (nullable — absent
-- for backends, like Rust's, that declare none); staleness compares it against the environment in
-- effect.
-- `workspace_root` is the canonicalized filesystem root the build indexed — the durable link from a
-- store to the workspace it describes, which a query compares against the root it is invoked with.
-- It is nullable: a root that is not valid UTF-8 cannot be recorded exactly, and a lossy rendering
-- would let a different workspace compare equal, so it is recorded as absent instead.
-- `workspace_id` stays a display name and is never compared.
-- `semantic_model_identity`, `corpus_definition_version`, `chunk_size`, and `chunk_overlap` are
-- the semantic-index identity in effect at build time — the embedding model (compiled into the
-- binary), the corpus/render definition, and the chunk parameters that produced the build's
-- semantic representations — retrievable as provenance. The chunk parameters are the operator's
-- values, so a build compares them when deciding whether stored vectors still describe the
-- workspace. `fragment_kgram_length` and `fragment_window` are the winnowing parameters the build's
-- clone fingerprints were derived under; a reader applies the recorded values, not the binary's.
-- `targets` is JSON: the build-target inventory the build's identities were assigned under and the
-- digest of each manifest it was read from, which a build compares to decide whether the stored
-- identities still describe the project.
CREATE TABLE IF NOT EXISTS index_metadata (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version      INTEGER NOT NULL,
    workspace_id        TEXT    NOT NULL,
    workspace_root      TEXT,
    analyzer_name       TEXT    NOT NULL,
    analyzer_version    TEXT    NOT NULL,
    environment         TEXT,
    semantic_model_identity   TEXT    NOT NULL,
    corpus_definition_version INTEGER NOT NULL,
    chunk_size                INTEGER NOT NULL,
    chunk_overlap             INTEGER NOT NULL,
    fragment_kgram_length     INTEGER NOT NULL,
    fragment_window           INTEGER NOT NULL,
    targets                   TEXT    NOT NULL,
    content_hash                   TEXT    NOT NULL,
    aligned_counts                 TEXT    NOT NULL DEFAULT '{}',
    text_mismatch_count            INTEGER NOT NULL DEFAULT 0,
    semantic_only_count            INTEGER NOT NULL DEFAULT 0,
    duplicate_ambiguous_count      INTEGER NOT NULL DEFAULT 0,
    syntax_only_count              INTEGER NOT NULL DEFAULT 0
);

-- One row per analyzed source file: the SHA-256 of the file's text, written in the same transaction
-- as the index metadata from the same source list the aggregate `content_hash` covers.
CREATE TABLE IF NOT EXISTS source_files (
    document_path  TEXT PRIMARY KEY,
    content_hash   TEXT NOT NULL
);

-- One row per persisted symbol. `class` is 'in_workspace' or 'external'; externals carry identity
-- and reference occurrences but no definition span (nullable span columns).
-- `duplicated` marks a true same-descriptor twin: an in-workspace definition whose identical
-- resolved descriptor is shared by at least one other definition. Distinct descriptors whose
-- canonical projections merely collide (and so carry a `#<rank>` disambiguator) are NOT duplicated.
-- `duplicate_key` is the identity of the shared descriptor without unit marker or disambiguator,
-- identical for every twin of that descriptor (including twins belonging to different units); NULL for a
-- symbol that is not duplicated.
-- `duplicate_descriptor` is the JSON serialization of that descriptor with its segment kinds, which
-- `duplicate_key` omits; twins group on it so descriptors differing only in a kind stay separate.
-- `start_line`, `start_column`, `end_line`, and `end_column` locate the span in lines and columns
-- from 1 (lines end at LF, CRLF, or a lone CR; columns count characters; the end is exclusive); they
-- are NULL exactly when the span is. `occurrences` and `join_discrepancies` carry the same four.
-- `header_end`, `header_end_line`, and `header_end_column` locate where the symbol's header ends: the
-- byte offset where its declaration form is cut before the body, with its line and column. The header
-- starts at the span's start. A symbol with no body distinct from its form, or stored with its name as
-- its span, ends its header at the span's end. They are NULL for a document's own module and for a
-- symbol without a span.
-- `span_text`, `signature_text`, and `interface_text` are the three content tiers: the definition
-- span's source verbatim, the declaration form without its body, and the signature together with the
-- symbol's own documentation. The latter two are derived from the source; the body tier is the
-- source. All three are NULL for externals and for in-workspace symbols carrying no definition span.
-- `test_rule` is the per-symbol test classification: NULL means non-test; a rule name means the
-- symbol is test code, with the convention rule that stamped it as provenance ('test_attribute',
-- 'test_configuration', 'test_file', 'test_directory' — an open set a consumer must not treat as
-- closed). The predicate and its provenance are one column so they cannot disagree.
-- `clone_formatting_key` and `clone_substitution_key` are the clone-equivalence keys over a leaf
-- symbol's spelled token sequence: the formatting-insensitive key collides exactly when two token
-- sequences are identical after comments and whitespace are disregarded; the substitution-insensitive
-- key collides exactly when they additionally match under a consistent one-to-one substitution of
-- identifiers and literal values. Both are NULL for containers (a container "clone" over interface
-- text would assert a body equivalence the key never examined), for externals, and for symbols whose
-- source spelled no tokens.
CREATE TABLE IF NOT EXISTS symbols (
    canonical_id     TEXT    PRIMARY KEY,
    display_name     TEXT    NOT NULL,
    kind             TEXT    NOT NULL,
    class            TEXT    NOT NULL,
    document_path    TEXT,
    span_start       INTEGER,
    span_end         INTEGER,
    start_line       INTEGER,
    start_column     INTEGER,
    end_line         INTEGER,
    end_column       INTEGER,
    header_end        INTEGER,
    header_end_line   INTEGER,
    header_end_column INTEGER,
    span_text        TEXT,
    signature_text   TEXT,
    interface_text   TEXT,
    duplicated       INTEGER NOT NULL DEFAULT 0,
    duplicate_key    TEXT,
    duplicate_descriptor TEXT,
    test_rule       TEXT,
    clone_formatting_key    TEXT,
    clone_substitution_key  TEXT,
    clone_token_count       INTEGER
);
CREATE INDEX IF NOT EXISTS symbols_by_clone_substitution_key ON symbols(clone_substitution_key);

-- Shared-fragment evidence: the winnowed fingerprints of a symbol's comparable token sequence, one
-- row per selected k-gram. A leaf's comparable sequence is the spelled sequence its clone keys read;
-- a function or method containing other corpus symbols has one over its own code, each contained
-- symbol replaced by one boundary token. `passage_id` is the symbol's `semantic_corpus` rowid and
-- `token_ordinal` is the k-gram's start index in the comparable sequence. A sequence shorter than the
-- guaranteed fragment length holds none. `symbols.clone_token_count` is the sequence's length, set
-- for every symbol with comparable content.
CREATE TABLE IF NOT EXISTS clone_fingerprints (
    hash           INTEGER NOT NULL,
    passage_id     INTEGER NOT NULL,
    token_ordinal  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS clone_fingerprints_by_hash ON clone_fingerprints(hash);

-- One row per occurrence of a symbol. `role` is 'definition' or 'reference'. `rule` is the
-- alignment rule that accepted the attribution ('exact', 'crate_root', 'operator_desugar',
-- 'module_span', 'self_keyword', 'module_name', 'self_name', 'module_marker', 'import_alias') —
-- its provenance. Aligned reference occurrences carry
-- `enclosing_id`, the nearest enclosing persisted declaration (NULL means the module/file itself is
-- the attribution). `locality` is the locality rule ('defining_document', 'module_chain',
-- 'target_metadata', 'unit_membership', or 'declaration_scope') that selected this attribution's twin,
-- for an occurrence resolved from a duplicated descriptor's group; NULL for an ordinary (non-duplicated)
-- attribution.
CREATE TABLE IF NOT EXISTS occurrences (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id      TEXT    NOT NULL REFERENCES symbols(canonical_id),
    document_path  TEXT    NOT NULL,
    span_start     INTEGER NOT NULL,
    span_end       INTEGER NOT NULL,
    start_line     INTEGER,
    start_column   INTEGER,
    end_line       INTEGER,
    end_column     INTEGER,
    role           TEXT    NOT NULL,
    rule           TEXT    NOT NULL,
    enclosing_id   TEXT    REFERENCES symbols(canonical_id),
    locality       TEXT
);
CREATE INDEX IF NOT EXISTS occurrences_by_symbol ON occurrences(symbol_id);
CREATE INDEX IF NOT EXISTS occurrences_by_enclosing ON occurrences(enclosing_id);

-- Type-tagged edges between symbols. `kind` is one of 'contains' (enclosure), 'uses' (a declaration
-- references a symbol in its body — reference-grade: any mention counts, not only a call),
-- 'imports' (a module references a symbol at module scope), 'type_hierarchy' (a type implements a
-- trait), or 'overrides' (a method overrides a method of a base class or trait). An edge is a relation instance, unique on (kind, src_id, dst_id): repeated occurrences of
-- the same relation persist as one row, with the per-site detail carried by the occurrences table.
CREATE TABLE IF NOT EXISTS edges (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    kind      TEXT NOT NULL,
    src_id    TEXT NOT NULL REFERENCES symbols(canonical_id),
    dst_id    TEXT NOT NULL REFERENCES symbols(canonical_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS edges_unique ON edges(kind, src_id, dst_id);
CREATE INDEX IF NOT EXISTS edges_by_src ON edges(kind, src_id);
CREATE INDEX IF NOT EXISTS edges_by_dst ON edges(kind, dst_id);

-- One row per non-aligned semantic occurrence, the inspectable detail behind the aggregate join
-- counts. `outcome` is 'text_mismatch', 'semantic_only', or 'duplicate_ambiguous'. `expected_name`
-- is the occurrence's expected name token; `found_text` is the source text at the location, truncated
-- to a bounded length, with `found_text_shortened` set when it was cut. The span columns are NULL when
-- the occurrence's coordinates could not be normalized onto the source — typed absence, never a
-- fabricated location. Wholly rewritten in the same transaction as each build (every prior row is
-- deleted first), so the table reflects only the most recent build.
CREATE TABLE IF NOT EXISTS join_discrepancies (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    document_path  TEXT    NOT NULL,
    span_start     INTEGER,
    span_end       INTEGER,
    start_line     INTEGER,
    start_column   INTEGER,
    end_line       INTEGER,
    end_column     INTEGER,
    outcome        TEXT    NOT NULL,
    expected_name  TEXT    NOT NULL,
    found_text     TEXT,
    found_text_shortened INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS join_discrepancies_by_group ON join_discrepancies(outcome, expected_name);

-- The persisted symbols a refused reference occurrence may denote, one row per link. A
-- duplicate-ambiguous occurrence links every member of its descriptor's duplicate group; any other
-- refused reference links its own symbol, when that symbol is persisted. A refused definition links
-- nothing. Rewritten with `join_discrepancies` on each build.
CREATE TABLE IF NOT EXISTS discrepancy_symbols (
    discrepancy_id  INTEGER NOT NULL REFERENCES join_discrepancies(id),
    symbol_id       TEXT    NOT NULL REFERENCES symbols(canonical_id),
    PRIMARY KEY (discrepancy_id, symbol_id)
);
CREATE INDEX IF NOT EXISTS discrepancy_symbols_by_symbol ON discrepancy_symbols(symbol_id);

-- One row per call whose called name no semantic occurrence covers: the indexer recorded nothing at
-- the name, whatever the call resolves to. `name` is the called name; the span and its line and
-- column locate it. `receiver` is the source text of the receiver expression or path as written,
-- cut to a bounded length, with `receiver_shortened` set when it was cut; NULL for a call with no
-- receiver. `enclosing_id` is the nearest enclosing persisted declaration, else the document's own
-- module; NULL only for a document with no persisted module. Wholly rewritten with each build.
CREATE TABLE IF NOT EXISTS unresolved_calls (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    document_path       TEXT    NOT NULL,
    name                TEXT    NOT NULL,
    span_start          INTEGER NOT NULL,
    span_end            INTEGER NOT NULL,
    start_line          INTEGER,
    start_column        INTEGER,
    end_line            INTEGER,
    end_column          INTEGER,
    receiver            TEXT,
    receiver_shortened  INTEGER NOT NULL DEFAULT 0,
    enclosing_id        TEXT    REFERENCES symbols(canonical_id)
);
CREATE INDEX IF NOT EXISTS unresolved_calls_by_name ON unresolved_calls(name);

-- The semantic corpus: one row per corpus-contributing symbol — one passage — holding the
-- deterministic render its representations derive from. A leaf's render carries its own source
-- content; a container's its interface tier. Wholly rewritten in the same transaction as each
-- build. The rowid links each passage to its vector row (`semantic_vectors`) and its lexical row
-- (`semantic_lexical`).
CREATE TABLE IF NOT EXISTS semantic_corpus (
    id        INTEGER PRIMARY KEY,
    symbol_id TEXT NOT NULL UNIQUE REFERENCES symbols(canonical_id),
    render    TEXT NOT NULL
);

-- The lexical representation: FTS5/BM25 over the identifier-split words of each passage's render,
-- rowid-linked to `semantic_corpus`. The words are pre-split (camel-case and underscore compounds
-- become plain words), so the default unicode61 tokenizer never mangles identifiers.
CREATE VIRTUAL TABLE IF NOT EXISTS semantic_lexical USING fts5(words);

-- The vector representation: sqlite-vec KNN over the chunk embeddings, one row per chunk. Each row
-- carries its passage's identity (`passage_id`, the `semantic_corpus` rowid) and its ordinal within
-- that passage as auxiliary columns. The embeddings are L2-normalized by the model, so the default
-- L2 distance orders identically to cosine similarity. sqlite-vec names its own internal vector
-- blocks "chunks" — hence its shadow tables `semantic_vectors_chunks` and
-- `semantic_vectors_vector_chunks00` — a storage detail of the extension, unrelated to a chunk of
-- passage text.
CREATE VIRTUAL TABLE IF NOT EXISTS semantic_vectors USING vec0(
    embedding float[256],
    +passage_id INTEGER,
    +ordinal INTEGER
);
"#;
