//! Backend contract tests for the semantic-engine port: the mandatory extraction contract, the
//! conformance gate, provenance, and queryable feature detection.

mod support;

use std::path::Path;

use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind};
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
        duplicate_groups: Vec::new(),
        targets: Default::default(),
        environment: None,
        local_occurrence_ranges: Default::default(),
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

// _(Optional capabilities are queryable)_ — a backend declaring enclosure is reported as supplying
// it.
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

// _(Same-descriptor definitions extracted as distinct symbols)_ — a backend that merges two
// definitions under one descriptor has them split when its output is taken through the backend
// contract (`analyze`), not only when `normalize` is called directly: the port-boundary wiring
// carries the contract for every backend. The fixture backend stands in for any SCIP adapter here.
#[test]
fn merged_twins_are_split_through_the_backend_contract() {
    let descriptor = Descriptor::new("c", vec![DescriptorSegment::new("Widget", SegmentKind::Type)]);
    let occ = |path: &str, line: u32, role: OccurrenceRole| ExtractedOccurrence {
        document_path: path.to_string(),
        range: SourceRange::new(line, 0, line, 1),
        role,
    };
    // One symbol carrying two definitions (the merged shape a raw adapter produces) plus a reference.
    let merged = ExtractedIndex {
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
        symbols: vec![ExtractedSymbol {
            descriptor: Some(descriptor.clone()),
            kind: SymbolKind::Type,
            class: SymbolClass::InWorkspace,
            occurrences: vec![
                occ("a.rs", 0, Definition),
                occ("b.rs", 0, Definition),
                occ("a.rs", 1, Reference),
            ],
        }],
        duplicate_groups: Vec::new(),
        targets: Default::default(),
        environment: None,
        local_occurrence_ranges: Default::default(),
    };

    // Take the index through the backend contract surface, exactly as ingestion does.
    let engine = FixtureEngine::new(merged);
    let extracted = engine.analyze(Path::new(".")).expect("fixture backend analyzes");

    // The twins arrive split — one symbol per definition, each anchored at its own location, neither
    // carrying the reference.
    assert_eq!(
        extracted.symbols.len(),
        2,
        "one symbol per definition through the contract"
    );
    let mut docs: Vec<&str> = extracted
        .symbols
        .iter()
        .map(|s| {
            s.definition()
                .expect("each twin keeps its definition")
                .document_path
                .as_str()
        })
        .collect();
    docs.sort();
    assert_eq!(docs, vec!["a.rs", "b.rs"], "twins anchored at their own definitions");
    for twin in &extracted.symbols {
        assert!(
            twin.occurrences.iter().all(|o| o.role == Definition),
            "no twin carries a reference through the contract"
        );
    }

    // The reference survives in the group-addressed collection, assigned to no single twin.
    assert_eq!(
        extracted.duplicate_groups.len(),
        1,
        "one group for the duplicated descriptor"
    );
    let group = &extracted.duplicate_groups[0];
    assert_eq!(group.descriptor, descriptor);
    assert_eq!(group.occurrences.len(), 1, "the reference moved to the group");
    assert!(group.occurrences.iter().all(|o| o.role == Reference));
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

// _(Module classification stays in the Python adapter)_ — the Rust translation reports every
// symbol's kind itself; module classification (a module-only descriptor is a module) would change
// none of them, and the Rust path does not apply it.
#[test]
fn rust_translation_kinds_are_not_reclassified() {
    use silent_cartographer::semantic::python_adapter::classify_module_kinds;
    use silent_cartographer::semantic::rust_adapter::translate_index;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exemplar.scip");
    let bytes = std::fs::read(&path).expect("checked-in exemplar.scip fixture present");
    let index: scip::types::Index = protobuf::Message::parse_from_bytes(&bytes).expect("valid SCIP protobuf");
    let provenance = AnalyzerProvenance {
        analyzer_name: index.metadata.tool_info.name.clone(),
        analyzer_version: index.metadata.tool_info.version.clone(),
    };
    let translated = translate_index(&index, &provenance);

    let mut classified = translated.clone();
    classify_module_kinds(&mut classified);
    let kinds = |index: &silent_cartographer::semantic::model::ExtractedIndex| {
        index.symbols.iter().map(|s| s.kind).collect::<Vec<_>>()
    };
    assert_eq!(kinds(&translated), kinds(&classified));
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

/// One-off generator for the checked-in Python fixture index. Run explicitly to (re)create it:
/// `cargo test --test semantic_engine -- --ignored generate_python_conformance_scip_fixture`
///
/// Generates THROUGH the adapter (its real invocation, project-name/version flags included), never
/// through an ad-hoc tool call, so the committed bytes reflect exactly what a build would parse.
/// Needs `python3` (to create the fixture venv on first run) and `scip-python` on PATH; it refuses
/// with a clear message when either is missing.
#[test]
#[ignore = "fixture generator; needs python3 + scip-python; run explicitly to regenerate tests/fixtures/python-conformance/index.scip"]
fn generate_python_conformance_scip_fixture() {
    use silent_cartographer::semantic::python_adapter::{PythonAdapter, PythonIndexer};

    let root = support::python_fixture_root();
    let venv = root.join(".venv");
    if !venv.is_dir() {
        let status = std::process::Command::new("python3")
            .args(["-m", "venv"])
            .arg(&venv)
            .status()
            .expect("python3 is required to create the fixture venv");
        assert!(status.success(), "python3 -m venv failed");
    }

    let indexer = PythonIndexer::ScipPython;
    let adapter = PythonAdapter::new(indexer, indexer.executable(), venv, "python-conformance")
        .expect("scip-python is required: npm install -g @sourcegraph/scip-python");
    let output = root.join("index.scip");
    adapter
        .write_scip_index(&root, &output)
        .expect("scip-python index runs");
    assert!(output.exists());

    // Record the tool version the committed index was generated with, inside the fixture dir.
    let version = adapter.provenance().analyzer_version;
    std::fs::write(root.join("SCIP-PYTHON-VERSION"), format!("{version}\n")).expect("record tool version");
}

/// One-off generator for the checked-in ty-scip fixture index. Run explicitly to (re)create it:
/// `cargo test --test semantic_engine -- --ignored generate_ty_scip_conformance_fixture`
///
/// Generates THROUGH the adapter, like the scip-python generator. Needs `uv` (to create the fixture
/// venv on first run, since ty-scip refuses an environment that does not exist) and `ty-scip` on PATH.
#[test]
#[ignore = "fixture generator; needs uv + ty-scip; run explicitly to regenerate tests/fixtures/python-conformance/ty-scip.scip"]
fn generate_ty_scip_conformance_fixture() {
    use silent_cartographer::semantic::python_adapter::{PythonAdapter, PythonIndexer};

    let root = support::python_fixture_root();
    let venv = root.join(".venv");
    if !venv.is_dir() {
        let status = std::process::Command::new("uv")
            .args(["venv", "--no-project"])
            .arg(&venv)
            .status()
            .expect("uv is required to create the fixture venv");
        assert!(status.success(), "uv venv failed");
    }

    let indexer = PythonIndexer::TyScip;
    let adapter = PythonAdapter::new(indexer, indexer.executable(), venv, "python-conformance")
        .expect("ty-scip is required: uv tool install ty-scip");
    let output = root.join("ty-scip.scip");
    adapter.write_scip_index(&root, &output).expect("ty-scip index runs");
    assert!(output.exists());

    let version = adapter.provenance().analyzer_version;
    std::fs::write(root.join("TY-SCIP-VERSION"), format!("{version}\n")).expect("record tool version");
}

/// One-off generator for the checked-in Rust fixture index. Run explicitly to (re)create it:
/// `cargo test --release --test semantic_engine generate_rust_conformance_scip_fixture -- --ignored --exact`
///
/// Generates THROUGH the adapter, whose `analyze` writes `index.scip` into the project directory, so
/// the committed bytes reflect exactly what a build would parse. Needs `rust-analyzer` on PATH.
#[test]
#[ignore = "fixture generator; needs rust-analyzer; run explicitly to regenerate tests/fixtures/rust-conformance/index.scip"]
fn generate_rust_conformance_scip_fixture() {
    use silent_cartographer::semantic::SemanticEngine;
    use silent_cartographer::semantic::rust_adapter::RustAdapter;

    let root = support::rust_fixture_root();
    let adapter =
        RustAdapter::new("rust-analyzer").expect("rust-analyzer is required: rustup component add rust-analyzer");
    adapter.analyze(&root).expect("rust-analyzer scip runs");
    assert!(root.join("index.scip").exists());

    // Record the tool version the committed index was generated with, inside the fixture dir.
    let version = adapter.provenance().analyzer_version;
    std::fs::write(root.join("RUST-ANALYZER-VERSION"), format!("{version}\n")).expect("record tool version");
}

/// One-off generator for the unresolved-calls Python fixture indexes. Run explicitly to (re)create them:
/// `cargo test --test semantic_engine -- --ignored generate_unresolved_calls_python_fixture`
///
/// Generates THROUGH the adapter for both indexers, like the python-conformance generators. Needs
/// `uv`, `scip-python`, and `ty-scip` on PATH.
#[test]
#[ignore = "fixture generator; needs uv + scip-python + ty-scip; run explicitly to regenerate the unresolved-calls Python indexes"]
fn generate_unresolved_calls_python_fixture() {
    use silent_cartographer::semantic::python_adapter::{PythonAdapter, PythonIndexer};

    let root = support::unresolved_calls_python_root();
    let venv = root.join(".venv");
    if !venv.is_dir() {
        let status = std::process::Command::new("uv")
            .args(["venv", "--no-project"])
            .arg(&venv)
            .status()
            .expect("uv is required to create the fixture venv");
        assert!(status.success(), "uv venv failed");
    }

    for (indexer, index_file, version_file) in [
        (PythonIndexer::ScipPython, "index.scip", "SCIP-PYTHON-VERSION"),
        (PythonIndexer::TyScip, "ty-scip.scip", "TY-SCIP-VERSION"),
    ] {
        let adapter = PythonAdapter::new(indexer, indexer.executable(), venv.clone(), "unresolved-calls").expect(
            "scip-python (npm install -g @sourcegraph/scip-python) and ty-scip (uv tool install ty-scip) are required",
        );
        let output = root.join(index_file);
        adapter.write_scip_index(&root, &output).expect("indexer runs");
        assert!(output.exists());
        let version = adapter.provenance().analyzer_version;
        std::fs::write(root.join(version_file), format!("{version}\n")).expect("record tool version");
    }
}

/// One-off generator for the unresolved-calls Rust fixture index. Run explicitly to (re)create it:
/// `cargo test --release --test semantic_engine generate_unresolved_calls_rust_fixture -- --ignored --exact`
///
/// Generates THROUGH the adapter. Needs `rust-analyzer` on PATH.
#[test]
#[ignore = "fixture generator; needs rust-analyzer; run explicitly to regenerate tests/fixtures/unresolved-calls/rust/index.scip"]
fn generate_unresolved_calls_rust_fixture() {
    use silent_cartographer::semantic::SemanticEngine;
    use silent_cartographer::semantic::rust_adapter::RustAdapter;

    let root = support::unresolved_calls_rust_root();
    let adapter =
        RustAdapter::new("rust-analyzer").expect("rust-analyzer is required: rustup component add rust-analyzer");
    adapter.analyze(&root).expect("rust-analyzer scip runs");
    assert!(root.join("index.scip").exists());

    let version = adapter.provenance().analyzer_version;
    std::fs::write(root.join("RUST-ANALYZER-VERSION"), format!("{version}\n")).expect("record tool version");
}

/// The committed Python fixture indexes, one per Python indexer.
const PYTHON_FIXTURE_INDEXES: [&str; 2] = ["index.scip", "ty-scip.scip"];

// _(Scenario: Python definition occurrence extracted)_ — from each committed fixture index, a class
// defined in the fixture arrives with its resolved descriptor and a definition-role occurrence
// whose range maps through a declared encoding.
#[test]
fn python_definition_occurrence_extracted() {
    for file in PYTHON_FIXTURE_INDEXES {
        let index = support::python_fixture_index_from(file);
        let widget = index
            .symbols
            .iter()
            .find(|s| s.terminal_name() == Some("Widget"))
            .unwrap_or_else(|| panic!("{file}: Widget symbol present in the committed index"));
        assert!(widget.descriptor.is_some(), "{file}: resolved descriptor present");
        let def = widget
            .definition()
            .unwrap_or_else(|| panic!("{file}: Widget has a definition occurrence"));
        assert_eq!(def.role, OccurrenceRole::Definition);
        assert_eq!(
            def.document_path, "pkg/shapes.py",
            "{file}: the definition maps to its module"
        );
        assert!(
            index.encoding_for(&def.document_path).is_some(),
            "{file}: range is mappable"
        );
    }
}

// _(Scenario: Python reference occurrence extracted)_ and _(Scenario: Each Python adapter passes the
// conformance suite)_ — a symbol used in a different module than the one defining it arrives with a
// reference-role occurrence at the use site, and each committed Python index passes the whole
// contract, same clauses as Rust.
#[test]
fn python_reference_occurrence_extracted() {
    for file in PYTHON_FIXTURE_INDEXES {
        let index = support::python_fixture_index_from(file);
        let widget = index
            .symbols
            .iter()
            .find(|s| s.terminal_name() == Some("Widget"))
            .unwrap_or_else(|| panic!("{file}: Widget symbol present"));
        let cross_module_reference = widget
            .occurrences
            .iter()
            .any(|o| o.role == OccurrenceRole::Reference && o.document_path == "pkg/consumer.py");
        assert!(
            cross_module_reference,
            "{file}: Widget is referenced from the consuming module: {:?}",
            widget.occurrences
        );
        let report = conformance::check_index(&index);
        assert!(
            report.is_conformant(),
            "{file}: python fixture translation passes the suite: {:?}",
            report.violations()
        );
    }
}

// _(Scenario: Backends gate independently)_ — the suite runs the same clauses per backend; a
// deliberately broken backend beside a conforming one is reported non-conformant without affecting
// the conforming backend's verdict.
#[test]
fn nonconformant_backend_does_not_affect_the_other() {
    // A conforming backend (the committed Python fixture through the shared translation).
    let conforming = FixtureEngine::new(support::python_fixture_index());
    // A deliberately broken backend: a non-local symbol with no resolved descriptor.
    let broken_index = ExtractedIndex {
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
        duplicate_groups: Vec::new(),
        targets: Default::default(),
        environment: None,
        local_occurrence_ranges: Default::default(),
    };
    let broken = FixtureEngine::new(broken_index);

    let reports = conformance::run_all(&[
        ("python", &conforming as &dyn SemanticEngine, Path::new(".")),
        ("broken", &broken as &dyn SemanticEngine, Path::new(".")),
    ]);
    let by_label: std::collections::HashMap<&str, bool> =
        reports.iter().map(|(l, r)| (*l, r.is_conformant())).collect();
    assert!(by_label["python"], "the conforming backend stays usable");
    assert!(!by_label["broken"], "the broken backend is gated out");
}

// A local occurrence range is held to the same range clauses as a symbol occurrence: its document
// declares an encoding and the range is well formed.
#[test]
fn local_occurrence_ranges_are_checked_like_occurrence_ranges() {
    let clauses_for = |path: &str, range: SourceRange| {
        let mut index = support::python_fixture_index();
        let declared = index.documents[0].path.clone();
        let path = if path.is_empty() { declared } else { path.to_string() };
        index.local_occurrence_ranges.insert(path, vec![range]);
        conformance::check_index(&index)
            .violations()
            .iter()
            .map(|v| v.clause)
            .collect::<Vec<_>>()
    };
    assert!(clauses_for("", SourceRange::new(1, 0, 1, 4)).is_empty());
    assert_eq!(clauses_for("", SourceRange::new(2, 5, 1, 0)), ["range-well-formed"]);
    assert_eq!(
        clauses_for("undeclared.py", SourceRange::new(1, 0, 1, 4)),
        ["range-mappable"]
    );
}

// _(Live backend conformance)_ — the real scip-python over the fixture project must match the
// committed index's shape. When the tool (or python3 for the venv) is absent this leg SKIPs
// explicitly, never silently passing.
#[test]
fn python_live_tool_matches_committed_fixture_shape() {
    use silent_cartographer::semantic::python_adapter::{PythonAdapter, PythonIndexer};

    if PythonAdapter::discover_version(PythonIndexer::ScipPython, PythonIndexer::ScipPython.executable()).is_err() {
        eprintln!("SKIP: scip-python not installed");
        return;
    }
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("SKIP: python3 not available to create the fixture venv");
        return;
    }

    // Copy the fixture project into a temp dir and give it a fresh venv, so the live run needs
    // nothing pre-existing in the repo tree.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let fixture = support::python_fixture_root();
    std::fs::copy(fixture.join("pyproject.toml"), root.join("pyproject.toml")).unwrap();
    for (rel, _) in support::python_fixture_sources() {
        std::fs::create_dir_all(root.join(&rel).parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(&rel), root.join(&rel)).unwrap();
    }
    let venv = root.join(".venv");
    let status = std::process::Command::new("python3")
        .args(["-m", "venv"])
        .arg(&venv)
        .status()
        .unwrap();
    assert!(status.success(), "python3 -m venv failed");

    let indexer = PythonIndexer::ScipPython;
    let adapter =
        PythonAdapter::new(indexer, indexer.executable(), venv, "python-conformance").expect("tool present (checked)");
    let live = adapter.analyze(root).expect("live scip-python indexes the fixture");

    // Shape comparison: the live run resolves exactly the committed index's in-workspace symbols.
    let committed = support::python_fixture_index();
    let shape = |index: &ExtractedIndex| -> Vec<String> {
        let mut names: Vec<String> = index
            .symbols
            .iter()
            .filter(|s| s.class == SymbolClass::InWorkspace)
            .filter_map(|s| s.descriptor.as_ref())
            .map(|d| {
                let segments: Vec<&str> = d.segments.iter().map(|seg| seg.name.as_str()).collect();
                format!("{} {}", d.package, segments.join("::"))
            })
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        shape(&live),
        shape(&committed),
        "live scip-python output drifted from the committed fixture shape (tool version: {})",
        adapter.provenance().analyzer_version
    );
    // The live output passes the same conformance clauses.
    let report = conformance::check_index(&live);
    assert!(
        report.is_conformant(),
        "live output conforms: {:?}",
        report.violations()
    );
}

/// The symbols of the single-line occurrences in the raw SCIP document `path` whose range covers the
/// `name` token found inside the first occurrence of `needle` in `source`. Multi-line occurrences
/// (a Rust crate's whole-file module definition) are skipped.
fn symbols_covering(index: &scip::types::Index, path: &str, source: &str, needle: &str, name: &str) -> Vec<String> {
    let at = source.find(needle).unwrap_or_else(|| panic!("{needle:?} in {path}")) + needle.find(name).unwrap();
    let (line, col) = support::line_col(source, at);
    let (line, col, end) = (line as i32, col as i32, col as i32 + name.len() as i32);
    let doc = index
        .documents
        .iter()
        .find(|d| d.relative_path == path)
        .unwrap_or_else(|| panic!("{path} in the index"));
    doc.occurrences
        .iter()
        .filter(|o| matches!(o.range.as_slice(), [l, s, e] if *l == line && *s < end && col < *e))
        .map(|o| o.symbol.clone())
        .collect()
}

fn read_committed_index(root: &Path, file: &str) -> scip::types::Index {
    let bytes = std::fs::read(root.join(file)).expect("committed fixture index present");
    protobuf::Message::parse_from_bytes(&bytes).expect("valid SCIP protobuf")
}

// _(Fixture premise)_ — under each Python indexer, the unannotated-receiver calls have no occurrence
// at the called name and the annotated call has one. The `callback` token's coverage is reported.
#[test]
fn unresolved_calls_python_fixture_premise_holds() {
    let root = support::unresolved_calls_python_root();
    for file in PYTHON_FIXTURE_INDEXES {
        let index = read_committed_index(&root, file);
        let cover = |path: &str, needle: &str, name: &str| {
            let source = std::fs::read_to_string(root.join(path)).unwrap();
            symbols_covering(&index, path, &source, needle, name)
        };
        assert!(
            cover("tests/test_client.py", "client.fetch()", "fetch").is_empty(),
            "{file}: client.fetch() in the test"
        );
        assert!(
            cover("pkg/use.py", "client.drain()", "drain").is_empty(),
            "{file}: client.drain()"
        );
        assert!(
            cover("pkg/use.py", "shared.append(1)", "append").is_empty(),
            "{file}: shared.append(1)"
        );
        assert!(
            cover("pkg/use.py", ").strip()", "strip").is_empty(),
            "{file}: .strip() on a call with a long multi-line argument"
        );
        assert!(
            !cover("pkg/use.py", "return client.fetch()", "fetch").is_empty(),
            "{file}: annotated client.fetch()"
        );
        eprintln!(
            "{file}: callback() covered by {:?}",
            cover("pkg/use.py", "return callback()", "callback")
        );
    }
}

// _(Fixture premise)_ — in the Rust fixture, the calls under `#[cfg(any())]` have no occurrence at the
// called name, and the live calls do.
#[test]
fn unresolved_calls_rust_fixture_premise_holds() {
    let root = support::unresolved_calls_rust_root();
    let index = read_committed_index(&root, "index.scip");
    let source = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let cover = |needle: &str, name: &str| symbols_covering(&index, "src/lib.rs", &source, needle, name);
    assert!(cover("value.render()", "render").is_empty());
    assert!(cover("_built = Widget::build()", "build").is_empty());
    assert!(!cover("widget.render()", "render").is_empty());
    assert!(!cover("widget = Widget::build()", "build").is_empty());
    eprintln!("rust callback() covered by {:?}", cover("    callback()", "callback"));
}

/// Assert that `index` lists the range of the `name` token inside the first occurrence of `needle` in
/// `path`'s `source` as a local occurrence range of that document, and that no persisted symbol
/// carries an occurrence there.
fn assert_local_range_extracted(
    label: &str,
    index: &ExtractedIndex,
    path: &str,
    source: &str,
    needle: &str,
    name: &str,
) {
    let at = source.find(needle).unwrap_or_else(|| panic!("{needle:?} in {path}")) + needle.find(name).unwrap();
    let (line, col) = support::line_col(source, at);
    let range = SourceRange {
        start_line: line,
        start_char: col,
        end_line: line,
        end_char: col + name.len() as u32,
    };
    assert!(
        index
            .local_occurrence_ranges
            .get(path)
            .is_some_and(|ranges| ranges.contains(&range)),
        "{label}: {name} at {range:?} is a local occurrence range of {path}"
    );
    assert!(
        index
            .symbols
            .iter()
            .flat_map(|s| &s.occurrences)
            .all(|o| o.document_path != path || o.range != range),
        "{label}: no symbol occurrence is created at {range:?}"
    );
}

// _(Scenario: Local occurrence ranges extracted)_ — a variable assigned inside a Python function and
// called as `helper()`, under each Python indexer, and a Rust closure parameter called as
// `callback()` are reported as local occurrence ranges of their documents.
#[test]
fn local_occurrence_ranges_extracted() {
    let root = support::unresolved_calls_python_root();
    let source = std::fs::read_to_string(root.join("pkg/use.py")).unwrap();
    for file in PYTHON_FIXTURE_INDEXES {
        let index = support::unresolved_calls_python_index_from(file);
        assert_local_range_extracted(file, &index, "pkg/use.py", &source, "return helper()", "helper");
    }

    let root = support::unresolved_calls_rust_root();
    let source = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let index = support::unresolved_calls_rust_index();
    assert_local_range_extracted("rust", &index, "src/lib.rs", &source, "    callback()", "callback");
}
