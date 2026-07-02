//! The SQLite-core schema.
//!
//! One embedded store holds the symbols, their occurrences, type-tagged edges, and per-index
//! metadata (analyzer provenance, source content hash, join-alignment accounting). The shape
//! reserves room for the deferred semantic pillar — a vector column on `symbols` and an FTS5 index
//! over spans — without committing to their form; those arrive as an additive migration.

/// The current schema version. Bumped on any schema-affecting change under the reproducibility
/// policy.
pub const SCHEMA_VERSION: i64 = 1;

/// The DDL that creates the full schema. Idempotent via `IF NOT EXISTS`.
pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

-- One row per indexed workspace build. Holds provenance, the content-hash gate, and the
-- join-alignment accounting for the build.
CREATE TABLE IF NOT EXISTS index_metadata (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version      INTEGER NOT NULL,
    workspace_id        TEXT    NOT NULL,
    analyzer_name       TEXT    NOT NULL,
    analyzer_version    TEXT    NOT NULL,
    content_hash        TEXT    NOT NULL,
    aligned_count       INTEGER NOT NULL DEFAULT 0,
    text_mismatch_count INTEGER NOT NULL DEFAULT 0,
    semantic_only_count INTEGER NOT NULL DEFAULT 0,
    syntax_only_count   INTEGER NOT NULL DEFAULT 0
);

-- One row per persisted symbol. `class` is 'in_workspace' or 'external'; externals carry identity
-- and reference occurrences but no definition span (nullable span columns).
-- The reserved `embedding` column holds the deferred semantic-pillar vector; its shape is not yet
-- committed, so it is a nullable BLOB placeholder that a future additive migration reshapes.
CREATE TABLE IF NOT EXISTS symbols (
    canonical_id   TEXT    PRIMARY KEY,
    display_name   TEXT    NOT NULL,
    kind           TEXT    NOT NULL,
    class          TEXT    NOT NULL,
    document_path  TEXT,
    span_start     INTEGER,
    span_end       INTEGER,
    span_text      TEXT,
    embedding      BLOB
);

-- One row per occurrence of a symbol. `role` is 'definition' or 'reference'. Aligned reference
-- occurrences carry `enclosing_id`, the nearest enclosing persisted declaration (NULL means the
-- module/file itself is the attribution).
CREATE TABLE IF NOT EXISTS occurrences (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol_id      TEXT    NOT NULL REFERENCES symbols(canonical_id),
    document_path  TEXT    NOT NULL,
    span_start     INTEGER NOT NULL,
    span_end       INTEGER NOT NULL,
    role           TEXT    NOT NULL,
    enclosing_id   TEXT    REFERENCES symbols(canonical_id)
);
CREATE INDEX IF NOT EXISTS occurrences_by_symbol ON occurrences(symbol_id);
CREATE INDEX IF NOT EXISTS occurrences_by_enclosing ON occurrences(enclosing_id);

-- Type-tagged edges between symbols. `kind` is one of 'contains', 'calls', 'imports',
-- 'type_hierarchy'. `verified` is 0 for edges populated but not yet contracted for reading
-- (calls / imports / type_hierarchy remain unverified until proposal 2).
CREATE TABLE IF NOT EXISTS edges (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    kind      TEXT NOT NULL,
    src_id    TEXT NOT NULL REFERENCES symbols(canonical_id),
    dst_id    TEXT NOT NULL REFERENCES symbols(canonical_id),
    verified  INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS edges_by_kind ON edges(kind);
CREATE INDEX IF NOT EXISTS edges_by_src ON edges(kind, src_id);
CREATE INDEX IF NOT EXISTS edges_by_dst ON edges(kind, dst_id);
"#;
