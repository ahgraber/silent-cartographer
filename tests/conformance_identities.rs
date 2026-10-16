//! The reference identity lists of the shared conformance projects: each backend's committed index,
//! projected to canonical identities, is held to the project's hand-written `IDENTITIES.txt`.

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use silent_cartographer::graph::chunk::ChunkParams;
use silent_cartographer::graph::ingest_with_params;
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::identity::CanonicalId;
use silent_cartographer::semantic::model::ExtractedIndex;

const WS_PREFIX: &str = "test-ws::";

/// The store kinds the reference lists cover: module, struct or class, function, method, and module
/// global.
const LISTED_KINDS: [&str; 5] = ["module", "type", "function", "method", "constant"];

/// The identities in a project's `IDENTITIES.txt`, as listed (no workspace prefix).
fn reference_list(project: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(project.join("IDENTITIES.txt"))
        .expect("the project carries a reference list")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Project `index` over `sources` into a store file and return the store and its in-workspace
/// identities of the listed kinds, workspace prefix removed.
fn projected(index: &ExtractedIndex, sources: &[(String, String)], dir: &Path) -> (GraphStore, BTreeSet<String>) {
    let db = dir.join("index.db");
    let mut store = GraphStore::open_or_replace(&db).unwrap();
    ingest_with_params(
        &mut store,
        &support::ws(),
        Some("/test-ws"),
        index,
        sources,
        &ChunkParams::default(),
    )
    .unwrap();
    let conn = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn
        .prepare("SELECT canonical_id, kind FROM symbols WHERE class = 'in_workspace'")
        .unwrap();
    let identities = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .filter(|(_, kind)| LISTED_KINDS.contains(&kind.as_str()))
        .map(|(id, _)| {
            id.strip_prefix(WS_PREFIX)
                .expect("workspace-prefixed identity")
                .to_string()
        })
        .collect();
    (store, identities)
}

/// The Python identities the index file `file` of the Python conformance project yields, with the
/// store they were projected into. A function of the index file, so any Python backend's committed
/// index can be held to the same list.
fn python_identities_of(file: &str, dir: &Path) -> (GraphStore, BTreeSet<String>) {
    projected(
        &support::python_fixture_index_from(file),
        &support::python_fixture_sources(),
        dir,
    )
}

// _(scip-python yields the Python reference identities)_ — every module, class, function, method, and
// module global the committed scip-python index reports has exactly its listed identity, every listed
// identity is reported, and the reference to `Widget` from `consumer.py` resolves to the listed one.
#[test]
fn scip_python_yields_python_reference_identities() {
    let dir = tempfile::tempdir().unwrap();
    let (store, reported) = python_identities_of("index.scip", dir.path());
    assert_eq!(reported, reference_list(&support::python_fixture_root()));

    let widget = CanonicalId::from_raw("test-ws::python-conformance::pkg::shapes::Widget");
    let references = store.references_of(&widget).unwrap();
    assert!(
        references.iter().any(|r| r.document_path == "pkg/consumer.py"),
        "the reference from consumer.py resolves to the listed Widget: {references:?}"
    );
}

/// The property `Widget.size`, which the project defines twice (getter and setter).
const TWICE_DEFINED: &str = "python-conformance::pkg::shapes::Widget::size";

/// The instance attribute `Widget._size`, which only scip-python reports.
const SCIP_PYTHON_ONLY: &str = "python-conformance::pkg::shapes::Widget::_size";

// _(ty-scip yields the Python reference identities)_ — every identity the committed ty-scip index reports
// with one definition is listed, and every listed identity is reported, except the twice-defined property
// and the attribute only scip-python reports, which the requirement does not cover. The reference to
// `Widget` from `consumer.py` resolves to the listed one.
#[test]
fn ty_scip_yields_python_reference_identities() {
    let dir = tempfile::tempdir().unwrap();
    let (store, reported) = python_identities_of("ty-scip.scip", dir.path());
    let single_definitions: BTreeSet<String> = reported
        .into_iter()
        .filter(|id| !id.starts_with(TWICE_DEFINED))
        .collect();
    let mut expected = reference_list(&support::python_fixture_root());
    expected.remove(TWICE_DEFINED);
    expected.remove(SCIP_PYTHON_ONLY);
    assert_eq!(single_definitions, expected);

    let widget = CanonicalId::from_raw("test-ws::python-conformance::pkg::shapes::Widget");
    let references = store.references_of(&widget).unwrap();
    assert!(
        references.iter().any(|r| r.document_path == "pkg/consumer.py"),
        "the reference from consumer.py resolves to the listed Widget: {references:?}"
    );
}

// _(A name defined twice has one identity per definition under ty-scip)_ — scip-python reports the getter
// and setter of `Widget.size` as one definition and ty-scip as two, so the property has one identity in
// the scip-python store and two distinct identities in the ty-scip store.
#[test]
fn name_defined_twice_has_one_identity_per_definition_under_ty_scip() {
    let size_identities = |file: &str| -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let (_, reported) = python_identities_of(file, dir.path());
        reported
            .into_iter()
            .filter(|id| id.starts_with(TWICE_DEFINED))
            .collect()
    };
    assert_eq!(size_identities("index.scip"), [TWICE_DEFINED]);
    let ty_scip = size_identities("ty-scip.scip");
    assert_eq!(ty_scip.len(), 2, "one identity per definition: {ty_scip:?}");
    assert!(!ty_scip.contains(&TWICE_DEFINED.to_string()), "{ty_scip:?}");
}

// _(rust-analyzer yields the Rust reference identities)_ — every module, struct, function, and method
// the committed rust-analyzer index reports, in the library, the binary, and the tests, has exactly its
// listed identity, every listed identity is reported, and `libbin::util::load()` resolves to the
// library's `load`.
#[test]
fn rust_analyzer_yields_rust_reference_identities() {
    let dir = tempfile::tempdir().unwrap();
    let (store, reported) = projected(
        &support::rust_fixture_index(),
        &support::rust_fixture_sources(),
        dir.path(),
    );
    assert_eq!(reported, reference_list(&support::rust_fixture_root()));

    let library_load = CanonicalId::from_raw("test-ws::libbin::util::load");
    let references = store.references_of(&library_load).unwrap();
    assert!(
        references.iter().any(|r| r.document_path == "src/bin/tool.rs"),
        "the qualified call in the binary resolves to the library's load: {references:?}"
    );
}
