//! Operational tests: `build` produces a queryable index end-to-end, and `status` reports
//! provenance, freshness, and the join-alignment counts.

mod support;

use std::path::Path;

use silent_cartographer::commands::{build_from_index, run_status};
use silent_cartographer::graph::content_hash;
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::{Detail, QueryEngine};

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
    let engine = QueryEngine::new(&store, support::provenance(), content_hash(&sources()));
    let answer = engine.get("net::Client::connect", Detail::Body).unwrap();
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
    let report = run_status(&db, dir.path(), "definitely-not-a-real-analyzer", true).unwrap();

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

// The reindex doorbell hook exists, is executable, and invokes `c10r build`.
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
