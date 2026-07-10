//! The SQLite-core store: persistence and retrieval of symbols, occurrences, edges, and index
//! metadata, plus staleness evaluation.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::identity::{CanonicalId, WorkspaceId};
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts};

use super::join::JoinAccounting;
use super::schema::{SCHEMA_SQL, SCHEMA_VERSION};

/// An edge kind in the graph. All four kinds are contracted: `Contains` is enclosure; the three
/// dependency kinds — `Uses`, `Imports`, `TypeHierarchy` — are read by the dependents traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Enclosure: a declaration directly contains another.
    Contains,
    /// Reference-grade dependency: a declaration references a symbol in its body. Any mention counts
    /// — a call, a type usage, a constant read — invocation is not required.
    Uses,
    /// A module references a symbol at module scope.
    Imports,
    /// A type implements a trait.
    TypeHierarchy,
}

impl EdgeKind {
    /// The stored tag for this edge kind.
    pub fn tag(&self) -> &'static str {
        match self {
            EdgeKind::Contains => "contains",
            EdgeKind::Uses => "uses",
            EdgeKind::Imports => "imports",
            EdgeKind::TypeHierarchy => "type_hierarchy",
        }
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
    /// Whether this symbol is a true same-descriptor twin: an in-workspace definition whose
    /// identical resolved descriptor is shared by at least one other definition. Distinct
    /// descriptors whose canonical projections merely collide are not duplicated.
    pub duplicated: bool,
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
    /// The alignment rule that accepted the attribution (`exact`, `crate_root`, `operator_desugar`,
    /// `module_span`, `self_keyword`, or `module_name`) — its provenance.
    pub rule: String,
    /// The nearest enclosing persisted declaration, if attributed.
    pub enclosing_id: Option<CanonicalId>,
    /// The locality rule (`defining_document`, `module_chain`, `target_metadata`, or
    /// `declaration_scope`) that selected this attribution's twin, for an occurrence resolved from a
    /// duplicated descriptor's group; `None` for an ordinary (non-duplicated) attribution.
    pub locality: Option<String>,
}

/// The maximum number of `found_text` bytes persisted per discrepancy row.
///
/// A provisional design constant: name tokens are far shorter than this, and the equality check that
/// classifies the outcome runs on the full bytes before truncation, so classification is unaffected.
/// Superseded when the interface-layer pagination change defines the real bounded-output model.
pub const FOUND_TEXT_MAX_BYTES: usize = 120;

/// The maximum number of groups the default discrepancy summary displays.
///
/// A provisional design constant, superseded by the pagination change. When more groups exist, the
/// summary marks itself truncated; its totals are computed over the full persisted set regardless.
pub const DISCREPANCY_GROUP_CAP: usize = 50;

/// The maximum hop distance the dependents traversal walks.
///
/// A provisional design constant — like [`DISCREPANCY_GROUP_CAP`], it bounds the walk until the
/// pagination change lands. It exceeds any plausible real dependency chain while keeping the
/// recursive query cheap, and it guarantees termination even on cyclic dependency graphs.
pub const DEPENDENTS_HORIZON: u32 = 20;

/// One dependent of a seed symbol: its identity, its shortest hop distance from the seed, and the
/// kind of dependency edge that connected it at that distance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependentRow {
    /// The dependent symbol's canonical identity.
    pub id: CanonicalId,
    /// The shortest hop distance from the seed.
    pub depth: u32,
    /// The connecting edge kind, chosen from a shortest-depth hop under the fixed tie-break.
    pub kind: String,
}

/// A persisted join-discrepancy row: one non-aligned occurrence's inspectable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscrepancyRow {
    /// The document the occurrence sits in.
    pub document_path: String,
    /// The byte span at the occurrence's location, or `None` when the occurrence's coordinates could
    /// not be normalized onto the source — typed absence, never a fabricated location.
    pub span: Option<(usize, usize)>,
    /// The outcome kind tag (`text_mismatch`, `semantic_only`, or `duplicate_ambiguous`).
    pub outcome: String,
    /// The expected name token.
    pub expected_name: String,
    /// The source text found at the location, truncated to [`FOUND_TEXT_MAX_BYTES`], if any.
    pub found_text: Option<String>,
}

/// One group in the bounded discrepancy summary: an `(outcome, expected_name)` key with its count,
/// the number of distinct documents it spans, and one exemplar location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscrepancyGroup {
    /// The outcome kind tag.
    pub outcome: String,
    /// The expected name token shared by the group.
    pub expected_name: String,
    /// How many discrepancies fall in this group.
    pub count: u64,
    /// How many distinct documents the group's discrepancies span.
    pub document_count: u64,
    /// One exemplar location (document and span) drawn from the group.
    pub exemplar: DiscrepancyRow,
}

/// The bounded discrepancy summary: the displayed groups plus the full-set totals it summarizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscrepancySummary {
    /// The groups shown, most-populous first, capped at [`DISCREPANCY_GROUP_CAP`].
    pub groups: Vec<DiscrepancyGroup>,
    /// The total number of distinct `(outcome, expected_name)` groups in the full persisted set.
    pub total_groups: u64,
    /// The total number of discrepancy rows in the full persisted set.
    pub total_discrepancies: u64,
}

impl DiscrepancySummary {
    /// Whether the summary withheld groups under the display cap.
    pub fn truncated(&self) -> bool {
        self.total_groups > self.groups.len() as u64
    }
}

/// A duplicated-descriptor group: the shared descriptor's canonical base and the persisted
/// definitions that share it.
///
/// Derived at query time from the persisted symbols table (design.md: "derived, not separately
/// persisted"): members are the symbols marked `duplicated` at ingest — true same-descriptor twins,
/// never canonical-projection collisions of distinct descriptors — grouped by their identity with
/// the `#<rank>` disambiguator stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicatedGroup {
    /// The shared descriptor's canonical base (the identity with its `#<rank>` suffix stripped).
    pub descriptor_base: String,
    /// The definitions that share the descriptor, ordered by canonical identity (their disambiguator
    /// rank).
    pub definitions: Vec<SymbolRow>,
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
    /// The backend's declared interpreter-environment facts, if it declared any (`None` for the
    /// Rust adapter). Compared whole against the environment in effect by [`GraphStore::freshness`].
    pub environment: Option<EnvironmentFacts>,
}

/// The freshness of the index relative to the sources, analyzer, and declared environment currently
/// in effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Sources, analyzer, and declared environment are unchanged since indexing.
    Fresh,
    /// The source content changed since indexing.
    StaleContent,
    /// The analyzer version differs from the recorded provenance.
    StaleVersion,
    /// The recorded interpreter environment differs from the one in effect.
    StaleEnvironment,
}

impl Freshness {
    /// Whether the index is stale (either reason).
    pub fn is_stale(&self) -> bool {
        !matches!(self, Freshness::Fresh)
    }
}

/// An error opening a graph store.
///
/// The schema-version guard reads `PRAGMA user_version` before any table access — the pragma is
/// readable regardless of table shapes, which is exactly why it is the guard mechanism (the in-row
/// `schema_version` column remains as provenance only).
#[derive(Debug, thiserror::Error)]
pub enum StoreOpenError {
    /// The store was written under a different schema version than this binary expects. A pre-guard
    /// store carries no stamp and reads as version 0.
    #[error(
        "index store at {path} carries schema version {found}, but this binary expects version {expected}; \
         run `c10r build` to rebuild it (or delete the file)"
    )]
    SchemaVersionMismatch {
        /// The store's path, for the teaching message.
        path: String,
        /// The version stamped in the store (0 for an unstamped, pre-guard store).
        found: i64,
        /// The version this binary writes.
        expected: i64,
    },
    /// The underlying storage failed.
    #[error("store error: {0}")]
    Storage(#[from] rusqlite::Error),
    /// Deleting an incompatible store for replacement failed at the filesystem.
    #[error("replacing incompatible index store: {0}")]
    Replace(#[from] std::io::Error),
}

/// The SQLite-backed graph store.
pub struct GraphStore {
    conn: Connection,
}

impl GraphStore {
    /// Open a store at `path` for reading, creating a fresh one if none exists.
    ///
    /// An existing store's `PRAGMA user_version` stamp is validated against [`SCHEMA_VERSION`]
    /// before any table is touched; a mismatch (including an unstamped pre-guard store, which reads
    /// as version 0) refuses with a typed teaching error rather than failing mid-operation on a
    /// changed table shape.
    pub fn open(path: &Path) -> Result<Self, StoreOpenError> {
        if !path.exists() {
            return Ok(Self::create(path)?);
        }
        let conn = Connection::open(path)?;
        let found = stamped_version(&conn)?;
        if found != SCHEMA_VERSION {
            return Err(StoreOpenError::SchemaVersionMismatch {
                path: path.display().to_string(),
                found,
                expected: SCHEMA_VERSION,
            });
        }
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self { conn })
    }

    /// Open a store at `path` for a build, replacing it wholesale on a schema-version mismatch.
    ///
    /// The index is derived, replayable data, so rebuild is the migration: an incompatible store is
    /// deleted and recreated at the current version, and the build proceeds.
    pub fn open_or_replace(path: &Path) -> Result<Self, StoreOpenError> {
        match Self::open(path) {
            Err(StoreOpenError::SchemaVersionMismatch { .. }) => {
                remove_store_files(path)?;
                Ok(Self::create(path)?)
            }
            other => other,
        }
    }

    /// Create a fresh store at `path`: apply the schema and stamp the version pragma.
    fn create(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Open an in-memory store (for tests), applying the schema and version stamp.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Begin the single transaction a build's writes run inside.
    ///
    /// The returned guard rolls the transaction back on drop unless committed, so a failed build
    /// never leaves the store half-cleared or half-written — the prior build stays authoritative
    /// wholesale. Uses an unchecked transaction so the store's `&self` write methods remain callable
    /// while the guard is alive (every statement on the connection joins the open transaction).
    pub fn begin_build(&self) -> rusqlite::Result<rusqlite::Transaction<'_>> {
        self.conn.unchecked_transaction()
    }

    /// Delete every derived row — occurrences, edges, discrepancies, then symbols — so the running
    /// build wholly supersedes the prior one.
    ///
    /// Children first: `occurrences` and `edges` carry foreign keys into `symbols`. Meant to run
    /// inside [`Self::begin_build`]'s transaction; `index_metadata` is not touched (its single row is
    /// replaced by [`Self::write_metadata`]).
    pub fn clear_derived(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM occurrences", [])?;
        self.conn.execute("DELETE FROM edges", [])?;
        self.conn.execute("DELETE FROM join_discrepancies", [])?;
        self.conn.execute("DELETE FROM symbols", [])?;
        Ok(())
    }

    /// Replace the index metadata row with the given build's metadata.
    pub fn write_metadata(&self, meta: &IndexMetadata) -> rusqlite::Result<()> {
        // Declared environment facts persist as JSON text; a backend that declares none writes NULL.
        let environment = meta
            .environment
            .as_ref()
            .map(|facts| serde_json::to_string(facts).expect("environment facts serialize"));
        self.conn.execute(
            "INSERT OR REPLACE INTO index_metadata
                (id, schema_version, workspace_id, analyzer_name, analyzer_version, environment, content_hash,
                 aligned_exact_count, aligned_crate_root_count, aligned_operator_desugar_count,
                 aligned_module_span_count, aligned_self_keyword_count, aligned_module_name_count,
                 text_mismatch_count, semantic_only_count, duplicate_ambiguous_count, syntax_only_count)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                SCHEMA_VERSION,
                meta.workspace_id.as_str(),
                meta.provenance.analyzer_name,
                meta.provenance.analyzer_version,
                environment,
                meta.content_hash,
                meta.accounting.aligned_exact as i64,
                meta.accounting.aligned_crate_root as i64,
                meta.accounting.aligned_operator_desugar as i64,
                meta.accounting.aligned_module_span as i64,
                meta.accounting.aligned_self_keyword as i64,
                meta.accounting.aligned_module_name as i64,
                meta.accounting.text_mismatch as i64,
                meta.accounting.semantic_only as i64,
                meta.accounting.duplicate_ambiguous as i64,
                meta.accounting.syntax_only as i64,
            ],
        )?;
        Ok(())
    }

    /// Read the recorded index metadata, if any build has been persisted.
    pub fn read_metadata(&self) -> rusqlite::Result<Option<IndexMetadata>> {
        self.conn
            .query_row(
                "SELECT workspace_id, analyzer_name, analyzer_version, environment, content_hash,
                        aligned_exact_count, aligned_crate_root_count, aligned_operator_desugar_count,
                        aligned_module_span_count, aligned_self_keyword_count, aligned_module_name_count,
                        text_mismatch_count, semantic_only_count, duplicate_ambiguous_count, syntax_only_count
                 FROM index_metadata WHERE id = 1",
                [],
                |r| {
                    // A NULL column is a backend that declared no environment; unparsable JSON is a
                    // corrupt row surfaced as a typed conversion error, never silently dropped
                    // (dropping it would report a drifted environment as fresh).
                    let environment = r
                        .get::<_, Option<String>>(3)?
                        .map(|json| {
                            serde_json::from_str::<EnvironmentFacts>(&json).map_err(|e| {
                                rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
                            })
                        })
                        .transpose()?;
                    Ok(IndexMetadata {
                        workspace_id: WorkspaceId::new(r.get::<_, String>(0)?),
                        provenance: AnalyzerProvenance {
                            analyzer_name: r.get(1)?,
                            analyzer_version: r.get(2)?,
                        },
                        environment,
                        content_hash: r.get(4)?,
                        accounting: JoinAccounting {
                            aligned_exact: r.get::<_, i64>(5)? as u64,
                            aligned_crate_root: r.get::<_, i64>(6)? as u64,
                            aligned_operator_desugar: r.get::<_, i64>(7)? as u64,
                            aligned_module_span: r.get::<_, i64>(8)? as u64,
                            aligned_self_keyword: r.get::<_, i64>(9)? as u64,
                            aligned_module_name: r.get::<_, i64>(10)? as u64,
                            text_mismatch: r.get::<_, i64>(11)? as u64,
                            semantic_only: r.get::<_, i64>(12)? as u64,
                            duplicate_ambiguous: r.get::<_, i64>(13)? as u64,
                            syntax_only: r.get::<_, i64>(14)? as u64,
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
                (canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.canonical_id.as_str(),
                row.display_name,
                row.kind,
                row.class.tag(),
                row.document_path,
                row.span.map(|s| s.0 as i64),
                row.span.map(|s| s.1 as i64),
                row.span_text,
                row.duplicated as i64,
            ],
        )?;
        Ok(())
    }

    /// Insert an occurrence row.
    pub fn insert_occurrence(&self, row: &OccurrenceRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO occurrences
                (symbol_id, document_path, span_start, span_end, role, rule, enclosing_id, locality)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.symbol_id.as_str(),
                row.document_path,
                row.span.0 as i64,
                row.span.1 as i64,
                row.role,
                row.rule,
                row.enclosing_id.as_ref().map(|e| e.as_str().to_string()),
                row.locality,
            ],
        )?;
        Ok(())
    }

    /// Insert a type-tagged edge idempotently: an edge is a relation instance, unique on
    /// `(kind, src, dst)`, so re-inserting the same relation (a repeated occurrence, or a future
    /// incremental rebuild) leaves exactly one row. Uniqueness is enforced by the schema index, so
    /// every write path inherits the dedup.
    pub fn insert_edge(&self, kind: EdgeKind, src: &CanonicalId, dst: &CanonicalId) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO edges (kind, src_id, dst_id) VALUES (?1, ?2, ?3)",
            params![kind.tag(), src.as_str(), dst.as_str()],
        )?;
        Ok(())
    }

    /// Insert the build's join-discrepancy detail rows.
    ///
    /// The prior build's rows are removed by [`Self::clear_derived`], and atomicity — a reader sees
    /// either the old build's set or the new one, never a mix — comes from the surrounding
    /// [`Self::begin_build`] transaction. Each `found_text` is truncated to
    /// [`FOUND_TEXT_MAX_BYTES`] on a UTF-8 boundary before persistence.
    pub fn insert_discrepancies(&self, rows: &[DiscrepancyRow]) -> rusqlite::Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO join_discrepancies
                (document_path, span_start, span_end, outcome, expected_name, found_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for row in rows {
            let found = row.found_text.as_deref().map(truncate_on_boundary);
            stmt.execute(params![
                row.document_path,
                row.span.map(|s| s.0 as i64),
                row.span.map(|s| s.1 as i64),
                row.outcome,
                row.expected_name,
                found,
            ])?;
        }
        Ok(())
    }

    /// The bounded discrepancy summary: groups by `(outcome, expected_name)` ordered by descending
    /// count (capped at [`DISCREPANCY_GROUP_CAP`]), with the full-set totals the cap summarizes.
    pub fn discrepancy_summary(&self) -> rusqlite::Result<DiscrepancySummary> {
        let mut stmt = self.conn.prepare(
            "SELECT outcome, expected_name, COUNT(*) AS n, COUNT(DISTINCT document_path) AS docs
             FROM join_discrepancies
             GROUP BY outcome, expected_name
             ORDER BY n DESC, outcome ASC, expected_name ASC
             LIMIT ?1",
        )?;
        let keyed: Vec<(String, String, u64, u64)> = stmt
            .query_map(params![DISCREPANCY_GROUP_CAP as i64], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, i64>(3)? as u64,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;

        // The exemplar is a genuine persisted row from the group — never a synthetic pairing of
        // per-column aggregates — chosen deterministically, preferring a row with a real span so a
        // typed-absence span appears only when the whole group lacks locations.
        let mut exemplar_stmt = self.conn.prepare(
            "SELECT document_path, span_start, span_end, outcome, expected_name, found_text
             FROM join_discrepancies
             WHERE outcome = ?1 AND expected_name = ?2
             ORDER BY span_start IS NULL, document_path, span_start, span_end
             LIMIT 1",
        )?;
        let mut groups = Vec::with_capacity(keyed.len());
        for (outcome, expected_name, count, document_count) in keyed {
            let exemplar = exemplar_stmt.query_row(params![outcome, expected_name], Self::map_discrepancy)?;
            groups.push(DiscrepancyGroup {
                outcome,
                expected_name,
                count,
                document_count,
                exemplar,
            });
        }

        // Full-set totals: computed over every persisted row, never only the displayed groups.
        let total_discrepancies: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM join_discrepancies", [], |r| r.get(0))?;
        let total_groups: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM join_discrepancies GROUP BY outcome, expected_name)",
            [],
            |r| r.get(0),
        )?;

        Ok(DiscrepancySummary {
            groups,
            total_groups: total_groups as u64,
            total_discrepancies: total_discrepancies as u64,
        })
    }

    /// Every persisted discrepancy row, ordered deterministically — the explicit full listing.
    pub fn all_discrepancies(&self) -> rusqlite::Result<Vec<DiscrepancyRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_path, span_start, span_end, outcome, expected_name, found_text
             FROM join_discrepancies
             ORDER BY document_path, span_start, span_end, outcome, expected_name",
        )?;
        let rows = stmt.query_map([], Self::map_discrepancy)?;
        rows.collect()
    }

    fn map_discrepancy(r: &rusqlite::Row) -> rusqlite::Result<DiscrepancyRow> {
        let span_start: Option<i64> = r.get(1)?;
        let span_end: Option<i64> = r.get(2)?;
        let span = match (span_start, span_end) {
            (Some(s), Some(e)) => Some((s as usize, e as usize)),
            _ => None,
        };
        Ok(DiscrepancyRow {
            document_path: r.get(0)?,
            span,
            outcome: r.get(3)?,
            expected_name: r.get(4)?,
            found_text: r.get(5)?,
        })
    }

    /// Fetch a symbol by canonical identity.
    pub fn symbol(&self, id: &CanonicalId) -> rusqlite::Result<Option<SymbolRow>> {
        self.conn
            .query_row(
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated
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
            duplicated: r.get::<_, i64>(8)? != 0,
        })
    }

    /// All symbols whose display name equals `shortname`.
    pub fn symbols_by_shortname(&self, shortname: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated
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
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated
             FROM symbols
             WHERE canonical_id = ?1 OR canonical_id LIKE ?2 ESCAPE '\\'
             ORDER BY canonical_id",
        )?;
        let like = format!("%{}", escape_like(&suffix));
        let rows = stmt.query_map(params![qualified, like], Self::map_symbol)?;
        rows.collect()
    }

    /// The duplicated-descriptor groups the build encountered: symbols marked `duplicated` at ingest
    /// (true same-descriptor twins, under the two-definition quorum), grouped by their canonical
    /// identity with the `#<rank>` disambiguator stripped (the shared descriptor's base).
    ///
    /// The `duplicated` mark is what distinguishes real twins from canonical-projection collisions:
    /// two DISTINCT descriptors whose base projections happen to collide also carry `#<rank>`
    /// suffixes, but they are not duplicated descriptors — their references attribute normally — and
    /// they are never reported here.
    ///
    /// A definite empty set (`Vec::new()`) when no duplicated descriptor was observed, distinct from
    /// a query failure.
    pub fn duplicated_groups(&self) -> rusqlite::Result<Vec<DuplicatedGroup>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated
             FROM symbols WHERE class = 'in_workspace' AND duplicated = 1 ORDER BY canonical_id",
        )?;
        let rows: Vec<SymbolRow> = stmt.query_map([], Self::map_symbol)?.collect::<rusqlite::Result<_>>()?;

        let mut by_base: std::collections::BTreeMap<String, Vec<SymbolRow>> = std::collections::BTreeMap::new();
        for row in rows {
            if let Some(base) = disambiguator_base(row.canonical_id.as_str()) {
                by_base.entry(base).or_default().push(row);
            }
        }

        Ok(by_base
            .into_iter()
            .filter(|(_, members)| members.len() > 1)
            .map(|(descriptor_base, definitions)| DuplicatedGroup {
                descriptor_base,
                definitions,
            })
            .collect())
    }

    /// The occurrences of a symbol, ordered deterministically.
    pub fn occurrences_of(&self, id: &CanonicalId) -> rusqlite::Result<Vec<OccurrenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol_id, document_path, span_start, span_end, role, rule, enclosing_id, locality
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
            rule: r.get(5)?,
            enclosing_id: r.get::<_, Option<String>>(6)?.map(CanonicalId::from_raw),
            locality: r.get(7)?,
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
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text, duplicated
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

    /// Every `(src, dst)` pair of a given edge kind, ordered deterministically. Exposes the derived
    /// dependency edges for inspection and ground-truth comparison.
    pub fn edges(&self, kind: EdgeKind) -> rusqlite::Result<Vec<(CanonicalId, CanonicalId)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src_id, dst_id FROM edges WHERE kind = ?1 ORDER BY src_id, dst_id")?;
        let rows = stmt.query_map(params![kind.tag()], |r| {
            Ok((
                CanonicalId::from_raw(r.get::<_, String>(0)?),
                CanonicalId::from_raw(r.get::<_, String>(1)?),
            ))
        })?;
        rows.collect()
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

    /// Compute the seed's dependents: symbols whose dependency edges (`uses`, `imports`,
    /// `type_hierarchy`) reach the seed directly or transitively, walking each edge `dst → src`.
    ///
    /// Enclosure (`contains`) never propagates dependence — it supplies attribution, not impact — so
    /// it is excluded from the walk. Each dependent is returned exactly once at its shortest hop
    /// distance, carrying the connecting edge kind chosen from a shortest-depth hop under a fixed
    /// tie-break: kind order (`uses` < `imports` < `type_hierarchy`), then canonical identity. The
    /// seed never appears as its own dependent (a self-loop is inert), and the walk is capped at
    /// `horizon` hops so cycles terminate. Results are ordered by `(depth, kind order, identity)`.
    pub fn dependents(&self, seed: &CanonicalId, horizon: u32) -> rusqlite::Result<Vec<DependentRow>> {
        // `UNION` (not `UNION ALL`) dedupes emitted `(id, depth, kind)` rows globally as SQLite
        // evaluates the recursion, so a node reachable through many paths is queued and expanded once
        // rather than once per path — the row count grows with node count, not path count, which
        // matters because fan-in makes path count exponential while node count stays polynomial. The
        // dedup key is exactly the three projected columns, so keep them id/depth/kind, unmixed with
        // anything path-dependent (e.g. no path list), or a distinct-per-path row would slip back in.
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE reach(id, depth, kind) AS (
                SELECT src_id, 1, kind FROM edges
                  WHERE kind IN ('uses', 'imports', 'type_hierarchy') AND dst_id = ?1 AND src_id <> ?1
                UNION
                SELECT e.src_id, r.depth + 1, e.kind
                  FROM edges e JOIN reach r ON e.dst_id = r.id
                  WHERE e.kind IN ('uses', 'imports', 'type_hierarchy') AND e.src_id <> ?1 AND r.depth < ?2
             )
             SELECT id, depth, kind FROM reach",
        )?;
        let raw: Vec<(String, u32, String)> = stmt
            .query_map(params![seed.as_str(), horizon as i64], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)? as u32, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;

        // Collapse the raw hops to one entry per symbol: the minimum depth, and among the hops at
        // that minimum depth the connecting kind lowest under the fixed kind order.
        let mut best: std::collections::HashMap<String, (u32, String)> = std::collections::HashMap::new();
        for (id, depth, kind) in raw {
            let replace = match best.get(&id) {
                None => true,
                Some((d, k)) => depth < *d || (depth == *d && kind_order(&kind) < kind_order(k)),
            };
            if replace {
                best.insert(id, (depth, kind));
            }
        }

        let mut out: Vec<DependentRow> = best
            .into_iter()
            .map(|(id, (depth, kind))| DependentRow {
                id: CanonicalId::from_raw(id),
                depth,
                kind,
            })
            .collect();
        out.sort_by(|a, b| {
            a.depth
                .cmp(&b.depth)
                .then_with(|| kind_order(&a.kind).cmp(&kind_order(&b.kind)))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }

    /// Every reference-role occurrence of `id`, ordered deterministically.
    pub fn references_of(&self, id: &CanonicalId) -> rusqlite::Result<Vec<OccurrenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT symbol_id, document_path, span_start, span_end, role, rule, enclosing_id, locality
             FROM occurrences WHERE symbol_id = ?1 AND role = 'reference'
             ORDER BY document_path, span_start, span_end",
        )?;
        let rows = stmt.query_map(params![id.as_str()], Self::map_occurrence)?;
        rows.collect()
    }

    /// Evaluate freshness against the sources' current content hash, the analyzer in effect, and
    /// the declared environment in effect (`None` when no environment applies or none resolves).
    pub fn freshness(
        &self,
        current_hash: &str,
        current: &AnalyzerProvenance,
        current_environment: Option<&EnvironmentFacts>,
    ) -> rusqlite::Result<Option<Freshness>> {
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
        // The whole declared environment must match, absence included: a recorded environment that
        // can no longer be resolved is drift, never reported fresh.
        if meta.environment.as_ref() != current_environment {
            return Ok(Some(Freshness::StaleEnvironment));
        }
        Ok(Some(Freshness::Fresh))
    }
}

/// The schema version stamped in a store's `PRAGMA user_version` (0 for an unstamped store).
fn stamped_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// Delete an incompatible store's file and its SQLite sidecar files, tolerating absent sidecars.
fn remove_store_files(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        match std::fs::remove_file(std::path::Path::new(&sidecar)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The descriptor base of a canonical identity: the identity with a trailing `#<digits>`
/// disambiguator stripped, or `None` when the identity carries no disambiguator.
///
/// [`project_all`](crate::identity::project_all) appends `#<rank>` only to members of an observed
/// collision group, so this recovers exactly the shared base a duplicated-descriptor group formed
/// under.
fn disambiguator_base(canonical_id: &str) -> Option<String> {
    let (base, rank) = canonical_id.rsplit_once('#')?;
    if !rank.is_empty() && rank.bytes().all(|b| b.is_ascii_digit()) {
        Some(base.to_string())
    } else {
        None
    }
}

/// Truncate `s` to at most [`FOUND_TEXT_MAX_BYTES`] bytes, cutting on a UTF-8 character boundary so
/// the persisted text is always valid UTF-8.
fn truncate_on_boundary(s: &str) -> String {
    if s.len() <= FOUND_TEXT_MAX_BYTES {
        return s.to_string();
    }
    let mut end = FOUND_TEXT_MAX_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// The fixed order dependency edge kinds break ties by, so equal-depth hops choose a connecting kind
/// deterministically and results order reproducibly. Non-dependency tags sort last.
///
/// The fallback bucket is unreachable while `EdgeKind` stays closed to the three dependency kinds
/// above; adding a new edge kind to the dependents walk requires adding it here too, or it will
/// silently sort last instead of taking its intended tie-break position.
fn kind_order(tag: &str) -> u8 {
    match tag {
        "uses" => 0,
        "imports" => 1,
        "type_hierarchy" => 2,
        _ => 3,
    }
}

/// Escape LIKE wildcards in a literal fragment (using `\` as the escape char).
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}
