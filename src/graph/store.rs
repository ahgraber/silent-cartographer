//! The SQLite-core store: persistence and retrieval of symbols, occurrences, edges, and index
//! metadata, plus staleness evaluation.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::identity::{CanonicalId, WorkspaceId};
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts};

use super::chunk::ChunkParams;
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
    /// The signature tier: the declaration form without its body. `None` for externals and for
    /// in-workspace symbols carrying no definition span.
    pub signature_text: Option<String>,
    /// The interface tier: the signature together with the symbol's own documentation, equal to
    /// the signature when the symbol carries no documentation. `None` for externals and for
    /// in-workspace symbols carrying no definition span.
    pub interface_text: Option<String>,
    /// Whether this symbol is a true same-descriptor twin: an in-workspace definition whose
    /// identical resolved descriptor is shared by at least one other definition. Distinct
    /// descriptors whose canonical projections merely collide are not duplicated.
    pub duplicated: bool,
    /// The per-symbol test classification: `None` means non-test; a rule name means the symbol is
    /// test code, carrying the convention rule that stamped it as provenance. The rule vocabulary is
    /// open — an unrecognized name is a valid classification, never an error.
    pub test_rule: Option<String>,
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

/// The largest `k` the sqlite-vec extension accepts in one KNN query. A vector-signal request over a
/// corpus larger than this ranks the nearest `KNN_MAX_K` candidates rather than erroring.
const KNN_MAX_K: usize = 4096;

/// The widest frontier the dependents walk binds into a single query; a wider frontier is batched
/// across several queries within the same round. Held safely under SQLite's lowest historical
/// bound-parameter limit (999) so the walk never fails on a hub symbol's frontier.
const DEPENDENTS_FRONTIER_BATCH: usize = 900;

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

/// The rank graph projection [`GraphStore::rank_projection`] loads: the node universe and the
/// collapsed dependency edges, both in canonical-identity order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankProjection {
    /// Every in-workspace symbol, ordered by canonical identity. Isolated symbols are included.
    pub nodes: Vec<CanonicalId>,
    /// The collapsed dependency edges as `(src, dst)` indices into `nodes`, ordered by the source's
    /// then the destination's canonical identity. `src` depends on `dst`.
    pub edges: Vec<(usize, usize)>,
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
    /// The canonicalized filesystem root the build indexed — the store's durable link to the
    /// workspace it describes, compared against the root a query is invoked with.
    ///
    /// `None` when the build's root could not be recorded exactly (a path that is not valid UTF-8).
    /// Typed absence rather than a lossy rendering: two distinct roots can render to the same lossy
    /// text, and a comparison against that text would report a different workspace as a match.
    pub workspace_root: Option<String>,
    /// The analyzer provenance.
    pub provenance: AnalyzerProvenance,
    /// The content hash of the analyzed sources.
    pub content_hash: String,
    /// The join-alignment accounting for the build.
    pub accounting: JoinAccounting,
    /// The backend's declared interpreter-environment facts, if it declared any (`None` for the
    /// Rust adapter). Compared whole against the environment in effect by [`GraphStore::freshness`].
    pub environment: Option<EnvironmentFacts>,
    /// The chunk parameters the build ran under, recorded as part of the semantic-index identity.
    pub chunk_params: ChunkParams,
}

/// The semantic-index identity recorded with a build: the embedding model, the corpus/render
/// definition, and the chunk parameters that produced the build's semantic representations.
/// Carried on every `search` and `similar` answer as provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexIdentity {
    /// The embedding model identity (upstream repository at its vendored revision).
    pub model_identity: String,
    /// The corpus definition version the render derives under.
    pub corpus_definition_version: u32,
    /// The chunk parameters the build ran under — the operator's values, not the release's.
    pub chunk_params: ChunkParams,
}

/// One persisted semantic representation: a passage's render together with the embedding bytes of
/// its chunks in ordinal order, keyed by its symbol identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRepresentation {
    /// The render text the representations derive from.
    pub render: String,
    /// The embedding vector of each chunk, ordinal order, as little-endian `f32` bytes.
    pub embeddings: Vec<Vec<u8>>,
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

/// The SQLite `application_id` stamped into every store this binary creates: the big-endian bytes of
/// the ASCII string `c10r`.
///
/// The ownership marker, held apart from the schema version so migrating the schema never disturbs
/// the proof of whose file this is. It is a permanent file-format commitment: changing it would
/// orphan every stamped store.
pub const APPLICATION_ID: i32 = 0x6331_3072;

/// The 16-byte magic every SQLite database file opens with.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// How much of a candidate file's header the recognizer reads: the SQLite header is 100 bytes, and
/// the two fields the verdict turns on (the magic at offset 0, the `application_id` at offset 68)
/// sit inside it.
const HEADER_BYTES: usize = 100;

/// The byte offset of `application_id` in the SQLite file header.
const APPLICATION_ID_OFFSET: usize = 68;

/// What sits at a store path, decided from the file's header alone.
///
/// The verdict is reached by a plain read — no SQLite connection is opened — so recognizing a file
/// never locks it, never creates journal sidecars beside it, and never writes to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreRecognition {
    /// No file exists at the path.
    Absent,
    /// A file exists, but it is not a store this binary created: not a database at all, truncated,
    /// or a database carrying a different application's marker (or none).
    Unrecognized,
    /// A store carrying this binary's ownership marker.
    Recognized,
}

/// Recognize what sits at `path` by reading its header, without opening it as a database.
///
/// The single ownership test every store-touching path consults before it writes, replaces, or
/// deletes. An I/O failure other than "no such file" is surfaced rather than folded into a verdict,
/// so an unreadable path is never mistaken for an unrecognized one.
pub fn recognize_store(path: &Path) -> std::io::Result<StoreRecognition> {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StoreRecognition::Absent),
        Err(e) => return Err(e),
    };
    let mut header = [0u8; HEADER_BYTES];
    let mut filled = 0;
    while filled < header.len() {
        // A signal arriving mid-read is not a failed examination: retry rather than surfacing it.
        let n = match file.read(&mut header[filled..]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        filled += n;
    }
    if filled < header.len() || &header[0..SQLITE_MAGIC.len()] != SQLITE_MAGIC {
        return Ok(StoreRecognition::Unrecognized);
    }
    let stamped = i32::from_be_bytes([
        header[APPLICATION_ID_OFFSET],
        header[APPLICATION_ID_OFFSET + 1],
        header[APPLICATION_ID_OFFSET + 2],
        header[APPLICATION_ID_OFFSET + 3],
    ]);
    if stamped == APPLICATION_ID {
        Ok(StoreRecognition::Recognized)
    } else {
        Ok(StoreRecognition::Unrecognized)
    }
}

/// The recognizer's verdict, with a failed examination rendered as [`StoreOpenError::UnreadableStore`]
/// rather than a bare I/O error.
///
/// Every path that refuses on the verdict reads it through here. A path that cannot be examined is
/// refused in the same category as an unrecognized one — the caller's next action is identical, so the
/// exit code is too — while the message stays honest that ownership was never established either way.
pub fn recognize_or_refuse(path: &Path) -> Result<StoreRecognition, StoreOpenError> {
    recognize_store(path).map_err(|cause| unreadable_store(path, &cause))
}

/// Build the refusal for a path whose contents could not be examined.
///
/// The operating system's own words carry the cause. A hint is added only where those words do not
/// imply the fix: naming a directory is the one-token `--db` mistype, and a permissions failure is the
/// case where claiming the target is not this binary's store would be a claim the read never
/// established. Every other cause stands on the OS message alone.
///
/// The message stays on one line: every refusal reaches the caller through
/// [`crate::render::sanitize`], which makes control characters visible rather than executing them, so
/// a literal newline would render as a replacement character instead of a break.
fn unreadable_store(path: &Path, cause: &std::io::Error) -> StoreOpenError {
    let hint = match cause.kind() {
        std::io::ErrorKind::IsADirectory => format!(
            "; `--db` wants the database file inside it, such as {}",
            path.join("index.db").display()
        ),
        std::io::ErrorKind::PermissionDenied => "; check the file's owner and permissions".to_string(),
        _ => String::new(),
    };
    StoreOpenError::UnreadableStore {
        path: path.display().to_string(),
        detail: format!("{cause}{hint}"),
    }
}

/// Register the sqlite-vec extension for every connection this process opens, exactly once.
///
/// The schema declares a `vec0` virtual table, so the extension must be present before any
/// connection creates, opens, or queries a store. Auto-extension registration makes SQLite load the
/// statically-compiled extension into each new connection; both open chokepoints
/// ([`open_literal`], [`GraphStore::open_in_memory`]) call this first.
fn ensure_vec_extension() {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ())));
    });
}

/// Open `path` as a literal filename, never as a URI, with `access` deciding read-only, read-write,
/// or read-write-and-create.
///
/// SQLite interprets a filename that *begins* with `file:` as a URI naming a different file than the
/// one on disk with that name — and this build enables that interpretation globally, so withholding
/// `SQLITE_OPEN_URI` does not switch it off. The ownership guard inspects the path as a literal
/// filename, so a connection that resolved it any other way would read or write a file the guard
/// never checked. Handing SQLite an absolute path is what closes that gap: an absolute path cannot
/// begin with `file:`, so it can only ever name the file the guard inspected. Absolutizing is
/// lexical — it resolves no symlinks — so which file is named is otherwise unchanged.
fn open_literal(path: &Path, access: OpenFlags) -> Result<Connection, StoreOpenError> {
    ensure_vec_extension();
    let literal = std::path::absolute(path)?;
    Ok(Connection::open_with_flags(
        literal,
        access | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// The path a store is built at before it is renamed into place: the final path with a distinct
/// suffix carrying the building process's id, so two concurrent builds never contend for one name
/// and a leftover from an interrupted build never sits at the store path itself.
fn temp_store_path(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".c10r-tmp-{}", std::process::id()));
    PathBuf::from(temp)
}

/// An error opening a graph store.
///
/// The ownership guard reads the file header before any connection opens; the schema-version guard
/// then reads `PRAGMA user_version` before any table access — the pragma is readable regardless of
/// table shapes, which is exactly why it is the guard mechanism (the in-row `schema_version` column
/// remains as provenance only).
#[derive(Debug, thiserror::Error)]
pub enum StoreOpenError {
    /// A file sits at the store path that this binary did not create, so no operation may touch it.
    ///
    /// The two recoveries are stated separately and conditionally: a refused file may be data the
    /// user must not delete, so the message never instructs deletion unconditionally.
    #[error(
        "{path} is not a c10r index store. \
         If this is an old c10r index, remove it and run `c10r build`; otherwise point `--db` elsewhere."
    )]
    UnrecognizedStore {
        /// The refused file's path, for the teaching message.
        path: String,
    },
    /// A path at the store location whose contents could not be examined at all, so ownership was
    /// never established either way.
    ///
    /// Distinct from an unrecognized file in what it claims, not in what it does: it refuses in the
    /// same category, but it asserts neither that the target is nor that it is not this binary's
    /// store. Built by [`unreadable_store`], which supplies the cause and any hint.
    #[error("cannot read {path}: {detail}")]
    UnreadableStore {
        /// The unexaminable path, for the teaching message.
        path: String,
        /// The operating system's account of the failure, plus a hint where its words do not imply
        /// the fix.
        detail: String,
    },
    /// No file exists at the store path, and the operation asked for does not create one.
    #[error("no index store at {path}; run `c10r build` to create one")]
    MissingStore {
        /// The empty path, for the teaching message.
        path: String,
    },
    /// A file already occupies the path a new store is built at.
    ///
    /// The exclusive create exists because ownership of that path is unproven — a build file's name
    /// embeds a process id, and process ids recycle — so the message conditions removal on the file
    /// being this binary's own leftover rather than asserting that it is.
    #[error(
        "build file {path} already exists. \
         If this is a leftover from an interrupted c10r build, removing it is safe; \
         otherwise point `--db` elsewhere."
    )]
    StaleBuildFile {
        /// The occupying file's path, for the teaching message.
        path: String,
    },
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
    /// A filesystem operation on the store's own files — recognizing, creating, renaming, or
    /// removing them — failed.
    #[error("index store file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The SQLite-backed graph store.
pub struct GraphStore {
    conn: Connection,
}

impl GraphStore {
    /// Open an existing store at `path` for reading, creating nothing.
    ///
    /// Two guards run in order, both before anything is read from the store: the file must be
    /// recognizable as this binary's own creation (an unrecognized file refuses untouched, one that
    /// cannot be examined refuses naming why, an absent one refuses naming the build that would create
    /// it), and its `PRAGMA user_version` stamp must
    /// match [`SCHEMA_VERSION`] — a mismatch refuses with a typed teaching error rather than failing
    /// mid-operation on a changed table shape. The connection itself is opened read-only, so a read
    /// path cannot write to the store even by accident.
    pub fn open(path: &Path) -> Result<Self, StoreOpenError> {
        match recognize_or_refuse(path)? {
            StoreRecognition::Absent => {
                return Err(StoreOpenError::MissingStore {
                    path: path.display().to_string(),
                });
            }
            StoreRecognition::Unrecognized => {
                return Err(StoreOpenError::UnrecognizedStore {
                    path: path.display().to_string(),
                });
            }
            StoreRecognition::Recognized => {}
        }
        let conn = open_literal(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Self::guard_version(&conn, path)?;
        Ok(Self { conn })
    }

    /// Open a store at `path` for a build: create one if none exists, replace a recognized store
    /// whose schema version differs, and refuse a file this binary did not create.
    ///
    /// The index is derived, replayable data, so rebuild is the migration for a store of this
    /// binary's own: an incompatible one is deleted and recreated at the current version, and the
    /// build proceeds. Replacement reaches only recognized stores — an unrecognized file is refused
    /// with the ownership error, never deleted.
    pub fn open_or_replace(path: &Path) -> Result<Self, StoreOpenError> {
        match recognize_or_refuse(path)? {
            StoreRecognition::Absent => Self::create(path),
            StoreRecognition::Unrecognized => Err(StoreOpenError::UnrecognizedStore {
                path: path.display().to_string(),
            }),
            StoreRecognition::Recognized => {
                let conn = open_literal(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
                if stamped_version(&conn)? == SCHEMA_VERSION {
                    return Ok(Self { conn });
                }
                // Close the connection before the files go, so no handle outlives them.
                drop(conn);
                remove_store_files(path)?;
                Self::create(path)
            }
        }
    }

    /// Create a fresh store at `path`, complete and stamped before it ever appears there.
    ///
    /// The store is built in a distinctly-named temporary file beside the final path — created
    /// exclusively, so a leftover from an interrupted build is never reused — and renamed into place
    /// once its schema and stamps are committed. The store path therefore only ever holds a
    /// complete, marked store or nothing, so a crash mid-create can never strand a file that the
    /// ownership guard would later refuse.
    fn create(path: &Path) -> Result<Self, StoreOpenError> {
        let temp = temp_store_path(path);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StoreOpenError::StaleBuildFile {
                    path: temp.display().to_string(),
                });
            }
            Err(e) => return Err(e.into()),
        }
        // Scoped so the connection closes — flushing the store whole — before the rename.
        {
            let conn = open_literal(&temp, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE)?;
            conn.execute_batch(SCHEMA_SQL)?;
            conn.pragma_update(None, "application_id", APPLICATION_ID)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        std::fs::rename(&temp, path)?;
        Ok(Self {
            conn: open_literal(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?,
        })
    }

    /// Open an in-memory store (for tests), applying the schema and both stamps.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        ensure_vec_extension();
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    /// Refuse a recognized store whose recorded schema version is not the one this binary writes.
    fn guard_version(conn: &Connection, path: &Path) -> Result<(), StoreOpenError> {
        let found = stamped_version(conn)?;
        if found != SCHEMA_VERSION {
            return Err(StoreOpenError::SchemaVersionMismatch {
                path: path.display().to_string(),
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(())
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
        self.conn.execute("DELETE FROM semantic_lexical", [])?;
        self.conn.execute("DELETE FROM semantic_vectors", [])?;
        self.conn.execute("DELETE FROM semantic_corpus", [])?;
        self.conn.execute("DELETE FROM symbols", [])?;
        Ok(())
    }

    /// Replace the index metadata row with the given build's metadata.
    ///
    /// The semantic-index identity columns are written from the binary's own constants
    /// ([`crate::graph::embed::MODEL_ID`], [`crate::graph::corpus::CORPUS_DEFINITION_VERSION`]):
    /// the model is compiled into the binary, so the identity in effect at build time is exactly
    /// the binary's identity — no caller can supply a different one.
    pub fn write_metadata(&self, meta: &IndexMetadata) -> rusqlite::Result<()> {
        // Declared environment facts persist as JSON text; a backend that declares none writes NULL.
        let environment = meta
            .environment
            .as_ref()
            .map(|facts| serde_json::to_string(facts).expect("environment facts serialize"));
        self.conn.execute(
            "INSERT OR REPLACE INTO index_metadata
                (id, schema_version, workspace_id, workspace_root, analyzer_name, analyzer_version, environment,
                 content_hash,
                 aligned_exact_count, aligned_crate_root_count, aligned_operator_desugar_count,
                 aligned_module_span_count, aligned_self_keyword_count, aligned_module_name_count,
                 aligned_self_name_count, aligned_module_marker_count, aligned_import_alias_count,
                 aligned_range_literal_count, aligned_use_list_self_count, aligned_super_keyword_count,
                 text_mismatch_count, semantic_only_count, duplicate_ambiguous_count, syntax_only_count,
                 semantic_model_identity, corpus_definition_version, chunk_size, chunk_overlap)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                     ?22, ?23, ?24, ?25, ?26, ?27)",
            params![
                SCHEMA_VERSION,
                meta.workspace_id.as_str(),
                meta.workspace_root,
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
                meta.accounting.aligned_self_name as i64,
                meta.accounting.aligned_module_marker as i64,
                meta.accounting.aligned_import_alias as i64,
                meta.accounting.aligned_range_literal as i64,
                meta.accounting.aligned_use_list_self as i64,
                meta.accounting.aligned_super_keyword as i64,
                meta.accounting.text_mismatch as i64,
                meta.accounting.semantic_only as i64,
                meta.accounting.duplicate_ambiguous as i64,
                meta.accounting.syntax_only as i64,
                crate::graph::embed::MODEL_ID,
                crate::graph::corpus::CORPUS_DEFINITION_VERSION as i64,
                meta.chunk_params.chunk_size as i64,
                meta.chunk_params.overlap as i64,
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
                        aligned_self_name_count, aligned_module_marker_count, aligned_import_alias_count,
                        aligned_range_literal_count, aligned_use_list_self_count, aligned_super_keyword_count,
                        text_mismatch_count, semantic_only_count, duplicate_ambiguous_count, syntax_only_count,
                        workspace_root, chunk_size, chunk_overlap
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
                        workspace_root: r.get(21)?,
                        chunk_params: ChunkParams {
                            chunk_size: r.get::<_, i64>(22)? as usize,
                            overlap: r.get::<_, i64>(23)? as usize,
                        },
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
                            aligned_self_name: r.get::<_, i64>(11)? as u64,
                            aligned_module_marker: r.get::<_, i64>(12)? as u64,
                            aligned_import_alias: r.get::<_, i64>(13)? as u64,
                            aligned_range_literal: r.get::<_, i64>(14)? as u64,
                            aligned_use_list_self: r.get::<_, i64>(15)? as u64,
                            aligned_super_keyword: r.get::<_, i64>(16)? as u64,
                            text_mismatch: r.get::<_, i64>(17)? as u64,
                            semantic_only: r.get::<_, i64>(18)? as u64,
                            duplicate_ambiguous: r.get::<_, i64>(19)? as u64,
                            syntax_only: r.get::<_, i64>(20)? as u64,
                        },
                    })
                },
            )
            .optional()
    }

    /// The semantic-index identity recorded with the persisted build, if one exists.
    pub fn semantic_index_identity(&self) -> rusqlite::Result<Option<SemanticIndexIdentity>> {
        self.conn
            .query_row(
                "SELECT semantic_model_identity, corpus_definition_version, chunk_size, chunk_overlap
                 FROM index_metadata WHERE id = 1",
                [],
                |r| {
                    Ok(SemanticIndexIdentity {
                        model_identity: r.get(0)?,
                        corpus_definition_version: r.get::<_, i64>(1)? as u32,
                        chunk_params: ChunkParams {
                            chunk_size: r.get::<_, i64>(2)? as usize,
                            overlap: r.get::<_, i64>(3)? as usize,
                        },
                    })
                },
            )
            .optional()
    }

    /// The persisted semantic representations, keyed by symbol identity, each passage's chunk
    /// embeddings in ordinal order.
    ///
    /// The build reads this before [`Self::clear_derived`] so a passage whose render is unchanged
    /// carries its embeddings forward instead of re-embedding; tests read it to observe what a
    /// build persisted.
    pub fn semantic_representations(
        &self,
    ) -> rusqlite::Result<std::collections::HashMap<String, SemanticRepresentation>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.symbol_id, c.render, v.embedding
             FROM semantic_corpus c JOIN semantic_vectors v ON v.passage_id = c.id
             ORDER BY c.symbol_id, v.ordinal",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?))
        })?;
        let mut out: std::collections::HashMap<String, SemanticRepresentation> = std::collections::HashMap::new();
        for row in rows {
            let (symbol, render, embedding) = row?;
            out.entry(symbol)
                .or_insert_with(|| SemanticRepresentation {
                    render,
                    embeddings: Vec::new(),
                })
                .embeddings
                .push(embedding);
        }
        Ok(out)
    }

    /// Insert one passage with both its representations: the render row and the lexical row over
    /// its identifier-split words sharing one rowid, and one vector row per chunk, each carrying
    /// the passage's rowid and its ordinal within the passage.
    pub fn insert_passage(
        &self,
        symbol_id: &CanonicalId,
        render: &str,
        words: &str,
        chunk_embeddings: &[Vec<u8>],
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO semantic_corpus (symbol_id, render) VALUES (?1, ?2)",
            params![symbol_id.as_str(), render],
        )?;
        let rowid = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO semantic_lexical (rowid, words) VALUES (?1, ?2)",
            params![rowid, words],
        )?;
        for (ordinal, embedding) in chunk_embeddings.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO semantic_vectors (embedding, passage_id, ordinal) VALUES (?1, ?2, ?3)",
                params![embedding, rowid, ordinal as i64],
            )?;
        }
        Ok(())
    }

    /// The persisted embeddings of one passage's chunks, in ordinal order. Empty when the symbol
    /// contributes no passage.
    pub fn chunk_vectors_of(&self, id: &CanonicalId) -> rusqlite::Result<Vec<Vec<u8>>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.embedding FROM semantic_corpus c JOIN semantic_vectors v ON v.passage_id = c.id
             WHERE c.symbol_id = ?1
             ORDER BY v.ordinal",
        )?;
        let rows = stmt.query_map(params![id.as_str()], |r| r.get(0))?;
        rows.collect()
    }

    /// The persisted render of one passage — the text the lexical signal of a `similar` query
    /// derives its word set from. `None` when the symbol contributes no passage.
    pub fn semantic_render_of(&self, id: &CanonicalId) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT render FROM semantic_corpus WHERE symbol_id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()
    }

    /// The number of passages in the persisted build.
    pub fn corpus_size(&self) -> rusqlite::Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_corpus", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
    }

    /// The number of chunk vectors in the persisted build — the dense candidate pool's full width.
    pub fn chunk_count(&self) -> rusqlite::Result<u64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_vectors", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
    }

    /// The vector signal: the nearest passages to `query` (little-endian `f32` bytes), as
    /// `(symbol, distance)` ordered nearest first, distance ties broken by canonical identity.
    ///
    /// The KNN pool is requested in chunks — `k` chunk rows — and deduplicated to symbols by best
    /// chunk before returning, so a symbol scores as its most relevant part and gains nothing from
    /// the number of chunks representing it.
    ///
    /// The embeddings are L2-normalized, so the L2 ordering is the cosine ordering. `k` is clamped
    /// to the extension's KNN ceiling: on a corpus larger than the ceiling the signal ranks the
    /// nearest [`KNN_MAX_K`] candidates — an honest candidate pool under the
    /// candidates-not-completeness framing, never an error.
    pub fn vector_neighbors(&self, query: &[u8], k: usize) -> rusqlite::Result<Vec<(CanonicalId, f64)>> {
        let k = k.min(KNN_MAX_K);
        // The KNN scan materializes first: sqlite-vec refuses any constraint on an auxiliary
        // column inside a KNN query, and a plain join would push `passage_id` down into it.
        let mut stmt = self.conn.prepare(
            "WITH knn AS MATERIALIZED (
                 SELECT passage_id, distance FROM semantic_vectors WHERE embedding MATCH ?1 AND k = ?2
             )
             SELECT c.symbol_id, knn.distance
             FROM knn JOIN semantic_corpus c ON c.id = knn.passage_id",
        )?;
        let rows = stmt.query_map(params![query, k as i64], |r| {
            Ok((CanonicalId::from_raw(r.get::<_, String>(0)?), r.get::<_, f64>(1)?))
        })?;
        let mut best: std::collections::HashMap<CanonicalId, f64> = std::collections::HashMap::new();
        for row in rows {
            let (symbol, distance) = row?;
            let entry = best.entry(symbol).or_insert(distance);
            if distance < *entry {
                *entry = distance;
            }
        }
        let mut out: Vec<(CanonicalId, f64)> = best.into_iter().collect();
        out.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
        Ok(out)
    }

    /// The lexical signal: the passages matching an FTS5 `match_expr` over the identifier-split
    /// render words, as `(symbol, rank)` ordered best first (FTS5 `rank` ascends from best), rank
    /// ties broken by canonical identity.
    pub fn lexical_neighbors(&self, match_expr: &str, k: usize) -> rusqlite::Result<Vec<(CanonicalId, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.symbol_id, l.rank
             FROM semantic_lexical l JOIN semantic_corpus c ON c.id = l.rowid
             WHERE l.words MATCH ?1
             ORDER BY l.rank LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![match_expr, k as i64], |r| {
            Ok((CanonicalId::from_raw(r.get::<_, String>(0)?), r.get::<_, f64>(1)?))
        })?;
        let mut out: Vec<(CanonicalId, f64)> = rows.collect::<rusqlite::Result<_>>()?;
        out.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
        Ok(out)
    }

    /// Record a leaf symbol's clone-equivalence keys.
    pub fn set_clone_keys(
        &self,
        id: &CanonicalId,
        formatting_key: &str,
        substitution_key: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE symbols SET clone_formatting_key = ?2, clone_substitution_key = ?3 WHERE canonical_id = ?1",
            params![id.as_str(), formatting_key, substitution_key],
        )?;
        Ok(())
    }

    /// The clone-equivalence keys persisted for a symbol: `(formatting, substitution)`, or `None`
    /// when the symbol carries no keys (a container, an external, or an unknown identity).
    pub fn clone_keys_of(&self, id: &CanonicalId) -> rusqlite::Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT clone_formatting_key, clone_substitution_key FROM symbols WHERE canonical_id = ?1",
                params![id.as_str()],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map(|row| match row {
                Some((Some(f), Some(s))) => Some((f, s)),
                _ => None,
            })
    }

    /// Insert a symbol row.
    pub fn insert_symbol(&self, row: &SymbolRow) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO symbols
                (canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                 signature_text, interface_text, duplicated, test_rule)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                row.canonical_id.as_str(),
                row.display_name,
                row.kind,
                row.class.tag(),
                row.document_path,
                row.span.map(|s| s.0 as i64),
                row.span.map(|s| s.1 as i64),
                row.span_text,
                row.signature_text,
                row.interface_text,
                row.duplicated as i64,
                row.test_rule,
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
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                        signature_text, interface_text, duplicated, test_rule
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
            signature_text: r.get(8)?,
            interface_text: r.get(9)?,
            duplicated: r.get::<_, i64>(10)? != 0,
            test_rule: r.get(11)?,
        })
    }

    /// All symbols whose display name equals `shortname`.
    pub fn symbols_by_shortname(&self, shortname: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                    signature_text, interface_text, duplicated, test_rule
             FROM symbols WHERE display_name = ?1 ORDER BY canonical_id",
        )?;
        let rows = stmt.query_map(params![shortname], Self::map_symbol)?;
        rows.collect()
    }

    /// All symbols whose `display_name` contains `fragment`, matched case-insensitively (SQLite's
    /// `LIKE` case-folds ASCII by default) and ordered by canonical identity. `LIKE` metacharacters in
    /// `fragment` are escaped, so a literal `%`/`_` in the search text matches literally rather than
    /// as a wildcard.
    pub fn symbols_by_fragment(&self, fragment: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                    signature_text, interface_text, duplicated, test_rule
             FROM symbols WHERE display_name LIKE ?1 ESCAPE '\\'
             ORDER BY canonical_id",
        )?;
        let like = format!("%{}%", escape_like(fragment));
        let rows = stmt.query_map(params![like], Self::map_symbol)?;
        rows.collect()
    }

    /// All symbols whose canonical identity ends with the qualified-name suffix `::<qualified>`, or
    /// equals it exactly. Used to resolve a qualified name that omits the workspace prefix.
    pub fn symbols_by_qualified_suffix(&self, qualified: &str) -> rusqlite::Result<Vec<SymbolRow>> {
        let suffix = format!("::{qualified}");
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                    signature_text, interface_text, duplicated, test_rule
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
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                    signature_text, interface_text, duplicated, test_rule
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
                "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                        signature_text, interface_text, duplicated, test_rule
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

    /// The symbols whose definition spans overlap the half-open byte range `[start, end)` in
    /// `document`, ordered by `(span_start, span_end, canonical_id)` so a seed set is reproducible.
    ///
    /// Additive beside [`GraphStore::symbol_enclosing_position`], which resolves a single point: a
    /// diff hunk is a byte range, and a range can straddle several declarations, so every overlapping
    /// symbol is returned rather than only the tightest one. Selecting *which* of the overlapping
    /// declarations seed an impact assessment is a policy the query layer applies on top of this raw
    /// overlap.
    pub fn symbols_overlapping_span(
        &self,
        document: &str,
        start: usize,
        end: usize,
    ) -> rusqlite::Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT canonical_id, display_name, kind, class, document_path, span_start, span_end, span_text,
                    signature_text, interface_text, duplicated, test_rule
             FROM symbols
             WHERE document_path = ?1 AND span_start IS NOT NULL AND span_start < ?3 AND ?2 < span_end
             ORDER BY span_start, span_end, canonical_id",
        )?;
        let rows = stmt.query_map(params![document, start as i64, end as i64], Self::map_symbol)?;
        rows.collect()
    }

    /// Whether the index holds any symbol defined in `document` — the test that separates a changed
    /// region the graph simply tracks nothing in from one in a document the index never saw at all.
    pub fn holds_document(&self, document: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM symbols WHERE document_path = ?1)",
            params![document],
            |r| r.get(0),
        )
    }

    /// The module-kind symbol persisted for `document` — the symbol a reference site attributes to
    /// when it carries no narrower enclosing declaration (`enclosing_id` is `None`).
    ///
    /// When several module rows share the document, the module whose own definition occurrence in
    /// that document is widest wins: a file module's definition occurrence spans the whole document,
    /// while an inline `mod` block's or a re-export-defined module's definition sits on a name token
    /// — so the file module is selected even when persisted spans tie (a re-export-defined module
    /// persists the same whole-document span as the file module it is declared in). Ties beyond that
    /// fall to the wider persisted span, then canonical identity, keeping the answer deterministic.
    pub fn module_of_document(&self, document: &str) -> rusqlite::Result<Option<SymbolRow>> {
        self.conn
            .query_row(
                "SELECT s.canonical_id, s.display_name, s.kind, s.class, s.document_path, s.span_start, s.span_end,
                        s.span_text, s.signature_text, s.interface_text, s.duplicated, s.test_rule
                 FROM symbols s
                 LEFT JOIN occurrences o
                   ON o.symbol_id = s.canonical_id AND o.document_path = ?1 AND o.role = 'definition'
                 WHERE s.document_path = ?1 AND s.kind = 'module'
                 ORDER BY COALESCE(o.span_end - o.span_start, -1) DESC,
                          (s.span_end - s.span_start) DESC,
                          s.canonical_id ASC
                 LIMIT 1",
                params![document],
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

    /// The symbols whose edge of `kind` targets `id` — the reverse, source-by-destination walk used
    /// by the `importers` (`imports`) and `implementers` (`type_hierarchy`) relations.
    pub fn edge_sources(&self, kind: EdgeKind, id: &CanonicalId) -> rusqlite::Result<Vec<CanonicalId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src_id FROM edges WHERE kind = ?1 AND dst_id = ?2 ORDER BY src_id")?;
        let rows = stmt.query_map(params![kind.tag(), id.as_str()], |r| {
            Ok(CanonicalId::from_raw(r.get::<_, String>(0)?))
        })?;
        rows.collect()
    }

    /// Compute the seed's dependents: symbols whose dependency edges (`uses`, `imports`,
    /// `type_hierarchy`) reach the seed directly or transitively, walking each edge `dst → src`.
    ///
    /// The one-element case of [`dependents_of_seeds`](Self::dependents_of_seeds); see there for the
    /// full contract.
    pub fn dependents(&self, seed: &CanonicalId, horizon: u32) -> rusqlite::Result<Vec<DependentRow>> {
        self.dependents_of_seeds(std::slice::from_ref(seed), horizon)
    }

    /// Compute the combined dependents of several seeds at once: the symbols whose dependency edges
    /// (`uses`, `imports`, `type_hierarchy`) reach any seed directly or transitively, walking each
    /// edge `dst → src` breadth-first from the whole seed set.
    ///
    /// Enclosure (`contains`) never propagates dependence — it supplies attribution, not impact — so
    /// it is excluded from the walk. Each dependent is returned exactly once at its shortest hop
    /// distance from any seed, carrying the connecting edge kind chosen among the edges arriving at
    /// that distance under a fixed tie-break: kind order (`uses` < `imports` < `type_hierarchy`),
    /// then canonical identity. A seed never appears as its own dependent — a seed is reported only
    /// at its shortest distance to a seed *other than itself*, so one that depends on another seed
    /// is a real dependent while one reached only through its own cycle is not. The walk ends when
    /// the frontier empties (a closed reachable set never pays for the remaining bound) or at
    /// `horizon` hops; the horizon is a hard cap on hop distance, so a zero horizon reports
    /// nothing. Results are ordered by `(depth, kind order, identity)`.
    ///
    /// Each round expands the whole frontier level-at-a-time — one `IN`-list query over the
    /// destination-keyed edge index, batched at [`DEPENDENTS_FRONTIER_BATCH`] bound parameters and
    /// merged before filtering, so a hub symbol's frontier cannot fail the query outright. Every
    /// symbol is expanded at most twice across the whole walk (once per distinct walk root it
    /// carries, see below) rather than once per depth or once per seed.
    pub fn dependents_of_seeds(&self, seeds: &[CanonicalId], horizon: u32) -> rusqlite::Result<Vec<DependentRow>> {
        use std::collections::{BTreeSet, HashMap};

        if seeds.is_empty() {
            return Ok(Vec::new());
        }

        // Each node carries up to two *distinct* walk roots (seed indices). One root suffices for
        // the union's shortest distance — breadth-first order reaches every node first along a
        // shortest path — but seed self-exclusion needs a node's shortest distance to a root *other
        // than* one excluded seed, and for any single excluded root at least one of two distinct
        // roots differs from it. A node re-enters the frontier when it gains a root, so it is
        // expanded at most twice.
        let seed_root: HashMap<&str, u32> = {
            let mut m = HashMap::new();
            for (i, s) in seeds.iter().enumerate() {
                m.entry(s.as_str()).or_insert(i as u32);
            }
            m
        };
        let mut roots_of: HashMap<String, RootPair> = HashMap::new();
        let mut reported: HashMap<String, (u32, String)> = HashMap::new();
        // The frontier: nodes that gained roots last round, with exactly the roots they gained.
        let mut frontier: Vec<(String, RootPair)> = Vec::with_capacity(seeds.len());
        for (id, root) in &seed_root {
            let mut pair = RootPair::default();
            pair.insert(*root);
            roots_of.insert((*id).to_string(), pair);
            frontier.push(((*id).to_string(), pair));
        }

        for depth in 1..=horizon {
            if frontier.is_empty() {
                break;
            }
            let new_roots: HashMap<&str, RootPair> = frontier.iter().map(|(n, p)| (n.as_str(), *p)).collect();

            // One level-at-a-time query per batch: every dependency edge into the frontier.
            let ids: Vec<&str> = frontier.iter().map(|(n, _)| n.as_str()).collect();
            let mut hops: Vec<(String, String, String)> = Vec::new();
            for batch in ids.chunks(DEPENDENTS_FRONTIER_BATCH) {
                let placeholders = vec!["?"; batch.len()].join(", ");
                let sql = format!(
                    "SELECT src_id, kind, dst_id FROM edges \
                     WHERE kind IN ('uses', 'imports', 'type_hierarchy') AND dst_id IN ({placeholders})"
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(batch.iter().copied()), |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
                for row in rows {
                    hops.push(row?);
                }
            }

            let mut by_src: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
            for (src, kind, dst) in &hops {
                by_src
                    .entry(src.as_str())
                    .or_default()
                    .push((kind.as_str(), dst.as_str()));
            }

            let mut next: Vec<(String, RootPair)> = Vec::new();
            for (src, edges_in) in by_src {
                let existing = roots_of.get(src).copied().unwrap_or_default();
                if existing.full() {
                    continue;
                }
                // The roots newly reaching `src`: every root a frontier destination gained last
                // round that `src` does not already carry. Sorted so root selection is
                // deterministic; a self-loop contributes nothing because the node already carries
                // every root its own frontier entry gained.
                let mut cands: BTreeSet<u32> = BTreeSet::new();
                for (_, dst) in &edges_in {
                    let gained = new_roots.get(dst).expect("destination came from the frontier");
                    for root in gained.iter() {
                        if !existing.contains(root) {
                            cands.insert(root);
                        }
                    }
                }
                if cands.is_empty() {
                    continue;
                }

                if existing.is_empty() {
                    // First reach of an ordinary node: this round is its shortest distance, and the
                    // connecting kind is the lowest-ordered among the edges arriving now.
                    let kind = edges_in
                        .iter()
                        .map(|(k, _)| *k)
                        .min_by_key(|k| kind_order(k))
                        .expect("a grouped source has at least one edge");
                    reported.insert(src.to_string(), (depth, kind.to_string()));
                } else if seed_root.contains_key(src) && !reported.contains_key(src) {
                    // A seed reached by another walk root: its shortest distance to a seed other
                    // than itself, so it is reported as that seed's dependent. Only the edges
                    // arriving with a foreign root qualify for the kind tie-break; an unreported
                    // seed carries exactly its own root, so any candidate root is foreign.
                    let kind = edges_in
                        .iter()
                        .filter(|(_, dst)| {
                            let gained = new_roots.get(dst).expect("destination came from the frontier");
                            gained.iter().any(|root| !existing.contains(root))
                        })
                        .map(|(k, _)| *k)
                        .min_by_key(|k| kind_order(k))
                        .expect("a candidate root arrived on some edge");
                    reported.insert(src.to_string(), (depth, kind.to_string()));
                }

                let mut pair = existing;
                let mut added = RootPair::default();
                for root in cands {
                    if pair.full() {
                        break;
                    }
                    pair.insert(root);
                    added.insert(root);
                }
                roots_of.insert(src.to_string(), pair);
                next.push((src.to_string(), added));
            }
            frontier = next;
        }

        let mut out: Vec<DependentRow> = reported
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

    /// The rank graph projection: every in-workspace symbol, and every dependency edge (`uses`,
    /// `imports`, `type_hierarchy`) whose both endpoints are in-workspace, with a pair related under
    /// several kinds collapsed to a single edge.
    ///
    /// Nodes are returned in canonical-identity order and edges in `(src, dst)` canonical-identity
    /// order, so a computation iterating them visits in a fixed order — the property the ranking's
    /// determinism rests on. External symbols are excluded outright (they never appear as
    /// dependents, and their mass would distort the scores of symbols that do); `contains` is
    /// excluded because enclosure is structure, not dependency. An in-workspace symbol with no
    /// projected edge is still a node, so it participates in the rank universe.
    pub fn rank_projection(&self) -> rusqlite::Result<RankProjection> {
        // Both reads run inside one transaction so they share a single snapshot: a rebuild
        // committing between them could otherwise surface an edge whose endpoints the node query
        // never saw, and the endpoint lookups below would abort on a torn view rather than a real
        // invariant violation. Read-only, so the rollback on drop is a no-op.
        let snapshot = self.conn.unchecked_transaction()?;
        let mut stmt = self
            .conn
            .prepare("SELECT canonical_id FROM symbols WHERE class = 'in_workspace' ORDER BY canonical_id")?;
        let nodes: Vec<CanonicalId> = stmt
            .query_map([], |r| Ok(CanonicalId::from_raw(r.get::<_, String>(0)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let index_of: std::collections::HashMap<&str, usize> =
            nodes.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT e.src_id, e.dst_id FROM edges e
             JOIN symbols s ON s.canonical_id = e.src_id AND s.class = 'in_workspace'
             JOIN symbols d ON d.canonical_id = e.dst_id AND d.class = 'in_workspace'
             WHERE e.kind IN ('uses', 'imports', 'type_hierarchy')
             ORDER BY e.src_id, e.dst_id",
        )?;
        let edges: Vec<(usize, usize)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .map(|row| {
                let (src, dst) = row?;
                Ok((
                    *index_of.get(src.as_str()).expect("edge source is a projected node"),
                    *index_of
                        .get(dst.as_str())
                        .expect("edge destination is a projected node"),
                ))
            })
            .collect::<rusqlite::Result<_>>()?;
        drop(snapshot);

        Ok(RankProjection { nodes, edges })
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
        Ok(self
            .read_metadata()?
            .map(|meta| freshness_of(&meta, current_hash, current, current_environment)))
    }
}

/// Evaluate one metadata value's freshness against the sources' current content hash, the analyzer
/// in effect, and the declared environment in effect.
///
/// Pure over the metadata it is handed, so a caller that already read the metadata — the build's
/// currency check — compares every field from that one read rather than racing a second read
/// against a concurrent build.
pub fn freshness_of(
    meta: &IndexMetadata,
    current_hash: &str,
    current: &AnalyzerProvenance,
    current_environment: Option<&EnvironmentFacts>,
) -> Freshness {
    // Content change takes precedence in reporting; version change flags reindex.
    if meta.content_hash != current_hash {
        return Freshness::StaleContent;
    }
    if meta.provenance.analyzer_version != current.analyzer_version
        || meta.provenance.analyzer_name != current.analyzer_name
    {
        return Freshness::StaleVersion;
    }
    // The whole declared environment must match, absence included: a recorded environment that
    // can no longer be resolved is drift, never reported fresh.
    if meta.environment.as_ref() != current_environment {
        return Freshness::StaleEnvironment;
    }
    Freshness::Fresh
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

/// Up to two distinct walk roots (seed indices) a traversal node carries, in arrival order.
///
/// Two is exactly enough for seed self-exclusion in [`GraphStore::dependents_of_seeds`]: for any
/// single excluded root, at least one of two distinct roots differs from it, so a node's earliest
/// two roots always answer "how far to a seed other than X" for every X.
#[derive(Clone, Copy, Default)]
struct RootPair(Option<u32>, Option<u32>);

impl RootPair {
    fn is_empty(self) -> bool {
        self.0.is_none()
    }

    fn full(self) -> bool {
        self.1.is_some()
    }

    fn contains(self, root: u32) -> bool {
        self.0 == Some(root) || self.1 == Some(root)
    }

    /// Record `root` in the first free slot; a root already carried or beyond the second slot is
    /// dropped.
    fn insert(&mut self, root: u32) {
        if self.contains(root) {
            return;
        }
        if self.0.is_none() {
            self.0 = Some(root);
        } else if self.1.is_none() {
            self.1 = Some(root);
        }
    }

    fn iter(self) -> impl Iterator<Item = u32> {
        [self.0, self.1].into_iter().flatten()
    }
}

/// The fixed order dependency edge kinds break ties by, so equal-depth hops choose a connecting kind
/// deterministically and results order reproducibly. Non-dependency tags sort last.
///
/// The fallback bucket is unreachable while `EdgeKind` stays closed to the three dependency kinds
/// above; adding a new edge kind to the dependents walk requires adding it here too, or it will
/// silently sort last instead of taking its intended tie-break position.
pub(crate) fn kind_order(tag: &str) -> u8 {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal a store-opening call is expected to produce. A store handle carries a live
    /// connection and so is not printable, which is why this stands in for `expect_err`.
    fn refused(result: Result<GraphStore, StoreOpenError>, expectation: &str) -> StoreOpenError {
        match result {
            Ok(_) => panic!("{expectation}"),
            Err(error) => error,
        }
    }

    /// Write a database at `path` carrying `application_id` and `user_version` as given.
    fn write_database(path: &Path, application_id: Option<i32>, user_version: i64) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
        if let Some(id) = application_id {
            conn.pragma_update(None, "application_id", id).unwrap();
        }
        conn.pragma_update(None, "user_version", user_version).unwrap();
    }

    // The recognizer's verdict for every shape a store path can hold: nothing, a file too small or
    // too foreign to be a database, another application's database (marked or not), and c10r's own.
    #[test]
    fn recognize_store_separates_absent_unrecognized_and_own() {
        let dir = tempfile::tempdir().unwrap();

        let absent = dir.path().join("absent.db");
        assert_eq!(recognize_store(&absent).unwrap(), StoreRecognition::Absent);

        let empty = dir.path().join("empty.db");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(recognize_store(&empty).unwrap(), StoreRecognition::Unrecognized);

        let text = dir.path().join("notes.txt");
        std::fs::write(&text, b"this is not a database").unwrap();
        assert_eq!(recognize_store(&text).unwrap(), StoreRecognition::Unrecognized);

        // Long enough to be plausible, short enough that the header never completes.
        let truncated = dir.path().join("truncated.db");
        std::fs::write(&truncated, &b"SQLite format 3\0"[..]).unwrap();
        assert_eq!(recognize_store(&truncated).unwrap(), StoreRecognition::Unrecognized);

        let unmarked = dir.path().join("unmarked.db");
        write_database(&unmarked, None, 12);
        assert_eq!(recognize_store(&unmarked).unwrap(), StoreRecognition::Unrecognized);

        let foreign = dir.path().join("foreign.db");
        write_database(&foreign, Some(0x0102_0304), 12);
        assert_eq!(recognize_store(&foreign).unwrap(), StoreRecognition::Unrecognized);

        let own = dir.path().join("own.db");
        write_database(&own, Some(APPLICATION_ID), SCHEMA_VERSION);
        assert_eq!(recognize_store(&own).unwrap(), StoreRecognition::Recognized);
    }

    // The ownership refusal states both recoveries conditionally and never instructs deletion on its
    // own: a refused file may be data the caller must not delete, so every mention of deleting sits
    // inside the branch that supposes the file is a stale c10r index.
    #[test]
    fn the_ownership_refusal_states_both_recoveries_and_deletes_nothing_unconditionally() {
        let message = StoreOpenError::UnrecognizedStore {
            path: "/w/.c10r/index.db".to_string(),
        }
        .to_string();

        assert!(message.contains("/w/.c10r/index.db"), "names the path: {message}");
        assert!(
            message.contains("c10r build"),
            "names the rebuild recovery for a stale index: {message}"
        );
        assert!(
            message.contains("--db"),
            "names the path-correction recovery for somebody else's file: {message}"
        );
        // Structural, not phrase-matched: wherever the conditional opens, every way of saying
        // "get rid of it" has to sit after it. Probing for one exact sentence would pass a reworded
        // message that moved the instruction out of the branch.
        let lowered = message.to_lowercase();
        let conditional = lowered.find("if this is").expect("the rebuild branch is conditional");
        for instruction in ["remove", "delete", "rm "] {
            assert!(
                lowered.find(instruction).is_none_or(|at| at > conditional),
                "`{instruction}` stands outside a conditional branch: {message}"
            );
        }
    }

    // Every refusal reaches the caller through the diagnostic sanitizer, which makes control
    // characters visible rather than executing them. A message carrying its own newline therefore
    // renders a replacement character where it meant a line break, so the refusals stay single-line by
    // construction — a property worth pinning, because the damage is invisible until someone reads
    // real terminal output.
    #[test]
    fn every_store_refusal_survives_the_diagnostic_sanitizer() {
        let refusals = [
            StoreOpenError::UnrecognizedStore {
                path: "/w/index.db".to_string(),
            },
            StoreOpenError::UnreadableStore {
                path: "/w/.c10r".to_string(),
                detail: "Is a directory (os error 21); `--db` wants the database file inside it".to_string(),
            },
            StoreOpenError::MissingStore {
                path: "/w/index.db".to_string(),
            },
            StoreOpenError::StaleBuildFile {
                path: "/w/index.db.c10r-tmp-1".to_string(),
            },
            StoreOpenError::SchemaVersionMismatch {
                path: "/w/index.db".to_string(),
                found: 12,
                expected: SCHEMA_VERSION,
            },
        ];
        for refusal in refusals {
            let message = refusal.to_string();
            assert_eq!(
                crate::render::sanitize(&message),
                message,
                "the message is altered on its way to the terminal: {message:?}"
            );
        }
    }

    // A path whose contents cannot be examined at all is refused in the ownership category without an
    // ownership claim. The read never happened, so the refusal names the path and why, and says
    // nothing about whose file it is — including through the `--db` hint, which addresses the mistype
    // rather than the file's provenance.
    #[cfg(unix)]
    #[test]
    fn an_unexaminable_path_is_refused_without_an_ownership_claim() {
        let dir = tempfile::tempdir().unwrap();
        // The realistic mistype: `--db` pointed at the directory holding the store, not the store.
        let db = dir.path().join(".c10r");
        std::fs::create_dir(&db).unwrap();

        for error in [
            refused(GraphStore::open(&db), "the read path refuses an unexaminable path"),
            refused(
                GraphStore::open_or_replace(&db),
                "the build path refuses an unexaminable path",
            ),
        ] {
            assert!(
                matches!(error, StoreOpenError::UnreadableStore { .. }),
                "the refusal is the unexaminable-path error, not a bare I/O failure: {error}"
            );
            let message = error.to_string();
            assert!(message.contains(&db.display().to_string()), "names the path: {message}");
            assert!(
                message.contains(&db.join("index.db").display().to_string()),
                "names the database file `--db` wants instead: {message}"
            );
            assert!(
                !message.contains("is not a c10r index store"),
                "no ownership verdict is claimed from a read that never happened: {message}"
            );
        }
        assert!(db.is_dir(), "the unexaminable path is left where it was");
    }

    // The same refusal for a file the process cannot read — and here the store is genuinely c10r's
    // own, which is the case that makes the rule matter: a store built by another user must never be
    // told it is somebody else's file.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_refused_without_denying_it_is_ours() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        write_database(&db, Some(APPLICATION_ID), SCHEMA_VERSION);
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads straight through the permission bit, so the guarantee is unobservable there.
        if std::fs::File::open(&db).is_ok() {
            return;
        }

        let error = refused(GraphStore::open(&db), "an unreadable file refuses");
        assert!(
            matches!(error, StoreOpenError::UnreadableStore { .. }),
            "the refusal is the unexaminable-path error: {error}"
        );
        let message = error.to_string();
        assert!(
            message.contains("owner"),
            "points at the file's owner and permissions: {message}"
        );
        assert!(
            !message.contains("is not a c10r index store"),
            "the file is c10r's own store; the refusal must not say otherwise: {message}"
        );
    }

    // A fresh create lands a store that is recognized, current, and readable — and leaves no build
    // leftover at either the store path or the temporary path it was built at.
    #[test]
    fn create_lands_a_recognized_current_store_and_no_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");

        let store = GraphStore::create(&db).unwrap();
        assert!(store.read_metadata().unwrap().is_none(), "a fresh store holds no build");

        assert_eq!(recognize_store(&db).unwrap(), StoreRecognition::Recognized);
        assert_eq!(stamped_version(&store.conn).unwrap(), SCHEMA_VERSION);
        let temp = temp_store_path(&db);
        assert_ne!(temp, db, "the store is built at a path distinct from its own");
        assert!(
            !temp.exists(),
            "the temporary build file is renamed away, not left behind"
        );
    }

    // A leftover build file from an interrupted build is never reused or overwritten: the create
    // refuses, naming it, and the leftover's contents are untouched.
    #[test]
    fn create_refuses_to_reuse_a_leftover_build_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        let temp = temp_store_path(&db);
        std::fs::write(&temp, b"leftover").unwrap();

        let error = refused(GraphStore::create(&db), "a leftover build file refuses the create");
        assert!(
            matches!(error, StoreOpenError::StaleBuildFile { .. }),
            "the refusal is typed: {error}"
        );
        assert_eq!(std::fs::read(&temp).unwrap(), b"leftover", "the leftover is untouched");
        assert!(!db.exists(), "nothing was created at the store path");

        // The exclusive create exists because ownership of that path is unproven, so the message
        // supposes a leftover rather than asserting one, and conditions removal on that supposition.
        let message = error.to_string();
        assert!(
            message.contains(&temp.display().to_string()),
            "names the occupying file: {message}"
        );
        let lowered = message.to_lowercase();
        let conditional = lowered.find("if this is").expect("the leftover branch is conditional");
        for claim in ["leftover", "removing", "remove", "delete", "rm "] {
            assert!(
                lowered.find(claim).is_none_or(|at| at > conditional),
                "`{claim}` stands outside the conditional branch: {message}"
            );
        }
    }

    // The read path creates nothing: an absent store refuses with the typed missing-store error and
    // no file appears at the path.
    #[test]
    fn open_refuses_an_absent_store_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");

        let error = refused(GraphStore::open(&db), "a read against nothing refuses");
        assert!(
            matches!(error, StoreOpenError::MissingStore { .. }),
            "the refusal is typed: {error}"
        );
        assert!(error.to_string().contains("c10r build"), "names the remedy: {error}");
        assert!(!db.exists(), "the read brought no store into being");
    }

    // A database c10r did not create is refused by the read path with nothing written into it — even
    // when its version stamp happens to equal the schema version this binary expects, the coincidence
    // the marker exists to survive.
    #[test]
    fn open_refuses_a_foreign_database_leaving_it_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        for (name, user_version) in [("other.db", 3), ("coincident.db", SCHEMA_VERSION)] {
            let db = dir.path().join(name);
            write_database(&db, None, user_version);
            let before = std::fs::read(&db).unwrap();

            let error = refused(GraphStore::open(&db), "a foreign database refuses");
            assert!(
                matches!(error, StoreOpenError::UnrecognizedStore { .. }),
                "{name} is refused for ownership, not version: {error}"
            );
            assert_eq!(std::fs::read(&db).unwrap(), before, "{name} is untouched");
            for suffix in ["-wal", "-shm"] {
                let sidecar = dir.path().join(format!("{name}{suffix}"));
                assert!(!sidecar.exists(), "no sidecar was created beside {name}");
            }
        }
    }

    // A store c10r created at a different schema version is its own to replace: the build path
    // replaces it and stamps the current version, while a foreign database at any version — including
    // one that coincidentally matches — is refused byte-identical instead.
    #[test]
    fn open_or_replace_replaces_its_own_store_and_refuses_foreign_ones() {
        let dir = tempfile::tempdir().unwrap();

        let own = dir.path().join("own.db");
        write_database(&own, Some(APPLICATION_ID), SCHEMA_VERSION - 1);
        let store = GraphStore::open_or_replace(&own).expect("c10r's own old store is replaced");
        assert_eq!(stamped_version(&store.conn).unwrap(), SCHEMA_VERSION);

        for (name, user_version) in [("foreign.db", 3), ("coincident.db", SCHEMA_VERSION)] {
            let db = dir.path().join(name);
            write_database(&db, None, user_version);
            let before = std::fs::read(&db).unwrap();

            let error = refused(GraphStore::open_or_replace(&db), "a foreign database refuses");
            assert!(
                matches!(error, StoreOpenError::UnrecognizedStore { .. }),
                "{name} is refused: {error}"
            );
            assert_eq!(std::fs::read(&db).unwrap(), before, "{name} survives byte-identical");
        }
    }

    // A file that is not a database at all is refused with the typed ownership error, never a raw
    // storage failure leaking the SQLite layer's own words.
    #[test]
    fn a_non_database_file_is_refused_with_the_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        std::fs::write(&db, b"just some text a caller kept here").unwrap();

        for error in [
            refused(GraphStore::open(&db), "the read path refuses"),
            refused(GraphStore::open_or_replace(&db), "the build path refuses"),
        ] {
            assert!(
                matches!(error, StoreOpenError::UnrecognizedStore { .. }),
                "typed ownership refusal, not a storage error: {error}"
            );
        }
        assert_eq!(std::fs::read(&db).unwrap(), b"just some text a caller kept here");
    }

    // A store built before ownership marking — a real c10r index by shape, carrying a version stamp
    // and no marker — is refused, and the refusal's rebuild branch names the recovery.
    #[test]
    fn an_unmarked_legacy_store_is_refused_with_rebuild_guidance() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION - 1).unwrap();
        drop(conn);

        let error = refused(GraphStore::open(&db), "an unmarked store refuses");
        assert!(
            matches!(error, StoreOpenError::UnrecognizedStore { .. }),
            "unmarked is unrecognized, whatever its version: {error}"
        );
        assert!(
            error.to_string().contains("c10r build"),
            "the rebuild branch names the recovery: {error}"
        );
    }

    // The recorded workspace root round-trips through the metadata a build writes.
    #[test]
    fn metadata_carries_the_recorded_workspace_root() {
        let store = GraphStore::open_in_memory().unwrap();
        store
            .write_metadata(&IndexMetadata {
                workspace_id: WorkspaceId::new("ws"),
                workspace_root: Some("/projects/ws".to_string()),
                provenance: AnalyzerProvenance {
                    analyzer_name: "test".to_string(),
                    analyzer_version: "0".to_string(),
                },
                content_hash: "hash".to_string(),
                accounting: JoinAccounting::default(),
                environment: None,
                chunk_params: Default::default(),
            })
            .unwrap();

        let read = store.read_metadata().unwrap().expect("metadata present");
        assert_eq!(read.workspace_root.as_deref(), Some("/projects/ws"));
    }

    // The chunks of one passage round-trip with their ordinals, and the corpus table still holds
    // exactly one row per symbol.
    #[test]
    fn chunk_vectors_round_trip_with_ordinals() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::long", "doc.rs", 0, 10)).unwrap();
        let id = CanonicalId::from_raw("ws::long".to_string());
        let vector = |seed: f32| -> Vec<u8> {
            let mut v = vec![0.0f32; 256];
            v[0] = seed;
            v.iter().flat_map(|x| x.to_le_bytes()).collect()
        };
        let chunks = vec![vector(1.0), vector(2.0), vector(3.0)];
        store.insert_passage(&id, "render", "words", &chunks).unwrap();

        assert_eq!(
            store.chunk_vectors_of(&id).unwrap(),
            chunks,
            "ordinal order round-trips"
        );
        assert_eq!(store.corpus_size().unwrap(), 1, "one corpus row per symbol");
        assert_eq!(store.chunk_count().unwrap(), 3, "one vector row per chunk");
    }

    // The recorded semantic-index identity carries the model identity, the corpus definition
    // version, and the chunk parameters; two stores built under different parameters carry
    // different identities.
    #[test]
    fn identity_carries_the_chunk_parameters() {
        let metadata_with = |params: ChunkParams| IndexMetadata {
            workspace_id: WorkspaceId::new("ws"),
            workspace_root: Some("/ws".to_string()),
            provenance: AnalyzerProvenance {
                analyzer_name: "test".to_string(),
                analyzer_version: "0".to_string(),
            },
            content_hash: "hash".to_string(),
            accounting: JoinAccounting::default(),
            environment: None,
            chunk_params: params,
        };
        let params = ChunkParams {
            chunk_size: 256,
            overlap: 32,
        };
        let store = GraphStore::open_in_memory().unwrap();
        store.write_metadata(&metadata_with(params)).unwrap();
        let identity = store.semantic_index_identity().unwrap().expect("identity recorded");
        assert_eq!(identity.model_identity, crate::graph::embed::MODEL_ID);
        assert_eq!(
            identity.corpus_definition_version,
            crate::graph::corpus::CORPUS_DEFINITION_VERSION
        );
        assert_eq!(identity.chunk_params, params, "the operator's values are recorded");

        let other = GraphStore::open_in_memory().unwrap();
        other.write_metadata(&metadata_with(ChunkParams::default())).unwrap();
        let other_identity = other.semantic_index_identity().unwrap().expect("identity recorded");
        assert_ne!(identity, other_identity, "different parameters are distinguishable");
    }

    /// An in-workspace symbol row with a definition span, minimal in every other field.
    fn symbol_at(id: &str, document: &str, start: usize, end: usize) -> SymbolRow {
        SymbolRow {
            canonical_id: CanonicalId::from_raw(id.to_string()),
            display_name: id.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some(document.to_string()),
            span: Some((start, end)),
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        }
    }

    // A range overlapping two symbols' spans returns both, ordered by span start.
    #[test]
    fn symbols_overlapping_span_returns_overlapping_symbols_in_span_order() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::b", "doc.rs", 20, 30)).unwrap();
        store.insert_symbol(&symbol_at("ws::a", "doc.rs", 0, 10)).unwrap();
        store.insert_symbol(&symbol_at("ws::c", "doc.rs", 40, 50)).unwrap();

        let rows = store.symbols_overlapping_span("doc.rs", 5, 25).unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.canonical_id.as_str()).collect();
        assert_eq!(ids, vec!["ws::a", "ws::b"]);
    }

    // A range touching no symbol's span returns none.
    #[test]
    fn symbols_overlapping_span_touching_neither_returns_none() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::a", "doc.rs", 0, 10)).unwrap();
        store.insert_symbol(&symbol_at("ws::b", "doc.rs", 20, 30)).unwrap();

        let rows = store.symbols_overlapping_span("doc.rs", 12, 18).unwrap();
        assert!(rows.is_empty(), "{rows:?}");
    }

    // A zero-width range in the gap between two symbols' spans overlaps neither — the query layer
    // never issues one (`LineIndex::byte_range` widens an insertion point before it reaches the
    // store), but the predicate itself must not fabricate an overlap for a point outside every span.
    #[test]
    fn symbols_overlapping_span_zero_width_range_returns_none() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::a", "doc.rs", 0, 10)).unwrap();
        store.insert_symbol(&symbol_at("ws::b", "doc.rs", 20, 30)).unwrap();

        let rows = store.symbols_overlapping_span("doc.rs", 15, 15).unwrap();
        assert!(rows.is_empty(), "{rows:?}");
    }

    // `holds_document` is true for a document holding a symbol and false for one that holds none.
    #[test]
    fn holds_document_true_only_when_a_symbol_is_defined_there() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::a", "doc.rs", 0, 10)).unwrap();

        assert!(store.holds_document("doc.rs").unwrap());
        assert!(!store.holds_document("other.rs").unwrap());
    }

    // A frontier wider than `DEPENDENTS_FRONTIER_BATCH` is batched across several queries within
    // the same round, with results merged before filtering, and still answers correctly: a
    // dependent reaching the frontier through every batch is reported once at its shortest
    // distance.
    #[test]
    fn a_frontier_wider_than_the_batch_limit_batches_within_the_round() {
        let n = DEPENDENTS_FRONTIER_BATCH + 200;
        let store = GraphStore::open_in_memory().unwrap();
        let seed = CanonicalId::from_raw("ws::seed".to_string());
        let outer = CanonicalId::from_raw("ws::outer".to_string());
        store.insert_symbol(&symbol_at("ws::seed", "doc.rs", 0, 10)).unwrap();
        store.insert_symbol(&symbol_at("ws::outer", "doc.rs", 20, 30)).unwrap();
        for i in 0..n {
            let id = format!("ws::w_{i}");
            store.insert_symbol(&symbol_at(&id, "doc.rs", 40 + i, 41 + i)).unwrap();
            let wide = CanonicalId::from_raw(id);
            store.insert_edge(EdgeKind::Uses, &wide, &seed).unwrap();
            store.insert_edge(EdgeKind::Uses, &outer, &wide).unwrap();
        }

        let deps = store.dependents(&seed, DEPENDENTS_HORIZON).unwrap();
        assert_eq!(deps.len(), n + 1, "all direct dependents plus outer");
        let outer_rows: Vec<_> = deps.iter().filter(|d| d.id == outer).collect();
        assert_eq!(outer_rows.len(), 1, "outer reported once despite reaching every chunk");
        assert_eq!(outer_rows[0].depth, 2);
    }

    /// An external symbol row (no definition span), minimal in every other field.
    fn external_symbol(id: &str) -> SymbolRow {
        SymbolRow {
            canonical_id: CanonicalId::from_raw(id.to_string()),
            display_name: id.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::External,
            document_path: None,
            span: None,
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        }
    }

    // The rank projection holds exactly the in-workspace universe and the collapsed dependency
    // edges: an external symbol is absent (as node and through its edges), a `contains` edge is
    // absent, a pair related under two kinds yields one edge, and an isolated in-workspace symbol
    // is present in the node universe.
    #[test]
    fn rank_projection_projects_workspace_dependency_edges_only() {
        let store = GraphStore::open_in_memory().unwrap();
        store.insert_symbol(&symbol_at("ws::a", "doc.rs", 0, 10)).unwrap();
        store.insert_symbol(&symbol_at("ws::b", "doc.rs", 20, 30)).unwrap();
        store
            .insert_symbol(&symbol_at("ws::isolated", "doc.rs", 40, 50))
            .unwrap();
        store.insert_symbol(&external_symbol("ext::x")).unwrap();

        let a = CanonicalId::from_raw("ws::a".to_string());
        let b = CanonicalId::from_raw("ws::b".to_string());
        let ext = CanonicalId::from_raw("ext::x".to_string());
        // The same pair under two kinds collapses to one projected edge.
        store.insert_edge(EdgeKind::Uses, &a, &b).unwrap();
        store.insert_edge(EdgeKind::Imports, &a, &b).unwrap();
        // Enclosure is excluded outright.
        store.insert_edge(EdgeKind::Contains, &b, &a).unwrap();
        // An edge touching an external endpoint is excluded in either direction.
        store.insert_edge(EdgeKind::Uses, &a, &ext).unwrap();
        store.insert_edge(EdgeKind::Uses, &ext, &b).unwrap();

        let projection = store.rank_projection().unwrap();
        let names: Vec<&str> = projection.nodes.iter().map(|n| n.as_str()).collect();
        assert_eq!(names, vec!["ws::a", "ws::b", "ws::isolated"]);
        assert_eq!(
            projection.edges,
            vec![(0, 1)],
            "one collapsed a→b edge and nothing else"
        );
    }
}
