//! Backend contract tests for the semantic-engine port: the mandatory extraction contract, the
//! conformance gate, provenance, and queryable feature detection.

mod support;

use std::path::Path;

use silent_cartographer::semantic::conformance;
use silent_cartographer::semantic::fixture::FixtureEngine;
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, OccurrenceRole::*,
    PositionEncoding, SourceDocument, SourceRange, SymbolClass, SymbolKind,
};
use silent_cartographer::semantic::{Capabilities, SemanticEngine};

// _(Mandatory extraction contract)_ — a declaration yields the symbol with a definition-role
// occurrence whose range maps to the declaration.
#[test]
fn definition_occurrence_extracted() {
    let index = support::fixture_index();
    let client = index
        .symbols
        .iter()
        .find(|s| s.terminal_name() == Some("Client"))
        .expect("Client symbol present");
    let def = client.definition().expect("Client has a definition occurrence");
    assert_eq!(def.role, OccurrenceRole::Definition);
    // Range maps to a declared-encoding document.
    assert!(index.encoding_for(&def.document_path).is_some());
}

// _(Mandatory extraction contract)_ — a use site yields a reference-role occurrence with a range.
#[test]
fn reference_occurrence_extracted() {
    let index = support::fixture_index();
    let client = index
        .symbols
        .iter()
        .find(|s| s.terminal_name() == Some("Client"))
        .unwrap();
    let has_reference = client.occurrences.iter().any(|o| o.role == OccurrenceRole::Reference);
    assert!(has_reference, "Client has at least one reference occurrence");
}

// _(Mandatory extraction contract)_ — the conformance suite passes a conformant backend.
#[test]
fn conformant_backend_passes_the_suite() {
    let engine = FixtureEngine::new(support::fixture_index());
    let report = conformance::run(&engine, Path::new("."));
    assert!(
        report.is_conformant(),
        "conformant backend flagged: {:?}",
        report.violations()
    );
}

// _(Mandatory extraction contract)_ — a backend that violates a clause is reported non-conformant
// and not treated as usable.
#[test]
fn non_conformant_backend_is_rejected() {
    // A backend that produces a non-local symbol with no resolved descriptor violates the contract.
    let broken = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "src/a.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: None,
            kind: SymbolKind::Type,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: "src/a.rs".to_string(),
                range: SourceRange::new(0, 0, 0, 1),
                role: Definition,
            }],
        }],
    };
    let engine = FixtureEngine::new(broken);
    let report = conformance::run(&engine, Path::new("."));
    assert!(!report.is_conformant(), "broken backend was treated as conformant");
    assert!(
        report.violations().iter().any(|v| v.clause == "resolved-descriptor"),
        "expected a resolved-descriptor violation, got {:?}",
        report.violations()
    );
}

// _(Mandatory extraction contract)_ — an unavailable backend is non-conformant, not usable.
#[test]
fn unavailable_backend_is_non_conformant() {
    let engine = FixtureEngine::failing(support::provenance(), "tool not installed");
    let report = conformance::run(&engine, Path::new("."));
    assert!(!report.is_conformant());
}

// _(Backend provenance reporting)_ — an ingested index records the analyzer name and version.
#[test]
fn index_records_analyzer_provenance() {
    let engine = FixtureEngine::new(support::fixture_index());
    let index = engine.analyze(Path::new(".")).unwrap();
    let expected: AnalyzerProvenance = support::provenance();
    assert_eq!(index.provenance, expected);
    assert_eq!(index.provenance.analyzer_name, "rust-analyzer");
    assert!(!index.provenance.analyzer_version.is_empty());
}

// _(Optional capabilities are queryable)_ — a backend declaring enclosure is detected and relied
// upon.
#[test]
fn declared_capability_is_present() {
    let engine = FixtureEngine::new(support::fixture_index()).with_capabilities(Capabilities::none().with_enclosure());
    assert!(engine.capabilities().supplies_enclosure());
}

// _(Optional capabilities are queryable)_ — a backend not declaring base-index eligibility is not
// used as a base index and its absence is explicit.
#[test]
fn undeclared_capability_is_not_assumed() {
    let engine = FixtureEngine::new(support::fixture_index()).with_capabilities(Capabilities::none().with_enclosure());
    // Enclosure is declared, base-index eligibility is not; the latter must read as explicitly
    // absent rather than inferred present.
    assert!(engine.capabilities().supplies_enclosure());
    assert!(!engine.capabilities().base_index_eligible());
}

// _(Optional capabilities are queryable — live-updates arm)_ — the live-updates capability is
// queryable: declared it reads present, undeclared it reads explicitly absent.
#[test]
fn live_updates_capability_is_queryable() {
    // A backend declaring live updates is detected present.
    let live =
        FixtureEngine::new(support::fixture_index()).with_capabilities(Capabilities::none().with_live_updates());
    assert!(live.capabilities().live_updates());

    // A backend not declaring it (the batch Rust adapter's shape) reads as explicitly absent —
    // never inferred present.
    let batch = FixtureEngine::new(support::fixture_index())
        .with_capabilities(Capabilities::none().with_enclosure().with_base_index());
    assert!(
        !batch.capabilities().live_updates(),
        "undeclared live-updates is explicit absence"
    );
}

/// Build the exemplar SCIP index (mirroring `support::SOURCE`) as `scip::types::Index`.
///
/// Symbol strings use rust-analyzer's SCIP shape; ranges are the byte-exact columns of the shared
/// fixture source.
fn exemplar_scip_index() -> scip::types::Index {
    fn occurrence(range: Vec<i32>, symbol: &str, definition: bool) -> scip::types::Occurrence {
        let mut occ = scip::types::Occurrence::new();
        occ.range = range;
        occ.symbol = symbol.to_string();
        occ.symbol_roles = if definition { 1 } else { 0 };
        occ
    }

    const NET: &str = "rust-analyzer cargo mycrate 0.1.0 net/";
    const CLIENT: &str = "rust-analyzer cargo mycrate 0.1.0 net/Client#";
    const CONNECT: &str = "rust-analyzer cargo mycrate 0.1.0 net/Client#connect().";
    const DISCONNECT: &str = "rust-analyzer cargo mycrate 0.1.0 net/Client#disconnect().";
    const OPEN: &str = "rust-analyzer cargo mycrate 0.1.0 net/open().";

    let mut doc = scip::types::Document::new();
    doc.relative_path = support::DOC.to_string();
    doc.language = "rust".to_string();
    doc.position_encoding = scip::types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into();
    doc.occurrences = vec![
        occurrence(vec![0, 4, 7], NET, true),
        occurrence(vec![1, 15, 21], CLIENT, true),
        occurrence(vec![2, 9, 15], CLIENT, false),
        occurrence(vec![6, 21, 27], CLIENT, false),
        occurrence(vec![7, 16, 22], CLIENT, false),
        occurrence(vec![3, 15, 22], CONNECT, true),
        occurrence(vec![8, 19, 26], CONNECT, false),
        occurrence(vec![4, 15, 25], DISCONNECT, true),
        occurrence(vec![6, 11, 15], OPEN, true),
    ];

    let mut tool = scip::types::ToolInfo::new();
    tool.name = "rust-analyzer".to_string();
    tool.version = "1.85.0-fixture".to_string();
    let mut metadata = scip::types::Metadata::new();
    metadata.tool_info = protobuf::MessageField::some(tool);

    let mut index = scip::types::Index::new();
    index.metadata = protobuf::MessageField::some(metadata);
    index.documents = vec![doc];
    index
}

/// One-off generator for the checked-in fixture. Run explicitly to (re)create it:
/// `cargo test --test semantic_engine -- --ignored generate_exemplar_scip_fixture`
#[test]
#[ignore = "fixture generator; run explicitly to regenerate tests/fixtures/exemplar.scip"]
fn generate_exemplar_scip_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exemplar.scip");
    scip::write_message_to_file(&path, exemplar_scip_index()).expect("write fixture");
    assert!(path.exists());
}

// _(Mandatory extraction contract — exemplar write-site)_ — a checked-in SCIP index fixture run
// through the Rust adapter's translation passes the backend conformance suite.
#[test]
fn exemplar_scip_fixture_translates_and_passes_conformance() {
    use silent_cartographer::semantic::model::SymbolClass as Class;
    use silent_cartographer::semantic::rust_adapter::translate_index;

    // Read and parse the checked-in fixture — the same bytes-on-disk path `analyze` takes after
    // `rust-analyzer scip` has written its index.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exemplar.scip");
    let bytes = std::fs::read(&path).expect("checked-in exemplar.scip fixture present");
    let index: scip::types::Index = protobuf::Message::parse_from_bytes(&bytes).expect("valid SCIP protobuf");

    // Provenance comes from the index's recorded tool info, as the adapter reports it.
    let provenance = AnalyzerProvenance {
        analyzer_name: index.metadata.tool_info.name.clone(),
        analyzer_version: index.metadata.tool_info.version.clone(),
    };
    let translated = translate_index(&index, &provenance);

    // The translation is substantive: the method symbol arrives in-workspace with both roles.
    let connect = translated
        .symbols
        .iter()
        .find(|s| s.terminal_name() == Some("connect"))
        .expect("connect symbol translated");
    assert_eq!(connect.class, Class::InWorkspace);
    assert!(connect.occurrences.iter().any(|o| o.role == OccurrenceRole::Definition));
    assert!(connect.occurrences.iter().any(|o| o.role == OccurrenceRole::Reference));

    // The exemplar output satisfies the mandatory extraction contract through the conformance suite.
    let engine = FixtureEngine::new(translated);
    let report = conformance::run(&engine, Path::new("."));
    assert!(
        report.is_conformant(),
        "exemplar translation must pass the conformance suite: {:?}",
        report.violations()
    );
}
