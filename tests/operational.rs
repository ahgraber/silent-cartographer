//! Operational tests: `build` produces a queryable index end-to-end, and `status` reports
//! provenance, freshness, and the join-alignment counts.

mod support;

use std::path::Path;

use silent_cartographer::commands::{
    build_accounting_json, build_accounting_line, build_from_index, collect_python_sources, collect_rust_sources,
    detect_language, resolve_workspace, run_build, run_status,
};
use silent_cartographer::graph::content_hash;
use silent_cartographer::graph::join::{AlignmentRule, JoinAccounting};
use silent_cartographer::graph::store::{DISCREPANCY_GROUP_CAP, GraphStore};
use silent_cartographer::graph::syntax::Language;
use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::{Detail, QueryEngine};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};
use silent_cartographer::semantic::python_adapter::{PythonAdapter, environment_facts};
use strum::VariantArray as _;

use crate::support::sources;

// _(exercises the join + persistence path end-to-end)_ — `build` over a fixture workspace produces a
// queryable index.
#[test]
fn build_produces_a_queryable_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // Build the index from the exemplar fixture (a live analyzer is not available in this
    // environment; the ingest path is identical to `run_build`'s once an index is produced).
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();
    assert!(db.exists(), "build wrote the index database");

    // The index is queryable: resolve and retrieve a symbol from the freshly built store.
    let store = GraphStore::open(&db).unwrap();
    let engine = QueryEngine::new(&store, support::provenance(), content_hash(&sources()), None);
    let answer = engine.get("net::Client::connect", Detail::Body, None, 1).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Found { .. }),
        "built index answers a query"
    );

    let connect = project_one(
        &WorkspaceId::new("op-ws"),
        &Descriptor::new(
            "mycrate",
            vec![
                DescriptorSegment::new("net", SegmentKind::Module),
                DescriptorSegment::new("Client", SegmentKind::Type),
                DescriptorSegment::new("connect", SegmentKind::Method),
            ],
        ),
    );
    assert!(
        store.symbol(&connect).unwrap().is_some(),
        "the built index contains the expected symbol"
    );
}

// _(Backend provenance reporting; Staleness reflects underlying change; Join alignment accounting —
// CLI surface)_ — `status` reports provenance, the freshness state, and the join-alignment counts.
#[test]
fn status_reports_provenance_freshness_and_alignment() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    // `status` in JSON, with no live analyzer, echoes the recorded provenance and reports counts.
    // The workspace root has no `.rs` files, so the current hash differs from the built hash and the
    // index reads stale — status still reports the recorded provenance and the alignment counts.
    let report = run_status(
        &db,
        dir.path(),
        "definitely-not-a-real-analyzer",
        true,
        false,
        false,
        false,
    )
    .unwrap();

    assert!(
        report.contains("rust-analyzer"),
        "status reports the analyzer provenance: {report}"
    );
    assert!(
        report.contains("join_alignment"),
        "status reports the join-alignment counts: {report}"
    );
    assert!(report.contains("aligned"), "status reports the aligned count");
    // Freshness state is present (fresh or a stale reason).
    assert!(
        report.contains("freshness"),
        "status reports the freshness state: {report}"
    );
    // The recorded semantic-index identity rides status, the chunk parameters included.
    let parsed: serde_json::Value = serde_json::from_str(&report).expect("status emits JSON");
    let semantic = &parsed["semantic_index"];
    assert!(
        semantic["model_identity"].as_str().is_some_and(|m| !m.is_empty()),
        "status discloses the embedding model identity: {report}"
    );
    assert_eq!(
        semantic["chunk_size"].as_u64(),
        Some(silent_cartographer::graph::chunk::DEFAULT_CHUNK_SIZE as u64),
        "status discloses the recorded chunk size: {report}"
    );
    assert_eq!(
        semantic["chunk_overlap"].as_u64(),
        Some(silent_cartographer::graph::chunk::DEFAULT_CHUNK_OVERLAP as u64),
        "status discloses the recorded overlap: {report}"
    );
}

// _(Derived default workspace identity)_ — with no workspace supplied, the identity is derived from
// the workspace root's directory name, and it namespaces every persisted symbol.
#[test]
fn default_workspace_derived_from_root_name() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("my-codebase");
    std::fs::create_dir(&root).unwrap();

    let derived = resolve_workspace(None, &root).unwrap();
    assert_eq!(derived.as_str(), "my-codebase", "derived from the root directory name");

    // The derived identity namespaces every persisted symbol.
    let db = parent.path().join("index.db");
    build_from_index(&db, derived.as_str(), &root, &support::fixture_index(), &sources()).unwrap();
    let store = GraphStore::open(&db).unwrap();
    let connect = project_one(
        &derived,
        &Descriptor::new(
            "mycrate",
            vec![
                DescriptorSegment::new("net", SegmentKind::Module),
                DescriptorSegment::new("Client", SegmentKind::Type),
                DescriptorSegment::new("connect", SegmentKind::Method),
            ],
        ),
    );
    assert!(
        connect.as_str().starts_with("my-codebase::"),
        "identity is namespaced by the derived workspace: {connect}"
    );
    assert!(
        store.symbol(&connect).unwrap().is_some(),
        "the derived-namespace symbol is persisted"
    );
}

// _(Derived default workspace identity)_ — two derivations of the same root are identical.
#[test]
fn default_workspace_is_deterministic() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let first = resolve_workspace(None, &root).unwrap();
    let second = resolve_workspace(None, &root).unwrap();
    assert_eq!(first, second, "the same root always derives the same identity");
}

// _(Derived default workspace identity — supplied branch)_ — an explicit workspace is used verbatim.
#[test]
fn supplied_workspace_overrides_the_default() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("my-codebase");
    std::fs::create_dir(&root).unwrap();
    let resolved = resolve_workspace(Some("explicit-name"), &root).unwrap();
    assert_eq!(
        resolved.as_str(),
        "explicit-name",
        "the supplied identity wins over the derived default"
    );
}

/// Build an index of `n` distinctly-named symbols, each a definition pointing at the single token
/// `x` on line 0 — so each is a text-mismatch with a distinct expected name, i.e. `n` distinct
/// discrepancy groups. Persist it and return the database path (kept alive by the returned tempdir).
fn build_n_discrepancy_groups(n: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let source = "let x = 1;\n";
    let symbols: Vec<ExtractedSymbol> = (0..n)
        .map(|i| ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new(format!("name{i}"), SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            // Points at "x" (line 0, cols 4..5): spells "x", not "name{i}" → text-mismatch.
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(0, 4, 0, 5),
                role: OccurrenceRole::Definition,
            }],
        })
        .collect();
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(
        &db,
        "op-ws",
        dir.path(),
        &index,
        &[("m.rs".to_string(), source.to_string())],
    )
    .unwrap();
    (dir, db)
}

// _(Join discrepancies are inspectable — truncation branch)_ — with more groups than the cap, the
// summary states it is truncated and its totals reflect every persisted discrepancy.
#[test]
fn discrepancy_summary_is_truncated_but_totals_cover_the_full_set() {
    let groups = DISCREPANCY_GROUP_CAP + 5;
    let (_dir, db) = build_n_discrepancy_groups(groups);
    let report = run_status(&db, Path::new("."), "not-a-real-analyzer", true, true, false, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    let disc = &v["discrepancies"];
    assert_eq!(disc["listing"], "summary");
    assert_eq!(
        disc["truncated"], true,
        "more groups than the cap → truncated: {report}"
    );
    // Displayed groups are capped, but the totals summarize the whole persisted set.
    assert_eq!(
        disc["groups"].as_array().unwrap().len(),
        DISCREPANCY_GROUP_CAP,
        "displayed groups are capped"
    );
    assert_eq!(
        disc["total_groups"].as_u64().unwrap(),
        groups as u64,
        "total_groups counts every group, not only the displayed ones"
    );
    assert_eq!(
        disc["total_discrepancies"].as_u64().unwrap(),
        groups as u64,
        "total_discrepancies counts every persisted row"
    );
}

// _(Join discrepancies are inspectable — full-listing branch)_ — the explicit switch returns every
// persisted discrepancy row.
#[test]
fn full_listing_returns_every_row() {
    let groups = DISCREPANCY_GROUP_CAP + 5;
    let (_dir, db) = build_n_discrepancy_groups(groups);
    let report = run_status(&db, Path::new("."), "not-a-real-analyzer", true, false, true, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    let disc = &v["discrepancies"];
    assert_eq!(disc["listing"], "all");
    assert_eq!(
        disc["rows"].as_array().unwrap().len(),
        groups,
        "the full listing returns every persisted discrepancy row, not the bounded summary"
    );
}

// _(Join discrepancies are inspectable — CLI surface)_ — each displayed group carries outcome kind,
// expected token, count, and an exemplar location.
#[test]
fn summary_group_shape_carries_kind_token_count_and_exemplar() {
    let (_dir, db) = build_n_discrepancy_groups(3);
    let report = run_status(&db, Path::new("."), "not-a-real-analyzer", true, true, false, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    let groups = v["discrepancies"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3, "all three groups displayed under the cap");
    for g in groups {
        assert_eq!(g["outcome"], "text_mismatch", "group carries its outcome kind");
        assert!(
            g["expected_name"].as_str().unwrap().starts_with("name"),
            "group carries its expected token: {g}"
        );
        assert_eq!(g["count"].as_u64().unwrap(), 1, "group carries its count");
        let ex = &g["exemplar"];
        assert_eq!(ex["document_path"], "m.rs", "exemplar carries a location document");
        assert!(
            ex["span_end"].as_u64().unwrap() >= ex["span_start"].as_u64().unwrap(),
            "exemplar carries a location span: {ex}"
        );
    }
}

/// Lay down a pre-guard store shaped like the v1 index: an `index_metadata` table missing the
/// current accounting columns and no `PRAGMA user_version` stamp (reads as version 0).
fn write_pre_guard_store(db: &Path) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(
        "CREATE TABLE index_metadata (
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
        INSERT INTO index_metadata VALUES (1, 1, 'old-ws', 'rust-analyzer', '1.0.0', 'deadbeef', 1, 0, 0, 0);",
    )
    .unwrap();
}

/// Lay down a store c10r recognizes as its own — carrying the ownership marker — at a schema version
/// this binary does not write. This is the shape the replace-or-refuse contract governs: replacement
/// reaches c10r's own stores at any version, and nothing else.
fn write_marked_old_store(db: &Path) {
    write_pre_guard_store(db);
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.pragma_update(
        None,
        "application_id",
        silent_cartographer::graph::store::APPLICATION_ID,
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 1i64).unwrap();
}

// _(Incompatible index stores are replaced or refused — build branch)_ — a build over a store c10r
// created under a different schema version succeeds and leaves a store carrying the current version.
#[test]
fn build_replaces_an_incompatible_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_marked_old_store(&db);

    // The build succeeds despite the incompatible store — replace, not a raw storage failure.
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    // The resulting store carries the current schema version and answers queries.
    let conn = rusqlite::Connection::open(&db).unwrap();
    let stamped: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(
        stamped,
        silent_cartographer::graph::schema::SCHEMA_VERSION,
        "the replaced store is stamped with the current schema version"
    );
    drop(conn);
    let store = GraphStore::open(&db).unwrap();
    assert!(
        store.read_metadata().unwrap().is_some(),
        "the replaced store holds the new build"
    );
}

// _(Incompatible index stores are replaced or refused — query branch)_ — a query against a
// mismatched store refuses with a teaching error naming both versions and the recovery action,
// never a storage-level error.
#[test]
fn query_refuses_an_incompatible_store_with_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_marked_old_store(&db);

    let err = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false)
        .expect_err("status against an incompatible store must refuse");

    // The refusal is the typed guard error, not a storage-level failure leaking through.
    let chain = format!("{err:#}");
    assert!(
        err.chain().any(|e| e
            .downcast_ref::<silent_cartographer::graph::store::StoreOpenError>()
            .is_some()),
        "the error is the typed store-open refusal: {chain}"
    );
    assert!(chain.contains("version 1"), "names the store's version: {chain}");
    assert!(
        chain.contains(&format!(
            "version {}",
            silent_cartographer::graph::schema::SCHEMA_VERSION
        )),
        "names the expected version: {chain}"
    );
    assert!(chain.contains("c10r build"), "names the recovery action: {chain}");
    assert!(
        !chain.contains("no column") && !chain.contains("no such"),
        "no storage-level error surfaces: {chain}"
    );
}

// _(Incompatible index stores are replaced or refused — matching branch)_ — a current-version store
// opens and operates normally under every command.
#[test]
fn matching_version_store_operates_normally() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    // A rebuild over the matching store succeeds (write path, no version error).
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    // Read paths operate normally.
    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false).unwrap();
    assert!(report.contains("join_alignment"), "status answers normally: {report}");
    let store = GraphStore::open(&db).unwrap();
    let engine = QueryEngine::new(&store, support::provenance(), content_hash(&sources()), None);
    let answer = engine.get("net::Client::connect", Detail::Location, None, 1).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Found { .. }),
        "queries answer normally against a matching store"
    );
}

// _(Reindex doorbell)_ — the post-commit hook is present, executable, and invokes `c10r build`, so a
// commit reindexes the workspace without a manual trigger.
#[test]
fn reindex_doorbell_hook_is_present_and_invokes_build() {
    let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("hooks/post-commit");
    assert!(hook.exists(), "post-commit reindex doorbell exists");
    let body = std::fs::read_to_string(&hook).unwrap();
    assert!(body.contains("build"), "the doorbell triggers a build");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "the doorbell is executable");
    }
}

// _(Join alignment accounting — CLI surface)_ — `status` reports each alignment rule's acceptance
// count alongside the refusal counts.
#[test]
fn status_reports_per_rule_acceptance_buckets() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();
    let alignment = &v["join_alignment"];

    // Every rule in the vocabulary is reported, keyed by its stored tag — checked against the
    // vocabulary itself rather than a hand-copied list, so a rule added to it is reported here or
    // this fails. The all-exact fixture fills only the default rule; the rest report zero.
    let aligned = alignment["aligned"].as_object().expect("an aligned block");
    for rule in AlignmentRule::VARIANTS {
        assert!(
            aligned.contains_key(rule.tag()),
            "the {} bucket is reported: {report}",
            rule.tag()
        );
    }
    assert_eq!(
        aligned.len(),
        AlignmentRule::VARIANTS.len() + 1,
        "the block carries exactly the rule buckets plus the derived total: {report}"
    );
    assert!(
        aligned["exact"].as_u64().unwrap() > 0,
        "the all-exact fixture fills the default rule: {report}"
    );
    let bucket_sum: u64 = aligned
        .iter()
        .filter(|(name, _)| name.as_str() != "total")
        .map(|(_, value)| value.as_u64().expect("a count"))
        .sum();
    assert_eq!(
        aligned["total"].as_u64().unwrap(),
        bucket_sum,
        "the total is the sum of the rule buckets"
    );

    // The refusal counts sit alongside the acceptance buckets.
    for refusal in ["text_mismatch", "semantic_only", "duplicate_ambiguous", "syntax_only"] {
        assert!(alignment[refusal].is_u64(), "{refusal} reported alongside: {report}");
    }
}

// _(The build line renders every acceptance bucket)_ — with every accounting field non-zero, the
// per-rule buckets inside the rendered line's parentheses sum to its `aligned=` total. A bucket
// added to the accounting without a render site breaks this sum, so a new rule can never silently
// vanish from the build output.
#[test]
fn accounting_line_renders_every_bucket() {
    // Every bucket non-zero and distinct, in rule order. The array length is the rule count, so
    // this literal stops compiling when the vocabulary grows — which is what keeps "every bucket"
    // meaning every bucket.
    let accounting = JoinAccounting::with_counts([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 13, 14, 15, 16);
    let line = build_accounting_line(&accounting);

    // The rendered line in full. Pinned because `build` prints it to a human on every invocation:
    // the bucket order, the parenthesisation, and each label are the output's shape, not incidental.
    assert_eq!(
        line,
        "built: aligned=78 (exact=1 crate_root=2 operator_desugar=3 module_span=4 self_keyword=5 \
         module_name=6 self_name=7 module_marker=8 import_alias=9 range_literal=10 use_list_self=11 \
         super_keyword=12) text_mismatch=13 semantic_only=14 duplicate_ambiguous=15 syntax_only=16"
    );

    let aligned_total: u64 = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("aligned="))
        .expect("the line carries an aligned= total")
        .parse()
        .expect("the aligned= total is a count");
    assert_eq!(aligned_total, accounting.aligned_total(), "the total is the real total");

    let open = line.find('(').expect("the per-rule buckets are parenthesized");
    let close = line.find(')').expect("the per-rule buckets are parenthesized");
    let bucket_sum: u64 = line[open + 1..close]
        .split_whitespace()
        .map(|pair| {
            pair.split('=')
                .nth(1)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("every parenthesized entry is a bucket=count pair: {pair:?}"))
        })
        .sum();
    assert_eq!(
        bucket_sum, aligned_total,
        "the rendered buckets sum to the aligned= total — a bucket missing from the line breaks this: {line}"
    );

    // The refusal and syntax-only counts render alongside.
    for (label, value) in [
        ("text_mismatch", accounting.text_mismatch),
        ("semantic_only", accounting.semantic_only),
        ("duplicate_ambiguous", accounting.duplicate_ambiguous),
        ("syntax_only", accounting.syntax_only),
    ] {
        assert!(
            line.contains(&format!("{label}={value}")),
            "{label} renders on the line: {line}"
        );
    }
}

// _(The `--json build` projection carries every acceptance bucket)_ — with every accounting field
// non-zero, the machine projection's per-rule buckets sum to its aligned total, and the refusal
// counts are present. A bucket added to the accounting without a projection site breaks this sum, so
// a new rule can never silently vanish from `--json build`.
#[test]
fn build_json_projection_carries_every_bucket() {
    // Every bucket non-zero and distinct, in rule order. The array length is the rule count, so
    // this literal stops compiling when the vocabulary grows — which is what keeps "every bucket"
    // meaning every bucket.
    let accounting = JoinAccounting::with_counts([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 13, 14, 15, 16);
    let json = build_accounting_json(&accounting);

    let aligned = json["aligned"]
        .as_object()
        .expect("the projection carries an aligned block");
    assert_eq!(
        json["aligned"]["total"].as_u64(),
        Some(accounting.aligned_total()),
        "the projection's aligned total is the real total"
    );

    // Every per-rule bucket except the derived `total` sums to the aligned total.
    let bucket_sum: u64 = aligned
        .iter()
        .filter(|(name, _)| name.as_str() != "total")
        .map(|(name, value)| {
            value
                .as_u64()
                .unwrap_or_else(|| panic!("bucket {name} is a count: {value}"))
        })
        .sum();
    assert_eq!(
        bucket_sum,
        accounting.aligned_total(),
        "the projection's buckets sum to the aligned total — a bucket missing from it breaks this: {json}"
    );

    // The refusal and syntax-only counts sit alongside the acceptance buckets.
    for (label, value) in [
        ("text_mismatch", accounting.text_mismatch),
        ("semantic_only", accounting.semantic_only),
        ("duplicate_ambiguous", accounting.duplicate_ambiguous),
        ("syntax_only", accounting.syntax_only),
    ] {
        assert_eq!(
            json[label].as_u64(),
            Some(value),
            "{label} is carried in the projection: {json}"
        );
    }
}

// _(Scenario: Rebuilding an unchanged workspace analyzes nothing — the report)_ — the two build
// outcomes render distinctly: a skip's human line names the index current and the explicit rebuild
// option, and the machine projection carries `rebuilt` for both outcomes.
#[test]
fn build_outcome_renders_distinguish_a_skip_from_a_build() {
    let accounting = JoinAccounting::default();

    let skip = silent_cartographer::commands::BuildOutcome::AlreadyCurrent(accounting);
    let line = silent_cartographer::commands::build_outcome_line(&skip);
    assert!(line.contains("already current"), "the skip line says so: {line}");
    assert!(line.contains("--force"), "the skip line names the escape hatch: {line}");
    let json = silent_cartographer::commands::build_outcome_json(&skip);
    assert_eq!(json["rebuilt"], serde_json::Value::Bool(false), "a skip: {json}");

    let built = silent_cartographer::commands::BuildOutcome::Rebuilt(accounting);
    let line = silent_cartographer::commands::build_outcome_line(&built);
    assert!(
        line.starts_with("built:"),
        "a build renders the accounting line: {line}"
    );
    let json = silent_cartographer::commands::build_outcome_json(&built);
    assert_eq!(json["rebuilt"], serde_json::Value::Bool(true), "a build: {json}");
}

/// An index over two documents, each defining `dupcrate::Widget` under an identical descriptor — a
/// duplicated-descriptor group with no group-addressed references (the disclosure surface cares only
/// about the group's shared descriptor and its definitions).
fn duplicated_group_index() -> ExtractedIndex {
    let widget = |doc: &str| ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new("Widget", SegmentKind::Type)],
        )),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: doc.to_string(),
            range: SourceRange::new(0, 7, 0, 13),
            role: OccurrenceRole::Definition,
        }],
    };
    ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![
            SourceDocument {
                path: "a.rs".to_string(),
                encoding: PositionEncoding::Utf8,
            },
            SourceDocument {
                path: "b.rs".to_string(),
                encoding: PositionEncoding::Utf8,
            },
        ],
        symbols: vec![widget("a.rs"), widget("b.rs")],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

// _(Duplicated descriptors are disclosed — retrievable branch)_ — a build with a duplicated
// descriptor discloses the group count in the summary and, with `--duplicates`, the group's shared
// descriptor and member definitions.
#[test]
fn duplicated_groups_are_retrievable_with_descriptor_and_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let a_source = "struct Widget;\n";
    let b_source = "struct Widget;\n";
    build_from_index(
        &db,
        "op-ws",
        dir.path(),
        &duplicated_group_index(),
        &[
            ("a.rs".to_string(), a_source.to_string()),
            ("b.rs".to_string(), b_source.to_string()),
        ],
    )
    .unwrap();

    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    assert_eq!(
        v["duplicated_descriptors"]["group_count"].as_u64(),
        Some(1),
        "the summary discloses one duplicated group: {report}"
    );
    let groups = v["duplicated_descriptors"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "one group detail returned: {report}");
    let group = &groups[0];
    let defs = group["definitions"].as_array().unwrap();
    assert_eq!(
        defs.len(),
        2,
        "both definitions sharing the descriptor are listed: {group}"
    );
    let docs: std::collections::HashSet<&str> = defs.iter().map(|d| d["document_path"].as_str().unwrap()).collect();
    assert_eq!(
        docs,
        std::collections::HashSet::from(["a.rs", "b.rs"]),
        "each definition's own document is disclosed: {group}"
    );
}

// _(Duplicated descriptors are disclosed — no-duplicates branch)_ — a build with no duplicated
// descriptor discloses a definite zero group count, distinct from an unavailable answer.
#[test]
fn no_duplicates_is_a_definite_zero_group_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();

    assert_eq!(
        v["duplicated_descriptors"]["group_count"].as_u64(),
        Some(0),
        "a definite zero, not a missing field: {report}"
    );
    assert_eq!(
        v["duplicated_descriptors"]["groups"].as_array().unwrap().len(),
        0,
        "an explicit empty set of groups: {report}"
    );
}

// _(Duplicated descriptors are disclosed — summary surface)_ — the duplicated-group count is present
// in `status` JSON even without the `--duplicates` detail flag.
#[test]
fn status_json_always_carries_the_duplicated_group_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let a_source = "struct Widget;\n";
    let b_source = "struct Widget;\n";
    build_from_index(
        &db,
        "op-ws",
        dir.path(),
        &duplicated_group_index(),
        &[
            ("a.rs".to_string(), a_source.to_string()),
            ("b.rs".to_string(), b_source.to_string()),
        ],
    )
    .unwrap();

    // No --duplicates flag: the count is still present, but not the per-group detail.
    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(
        v["duplicated_descriptors"]["group_count"].as_u64(),
        Some(1),
        "the group count is always disclosed alongside the join-outcome counts: {report}"
    );
    assert!(
        v["duplicated_descriptors"].get("groups").is_none(),
        "per-group detail is withheld without --duplicates: {report}"
    );
}

/// Write a minimal manifest of the given name into `dir`.
fn write_manifest(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), "# fixture manifest\n").unwrap();
}

// _(Scenario: Single-language workspace selects its backend)_ — a workspace declaring only a
// pyproject.toml selects the Python backend with no explicit selection, and the selected backend's
// analyzer identity (what build records as provenance) is scip-python.
#[test]
fn python_manifest_selects_python_backend() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "pyproject.toml");

    let language = detect_language(None, dir.path()).unwrap();
    assert_eq!(language, Language::Python, "pyproject.toml marks Python");
    assert_eq!(
        PythonAdapter::analyzer_name(),
        "scip-python",
        "the selected backend's provenance identity"
    );

    // Driving `build` down the selected path reaches the Python backend: with the indexer
    // deterministically absent, the refusal names scip-python — evidence of the selection.
    std::fs::create_dir(dir.path().join(".venv")).unwrap();
    let db = dir.path().join("index.db");
    let missing_tool = dir.path().join("no-tools").join("scip-python");
    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        missing_tool.to_str().unwrap(),
        None,
        None,
        false,
        &Default::default(),
    )
    .expect_err("the indexer is absent");
    assert!(
        err.to_string().contains("scip-python"),
        "the build went down the Python path: {err}"
    );
}

// _(Scenario: Two manifests without a selection refuse)_ — both manifests and no explicit selection
// is a typed refusal naming the selection mechanism, and no store is written from the attempt.
#[test]
fn two_manifests_without_selection_refuse() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    write_manifest(dir.path(), "pyproject.toml");

    let db = dir.path().join("index.db");
    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        "scip-python",
        None,
        None,
        false,
        &Default::default(),
    )
    .expect_err("two manifests are ambiguous");
    assert!(
        err.to_string().contains("--language"),
        "the refusal names the selection mechanism: {err}"
    );
    assert!(!db.exists(), "no store is written from the refused attempt");
}

// _(Scenario: Explicit selection overrides detection)_ — with both manifests present, an explicit
// selection chooses the backend; an explicit selection also needs no manifest at all (legacy
// layouts are served by the flag rather than detection heuristics).
#[test]
fn explicit_selection_overrides_detection() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    write_manifest(dir.path(), "pyproject.toml");

    assert_eq!(
        detect_language(Some(Language::Rust), dir.path()).unwrap(),
        Language::Rust
    );
    assert_eq!(
        detect_language(Some(Language::Python), dir.path()).unwrap(),
        Language::Python
    );

    let bare = tempfile::tempdir().unwrap();
    assert_eq!(
        detect_language(Some(Language::Python), bare.path()).unwrap(),
        Language::Python,
        "an explicit selection needs no manifest"
    );

    // And with no manifest and no selection, the refusal says no supported project was detected.
    let err = detect_language(None, bare.path()).expect_err("nothing to detect");
    assert!(
        err.to_string().contains("no supported project"),
        "the refusal is a teaching error: {err}"
    );
}

// _(Scenario: Missing indexer tool refuses with guidance — the store-untouched clause)_ — a build
// attempt that fails on an unavailable tool leaves an existing store's rows and metadata
// byte-identical, on both backends' refusal paths.
#[test]
fn failed_build_leaves_existing_store_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();
    let before = std::fs::read(&db).unwrap();

    // The Rust path, refused on an unavailable rust-analyzer.
    write_manifest(dir.path(), "Cargo.toml");
    let missing_analyzer = dir.path().join("no-tools").join("rust-analyzer");
    run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        missing_analyzer.to_str().unwrap(),
        "scip-python",
        None,
        None,
        false,
        &Default::default(),
    )
    .expect_err("the analyzer is absent");
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "the store is byte-identical after the refused Rust build"
    );

    // The Python path, refused on an unavailable scip-python (explicitly selected so the ambient
    // machine's manifests play no part).
    std::fs::create_dir(dir.path().join(".venv")).unwrap();
    let missing_scip = dir.path().join("no-tools").join("scip-python");
    run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        missing_scip.to_str().unwrap(),
        None,
        Some(Language::Python),
        false,
        &Default::default(),
    )
    .expect_err("the indexer is absent");
    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "the store is byte-identical after the refused Python build"
    );
}

// _(Scenario: Unresolvable environment refuses with guidance — explicit-flag half)_ — an explicit
// `--environment` pointing at a nonexistent path refuses with the typed environment error before
// the tool is even looked up (environment resolution precedes adapter construction) and before any
// store is touched.
#[test]
fn explicit_environment_refusal_precedes_tool_lookup() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "pyproject.toml");

    let db = dir.path().join("index.db");
    let missing_env = dir.path().join("no-such-venv");
    // The scip-python argument is also deliberately absent: seeing the *environment* error proves
    // resolution ran first and the tool was never looked up.
    let missing_tool = dir.path().join("no-tools").join("scip-python");
    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        missing_tool.to_str().unwrap(),
        Some(missing_env.as_path()),
        None,
        false,
        &Default::default(),
    )
    .expect_err("the explicit environment does not exist");
    let message = err.to_string();
    assert!(
        message.contains("environment") && message.contains("no-such-venv"),
        "the typed refusal names the explicit environment problem: {message}"
    );
    assert!(
        !message.contains("scip-python"),
        "the refusal precedes any tool lookup: {message}"
    );
    assert!(!db.exists(), "no store is touched by the refused attempt");
}

/// A workspace directory holding the fixture's single source file at its real path, so a query from
/// it recomputes exactly the content hash the fixture build recorded — which keeps freshness out of
/// the way when a test is about workspace identity rather than drift.
fn workspace_with_fixture_source() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(support::DOC);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, support::SOURCE).unwrap();
    dir
}

/// Retrieve a symbol through the `get` handler, in machine or human form.
fn get_answer(db: &Path, root: &Path, json: bool) -> String {
    silent_cartographer::commands::run_get(
        db,
        root,
        "not-a-real-analyzer",
        Some("net::Client::connect"),
        /* at */ None,
        Detail::Location,
        /* max_lines */ 0,
        /* from */ 1,
        /* max_lines_explicit */ false,
        /* from_explicit */ false,
        /* limit */ 25,
        /* cursor */ None,
        json,
        /* styled */ false,
    )
    .expect("the query answers")
}

// _(Read operations create no store)_ — a query and a status request against a path where no file
// exists both refuse, name the build that would fix it, and leave the path empty. A read that
// silently conjured an empty store would answer later queries from a graph nobody built.
#[test]
fn a_query_or_status_against_a_missing_store_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let query_err = silent_cartographer::commands::run_get(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        Some("net::Client"),
        None,
        Detail::Location,
        0,
        1,
        false,
        false,
        25,
        None,
        true,
        false,
    )
    .expect_err("a query against nothing refuses");
    let status_err = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false)
        .expect_err("a status request against nothing refuses");

    for (what, err) in [("query", query_err), ("status", status_err)] {
        let message = format!("{err:#}");
        assert!(message.contains("c10r build"), "{what} names the remedy: {message}");
        assert_eq!(
            silent_cartographer::exit::classify(&err),
            silent_cartographer::exit::ExitCode::NoIndex,
            "{what} refuses as an absent index: {message}"
        );
    }
    assert!(!db.exists(), "neither read brought a store into being");
}

// _(Stores record and disclose their workspace: matching workspace carries no marker)_ — an answer
// from the workspace its store was built for carries no workspace disclosure at all, in the machine
// answer or the human render.
#[test]
fn a_matching_workspace_carries_no_marker() {
    let dir = workspace_with_fixture_source();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    let machine: serde_json::Value = serde_json::from_str(&get_answer(&db, dir.path(), true)).unwrap();
    assert!(
        machine.get("workspace_relation").is_none(),
        "a matched workspace carries no marker: {machine}"
    );
    assert_eq!(
        machine["stale"], false,
        "the fixture source is on disk, so nothing drifted"
    );

    let human = get_answer(&db, dir.path(), false);
    assert!(
        !human.contains("workspace"),
        "the human render carries no workspace line: {human}"
    );
}

// _(Stores record and disclose their workspace: different workspace carries the marker)_ — an answer
// from a store built for somewhere else carries the mismatch marker, naming the root it was built
// for, in the machine answer and the human render alike. The graph is confidently about another
// project, which is the one thing an unmarked answer would hide.
#[test]
fn a_different_workspace_carries_the_marker_in_both_renderings() {
    let built_in = workspace_with_fixture_source();
    let queried_from = workspace_with_fixture_source();
    let db = built_in.path().join("index.db");
    build_from_index(&db, "op-ws", built_in.path(), &support::fixture_index(), &sources()).unwrap();

    let recorded = std::fs::canonicalize(built_in.path()).unwrap().display().to_string();

    let machine: serde_json::Value = serde_json::from_str(&get_answer(&db, queried_from.path(), true)).unwrap();
    assert_eq!(
        machine["workspace_relation"]["state"], "mismatched",
        "the answer discloses the mismatch: {machine}"
    );
    assert_eq!(
        machine["workspace_relation"]["recorded_root"], recorded,
        "the disclosure names the workspace the store describes: {machine}"
    );

    let human = get_answer(&db, queried_from.path(), false);
    assert!(
        human.contains("different workspace") && human.contains(&recorded),
        "the human render names the mismatch and the recorded root: {human}"
    );
}

// _(Stores record and disclose their workspace: the marker composes with staleness)_ — a store built
// elsewhere whose sources have also drifted carries both the mismatch marker and the staleness flag,
// each independently. Staleness says the graph is behind; the marker says it is about somewhere else,
// and reporting only the first would explain the wrong problem.
#[test]
fn the_mismatch_marker_composes_with_staleness() {
    let built_in = workspace_with_fixture_source();
    let queried_from = workspace_with_fixture_source();
    let db = built_in.path().join("index.db");
    build_from_index(&db, "op-ws", built_in.path(), &support::fixture_index(), &sources()).unwrap();

    // Drift the queried workspace's source away from what the store recorded.
    std::fs::write(
        queried_from.path().join(support::DOC),
        format!("{}\n// drifted\n", support::SOURCE),
    )
    .unwrap();

    let machine: serde_json::Value = serde_json::from_str(&get_answer(&db, queried_from.path(), true)).unwrap();
    assert_eq!(
        machine["workspace_relation"]["state"], "mismatched",
        "the mismatch is disclosed: {machine}"
    );
    assert_eq!(
        machine["stale"], true,
        "the staleness flag stands on its own: {machine}"
    );
    assert_eq!(machine["freshness"], "stale_content", "with its own reason: {machine}");
}

// _(Stores record and disclose their workspace: an unevaluable comparison is disclosed as unknown)_
// — a store that records no workspace root cannot be compared against anything, so its answers say
// the relationship is unknown rather than carrying no marker. Silence is what a match looks like, and
// "I could not tell" must never be served as "they match".
#[test]
fn a_store_recording_no_workspace_is_disclosed_as_unknown() {
    let dir = workspace_with_fixture_source();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    // A root that could not be recorded exactly — what a build over a workspace path that will not
    // convert to text leaves behind.
    {
        let store = GraphStore::open_or_replace(&db).unwrap();
        let mut meta = store.read_metadata().unwrap().expect("the fixture recorded metadata");
        meta.workspace_root = None;
        store.write_metadata(&meta).unwrap();
    }

    let machine: serde_json::Value = serde_json::from_str(&get_answer(&db, dir.path(), true)).unwrap();
    assert_eq!(
        machine["workspace_relation"]["state"], "unknown",
        "the answer discloses that the comparison could not be made: {machine}"
    );
    assert!(
        machine["workspace_relation"].get("recorded_root").is_none(),
        "there is no root to name: {machine}"
    );

    let human = get_answer(&db, dir.path(), false);
    assert!(
        human.contains("could not be determined"),
        "the human render says so too: {human}"
    );
}

// _(Stores record and disclose their workspace: every query family discloses)_ — the disclosure is a
// property of the answer envelope, not of one command, so `trace` (both its plain-relation and its
// `dependents` answer), `find`, and `status` carry it too.
#[test]
fn every_query_family_carries_the_workspace_disclosure() {
    let built_in = workspace_with_fixture_source();
    let queried_from = workspace_with_fixture_source();
    let db = built_in.path().join("index.db");
    build_from_index(&db, "op-ws", built_in.path(), &support::fixture_index(), &sources()).unwrap();
    let root = queried_from.path();

    let trace = silent_cartographer::commands::run_trace(
        &db,
        root,
        "not-a-real-analyzer",
        "net::Client",
        silent_cartographer::query::Relation::Contains,
        None,
        None,
        0,
        false,
        silent_cartographer::query::OrderMode::Ranked,
        false,
        25,
        None,
        true,
        false,
    )
    .expect("trace answers");
    // `dependents` assembles its own answer shape, so it is a separate site the disclosure has to
    // reach — a relation-by-relation check, not a single wiring point.
    let dependents = silent_cartographer::commands::run_trace(
        &db,
        root,
        "not-a-real-analyzer",
        "net::Client::connect",
        silent_cartographer::query::Relation::Dependents,
        Some(1),
        None,
        0,
        false,
        silent_cartographer::query::OrderMode::Ranked,
        false,
        25,
        None,
        true,
        false,
    )
    .expect("dependents answers");
    let find =
        silent_cartographer::commands::run_find(&db, root, "not-a-real-analyzer", "connect", 25, None, true, false)
            .expect("find answers");
    let status = run_status(&db, root, "not-a-real-analyzer", true, false, false, false).expect("status answers");

    for (command, answer) in [
        ("trace", trace),
        ("dependents", dependents),
        ("find", find),
        ("status", status),
    ] {
        let value: serde_json::Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(
            value["workspace_relation"]["state"], "mismatched",
            "{command} discloses the mismatch: {value}"
        );
    }
}

// _(Stores record and disclose their workspace: a build hands a store between workspaces)_ — building
// over a store recorded for a different workspace succeeds and re-records the workspace the store now
// describes, which clears the marker for later queries from there.
#[test]
fn a_build_over_another_workspaces_store_re_records_the_workspace() {
    let first = workspace_with_fixture_source();
    let second = workspace_with_fixture_source();
    let db = second.path().join("index.db");
    build_from_index(&db, "op-ws", first.path(), &support::fixture_index(), &sources()).unwrap();

    build_from_index(&db, "op-ws", second.path(), &support::fixture_index(), &sources()).unwrap();

    let machine: serde_json::Value = serde_json::from_str(&get_answer(&db, second.path(), true)).unwrap();
    assert!(
        machine.get("workspace_relation").is_none(),
        "the rebuild re-recorded the workspace, clearing the marker: {machine}"
    );
}

// _(Store ownership recognition: a redirected path is refused)_ — a `--db` path that resolves through
// a symlink to a database c10r did not create is refused, and the link's target is left
// byte-identical. A symlinked `.c10r` directory is exactly how a mis-pointed path stops looking
// mis-pointed.
#[cfg(unix)]
#[test]
fn a_symlinked_path_to_a_foreign_database_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("someone-elses.db");
    {
        let conn = rusqlite::Connection::open(&target).unwrap();
        conn.execute_batch("CREATE TABLE payroll (x); INSERT INTO payroll VALUES (1);")
            .unwrap();
        conn.pragma_update(None, "user_version", 4i64).unwrap();
    }
    let before = std::fs::read(&target).unwrap();

    let link = dir.path().join("index.db");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let err = build_from_index(&link, "op-ws", dir.path(), &support::fixture_index(), &sources())
        .expect_err("a build through the link refuses");
    assert!(
        err.chain().any(|e| matches!(
            e.downcast_ref::<silent_cartographer::graph::store::StoreOpenError>(),
            Some(silent_cartographer::graph::store::StoreOpenError::UnrecognizedStore { .. })
        )),
        "the refusal is the typed ownership error: {err:#}"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        before,
        "the link's target is untouched"
    );
}

/// Write an executable stub analyzer that answers `--version` with `version` and fails every other
/// invocation, so a test can tell whether a build attempted analysis.
fn write_version_only_stub(dir: &Path, name: &str, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"{version}\"\n  exit 0\nfi\nexit 1\n"),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

/// A workspace whose store already describes it, alongside a stub analyzer that reports the recorded
/// version and fails if asked to analyze.
fn already_built_workspace() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();

    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources()).unwrap();

    let stub = write_version_only_stub(dir.path(), "rust-analyzer", &support::provenance().analyzer_version);
    (dir, db, stub)
}

// _(Scenario: Rebuilding an unchanged workspace analyzes nothing)_ — the stub analyzer fails on any
// invocation other than `--version`, so a build that reports the index already current is a build
// that never analyzed; the store's bytes are unchanged.
#[test]
fn a_build_over_an_unchanged_workspace_analyzes_nothing() {
    let (dir, db, stub) = already_built_workspace();
    let before = std::fs::read(&db).unwrap();

    let outcome = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect("an already-current index is not an error");

    assert!(!outcome.rebuilt(), "the build reports the index already current");
    assert_eq!(std::fs::read(&db).unwrap(), before, "the store is byte-identical");
}

// _(Scenario: A changed chunk parameter rebuilds)_ — the same workspace, sources, analyzer, and
// environment under a different chunk size is not current: the build analyzes (which the stub
// refuses, surfacing as an indexing failure), while the unchanged-parameter arm still skips.
#[test]
fn a_changed_chunk_parameter_rebuilds() {
    use silent_cartographer::graph::chunk::ChunkParams;

    let (dir, db, stub) = already_built_workspace();

    let outcome = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect("unchanged parameters remain current");
    assert!(!outcome.rebuilt(), "the unchanged-parameter arm skips");

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &ChunkParams {
            chunk_size: 256,
            overlap: 0,
        },
    )
    .expect_err("a changed chunk size drives the build to analyze, which the stub refuses");
    assert!(
        err.to_string().contains("indexing failed"),
        "the changed-parameter build attempted analysis: {err}"
    );
}

// _(Scenario: An explicit rebuild ignores currency)_ — with the explicit option the same workspace is
// analyzed, which the stub refuses, so the attempt surfaces as an indexing failure rather than as a
// skip.
#[test]
fn an_explicit_rebuild_analyzes_an_unchanged_workspace() {
    let (dir, db, stub) = already_built_workspace();

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        true,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the forced build attempted analysis: {err}"
    );
}

// _(Scenario: Rebuilding an unchanged workspace analyzes nothing — unreadable-metadata arm)_ — a
// recognized store whose recorded metadata cannot be read is not current: the build analyzes and
// re-records rather than aborting on the corrupt row.
#[test]
fn corrupt_metadata_in_a_recognized_store_rebuilds() {
    let (dir, db, stub) = already_built_workspace();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("UPDATE index_metadata SET environment = 'not json'", [])
            .unwrap();
    }

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "corrupt metadata drove the build to analyze: {err}"
    );
}

// _(Scenario: An edited source rebuilds)_ — editing one source file makes the store stale, so the
// build analyzes rather than reporting the index current.
#[test]
fn an_edited_source_rebuilds() {
    let (dir, db, stub) = already_built_workspace();
    std::fs::write(
        dir.path().join(support::DOC),
        format!("{}\n// edited\n", support::SOURCE),
    )
    .unwrap();

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the changed source drove the build to analyze: {err}"
    );
}

// _(Scenario: A changed analyzer rebuilds)_ — the same sources under a different analyzer version are
// not current, because resolution can differ between analyzer versions.
#[test]
fn a_changed_analyzer_version_rebuilds() {
    let (dir, db, _) = already_built_workspace();
    let newer = write_version_only_stub(dir.path(), "rust-analyzer-next", "9.99.0");

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        newer.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the changed analyzer version drove the build to analyze: {err}"
    );
}

/// A fake Python environment under `dir/.venv`: a version-only `bin/python` stub and a
/// site-packages directory holding one installed distribution — enough for `environment_facts` to
/// resolve an interpreter version and a package fingerprint.
fn write_fake_venv(dir: &Path) -> std::path::PathBuf {
    let venv = dir.join(".venv");
    let bin = venv.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_version_only_stub(&bin, "python", "Python 3.12.0");
    let site = venv.join("lib").join("python3.12").join("site-packages");
    std::fs::create_dir_all(site.join("foo-1.0.dist-info")).unwrap();
    venv
}

// _(Scenario: A changed environment rebuilds)_ — the same sources under the same analyzer but a
// drifted interpreter environment (here, one more installed distribution) are not current, because
// Python resolution depends on the installed packages.
#[test]
fn a_changed_declared_environment_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "pyproject.toml");
    std::fs::write(dir.path().join("app.py"), "x = 1\n").unwrap();
    let venv = write_fake_venv(dir.path());
    let stub = write_version_only_stub(dir.path(), "scip-python", "0.6.0");

    // Record the store exactly as a Python build would: scip-python provenance, the environment
    // facts in effect, and the hash of the sources discovery collects.
    let index = ExtractedIndex {
        provenance: AnalyzerProvenance {
            analyzer_name: "scip-python".to_string(),
            analyzer_version: "0.6.0".to_string(),
        },
        documents: vec![],
        symbols: vec![],
        duplicate_groups: vec![],
        library_roots: Default::default(),
        environment: Some(environment_facts(&venv).unwrap()),
    };
    let db = dir.path().join("index.db");
    let collected = collect_python_sources(dir.path()).unwrap();
    build_from_index(&db, "op-ws", dir.path(), &index, &collected).unwrap();

    // Sanity: with the environment unchanged the build reports the index already current, so the
    // rebuild below is attributable to the environment alone.
    let outcome = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        stub.to_str().unwrap(),
        Some(venv.as_path()),
        None,
        false,
        &Default::default(),
    )
    .expect("an unchanged environment is current");
    assert!(
        !outcome.rebuilt(),
        "the unchanged environment reports the index current"
    );

    // Install one more distribution: the package fingerprint drifts.
    std::fs::create_dir(
        venv.join("lib")
            .join("python3.12")
            .join("site-packages")
            .join("bar-2.0.dist-info"),
    )
    .unwrap();

    let err = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        "rust-analyzer",
        stub.to_str().unwrap(),
        Some(venv.as_path()),
        None,
        false,
        &Default::default(),
    )
    .expect_err("the stub indexer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the changed environment drove the build to analyze: {err}"
    );
}

// _(Scenario: An absent index builds)_ — with no store at the path there is nothing to compare
// against, so the build analyzes.
#[test]
fn an_absent_index_builds() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    let stub = write_version_only_stub(dir.path(), "rust-analyzer", "1.85.0");

    let err = run_build(
        &dir.path().join("absent.db"),
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the absent index drove the build to analyze: {err}"
    );
}

// _(Scenario: An absent index builds — incompatible-store arm)_ — the other way to have no
// compatible store to compare against: one c10r created under a schema version this binary does not
// recognize. It is never reported current; the build analyzes and replaces it, leaving a store at
// the current schema version.
#[test]
fn an_incompatible_store_builds_rather_than_reporting_it_current() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();

    let db = dir.path().join("index.db");
    write_marked_old_store(&db);
    let stub = write_indexing_stub(dir.path(), "rust-analyzer", "1.85.0");

    let outcome = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect("an incompatible store is c10r's own to replace, not a refusal");

    assert!(outcome.rebuilt(), "the incompatible store drove the build to analyze");
    let conn = rusqlite::Connection::open(&db).unwrap();
    let stamped: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(
        stamped,
        silent_cartographer::graph::schema::SCHEMA_VERSION,
        "the replaced store carries the current schema version"
    );
}

// _(Source discovery excludes undecodable files — build completes and stays queryable)_ — a
// workspace holding a source file whose bytes are not valid UTF-8, alongside the fixture's decodable
// document, still builds to a queryable index: `collect_rust_sources` (the same discovery `run_build`
// calls) excludes the undecodable file rather than failing, and the fixture's symbol is retrievable
// exactly as it is from a workspace holding no undecodable file.
#[test]
fn a_build_completes_and_stays_queryable_with_an_undecodable_file_present() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    // A CP-1251 encoding of a Cyrillic comment: bytes that are not valid UTF-8.
    std::fs::write(dir.path().join("src/undecodable.rs"), [b'/', b'/', 0xCF, 0xF0, b'\n']).unwrap();

    let collected = collect_rust_sources(dir.path()).expect("discovery excludes the bad file, not fails");
    assert!(
        collected.iter().all(|(path, _)| path != "src/undecodable.rs"),
        "the undecodable file is excluded from what a build would ingest"
    );

    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &collected).unwrap();

    let store = GraphStore::open(&db).unwrap();
    let engine = QueryEngine::new(&store, support::provenance(), content_hash(&collected), None);
    let answer = engine.get("net::Client::connect", Detail::Body, None, 1).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Found { .. }),
        "a symbol from the decodable sibling is retrievable: {answer:?}"
    );
}

// _(Source discovery excludes undecodable files — build and currency check agree)_ — the store is
// seeded exactly as `build_from_index` above seeds it, over sources `collect_rust_sources` has
// already excluded the undecodable file from. `run_build`'s currency check recollects and rehashes
// through the same discovery, so the index it just built reports current: the build side and the
// currency-check side exclude the same file.
#[test]
fn a_build_and_the_currency_check_agree_on_an_undecodable_file() {
    let dir = tempfile::tempdir().unwrap();
    write_manifest(dir.path(), "Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    std::fs::write(dir.path().join("src/undecodable.rs"), [b'/', b'/', 0xCF, 0xF0, b'\n']).unwrap();

    let collected = collect_rust_sources(dir.path()).unwrap();
    let db = dir.path().join("index.db");
    build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &collected).unwrap();

    let stub = write_version_only_stub(dir.path(), "rust-analyzer", &support::provenance().analyzer_version);
    let outcome = run_build(
        &db,
        Some("op-ws"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect("the seeded index is current");

    assert!(
        !outcome.rebuilt(),
        "the currency check sees the same discovered set the seeding build did"
    );
}

// _(Scenario: A store recorded for another workspace builds)_ — the same sources under a different
// workspace identity produce different canonical identities, so a store recorded for another
// workspace is never current: the build analyzes and re-records rather than skipping.
#[test]
fn a_store_recorded_for_another_workspace_is_not_current() {
    let (dir, db, stub) = already_built_workspace();

    let err = run_build(
        &db,
        Some("a-different-workspace"),
        dir.path(),
        stub.to_str().unwrap(),
        "scip-python",
        None,
        Some(Language::Rust),
        false,
        &Default::default(),
    )
    .expect_err("the stub analyzer refuses to analyze");
    assert!(
        err.to_string().contains("indexing failed"),
        "the workspace mismatch drove the build to analyze: {err}"
    );
}

/// Write an executable stub analyzer that answers `--version` with `version` and, for any other
/// invocation, writes an empty SCIP index to the path after `--output` and succeeds — so a build
/// driven by it runs to completion without a real analyzer.
fn write_indexing_stub(dir: &Path, name: &str, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then\n  echo \"{version}\"\n  exit 0\nfi\n\
             while [ $# -gt 0 ]; do\n\
             \x20 if [ \"$1\" = \"--output\" ]; then\n    : > \"$2\"\n    exit 0\n  fi\n\
             \x20 shift\n\
             done\n\
             exit 1\n"
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

// _(Scenario: A store recorded for another workspace builds)_ — the recorded workspace root is part
// of the same comparison, and the whole outcome holds: the build analyzes rather than reporting the
// index current, discloses the handoff naming both roots, and leaves the store recording the
// workspace it now describes. Driven through the built binary, because the disclosure is a
// stderr-stream guarantee.
#[test]
fn a_store_recorded_at_another_root_builds_discloses_and_re_records() {
    // The original workspace is kept alive for the store it holds, but the build runs against the
    // second one below.
    let (dir, db, _) = already_built_workspace();
    let stub = write_indexing_stub(
        dir.path(),
        "rust-analyzer-indexing",
        &support::provenance().analyzer_version,
    );
    let recorded_root = std::fs::canonicalize(dir.path()).unwrap();

    // A second workspace holding byte-identical sources, so only the recorded root differs.
    let elsewhere = tempfile::tempdir().unwrap();
    write_manifest(elsewhere.path(), "Cargo.toml");
    std::fs::create_dir_all(elsewhere.path().join("src")).unwrap();
    std::fs::write(elsewhere.path().join(support::DOC), support::SOURCE).unwrap();
    let new_root = std::fs::canonicalize(elsewhere.path()).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"))
        .arg("--db")
        .arg(&db)
        .args(["--workspace", "op-ws", "build"])
        .arg(elsewhere.path())
        .arg("--rust-analyzer")
        .arg(&stub)
        .args(["--language", "rust"])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "the handoff build succeeds: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let notice = String::from_utf8_lossy(&out.stderr);
    assert!(
        notice.contains(recorded_root.to_str().unwrap()) && notice.contains(new_root.to_str().unwrap()),
        "the handoff is disclosed naming both roots: {notice}"
    );

    let store = GraphStore::open(&db).unwrap();
    let metadata = store.read_metadata().unwrap().expect("the handoff build re-recorded");
    assert_eq!(
        metadata.workspace_root.as_deref(),
        new_root.to_str(),
        "the store records the workspace it now describes"
    );
}
