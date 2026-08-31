//! Command handlers backing the CLI: the query commands (`get`, `trace`, `find`, `impact`) and the
//! operational set (`build`, `status`, `doctor`, `cache`).
//!
//! These are thin orchestration over the query engine and the ingest path, kept out of `main` so
//! they are testable without spawning a process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

use crate::exit::Failure;
use crate::git::{GitError, GitRepo, SeedMode};
use crate::graph::chunk::ChunkParams;
use crate::graph::join::JoinAccounting;
use crate::graph::store::{Freshness, GraphStore, StoreOpenError};
use crate::graph::syntax::Language;
use crate::graph::{content_hash, ingest};
use crate::identity::WorkspaceId;
use crate::query::diff::{self, FileChange};
use crate::query::impact::{ImpactRequest, shell_quote};
use crate::query::output::{Answer, WorkspaceRelation};
use crate::query::page::{
    OrderingIdentity, PageIdentity, apply_dependents_pagination, apply_impact_pagination, apply_pagination,
    precheck_cursor,
};
use crate::query::{Detail, OrderMode, QueryEngine, Relation};
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts, ExtractedIndex};
use crate::semantic::probe::PROBE_DEADLINE;
use crate::semantic::python_adapter::{PythonAdapter, environment_facts, resolve_environment};
use crate::semantic::{SemanticEngine, SemanticError, rust_adapter::RustAdapter};

/// The file extension and skipped top-level-name directories [`collect_sources`] walks with for
/// `language` — the single place the per-language discovery rules live, shared by
/// [`collect_rust_sources`]/[`collect_python_sources`] and [`is_discovered_path`] so the two rule
/// sets cannot drift.
fn language_discovery_rules(language: Language) -> (&'static str, &'static [&'static str]) {
    match language {
        Language::Rust => ("rs", &["target"]),
        Language::Python => ("py", &["venv"]),
    }
}

/// Collect `(workspace_relative_path, source_text)` for every `.rs` file under `root`.
///
/// Paths are relative to `root` and use `/` separators to match SCIP document paths. `target/` and
/// hidden directories are skipped. A file whose bytes are not valid UTF-8, or whose path is not
/// valid Unicode, is excluded and reported on standard error rather than failing the collection.
pub fn collect_rust_sources(root: &Path) -> Result<Vec<(String, String)>> {
    let (extension, skip_dirs) = language_discovery_rules(Language::Rust);
    collect_sources(root, extension, skip_dirs)
}

/// Collect `(workspace_relative_path, source_text)` for every `.py` file under `root`.
///
/// Paths are relative to `root` and use `/` separators to match SCIP document paths. `venv/` and
/// hidden directories (`.venv/` included) are skipped — the environment's own sources are not the
/// workspace's. A file whose bytes are not valid UTF-8, or whose path is not valid Unicode, is
/// excluded and reported on standard error rather than failing the collection.
pub fn collect_python_sources(root: &Path) -> Result<Vec<(String, String)>> {
    let (extension, skip_dirs) = language_discovery_rules(Language::Python);
    collect_sources(root, extension, skip_dirs)
}

/// Walk `root` for every file matching `extension`, skipping `skip_dirs` and hidden directories, and
/// delegate the per-file read (and its undecodable-content and non-Unicode-path exclusions) to
/// [`collect_dir`].
fn collect_sources(root: &Path, extension: &str, skip_dirs: &[&str]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    collect_dir(root, root, extension, skip_dirs, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn collect_dir(
    root: &Path,
    dir: &Path,
    extension: &str,
    skip_dirs: &[&str],
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if name.starts_with('.') || skip_dirs.contains(&name.as_ref()) {
                continue;
            }
            collect_dir(root, &path, extension, skip_dirs, out)?;
        } else if path.extension().is_some_and(|e| e == extension) {
            let rel_path = path.strip_prefix(root).unwrap_or(&path);
            // A path that is not valid Unicode cannot be used as a document-path key at all — the
            // lossy conversion below would replace its invalid bytes with a placeholder character,
            // and two distinct such paths can collide onto the same placeholder text, making the key
            // ambiguous. Excluded outright rather than risking that collision; the warning still
            // identifies the path on a best-effort basis for a human to locate it.
            let Some(rel) = rel_path.to_str() else {
                eprintln!(
                    "{}",
                    crate::render::sanitize(&format!(
                        "warning: {} has a path that is not valid Unicode; excluded from the build",
                        rel_path.to_string_lossy()
                    ))
                );
                continue;
            };
            let rel = rel.replace('\\', "/");
            match std::fs::read_to_string(&path) {
                Ok(text) => out.push((rel, text)),
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                    eprintln!(
                        "{}",
                        crate::render::sanitize(&format!(
                            "warning: {rel} is not valid UTF-8; excluded from the build"
                        ))
                    );
                }
                Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
            }
        }
    }
    Ok(())
}

/// Whether source discovery would have collected `path` — the same extension and skipped-directory
/// rules [`collect_sources`] walks with, applied to a path rather than to a directory entry.
///
/// Restoring a deleted or renamed file into the pre-change set must not add a path `build` would
/// never have seen (a file under `target/`, a dotted directory, or one with the wrong extension),
/// which would make the pre-change hash unreachable by any real build.
fn is_discovered_path(path: &str, language: Language) -> bool {
    let (extension, skip_dirs) = language_discovery_rules(language);
    if Path::new(path).extension().is_none_or(|e| e != extension) {
        return false;
    }
    // Every directory component (every segment but the last) must clear the same skip check
    // `collect_dir` applies before it recurses into a directory.
    let mut components = path.split('/');
    components.next_back();
    components.all(|dir| !dir.starts_with('.') && !skip_dirs.contains(&dir))
}

/// The workspace's discovered sources with `changes` reverted — the change's pre-change side, as
/// `build` would have seen it.
///
/// Exactness is graded against the *current discovered set with the diff undone*, not against the
/// base git tree: source discovery walks the filesystem by extension and ignores git tracking, so an
/// untracked-but-discovered source (a scratch script, a generated module) is part of what `build`
/// hashed. Hashing the base tree instead would leave any such file mismatched forever, putting
/// `exact` permanently out of reach even seconds after a fresh build.
///
/// Reverting undoes exactly what the change did: a modified file takes its pre-change content, a
/// file the change added is dropped, a file it deleted is restored, and a renamed file is restored
/// at its pre-change path. Untracked files participate with their current content, exactly as
/// `build` saw them, so an untracked source edited after the build correctly grades the answer
/// approximate — drift a base-tree hash could never see.
pub fn revert_changes(
    sources: &[(String, String)],
    changes: &[FileChange],
    pre_contents: &BTreeMap<String, String>,
    language: Language,
) -> Vec<(String, String)> {
    let mut set: BTreeMap<String, String> = sources.iter().cloned().collect();
    for change in changes {
        // Drop the post-change path when it differs from the pre-change path: an added file has no
        // pre-change path at all, and a renamed file's identity moves to its pre-change path. A
        // modified file keeps the same path on both sides, so no removal is needed — the insertion
        // below overwrites it in place.
        if let Some(post_path) = &change.post_path
            && change.pre_path.as_ref() != Some(post_path)
        {
            set.remove(post_path);
        }
        // Restore the pre-change content at the pre-change path, but only when that path is one
        // source discovery would actually have collected, and only when the boundary supplied its
        // pre-change text.
        if let Some(pre_path) = &change.pre_path
            && is_discovered_path(pre_path, language)
            && let Some(content) = pre_contents.get(pre_path)
        {
            set.insert(pre_path.clone(), content.clone());
        }
    }
    set.into_iter().collect()
}

/// Render an answer, honoring the `--json` flag: the serialized machine answer under `--json`, or the
/// human projection of that same value otherwise. `styled` gates structural styling on the human
/// render and is ignored under `--json` (the machine answer is never styled).
pub fn render<T: serde::Serialize + crate::render::HumanRender>(
    answer: &Answer<T>,
    json: bool,
    styled: bool,
) -> String {
    if json {
        answer.to_json()
    } else {
        crate::render::to_human(answer, styled)
    }
}

/// Open an existing index store for a read-only query, refusing to create one as a side effect.
///
/// A query must never bring an empty store into being merely by being asked, so a missing database
/// file is reported as an absent index naming the build that would fix it, and nothing is written at
/// the path. A file present that c10r did not create, and a store present but written under a
/// different schema version, both surface through [`GraphStore::open`]'s typed guards. A
/// schema-stamped store that carries no build metadata — a never-completed build, its tables created
/// but never populated — is reported as an absent index rather than answered as an empty one, since
/// no build has actually run against it.
fn open_query_store(db: &Path) -> Result<GraphStore> {
    if !db.exists() {
        return Err(Failure::NoIndex(format!("no index found at {}; run `c10r build` first", db.display())).into());
    }
    let store = GraphStore::open(db).context("opening index database")?;
    if store.read_metadata()?.is_none() {
        return Err(Failure::NoIndex(format!(
            "index at {} is incomplete (no build metadata); run `c10r build`",
            db.display()
        ))
        .into());
    }
    Ok(store)
}

/// How the store's recorded workspace relates to the root the query was invoked against: `None` when
/// they match (a match carries no marker), a disclosure otherwise.
///
/// The comparison is between canonicalized roots, so a relative invocation path or a symlinked parent
/// still reads as the same workspace. It never refuses: the store is c10r's own replayable artifact,
/// and a repo that moved or a container that remounts the same checkout elsewhere is a legitimate
/// state — one the marker labels and a rebuild clears. A root that cannot be canonicalized leaves the
/// comparison unevaluable, which is disclosed as unknown rather than passed off as a match.
fn workspace_relation(store: &GraphStore, root: &Path) -> Result<Option<WorkspaceRelation>> {
    // Every arm that cannot produce an exact comparison lands on `Unknown`: no recorded build, no
    // recorded root, a root that will not resolve, or either side unrepresentable as text. Only an
    // exact match returns "no marker", so nothing is ever passed off as matched by default.
    let Some(recorded_root) = store.read_metadata()?.and_then(|meta| meta.workspace_root) else {
        return Ok(Some(WorkspaceRelation::Unknown));
    };
    let Ok(current) = std::fs::canonicalize(root) else {
        return Ok(Some(WorkspaceRelation::Unknown));
    };
    let Some(current) = current.to_str() else {
        return Ok(Some(WorkspaceRelation::Unknown));
    };
    if current == recorded_root {
        return Ok(None);
    }
    Ok(Some(WorkspaceRelation::Mismatched { recorded_root }))
}

/// The provenance, source hash, and declared environment currently in effect for a workspace root,
/// used to mark answers fresh or stale. The recorded metadata's analyzer identity says which
/// backend's state applies. When no analyzer is available, the recorded provenance is echoed so
/// queries still work against a previously-built index.
fn current_state(
    store: &GraphStore,
    root: &Path,
    rust_analyzer: &str,
) -> Result<(AnalyzerProvenance, String, Option<EnvironmentFacts>)> {
    let recorded = store.read_metadata()?;
    // A recorded analyzer decides the language through the one dispatch, so an unrecognized one is
    // refused here as it is at build time. No recorded index at all is not that case: there is no
    // analyzer to read, and the Rust collection is the default a first build assumes.
    let recorded_language = match recorded.as_ref() {
        Some(m) => Some(crate::graph::backend_language(&m.provenance.analyzer_name)?),
        None => None,
    };
    let is_python = recorded_language == Some(Language::Python);

    if is_python {
        let sources = collect_python_sources(root)?;
        let hash = content_hash(&sources);
        // The environment in effect, resolved exactly as a build resolves it. An environment that
        // does not resolve (or whose facts cannot be read) is absent — which the freshness
        // comparison reports as drift against a recorded environment, never as fresh.
        let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
        let environment = match resolve_environment(None, virtual_env.as_deref(), root) {
            Ok(env) => match environment_facts(&env) {
                Ok(facts) => Some(facts),
                Err(SemanticError::Timeout(_)) => {
                    // A present-but-unresponsive interpreter is disclosed, then the environment is
                    // treated as absent — the same machine-answer shape a failed read already yields.
                    disclose_probe_timeout("the environment's python interpreter", "environment treated as absent");
                    None
                }
                Err(_) => None,
            },
            Err(_) => None,
        };
        // Prefer the live tool's version; fall back to the recorded provenance. A timeout is disclosed
        // before the fallback; a missing tool falls back silently.
        let provenance = match PythonAdapter::discover_version(PythonAdapter::analyzer_name()) {
            Ok(version) => AnalyzerProvenance {
                analyzer_name: PythonAdapter::analyzer_name().to_string(),
                analyzer_version: version,
            },
            Err(e) => {
                if matches!(e, SemanticError::Timeout(_)) {
                    disclose_probe_timeout("scip-python version", "freshness reflects recorded provenance");
                }
                recorded
                    .map(|m| m.provenance)
                    .expect("python identity implies metadata")
            }
        };
        return Ok((provenance, hash, environment));
    }

    let sources = collect_rust_sources(root)?;
    let hash = content_hash(&sources);
    // Prefer a live analyzer's version; fall back to the recorded provenance. A timeout is disclosed
    // before the fallback; a missing analyzer falls back silently (the common no-tool case).
    let provenance = match RustAdapter::new(rust_analyzer) {
        Ok(adapter) => adapter.provenance(),
        Err(e) => {
            if matches!(e, SemanticError::Timeout(_)) {
                disclose_probe_timeout("rust-analyzer version", "freshness reflects recorded provenance");
            }
            recorded.map(|m| m.provenance).unwrap_or(AnalyzerProvenance {
                analyzer_name: RustAdapter::analyzer_name().to_string(),
                analyzer_version: "unknown".to_string(),
            })
        }
    };
    Ok((provenance, hash, None))
}

/// Emit a one-line stderr diagnostic that a bounded tool probe timed out, routed through the same
/// [`crate::render::sanitize`] guard every diagnostic passes. Disclosed only on a timeout — a missing
/// tool stays silent so the common no-tool case adds no noise — so a query still answers from recorded
/// state while telling the caller the freshness read was degraded.
fn disclose_probe_timeout(probe: &str, consequence: &str) {
    eprintln!(
        "{}",
        crate::render::sanitize(&format!(
            "warning: {probe} probe timed out after {}s; {consequence}",
            PROBE_DEADLINE.as_secs()
        ))
    );
}

/// Resolve the workspace identity for a build: a supplied identity is used verbatim; otherwise it is
/// derived deterministically from the canonicalized workspace root's directory name.
///
/// The derivation is a pure function of the root path — no git or remote probing — so the default
/// never shifts when unrelated configuration changes. When canonicalization leaves no name component
/// (e.g. the filesystem root), the build refuses with a teaching error pointing at `--workspace`
/// rather than silently sharing a namespace.
pub fn resolve_workspace(supplied: Option<&str>, root: &Path) -> Result<WorkspaceId> {
    if let Some(name) = supplied {
        return Ok(WorkspaceId::new(name));
    }
    let canonical =
        std::fs::canonicalize(root).with_context(|| format!("canonicalizing workspace root {}", root.display()))?;
    let name = canonical.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        anyhow!(
            "workspace root {} has no directory name to derive an identity from; pass --workspace <name>",
            canonical.display()
        )
    })?;
    Ok(WorkspaceId::new(name))
}

/// Select the language backend for a build: an explicit selection overrides detection outright
/// (legacy layouts without a modern manifest are served by the flag); detection reads the
/// workspace's project manifests — `Cargo.toml` marks Rust, `pyproject.toml` marks Python.
///
/// Both manifests without a selection, or neither manifest, refuse with a typed teaching error
/// rather than guessing a backend.
pub fn detect_language(explicit: Option<Language>, root: &Path) -> Result<Language> {
    if let Some(language) = explicit {
        return Ok(language);
    }
    let rust = root.join("Cargo.toml").exists();
    let python = root.join("pyproject.toml").exists();
    match (rust, python) {
        (true, false) => Ok(Language::Rust),
        (false, true) => Ok(Language::Python),
        (true, true) => Err(anyhow!(
            "both Cargo.toml and pyproject.toml are present in {}; pass --language rust|python to select the backend",
            root.display()
        )),
        (false, false) => Err(anyhow!(
            "no supported project was detected in {} (no Cargo.toml or pyproject.toml); \
             pass --language rust|python to select a backend explicitly",
            root.display()
        )),
    }
}

/// `build`: (re)build the index for the workspace via the ingest path, through the backend the
/// workspace's manifest selects (or `language` explicitly names).
///
/// Every refusal — ambiguous or missing manifests, an unresolvable Python environment, an
/// unavailable tool — happens before the store is opened, so a failed attempt leaves an existing
/// store untouched.
#[allow(clippy::too_many_arguments)] // the CLI's flat build-command surface travels together
pub fn run_build(
    db: &Path,
    workspace: Option<&str>,
    root: &Path,
    rust_analyzer: &str,
    scip_python: &str,
    environment: Option<&Path>,
    language: Option<Language>,
    force: bool,
    chunk_params: &ChunkParams,
) -> Result<BuildOutcome> {
    // The chunk parameters are validated before anything is resolved or analyzed: a malformed pair
    // is a usage error, and no build begins from it.
    if chunk_params.chunk_size < crate::graph::chunk::MIN_CHUNK_SIZE {
        return Err(Failure::Usage(format!(
            "--chunk-size {} leaves no room for content beside a passage's header; the minimum is {}",
            chunk_params.chunk_size,
            crate::graph::chunk::MIN_CHUNK_SIZE
        ))
        .into());
    }
    if chunk_params.overlap >= chunk_params.chunk_size {
        return Err(Failure::Usage(format!(
            "--chunk-overlap {} must be smaller than --chunk-size {}",
            chunk_params.overlap, chunk_params.chunk_size
        ))
        .into());
    }
    let workspace_id = resolve_workspace(workspace, root)?;
    let language = detect_language(language, root)?;

    // The analyzer is resolved before anything is analyzed, because its identity is one of the
    // inputs the currency check compares — and resolving it once serves both the check and the
    // analysis that may follow.
    let engine: Box<dyn SemanticEngine> = match language {
        Language::Rust => Box::new(
            RustAdapter::new(rust_analyzer)
                .map_err(|e| Failure::IndexerSetup(format!("rust-analyzer unavailable: {e}")))?,
        ),
        Language::Python => {
            // $VIRTUAL_ENV is read here, at the outer edge; resolution itself is pure.
            let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
            let resolved = resolve_environment(environment, virtual_env.as_deref(), root)
                .map_err(|e| Failure::IndexerSetup(format!("{e}")))?;
            Box::new(
                PythonAdapter::new(scip_python, resolved, workspace_id.as_str())
                    .map_err(|e| Failure::IndexerSetup(format!("{e}")))?,
            )
        }
    };

    let sources = match language {
        Language::Rust => collect_rust_sources(root)?,
        Language::Python => collect_python_sources(root)?,
    };

    // Nothing to do when the store already describes this workspace under this analyzer,
    // environment, and chunk parameters: analyzing would reproduce the rows it already holds.
    if !force
        && let Some(accounting) = current_index_accounting(
            db,
            &workspace_id,
            &sources,
            engine.as_ref(),
            language,
            environment,
            root,
            chunk_params,
        )?
    {
        return Ok(BuildOutcome::AlreadyCurrent(accounting));
    }

    let index = engine.analyze(root).map_err(|e| anyhow!("indexing failed: {e}"))?;
    ensure_parent_dir(db)?;
    let workspace_root = canonical_root(root)?;
    // The write path replaces an incompatible store of its own outright: the index is derived,
    // replayable data, so rebuild is the migration. A file c10r did not create is refused instead.
    let mut store = GraphStore::open_or_replace(db).context("opening index database")?;
    disclose_workspace_handoff(&store, workspace_root.as_deref());
    let accounting = crate::graph::ingest_with_params(
        &mut store,
        &workspace_id,
        workspace_root.as_deref(),
        &index,
        &sources,
        chunk_params,
    )
    .map_err(|e| anyhow!("ingest failed: {e}"))?;
    Ok(BuildOutcome::Rebuilt(accounting))
}

/// The outcome of a build request: the workspace was analyzed and the store rewritten, or the store
/// already described the workspace and nothing was analyzed or written.
///
/// Both carry the same accounting — the freshly produced one, or the one the store recorded when it
/// was built — so a caller renders one answer shape either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildOutcome {
    /// The workspace was analyzed and the store rebuilt.
    Rebuilt(crate::graph::join::JoinAccounting),
    /// The stored index already described the workspace's sources, analyzer, and environment.
    AlreadyCurrent(crate::graph::join::JoinAccounting),
}

impl BuildOutcome {
    /// The join accounting the build produced, or the one the current store already recorded.
    pub fn accounting(&self) -> crate::graph::join::JoinAccounting {
        match self {
            BuildOutcome::Rebuilt(accounting) | BuildOutcome::AlreadyCurrent(accounting) => *accounting,
        }
    }

    /// Whether the workspace was analyzed and the store rewritten.
    pub fn rebuilt(&self) -> bool {
        matches!(self, BuildOutcome::Rebuilt(_))
    }
}

/// The accounting recorded by a store that already describes `sources` under `engine`'s analyzer,
/// the environment in effect, and the chunk parameters in effect, or `None` when a build is
/// required.
///
/// A store that is absent, unreadable, carries no metadata, or was built at an incompatible schema
/// version is not current — every one of those states means the build must run, so none of them is
/// an error here.
#[allow(clippy::too_many_arguments)] // the currency inputs travel together with the build's
fn current_index_accounting(
    db: &Path,
    workspace: &WorkspaceId,
    sources: &[(String, String)],
    engine: &dyn SemanticEngine,
    language: Language,
    environment: Option<&Path>,
    root: &Path,
    chunk_params: &ChunkParams,
) -> Result<Option<crate::graph::join::JoinAccounting>> {
    if !db.exists() {
        return Ok(None);
    }
    let Ok(store) = GraphStore::open(db) else {
        return Ok(None);
    };
    // One metadata read backs the whole check: ownership, the freshness comparison, and the
    // returned accounting all describe the same recorded build, so a concurrent build committing
    // mid-check cannot split the picture. Metadata that cannot be read — a corrupt row in a store
    // `open` already recognized as the system's own — is not current: the rebuild re-records it,
    // and a store the system must not replace was refused before this point.
    let Ok(Some(metadata)) = store.read_metadata() else {
        return Ok(None);
    };
    // A store recorded for another workspace describes different identities under the same source
    // text, and taking it over is a disclosed, re-recording build — never a skip.
    if &metadata.workspace_id != workspace || metadata.workspace_root != canonical_root(root)? {
        return Ok(None);
    }
    // The declared environment is part of what a Python index resolves against, so a build compares
    // it exactly as `status` does; the Rust backend declares none.
    let environment_facts = match language {
        Language::Rust => None,
        Language::Python => {
            let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
            resolve_environment(environment, virtual_env.as_deref(), root)
                .ok()
                .and_then(|env| environment_facts(&env).ok())
        }
    };
    // The chunk parameters are part of the semantic-index identity: a store built under different
    // parameters holds vectors from a regime the requested build would not produce.
    if metadata.chunk_params != *chunk_params {
        return Ok(None);
    }
    let hash = content_hash(sources);
    let fresh = crate::graph::store::freshness_of(&metadata, &hash, &engine.provenance(), environment_facts.as_ref());
    Ok((fresh == Freshness::Fresh).then_some(metadata.accounting))
}

/// The canonicalized workspace root a build records and a query compares against, or `None` when the
/// path cannot be represented exactly.
///
/// Canonicalization is what makes the comparison meaningful: two spellings of the same directory —
/// a relative path, a symlinked parent — must read as the same workspace. The conversion to text is
/// strict rather than lossy: a lossy rendering maps distinct paths onto the same string, and two
/// workspaces that collided there would compare equal, which is the one answer the comparison must
/// never give wrongly. A path that will not convert is recorded as absent instead.
fn canonical_root(root: &Path) -> Result<Option<String>> {
    let canonical =
        std::fs::canonicalize(root).with_context(|| format!("canonicalizing workspace root {}", root.display()))?;
    Ok(canonical.to_str().map(str::to_string))
}

/// The handoff notice for a build over a store recorded for a different workspace: both roots named,
/// or `None` when the store already describes this workspace.
///
/// The build re-records the root, so the handoff is a one-time announcement rather than a refusal: a
/// store is replayable data, and a moved or re-pointed workspace is a legitimate thing to do. A store
/// carrying no readable metadata — freshly created, or just replaced at a new schema version — has no
/// prior identity to disclose, which is why the guarantee reaches only current-version stores.
fn workspace_handoff_notice(store: &GraphStore, workspace_root: Option<&str>) -> Option<String> {
    let recorded = store.read_metadata().ok().flatten()?.workspace_root?;
    let rebuilding_for = workspace_root?;
    if recorded == rebuilding_for {
        return None;
    }
    Some(format!(
        "warning: the index at this path was built for workspace root {recorded}; rebuilding it for {rebuilding_for}"
    ))
}

/// Emit the workspace-handoff notice, if the build is taking a store over from another workspace.
fn disclose_workspace_handoff(store: &GraphStore, workspace_root: Option<&str>) {
    if let Some(notice) = workspace_handoff_notice(store, workspace_root) {
        eprintln!("{}", crate::render::sanitize(&notice));
    }
}

/// The one-line `build` accounting summary: every per-rule acceptance bucket inside the
/// parentheses (their sum is the `aligned=` total), followed by the refusal and syntax-only counts.
///
/// The buckets are read from [`JoinAccounting::by_rule`], which walks the whole rule vocabulary, so
/// a rule added to that vocabulary appears here with no edit and none can be left out.
pub fn build_accounting_line(accounting: &JoinAccounting) -> String {
    let buckets: Vec<String> = accounting
        .by_rule()
        .map(|(rule, count)| format!("{}={count}", rule.tag()))
        .collect();
    format!(
        "built: aligned={} ({}) text_mismatch={} semantic_only={} duplicate_ambiguous={} syntax_only={}",
        accounting.aligned_total(),
        buckets.join(" "),
        accounting.text_mismatch,
        accounting.semantic_only,
        accounting.duplicate_ambiguous,
        accounting.syntax_only
    )
}

/// The per-rule acceptance buckets as a JSON object keyed by each rule's stored tag, with the
/// derived `total` alongside — the `aligned` block both machine views carry.
fn aligned_buckets_json(accounting: &JoinAccounting) -> serde_json::Value {
    let mut aligned = serde_json::Map::new();
    for (rule, count) in accounting.by_rule() {
        aligned.insert(rule.tag().to_string(), serde_json::json!(count));
    }
    aligned.insert("total".to_string(), serde_json::json!(accounting.aligned_total()));
    serde_json::Value::Object(aligned)
}

/// The human `build` answer: the accounting line for a build that ran, or a statement that the
/// stored index already described the workspace and nothing was analyzed.
pub fn build_outcome_line(outcome: &BuildOutcome) -> String {
    match outcome {
        BuildOutcome::Rebuilt(accounting) => build_accounting_line(accounting),
        BuildOutcome::AlreadyCurrent(_) => {
            "already current: sources, analyzer, and environment match the stored index; nothing built \
             (use --force to build anyway)"
                .to_string()
        }
    }
}

/// The structured `build` answer: the accounting projection, plus whether this invocation rebuilt
/// the store or found it already current.
pub fn build_outcome_json(outcome: &BuildOutcome) -> serde_json::Value {
    let mut value = build_accounting_json(&outcome.accounting());
    if let Some(object) = value.as_object_mut() {
        object.insert("rebuilt".to_string(), serde_json::Value::Bool(outcome.rebuilt()));
    }
    value
}

/// The structured `build` accounting: the machine projection `--json build` emits, carrying every
/// per-rule acceptance bucket, their aligned total, and the refusal counts.
///
/// Derived from the same [`JoinAccounting`] the human [`build_accounting_line`] renders, and mirroring
/// the bucket names `run_status` uses in its `join_alignment` block so the two machine views agree.
pub fn build_accounting_json(accounting: &JoinAccounting) -> serde_json::Value {
    serde_json::json!({
        "aligned": aligned_buckets_json(accounting),
        "text_mismatch": accounting.text_mismatch,
        "semantic_only": accounting.semantic_only,
        "duplicate_ambiguous": accounting.duplicate_ambiguous,
        "syntax_only": accounting.syntax_only,
    })
}

/// Build directly from a pre-produced index (used where a live analyzer is unavailable, e.g. tests
/// and the exemplar fixtures).
///
/// `root` is the workspace the index describes: it is recorded canonicalized, exactly as `run_build`
/// records it, so a store built this way carries the same workspace identity a query compares
/// against.
pub fn build_from_index(
    db: &Path,
    workspace: &str,
    root: &Path,
    index: &ExtractedIndex,
    sources: &[(String, String)],
) -> Result<()> {
    ensure_parent_dir(db)?;
    let workspace_root = canonical_root(root)?;
    // Same write-path replacement policy as `run_build`.
    let mut store = GraphStore::open_or_replace(db).context("opening index database")?;
    disclose_workspace_handoff(&store, workspace_root.as_deref());
    ingest(
        &mut store,
        &WorkspaceId::new(workspace),
        workspace_root.as_deref(),
        index,
        sources,
    )
    .map_err(|e| anyhow!("ingest failed: {e}"))?;
    Ok(())
}

/// `status`: report provenance, freshness, and the join-alignment counts. With `discrepancies`, add
/// the bounded grouped discrepancy summary; with `all`, add every persisted discrepancy row instead.
/// With `duplicates`, add the duplicated-descriptor group detail; the group count is always reported.
pub fn run_status(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    json: bool,
    discrepancies: bool,
    all: bool,
    duplicates: bool,
) -> Result<String> {
    let store = open_query_store(db)?;
    let Some(meta) = store.read_metadata()? else {
        return Err(Failure::NoIndex(format!("no index built at {}; run `c10r build` first", db.display())).into());
    };
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let freshness = store
        .freshness(&hash, &provenance, environment.as_ref())?
        .expect("metadata present");
    let duplicated_groups = store.duplicated_groups()?;

    let mut report = serde_json::json!({
        "workspace": meta.workspace_id.as_str(),
        "provenance": {
            "analyzer_name": meta.provenance.analyzer_name,
            "analyzer_version": meta.provenance.analyzer_version,
        },
        "freshness": match freshness {
            crate::graph::store::Freshness::Fresh => "fresh",
            crate::graph::store::Freshness::StaleContent => "stale_content",
            crate::graph::store::Freshness::StaleVersion => "stale_version",
            crate::graph::store::Freshness::StaleEnvironment => "stale_environment",
        },
        "stale": freshness.is_stale(),
        "join_alignment": {
            // Per-rule acceptance buckets alongside the refusal counts, from the same walk over the
            // rule vocabulary `--json build` uses, so the two machine views cannot disagree.
            "aligned": aligned_buckets_json(&meta.accounting),
            "text_mismatch": meta.accounting.text_mismatch,
            "semantic_only": meta.accounting.semantic_only,
            "duplicate_ambiguous": meta.accounting.duplicate_ambiguous,
            "syntax_only": meta.accounting.syntax_only,
        },
        "duplicated_descriptors": {
            "group_count": duplicated_groups.len(),
        },
    });

    // The semantic-index identity in effect at build time, the chunk parameters included, rides
    // status as provenance beside the analyzer's.
    if let Some(identity) = store.semantic_index_identity()? {
        report["semantic_index"] = serde_json::json!({
            "model_identity": identity.model_identity,
            "corpus_definition_version": identity.corpus_definition_version,
            "chunk_size": identity.chunk_params.chunk_size,
            "chunk_overlap": identity.chunk_params.overlap,
        });
    }

    // The workspace-relationship disclosure rides beside freshness, exactly as it does on a query
    // answer: absent on a match, present otherwise. `workspace` above is the store's display
    // identity, which is never what the comparison turns on.
    if let Some(relation) = workspace_relation(&store, root)? {
        report["workspace_relation"] = serde_json::to_value(&relation)?;
    }

    if duplicates {
        report["duplicated_descriptors"]["groups"] = serde_json::json!(
            duplicated_groups
                .iter()
                .map(|g| {
                    serde_json::json!({
                        "descriptor_base": g.descriptor_base,
                        "definitions": g
                            .definitions
                            .iter()
                            .map(|d| {
                                serde_json::json!({
                                    "canonical_id": d.canonical_id.as_str(),
                                    "display_name": d.display_name,
                                    "document_path": d.document_path,
                                })
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>()
        );
    }

    if all {
        let rows = store.all_discrepancies()?;
        report["discrepancies"] = serde_json::json!({
            "listing": "all",
            "rows": rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "document_path": r.document_path,
                        // Typed absence: null when the occurrence's coordinates could not normalize.
                        "span_start": r.span.map(|s| s.0),
                        "span_end": r.span.map(|s| s.1),
                        "outcome": r.outcome,
                        "expected_name": r.expected_name,
                        "found_text": r.found_text,
                    })
                })
                .collect::<Vec<_>>(),
        });
    } else if discrepancies {
        let summary = store.discrepancy_summary()?;
        report["discrepancies"] = serde_json::json!({
            "listing": "summary",
            "truncated": summary.truncated(),
            "total_groups": summary.total_groups,
            "total_discrepancies": summary.total_discrepancies,
            "groups": summary
                .groups
                .iter()
                .map(|g| {
                    serde_json::json!({
                        "outcome": g.outcome,
                        "expected_name": g.expected_name,
                        "count": g.count,
                        "document_count": g.document_count,
                        "exemplar": {
                            "document_path": g.exemplar.document_path,
                            // Typed absence: null when the exemplar's coordinates could not normalize.
                            "span_start": g.exemplar.span.map(|s| s.0),
                            "span_end": g.exemplar.span.map(|s| s.1),
                        },
                    })
                })
                .collect::<Vec<_>>(),
        });
    }

    if json {
        Ok(serde_json::to_string_pretty(&report)?)
    } else {
        Ok(format!("{report:#}"))
    }
}

/// The current index state for `manifest`'s orientation block: whether an index has been built, and
/// — when one has — its workspace identity and freshness. Reuses the same metadata/freshness read
/// `status` performs, kept small (no join-accounting detail; that is `status`'s job).
///
/// An agent orienting on arrival may not have built an index yet, so no usable index answers
/// `built: false` rather than failing `manifest` itself: unlike a query, `manifest` does not need an
/// index to answer. Why it is unusable still travels with that answer — a file c10r did not create and
/// a path whose contents cannot be examined each carry their own reading, distinct from an absent
/// index and from each other, because their remedies are opposite (build here, versus point `--db`
/// somewhere else) and collapsing them would invite an agent to build over data that is not c10r's.
pub fn index_state(db: &Path, root: &Path, rust_analyzer: &str) -> serde_json::Value {
    match crate::graph::store::recognize_store(db) {
        Ok(crate::graph::store::StoreRecognition::Absent) => return serde_json::json!({ "built": false }),
        Ok(crate::graph::store::StoreRecognition::Unrecognized) => {
            return serde_json::json!({ "built": false, "store": "unrecognized" });
        }
        Ok(crate::graph::store::StoreRecognition::Recognized) => {}
        // A path that cannot be examined is its own reading: neither absent nor refused by marker.
        // Collapsing it onto absent would answer with the one remedy the guard exists to withhold —
        // build here — against something orientation never established the nature of.
        Err(_) => return serde_json::json!({ "built": false, "store": "unreadable" }),
    }
    match read_index_state(db, root, rust_analyzer) {
        Ok(state) => state,
        // A store that cleared recognition but carries a schema version this binary does not read is
        // its own reading too. An index does exist at the path, which "absent" denies: every query
        // against it names the version mismatch and refuses, so orientation reporting nothing there
        // would disagree with the rest of the surface about the same file.
        Err(error) if version_mismatched(&error) => serde_json::json!({ "built": false, "store": "incompatible" }),
        Err(_) => serde_json::json!({ "built": false }),
    }
}

/// Whether `error` carries a store-open refusal over a recognized store's schema version.
fn version_mismatched(error: &anyhow::Error) -> bool {
    matches!(
        error.chain().find_map(|e| e.downcast_ref::<StoreOpenError>()),
        Some(StoreOpenError::SchemaVersionMismatch { .. })
    )
}

fn read_index_state(db: &Path, root: &Path, rust_analyzer: &str) -> Result<serde_json::Value> {
    let store = GraphStore::open(db).context("opening index database")?;
    let Some(meta) = store.read_metadata()? else {
        return Ok(serde_json::json!({ "built": false }));
    };
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let freshness = store
        .freshness(&hash, &provenance, environment.as_ref())?
        .expect("metadata present");
    Ok(serde_json::json!({
        "built": true,
        "workspace": meta.workspace_id.as_str(),
        "freshness": match freshness {
            crate::graph::store::Freshness::Fresh => "fresh",
            crate::graph::store::Freshness::StaleContent => "stale_content",
            crate::graph::store::Freshness::StaleVersion => "stale_version",
            crate::graph::store::Freshness::StaleEnvironment => "stale_environment",
        },
        "stale": freshness.is_stale(),
    }))
}

/// `get`: retrieve a symbol at a detail level, by reference or by position.
///
/// `--max-lines` and `--from` bound and window the returned content; both apply only to a
/// content-bearing detail (`signature`/`interface`/`body`). Supplying either _explicitly_ with a
/// non-content detail (`location`, or the `location` default) is a usage error naming the details that
/// accept it; the defaulted values stay dormant otherwise.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_get(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: Option<&str>,
    at: Option<&str>,
    detail: Detail,
    max_lines: usize,
    from: usize,
    max_lines_explicit: bool,
    from_explicit: bool,
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    // The invocation's shape is validated before the store is opened, so a malformed `get` is a
    // usage error even against an absent index.
    let position = at.map(parse_position).transpose()?;
    let reference = match position {
        Some(_) => None,
        None => {
            Some(reference.ok_or_else(|| Failure::Usage("get requires a reference or --at position".to_string()))?)
        }
    };
    // Content bounds apply only to a content-bearing detail; an explicit bound on a non-content detail
    // is a modal usage error, decided before the store opens.
    let content_bearing = matches!(detail, Detail::Signature | Detail::Interface | Detail::Body);
    if !content_bearing {
        if max_lines_explicit {
            return Err(content_bound_misuse("--max-lines").into());
        }
        if from_explicit {
            return Err(content_bound_misuse("--from").into());
        }
    }
    // `0` is the unbounded sentinel for both bounds; a positive value is the applied bound.
    let effective_limit = (limit != 0).then_some(limit);
    let effective_max_lines = (max_lines != 0).then_some(max_lines);
    // The subject identity a continuation token binds to: the reference, or the source-position
    // string when retrieving by position.
    let subject = reference
        .map(str::to_string)
        .or_else(|| at.map(str::to_string))
        .unwrap_or_default();
    // Store-independent cursor checks run before the store opens, so a malformed cursor or a cursor
    // against an unbounded set is a usage error even against an absent index.
    precheck_cursor(cursor, effective_limit)?;

    let store = open_query_store(db)?;
    let index_hash = recorded_index_hash(&store)?;
    let relation = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);

    let answer = match &position {
        Some((doc, offset)) => engine.get_by_position(doc, *offset, detail, effective_max_lines, from)?,
        None => engine.get(
            reference.expect("required when --at is absent"),
            detail,
            effective_max_lines,
            from,
        )?,
    };
    let identity = PageIdentity {
        command: "get",
        reference: subject,
        relation: None,
        detail: Some(detail_label(detail)),
        depth: None,
        ordering: None,
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: Some(from),
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?.with_workspace_relation(relation);
    Ok(render(&answer, json, styled))
}

/// `trace`: return the symbols in a relation to the subject.
///
/// `depth` is meaningful only for the `dependents` relation, where it bounds the detailed impact
/// reach (default 1). Supplying it with any other relation is a typed teaching error that names the
/// flag, the offending relation, and the relations that accept it — rather than silently ignoring a
/// meaningless flag.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_trace(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: &str,
    relation: Relation,
    depth: Option<u32>,
    detail: Option<Detail>,
    max_lines: usize,
    max_lines_explicit: bool,
    order: OrderMode,
    order_explicit: bool,
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    if !matches!(relation, Relation::Dependents) && depth.is_some() {
        return Err(Failure::Usage(format!(
            "the `--depth` flag applies only to the `dependents` relation, but it was given with `{}`; \
             relations that accept `--depth`: dependents",
            relation_label(relation)
        ))
        .into());
    }
    // The order selector is defined only where its orderings are — a `dependents` trace (and
    // `impact`); an explicit `--order` with any other relation is refused before any traversal.
    // The defaulted value stays dormant, so an orderless invocation of another relation is fine.
    if !matches!(relation, Relation::Dependents) && order_explicit {
        return Err(Failure::Usage(format!(
            "the `--order` flag applies only to the `dependents` relation and `impact`, but it was given with \
             `{}`; relations that accept `--order`: dependents",
            relation_label(relation)
        ))
        .into());
    }
    // Trace content exists only under a content-bearing `--detail`; an explicit `--max-lines` without
    // one is a modal usage error, decided before the store opens.
    let content_bearing = matches!(
        detail,
        Some(Detail::Signature) | Some(Detail::Interface) | Some(Detail::Body)
    );
    if !content_bearing && max_lines_explicit {
        return Err(content_bound_misuse("--max-lines").into());
    }
    let effective_limit = (limit != 0).then_some(limit);
    let effective_max_lines = (max_lines != 0).then_some(max_lines);
    // Store-independent cursor checks run before the store opens, so a malformed cursor or a cursor
    // against an unbounded set is a usage error even against an absent index.
    precheck_cursor(cursor, effective_limit)?;
    let store = open_query_store(db)?;
    let index_hash = recorded_index_hash(&store)?;
    let relation_disclosure = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let identity = PageIdentity {
        command: "trace",
        reference: reference.to_string(),
        relation: Some(relation_label(relation)),
        detail: detail.map(detail_label),
        // The effective depth bound: `dependents` defaults to 1 when the flag is omitted, so an
        // explicit `--depth 1` and the default bind to the same identity.
        depth: matches!(relation, Relation::Dependents).then(|| depth.unwrap_or(1)),
        // The ordering identity — the selector together with the ranking model's version — binds
        // only where an ordering is defined; an explicit `--order ranked` and the default bind to
        // the same identity.
        ordering: matches!(relation, Relation::Dependents).then(|| OrderingIdentity::current(order)),
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: None,
        index_hash,
    };
    // The relation decides which engine method answers it. `dependents` is the one whose answer does
    // not fit a flat item list, so it is the one `as_trace` declines; every other relation yields the
    // trace relation `trace` accepts, and no run-time refusal stands between them.
    let Some(trace_relation) = relation.as_trace() else {
        let answer = engine.dependents(reference, depth.unwrap_or(1), detail, effective_max_lines, order)?;
        // Dependents pages its detailed rows under the limit; the depth/horizon/disclosure/beyond-bound
        // summary is repeated on every page as context. Every dependents answer — typed-empty
        // included — carries the ordering disclosure.
        let answer = apply_dependents_pagination(answer, effective_limit, cursor, &identity)?
            .with_workspace_relation(relation_disclosure)
            .with_ordering(order);
        return Ok(render(&answer, json, styled));
    };
    let answer = engine.trace(reference, trace_relation, detail, effective_max_lines)?;
    let answer =
        apply_pagination(answer, effective_limit, cursor, &identity)?.with_workspace_relation(relation_disclosure);
    Ok(render(&answer, json, styled))
}

/// `find`: return every symbol whose name contains `fragment`, matched case-insensitively, as a
/// bounded search independent of exact reference resolution.
/// `find` carries no content, so it takes only the result-set bounds (`--limit`/`--cursor`); it has
/// no `--max-lines` or `--from`.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_find(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    fragment: &str,
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    let effective_limit = (limit != 0).then_some(limit);
    // Store-independent cursor checks run before the store opens, so a malformed cursor or a cursor
    // against an unbounded set is a usage error even against an absent index.
    precheck_cursor(cursor, effective_limit)?;
    let store = open_query_store(db)?;
    let index_hash = recorded_index_hash(&store)?;
    let relation = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let answer = engine.find(fragment)?;
    let identity = PageIdentity {
        command: "find",
        reference: fragment.to_string(),
        relation: None,
        detail: None,
        depth: None,
        ordering: None,
        limit: effective_limit,
        max_lines: None,
        from: None,
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?.with_workspace_relation(relation);
    Ok(render(&answer, json, styled))
}

/// `search`: the corpus symbols nearest a natural-language query by estimated relevance — the
/// fused vector and lexical ranking over the persisted semantic corpus.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_search(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    query: &str,
    detail: Detail,
    max_lines: usize,
    max_lines_explicit: bool,
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    // Row content exists only under a content-bearing detail; an explicit `--max-lines` at location
    // detail is a modal usage error, decided before the store opens.
    let content_bearing = matches!(detail, Detail::Signature | Detail::Interface | Detail::Body);
    if !content_bearing && max_lines_explicit {
        return Err(content_bound_misuse("--max-lines").into());
    }
    let effective_limit = (limit != 0).then_some(limit);
    let effective_max_lines = (max_lines != 0).then_some(max_lines);
    // Store-independent cursor checks run before the store opens, so a malformed cursor or a cursor
    // against an unbounded set is a usage error even against an absent index.
    precheck_cursor(cursor, effective_limit)?;
    let store = open_query_store(db)?;
    let index_hash = recorded_index_hash(&store)?;
    let relation = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let answer = engine.search(query, detail, effective_max_lines)?;
    let identity = PageIdentity {
        command: "search",
        reference: query.to_string(),
        relation: None,
        detail: Some(detail_label(detail)),
        depth: None,
        ordering: None,
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: None,
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?.with_workspace_relation(relation);
    Ok(render(&answer, json, styled))
}

/// `similar`: the corpus symbols most similar in content to a subject symbol, the subject named by
/// reference or by source position, with deterministic clone-certainty markers above the estimated
/// ranking.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_similar(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: Option<&str>,
    at: Option<&str>,
    detail: Detail,
    max_lines: usize,
    max_lines_explicit: bool,
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    // The invocation's shape is validated before the store is opened, so a malformed `similar` is a
    // usage error even against an absent index.
    let position = at.map(parse_position).transpose()?;
    let reference = match position {
        Some(_) => None,
        None => Some(
            reference.ok_or_else(|| Failure::Usage("similar requires a reference or --at position".to_string()))?,
        ),
    };
    let content_bearing = matches!(detail, Detail::Signature | Detail::Interface | Detail::Body);
    if !content_bearing && max_lines_explicit {
        return Err(content_bound_misuse("--max-lines").into());
    }
    let effective_limit = (limit != 0).then_some(limit);
    let effective_max_lines = (max_lines != 0).then_some(max_lines);
    let subject = reference
        .map(str::to_string)
        .or_else(|| at.map(str::to_string))
        .unwrap_or_default();
    precheck_cursor(cursor, effective_limit)?;
    let store = open_query_store(db)?;
    let index_hash = recorded_index_hash(&store)?;
    let relation = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let answer = match &position {
        Some((doc, offset)) => engine.similar_by_position(doc, *offset, detail, effective_max_lines)?,
        None => engine.similar(
            reference.expect("required when --at is absent"),
            detail,
            effective_max_lines,
        )?,
    };
    let identity = PageIdentity {
        command: "similar",
        reference: subject,
        relation: None,
        detail: Some(detail_label(detail)),
        depth: None,
        ordering: None,
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: None,
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?.with_workspace_relation(relation);
    Ok(render(&answer, json, styled))
}

/// `impact`: the reverse-reachability impact of a change, seeded from a git diff rather than from a
/// symbol the caller names.
///
/// Order is load-bearing:
///
/// 1. Argument validation runs before any side effect — clap's `conflicts_with` already refuses a
///    revspec together with `--staged`, so what remains here is the cursor precheck.
/// 2. The git boundary is resolved before the store is opened. Whether `git` is usable does not
///    depend on whether an index exists, so an absent `git`, a non-worktree directory, or a bounded
///    timeout (all [`crate::exit::ExitCode::IndexerSetup`]) and a malformed or unresolvable revision
///    spec ([`crate::exit::ExitCode::Usage`]) are all decided — and reported — before the no-index
///    outcome would otherwise fire, which is the more actionable diagnostic when both are unmet at
///    once.
/// 3. The patch is parsed once the diff is in hand, and narrowed to `paths` against the parsed change
///    set (never as a `git diff` pathspec, which would break rename pairing) — before anything else
///    consumes the change set, so narrowing scopes the whole assessment.
/// 4. The store opens, and the current provenance/hash/environment are read exactly as the sibling
///    query handlers read them.
/// 5. Each touched file's pre-change content is fetched once per distinct path.
/// 6. The pre-change hash — the discovered source set with the diff reverted — is computed to grade
///    the answer's exactness.
/// 7. Untracked discovered sources are detected: the recovery recipe's copy step must materialize
///    them into its throwaway worktree, since a bare `git worktree add` populates only tracked files.
/// 8. Whether the change's pre-change side can actually be reconstructed from this workspace is
///    decided per mode — a hash match alone does not certify exactness, since the substitution that
///    produces it can itself be lossy — and, for a revision-range seed, its head endpoint is resolved
///    so the recovery recipe can re-run from a worktree at that head.
/// 9. The request is assembled, the assessment run, the answer paged, and rendered.
#[allow(clippy::too_many_arguments)] // the CLI's flat query-command surface travels together
pub fn run_impact(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    workspace: Option<&str>,
    revspec: Option<&str>,
    staged: bool,
    depth: u32,
    order: OrderMode,
    paths: &[PathBuf],
    limit: usize,
    cursor: Option<&str>,
    json: bool,
    styled: bool,
) -> Result<String> {
    let effective_limit = (limit != 0).then_some(limit);
    // Store-independent cursor checks run before the store opens, so a malformed cursor or a cursor
    // against an unbounded set is a usage error even against an absent index.
    precheck_cursor(cursor, effective_limit)?;

    let mode = match (revspec, staged) {
        (Some(spec), _) => SeedMode::Revspec(spec.to_string()),
        (None, true) => SeedMode::Staged,
        (None, false) => SeedMode::WorkingTree,
    };

    // The git boundary, before the store: every `GitError` is converted through `into_failure()`, so
    // the setup/usage split holds however the failure arose.
    let repo = GitRepo::discover(root).map_err(GitError::into_failure)?;
    let base_revision = repo.base_revision(&mode).map_err(GitError::into_failure)?;
    let patch = repo.diff(&mode).map_err(GitError::into_failure)?;

    // Narrowing is applied to the parsed change set, not handed to `git diff` as a pathspec: a
    // pathspec is applied before rename detection, so narrowing to a renamed file's post-change path
    // would leave git with nothing to pair it against. Applied immediately after parsing and before
    // anything else consumes the change set, so the pre-change hash reversion below also sees the
    // narrowed set — narrowing scopes the whole assessment, not just the seeds.
    let changes = diff::narrow_to_paths(diff::parse_patch(&patch), paths);

    let store = open_query_store(db)?;
    let page_index_hash = recorded_index_hash(&store)?;
    let relation = workspace_relation(&store, root)?;
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let meta = store
        .read_metadata()?
        .expect("open_query_store guarantees a completed build's metadata");

    // The language the recorded metadata's analyzer decides, which is also what source discovery
    // would have collected. Read through the one dispatch a build uses, so the two can never
    // disagree about what an index's documents are.
    let language = crate::graph::backend_language(&meta.provenance.analyzer_name)?;

    // Only a path source discovery would have collected can carry a seed or an unmappable region. A
    // change to a README, a lockfile, or a JSON fixture is not something the graph would ever track,
    // so it contributes nothing at all — reporting it as a region this index cannot resolve would
    // dress a file the graph never had an opinion about as a resolution gap, and would keep a diff
    // touching only such files from reaching the definite-none answer it deserves.
    let seed_changes: Vec<FileChange> = changes
        .iter()
        .filter(|change| {
            change
                .pre_path
                .as_deref()
                .is_some_and(|path| is_discovered_path(path, language))
        })
        .cloned()
        .collect();

    // Fetch the pre-change content of every touched source, once per distinct path. A path `show`
    // reports absent (deleted upstream of the base revision, or simply never existed there) gets no
    // entry — the query layer discloses it as unmappable rather than failing the whole assessment.
    let mut pre_contents: BTreeMap<String, String> = BTreeMap::new();
    for change in &seed_changes {
        let Some(pre_path) = &change.pre_path else { continue };
        if pre_contents.contains_key(pre_path) {
            continue;
        }
        if let Some(content) = repo.show(&base_revision, pre_path).map_err(GitError::into_failure)? {
            pre_contents.insert(pre_path.clone(), content);
        }
    }

    // The pre-change hash: that language's currently-discovered sources with the diff reverted. The
    // reversion sees the whole change, not just the seed-bearing part, so a rename that moves a file
    // out of the discovered set still drops its post-change path.
    let sources = match language {
        Language::Python => collect_python_sources(root)?,
        Language::Rust => collect_rust_sources(root)?,
    };
    let reverted = revert_changes(&sources, &changes, &pre_contents, language);
    let pre_change_hash = content_hash(&reverted);

    // A bare `git worktree add` (the recovery recipe's first step) materializes only tracked files,
    // so a discovered source `git` does not track needs its own copy step in that recipe.
    let tracked: BTreeSet<String> = repo
        .tracked_files()
        .map_err(GitError::into_failure)?
        .into_iter()
        .collect();
    let untracked_sources: Vec<String> = sources
        .iter()
        .filter(|(path, _)| !tracked.contains(path))
        .map(|(path, _)| path.clone())
        .collect();

    // The resolved head revision of a `Revspec` range, or `None` for every other mode and for a bare
    // single-revision spec — both the reconstructibility check below and the recovery recipe need it.
    let range_head = repo.range_head(&mode).map_err(GitError::into_failure)?;

    // Reconstructibility gates `Exactness::Exact` alongside the hash comparison: the pre-change side
    // is rebuilt by substituting each changed file's base content wholesale, which is lossy whenever
    // a file carries edits the selected diff does not describe.
    //
    // - `WorkingTree`'s diff spans base-to-worktree by definition, so nothing is outside it.
    // - `Staged` is reconstructible unless some unstaged path is also part of the selected diff — the
    //   substitution would then discard that unstaged edit too, landing on a state the diff does not
    //   actually describe.
    // - A bare single-revision spec behaves like `WorkingTree` (`git diff <rev>` also spans that
    //   revision to the worktree). A range is reconstructible only when its head is what is currently
    //   checked out and no uncommitted change touches a discovered source — otherwise reverting the
    //   range's diff from this workspace yields a hybrid no index can match.
    let reconstructible = match &mode {
        SeedMode::WorkingTree => true,
        SeedMode::Staged => {
            let unstaged: BTreeSet<String> = repo
                .unstaged_paths()
                .map_err(GitError::into_failure)?
                .into_iter()
                .collect();
            !changes.iter().any(|change| {
                change.pre_path.as_ref().is_some_and(|p| unstaged.contains(p))
                    || change.post_path.as_ref().is_some_and(|p| unstaged.contains(p))
            })
        }
        SeedMode::Revspec(_) => match &range_head {
            None => true,
            Some(head) => {
                let current_head = repo
                    .base_revision(&SeedMode::WorkingTree)
                    .map_err(GitError::into_failure)?;
                if *head != current_head {
                    false
                } else {
                    let uncommitted = repo.uncommitted_paths().map_err(GitError::into_failure)?;
                    !uncommitted.iter().any(|p| is_discovered_path(p, language))
                }
            }
        },
    };

    let workspace_id = resolve_workspace(workspace, root)?;
    let db_display = db.display().to_string();
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let request = ImpactRequest {
        mode: &mode,
        paths,
        base_revision: &base_revision,
        changes: &seed_changes,
        pre_contents: &pre_contents,
        pre_change_hash: &pre_change_hash,
        index_hash: &meta.content_hash,
        db: &db_display,
        workspace: workspace_id.as_str(),
        prefix: repo.prefix(),
        reconstructible,
        untracked_sources: &untracked_sources,
        range_head: range_head.as_deref(),
        depth,
        order,
    };
    let answer = engine.impact(&request)?;

    // The reference a continuation token binds to: the seed mode, the resolved base revision, the
    // narrowing paths, and a digest of the patch itself, joined by a unit separator that cannot appear
    // in any of them. The resolved revision alone is not enough: every other query's result set is a
    // function of the index, but an impact answer is also a function of the diff, which is not a flag
    // — the working tree can change between two pages, and two different revision ranges sharing a
    // base (`A..B` and `A..C`) resolve to the same base revision and flags yet seed a different
    // answer. Binding the patch digest itself is what tells the two apart.
    let reference = [
        seed_mode_label(&mode).to_string(),
        base_revision.clone(),
        paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\u{1f}"),
        hex_sha256(&patch),
    ]
    .join("\u{1f}");
    let identity = PageIdentity {
        command: "impact",
        reference,
        relation: None,
        detail: None,
        depth: Some(depth),
        ordering: Some(OrderingIdentity::current(order)),
        limit: effective_limit,
        max_lines: None,
        from: None,
        index_hash: page_index_hash,
    };
    // Every impact answer carries the ordering disclosure, whatever its outcome.
    let answer = apply_impact_pagination(answer, effective_limit, cursor, &identity)?
        .with_workspace_relation(relation)
        .with_ordering(order);
    Ok(render(&answer, json, styled))
}

/// The seed mode label used in `impact`'s continuation-token identity: `working_tree`, `staged`, or
/// `range` — the same category labels the answer's own `seed_mode` field reports.
fn seed_mode_label(mode: &SeedMode) -> &'static str {
    match mode {
        SeedMode::WorkingTree => "working_tree",
        SeedMode::Staged => "staged",
        SeedMode::Revspec(_) => "range",
    }
}

/// The readiness of one required indexer, as reported by `doctor`.
///
/// Three states are distinguished so the caller can tell a tool that is not installed from one that
/// is installed but wedged — different problems with different fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    /// The indexer was found and answered the readiness probe.
    Present,
    /// The indexer was not found (or could not be spawned).
    Absent,
    /// The indexer was found but did not answer the readiness probe within the bounded deadline.
    Unresponsive,
}

/// One required indexer's readiness, as reported by `doctor`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexerStatus {
    /// The indexer's name (`rust-analyzer` or `scip-python`).
    pub name: String,
    /// Whether the indexer is present, absent, or present-but-unresponsive.
    pub presence: Presence,
    /// The indexer's discovered version, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// An install hint when absent, or an investigation hint when unresponsive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl IndexerStatus {
    /// Whether this indexer is ready — found and responsive. An absent or unresponsive indexer is not
    /// ready, and both drive the indexer/setup-failure exit code.
    pub fn is_ready(&self) -> bool {
        matches!(self.presence, Presence::Present)
    }
}

/// `doctor`: probe each required language indexer for presence and version, by the same default
/// executable name `build` invokes for it. Both indexers are probed unconditionally — the report
/// covers what a build could need, not what the current workspace's language happens to be — and a
/// tool that cannot be run for any reason (missing, unexecutable, or erroring) reports absent with an
/// install hint rather than propagating a failure.
pub fn run_doctor() -> Vec<IndexerStatus> {
    let rust_analyzer = match RustAdapter::new("rust-analyzer") {
        Ok(adapter) => present_status(RustAdapter::analyzer_name(), adapter.provenance().analyzer_version),
        Err(SemanticError::Timeout(_)) => unresponsive_status(RustAdapter::analyzer_name()),
        Err(_) => absent_status(
            RustAdapter::analyzer_name(),
            "install: rustup component add rust-analyzer",
        ),
    };
    let scip_python = match PythonAdapter::discover_version("scip-python") {
        Ok(version) => present_status(PythonAdapter::analyzer_name(), version),
        Err(SemanticError::Timeout(_)) => unresponsive_status(PythonAdapter::analyzer_name()),
        Err(_) => absent_status(
            PythonAdapter::analyzer_name(),
            "install: npm install -g @sourcegraph/scip-python",
        ),
    };
    vec![rust_analyzer, scip_python]
}

/// A present indexer's status, carrying its discovered version and no hint.
fn present_status(name: &str, version: String) -> IndexerStatus {
    IndexerStatus {
        name: name.to_string(),
        presence: Presence::Present,
        version: Some(version),
        hint: None,
    }
}

/// An absent indexer's status, carrying the install hint that tells the caller how to obtain it.
fn absent_status(name: &str, install_hint: &str) -> IndexerStatus {
    IndexerStatus {
        name: name.to_string(),
        presence: Presence::Absent,
        version: None,
        hint: Some(install_hint.to_string()),
    }
}

/// An unresponsive indexer's status: found but silent within the probe deadline, carrying an
/// investigation hint distinct from the install hint an absent tool carries.
fn unresponsive_status(name: &str) -> IndexerStatus {
    IndexerStatus {
        name: name.to_string(),
        presence: Presence::Unresponsive,
        version: None,
        hint: Some(format!(
            "found but did not respond within {}s; check for a stuck process or a slow filesystem",
            PROBE_DEADLINE.as_secs()
        )),
    }
}

/// The fixed-width human rendering of a `doctor` report: one line per indexer, its name and status
/// word in fixed columns, followed by its version when present, its install hint when absent, or its
/// investigation hint when unresponsive.
///
/// The indexer name and version/hint are sanitized: the version comes from the probed tool's own
/// `--version` output, which a hostile or misbehaving indexer could use to embed terminal-control
/// bytes.
pub fn render_doctor_report(statuses: &[IndexerStatus]) -> String {
    statuses
        .iter()
        .map(|s| {
            let name = crate::render::sanitize(&s.name);
            let (status, detail) = match s.presence {
                Presence::Present => ("present", s.version.as_deref().unwrap_or("")),
                Presence::Absent => ("absent", s.hint.as_deref().unwrap_or("")),
                Presence::Unresponsive => ("unresponsive", s.hint.as_deref().unwrap_or("")),
            };
            let detail = crate::render::sanitize(detail);
            format!("{name:<15}{status:<13}{detail}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The outcome of `cache`: the affected `--db` path and whether an index was actually removed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheOutcome {
    /// The `--db` path the removal targeted.
    pub path: String,
    /// Whether an index was removed (`false` when there was nothing to remove).
    pub removed: bool,
}

/// `cache`: remove the stored index at `db`. An already-absent index is success with `removed:
/// false`. Removal applies only to a store c10r created — recognized by the ownership marker in the
/// file header, the same test every other store-touching path consults — so a mis-pointed `--db`
/// cannot unlink an unrelated file, not even another application's versioned database; that target is
/// refused with a teaching message naming the manual alternative. When an index is removed, its
/// SQLite WAL/SHM sidecar files (present under concurrent access) are removed alongside it on a
/// best-effort basis — their absence or removal failure never fails the command, since the primary
/// database file is the index's identity. An OS error removing the primary file is surfaced to the
/// caller, naming the path.
pub fn run_cache(db: &Path) -> Result<CacheOutcome> {
    if !db.exists() {
        return Ok(CacheOutcome {
            path: db.display().to_string(),
            removed: false,
        });
    }
    if !is_removable_index(db)? {
        // The ownership category, the same one a build or a query reaches over this file: what the
        // caller must do next is decided by the target, not by which command met it. The message
        // keeps reset's own shape — naming the manual `rm` — because this caller already asked for a
        // deletion, so naming the manual step is informed consent rather than a footgun.
        return Err(Failure::UnrecognizedStore(format!(
            "{path} is not a c10r index store, so c10r will not remove it \
             — if you mean to delete it: rm {path}",
            path = db.display()
        ))
        .into());
    }
    std::fs::remove_file(db).with_context(|| format!("failed to remove index at {}", db.display()))?;
    // Build the WAL/SHM sidecar paths by pushing the suffix onto the primary path's os-string, so a
    // non-UTF-8 `--db` path is preserved byte-for-byte rather than lossily round-tripped through
    // `display()`.
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = db.as_os_str().to_os_string();
        sidecar.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(sidecar));
    }
    Ok(CacheOutcome {
        path: db.display().to_string(),
        removed: true,
    })
}

/// Whether the file at `db` is a store c10r created, and so safe for `cache` to remove.
///
/// The decision is the shared ownership recognizer's: a SQLite database carrying c10r's own
/// application marker. A store written under an older schema version still carries the marker, so it
/// remains removable — resetting it is the recovery path. Anything else — a text file, an empty file,
/// another application's database however it is versioned — is refused, so `cache` never unlinks an
/// unrelated file at a mis-pointed `--db` path.
fn is_removable_index(db: &Path) -> Result<bool> {
    // A path that cannot be examined refuses through the same typed error every other store-touching
    // path raises, so a failed inspection reads as an ownership refusal naming the cause rather than a
    // bare OS error — and it never becomes a verdict that the target is somebody else's.
    let recognition = crate::graph::store::recognize_or_refuse(db)?;
    Ok(matches!(recognition, crate::graph::store::StoreRecognition::Recognized))
}

/// The rendering of a `cache` report: the JSON structured answer under `--json`, or a one-line human
/// summary naming the affected path.
pub fn render_cache_report(outcome: &CacheOutcome, json: bool) -> String {
    if json {
        serde_json::to_string_pretty(outcome).unwrap_or_else(|_| "{}".to_string())
    } else if outcome.removed {
        format!("removed index at {}", crate::render::sanitize(&outcome.path))
    } else {
        format!("nothing to remove at {}", crate::render::sanitize(&outcome.path))
    }
}

/// The outcome of `hooks install`: the hook path written.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HookOutcome {
    /// The path the hook was written to.
    pub path: String,
}

/// The post-commit hook script for a workspace at `root`, indexed at `db` under `workspace`: a
/// `#!/bin/sh` script that reruns `c10r build` after every commit, keeping the index at the committed
/// state the diff-seeded assessment's exact path depends on.
///
/// All three are substituted with resolved, absolute, shell-quoted values rather than left to
/// defaults. Git invokes `post-commit` with the working directory at the worktree's top level, which
/// is neither the caller's invocation directory nor — for a workspace below the repository root — the
/// workspace at all, so a bare `c10r build` would index the wrong tree into the wrong store under the
/// wrong identity. `--workspace` is load-bearing for the same reason it is in the recovery recipe:
/// `build` derives the identity from the root directory name, so leaving it implicit would namespace
/// symbols under whatever directory the hook happened to run from.
fn post_commit_hook_script(root: &Path, db: &Path, workspace: &str) -> String {
    format!(
        "#!/bin/sh\nc10r build {} --db {} --workspace {}\n",
        shell_quote(&root.display().to_string()),
        shell_quote(&db.display().to_string()),
        shell_quote(workspace),
    )
}

/// `hooks install`: write the post-commit hook that refreshes the index after every commit.
///
/// Refuses rather than overwrites when anything already exists at the resolved hook path: a commit
/// hook is the caller's, and silently replacing one is destructive — a query-shaped tool has no
/// license to perform it. The written file is made executable (`0o755`), since a hook git will not
/// execute is not installed.
///
/// `root` and `db` are made absolute before they are written into the script, because the hook runs
/// from git's own working directory rather than from the caller's.
pub fn run_hooks_install(root: &Path, db: &Path, workspace: Option<&str>) -> Result<HookOutcome> {
    let repo = GitRepo::discover(root).map_err(GitError::into_failure)?;
    let path = repo.post_commit_hook_path().map_err(GitError::into_failure)?;
    let workspace_id = resolve_workspace(workspace, root)?;
    let absolute_root = root
        .canonicalize()
        .with_context(|| format!("resolving the workspace root {}", root.display()))?;
    // Joined rather than canonicalized: the index need not exist yet, and `canonicalize` requires
    // every component to. An already-absolute `--db` wins outright, since `join` replaces rather than
    // appends when its argument is absolute.
    let absolute_db = absolute_root.join(db);

    // `symlink_metadata` rather than `exists`, which follows symlinks: a dangling symlink at the hook
    // path would read as absent and the write below would follow it, landing outside the hooks
    // directory — the one case where refusing-rather-than-overwriting would do neither.
    if path.symlink_metadata().is_ok() {
        return Err(anyhow!(
            "a hook already exists at {}; refusing to overwrite it — remove it first if you mean to \
             replace it, or add `c10r build` to it by hand",
            path.display()
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let script = post_commit_hook_script(&absolute_root, &absolute_db, workspace_id.as_str());
    std::fs::write(&path, &script).with_context(|| format!("writing hook at {}", path.display()))?;

    // Git hooks are a POSIX-shell mechanism invoked directly by the OS, so the executable bit is a
    // Unix-only concept; there is no Windows equivalent to gate here.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("marking hook executable at {}", path.display()))?;
    }

    Ok(HookOutcome {
        path: path.display().to_string(),
    })
}

/// The rendering of a `hooks install` report: the JSON structured answer under `--json`, or a
/// one-line human summary naming the path written.
pub fn render_hook_report(outcome: &HookOutcome, json: bool) -> String {
    if json {
        serde_json::to_string_pretty(outcome).unwrap_or_else(|_| "{}".to_string())
    } else {
        format!("installed hook at {}", crate::render::sanitize(&outcome.path))
    }
}

/// A hex SHA-256 digest of `text`, the idiom [`crate::graph::content_hash`] and
/// [`crate::query::page::PageIdentity`]'s parameter hash both use — here, for binding an `impact`
/// continuation token to the patch it was seeded from.
fn hex_sha256(text: &str) -> String {
    use std::fmt::Write;
    let digest = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The recorded index identity a continuation token binds to: the source content-hash composed with
/// the recorded analyzer provenance (name and version), joined by a unit separator that cannot appear
/// in either component. A rebuild that changes the sources (a new content-hash) or the analyzer
/// version — extraction, and thus result order, may shift under a new analyzer even over identical
/// sources — invalidates in-flight cursors. Absent metadata yields an empty identity — the query
/// still runs, and a token cannot outlive a since-built index it was never paired with.
fn recorded_index_hash(store: &GraphStore) -> Result<String> {
    Ok(store
        .read_metadata()?
        .map(|m| {
            format!(
                "{}\u{1f}{}\u{1f}{}",
                m.content_hash, m.provenance.analyzer_name, m.provenance.analyzer_version
            )
        })
        .unwrap_or_default())
}

/// The modal teaching error for a content bound (`--max-lines`/`--from`) supplied when no
/// content-bearing detail is in play, mirroring the `--depth`-with-wrong-relation error style: it
/// names the offending flag and the details that accept it.
fn content_bound_misuse(flag: &str) -> Failure {
    Failure::Usage(format!(
        "`{flag}` applies only to content-bearing details; details that accept it: signature, interface, body"
    ))
}

/// The CLI label for a detail level, for a continuation token's parameter identity.
fn detail_label(detail: Detail) -> &'static str {
    match detail {
        Detail::Location => "location",
        Detail::Signature => "signature",
        Detail::Interface => "interface",
        Detail::Body => "body",
    }
}

/// The CLI label for a relation, for the `--depth` teaching error.
fn relation_label(relation: Relation) -> &'static str {
    match relation {
        Relation::Containers => "containers",
        Relation::Contains => "contains",
        Relation::References => "references",
        Relation::Dependents => "dependents",
        Relation::Importers => "importers",
        Relation::Implementers => "implementers",
        Relation::Tests => "tests",
    }
}

/// Parse a `path:byte_offset` position argument.
fn parse_position(arg: &str) -> Result<(String, usize)> {
    let (path, offset) = arg
        .rsplit_once(':')
        .ok_or_else(|| Failure::Usage("position must be `path:byte_offset`".to_string()))?;
    let offset: usize = offset
        .parse()
        .map_err(|_| Failure::Usage(format!("invalid byte offset in {arg}")))?;
    Ok((path.to_string(), offset))
}

fn ensure_parent_dir(db: &Path) -> Result<()> {
    if let Some(parent) = db.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::content_hash;
    use crate::graph::join::JoinAccounting;
    use crate::graph::store::IndexMetadata;
    use crate::query::diff::ChangeKind;

    /// A store recording `workspace_root` as the workspace it describes.
    fn store_recorded_for(workspace_root: &str) -> GraphStore {
        let store = GraphStore::open_in_memory().unwrap();
        store
            .write_metadata(&IndexMetadata {
                workspace_id: WorkspaceId::new("ws"),
                workspace_root: Some(workspace_root.to_string()),
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
        store
    }

    // A comparison that cannot be evaluated — here, a query root that does not resolve — is disclosed
    // as unknown. Reporting nothing would be indistinguishable from a match, which is the one thing
    // an unevaluable comparison must never look like.
    #[test]
    fn an_unevaluable_workspace_comparison_is_disclosed_as_unknown() {
        let store = store_recorded_for("/projects/ws");

        let relation = workspace_relation(&store, Path::new("/no/such/directory/anywhere")).unwrap();

        assert_eq!(
            relation,
            Some(WorkspaceRelation::Unknown),
            "an unresolvable root leaves the relationship unknown, never implied to match"
        );
    }

    // A store with no recorded build cannot be compared either, and is likewise disclosed as unknown.
    #[test]
    fn a_store_without_metadata_is_disclosed_as_unknown() {
        let store = GraphStore::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let relation = workspace_relation(&store, dir.path()).unwrap();

        assert_eq!(relation, Some(WorkspaceRelation::Unknown));
    }

    // A build taking a store over from another workspace announces the handoff naming both roots;
    // rebuilding a store for the workspace it already describes says nothing.
    #[test]
    fn the_workspace_handoff_notice_names_both_roots_only_on_a_handoff() {
        let store = store_recorded_for("/projects/first");

        let notice = workspace_handoff_notice(&store, Some("/projects/second")).expect("a handoff is disclosed");
        assert!(notice.contains("/projects/first"), "names the recorded root: {notice}");
        assert!(notice.contains("/projects/second"), "names the new root: {notice}");

        assert!(
            workspace_handoff_notice(&store, Some("/projects/first")).is_none(),
            "rebuilding in place is not a handoff"
        );
    }

    fn change(kind: ChangeKind, pre_path: Option<&str>, post_path: Option<&str>) -> FileChange {
        FileChange {
            kind,
            pre_path: pre_path.map(str::to_string),
            post_path: post_path.map(str::to_string),
            pre_ranges: Vec::new(),
        }
    }

    // `is_discovered_path` agrees with `collect_sources` over a real temp tree: a source under a
    // plain directory is discovered, one under `target/` or a dotted directory is not, and one with
    // the wrong extension is not.
    #[test]
    fn is_discovered_path_agrees_with_collect_sources() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(dir.path().join("target/b.rs"), "fn b() {}").unwrap();
        std::fs::write(dir.path().join(".hidden/c.rs"), "fn c() {}").unwrap();
        std::fs::write(dir.path().join("src/d.txt"), "not rust").unwrap();

        let collected = collect_rust_sources(dir.path()).unwrap();
        let collected_paths: std::collections::BTreeSet<&str> = collected.iter().map(|(p, _)| p.as_str()).collect();

        let candidates = ["src/a.rs", "target/b.rs", ".hidden/c.rs", "src/d.txt"];
        for path in candidates {
            assert_eq!(
                is_discovered_path(path, Language::Rust),
                collected_paths.contains(path),
                "disagreement on {path}"
            );
        }
        assert_eq!(collected_paths, ["src/a.rs"].into_iter().collect());
    }

    // A file whose bytes are not valid UTF-8 is excluded rather than failing the whole collection;
    // its decodable siblings are still collected.
    #[test]
    fn a_file_not_valid_utf8_is_excluded_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
        // A CP-1251 encoding of a Cyrillic comment: bytes that are not valid UTF-8.
        std::fs::write(dir.path().join("bad.rs"), [b'/', b'/', 0xCF, 0xF0, b'\n']).unwrap();
        std::fs::write(dir.path().join("c.rs"), "fn c() {}").unwrap();

        let collected = collect_rust_sources(dir.path()).expect("collection succeeds despite the bad file");
        let collected_paths: std::collections::BTreeSet<&str> = collected.iter().map(|(p, _)| p.as_str()).collect();

        assert_eq!(
            collected_paths,
            ["a.rs", "c.rs"].into_iter().collect(),
            "the undecodable file is excluded; its decodable siblings are not"
        );
    }

    // A file whose path (not content) is not valid Unicode is excluded rather than risking a
    // collision with another such path once both are lossily rendered to the same placeholder text.
    //
    // The fixture itself gates the test, at runtime rather than by platform: unlike Linux, macOS's
    // filesystem (APFS/HFS+) enforces valid-UTF-8 file names at the OS level and refuses to create
    // one, so this probes the actual capability (can this filesystem hold such a name at all) instead
    // of assuming it from `target_os` — the same shape as the permission probe just above, and it
    // means the test compiles and its skip reason is visible everywhere, rather than the function not
    // existing at all on a platform that can't exercise it.
    #[cfg(unix)]
    #[test]
    fn a_non_unicode_path_is_excluded_not_fatal() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn a() {}").unwrap();
        // A file name that is not valid Unicode: a lone continuation byte cannot start a UTF-8
        // sequence, so no `.rs`-suffixed decoding of these bytes is valid Unicode.
        let bad_name = OsStr::from_bytes(&[0x80, b'.', b'r', b's']);
        if let Err(e) = std::fs::write(dir.path().join(bad_name), "fn bad() {}") {
            eprintln!(
                "skipped a_non_unicode_path_is_excluded_not_fatal: this filesystem refuses a \
                 non-Unicode file name outright ({e}), so the exclusion this test targets cannot \
                 be constructed here — expected on macOS (APFS/HFS+); run on Linux to exercise it"
            );
            return;
        }

        let collected = collect_rust_sources(dir.path()).expect("collection succeeds despite the bad path");
        let collected_paths: std::collections::BTreeSet<&str> = collected.iter().map(|(p, _)| p.as_str()).collect();

        assert_eq!(
            collected_paths,
            ["a.rs"].into_iter().collect(),
            "the non-Unicode-path file is excluded; its decodable sibling is not"
        );
    }

    // A discovered file that cannot be read for a reason other than its encoding still fails
    // collection outright, naming the file.
    #[cfg(unix)]
    #[test]
    fn a_file_unreadable_for_a_non_encoding_reason_still_fails() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let unreadable = dir.path().join("locked.rs");
        std::fs::write(&unreadable, "fn locked() {}").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = std::fs::read_to_string(&unreadable);
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
        if result.is_ok() {
            // Running with privileges that ignore the permission bits (e.g. root) — the read
            // succeeded despite the denied mode, so this environment cannot exercise the case.
            return;
        }

        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        let err = collect_rust_sources(dir.path()).expect_err("an unreadable file fails the collection");
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            err.to_string().contains("locked.rs"),
            "the error names the unreadable file: {err}"
        );
    }

    // A modified file takes its pre-change content; an added file is dropped; a deleted source is
    // restored; a renamed file moves back to its pre-change path; a deleted file under `target/` is
    // not restored; an untracked-in-git file untouched by the change passes through unchanged.
    #[test]
    fn revert_changes_undoes_each_change_kind() {
        let sources = vec![
            ("src/modified.rs".to_string(), "after".to_string()),
            ("src/added.rs".to_string(), "new content".to_string()),
            ("src/renamed_to.rs".to_string(), "renamed content".to_string()),
            ("src/untouched.rs".to_string(), "unchanged".to_string()),
        ];
        let changes = vec![
            change(ChangeKind::Modified, Some("src/modified.rs"), Some("src/modified.rs")),
            change(ChangeKind::Added, None, Some("src/added.rs")),
            change(ChangeKind::Deleted, Some("src/deleted.rs"), None),
            change(ChangeKind::Deleted, Some("target/deleted.rs"), None),
            change(
                ChangeKind::Renamed,
                Some("src/renamed_from.rs"),
                Some("src/renamed_to.rs"),
            ),
        ];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("src/modified.rs".to_string(), "before".to_string());
        pre_contents.insert("src/deleted.rs".to_string(), "deleted content".to_string());
        pre_contents.insert("target/deleted.rs".to_string(), "deleted under target".to_string());
        pre_contents.insert("src/renamed_from.rs".to_string(), "pre-rename content".to_string());

        let reverted = revert_changes(&sources, &changes, &pre_contents, Language::Rust);
        let map: BTreeMap<String, String> = reverted.into_iter().collect();

        assert_eq!(map.get("src/modified.rs"), Some(&"before".to_string()));
        assert_eq!(map.get("src/added.rs"), None, "an added file is dropped");
        assert_eq!(map.get("src/deleted.rs"), Some(&"deleted content".to_string()));
        assert_eq!(
            map.get("target/deleted.rs"),
            None,
            "a deleted file under target/ is not restored — build would never have discovered it"
        );
        assert_eq!(map.get("src/renamed_to.rs"), None, "the post-change path is dropped");
        assert_eq!(map.get("src/renamed_from.rs"), Some(&"pre-rename content".to_string()));
        assert_eq!(map.get("src/untouched.rs"), Some(&"unchanged".to_string()));
    }

    // Reverting a change over a source set whose hash matches a recorded hash: the content hash of
    // the reverted set equals the content hash of the hand-built expected pre-change set.
    #[test]
    fn revert_changes_hash_matches_hand_built_pre_change_set() {
        let sources = vec![
            ("src/modified.rs".to_string(), "after".to_string()),
            ("src/untouched.rs".to_string(), "unchanged".to_string()),
        ];
        let changes = vec![change(
            ChangeKind::Modified,
            Some("src/modified.rs"),
            Some("src/modified.rs"),
        )];
        let mut pre_contents = BTreeMap::new();
        pre_contents.insert("src/modified.rs".to_string(), "before".to_string());

        let reverted = revert_changes(&sources, &changes, &pre_contents, Language::Rust);
        let expected = vec![
            ("src/modified.rs".to_string(), "before".to_string()),
            ("src/untouched.rs".to_string(), "unchanged".to_string()),
        ];
        assert_eq!(content_hash(&reverted), content_hash(&expected));
    }

    // Every value the hook script carries is a path or an identity the caller chose, and any of them
    // may contain a space — the hook is a shell script, so an unquoted space would split one argument
    // into two and point `build` at a path that does not exist. Each of the three is single-quoted.
    #[test]
    fn post_commit_hook_script_shell_quotes_values_containing_a_space() {
        let script = post_commit_hook_script(
            Path::new("/home/dev/my project"),
            Path::new("/home/dev/my project/.c10r/index.db"),
            "my project",
        );
        assert_eq!(
            script,
            "#!/bin/sh\nc10r build '/home/dev/my project' --db '/home/dev/my project/.c10r/index.db' \
             --workspace 'my project'\n"
        );
    }
}
