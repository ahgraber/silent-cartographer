//! The SQLite-core schema.
//!
//! One embedded store holds the symbols, their occurrences, type-tagged edges, and per-index
//! metadata (analyzer provenance, source content hash, join-alignment accounting). The shape
//! reserves room for the deferred semantic pillar — a vector column on `symbols` and an FTS5 index
//! over spans — without committing to their form; those arrive as an additive migration.

/// The current schema version. Bumped on any schema-affecting change under the reproducibility
/// policy. Stamped into each store's `PRAGMA user_version` at creation and validated at open,
/// before any table access; the `index_metadata.schema_version` column carries it as provenance.
pub const SCHEMA_VERSION: i64 = 6;

/// The DDL that creates the full schema. Idempotent via `IF NOT EXISTS`.
pub const SCHEMA_SQL: &str = r#"
PRAGMA foreign_keys = ON;

-- One row per indexed workspace build. Holds provenance, the content-hash gate, and the
-- join-alignment accounting: one acceptance count per alignment rule, plus the refusal counts.
CREATE TABLE IF NOT EXISTS index_metadata (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version      INTEGER NOT NULL,
    workspace_id        TEXT    NOT NULL,
    analyzer_name       TEXT    NOT NULL,
    analyzer_version    TEXT    NOT NULL,
    content_hash                   TEXT    NOT NULL,
    aligned_exact_count            INTEGER NOT NULL DEFAULT 0,
    aligned_crate_root_count       INTEGER NOT NULL DEFAULT 0,
    aligned_operator_desugar_count INTEGER NOT NULL DEFAULT 0,
    aligned_module_span_count      INTEGER NOT NULL DEFAULT 0,
    aligned_self_keyword_count     INTEGER NOT NULL DEFAULT 0,
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
    duplicated     INTEGER NOT NULL DEFAULT 0,
    embedding      BLOB
);

-- One row per occurrence of a symbol. `role` is 'definition' or 'reference'. `rule` is the
-- alignment rule that accepted the attribution ('exact', 'crate_root', 'operator_desugar',
-- 'module_span', 'self_keyword') — its provenance. Aligned reference occurrences carry
-- `enclosing_id`, the nearest enclosing persisted declaration (NULL means the module/file itself is
-- the attribution). `locality` is the locality rule ('defining_document' or 'module_chain') that
-- selected this attribution's twin, for an occurrence resolved from a duplicated descriptor's group;
-- NULL for an ordinary (non-duplicated) attribution.
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
"#;
