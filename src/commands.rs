//! Command handlers backing the CLI: `get`, `trace`, `build`, `status`.
//!
//! These are thin orchestration over the query engine and the ingest path, kept out of `main` so
//! they are testable without spawning a process.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::graph::store::GraphStore;
use crate::graph::{content_hash, ingest};
use crate::identity::WorkspaceId;
use crate::query::output::Answer;
use crate::query::{Detail, QueryEngine, Relation};
use crate::semantic::model::{AnalyzerProvenance, ExtractedIndex};
use crate::semantic::{SemanticEngine, rust_adapter::RustAdapter};

/// Collect `(workspace_relative_path, source_text)` for every `.rs` file under `root`.
///
/// Paths are relative to `root` and use `/` separators to match SCIP document paths. `target/` and
/// hidden directories are skipped.
pub fn collect_rust_sources(root: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    collect_dir(root, root, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if name.starts_with('.') || name == "target" {
                continue;
            }
            collect_dir(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            out.push((rel, text));
        }
    }
    Ok(())
}

/// Render an answer, honoring the `--json` flag.
pub fn render<T: serde::Serialize + std::fmt::Debug>(answer: &Answer<T>, json: bool) -> String {
    if json { answer.to_json() } else { format!("{answer:#?}") }
}

/// The provenance and source hash currently in effect for a workspace root, used to mark answers
/// fresh or stale. When no analyzer is available, the recorded provenance is echoed so queries still
/// work against a previously-built index.
fn current_state(store: &GraphStore, root: &Path, rust_analyzer: &str) -> Result<(AnalyzerProvenance, String)> {
    let sources = collect_rust_sources(root)?;
    let hash = content_hash(&sources);
    // Prefer a live analyzer's version; fall back to the recorded provenance.
    let provenance = match RustAdapter::new(rust_analyzer) {
        Ok(adapter) => adapter.provenance(),
        Err(_) => store
            .read_metadata()?
            .map(|m| m.provenance)
            .unwrap_or(AnalyzerProvenance {
                analyzer_name: RustAdapter::analyzer_name().to_string(),
                analyzer_version: "unknown".to_string(),
            }),
    };
    Ok((provenance, hash))
}

/// `build`: (re)build the index for the workspace via the ingest path.
pub fn run_build(
    db: &Path,
    workspace: &str,
    root: &Path,
    rust_analyzer: &str,
) -> Result<crate::graph::join::JoinAccounting> {
    let adapter = RustAdapter::new(rust_analyzer).map_err(|e| anyhow!("rust-analyzer unavailable: {e}"))?;
    let index: ExtractedIndex = adapter.analyze(root).map_err(|e| anyhow!("indexing failed: {e}"))?;
    let sources = collect_rust_sources(root)?;
    ensure_parent_dir(db)?;
    let mut store = GraphStore::open(db).context("opening index database")?;
    let accounting = ingest(&mut store, &WorkspaceId::new(workspace), &index, &sources)
        .map_err(|e| anyhow!("ingest failed: {e}"))?;
    Ok(accounting)
}

/// Build directly from a pre-produced index (used where a live analyzer is unavailable, e.g. tests
/// and the exemplar fixtures).
pub fn build_from_index(
    db: &Path,
    workspace: &str,
    index: &ExtractedIndex,
    sources: &[(String, String)],
) -> Result<()> {
    ensure_parent_dir(db)?;
    let mut store = GraphStore::open(db).context("opening index database")?;
    ingest(&mut store, &WorkspaceId::new(workspace), index, sources).map_err(|e| anyhow!("ingest failed: {e}"))?;
    Ok(())
}

/// `status`: report provenance, freshness, and the join-alignment counts.
pub fn run_status(db: &Path, root: &Path, rust_analyzer: &str, json: bool) -> Result<String> {
    let store = GraphStore::open(db).context("opening index database")?;
    let Some(meta) = store.read_metadata()? else {
        return Ok("no index built".to_string());
    };
    let (provenance, hash) = current_state(&store, root, rust_analyzer)?;
    let freshness = store.freshness(&hash, &provenance)?.expect("metadata present");

    let report = serde_json::json!({
        "workspace": meta.workspace_id.as_str(),
        "provenance": {
            "analyzer_name": meta.provenance.analyzer_name,
            "analyzer_version": meta.provenance.analyzer_version,
        },
        "freshness": match freshness {
            crate::graph::store::Freshness::Fresh => "fresh",
            crate::graph::store::Freshness::StaleContent => "stale_content",
            crate::graph::store::Freshness::StaleVersion => "stale_version",
        },
        "stale": freshness.is_stale(),
        "join_alignment": {
            "aligned": meta.accounting.aligned,
            "text_mismatch": meta.accounting.text_mismatch,
            "semantic_only": meta.accounting.semantic_only,
            "syntax_only": meta.accounting.syntax_only,
        },
    });
    if json {
        Ok(serde_json::to_string_pretty(&report)?)
    } else {
        Ok(format!("{report:#}"))
    }
}

/// `get`: retrieve a symbol at a detail level, by reference or by position.
pub fn run_get(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: Option<&str>,
    at: Option<&str>,
    detail: Detail,
    json: bool,
) -> Result<String> {
    let store = GraphStore::open(db).context("opening index database")?;
    let (provenance, hash) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash);

    if let Some(at) = at {
        let (doc, offset) = parse_position(at)?;
        let answer = engine.get_by_position(&doc, offset, detail)?;
        return Ok(render(&answer, json));
    }
    let reference = reference.ok_or_else(|| anyhow!("get requires a reference or --at position"))?;
    let answer = engine.get(reference, detail)?;
    Ok(render(&answer, json))
}

/// `trace`: return the symbols in a relation to the subject.
pub fn run_trace(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: &str,
    relation: Relation,
    json: bool,
) -> Result<String> {
    let store = GraphStore::open(db).context("opening index database")?;
    let (provenance, hash) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash);
    let answer = engine.trace(reference, relation)?;
    Ok(render(&answer, json))
}

/// Parse a `path:byte_offset` position argument.
fn parse_position(arg: &str) -> Result<(String, usize)> {
    let (path, offset) = arg
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("position must be `path:byte_offset`"))?;
    let offset: usize = offset
        .parse()
        .with_context(|| format!("invalid byte offset in {arg}"))?;
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
