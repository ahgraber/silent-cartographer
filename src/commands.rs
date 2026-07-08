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

/// `build`: (re)build the index for the workspace via the ingest path.
pub fn run_build(
    db: &Path,
    workspace: Option<&str>,
    root: &Path,
    rust_analyzer: &str,
) -> Result<crate::graph::join::JoinAccounting> {
    let workspace_id = resolve_workspace(workspace, root)?;
    let adapter = RustAdapter::new(rust_analyzer).map_err(|e| anyhow!("rust-analyzer unavailable: {e}"))?;
    let index: ExtractedIndex = adapter.analyze(root).map_err(|e| anyhow!("indexing failed: {e}"))?;
    let sources = collect_rust_sources(root)?;
    ensure_parent_dir(db)?;
    // The write path replaces an incompatible store outright: the index is derived, replayable
    // data, so rebuild is the migration.
    let mut store = GraphStore::open_or_replace(db).context("opening index database")?;
    let accounting = ingest(&mut store, &workspace_id, &index, &sources).map_err(|e| anyhow!("ingest failed: {e}"))?;
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
    // Same write-path replacement policy as `run_build`.
    let mut store = GraphStore::open_or_replace(db).context("opening index database")?;
    ingest(&mut store, &WorkspaceId::new(workspace), index, sources).map_err(|e| anyhow!("ingest failed: {e}"))?;
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
    let store = GraphStore::open(db).context("opening index database")?;
    let Some(meta) = store.read_metadata()? else {
        return Ok("no index built".to_string());
    };
    let (provenance, hash) = current_state(&store, root, rust_analyzer)?;
    let freshness = store.freshness(&hash, &provenance)?.expect("metadata present");
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
        },
        "stale": freshness.is_stale(),
        "join_alignment": {
            // Per-rule acceptance buckets alongside the refusal counts.
            "aligned": {
                "exact": meta.accounting.aligned_exact,
                "crate_root": meta.accounting.aligned_crate_root,
                "operator_desugar": meta.accounting.aligned_operator_desugar,
                "module_span": meta.accounting.aligned_module_span,
                "self_keyword": meta.accounting.aligned_self_keyword,
                "total": meta.accounting.aligned_total(),
            },
            "text_mismatch": meta.accounting.text_mismatch,
            "semantic_only": meta.accounting.semantic_only,
            "duplicate_ambiguous": meta.accounting.duplicate_ambiguous,
            "syntax_only": meta.accounting.syntax_only,
        },
        "duplicated_descriptors": {
            "group_count": duplicated_groups.len(),
        },
    });

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
///
/// `depth` is meaningful only for the `dependents` relation, where it bounds the detailed impact
/// reach (default 1). Supplying it with any other relation is a typed teaching error that names the
/// flag, the offending relation, and the relations that accept it — rather than silently ignoring a
/// meaningless flag.
pub fn run_trace(
    db: &Path,
    root: &Path,
    rust_analyzer: &str,
    reference: &str,
    relation: Relation,
    depth: Option<u32>,
    json: bool,
) -> Result<String> {
    if !matches!(relation, Relation::Dependents) && depth.is_some() {
        return Err(anyhow!(
            "the `--depth` flag applies only to the `dependents` relation, but it was given with `{}`; \
             relations that accept `--depth`: dependents",
            relation_label(relation)
        ));
    }
    let store = GraphStore::open(db).context("opening index database")?;
    let (provenance, hash) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash);
    if matches!(relation, Relation::Dependents) {
        let answer = engine.dependents(reference, depth.unwrap_or(1))?;
        return Ok(render(&answer, json));
    }
    let answer = engine.trace(reference, relation)?;
    Ok(render(&answer, json))
}

/// The CLI label for a relation, for the `--depth` teaching error.
fn relation_label(relation: Relation) -> &'static str {
    match relation {
        Relation::Containers => "containers",
        Relation::Contains => "contains",
        Relation::References => "references",
        Relation::Dependents => "dependents",
    }
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
