//! The SQLite-core store: persistence and retrieval of symbols, occurrences, edges, and index
//! metadata, plus staleness evaluation.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::identity::{CanonicalId, WorkspaceId};
use crate::semantic::model::AnalyzerProvenance;

use super::join::JoinAccounting;
use super::schema::{SCHEMA_SQL, SCHEMA_VERSION};

/// An edge kind in the graph. Only `Contains` and occurrence-derived references are contracted this
/// change; the dependency kinds are populated but unverified until proposal 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Enclosure: a declaration directly contains another.
    Contains,
    /// A call from one symbol to another (unverified until proposal 2).
    Calls,
    /// An import relationship (unverified until proposal 2).
    Imports,
    /// A type-hierarchy relationship (unverified until proposal 2).
    TypeHierarchy,
}

impl EdgeKind {
    /// The stored tag for this edge kind.
    pub fn tag(&self) -> &'static str {
        match self {
            EdgeKind::Contains => "contains",
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::TypeHierarchy => "type_hierarchy",
        }
    }

    /// Whether this edge kind is contracted for reading this change.
    pub fn is_verified(&self) -> bool {
        matches!(self, EdgeKind::Contains)
    }
}

/// The class a persisted symbol belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedClass {
    /// Defined in the workspace: carries a definition span.
    InWorkspace,
    /// Resolved but defined outside the workspace: no definition span.
    External,
}

impl PersistedClass {
    fn tag(&self) -> &'static str {
        match self {
            PersistedClass::InWorkspace => "in_workspace",
            PersistedClass::External => "external",
        }
    }

    fn from_tag(tag: &str) -> Self {
        match tag {
            "external" => PersistedClass::External,
            _ => PersistedClass::InWorkspace,
        }
    }
}

/// A symbol row as persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRow {
    /// The canonical identity.
    pub canonical_id: CanonicalId,
    /// A human-readable display name (the descriptor's terminal segment).
    pub display_name: String,
    /// The symbol kind tag.
    pub kind: String,
    /// The symbol class.
    pub class: PersistedClass,
    /// The document the definition sits in, if any.
    pub document_path: Option<String>,
    /// The definition span, if any: `(start, end)` byte offsets.
    pub span: Option<(usize, usize)>,
    /// The exact source text of the definition span, if any.
    pub span_text: Option<String>,
}

/// A persisted occurrence row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceRow {
    /// The symbol the occurrence belongs to.
    pub symbol_id: CanonicalId,
    /// The document the occurrence sits in.
    pub document_path: String,
    /// The occurrence's byte span.
    pub span: (usize, usize),
    /// The role tag (`definition` or `reference`).
    pub role: String,
    /// The nearest enclosing persisted declaration, if attributed.
    pub enclosing_id: Option<CanonicalId>,
}

/// The recorded index metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMetadata {
    /// The workspace the index was built under.
    pub workspace_id: WorkspaceId,
    /// The analyzer provenance.
    pub provenance: AnalyzerProvenance,
    /// The content hash of the analyzed sources.
    pub content_hash: String,
    /// The join-alignment accounting for the build.
    pub accounting: JoinAccounting,
}

/// The freshness of the index relative to the sources and analyzer currently in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Sources and analyzer are unchanged since indexing.
    Fresh,
    /// The source content changed since indexing.
    StaleContent,
    /// The analyzer version differs from the recorded provenance.
    StaleVersion,
}

impl Freshness {
    /// Whether the index is stale (either reason).
    pub fn is_stale(&self) -> bool {
        !matches!(self, Freshness::Fresh)
    }
}

/// The SQLite-backed graph store.
pub struct GraphStore {
    conn: Connection,
}

impl GraphStore {
    /// Open (creating if needed) a store at `path`, applying the schema.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (for tests), applying the schema.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// A mutable transaction handle over the connection.
    pub fn transaction(&mut self) -> rusqlite::Result<rusqlite::Transaction<'_>> {
        self.conn.transaction()
    }

    /// Replace the index metadata row with the given build's metadata.
    pub fn write_metadata(&self, meta: &IndexMetadata) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO index_metadata
                (id, schema_version, workspace_id, analyzer_name, analyzer_version, content_hash,
                 aligned_count, text_mismatch_count, semantic_only_count, syntax_only_count)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                SCHEMA_VERSION,
                meta.workspace_id.as_str(),
                meta.provenance.analyzer_name,
                meta.provenance.analyzer_version,
                meta.content_hash,
                meta.accounting.aligned as i64,
                meta.accounting.text_mismatch as i64,
                meta.accounting.semantic_only as i64,
                meta.accounting.syntax_only as i64,
            ],
        )?;
        Ok(())
    }

    /// Read the recorded index metadata, if any build has been persisted.
    pub fn read_metadata(&self) -> rusqlite::Result<Option<IndexMetadata>> {
        self.conn
            .query_row(
                "SELECT workspace_id, analyzer_name, analyzer_version, content_hash,
                        aligned_count, text_mismatch_count, semantic_only_count, syntax_only_count
                 FROM index_metadata WHERE id = 1",
                [],
                |r| {
                    Ok(IndexMetadata {
                        workspace_id: WorkspaceId::new(r.get::<_, String>(0)?),
                        provenance: AnalyzerProvenance {
                            analyzer_name: r.get(1)?,
                            analyzer_version: r.get(2)?,
                        },
                        content_hash: r.get(3)?,
                        accounting: JoinAccounting {
                            aligned: r.get::<_, i64>(4)? as u64,
                            text_mismatch: r.get::<_, i64>(5)? as u64,
                            semantic_only: r.get::<_, i64>(6)? as u64,
                            syntax_only: r.get::<_, i64>(7)? as u64,
                        },
                    })
                },
            )
            .optional()
    }

    /// Insert a symbol row.
    pub fn insert_symbol(&self, row: &SymbolRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO symbols
                (canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.canonical_id.as_str(),
                row.display_name,
                row.kind,
                row.class.tag(),
                row.document_path,
                row.span.map(|s| s.0 as i64),
                row.span.map(|s| s.1 as i64),
                row.span_text,
            ],
        )?;
        Ok(())
    }

    /// Insert an occurrence row.
    pub fn insert_occurrence(&self, row: &OccurrenceRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO occurrences
                (symbol_id, document_path, span_start, span_end, role, enclosing_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.symbol_id.as_str(),
                row.document_path,
                row.span.0 as i64,
                row.span.1 as i64,
                row.role,
                row.enclosing_id.as_ref().map(|e| e.as_str().to_string()),
            ],
        )?;
        Ok(())
    }

    /// Insert a type-tagged edge, marking it verified per its kind.
    pub fn insert_edge(&self, kind: EdgeKind, src: &CanonicalId, dst: &CanonicalId) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO edges (kind, src_id, dst_id, verified) VALUES (?1, ?2, ?3, ?4)",
            params![kind.tag(), src.as_str(), dst.as_str(), kind.is_verified() as i64],
        )?;
        Ok(())
    }

    /// Fetch a symbol by canonical identity.
    pub fn symbol(&self, id: &CanonicalId) -> rusqlite::Result<Option<SymbolRow>> {
        self.conn
            .query_row(
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text
                 FROM symbols WHERE canonical_id = ?1",
                params![id.as_str()],
                Self::map_symbol,
            )
            .optional()
    }

    fn map_symbol(r: &rusqlite::Row) -> rusqlite::Result<SymbolRow> {
        let span_start: Option<i64> = r.get(5)?;
        let span_end: Option<i64> = r.get(6)?;
        let span = match (span_start, span_end) {
            (Some(s), Some(e)) => Some((s as usize, e as usize)),
            _ => None,
        };
        Ok(SymbolRow {
            canonical_id: CanonicalId::from_raw(r.get::<_, String>(0)?),
            display_name: r.get(1)?,
            kind: r.get(2)?,
            class: PersistedClass::from_tag(&r.get::<_, String>(3)?),
            document_path: r.get(4)?,
            span,
            span_text: r.get(7)?,
        })
    }

    /// All symbols whose display name equals `shortname`.
    pub fn symbols_by_shortname(&self, shortname: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text
             FROM symbols WHERE display_name = ?1 ORDER BY canonical_id",
        )?;
        let rows = stmt.query_map(params![shortname], Self::map_symbol)?;
        rows.collect()
    }

    /// All symbols whose canonical identity ends with the qualified-name suffix `::<qualified>`, or
    /// equals it exactly. Used to resolve a qualified name that omits the workspace prefix.
    pub fn symbols_by_qualified_suffix(&self, qualified: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let suffix = format!("::{qualified}");
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text
             FROM symbols
             WHERE canonical_id = ?1 OR canonical_id LIKE ?2 ESCAPE '\\'
             ORDER BY canonical_id",
        )?;
        let like = format!("%{}", escape_like(&suffix));
        let rows = stmt.query_map(params![qualified, like], Self::map_symbol)?;
        rows.collect()
    }

    /// The occurrences of a symbol, ordered deterministically.
    pub fn occurrences_of(&self, id: &CanonicalId) -> rusqlite::Result<Vec<OccurrenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol_id, document_path, span_start, span_end, role, enclosing_id
             FROM occurrences WHERE symbol_id = ?1
             ORDER BY document_path, span_start, span_end",
        )?;
        let rows = stmt.query_map(params![id.as_str()], Self::map_occurrence)?;
        rows.collect()
    }

    fn map_occurrence(r: &rusqlite::Row) -> rusqlite::Result<OccurrenceRow> {
        Ok(OccurrenceRow {
            symbol_id: CanonicalId::from_raw(r.get::<_, String>(0)?),
            document_path: r.get(1)?,
            span: (r.get::<_, i64>(2)? as usize, r.get::<_, i64>(3)? as usize),
            role: r.get(4)?,
            enclosing_id: r.get::<_, Option<String>>(5)?.map(CanonicalId::from_raw),
        })
    }

    /// The symbol whose definition span most tightly encloses `byte_offset` in `document`.
    ///
    /// Preferring the tightest (smallest) enclosing span resolves a position inside a method body to
    /// the method rather than its enclosing type or module.
    pub fn symbol_enclosing_position(
        &self,
        document: &str,
        byte_offset: usize,
    ) -> rusqlite::Result<Option<SymbolRow>> {
        self.conn
            .query_row(
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text
                 FROM symbols
                 WHERE document_path = ?1 AND span_start IS NOT NULL
                   AND span_start <= ?2 AND ?2 < span_end
                 ORDER BY (span_end - span_start) ASC, canonical_id ASC
                 LIMIT 1",
                params![document, byte_offset as i64],
                Self::map_symbol,
            )
            .optional()
    }

    /// The symbols directly contained by `id` (the `contains` relation, downward).
    pub fn contains(&self, id: &CanonicalId) -> rusqlite::Result<Vec<CanonicalId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT dst_id FROM edges WHERE kind = 'contains' AND src_id = ?1 ORDER BY dst_id")?;
        let rows = stmt.query_map(params![id.as_str()], |r| {
            Ok(CanonicalId::from_raw(r.get::<_, String>(0)?))
        })?;
        rows.collect()
    }

    /// The declaration that directly contains `id` (the `containers` relation, upward).
    pub fn containers(&self, id: &CanonicalId) -> rusqlite::Result<Vec<CanonicalId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src_id FROM edges WHERE kind = 'contains' AND dst_id = ?1 ORDER BY src_id")?;
        let rows = stmt.query_map(params![id.as_str()], |r| {
            Ok(CanonicalId::from_raw(r.get::<_, String>(0)?))
        })?;
        rows.collect()
    }

    /// Every reference-role occurrence of `id`, ordered deterministically.
    pub fn references_of(&self, id: &CanonicalId) -> rusqlite::Result<Vec<OccurrenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol_id, document_path, span_start, span_end, role, enclosing_id
             FROM occurrences WHERE symbol_id = ?1 AND role = 'reference'
             ORDER BY document_path, span_start, span_end",
        )?;
        let rows = stmt.query_map(params![id.as_str()], Self::map_occurrence)?;
        rows.collect()
    }

    /// Evaluate freshness against the sources' current content hash and the analyzer in effect.
    pub fn freshness(&self, current_hash: &str, current: &AnalyzerProvenance) -> rusqlite::Result<Option<Freshness>> {
        let Some(meta) = self.read_metadata()? else {
            return Ok(None);
        };
        // Content change takes precedence in reporting; version change flags reindex.
        if meta.content_hash != current_hash {
            return Ok(Some(Freshness::StaleContent));
        }
        if meta.provenance.analyzer_version != current.analyzer_version
            || meta.provenance.analyzer_name != current.analyzer_name
        {
            return Ok(Some(Freshness::StaleVersion));
        }
        Ok(Some(Freshness::Fresh))
    }
}

/// Escape LIKE wildcards in a literal fragment (using `\` as the escape char).
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
