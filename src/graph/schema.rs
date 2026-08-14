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
/// Migration: version 14 added the semantic corpus (`semantic_corpus`, `semantic_lexical`,
/// `semantic_vectors`), the clone-key columns on `symbols`, and the semantic-index identity in
/// `index_metadata`. The index is derived, replayable
/// data, so rebuild is the migration — a build replaces an older store of its own wholesale and a
/// query refuses it with recovery guidance (the replace-or-refuse contract).
pub const SCHEMA_VERSION: i64 = 14;

/// The DDL that creates the full schema. Idempotent via `IF NOT EXISTS`.
pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

-- One row per indexed workspace build. Holds provenance, the content-hash gate, and the
-- join-alignment accounting: one acceptance count per alignment rule, plus the refusal counts.
-- `environment` is the backend's declared interpreter-environment facts as JSON (nullable — absent
-- for backends, like Rust's, that declare none); staleness compares it against the environment in
-- effect.
-- `workspace_root` is the canonicalized filesystem root the build indexed — the durable link from a
-- store to the workspace it describes, which a query compares against the root it is invoked with.
-- It is nullable: a root that is not valid UTF-8 cannot be recorded exactly, and a lossy rendering
-- would let a different workspace compare equal, so it is recorded as absent instead.
-- `workspace_id` stays a display name and is never compared.
-- `semantic_model_identity` and `corpus_definition_version` are the semantic-index identity in
-- effect at build time — the embedding model (compiled into the binary) and the corpus/render
-- definition that produced the build's semantic representations — retrievable as provenance and
-- carried on every semantic answer.
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
    content_hash                   TEXT    NOT NULL,
    aligned_exact_count            INTEGER NOT NULL DEFAULT 0,
    aligned_crate_root_count       INTEGER NOT NULL DEFAULT 0,
    aligned_operator_desugar_count INTEGER NOT NULL DEFAULT 0,
    aligned_module_span_count      INTEGER NOT NULL DEFAULT 0,
    aligned_self_keyword_count     INTEGER NOT NULL DEFAULT 0,
    aligned_module_name_count      INTEGER NOT NULL DEFAULT 0,
    aligned_self_name_count        INTEGER NOT NULL DEFAULT 0,
    aligned_module_marker_count    INTEGER NOT NULL DEFAULT 0,
    aligned_import_alias_count     INTEGER NOT NULL DEFAULT 0,
    aligned_range_literal_count    INTEGER NOT NULL DEFAULT 0,
    aligned_use_list_self_count    INTEGER NOT NULL DEFAULT 0,
    aligned_super_keyword_count    INTEGER NOT NULL DEFAULT 0,
    text_mismatch_count            INTEGER NOT NULL DEFAULT 0,
    semantic_only_count            INTEGER NOT NULL DEFAULT 0,
    duplicate_ambiguous_count      INTEGER NOT NULL DEFAULT 0,
    syntax_only_count              INTEGER NOT NULL DEFAULT 0
);

-- One row per persisted symbol. `class` is 'in_workspace' or 'external'; externals carry identity
-- and reference occurrences but no definition span (nullable span columns).
-- `duplicated` marks a true same-descriptor twin: an in-workspace definition whose identical
-- resolved descriptor is shared by at least one other definition. Distinct descriptors whose
-- canonical projections merely collide (and so carry a `#<rank>` disambiguator) are NOT duplicated.
-- `signature_text` and `interface_text` are the signature and interface tiers: the declaration form
-- without its body, and the signature together with the symbol's own documentation. Both are NULL
-- for externals and for in-workspace symbols carrying no definition span.
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
    span_text        TEXT,
    signature_text   TEXT,
    interface_text   TEXT,
    duplicated       INTEGER NOT NULL DEFAULT 0,
    test_rule        TEXT,
    clone_formatting_key    TEXT,
    clone_substitution_key  TEXT
);

-- One row per occurrence of a symbol. `role` is 'definition' or 'reference'. `rule` is the
-- alignment rule that accepted the attribution ('exact', 'crate_root', 'operator_desugar',
-- 'module_span', 'self_keyword', 'module_name', 'self_name', 'module_marker', 'import_alias') —
-- its provenance. Aligned reference occurrences carry
-- `enclosing_id`, the nearest enclosing persisted declaration (NULL means the module/file itself is
-- the attribution). `locality` is the locality rule ('defining_document', 'module_chain',
-- 'target_metadata', or 'declaration_scope') that selected this attribution's twin, for an occurrence
-- resolved from a duplicated descriptor's group; NULL for an ordinary (non-duplicated) attribution.
CREATE TABLE IF NOT EXISTS occurrences (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id      TEXT    NOT NULL REFERENCES symbols(canonical_id),
    document_path  TEXT    NOT NULL,
    span_start     INTEGER NOT NULL,
    span_end       INTEGER NOT NULL,
    role           TEXT    NOT NULL,
    rule           TEXT    NOT NULL,
    enclosing_id   TEXT    REFERENCES symbols(canonical_id),
    locality       TEXT
);
CREATE INDEX IF NOT EXISTS occurrences_by_symbol ON occurrences(symbol_id);
CREATE INDEX IF NOT EXISTS occurrences_by_enclosing ON occurrences(enclosing_id);

-- Type-tagged edges between symbols. `kind` is one of 'contains' (enclosure), 'uses' (a declaration
-- references a symbol in its body — reference-grade: any mention counts, not only a call),
-- 'imports' (a module references a symbol at module scope), or 'type_hierarchy' (a type implements a
-- trait). An edge is a relation instance, unique on (kind, src_id, dst_id): repeated occurrences of
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
-- to a bounded length. The span columns are NULL when the occurrence's coordinates could not be
-- normalized onto the source — typed absence, never a fabricated location. Wholly rewritten in the
-- same transaction as each build (every prior row is deleted first), so the table reflects only the
-- most recent build.
CREATE TABLE IF NOT EXISTS join_discrepancies (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    document_path  TEXT    NOT NULL,
    span_start     INTEGER,
    span_end       INTEGER,
    outcome        TEXT    NOT NULL,
    expected_name  TEXT    NOT NULL,
    found_text     TEXT
);
CREATE INDEX IF NOT EXISTS join_discrepancies_by_group ON join_discrepancies(outcome, expected_name);

-- The semantic corpus: one row per corpus-contributing symbol, holding the deterministic render its
-- representations derive from. A leaf's render carries its own source content; a container's its
-- interface tier. Wholly rewritten in the same transaction as each build. The rowid links each
-- entry to its vector row (`semantic_vectors`) and its lexical row (`semantic_lexical`).
CREATE TABLE IF NOT EXISTS semantic_corpus (
    id        INTEGER PRIMARY KEY,
    symbol_id TEXT NOT NULL UNIQUE REFERENCES symbols(canonical_id),
    render    TEXT NOT NULL
);

-- The lexical representation: FTS5/BM25 over the identifier-split words of each entry's render,
-- rowid-linked to `semantic_corpus`. The words are pre-split (camel-case and underscore compounds
-- become plain words), so the default unicode61 tokenizer never mangles identifiers.
CREATE VIRTUAL TABLE IF NOT EXISTS semantic_lexical USING fts5(words);

-- The vector representation: sqlite-vec KNN over the corpus embeddings, rowid-linked to
-- `semantic_corpus`. The embeddings are L2-normalized by the model, so the default L2 distance
-- orders identically to cosine similarity.
CREATE VIRTUAL TABLE IF NOT EXISTS semantic_vectors USING vec0(embedding float[256]);
"#;
