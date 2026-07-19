//! Command handlers backing the CLI: the query commands (`get`, `trace`, `find`) and the
//! operational set (`build`, `status`, `doctor`, `cache`).
//!
//! These are thin orchestration over the query engine and the ingest path, kept out of `main` so
//! they are testable without spawning a process.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::exit::Failure;
use crate::graph::join::JoinAccounting;
use crate::graph::store::GraphStore;
use crate::graph::syntax::Language;
use crate::graph::{content_hash, ingest};
use crate::identity::WorkspaceId;
use crate::query::output::Answer;
use crate::query::page::{PageIdentity, apply_dependents_pagination, apply_pagination, precheck_cursor};
use crate::query::{Detail, QueryEngine, Relation};
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts, ExtractedIndex};
use crate::semantic::probe::PROBE_DEADLINE;
use crate::semantic::python_adapter::{PythonAdapter, environment_facts, resolve_environment};
use crate::semantic::{SemanticEngine, SemanticError, rust_adapter::RustAdapter};

/// Collect `(workspace_relative_path, source_text)` for every `.rs` file under `root`.
///
/// Paths are relative to `root` and use `/` separators to match SCIP document paths. `target/` and
/// hidden directories are skipped.
pub fn collect_rust_sources(root: &Path) -> Result<Vec<(String, String)>> {
    collect_sources(root, "rs", &["target"])
}

/// Collect `(workspace_relative_path, source_text)` for every `.py` file under `root`.
///
/// Paths are relative to `root` and use `/` separators to match SCIP document paths. `venv/` and
/// hidden directories (`.venv/` included) are skipped — the environment's own sources are not the
/// workspace's.
pub fn collect_python_sources(root: &Path) -> Result<Vec<(String, String)>> {
    collect_sources(root, "py", &["venv"])
}

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
/// file is reported as an absent index before the connection is opened (the connection would
/// otherwise create the file). A store present but written under a different schema version surfaces
/// through [`GraphStore::open`]'s typed guard. A schema-stamped store that carries no build metadata —
/// a never-completed build, its tables created but never populated — is likewise reported as an absent
/// index rather than answered as an empty one, since no build has actually run against it.
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
    let is_python = recorded
        .as_ref()
        .is_some_and(|m| m.provenance.analyzer_name == PythonAdapter::analyzer_name());

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
pub fn run_build(
    db: &Path,
    workspace: Option<&str>,
    root: &Path,
    rust_analyzer: &str,
    scip_python: &str,
    environment: Option<&Path>,
    language: Option<Language>,
) -> Result<crate::graph::join::JoinAccounting> {
    let workspace_id = resolve_workspace(workspace, root)?;
    let language = detect_language(language, root)?;
    let (index, sources): (ExtractedIndex, Vec<(String, String)>) = match language {
        Language::Rust => {
            let adapter = RustAdapter::new(rust_analyzer)
                .map_err(|e| Failure::IndexerSetup(format!("rust-analyzer unavailable: {e}")))?;
            let index = adapter.analyze(root).map_err(|e| anyhow!("indexing failed: {e}"))?;
            (index, collect_rust_sources(root)?)
        }
        Language::Python => {
            // $VIRTUAL_ENV is read here, at the outer edge; resolution itself is pure.
            let virtual_env = std::env::var_os("VIRTUAL_ENV").map(PathBuf::from);
            let environment = resolve_environment(environment, virtual_env.as_deref(), root)
                .map_err(|e| Failure::IndexerSetup(format!("{e}")))?;
            let adapter = PythonAdapter::new(scip_python, environment, workspace_id.as_str())
                .map_err(|e| Failure::IndexerSetup(format!("{e}")))?;
            let index = adapter.analyze(root).map_err(|e| anyhow!("indexing failed: {e}"))?;
            (index, collect_python_sources(root)?)
        }
    };
    ensure_parent_dir(db)?;
    // The write path replaces an incompatible store outright: the index is derived, replayable
    // data, so rebuild is the migration.
    let mut store = GraphStore::open_or_replace(db).context("opening index database")?;
    let accounting = ingest(&mut store, &workspace_id, &index, &sources).map_err(|e| anyhow!("ingest failed: {e}"))?;
    Ok(accounting)
}

/// The one-line `build` accounting summary: every per-rule acceptance bucket inside the
/// parentheses (their sum is the `aligned=` total), followed by the refusal and syntax-only counts.
///
/// Every [`crate::graph::join::AlignmentRule`] bucket MUST render here:
/// `accounting_line_renders_every_bucket` sums the parenthesized buckets against
/// [`JoinAccounting::aligned_total`], so a bucket added to the accounting without a render site
/// fails that test instead of silently vanishing from the build output.
pub fn build_accounting_line(accounting: &JoinAccounting) -> String {
    format!(
        "built: aligned={} (exact={} crate_root={} operator_desugar={} module_span={} self_keyword={} \
         module_name={} self_name={} module_marker={} import_alias={} range_literal={} use_list_self={} \
         super_keyword={}) text_mismatch={} semantic_only={} duplicate_ambiguous={} syntax_only={}",
        accounting.aligned_total(),
        accounting.aligned_exact,
        accounting.aligned_crate_root,
        accounting.aligned_operator_desugar,
        accounting.aligned_module_span,
        accounting.aligned_self_keyword,
        accounting.aligned_module_name,
        accounting.aligned_self_name,
        accounting.aligned_module_marker,
        accounting.aligned_import_alias,
        accounting.aligned_range_literal,
        accounting.aligned_use_list_self,
        accounting.aligned_super_keyword,
        accounting.text_mismatch,
        accounting.semantic_only,
        accounting.duplicate_ambiguous,
        accounting.syntax_only
    )
}

/// The structured `build` accounting: the machine projection `--json build` emits, carrying every
/// per-rule acceptance bucket, their aligned total, and the refusal counts.
///
/// Derived from the same [`JoinAccounting`] the human [`build_accounting_line`] renders, and mirroring
/// the bucket names `run_status` uses in its `join_alignment` block so the two machine views agree.
pub fn build_accounting_json(accounting: &JoinAccounting) -> serde_json::Value {
    serde_json::json!({
        "aligned": {
            "exact": accounting.aligned_exact,
            "crate_root": accounting.aligned_crate_root,
            "operator_desugar": accounting.aligned_operator_desugar,
            "module_span": accounting.aligned_module_span,
            "self_keyword": accounting.aligned_self_keyword,
            "module_name": accounting.aligned_module_name,
            "self_name": accounting.aligned_self_name,
            "module_marker": accounting.aligned_module_marker,
            "import_alias": accounting.aligned_import_alias,
            "range_literal": accounting.aligned_range_literal,
            "use_list_self": accounting.aligned_use_list_self,
            "super_keyword": accounting.aligned_super_keyword,
            "total": accounting.aligned_total(),
        },
        "text_mismatch": accounting.text_mismatch,
        "semantic_only": accounting.semantic_only,
        "duplicate_ambiguous": accounting.duplicate_ambiguous,
        "syntax_only": accounting.syntax_only,
    })
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
            // Per-rule acceptance buckets alongside the refusal counts.
            "aligned": {
                "exact": meta.accounting.aligned_exact,
                "crate_root": meta.accounting.aligned_crate_root,
                "operator_desugar": meta.accounting.aligned_operator_desugar,
                "module_span": meta.accounting.aligned_module_span,
                "self_keyword": meta.accounting.aligned_self_keyword,
                "module_name": meta.accounting.aligned_module_name,
                "self_name": meta.accounting.aligned_self_name,
                "module_marker": meta.accounting.aligned_module_marker,
                "import_alias": meta.accounting.aligned_import_alias,
                "range_literal": meta.accounting.aligned_range_literal,
                "use_list_self": meta.accounting.aligned_use_list_self,
                "super_keyword": meta.accounting.aligned_super_keyword,
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

/// The current index state for `manifest`'s orientation block: whether an index has been built, and
/// — when one has — its workspace identity and freshness. Reuses the same metadata/freshness read
/// `status` performs, kept small (no join-accounting detail; that is `status`'s job).
///
/// An agent orienting on arrival may not have built an index yet, so a missing or unreadable index
/// answers `{"built": false}` rather than failing `manifest` itself: unlike a query, `manifest` does
/// not need an index to answer.
pub fn index_state(db: &Path, root: &Path, rust_analyzer: &str) -> serde_json::Value {
    if !db.exists() {
        return serde_json::json!({ "built": false });
    }
    read_index_state(db, root, rust_analyzer).unwrap_or_else(|_| serde_json::json!({ "built": false }))
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
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: Some(from),
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?;
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
        limit: effective_limit,
        max_lines: effective_max_lines,
        from: None,
        index_hash,
    };
    if matches!(relation, Relation::Dependents) {
        let answer = engine.dependents(reference, depth.unwrap_or(1), detail, effective_max_lines)?;
        // Dependents pages its detailed rows under the limit; the depth/horizon/disclosure/beyond-bound
        // summary is repeated on every page as context.
        let answer = apply_dependents_pagination(answer, effective_limit, cursor, &identity)?;
        return Ok(render(&answer, json, styled));
    }
    let answer = engine.trace(reference, relation, detail, effective_max_lines)?;
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?;
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
    let (provenance, hash, environment) = current_state(&store, root, rust_analyzer)?;
    let engine = QueryEngine::new(&store, provenance, hash, environment);
    let answer = engine.find(fragment)?;
    let identity = PageIdentity {
        command: "find",
        reference: fragment.to_string(),
        relation: None,
        detail: None,
        depth: None,
        limit: effective_limit,
        max_lines: None,
        from: None,
        index_hash,
    };
    let answer = apply_pagination(answer, effective_limit, cursor, &identity)?;
    Ok(render(&answer, json, styled))
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
/// false`. Removal applies only to a recognizable index store: the target must be a SQLite database
/// carrying a nonzero `user_version` stamp (the schema-version stamp every `c10r build` writes), so a
/// mis-pointed `--db` cannot unlink an unrelated file — that target is refused with a teaching message
/// naming the manual alternative. An empty (0-byte) file is a failed-create artifact and stays
/// removable. When an index is removed, its SQLite WAL/SHM sidecar files (present under concurrent
/// access) are removed alongside it on a best-effort basis — their absence or removal failure never
/// fails the command, since the primary database file is the index's identity. An OS error removing
/// the primary file is surfaced to the caller, naming the path.
pub fn run_cache(db: &Path) -> Result<CacheOutcome> {
    if !db.exists() {
        return Ok(CacheOutcome {
            path: db.display().to_string(),
            removed: false,
        });
    }
    if !is_removable_index(db)? {
        return Err(anyhow!(
            "{path} is not a c10r index (missing SQLite header or version stamp); refusing to remove it \
             — if you mean to delete it: rm {path}",
            path = db.display()
        ));
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

/// Whether the file at `db` is a recognizable c10r index store safe to remove.
///
/// A c10r index is a SQLite database — identified by the 16-byte magic header `SQLite format 3\0` —
/// carrying a nonzero `user_version` (the big-endian `u32` at header offset 60, which every
/// `c10r build` stamps with the schema version). A store written under an older schema version still
/// carries a nonzero stamp, so it remains removable — resetting it is the recovery path. An empty
/// (0-byte) file is a failed-create artifact and is removable; any other non-index file — a text file,
/// a SQLite database with a zero `user_version` — is refused so `cache` never unlinks an unrelated
/// file at a mis-pointed `--db` path.
fn is_removable_index(db: &Path) -> Result<bool> {
    // Any I/O error (including a directory at the path) is surfaced naming the path, so a failed
    // inspection reads as a path-named operational failure rather than a bare OS error.
    index_header_decision(db).with_context(|| format!("inspecting index store at {}", db.display()))
}

/// The header-guard decision for [`is_removable_index`], returning the raw I/O result so the caller
/// can attach path context once.
fn index_header_decision(db: &Path) -> std::io::Result<bool> {
    use std::io::Read;

    let mut file = std::fs::File::open(db)?;
    let mut header = [0u8; 64];
    let mut filled = 0;
    loop {
        let n = file.read(&mut header[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
        if filled == header.len() {
            break;
        }
    }
    if filled == 0 {
        // An empty file is a failed-create artifact, not a foreign file: removable.
        return Ok(true);
    }
    if filled < header.len() || &header[0..16] != b"SQLite format 3\0" {
        return Ok(false);
    }
    let user_version = u32::from_be_bytes([header[60], header[61], header[62], header[63]]);
    Ok(user_version != 0)
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
