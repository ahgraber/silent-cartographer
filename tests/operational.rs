//! Operational tests: `build` produces a queryable index end-to-end, and `status` reports
//! provenance, freshness, and the join-alignment counts.

mod support;

use std::path::Path;

use silent_cartographer::commands::{
    build_accounting_json, build_accounting_line, build_from_index, detect_language, resolve_workspace, run_build,
    run_status,
};
use silent_cartographer::graph::content_hash;
use silent_cartographer::graph::join::JoinAccounting;
use silent_cartographer::graph::store::{DISCREPANCY_GROUP_CAP, GraphStore};
use silent_cartographer::graph::syntax::Language;
use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::{Detail, QueryEngine};
use silent_cartographer::semantic::model::{
    ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding, SourceDocument,
    SourceRange, SymbolClass, SymbolKind,
};
use silent_cartographer::semantic::python_adapter::PythonAdapter;

fn sources() -> Vec<(String, String)> {
    vec![(support::DOC.to_string(), support::SOURCE.to_string())]
}

// _(exercises the join + persistence path end-to-end)_ — `build` over a fixture workspace produces a
// queryable index.
#[test]
fn build_produces_a_queryable_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // Build the index from the exemplar fixture (a live analyzer is not available in this
    // environment; the ingest path is identical to `run_build`'s once an index is produced).
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();
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
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

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
    build_from_index(&db, derived.as_str(), &support::fixture_index(), &sources()).unwrap();
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
    build_from_index(&db, "op-ws", &index, &[("m.rs".to_string(), source.to_string())]).unwrap();
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

// _(Incompatible index stores are replaced or refused — build branch)_ — a build over a store
// stamped with a different schema version succeeds and leaves a store carrying the current version.
#[test]
fn build_replaces_an_incompatible_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_pre_guard_store(&db);

    // The build succeeds despite the incompatible store — replace, not a raw storage failure.
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

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
    write_pre_guard_store(&db);

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
    assert!(chain.contains("version 0"), "names the store's version: {chain}");
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
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

    // A rebuild over the matching store succeeds (write path, no version error).
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

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
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

    let report = run_status(&db, dir.path(), "not-a-real-analyzer", true, false, false, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&report).unwrap();
    let alignment = &v["join_alignment"];

    // Each rule's acceptance bucket is present; the all-exact fixture fills only the default rule.
    let aligned = &alignment["aligned"];
    assert!(
        aligned["exact"].as_u64().unwrap() > 0,
        "exact bucket reported: {report}"
    );
    assert_eq!(aligned["crate_root"].as_u64(), Some(0), "crate-root bucket reported");
    assert_eq!(
        aligned["operator_desugar"].as_u64(),
        Some(0),
        "operator bucket reported"
    );
    assert_eq!(aligned["module_span"].as_u64(), Some(0), "module-span bucket reported");
    assert_eq!(
        aligned["self_keyword"].as_u64(),
        Some(0),
        "self-keyword bucket reported"
    );
    assert_eq!(aligned["module_name"].as_u64(), Some(0), "module-name bucket reported");
    assert_eq!(aligned["self_name"].as_u64(), Some(0), "self-name bucket reported");
    assert_eq!(
        aligned["module_marker"].as_u64(),
        Some(0),
        "module-marker bucket reported"
    );
    assert_eq!(
        aligned["import_alias"].as_u64(),
        Some(0),
        "import-alias bucket reported"
    );
    assert_eq!(
        aligned["range_literal"].as_u64(),
        Some(0),
        "range-literal bucket reported"
    );
    assert_eq!(
        aligned["use_list_self"].as_u64(),
        Some(0),
        "use-list-self bucket reported"
    );
    assert_eq!(
        aligned["super_keyword"].as_u64(),
        Some(0),
        "super-keyword bucket reported"
    );
    assert_eq!(
        aligned["total"].as_u64().unwrap(),
        aligned["exact"].as_u64().unwrap(),
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
    let accounting = JoinAccounting {
        aligned_exact: 1,
        aligned_crate_root: 2,
        aligned_operator_desugar: 3,
        aligned_module_span: 4,
        aligned_self_keyword: 5,
        aligned_module_name: 6,
        aligned_self_name: 7,
        aligned_module_marker: 8,
        aligned_import_alias: 9,
        aligned_range_literal: 10,
        aligned_use_list_self: 11,
        aligned_super_keyword: 12,
        text_mismatch: 13,
        semantic_only: 14,
        duplicate_ambiguous: 15,
        syntax_only: 16,
    };
    let line = build_accounting_line(&accounting);

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
    let accounting = JoinAccounting {
        aligned_exact: 1,
        aligned_crate_root: 2,
        aligned_operator_desugar: 3,
        aligned_module_span: 4,
        aligned_self_keyword: 5,
        aligned_module_name: 6,
        aligned_self_name: 7,
        aligned_module_marker: 8,
        aligned_import_alias: 9,
        aligned_range_literal: 10,
        aligned_use_list_self: 11,
        aligned_super_keyword: 12,
        text_mismatch: 13,
        semantic_only: 14,
        duplicate_ambiguous: 15,
        syntax_only: 16,
    };
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
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

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
    build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();
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
