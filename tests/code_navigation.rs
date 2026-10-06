//! Code-navigation tests: reference resolution across its forms, `get` at each detail, `trace` over the
//! supported relations, and the calibrated output contract.

mod support;

use silent_cartographer::graph::ingest;
use silent_cartographer::graph::range::{LineColumn, LineColumnSpan};
use silent_cartographer::graph::store::{
    DEPENDENTS_HORIZON, DependencyKind, EdgeKind, GraphStore, OccurrenceRow, PersistedClass, SymbolRow,
};
use silent_cartographer::graph::syntax::Language;
use silent_cartographer::identity::{
    CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one,
};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::resolve::{Resolution, SourcePosition};
use silent_cartographer::query::{Detail, OrderMode, QueryEngine, Relation, TraceRelation};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};

use crate::support::{
    id_of_pkg, line_col, one_doc_index, one_occ_symbol, prepend_path, sources, write_indexing_stub, ws,
};

/// The canonicalized workspace root a directly-ingested fixture store records. These stores are never
/// queried against a real filesystem root, so a stable stand-in keeps the recorded identity out of the
/// way of what each test is asserting.
const WS_ROOT: &str = "/test-ws";

fn built_store() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &support::fixture_index(), &sources()).unwrap();
    store
}

fn engine_over<'a>(store: &'a GraphStore, provenance: AnalyzerProvenance) -> QueryEngine<'a> {
    let hash = silent_cartographer::graph::content_hash(&sources());
    QueryEngine::new(store, provenance, hash, None)
}

/// The identity of a fixture symbol, from its descriptor segments.
fn id_of(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    project_one(&ws(), &Descriptor::new("mycrate", segs))
}

fn connect_id() -> CanonicalId {
    id_of(&[
        ("net", SegmentKind::Module),
        ("Client", SegmentKind::Type),
        ("connect", SegmentKind::Method),
    ])
}

fn client_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module), ("Client", SegmentKind::Type)])
}

/// A store over a documented Rust function (`double`) and an undocumented one (`triple`), for the
/// interface-detail `get` scenarios.
fn tier_store() -> (GraphStore, CanonicalId, CanonicalId) {
    let source = "\
/// Doubles a number.
pub fn double(x: u8) -> u8 {
    x * 2
}

pub fn triple(x: u8) -> u8 {
    x * 3
}
";
    let (dl, dc) = line_col(source, source.find("double").unwrap());
    let (tl, tc) = line_col(source, source.find("triple").unwrap());
    let double = one_occ_symbol(
        "tierscrate",
        &[("double", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        "tiers.rs",
        SourceRange::new(dl, dc, dl, dc + 6),
        OccurrenceRole::Definition,
    );
    let triple = one_occ_symbol(
        "tierscrate",
        &[("triple", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        "tiers.rs",
        SourceRange::new(tl, tc, tl, tc + 6),
        OccurrenceRole::Definition,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("tiers.rs".to_string(), source.to_string())];
    let index = one_doc_index("tiers.rs", vec![double, triple]);
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &src).unwrap();
    (
        store,
        id_of_pkg("tierscrate", &[("double", SegmentKind::Method)]),
        id_of_pkg("tierscrate", &[("triple", SegmentKind::Method)]),
    )
}

/// A store over a single Rust file module whose document opens with a `//!` doc comment, for the
/// module-detail `get` scenarios.
fn module_tier_store() -> (GraphStore, String, CanonicalId) {
    let source = "//! Networking primitives.\npub fn helper() {}\n";
    let module = one_occ_symbol(
        "tierscrate",
        &[("mymod", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        "mymod.rs",
        // One past the final newline (two newlines in `source`): the whole document.
        SourceRange::new(0, 0, 2, 0),
        OccurrenceRole::Definition,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("mymod.rs".to_string(), source.to_string())];
    let index = one_doc_index("mymod.rs", vec![module]);
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &src).unwrap();
    (
        store,
        source.to_string(),
        id_of_pkg("tierscrate", &[("mymod", SegmentKind::Module)]),
    )
}

/// A synthetic Python-language index: `provenance.analyzer_name` set to the Python adapter's name so
/// `ingest` selects the Python backend for the join and tier extraction.
fn py_synthetic_index(path: &str, symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
    ExtractedIndex {
        provenance: AnalyzerProvenance {
            analyzer_name: silent_cartographer::semantic::python_adapter::PythonAdapter::analyzer_name().to_string(),
            analyzer_version: "0".to_string(),
        },
        documents: vec![SourceDocument {
            path: path.to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

/// A store over a docstring-bearing Python function (`greet`), for the Python interface-detail `get`
/// scenario.
fn python_tier_store() -> (GraphStore, CanonicalId) {
    let source = "def greet(name):\n    \"\"\"Greets somebody.\"\"\"\n    return f\"hi {name}\"\n";
    let (line, col) = line_col(source, source.find("greet").unwrap());
    let greet = one_occ_symbol(
        "pkg",
        &[("m", SegmentKind::Module), ("greet", SegmentKind::Term)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        "m.py",
        SourceRange::new(line, col, line, col + 5),
        OccurrenceRole::Definition,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let index = py_synthetic_index("m.py", vec![greet]);
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &src).unwrap();
    (
        store,
        id_of_pkg("pkg", &[("m", SegmentKind::Module), ("greet", SegmentKind::Term)]),
    )
}

/// A store with a method whose body references a constant, for the reference-attribution `trace`
/// scenario: the reference site's enclosing declaration is the method, not the referenced constant.
fn method_reference_store() -> (GraphStore, CanonicalId, CanonicalId) {
    let source = "\
pub struct Registry;

impl Registry {
    pub fn lookup(&self, key: u8) -> u8 {
        key + OFFSET
    }
}

pub const OFFSET: u8 = 1;
";
    let (lookup_l, lookup_c) = line_col(source, source.find("lookup").unwrap());
    let (offset_ref_l, offset_ref_c) = line_col(source, source.find("OFFSET").unwrap());
    let (offset_def_l, offset_def_c) = line_col(source, source.rfind("OFFSET").unwrap());

    let lookup = one_occ_symbol(
        "tierscrate",
        &[("lookup", SegmentKind::Method)],
        SymbolKind::Method,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(lookup_l, lookup_c, lookup_l, lookup_c + 6),
        OccurrenceRole::Definition,
    );
    let offset = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "tierscrate",
            vec![DescriptorSegment::new("OFFSET", SegmentKind::Term)],
        )),
        kind: SymbolKind::Constant,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(offset_def_l, offset_def_c, offset_def_l, offset_def_c + 6),
                role: OccurrenceRole::Definition,
            },
            ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(offset_ref_l, offset_ref_c, offset_ref_l, offset_ref_c + 6),
                role: OccurrenceRole::Reference,
            },
        ],
    };

    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let index = one_doc_index("m.rs", vec![lookup, offset]);
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &src).unwrap();
    (
        store,
        id_of_pkg("tierscrate", &[("lookup", SegmentKind::Method)]),
        id_of_pkg("tierscrate", &[("OFFSET", SegmentKind::Term)]),
    )
}

// _(Symbol reference resolution — canonical path)_ — a qualified name denoting one symbol resolves
// uniquely.
#[test]
fn qualified_name_resolves_uniquely() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    match engine.resolve("net::Client::connect").unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, connect_id()),
        other => panic!("expected unique, got {other:?}"),
    }
}

// _(Symbol reference resolution — ambiguity branch)_ — a shared shortname returns a typed candidate
// set, not an arbitrary pick.
#[test]
fn ambiguous_shortname_returns_candidates() {
    // Two symbols named `connect` in different types → ambiguous shortname.
    let mut index = support::fixture_index();
    index
        .symbols
        .push(silent_cartographer::semantic::model::ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "mycrate",
                vec![
                    DescriptorSegment::new("net", SegmentKind::Module),
                    DescriptorSegment::new("Server", SegmentKind::Type),
                    DescriptorSegment::new("connect", SegmentKind::Method),
                ],
            )),
            kind: silent_cartographer::semantic::model::SymbolKind::Method,
            class: silent_cartographer::semantic::model::SymbolClass::InWorkspace,
            // Reuse connect's definition location; enough to persist the symbol as a distinct identity.
            occurrences: vec![silent_cartographer::semantic::model::ExtractedOccurrence {
                document_path: support::DOC.to_string(),
                range: silent_cartographer::semantic::model::SourceRange::new(3, 15, 3, 22),
                role: silent_cartographer::semantic::model::OccurrenceRole::Definition,
            }],
        });
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &sources()).unwrap();
    let engine = engine_over(&store, support::provenance());
    match engine.resolve("connect").unwrap() {
        Resolution::Ambiguous(rows) => {
            assert_eq!(rows.len(), 2, "both connect symbols returned as candidates");
        }
        other => panic!("expected ambiguous, got {other:?}"),
    }
}

// _(Symbol reference resolution — identity branch)_ — a canonical identity round-trips to exactly
// one symbol.
#[test]
fn canonical_identity_round_trips() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    match engine.resolve(connect_id().as_str()).unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, connect_id()),
        other => panic!("expected unique, got {other:?}"),
    }
}

// _(Symbol retrieval at a chosen detail)_ — `get` at location detail returns the definition file and
// position.
#[test]
fn get_location_returns_definition_position() {
    use silent_cartographer::query::DetailPayload;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Location, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0].payload {
        DetailPayload::Location { location: Some(loc) } => {
            assert_eq!(loc.document_path, support::DOC);
            assert!(loc.span_end > loc.span_start);
        }
        other => panic!("expected a location, got {other:?}"),
    }
}

// _(Symbol retrieval at a chosen detail)_ — `get` at body detail returns the span text byte-for-byte.
#[test]
fn get_body_returns_exact_span_text() {
    use silent_cartographer::query::DetailPayload;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Body, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    match &results[0].payload {
        DetailPayload::Body { body: Some(body) } => assert_eq!(body, "pub fn connect(&self) {}"),
        other => panic!("expected a body, got {other:?}"),
    }
}

// _(Symbol retrieval at a chosen detail)_ — `get` at signature detail returns the signature without
// the body.
#[test]
fn get_signature_returns_signature_without_body() {
    use silent_cartographer::query::DetailPayload;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Signature, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    match &results[0].payload {
        DetailPayload::Signature { signature: Some(sig) } => {
            assert_eq!(sig, "pub fn connect(&self)");
            assert!(!sig.contains('{'), "signature omits the body");
        }
        other => panic!("expected a signature, got {other:?}"),
    }
}

// _(Scenario: Retrieve interface by name)_ — a documented Rust symbol at interface detail returns its
// doc comment together with its signature, without the body.
#[test]
fn get_interface_returns_docs_and_signature_without_body() {
    use silent_cartographer::query::DetailPayload;
    let (store, double_id, _triple_id) = tier_store();
    let engine = dep_engine(&store);
    let answer = engine.get(double_id.as_str(), Detail::Interface, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0].payload {
        DetailPayload::Interface { interface: Some(text) } => {
            assert!(
                text.contains("Doubles a number."),
                "interface carries the doc comment: {text}"
            );
            assert!(
                text.contains("pub fn double(x: u8) -> u8"),
                "interface carries the signature: {text}"
            );
            assert!(!text.contains("x * 2"), "interface omits the body: {text}");
        }
        other => panic!("expected an interface, got {other:?}"),
    }
}

// _(Scenario: Interface without documentation falls back to signature)_ — a symbol with no
// documentation of its own returns its signature at interface detail.
#[test]
fn get_interface_without_documentation_falls_back_to_signature() {
    use silent_cartographer::query::DetailPayload;
    let (store, _double_id, triple_id) = tier_store();
    let engine = dep_engine(&store);

    let signature = engine.get(triple_id.as_str(), Detail::Signature, None, 1).unwrap();
    let Outcome::Found { results: sig_results } = &signature.outcome else {
        panic!("expected found");
    };
    let DetailPayload::Signature {
        signature: Some(sig_text),
    } = &sig_results[0].payload
    else {
        panic!("expected a signature, got {:?}", sig_results[0].payload);
    };

    let interface = engine.get(triple_id.as_str(), Detail::Interface, None, 1).unwrap();
    let Outcome::Found { results: iface_results } = &interface.outcome else {
        panic!("expected found");
    };
    let DetailPayload::Interface {
        interface: Some(iface_text),
    } = &iface_results[0].payload
    else {
        panic!("expected an interface, got {:?}", iface_results[0].payload);
    };

    assert_eq!(
        iface_text, sig_text,
        "an undocumented symbol's interface equals its signature"
    );
}

// `get --json` at interface detail carries the interface content within the serialized payload.
#[test]
fn get_json_interface_detail_carries_interface_content() {
    let (store, double_id, _triple_id) = tier_store();
    let engine = dep_engine(&store);
    let answer = engine.get(double_id.as_str(), Detail::Interface, None, 1).unwrap();
    let json = answer.to_json();
    assert!(
        json.contains("\"detail\": \"interface\""),
        "JSON carries the interface detail tag: {json}"
    );
    assert!(
        json.contains("Doubles a number."),
        "JSON carries the interface content: {json}"
    );
}

// _(Scenario: Retrieve module interface)_ — a module whose document opens with `//!` docs returns its
// signature and the module documentation at interface detail, not the whole document.
#[test]
fn get_module_interface_returns_module_documentation_without_whole_document() {
    use silent_cartographer::query::DetailPayload;
    let (store, source, module_id) = module_tier_store();
    let engine = dep_engine(&store);
    let answer = engine.get(module_id.as_str(), Detail::Interface, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let docs = silent_cartographer::graph::syntax::SyntaxTree::parse(
        &source,
        silent_cartographer::graph::syntax::Language::Rust,
    )
    .unwrap()
    .module_documentation()
    .unwrap();
    let qualified = module_id.as_str().split_once("::").map(|(_, rest)| rest).unwrap();
    let expected = format!("{qualified}\n{}", docs.trim_end());
    match &results[0].payload {
        DetailPayload::Interface { interface: Some(text) } => {
            assert_eq!(
                text, &expected,
                "the interface is the module's signature followed by its documentation"
            );
            assert!(
                !text.contains("pub fn helper"),
                "the interface is not the whole document: {text}"
            );
        }
        other => panic!("expected an interface, got {other:?}"),
    }
}

// _(Scenario: Retrieve module body returns the whole document)_ — a module at body detail returns the
// document byte-for-byte.
#[test]
fn get_module_body_returns_whole_document() {
    use silent_cartographer::query::DetailPayload;
    let (store, source, module_id) = module_tier_store();
    let engine = dep_engine(&store);
    let answer = engine.get(module_id.as_str(), Detail::Body, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0].payload {
        DetailPayload::Body { body: Some(text) } => {
            assert_eq!(text, &source, "the body is the whole document, byte-for-byte");
        }
        other => panic!("expected a body, got {other:?}"),
    }
}

// _(Scenario: Retrieve Python interface)_ — a Python function with a docstring at interface detail
// returns its header and docstring, without the body.
#[test]
fn get_python_interface_returns_header_and_docstring_without_body() {
    use silent_cartographer::query::DetailPayload;
    let (store, greet_id) = python_tier_store();
    let engine = dep_engine(&store);
    let answer = engine.get(greet_id.as_str(), Detail::Interface, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0].payload {
        DetailPayload::Interface { interface: Some(text) } => {
            assert!(
                text.contains("def greet(name):"),
                "interface carries the header: {text}"
            );
            assert!(
                text.contains("Greets somebody."),
                "interface carries the docstring: {text}"
            );
            assert!(!text.contains("return f"), "interface omits the body: {text}");
        }
        other => panic!("expected an interface, got {other:?}"),
    }
}

// _(Symbol retrieval at a chosen detail — position write-site)_ — `get` for a position returns the
// enclosing symbol.
#[test]
fn get_by_position_returns_enclosing_symbol() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // A byte offset inside connect's body. Find "connect(&self) {}" and point just after the brace.
    let idx = support::SOURCE.find("pub fn connect").unwrap();
    let inside = idx + "pub fn connect(&self) {".len(); // inside the method body span
    let answer = engine
        .get_by_position(support::DOC, SourcePosition::Byte(inside), Detail::Location, None, 1)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(
        results[0].symbol.canonical_id,
        connect_id(),
        "position resolves to the enclosing method"
    );
}

// _(Relationship trace — contains write-site)_ — `trace` over `contains` returns exactly the direct
// members.
#[test]
fn trace_contains_returns_direct_members() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client", TraceRelation::Contains, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let names: Vec<&str> = results
        .iter()
        .filter_map(|item| match item {
            silent_cartographer::query::TraceItem::Symbol { symbol, .. } => Some(symbol.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"connect"), "Client contains connect: {names:?}");
    assert!(names.contains(&"disconnect"), "Client contains disconnect: {names:?}");
}

// _(Symbol rows carry their definition location)_ — a `contains` row's `Symbol` variant carries the
// member's own definition location (document path and span), present in both the JSON answer and the
// human row (`at <path>:<start>-<end>`); a member with no persisted span (external) carries no
// location, honestly rendered without the suffix rather than a fabricated one.
#[test]
fn trace_contains_symbol_rows_carry_definition_location() {
    use silent_cartographer::query::TraceItem;

    let store = GraphStore::open_in_memory().unwrap();
    put_dep_symbol(&store, "subject");
    store
        .insert_symbol(&SymbolRow {
            canonical_id: dep_id("member"),
            display_name: "member".to_string(),
            kind: "method".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("m.rs".to_string()),
            span: Some((10, 20)),
            line_column: None,
            header_end: None,
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        })
        .unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: dep_id("ext_member"),
            display_name: "ext_member".to_string(),
            kind: "function".to_string(),
            class: PersistedClass::External,
            document_path: None,
            span: None,
            line_column: None,
            header_end: None,
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        })
        .unwrap();
    store
        .insert_edge(EdgeKind::Contains, &dep_id("subject"), &dep_id("member"))
        .unwrap();
    store
        .insert_edge(EdgeKind::Contains, &dep_id("subject"), &dep_id("ext_member"))
        .unwrap();
    let engine = dep_engine(&store);

    let answer = engine
        .trace("test-ws::subject", TraceRelation::Contains, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 2);

    for item in results {
        match item {
            TraceItem::Symbol { symbol, location, .. } if symbol.name == "member" => {
                let loc = location.as_ref().expect("the in-workspace member carries a location");
                assert_eq!(loc.document_path, "m.rs");
                assert_eq!((loc.span_start, loc.span_end), (10, 20));
            }
            TraceItem::Symbol { symbol, location, .. } if symbol.name == "ext_member" => {
                assert!(location.is_none(), "the external member carries no location, honestly");
            }
            other => panic!("expected a symbol row, got {other:?}"),
        }
    }

    // The same disclosure holds on the JSON answer's own shape.
    let value: serde_json::Value = serde_json::from_str(&answer.to_json()).unwrap();
    let json_results = value["outcome"]["results"].as_array().unwrap();
    let member = json_results.iter().find(|r| r["name"] == "member").unwrap();
    assert_eq!(member["location"]["document_path"], "m.rs");
    assert_eq!(member["location"]["span_start"], 10);
    assert_eq!(member["location"]["span_end"], 20);
    let ext = json_results.iter().find(|r| r["name"] == "ext_member").unwrap();
    assert!(
        ext.get("location").is_none(),
        "the external member's JSON row carries no location key: {ext}"
    );

    // And on the human render: the in-workspace member's row names its location; the external
    // member's row carries no `at <path:span>` suffix.
    let human = silent_cartographer::render::to_human(&answer, false, silent_cartographer::cli::PositionUnit::Byte);
    let member_line = human
        .lines()
        .find(|l| l.contains("member") && !l.contains("ext_member"));
    assert!(
        member_line.is_some_and(|l| l.contains(" at m.rs:10-20")),
        "the human row names the member's location: {human}"
    );
    let ext_line = human.lines().find(|l| l.contains("ext_member")).unwrap();
    assert!(
        !ext_line.contains(" at "),
        "the external member's human row carries no location suffix: {ext_line}"
    );
}

// _(Relationship trace — containers write-site)_ — `trace` over `containers` for a method returns its
// enclosing type.
#[test]
fn trace_containers_returns_enclosing_type() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client::connect", TraceRelation::Containers, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    match &results[0] {
        silent_cartographer::query::TraceItem::Symbol { symbol, .. } => assert_eq!(symbol.canonical_id, client_id()),
        other => panic!("expected a symbol, got {other:?}"),
    }
}

// _(Relationship trace — references write-site)_ — `trace` over `references` returns every reference
// site with none omitted.
#[test]
fn trace_references_returns_all_sites() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // Client is referenced at three sites in the fixture (impl, return type, let binding).
    let answer = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 3, "all Client references returned: {results:?}");
}

// _(Relationship trace — type-subject write-site)_ — `trace` over `references` for a type returns its
// use sites with locations.
#[test]
fn trace_type_references_returns_use_sites_with_locations() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    for item in results {
        match item {
            silent_cartographer::query::TraceItem::Reference { location, .. } => {
                assert_eq!(location.document_path, support::DOC);
                assert!(location.span_end > location.span_start);
            }
            other => panic!("expected a reference, got {other:?}"),
        }
    }
}

// _(Scenario: Default trace rows carry no tier content)_ — `trace` with no detail requested returns
// rows identifying symbol and location, carrying no tier content.
#[test]
fn trace_default_detail_carries_no_content() {
    use silent_cartographer::query::TraceItem;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    for item in results {
        match item {
            TraceItem::Reference { content, .. } => assert!(content.is_none(), "no detail requested: no content"),
            other => panic!("expected a reference, got {other:?}"),
        }
    }
    let json = answer.to_json();
    assert!(
        !json.contains("\"content\""),
        "the default JSON shape carries no content field: {json}"
    );
}

// _(Scenario: Trace at signature detail)_ — `trace` over `references` at signature detail carries
// each row's signature tier.
#[test]
fn trace_signature_detail_carries_tier_content() {
    use silent_cartographer::query::TraceItem;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client", TraceRelation::References, Some(Detail::Signature), None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert!(!results.is_empty(), "Client has reference sites in the fixture");
    for item in results {
        match item {
            TraceItem::Reference {
                content,
                enclosing,
                location,
                ..
            } => {
                // Each row carries exactly the signature tier of its attribution target: the
                // enclosing declaration when one is attributed, the document's module otherwise.
                let expected = match enclosing {
                    Some(id) => store.symbol(id).unwrap().expect("enclosing persisted").signature_text,
                    None => store
                        .module_of_document(&location.document_path)
                        .unwrap()
                        .and_then(|row| row.signature_text),
                };
                assert_eq!(
                    content, &expected,
                    "signature detail carries the attributed declaration's signature tier: {item:?}"
                );
                assert!(content.is_some(), "signature detail carries tier content: {item:?}");
            }
            other => panic!("expected a reference, got {other:?}"),
        }
    }
}

// _(Scenario: Detail does not change the result set)_ — the same trace at two detail levels returns
// the same symbols in the same order.
#[test]
fn trace_detail_does_not_change_the_result_set() {
    use silent_cartographer::query::TraceItem;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let location = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let signature = engine
        .trace("net::Client", TraceRelation::References, Some(Detail::Signature), None)
        .unwrap();

    let sites = |a: &silent_cartographer::query::output::Answer<TraceItem>| {
        let Outcome::Found { results } = &a.outcome else {
            panic!("expected found");
        };
        results
            .iter()
            .map(|item| match item {
                TraceItem::Reference { location, .. } => {
                    (location.document_path.clone(), location.span_start, location.span_end)
                }
                other => panic!("expected a reference, got {other:?}"),
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        sites(&location),
        sites(&signature),
        "detail does not change which results are returned or their order"
    );
}

// _(Scenario: Reference sites project their enclosing declaration)_ — a reference site inside a
// method body carries the method's signature (the attributed enclosing declaration), not the
// referenced subject's.
#[test]
fn trace_reference_site_projects_enclosing_declaration_signature() {
    use silent_cartographer::query::TraceItem;
    let (store, lookup_id, offset_id) = method_reference_store();
    let engine = dep_engine(&store);

    let answer = engine
        .trace(
            offset_id.as_str(),
            TraceRelation::References,
            Some(Detail::Signature),
            None,
        )
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 1, "OFFSET is referenced once, inside lookup's body");

    let lookup_row = store.symbol(&lookup_id).unwrap().expect("lookup persisted");
    let offset_row = store.symbol(&offset_id).unwrap().expect("OFFSET persisted");
    match &results[0] {
        TraceItem::Reference { enclosing, content, .. } => {
            assert_eq!(enclosing.as_ref(), Some(&lookup_id), "the site attributes to lookup");
            assert_eq!(
                content.as_deref(),
                lookup_row.signature_text.as_deref(),
                "the row carries the enclosing method's signature"
            );
            assert_ne!(
                content.as_deref(),
                offset_row.signature_text.as_deref(),
                "the row carries the enclosing declaration's signature, not the referenced subject's"
            );
        }
        other => panic!("expected a reference, got {other:?}"),
    }
}

// _(Reference sites project their enclosing declaration — module attribution)_ — a site attributed
// to the module/file itself (`enclosing_id` NULL) projects the FILE module's tiers even when a
// re-export-defined module row shares the document with an identical whole-document span: the file
// module's own definition occurrence spans the document, the re-export's sits on a name token, and
// that definition width — not row-identity order — decides. The re-export's identity sorts first
// here precisely so an identity-order tie-break would fail this test.
#[test]
fn module_scope_site_projects_the_file_module_over_a_reexport_twin() {
    use silent_cartographer::query::TraceItem;
    let store = GraphStore::open_in_memory().unwrap();
    let doc = "facade.rs";
    let put_module = |name: &str, signature: &str| {
        store
            .insert_symbol(&SymbolRow {
                canonical_id: dep_id(name),
                display_name: name.to_string(),
                kind: "module".to_string(),
                class: PersistedClass::InWorkspace,
                document_path: Some(doc.to_string()),
                span: Some((0, 100)),
                line_column: None,
                header_end: None,
                span_text: Some("whole document".to_string()),
                signature_text: Some(signature.to_string()),
                interface_text: Some(signature.to_string()),
                duplicated: false,
                test_rule: None,
            })
            .unwrap();
    };
    put_module("alpha_reexport", "alpha_reexport");
    put_module("zzz_file_module", "zzz_file_module");
    let put_definition = |name: &str, span: (usize, usize)| {
        store
            .insert_occurrence(&OccurrenceRow {
                symbol_id: dep_id(name),
                document_path: doc.to_string(),
                span,
                line_column: None,
                role: "definition".to_string(),
                rule: "exact".to_string(),
                enclosing_id: None,
                locality: None,
            })
            .unwrap();
    };
    put_definition("zzz_file_module", (0, 100));
    put_definition("alpha_reexport", (10, 24));
    put_dep_symbol(&store, "target");
    store
        .insert_occurrence(&OccurrenceRow {
            symbol_id: dep_id("target"),
            document_path: doc.to_string(),
            span: (40, 46),
            line_column: None,
            role: "reference".to_string(),
            rule: "exact".to_string(),
            enclosing_id: None,
            locality: None,
        })
        .unwrap();

    let engine = dep_engine(&store);
    let answer = engine
        .trace(
            "test-ws::target",
            TraceRelation::References,
            Some(Detail::Signature),
            None,
        )
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0] {
        TraceItem::Reference { content, .. } => assert_eq!(
            content.as_deref(),
            Some("zzz_file_module"),
            "the module-scope site projects the file module (whole-document definition), not the \
             re-export module its identity sorts behind"
        ),
        other => panic!("expected a reference, got {other:?}"),
    }
}

// _(Trace results at a chosen detail — symbol rows)_ — a `containers` row denotes a symbol, so at
// body detail it carries that symbol's own full body.
#[test]
fn trace_containers_at_body_detail_carries_the_container_body() {
    use silent_cartographer::query::TraceItem;
    let store = GraphStore::open_in_memory().unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: dep_id("Holder"),
            display_name: "Holder".to_string(),
            kind: "type".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("m.rs".to_string()),
            span: Some((0, 40)),
            line_column: None,
            header_end: None,
            span_text: Some("struct Holder {\n    member: u8,\n}".to_string()),
            signature_text: Some("struct Holder".to_string()),
            interface_text: Some("struct Holder".to_string()),
            duplicated: false,
            test_rule: None,
        })
        .unwrap();
    put_dep_symbol(&store, "member");
    store
        .insert_edge(EdgeKind::Contains, &dep_id("Holder"), &dep_id("member"))
        .unwrap();

    let engine = dep_engine(&store);
    let answer = engine
        .trace("test-ws::member", TraceRelation::Containers, Some(Detail::Body), None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0] {
        TraceItem::Symbol { symbol, content, .. } => {
            assert_eq!(symbol.name, "Holder");
            assert_eq!(
                content.as_deref(),
                Some("struct Holder {\n    member: u8,\n}"),
                "a symbol-denoting row at body detail carries its own full body"
            );
        }
        other => panic!("expected a symbol row, got {other:?}"),
    }
}

// _(Trace results at a chosen detail — location is the default projection)_ — explicitly requesting
// location detail is the no-content shape: the location already sits on every row.
#[test]
fn trace_explicit_location_detail_carries_no_content() {
    use silent_cartographer::query::TraceItem;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine
        .trace("net::Client", TraceRelation::References, Some(Detail::Location), None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    for item in results {
        match item {
            TraceItem::Reference { content, .. } => {
                assert!(content.is_none(), "location detail carries no content field")
            }
            other => panic!("expected a reference, got {other:?}"),
        }
    }
    assert!(
        !answer.to_json().contains("\"content\""),
        "explicit location detail keeps the no-content JSON shape"
    );
}

// _(Relationship trace — empty branch)_ — a subject with no instances of a relation returns typed
// absence.
#[test]
fn trace_empty_relation_is_typed_absence() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // disconnect has no references in the fixture.
    let answer = engine
        .trace("net::Client::disconnect", TraceRelation::References, None, None)
        .unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "empty relation is a typed Empty, not a failure: {:?}",
        answer.outcome
    );
}

// _(Symbol search by name fragment — several matches)_ — `find` returns every symbol whose name
// contains the fragment.
#[test]
fn find_returns_every_symbol_whose_name_contains_the_fragment() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // "conn" is a substring of both `connect` and `disconnect`.
    let answer = engine.find("conn").unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let names: Vec<&str> = results.iter().map(|item| item.symbol.name.as_str()).collect();
    assert!(names.contains(&"connect"), "connect contains the fragment: {names:?}");
    assert!(
        names.contains(&"disconnect"),
        "disconnect contains the fragment: {names:?}"
    );
}

// _(Symbol search by name fragment — case-insensitive matching)_ — `find` matches regardless of the
// fragment's case.
#[test]
fn find_matches_case_insensitively() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.find("CoNN").unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert!(
        results.iter().any(|item| item.symbol.name == "connect"),
        "a differently-cased fragment still matches: {results:?}"
    );
}

// _(Symbol search by name fragment — no match is typed absence)_ — a fragment matching no symbol is
// a definite "none", distinct from a failure.
#[test]
fn find_no_match_is_typed_absence() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.find("zzz_no_such_fragment").unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Absent),
        "a fragment matching nothing is typed absence: {:?}",
        answer.outcome
    );
}

// _(Symbol search by name fragment — LIKE metacharacters are literal)_ — a `_` or `%` in the
// fragment matches only itself, never as a SQL wildcard.
#[test]
fn find_treats_like_metacharacters_as_literal_text() {
    let store = GraphStore::open_in_memory().unwrap();
    put_dep_symbol(&store, "edge");
    put_dep_symbol(&store, "edge_sources");
    let engine = dep_engine(&store);

    // A `_` acting as a single-character wildcard would make "e_ge" match `edge`; literally it
    // matches nothing.
    let underscore = engine.find("e_ge").unwrap();
    assert!(
        matches!(underscore.outcome, Outcome::Absent),
        "a literal `_` is not a wildcard: {:?}",
        underscore.outcome
    );

    // A `%` acting as an any-run wildcard would make "edge%source" match `edge_sources`; literally
    // it matches nothing.
    let percent = engine.find("edge%source").unwrap();
    assert!(
        matches!(percent.outcome, Outcome::Absent),
        "a literal `%` is not a wildcard: {:?}",
        percent.outcome
    );

    // A `_` present in the stored name still matches as literal text.
    let literal = engine.find("edge_s").unwrap();
    let Outcome::Found { results } = &literal.outcome else {
        panic!("expected found, got {:?}", literal.outcome);
    };
    let names: Vec<&str> = results.iter().map(|item| item.symbol.name.as_str()).collect();
    assert_eq!(names, vec!["edge_sources"], "a literal `_` matches itself: {names:?}");
}

// _(Calibrated output contract — get write-site)_ — a fresh result carries provenance and is marked
// fresh.
#[test]
fn fresh_result_carries_provenance_and_is_fresh() {
    use silent_cartographer::query::output::FreshnessLabel;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Location, None, 1).unwrap();
    assert_eq!(answer.provenance.analyzer_name, "rust-analyzer");
    assert!(!answer.provenance.analyzer_version.is_empty());
    assert_eq!(answer.freshness, FreshnessLabel::Fresh);
    assert!(!answer.stale);
    // The JSON rendering carries provenance and the fresh flag.
    let json = answer.to_json();
    assert!(json.contains("rust-analyzer"));
    assert!(json.contains("fresh"));
}

// _(Calibrated output contract)_ — each returned symbol carries both its canonical identity and a
// human-readable name.
#[test]
fn result_identifies_symbols_by_identity_and_name() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Location, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    assert_eq!(results[0].symbol.canonical_id, connect_id());
    assert_eq!(results[0].symbol.name, "connect");
}

// _(Calibrated output contract — both query write-sites)_ — a result over changed sources is marked
// stale, through both `get` and `trace`.
#[test]
fn stale_result_flagged_through_get_and_trace() {
    let store = built_store();
    // A query engine told the current hash differs (sources changed since indexing).
    let engine = QueryEngine::new(&store, support::provenance(), "different-hash".to_string(), None);

    let get_answer = engine.get("net::Client::connect", Detail::Location, None, 1).unwrap();
    assert!(get_answer.stale, "get result over changed sources is stale");

    let trace_answer = engine
        .trace("net::Client", TraceRelation::Contains, None, None)
        .unwrap();
    assert!(trace_answer.stale, "trace result over changed sources is stale");
}

/// A fake Python environment under `dir/.venv`: a version-only `bin/python` stub and a site-packages
/// directory holding one installed distribution — enough for `environment_facts` to resolve an
/// interpreter version and a package fingerprint, and for `resolve_environment`'s `.venv` fallback to
/// find it with no `$VIRTUAL_ENV` set.
fn write_fake_venv(dir: &std::path::Path) -> std::path::PathBuf {
    let venv = dir.join(".venv");
    let bin = venv.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_indexing_stub(&bin, "python", "Python 3.12.0");
    let site = venv.join("lib").join("python3.12").join("site-packages");
    std::fs::create_dir_all(site.join("foo-1.0.dist-info")).unwrap();
    venv
}

// _(Scenario: Stale after changed sources)_ — an index whose recorded sources no longer match what is
// on disk answers stale, and the refresh command it carries names the index's location, the
// workspace identity, and the workspace root.
#[test]
fn stale_after_changed_sources_names_the_index_location_workspace_and_root() {
    let dir = tempfile::tempdir().unwrap();
    // No source file is written under `dir`, so a query's own content hash differs from the one the
    // fixture index recorded: the answer reads stale from the first query.
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "stale_content");
    assert_eq!(value["stale"], true);

    let refresh = value["refresh"]
        .as_str()
        .expect("a stale answer carries a refresh command");
    assert!(
        refresh.starts_with("c10r build "),
        "the command names the build subcommand: {refresh}"
    );
    let root = std::fs::canonicalize(dir.path()).unwrap();
    assert!(
        refresh.contains(&root.display().to_string()),
        "the command names the workspace root: {refresh}"
    );
    assert!(
        refresh.contains(&db.display().to_string()),
        "the command names the index location: {refresh}"
    );
    assert!(
        refresh.contains("refresh-ws"),
        "the command names the workspace identity: {refresh}"
    );
}

// _(Scenario: Stale after a changed analyzer)_ — an index recorded under one analyzer version while a
// different version is in effect answers stale and carries a refresh command.
#[test]
fn stale_after_a_changed_analyzer_carries_a_refresh_command() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    let db = support::build_fixture_db(dir.path(), "refresh-ws");
    // The fixture records `rust-analyzer 1.85.0` (`support::provenance`); this stub reports a
    // different version, so only the analyzer identity drifts.
    let stub = write_indexing_stub(dir.path(), "rust-analyzer-stub", "9.9.9");

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        stub.to_str().unwrap(),
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "stale_version");
    assert!(
        value["refresh"].as_str().unwrap().starts_with("c10r build "),
        "a version-stale answer still carries a refresh command: {value}"
    );
}

// _(Scenario: A fresh answer carries no refresh command)_ — an index whose sources and analyzer are
// unchanged since indexing answers fresh, and the answer carries no `refresh` field at all.
#[test]
fn a_fresh_answer_carries_no_refresh_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "fresh");
    assert_eq!(value["stale"], false);
    assert!(
        value.get("refresh").is_none(),
        "a fresh answer carries no refresh field: {value}"
    );
}

// _(Scenario: The human rendering names the refresh command)_ — a stale answer's human rendering
// includes the same refresh command the machine answer carries.
#[test]
fn human_rendering_includes_the_refresh_command() {
    let dir = tempfile::tempdir().unwrap();
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        false,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert!(
        out.lines().any(|line| line.starts_with("refresh: c10r build ")),
        "the human rendering names the refresh command: {out}"
    );
}

// _(Scenario: Running the refresh command makes the answer fresh)_ — running the refresh command a
// stale answer carries, against a stub analyzer reachable through `PATH` and the built `c10r` binary,
// brings the same query back to fresh.
#[test]
fn running_the_refresh_command_after_changed_sources_makes_the_answer_fresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "# fixture manifest\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    // The source changes on disk after the index was built, so a query over it reads stale.
    std::fs::write(
        dir.path().join(support::DOC),
        format!("{}\n// edited\n", support::SOURCE),
    )
    .unwrap();

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "stale_content");
    let refresh = value["refresh"].as_str().unwrap().to_string();

    // Run the refresh command for real: the built `c10r` binary and a stub `rust-analyzer` that
    // succeeds without doing real analysis, both reachable on `PATH`.
    let bin_dir = tempfile::tempdir().unwrap();
    write_indexing_stub(bin_dir.path(), "rust-analyzer", "1.85.0");
    let built_bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_c10r"))
        .parent()
        .unwrap()
        .to_path_buf();
    let path = prepend_path(&[&built_bin_dir, bin_dir.path()]);

    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&refresh)
        .env("PATH", &path)
        .current_dir(std::env::temp_dir())
        .status()
        .unwrap();
    assert!(status.success(), "the refresh command runs to completion: {refresh}");

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "fresh", "the rebuilt index answers fresh: {value}");
    assert!(
        value.get("refresh").is_none(),
        "a fresh answer carries no refresh field: {value}"
    );
}

// _(Scenario: Stale after a changed environment)_ — a recorded interpreter environment drifting (an
// installed distribution added) while the sources and analyzer stay put answers stale, and running
// the refresh command it carries — which pins the environment currently in effect, not the one
// originally recorded — brings the same query back to fresh under that same drifted environment.
#[test]
fn stale_after_a_changed_environment_is_fresh_once_the_refresh_command_runs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "# fixture manifest\n").unwrap();
    std::fs::write(dir.path().join("app.py"), "x = 1\n").unwrap();
    let venv = write_fake_venv(dir.path());

    let collected = silent_cartographer::commands::collect_python_sources(dir.path()).unwrap();
    let facts = silent_cartographer::semantic::python_adapter::environment_facts(&venv).unwrap();
    // Recorded at whatever version a real `scip-python` on this machine's `PATH` reports (falling
    // back to a fixed one when none resolves), so only the environment drifts below — never the
    // analyzer identity `current_state` would also compare.
    let analyzer_version = silent_cartographer::semantic::python_adapter::PythonAdapter::discover_version(
        silent_cartographer::semantic::python_adapter::PythonAdapter::analyzer_name(),
    )
    .unwrap_or_else(|_| "0.6.0".to_string());
    let index = ExtractedIndex {
        provenance: AnalyzerProvenance {
            analyzer_name: silent_cartographer::semantic::python_adapter::PythonAdapter::analyzer_name().to_string(),
            analyzer_version: analyzer_version.clone(),
        },
        documents: vec![SourceDocument {
            path: "app.py".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: Some(facts),
    };
    let db = dir.path().join("index.db");
    silent_cartographer::commands::build_from_index(&db, "refresh-ws", dir.path(), &index, &collected).unwrap();

    // Drift the environment: one more installed distribution changes the package fingerprint, with
    // the environment's own path unchanged.
    std::fs::create_dir(
        venv.join("lib")
            .join("python3.12")
            .join("site-packages")
            .join("bar-2.0.dist-info"),
    )
    .unwrap();

    // Queried through the built binary, not in-process: `current_state`'s environment resolution
    // reads the whole test process's `$VIRTUAL_ENV`, which an activated development environment may
    // set to something else entirely, and a child process is the only place that is safe to control.
    let query = |db: &std::path::Path, root: &std::path::Path| -> serde_json::Value {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"))
            .arg("--db")
            .arg(db)
            .args(["--json", "find", "x"])
            .current_dir(root)
            .env_remove("VIRTUAL_ENV")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "the query succeeds: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };

    let value = query(&db, dir.path());
    assert_eq!(value["freshness"], "stale_environment");
    let refresh = value["refresh"].as_str().unwrap().to_string();
    // Named canonically and absolutely, so the command runs from any directory.
    let named_environment = refresh
        .split("--environment='")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .expect("the command names --environment");
    assert!(
        std::path::Path::new(named_environment).is_absolute(),
        "the named environment is absolute: {refresh}"
    );
    assert_eq!(
        std::fs::canonicalize(named_environment).unwrap(),
        std::fs::canonicalize(&venv).unwrap(),
        "the refresh command names the environment in effect: {refresh}"
    );

    // Run the refresh command for real: the built `c10r` binary and a stub `scip-python` that
    // succeeds without doing real analysis, both reachable on `PATH`.
    let bin_dir = tempfile::tempdir().unwrap();
    // Reports the same version already recorded, so the final query below still sees a matching
    // analyzer identity and the environment is the only thing that was ever compared.
    write_indexing_stub(bin_dir.path(), "scip-python", &analyzer_version);
    let built_bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_c10r"))
        .parent()
        .unwrap()
        .to_path_buf();
    let path = prepend_path(&[&built_bin_dir, bin_dir.path()]);

    // Run from outside the workspace root: the named environment is absolute, so the command does
    // not depend on the directory it runs from.
    let elsewhere = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&refresh)
        .env("PATH", &path)
        .current_dir(elsewhere.path())
        .status()
        .unwrap();
    assert!(status.success(), "the refresh command runs to completion: {refresh}");

    // The same query, repeated under the same (still drifted) environment, now answers fresh.
    let value = query(&db, dir.path());
    assert_eq!(value["freshness"], "fresh", "the rebuilt index answers fresh: {value}");
    assert!(
        value.get("refresh").is_none(),
        "a fresh answer carries no refresh field: {value}"
    );
}

/// `PATH` holding only `bin_dir` plus the system directories a spawned process needs to resolve its
/// own dynamic loader and shared libraries — no inherited entry survives, so a stub placed in
/// `bin_dir` is the only tool of its name the child process can find.
fn hermetic_path(bin_dir: &std::path::Path) -> std::ffi::OsString {
    std::env::join_paths([bin_dir, std::path::Path::new("/usr/bin"), std::path::Path::new("/bin")]).unwrap()
}

/// Build the Python workspace at `dir` (a manifest and a fake `.venv`, no activated environment) with
/// the given root spelling, then query it from the workspace directory, both through the built binary
/// with a stub `scip-python` — answering the version probe and, for `index`, writing an empty (valid)
/// SCIP index — the only `scip-python` reachable on `PATH`. Returns the query's freshness answer.
fn build_and_query_env_spelling(
    dir: &std::path::Path,
    db: &std::path::Path,
    build_root: &std::ffi::OsStr,
) -> serde_json::Value {
    std::fs::write(
        dir.join("pyproject.toml"),
        "[project]\nname = \"pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    write_fake_venv(dir);
    let bin_dir = tempfile::tempdir().unwrap();
    write_indexing_stub(bin_dir.path(), "scip-python", "0.6.0");
    let path = hermetic_path(bin_dir.path());

    let build = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"))
        .arg("build")
        .arg(build_root)
        .arg("--db")
        .arg(db)
        .args(["--workspace", "env-spelling-ws"])
        .current_dir(dir)
        .env_remove("VIRTUAL_ENV")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "the build succeeds: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let query = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"))
        .arg("--db")
        .arg(db)
        .args(["--json", "--workspace", "env-spelling-ws", "find", "hello"])
        .current_dir(dir)
        .env_remove("VIRTUAL_ENV")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(
        query.status.success(),
        "the query succeeds: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    serde_json::from_slice(&query.stdout).unwrap()
}

// _(Scenario: An index built by the refresh hooks answers fresh)_ — a Python workspace's environment,
// found through a `.venv` directory with no activated environment, is recorded canonically at build
// time, so a build naming the workspace root by an absolute path — the way the refresh hooks build —
// answers fresh to a query run from the workspace directory.
#[test]
fn an_index_built_with_an_absolute_root_answers_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".c10r").join("index.db");
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let value = build_and_query_env_spelling(dir.path(), &db, root.as_os_str());
    assert_eq!(value["freshness"], "fresh", "built with an absolute root: {value}");
}

// _(Scenario: An index built from the workspace directory answers fresh)_ — the same workspace, built
// with the root given as `.`, also answers fresh: freshness does not depend on how the root was
// spelled at build time.
#[test]
fn an_index_built_from_the_workspace_directory_answers_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".c10r").join("index.db");
    let value = build_and_query_env_spelling(dir.path(), &db, std::ffi::OsStr::new("."));
    assert_eq!(value["freshness"], "fresh", "built with the root '.': {value}");
}

// _(Scenario: The index does not record its workspace root)_ — a stale answer over an index whose
// build root could not be recorded exactly says the root is unknown, as a shell comment that runs
// nothing rather than a placeholder a shell could misparse (`<`/`>` are redirection operators).
#[test]
fn stale_answer_with_no_recorded_root_says_the_root_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    // No source file is written under `dir`, so the query reads stale from the first query.
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    // `workspace_root` is `None` only when a build root could not be recorded exactly (not valid
    // UTF-8); the fixture helpers always record a canonicalized root, so it is erased here directly,
    // reading back the metadata `build_from_index` wrote and rewriting it with the one field cleared.
    let store = GraphStore::open_or_replace(&db).unwrap();
    let mut meta = store.read_metadata().unwrap().expect("metadata present");
    meta.workspace_root = None;
    store.write_metadata(&meta).unwrap();
    drop(store);

    let out = silent_cartographer::commands::run_find(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect",
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["freshness"], "stale_content");
    let refresh = value["refresh"]
        .as_str()
        .expect("a stale answer carries a refresh command");
    assert!(
        refresh.starts_with('#'),
        "an unknown root is a shell comment that runs nothing: {refresh}"
    );
    assert!(refresh.contains("root"), "the text says the root is unknown: {refresh}");
    // The db/workspace parts still ride along, exactly as the normal command shape carries them.
    assert!(
        refresh.contains(&db.display().to_string()),
        "still names the index location: {refresh}"
    );
    assert!(
        refresh.contains("refresh-ws"),
        "still names the workspace identity: {refresh}"
    );

    // A comment line is inert — running it does nothing and exits successfully — rather than a shell
    // misreading `<`/`>` in a bare placeholder as redirection.
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(refresh)
        .status()
        .unwrap();
    assert!(status.success(), "a comment line runs to a no-op success: {refresh}");
}

// _(Write-site coverage)_ — every query path that produces an `Answer` attaches a refresh command
// once its index is stale, not only `find`: `get`, `trace` (the item-relation branch and the
// `dependents` branch), `search`, `similar`, and `impact` each carry one too.
#[test]
fn every_query_path_carries_a_refresh_command_on_a_stale_answer() {
    let dir = tempfile::tempdir().unwrap();
    // No source file is written under `dir`, so every query below reads stale from its own content
    // hash mismatch — the one condition every command's freshness check shares.
    let db = support::build_fixture_db(dir.path(), "refresh-ws");

    let assert_refreshable = |label: &str, out: String| {
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["stale"], true, "{label} answer is stale: {value}");
        assert!(
            value["refresh"].as_str().unwrap_or_default().starts_with("c10r build "),
            "{label} carries a refresh command: {value}"
        );
    };

    let get = silent_cartographer::commands::run_get(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        Some("net::Client::connect"),
        None,
        Detail::Location,
        0,
        1,
        false,
        false,
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("get", get);

    let trace = silent_cartographer::commands::run_trace(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "net::Client",
        Relation::Contains,
        None,
        None,
        10,
        false,
        OrderMode::Unranked,
        false,
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("trace (contains)", trace);

    let dependents = silent_cartographer::commands::run_trace(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "net::Client::connect",
        Relation::Dependents,
        None,
        None,
        10,
        false,
        OrderMode::Unranked,
        false,
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("trace (dependents)", dependents);

    let search = silent_cartographer::commands::run_search(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "connect a client",
        Detail::Signature,
        10,
        false,
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("search", search);

    let similar = silent_cartographer::commands::run_similar(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        Some("net::Client::connect"),
        None,
        Detail::Signature,
        10,
        false,
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("similar", similar);

    // `impact` additionally needs a real git worktree; a root commit is enough for `HEAD` to
    // resolve, and a completely clean working tree yields a typed-empty seed set, which still
    // carries the same freshness/refresh envelope every other outcome does.
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} succeeds");
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.email=refresh-test@example.com",
        "-c",
        "user.name=Refresh Test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--allow-empty",
        "-q",
        "-m",
        "root",
    ]);
    let impact = silent_cartographer::commands::run_impact(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        None,
        None,
        false,
        1,
        OrderMode::Unranked,
        &[],
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .unwrap();
    assert_refreshable("impact", impact);
}

// _(Calibrated output contract — ambiguous write-site; Symbol retrieval at a chosen detail —
// ambiguous branch)_ — `get` with a shared shortname returns the typed ambiguous candidate outcome,
// asserted through the JSON rendering.
#[test]
fn get_ambiguous_shortname_returns_typed_candidates_in_json() {
    // Two symbols named `connect` in different types → ambiguous shortname (same setup as the
    // resolution-layer test, asserted here through the full `get` answer and its JSON).
    let mut index = support::fixture_index();
    index
        .symbols
        .push(silent_cartographer::semantic::model::ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "mycrate",
                vec![
                    DescriptorSegment::new("net", SegmentKind::Module),
                    DescriptorSegment::new("Server", SegmentKind::Type),
                    DescriptorSegment::new("connect", SegmentKind::Method),
                ],
            )),
            kind: silent_cartographer::semantic::model::SymbolKind::Method,
            class: silent_cartographer::semantic::model::SymbolClass::InWorkspace,
            occurrences: vec![silent_cartographer::semantic::model::ExtractedOccurrence {
                document_path: support::DOC.to_string(),
                range: silent_cartographer::semantic::model::SourceRange::new(3, 15, 3, 22),
                role: silent_cartographer::semantic::model::OccurrenceRole::Definition,
            }],
        });
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &sources()).unwrap();
    let engine = engine_over(&store, support::provenance());

    let answer = engine.get("connect", Detail::Location, None, 1).unwrap();
    let Outcome::Ambiguous { candidates, .. } = &answer.outcome else {
        panic!("expected ambiguous, got {:?}", answer.outcome);
    };
    assert_eq!(candidates.len(), 2, "both connect symbols returned as candidates");
    // Every candidate carries identity + name — no arbitrary pick.
    for c in candidates {
        assert_eq!(c.name, "connect");
        assert!(c.canonical_id.as_str().contains("connect"));
    }

    // The JSON rendering carries the typed outcome tag and both candidate identities.
    let json = answer.to_json();
    assert!(
        json.contains("\"ambiguous\""),
        "JSON carries the ambiguous outcome tag: {json}"
    );
    assert!(
        json.contains("Client::connect"),
        "JSON carries the first candidate identity: {json}"
    );
    assert!(
        json.contains("Server::connect"),
        "JSON carries the second candidate identity: {json}"
    );
}

// _(Calibrated output contract — absent write-site)_ — `get` for a reference denoting no symbol
// returns the typed absent outcome, distinct from both an empty relation and a failure, asserted
// through the JSON rendering.
#[test]
fn get_unknown_reference_returns_typed_absence_in_json() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());

    // A reference denoting no symbol: the query SUCCEEDS (no failure) with a typed Absent outcome.
    let answer = engine
        .get("no_such_symbol_anywhere", Detail::Location, None, 1)
        .expect("absence is a successful typed answer, not a failure");
    assert!(
        matches!(answer.outcome, Outcome::Absent),
        "expected absent, got {:?}",
        answer.outcome
    );

    // Absent is distinct from an empty relation: a subject with no instances of a relation yields
    // Empty, and the two render with different outcome tags.
    let empty = engine
        .trace("net::Client::disconnect", TraceRelation::References, None, None)
        .unwrap();
    assert!(matches!(empty.outcome, Outcome::Empty));

    let absent_json = answer.to_json();
    let empty_json = empty.to_json();
    assert!(
        absent_json.contains("\"absent\""),
        "JSON carries the absent outcome tag: {absent_json}"
    );
    assert!(
        empty_json.contains("\"empty\""),
        "JSON carries the empty outcome tag: {empty_json}"
    );
    assert!(
        !absent_json.contains("\"empty\"") && !empty_json.contains("\"absent\""),
        "absent and empty are distinct typed outcomes"
    );
    // Both still carry the full output contract (provenance + freshness), unlike a failure.
    assert!(absent_json.contains("rust-analyzer"));
    assert!(absent_json.contains("freshness"));
}

/// A synthetic identity for a dependents-graph symbol.
fn dep_id(name: &str) -> CanonicalId {
    CanonicalId::from_raw(format!("test-ws::{name}"))
}

/// Persist a bare in-workspace symbol so dependency edges satisfy the foreign-key constraint and the
/// symbol is resolvable by its canonical identity.
fn put_dep_symbol(store: &GraphStore, name: &str) {
    store
        .insert_symbol(&SymbolRow {
            canonical_id: dep_id(name),
            display_name: name.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: None,
            span: None,
            line_column: None,
            header_end: None,
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
            test_rule: None,
        })
        .unwrap();
}

/// Persist a dependent-graph symbol carrying the given tier content, for detail-projection scenarios.
fn put_dep_symbol_with_tiers(store: &GraphStore, name: &str, signature: Option<&str>, interface: Option<&str>) {
    store
        .insert_symbol(&SymbolRow {
            canonical_id: dep_id(name),
            display_name: name.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: None,
            span: None,
            line_column: None,
            header_end: None,
            span_text: None,
            signature_text: signature.map(str::to_string),
            interface_text: interface.map(str::to_string),
            duplicated: false,
            test_rule: None,
        })
        .unwrap();
}

/// A store over `symbols`, with `edges` as `(kind, src_name, dst_name)`. An edge `(Uses, "a", "b")`
/// means "a uses b", so `b`'s dependents include `a`.
fn dep_graph(symbols: &[&str], edges: &[(EdgeKind, &str, &str)]) -> GraphStore {
    let store = GraphStore::open_in_memory().unwrap();
    for s in symbols {
        put_dep_symbol(&store, s);
    }
    for (kind, src, dst) in edges {
        store.insert_edge(*kind, &dep_id(src), &dep_id(dst)).unwrap();
    }
    store
}

/// A query engine over a directly-built store (no metadata written; answers read stale, which is
/// irrelevant to the dependents outcome under test).
fn dep_engine(store: &GraphStore) -> QueryEngine<'_> {
    QueryEngine::new(store, support::provenance(), "hash".to_string(), None)
}

// _(Trace dependents; Detailed results carry kind and distance)_ — `dependents` returns direct
// dependents each labeled with the connecting edge kind and its distance.
#[test]
fn dependents_reports_kind_and_distance() {
    use silent_cartographer::query::DependentsReport;
    let store = dep_graph(&["seed", "caller"], &[(EdgeKind::Uses, "caller", "seed")]);
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.detail.len(), 1);
    assert_eq!(report.detail[0].symbol.canonical_id, dep_id("caller"));
    assert_eq!(
        report.detail[0].kind,
        DependencyKind::Uses,
        "detail carries the connecting kind"
    );
    assert_eq!(report.detail[0].distance, 1, "detail carries the hop distance");
}

// _(Reach ends within the bound)_ — a subject whose every dependent lies within the requested depth
// reports no reach beyond the detail.
#[test]
fn dependents_reach_ends_within_bound() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    // Two direct dependents, nothing deeper.
    let store = dep_graph(
        &["seed", "a", "b"],
        &[(EdgeKind::Uses, "a", "seed"), (EdgeKind::Uses, "b", "seed")],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.disclosure, HorizonDisclosure::EndsWithinBound);
    assert!(report.beyond_bound.is_empty(), "no reach beyond the bound: {report:?}");
    assert_eq!(report.detail.len(), 2);
}

// _(Reach extends beyond the bound)_ — a subject with deeper dependents stops detail at the bound and
// reports aggregate counts of the deeper dependents by edge kind and distance.
#[test]
fn dependents_beyond_bound_aggregates_by_kind_and_distance() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    // seed <- mid (uses, depth 1) <- outer (uses, depth 2).
    let store = dep_graph(
        &["seed", "mid", "outer"],
        &[(EdgeKind::Uses, "mid", "seed"), (EdgeKind::Uses, "outer", "mid")],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.disclosure, HorizonDisclosure::BeyondBound);
    assert_eq!(report.detail.len(), 1, "only the direct dependent is detailed");
    assert_eq!(report.detail[0].symbol.canonical_id, dep_id("mid"));
    assert_eq!(report.beyond_bound.len(), 1, "the deeper dependent is aggregated");
    let agg = &report.beyond_bound[0];
    assert_eq!(agg.kind, DependencyKind::Uses);
    assert_eq!(agg.distance, 2);
    assert_eq!(agg.count, 1);
}

// _(Reach extends beyond the bound)_ — the aggregate rows order by the same key the detailed rows
// break ties on: distance first, then the fixed dependency-kind order (uses, imports,
// type_hierarchy). A dependents answer never presents its detail and its aggregate under two
// different orderings.
#[test]
fn dependents_aggregate_orders_by_distance_then_the_fixed_kind_order() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    // seed <- mid (depth 1); three dependents of mid at depth 2, one per dependency kind, so the
    // aggregate holds three rows at the same distance and only the kind order can separate them.
    let store = dep_graph(
        &["seed", "mid", "by_uses", "by_imports", "by_hierarchy"],
        &[
            (EdgeKind::Uses, "mid", "seed"),
            (EdgeKind::Uses, "by_uses", "mid"),
            (EdgeKind::Imports, "by_imports", "mid"),
            (EdgeKind::TypeHierarchy, "by_hierarchy", "mid"),
        ],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.disclosure, HorizonDisclosure::BeyondBound);

    let ordered: Vec<(u32, DependencyKind)> = report.beyond_bound.iter().map(|a| (a.distance, a.kind)).collect();
    assert_eq!(
        ordered,
        vec![
            (2, DependencyKind::Uses),
            (2, DependencyKind::Imports),
            (2, DependencyKind::TypeHierarchy),
        ],
        "the aggregate follows the detailed rows' tie-break, not an incidental ordering"
    );
}

// _(Aggregate discloses its own horizon)_ — a dependency network extending past the internal horizon
// makes the answer state the aggregate itself is bounded, not the total reach. Uses a chain longer
// than the horizon.
#[test]
fn dependents_aggregate_discloses_the_horizon() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    // A chain f0 <- f1 <- ... <- fN, longer than the horizon, so a dependent sits at the horizon
    // depth and deeper links exist unseen.
    let n = (DEPENDENTS_HORIZON + 2) as usize;
    let names: Vec<String> = (0..=n).map(|i| format!("f{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    // f{i+1} uses f{i}: edge (Uses, "f{i+1}", "f{i}").
    let edges: Vec<(EdgeKind, &str, &str)> = (0..n)
        .map(|i| (EdgeKind::Uses, name_refs[i + 1], name_refs[i]))
        .collect();
    let store = dep_graph(&name_refs, &edges);
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::f0", 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(
        report.disclosure,
        HorizonDisclosure::CutAtHorizon,
        "the aggregate itself is bounded at the horizon: {report:?}"
    );
    // The deepest aggregated distance is the horizon; nothing beyond it is claimed.
    let max_distance = report.beyond_bound.iter().map(|a| a.distance).max().unwrap();
    assert_eq!(
        max_distance, DEPENDENTS_HORIZON,
        "reach is reported only to the horizon"
    );
}

// _(Horizon disclosure at depth == horizon)_ — requesting exactly the horizon depth still discloses
// the cut: a dependent sitting at the horizon means deeper reach may exist unexplored, regardless of
// whether the bound happens to coincide with the horizon.
#[test]
fn dependents_discloses_cut_when_depth_equals_horizon() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    let n = (DEPENDENTS_HORIZON + 2) as usize;
    let names: Vec<String> = (0..=n).map(|i| format!("f{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let edges: Vec<(EdgeKind, &str, &str)> = (0..n)
        .map(|i| (EdgeKind::Uses, name_refs[i + 1], name_refs[i]))
        .collect();
    let store = dep_graph(&name_refs, &edges);
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::f0", DEPENDENTS_HORIZON, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(
        report.disclosure,
        HorizonDisclosure::CutAtHorizon,
        "depth == horizon still discloses the cut: {report:?}"
    );
}

// _(Horizon disclosure at depth beyond the horizon)_ — requesting more depth than the horizon allows
// still discloses the cut, since the walk itself never reaches past the horizon.
#[test]
fn dependents_discloses_cut_when_depth_exceeds_horizon() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    let n = (DEPENDENTS_HORIZON + 2) as usize;
    let names: Vec<String> = (0..=n).map(|i| format!("f{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let edges: Vec<(EdgeKind, &str, &str)> = (0..n)
        .map(|i| (EdgeKind::Uses, name_refs[i + 1], name_refs[i]))
        .collect();
    let store = dep_graph(&name_refs, &edges);
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::f0", DEPENDENTS_HORIZON + 5, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(
        report.disclosure,
        HorizonDisclosure::CutAtHorizon,
        "depth beyond the horizon still discloses the cut: {report:?}"
    );
}

// _(Horizon disclosure does not over-fire)_ — a subject whose whole network ends well shallower than
// the horizon, queried with a depth larger than the network's actual depth, discloses EndsWithinBound
// rather than falsely claiming a horizon cut.
#[test]
fn dependents_shallow_network_ends_within_bound_even_at_large_depth() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    // seed <- mid (depth 1) <- outer (depth 2); network ends at depth 2, far short of the horizon.
    let store = dep_graph(
        &["seed", "mid", "outer"],
        &[(EdgeKind::Uses, "mid", "seed"), (EdgeKind::Uses, "outer", "mid")],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", DEPENDENTS_HORIZON, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(
        report.disclosure,
        HorizonDisclosure::EndsWithinBound,
        "a shallow network must not falsely disclose a horizon cut: {report:?}"
    );
}

// _(`--depth 0` semantics)_ — depth 0 details nothing; every dependent, however shallow, falls into
// the aggregate, and disclosure follows the same rules as any other depth.
#[test]
fn dependents_depth_zero_is_aggregate_only() {
    use silent_cartographer::query::{DependentsReport, HorizonDisclosure};
    let store = dep_graph(
        &["seed", "a", "b"],
        &[(EdgeKind::Uses, "a", "seed"), (EdgeKind::Uses, "b", "seed")],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::seed", 0, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert!(report.detail.is_empty(), "depth 0 details nothing: {report:?}");
    let total: u64 = report.beyond_bound.iter().map(|a| a.count).sum();
    assert_eq!(total, 2, "both dependents fall into the aggregate: {report:?}");
    assert_eq!(report.disclosure, HorizonDisclosure::BeyondBound);
}

// _(Empty relation is typed absence)_ — a symbol with no dependents returns a definite empty answer,
// distinct from a failure.
#[test]
fn dependents_of_leaf_is_typed_absence() {
    let store = dep_graph(&["lonely"], &[]);
    let engine = dep_engine(&store);
    let answer = engine
        .dependents("test-ws::lonely", 1, None, None, OrderMode::Unranked)
        .expect("no dependents is a successful typed answer, not a failure");
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "a symbol with no dependents is typed Empty: {:?}",
        answer.outcome
    );
    assert!(answer.to_json().contains("\"empty\""), "renders the empty outcome tag");
}

// _(Scenario: Trace dependents at interface detail)_ — each detailed dependent row carries its own
// interface tier alongside the edge kind and distance.
#[test]
fn trace_dependents_interface_detail_carries_dependent_interface() {
    use silent_cartographer::query::DependentsReport;
    let store = GraphStore::open_in_memory().unwrap();
    put_dep_symbol(&store, "seed");
    put_dep_symbol_with_tiers(&store, "caller", None, Some("fn caller() -- calls seed"));
    store
        .insert_edge(EdgeKind::Uses, &dep_id("caller"), &dep_id("seed"))
        .unwrap();
    let engine = dep_engine(&store);

    let answer = engine
        .dependents("test-ws::seed", 1, Some(Detail::Interface), None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.detail.len(), 1);
    assert_eq!(
        report.detail[0].kind,
        DependencyKind::Uses,
        "detail carries the connecting kind"
    );
    assert_eq!(report.detail[0].distance, 1, "detail carries the hop distance");
    assert_eq!(
        report.detail[0].content.as_deref(),
        Some("fn caller() -- calls seed"),
        "the detailed row carries the dependent's own interface"
    );
}

// trace `--json` with detail carries per-row content; dependents beyond-bound aggregate rows carry no
// content field.
#[test]
fn trace_json_with_detail_carries_content_and_aggregates_carry_none() {
    let store = GraphStore::open_in_memory().unwrap();
    put_dep_symbol(&store, "seed");
    put_dep_symbol_with_tiers(&store, "mid", Some("fn mid()"), None);
    put_dep_symbol(&store, "outer");
    store
        .insert_edge(EdgeKind::Uses, &dep_id("mid"), &dep_id("seed"))
        .unwrap();
    store
        .insert_edge(EdgeKind::Uses, &dep_id("outer"), &dep_id("mid"))
        .unwrap();
    let engine = dep_engine(&store);

    let answer = engine
        .dependents("test-ws::seed", 1, Some(Detail::Signature), None, OrderMode::Unranked)
        .unwrap();
    let json = answer.to_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let report = &value["outcome"]["results"][0];

    let detail_rows = report["detail"].as_array().unwrap();
    assert_eq!(detail_rows.len(), 1, "only mid is within the depth-1 bound");
    assert_eq!(
        detail_rows[0]["content"], "fn mid()",
        "the detailed row carries mid's signature: {detail_rows:?}"
    );
    // The connecting kind reaches the machine answer as its plain stored tag, on the detailed row
    // and the aggregate alike — a closed vocabulary a caller can branch on without unwrapping.
    assert_eq!(
        detail_rows[0]["kind"], "uses",
        "the detailed row names the connecting kind: {detail_rows:?}"
    );

    let beyond_bound = report["beyond_bound"].as_array().unwrap();
    assert_eq!(beyond_bound.len(), 1, "outer is aggregated beyond the bound");
    assert_eq!(
        beyond_bound[0]["kind"], "uses",
        "the aggregate row names the connecting kind: {beyond_bound:?}"
    );
    assert!(
        beyond_bound[0].get("content").is_none(),
        "aggregate rows never carry a content field: {beyond_bound:?}"
    );
}

// _(`trace` cannot express the dependents relation)_ — `dependents` is the one command-surface
// relation that yields no trace relation, so it can only be answered by the dependents path; every
// other relation yields one and is answerable by `trace`.
//
// `trace` cannot express the depth-bounded, horizon-aggregated dependents payload. That used to be
// a run-time refusal from a call the types permitted; the split makes the call unwritable, so what
// remains to check is the routing decision itself. That `trace --relation dependents` still answers
// as a dependents assessment is covered end to end through the binary in `output_bounding.rs`.
#[test]
fn only_the_dependents_relation_yields_no_trace_relation() {
    let relations = [
        Relation::Containers,
        Relation::Contains,
        Relation::References,
        Relation::Importers,
        Relation::Implementers,
        Relation::Tests,
    ];
    for relation in relations {
        assert!(
            relation.as_trace().is_some(),
            "{relation:?} is answerable by trace and must yield a trace relation"
        );
    }
    assert!(
        Relation::Dependents.as_trace().is_none(),
        "dependents carries a depth bound and horizon aggregate a flat item list cannot hold"
    );
}

// _(Relationship trace — importers write-site)_ — `trace` over `importers` returns exactly the
// modules that import the subject.
#[test]
fn trace_importers_returns_modules_that_import_the_subject() {
    let store = dep_graph(
        &["subject", "importer_a", "importer_b", "unrelated"],
        &[
            (EdgeKind::Imports, "importer_a", "subject"),
            (EdgeKind::Imports, "importer_b", "subject"),
        ],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .trace("test-ws::subject", TraceRelation::Importers, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let ids: Vec<CanonicalId> = results
        .iter()
        .map(|item| match item {
            silent_cartographer::query::TraceItem::Symbol { symbol, .. } => symbol.canonical_id.clone(),
            other => panic!("expected a symbol row, got {other:?}"),
        })
        .collect();
    assert_eq!(
        ids,
        vec![dep_id("importer_a"), dep_id("importer_b")],
        "exactly the importers, ordered by identity: {ids:?}"
    );
}

// _(Relationship trace — implementers write-site)_ — `trace` over `implementers` returns exactly the
// types that declare the subject as a supertype.
#[test]
fn trace_implementers_returns_types_declaring_the_subject_as_supertype() {
    let store = dep_graph(
        &["subject", "impl_a", "impl_b", "unrelated"],
        &[
            (EdgeKind::TypeHierarchy, "impl_a", "subject"),
            (EdgeKind::TypeHierarchy, "impl_b", "subject"),
        ],
    );
    let engine = dep_engine(&store);
    let answer = engine
        .trace("test-ws::subject", TraceRelation::Implementers, None, None)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let ids: Vec<CanonicalId> = results
        .iter()
        .map(|item| match item {
            silent_cartographer::query::TraceItem::Symbol { symbol, .. } => symbol.canonical_id.clone(),
            other => panic!("expected a symbol row, got {other:?}"),
        })
        .collect();
    assert_eq!(
        ids,
        vec![dep_id("impl_a"), dep_id("impl_b")],
        "exactly the implementers, ordered by identity: {ids:?}"
    );
}

// _(Relationship trace — implementers detail projection through the shared cap point)_ — an
// `implementers` row at signature detail carries the implementer's own tier content, capped to its
// first `max_lines` lines with the truncation disclosed.
#[test]
fn trace_implementers_signature_detail_is_projected_and_capped() {
    use silent_cartographer::query::TraceItem;
    let store = GraphStore::open_in_memory().unwrap();
    put_dep_symbol(&store, "subject");
    // A multi-line signature, so a line cap has a tail to drop.
    let signature = "struct Implementer {\n    a: u8,\n    b: u8,\n}";
    put_dep_symbol_with_tiers(&store, "implementer", Some(signature), None);
    store
        .insert_edge(EdgeKind::TypeHierarchy, &dep_id("implementer"), &dep_id("subject"))
        .unwrap();
    let engine = dep_engine(&store);

    // Uncapped: the row projects its full signature tier.
    let full = engine
        .trace(
            "test-ws::subject",
            TraceRelation::Implementers,
            Some(Detail::Signature),
            None,
        )
        .unwrap();
    let Outcome::Found { results } = &full.outcome else {
        panic!("expected found, got {:?}", full.outcome);
    };
    match &results[0] {
        TraceItem::Symbol {
            content,
            content_truncated,
            ..
        } => {
            assert_eq!(content.as_deref(), Some(signature));
            assert!(!content_truncated, "the full signature is not truncated");
        }
        other => panic!("expected a symbol row, got {other:?}"),
    }

    // Capped: the content is cut to the first `max_lines` lines and the truncation is disclosed.
    let capped = engine
        .trace(
            "test-ws::subject",
            TraceRelation::Implementers,
            Some(Detail::Signature),
            Some(2),
        )
        .unwrap();
    let Outcome::Found { results } = &capped.outcome else {
        panic!("expected found, got {:?}", capped.outcome);
    };
    match &results[0] {
        TraceItem::Symbol {
            content,
            content_truncated,
            ..
        } => {
            assert_eq!(
                content.as_deref(),
                Some("struct Implementer {\n    a: u8,"),
                "the content is capped to the first max_lines lines"
            );
            assert!(content_truncated, "the cap is disclosed on the row");
        }
        other => panic!("expected a symbol row, got {other:?}"),
    }
}

// _(Relationship trace — importers empty branch)_ — a subject imported by nothing is typed absence,
// consistent with the other relations' empty behavior.
#[test]
fn trace_importers_with_none_is_typed_absence() {
    let store = dep_graph(&["subject"], &[]);
    let engine = dep_engine(&store);
    let answer = engine
        .trace("test-ws::subject", TraceRelation::Importers, None, None)
        .unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "no importers is typed Empty: {:?}",
        answer.outcome
    );
}

// _(Relationship trace — implementers empty branch)_ — a subject implemented by nothing is typed
// absence, consistent with `importers` and the other relations' empty behavior.
#[test]
fn trace_implementers_with_none_is_typed_absence() {
    let store = dep_graph(&["subject"], &[]);
    let engine = dep_engine(&store);
    let answer = engine
        .trace("test-ws::subject", TraceRelation::Implementers, None, None)
        .unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "no implementers is typed Empty: {:?}",
        answer.outcome
    );
}

// _(Depth-bounded impact answer — teaching error)_ — supplying `--depth` with a relation other than
// `dependents` fails with a teaching error naming the flag, the relation, and the accepting relation.
#[test]
fn depth_with_non_dependents_relation_is_a_teaching_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    silent_cartographer::commands::build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources())
        .unwrap();

    let err = silent_cartographer::commands::run_trace(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "net::Client",
        Relation::Contains,
        Some(2),
        None,
        /* max_lines */ 10,
        /* max_lines_explicit */ false,
        /* order */ silent_cartographer::query::OrderMode::Ranked,
        /* order_explicit */ false,
        /* limit */ 25,
        /* cursor */ None,
        false,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .expect_err("--depth with contains must fail");
    let msg = format!("{err:#}");
    assert!(msg.contains("--depth"), "names the flag: {msg}");
    assert!(msg.contains("contains"), "names the offending relation: {msg}");
    assert!(msg.contains("dependents"), "names the relation that accepts it: {msg}");
}

/// The detailed dependent identities of a `dependents` answer under `order`, in answer order.
fn detail_ids_under(engine: &QueryEngine<'_>, reference: &str, depth: u32, order: OrderMode) -> Vec<String> {
    let answer = engine.dependents(reference, depth, None, None, order).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    results[0]
        .detail
        .iter()
        .map(|d| d.symbol.canonical_id.as_str().to_string())
        .collect()
}

// _(Ranked ordering of dependents: within a layer the widely-depended-upon dependent ranks first)_ —
// two direct dependents differ in codebase-wide importance: the ranked ordering puts the
// widely-depended-upon one first even though identity order favors the other, while the unranked
// ordering keeps the structural identity order.
#[test]
fn ranked_order_puts_the_widely_depended_upon_dependent_first() {
    // `a_leaf` and `z_hub` both use the seed; three further symbols use `z_hub`, so it is the
    // widely-depended-upon dependent, while identity order would put `a_leaf` first.
    let store = dep_graph(
        &["seed", "a_leaf", "z_hub", "u1", "u2", "u3"],
        &[
            (EdgeKind::Uses, "a_leaf", "seed"),
            (EdgeKind::Uses, "z_hub", "seed"),
            (EdgeKind::Uses, "u1", "z_hub"),
            (EdgeKind::Uses, "u2", "z_hub"),
            (EdgeKind::Uses, "u3", "z_hub"),
        ],
    );
    let engine = dep_engine(&store);

    assert_eq!(
        detail_ids_under(&engine, "test-ws::seed", 1, OrderMode::Ranked),
        vec!["test-ws::z_hub", "test-ws::a_leaf"],
        "ranked puts the widely-depended-upon dependent first"
    );
    assert_eq!(
        detail_ids_under(&engine, "test-ws::seed", 1, OrderMode::Unranked),
        vec!["test-ws::a_leaf", "test-ws::z_hub"],
        "unranked keeps the structural identity order"
    );
}

// _(Ranked ordering of dependents: distance outranks importance)_ — the most widely-depended-upon
// symbol in the graph sits at distance 2; every distance-1 row is still returned before it.
#[test]
fn ranked_order_keeps_distance_primary() {
    let store = dep_graph(
        &["seed", "d1a", "d1b", "z_hub", "u1", "u2", "u3", "u4"],
        &[
            (EdgeKind::Uses, "d1a", "seed"),
            (EdgeKind::Uses, "d1b", "seed"),
            (EdgeKind::Uses, "z_hub", "d1a"),
            (EdgeKind::Uses, "u1", "z_hub"),
            (EdgeKind::Uses, "u2", "z_hub"),
            (EdgeKind::Uses, "u3", "z_hub"),
            (EdgeKind::Uses, "u4", "z_hub"),
        ],
    );
    let engine = dep_engine(&store);

    let ids = detail_ids_under(&engine, "test-ws::seed", 2, OrderMode::Ranked);
    let hub_at = ids.iter().position(|id| id == "test-ws::z_hub").expect("hub detailed");
    for d1 in ["test-ws::d1a", "test-ws::d1b"] {
        let at = ids.iter().position(|id| id == d1).expect("distance-1 row detailed");
        assert!(at < hub_at, "distance-1 row {d1} precedes the distance-2 hub: {ids:?}");
    }
}

// _(Dependents order selector: both orderings return the same answer set)_ — the same trace under
// each ordering contains exactly the same symbols with the same distances and kinds, the same
// beyond-bound aggregates, and the same horizon disclosure; only row order differs.
#[test]
fn ranked_and_unranked_return_the_same_answer_set() {
    use silent_cartographer::query::DependentsReport;
    let store = dep_graph(
        &["seed", "d1a", "d1b", "z_hub", "u1", "u2", "u3", "u4"],
        &[
            (EdgeKind::Uses, "d1a", "seed"),
            (EdgeKind::Imports, "d1b", "seed"),
            (EdgeKind::Uses, "z_hub", "d1a"),
            (EdgeKind::Uses, "u1", "z_hub"),
            (EdgeKind::Uses, "u2", "z_hub"),
            (EdgeKind::Uses, "u3", "z_hub"),
            (EdgeKind::Uses, "u4", "z_hub"),
        ],
    );
    let engine = dep_engine(&store);

    let report_under = |order: OrderMode| -> DependentsReport {
        let answer = engine.dependents("test-ws::seed", 2, None, None, order).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found");
        };
        results.into_iter().next().unwrap()
    };
    let ranked = report_under(OrderMode::Ranked);
    let unranked = report_under(OrderMode::Unranked);

    let row_set = |report: &DependentsReport| -> std::collections::BTreeSet<(String, u32, DependencyKind)> {
        report
            .detail
            .iter()
            .map(|d| (d.symbol.canonical_id.as_str().to_string(), d.distance, d.kind))
            .collect()
    };
    assert_eq!(row_set(&ranked), row_set(&unranked), "the answer set never changes");
    assert_eq!(ranked.beyond_bound, unranked.beyond_bound, "aggregates are untouched");
    assert_eq!(
        ranked.disclosure, unranked.disclosure,
        "the horizon disclosure is untouched"
    );
    assert_eq!(ranked.depth_bound, unranked.depth_bound);
    assert_eq!(ranked.horizon, unranked.horizon);
}

// _(Dependents order selector: unranked ordering on request)_ — the unranked ordering is exactly
// distance, then dependency kind order (`uses` < `imports` < `type_hierarchy`), then canonical
// identity; the identities are chosen so a plain identity sort would disagree.
#[test]
fn unranked_order_is_distance_kind_then_identity() {
    let store = dep_graph(
        &["seed", "zz", "aa", "mm"],
        &[
            (EdgeKind::Uses, "zz", "seed"),
            (EdgeKind::Imports, "aa", "seed"),
            (EdgeKind::TypeHierarchy, "mm", "seed"),
        ],
    );
    let engine = dep_engine(&store);
    assert_eq!(
        detail_ids_under(&engine, "test-ws::seed", 1, OrderMode::Unranked),
        vec!["test-ws::zz", "test-ws::aa", "test-ws::mm"],
        "kind order breaks the distance tie, not identity"
    );
}

// _(Dependents order selector: an order requested outside its relations is refused)_ — an explicit
// `--order` with a non-`dependents` relation fails before any traversal with a teaching error
// naming the flag and where the selector applies; the defaulted value stays dormant.
#[test]
fn order_with_non_dependents_relation_is_a_teaching_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    silent_cartographer::commands::build_from_index(&db, "op-ws", dir.path(), &support::fixture_index(), &sources())
        .unwrap();

    let err = silent_cartographer::commands::run_trace(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "net::Client",
        Relation::References,
        None,
        None,
        /* max_lines */ 10,
        /* max_lines_explicit */ false,
        /* order */ OrderMode::Ranked,
        /* order_explicit */ true,
        /* limit */ 25,
        /* cursor */ None,
        false,
        false,
        silent_cartographer::cli::PositionUnit::Line,
    )
    .expect_err("--order with references must fail");
    let msg = format!("{err:#}");
    assert!(msg.contains("--order"), "names the flag: {msg}");
    assert!(msg.contains("references"), "names the offending relation: {msg}");
    assert!(msg.contains("dependents"), "names where the selector applies: {msg}");
    assert!(
        msg.contains("impact"),
        "names the sibling command that accepts it: {msg}"
    );
}

// _(Ordering disclosure: the self-description states the heuristic)_ — `trace`'s and `impact`'s
// self-descriptions present the ranked ordering as a structural-importance heuristic.
#[test]
fn order_self_description_presents_ranked_as_heuristic() {
    use clap::CommandFactory;
    let mut cmd = silent_cartographer::cli::Cli::command();
    for name in ["trace", "impact"] {
        let mut sub = cmd
            .find_subcommand_mut(name)
            .unwrap_or_else(|| panic!("{name} subcommand present"))
            .clone();
        let help = sub.render_long_help().to_string();
        assert!(
            help.contains("structural-importance heuristic"),
            "{name}'s self-description presents ranked ordering as heuristic: {help}"
        );
    }
}

// _(Relationship trace — self-description)_ — the command's self-description presents `dependents` as
// impact assessment.
#[test]
fn trace_self_description_frames_dependents_as_impact() {
    use clap::CommandFactory;
    let mut cmd = silent_cartographer::cli::Cli::command();
    let mut trace = cmd
        .find_subcommand_mut("trace")
        .expect("trace subcommand present")
        .clone();
    let help = trace.render_long_help().to_string();
    assert!(
        help.contains("impact assessment"),
        "the self-description frames dependents as impact assessment: {help}"
    );
    assert!(
        help.contains("what could break"),
        "the self-description carries the impact question: {help}"
    );
}

// _(Calibrated output contract)_ — repeated identical queries return locations in the same order.
#[test]
fn deterministic_ordering_across_repeated_queries() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let first = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let second = engine
        .trace("net::Client", TraceRelation::References, None, None)
        .unwrap();
    let locs = |a: &silent_cartographer::query::output::Answer<silent_cartographer::query::TraceItem>| {
        if let Outcome::Found { results } = &a.outcome {
            results
                .iter()
                .filter_map(|i| match i {
                    silent_cartographer::query::TraceItem::Reference { location, .. } => {
                        Some((location.span_start, location.span_end))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    assert_eq!(locs(&first), locs(&second), "identical queries return the same order");
}

// Python (fixture-level): navigation over the committed python-conformance fixture — no live tool.

fn py_ws() -> WorkspaceId {
    WorkspaceId::new("py-ws")
}

fn py_id(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    project_one(&py_ws(), &Descriptor::new("python-conformance", segs))
}

fn py_widget_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("Widget", SegmentKind::Type)])
}

fn py_render_id() -> CanonicalId {
    py_id(&[
        ("pkg.shapes", SegmentKind::Module),
        ("Widget", SegmentKind::Type),
        ("render", SegmentKind::Method),
    ])
}

fn py_build_id() -> CanonicalId {
    py_id(&[("pkg.consumer", SegmentKind::Module), ("build", SegmentKind::Method)])
}

fn py_consumer_module_id() -> CanonicalId {
    py_id(&[("pkg.consumer", SegmentKind::Module), ("__init__", SegmentKind::Meta)])
}

fn py_store() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(
        &mut store,
        &py_ws(),
        Some(WS_ROOT),
        &support::python_fixture_index(),
        &support::python_fixture_sources(),
    )
    .unwrap();
    store
}

fn py_engine(store: &GraphStore) -> QueryEngine<'_> {
    let provenance = support::python_fixture_index().provenance;
    let hash = silent_cartographer::graph::content_hash(&support::python_fixture_sources());
    QueryEngine::new(store, provenance, hash, None)
}

/// The fixture text of a Python source, for byte-exact expectations.
fn py_source(rel: &str) -> String {
    support::python_fixture_sources()
        .into_iter()
        .find(|(p, _)| p == rel)
        .expect("fixture source present")
        .1
}

// _(Scenario: Retrieve full body of a Python symbol)_ — the body spans several indentation levels
// and is returned byte-exact.
#[test]
fn python_body_retrieval_is_byte_exact() {
    use silent_cartographer::query::DetailPayload;
    let store = py_store();
    let engine = py_engine(&store);
    let answer = engine.get(py_render_id().as_str(), Detail::Body, None, 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };

    // The exact bytes of the whole `render` definition, sliced from the fixture source itself.
    let source = py_source("pkg/shapes.py");
    let start = source.find("def render").unwrap();
    let end = source.find("\"unreachable\"").unwrap() + "\"unreachable\"".len();
    let expected = &source[start..end];

    match &results[0].payload {
        DetailPayload::Body { body: Some(body) } => assert_eq!(body, expected, "byte-exact body"),
        other => panic!("expected a body, got {other:?}"),
    }
}

// _(Scenario: Retrieve Python symbol by position)_ — a position inside a method body resolves to
// the enclosing method.
#[test]
fn python_get_by_position_resolves_enclosing_method() {
    let store = py_store();
    let engine = py_engine(&store);
    let source = py_source("pkg/shapes.py");
    let inside_render = source.find("return \"widget\"").unwrap();
    let answer = engine
        .get_by_position(
            "pkg/shapes.py",
            SourcePosition::Byte(inside_render),
            Detail::Location,
            None,
            1,
        )
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(
        results[0].symbol.canonical_id,
        py_render_id(),
        "position resolves to the enclosing method, not the class or module"
    );
}

// _(Scenario: Trace dependents of a Python symbol)_ — Widget's dependents carry the connecting edge
// kind and hop distance: `build` uses it (the call inside the function body) and the consuming
// module imports it (the module-scope import line).
#[test]
fn python_dependents_trace_carries_kind_and_distance() {
    use silent_cartographer::query::DependentsReport;
    let store = py_store();
    let engine = py_engine(&store);
    let answer = engine
        .dependents(py_widget_id().as_str(), 1, None, None, OrderMode::Unranked)
        .unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let report: &DependentsReport = &results[0];

    let uses = report
        .detail
        .iter()
        .find(|d| d.symbol.canonical_id == py_build_id())
        .unwrap_or_else(|| panic!("build is a dependent: {:?}", report.detail));
    assert_eq!(
        uses.kind,
        DependencyKind::Uses,
        "the function that calls Widget is a uses dependent"
    );
    assert_eq!(uses.distance, 1);

    let imports = report
        .detail
        .iter()
        .find(|d| d.symbol.canonical_id == py_consumer_module_id())
        .unwrap_or_else(|| panic!("the importing module is a dependent: {:?}", report.detail));
    assert_eq!(
        imports.kind,
        DependencyKind::Imports,
        "the importing module is an imports dependent"
    );
    assert_eq!(imports.distance, 1);
}

// _(Ranked ordering of dependents — Python-built store)_ — the ranked ordering reflects
// codebase-wide importance over a store built through the Python pipeline: with the consumer
// module made more widely depended-upon than `build`, ranked order inverts the unranked kind order
// (`uses` before `imports`) that would otherwise put `build` first.
#[test]
fn python_ranked_order_reflects_codebase_importance() {
    let store = py_store();
    // Two further in-workspace symbols depending on the consumer module raise its codebase-wide
    // importance above `build`'s.
    for name in ["extra_a", "extra_b"] {
        let id = CanonicalId::from_raw(format!("python-conformance-ws::{name}"));
        store
            .insert_symbol(&SymbolRow {
                canonical_id: id.clone(),
                display_name: name.to_string(),
                kind: "function".to_string(),
                class: PersistedClass::InWorkspace,
                document_path: None,
                span: None,
                line_column: None,
                header_end: None,
                span_text: None,
                signature_text: None,
                interface_text: None,
                duplicated: false,
                test_rule: None,
            })
            .unwrap();
        store
            .insert_edge(EdgeKind::Uses, &id, &py_consumer_module_id())
            .unwrap();
    }
    let engine = py_engine(&store);

    let ids = |order: OrderMode| -> Vec<CanonicalId> {
        let answer = engine
            .dependents(py_widget_id().as_str(), 1, None, None, order)
            .unwrap();
        let Outcome::Found { results } = &answer.outcome else {
            panic!("expected found, got {:?}", answer.outcome);
        };
        results[0]
            .detail
            .iter()
            .map(|d| d.symbol.canonical_id.clone())
            .collect()
    };

    let ranked = ids(OrderMode::Ranked);
    let unranked = ids(OrderMode::Unranked);
    let position = |ids: &[CanonicalId], id: &CanonicalId| ids.iter().position(|x| x == id).expect("row present");
    assert!(
        position(&ranked, &py_consumer_module_id()) < position(&ranked, &py_build_id()),
        "ranked puts the widely-depended-upon consumer module first: {ranked:?}"
    );
    assert!(
        position(&unranked, &py_build_id()) < position(&unranked, &py_consumer_module_id()),
        "unranked keeps the kind order, `uses` before `imports`: {unranked:?}"
    );
}

// _(Scenario: Python dotted qualified name resolves)_ — a `pkg.module.Class` style reference
// resolves to exactly one symbol.
#[test]
fn python_dotted_qualified_name_resolves() {
    let store = py_store();
    let engine = py_engine(&store);
    match engine.resolve("pkg.shapes.Widget").unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, py_widget_id()),
        other => panic!("expected a unique resolution, got {other:?}"),
    }
}

const TR_LIB_DOC: &str = "src/lib.rs";
const TR_API_DOC: &str = "tests/api.rs";

const TR_LIB_SOURCE: &str = "\
pub fn subject_a() {}
pub fn subject_b() {}
pub fn subject_c() {}
pub fn subject_d() {}
pub fn subject_e() {}

pub fn production_caller() {
    subject_a();
    subject_d();
}

#[test]
fn test_caller() {
    subject_a();
    subject_e();
}

#[test]
fn test_caller_two() {
    subject_a();
}
";

const TR_API_SOURCE: &str = "\
use crate::subject_c;

pub fn helper() {
    subject_b();
    subject_e();
}
";

fn tr_source(doc: &str) -> &'static str {
    match doc {
        TR_LIB_DOC => TR_LIB_SOURCE,
        TR_API_DOC => TR_API_SOURCE,
        other => panic!("unknown fixture doc {other}"),
    }
}

/// A fixture symbol whose occurrences are `(doc, occurrence-of-token, role)` sites, each locating
/// the n-th (1-based) appearance of the symbol's own name in that document.
fn tr_symbol(name: &str, kind: SymbolKind, occs: &[(&str, usize, OccurrenceRole)]) -> ExtractedSymbol {
    let occurrences = occs
        .iter()
        .map(|(doc, nth, role)| {
            let source = tr_source(doc);
            let pos = source.match_indices(name).nth(nth - 1).expect("token present").0;
            let (l, c) = line_col(source, pos);
            ExtractedOccurrence {
                document_path: doc.to_string(),
                range: SourceRange::new(l, c, l, c + name.len() as u32),
                role: *role,
            }
        })
        .collect();
    ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "t",
            vec![DescriptorSegment::new(name, SegmentKind::Method)],
        )),
        kind,
        class: SymbolClass::InWorkspace,
        occurrences,
    }
}

fn tr_id(name: &str) -> CanonicalId {
    id_of_pkg("t", &[(name, SegmentKind::Method)])
}

fn tr_sources() -> Vec<(String, String)> {
    vec![
        (TR_LIB_DOC.to_string(), TR_LIB_SOURCE.to_string()),
        (TR_API_DOC.to_string(), TR_API_SOURCE.to_string()),
    ]
}

/// The `tests`-relation fixture store: subjects with mixed callers, a test-only helper caller, a
/// module-scope test reference, and a production-only subject, across a production document and an
/// integration-test document.
fn tr_store() -> GraphStore {
    use OccurrenceRole::{Definition, Reference};
    let mut symbols = vec![
        tr_symbol(
            "subject_a",
            SymbolKind::Function,
            &[
                (TR_LIB_DOC, 1, Definition),
                (TR_LIB_DOC, 2, Reference),
                (TR_LIB_DOC, 3, Reference),
                (TR_LIB_DOC, 4, Reference),
            ],
        ),
        tr_symbol(
            "subject_b",
            SymbolKind::Function,
            &[(TR_LIB_DOC, 1, Definition), (TR_API_DOC, 1, Reference)],
        ),
        tr_symbol(
            "subject_c",
            SymbolKind::Function,
            &[(TR_LIB_DOC, 1, Definition), (TR_API_DOC, 1, Reference)],
        ),
        tr_symbol(
            "subject_d",
            SymbolKind::Function,
            &[(TR_LIB_DOC, 1, Definition), (TR_LIB_DOC, 2, Reference)],
        ),
        tr_symbol(
            "subject_e",
            SymbolKind::Function,
            &[
                (TR_LIB_DOC, 1, Definition),
                (TR_LIB_DOC, 2, Reference),
                (TR_API_DOC, 1, Reference),
            ],
        ),
        tr_symbol(
            "production_caller",
            SymbolKind::Function,
            &[(TR_LIB_DOC, 1, Definition)],
        ),
        tr_symbol("test_caller", SymbolKind::Function, &[(TR_LIB_DOC, 1, Definition)]),
        tr_symbol("test_caller_two", SymbolKind::Function, &[(TR_LIB_DOC, 1, Definition)]),
        tr_symbol("helper", SymbolKind::Function, &[(TR_API_DOC, 1, Definition)]),
    ];
    // The integration-test document's file module, the attribution target of its module-scope
    // reference sites: a whole-document definition occurrence (line one past the final newline).
    let api_lines = TR_API_SOURCE.matches('\n').count() as u32;
    symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "t",
            vec![DescriptorSegment::new("api", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: TR_API_DOC.to_string(),
            range: SourceRange::new(0, 0, api_lines, 0),
            role: OccurrenceRole::Definition,
        }],
    });
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![
            SourceDocument {
                path: TR_LIB_DOC.to_string(),
                encoding: PositionEncoding::Utf8,
            },
            SourceDocument {
                path: TR_API_DOC.to_string(),
                encoding: PositionEncoding::Utf8,
            },
        ],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &tr_sources()).unwrap();
    store
}

fn tr_engine(store: &GraphStore) -> QueryEngine<'_> {
    let hash = silent_cartographer::graph::content_hash(&tr_sources());
    QueryEngine::new(store, support::provenance(), hash, None)
}

/// The reference locations of a found trace answer, in answer order.
fn tr_locations(
    answer: &silent_cartographer::query::output::Answer<silent_cartographer::query::TraceItem>,
) -> Vec<(String, usize)> {
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    results
        .iter()
        .map(|item| match item {
            silent_cartographer::query::TraceItem::Reference { location, .. } => {
                (location.document_path.clone(), location.span_start)
            }
            other => panic!("expected a reference, got {other:?}"),
        })
        .collect()
}

// _(Scenario: Trace tests of a Rust symbol)_ — a subject referenced from test-classified functions
// and a production function returns exactly the test-classified sites, and the production site is
// not among them.
#[test]
fn trace_tests_returns_exactly_test_classified_sites() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine.trace("subject_a", TraceRelation::Tests, None, None).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 2, "both test callers, never the production caller");
    for item in results {
        match item {
            silent_cartographer::query::TraceItem::Reference { enclosing, .. } => {
                let enclosing = enclosing.as_ref().expect("attributed to a declaration");
                assert!(
                    [tr_id("test_caller"), tr_id("test_caller_two")].contains(enclosing),
                    "the site is attributed to a test-classified declaration: {enclosing}"
                );
            }
            other => panic!("expected a reference, got {other:?}"),
        }
    }
}

// _(Scenario: Tests reach through a shared helper)_ — a subject referenced only from a
// test-classified helper returns the helper's site rather than an empty answer.
#[test]
fn trace_tests_reaches_through_a_shared_helper() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine.trace("subject_b", TraceRelation::Tests, None, None).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 1);
    match &results[0] {
        silent_cartographer::query::TraceItem::Reference { enclosing, .. } => {
            assert_eq!(
                enclosing.as_ref(),
                Some(&tr_id("helper")),
                "the helper's site is returned"
            );
        }
        other => panic!("expected a reference, got {other:?}"),
    }
}

// _(Scenario: Module-scope test references count)_ — a subject named by an import statement at
// module scope in a test-classified document returns that site, attributed to the document's module.
#[test]
fn trace_tests_module_scope_reference_counts() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine.trace("subject_c", TraceRelation::Tests, None, None).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 1);
    match &results[0] {
        silent_cartographer::query::TraceItem::Reference {
            enclosing, location, ..
        } => {
            assert_eq!(*enclosing, None, "the site attributes to the module itself");
            assert_eq!(location.document_path, TR_API_DOC);
        }
        other => panic!("expected a reference, got {other:?}"),
    }
}

// _(Scenario: Empty relation is typed absence — tests)_ — a subject with only production references
// returns a definite empty set, distinct from an unavailable or failed answer.
#[test]
fn trace_tests_only_production_references_is_typed_absence() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine.trace("subject_d", TraceRelation::Tests, None, None).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "a definite empty set: {:?}",
        answer.outcome
    );
}

// _(Scenario: Trace tests of a Python symbol)_ — a Python function referenced from a declaration in
// a test-classified document returns that reference site with its location.
#[test]
fn trace_tests_python_site_returned() {
    let api_source = "def subject():\n    pass\n";
    let test_source = "def test_subject():\n    subject()\n";
    let subject_def = api_source.find("subject").unwrap();
    let subject_ref = test_source.rfind("subject()").unwrap();
    let test_fn_def = test_source.find("test_subject").unwrap();
    let occ = |doc: &str, source: &str, pos: usize, len: usize, role: OccurrenceRole| {
        let (l, c) = line_col(source, pos);
        ExtractedOccurrence {
            document_path: doc.to_string(),
            range: SourceRange::new(l, c, l, c + len as u32),
            role,
        }
    };
    let symbols = vec![
        ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "p",
                vec![DescriptorSegment::new("subject", SegmentKind::Method)],
            )),
            kind: SymbolKind::Function,
            class: SymbolClass::InWorkspace,
            occurrences: vec![
                occ("pkg/api.py", api_source, subject_def, 7, OccurrenceRole::Definition),
                occ(
                    "pkg/test_api.py",
                    test_source,
                    subject_ref,
                    7,
                    OccurrenceRole::Reference,
                ),
            ],
        },
        ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "p",
                vec![DescriptorSegment::new("test_subject", SegmentKind::Method)],
            )),
            kind: SymbolKind::Function,
            class: SymbolClass::InWorkspace,
            occurrences: vec![occ(
                "pkg/test_api.py",
                test_source,
                test_fn_def,
                12,
                OccurrenceRole::Definition,
            )],
        },
    ];
    let index = ExtractedIndex {
        provenance: support::python_fixture_index().provenance,
        documents: vec![
            SourceDocument {
                path: "pkg/api.py".to_string(),
                encoding: PositionEncoding::Utf8,
            },
            SourceDocument {
                path: "pkg/test_api.py".to_string(),
                encoding: PositionEncoding::Utf8,
            },
        ],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let sources = vec![
        ("pkg/api.py".to_string(), api_source.to_string()),
        ("pkg/test_api.py".to_string(), test_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &py_ws(), Some(WS_ROOT), &index, &sources).unwrap();
    let provenance = support::python_fixture_index().provenance;
    let hash = silent_cartographer::graph::content_hash(&sources);
    let engine = QueryEngine::new(&store, provenance, hash, None);

    let answer = engine.trace("subject", TraceRelation::Tests, None, None).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    assert_eq!(results.len(), 1);
    match &results[0] {
        silent_cartographer::query::TraceItem::Reference { location, .. } => {
            assert_eq!(location.document_path, "pkg/test_api.py");
            assert!(location.span_end > location.span_start);
        }
        other => panic!("expected a reference, got {other:?}"),
    }
}

// _(Scenario: Machine answer carries the marker / Each site carries its classification rule /
// Resolved relations carry no heuristic marker)_ — a `tests` answer carries the answer-level
// convention marker and a per-site rule on every returned site (each site its own accepting rule,
// under more than one rule in one answer); a `references` answer over the same subject carries
// neither.
#[test]
fn tests_json_carries_marker_and_per_site_rules_references_carries_neither() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let tests_json = engine
        .trace("subject_e", TraceRelation::Tests, None, None)
        .unwrap()
        .to_json();
    assert!(
        tests_json.contains("\"classification\": \"convention\""),
        "the answer-level marker is structural: {tests_json}"
    );
    assert!(
        tests_json.contains("\"test_rule\": \"test_attribute\""),
        "the attribute-classified site carries its rule: {tests_json}"
    );
    assert!(
        tests_json.contains("\"test_rule\": \"test_directory\""),
        "the directory-classified site carries its rule: {tests_json}"
    );

    let refs_json = engine
        .trace("subject_e", TraceRelation::References, None, None)
        .unwrap()
        .to_json();
    assert!(
        !refs_json.contains("classification"),
        "a resolved relation carries no heuristic marker: {refs_json}"
    );
    assert!(
        !refs_json.contains("test_rule"),
        "a resolved relation's sites carry no rule field: {refs_json}"
    );
}

// _(Scenario: Empty answer keeps the marker)_ — an empty `tests` answer asserts only that no
// convention-classified reference site was found, and carries the marker like any other `tests`
// answer.
#[test]
fn empty_tests_json_keeps_the_marker() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine.trace("subject_d", TraceRelation::Tests, None, None).unwrap();
    assert!(matches!(answer.outcome, Outcome::Empty), "{:?}", answer.outcome);
    let json = answer.to_json();
    assert!(
        json.contains("\"classification\": \"convention\""),
        "the empty answer still carries the convention marker: {json}"
    );
}

// _(Scenario: Unresolved subject carries no marker)_ — an absent answer terminates before
// classification is consulted and contains no classification-derived content, so the marker — which
// asserts a derivation, never merely the relation requested — stays off.
#[test]
fn unresolved_tests_subject_carries_no_marker() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let answer = engine
        .trace("no_such_symbol", TraceRelation::Tests, None, None)
        .unwrap();
    assert!(matches!(answer.outcome, Outcome::Absent), "{:?}", answer.outcome);
    assert!(
        !answer.to_json().contains("classification"),
        "a typed absence asserts no classification derivation"
    );
}

// _(Scenario: Ambiguous subject carries no marker)_ — an ambiguous-reference answer terminates
// before classification is consulted and contains no classification-derived content, so — like the
// unresolved arm — the marker stays off.
#[test]
fn ambiguous_tests_subject_carries_no_marker() {
    // Two symbols named `connect` in different types → ambiguous shortname (the same setup the
    // ambiguous `get` test uses).
    let mut index = support::fixture_index();
    index
        .symbols
        .push(silent_cartographer::semantic::model::ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "mycrate",
                vec![
                    DescriptorSegment::new("net", SegmentKind::Module),
                    DescriptorSegment::new("Server", SegmentKind::Type),
                    DescriptorSegment::new("connect", SegmentKind::Method),
                ],
            )),
            kind: silent_cartographer::semantic::model::SymbolKind::Method,
            class: silent_cartographer::semantic::model::SymbolClass::InWorkspace,
            occurrences: vec![silent_cartographer::semantic::model::ExtractedOccurrence {
                document_path: support::DOC.to_string(),
                range: silent_cartographer::semantic::model::SourceRange::new(3, 15, 3, 22),
                role: silent_cartographer::semantic::model::OccurrenceRole::Definition,
            }],
        });
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &sources()).unwrap();
    let engine = engine_over(&store, support::provenance());

    let answer = engine.trace("connect", TraceRelation::Tests, None, None).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Ambiguous { .. }),
        "{:?}",
        answer.outcome
    );
    assert!(
        !answer.to_json().contains("classification"),
        "an ambiguous answer asserts no classification derivation"
    );
}

// _(Scenario: Heuristic marker composes with staleness)_ — a `tests` answer against an index whose
// sources changed carries both the staleness flag and the heuristic-grade marker, each
// independently.
#[test]
fn tests_marker_composes_with_staleness() {
    let store = tr_store();
    let engine = QueryEngine::new(&store, support::provenance(), "drifted-hash".to_string(), None);
    let answer = engine.trace("subject_a", TraceRelation::Tests, None, None).unwrap();
    assert!(answer.stale, "the changed sources flag the answer stale");
    let json = answer.to_json();
    assert!(json.contains("\"stale\": true"), "{json}");
    assert!(
        json.contains("\"classification\": \"convention\""),
        "the marker rides independently of freshness: {json}"
    );
}

// _(Relationship trace — self-description, tests)_ — the command's self-description presents `tests`
// as convention-based classification rather than resolved semantic fact, mirroring the
// dependents-as-impact framing.
#[test]
fn trace_self_description_frames_tests_as_convention_based() {
    use clap::CommandFactory;
    let mut cmd = silent_cartographer::cli::Cli::command();
    let mut trace = cmd
        .find_subcommand_mut("trace")
        .expect("trace subcommand present")
        .clone();
    let help = trace.render_long_help().to_string();
    assert!(
        help.contains("what test code exercises this symbol"),
        "the self-description carries the tests question: {help}"
    );
    assert!(
        help.contains("not resolved semantic fact"),
        "the self-description names the convention-based grade: {help}"
    );
}

// _(Scenario: Deterministic ordering — tests)_ — repeated identical `tests` queries return identical
// ordering, matching the order the same sites carry in a `references` answer.
#[test]
fn trace_tests_ordering_is_deterministic_and_matches_references() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let first = tr_locations(&engine.trace("subject_a", TraceRelation::Tests, None, None).unwrap());
    let second = tr_locations(&engine.trace("subject_a", TraceRelation::Tests, None, None).unwrap());
    assert_eq!(first, second, "repeated queries return identical ordering");

    let references = tr_locations(
        &engine
            .trace("subject_a", TraceRelation::References, None, None)
            .unwrap(),
    );
    let filtered: Vec<_> = references.into_iter().filter(|loc| first.contains(loc)).collect();
    assert_eq!(first, filtered, "the tests answer preserves the references ordering");
}

// _(Scenario: Trace at signature detail — tests)_ — a `tests` query at signature detail carries the
// attributed declaration's signature tier without changing the result set.
#[test]
fn trace_tests_signature_detail_projects_without_changing_results() {
    let store = tr_store();
    let engine = tr_engine(&store);
    let plain = tr_locations(&engine.trace("subject_b", TraceRelation::Tests, None, None).unwrap());
    let answer = engine
        .trace("subject_b", TraceRelation::Tests, Some(Detail::Signature), None)
        .unwrap();
    assert_eq!(tr_locations(&answer), plain, "detail never changes the result set");
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    match &results[0] {
        silent_cartographer::query::TraceItem::Reference { content, .. } => {
            let expected = store
                .symbol(&tr_id("helper"))
                .unwrap()
                .expect("helper persisted")
                .signature_text;
            assert_eq!(
                *content, expected,
                "the row carries the attributed declaration's signature"
            );
        }
        other => panic!("expected a reference, got {other:?}"),
    }
}

/// Spawn the built `c10r` with `args` from `dir`, with `path` as `PATH` and no inherited
/// `$VIRTUAL_ENV` unless `virtual_env` names one.
fn run_c10r(
    dir: &std::path::Path,
    path: &std::ffi::OsStr,
    virtual_env: Option<&std::path::Path>,
    args: &[&std::ffi::OsStr],
) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"));
    command
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env_remove("VIRTUAL_ENV");
    if let Some(env) = virtual_env {
        command.env("VIRTUAL_ENV", env);
    }
    command.output().unwrap()
}

/// A Rust workspace at `dir` built through the built binary (a stub `rust-analyzer`, extra `build`
/// arguments `build_args`), then edited so `find` reads stale. Returns the stale answer and the `PATH`
/// under which both the stub analyzer and `c10r` resolve.
fn stale_rust_workspace(
    dir: &std::path::Path,
    db: &std::path::Path,
    build_args: &[&str],
) -> (serde_json::Value, std::ffi::OsString) {
    std::fs::write(dir.join("Cargo.toml"), "# fixture manifest\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn hello() {}\n").unwrap();
    let bin_dir = dir.join(".stub-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_indexing_stub(&bin_dir, "rust-analyzer", "1.85.0");
    let built_bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_c10r"))
        .parent()
        .unwrap()
        .to_path_buf();
    let path = prepend_path(&[&built_bin_dir, &bin_dir]);

    let root = std::fs::canonicalize(dir).unwrap();
    let mut args: Vec<&std::ffi::OsStr> = vec!["build".as_ref(), root.as_os_str(), "--db".as_ref(), db.as_os_str()];
    args.extend(build_args.iter().map(std::ffi::OsStr::new));
    let build = run_c10r(dir, &path, None, &args);
    assert!(
        build.status.success(),
        "the build succeeds: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    std::fs::write(dir.join("src/lib.rs"), "pub fn hello() {}\n// edited\n").unwrap();
    (find_json(dir, db, &path, build_args), path)
}

/// `find hello --json` through the built binary, naming the workspace identity `build_args` carries.
fn find_json(
    dir: &std::path::Path,
    db: &std::path::Path,
    path: &std::ffi::OsStr,
    build_args: &[&str],
) -> serde_json::Value {
    let mut args: Vec<&std::ffi::OsStr> = vec!["--db".as_ref(), db.as_os_str(), "--json".as_ref()];
    args.extend(
        build_args
            .iter()
            .filter(|a| a.starts_with("--workspace="))
            .map(std::ffi::OsStr::new),
    );
    args.extend([std::ffi::OsStr::new("find"), std::ffi::OsStr::new("hello")]);
    let out = run_c10r(dir, path, None, &args);
    assert!(
        out.status.success(),
        "the query succeeds: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Run `refresh` through `sh` from an unrelated directory and assert it succeeds.
fn run_refresh(refresh: &str, path: &std::ffi::OsStr) {
    let elsewhere = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(refresh)
        .env("PATH", path)
        .env_remove("VIRTUAL_ENV")
        .current_dir(elsewhere.path())
        .status()
        .unwrap();
    assert!(status.success(), "the refresh command runs to completion: {refresh}");
}

// _(Scenario: The refresh command keeps the recorded chunk parameters)_ — an index built with
// non-default chunk parameters answers stale with a command that carries them, and running it makes
// the answer fresh.
#[test]
fn the_refresh_command_carries_non_default_chunk_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".c10r").join("index.db");
    let (stale, path) = stale_rust_workspace(
        dir.path(),
        &db,
        &["--workspace=chunky-ws", "--chunk-size=128", "--chunk-overlap=16"],
    );
    assert_eq!(stale["freshness"], "stale_content", "{stale}");
    let refresh = stale["refresh"].as_str().unwrap();
    assert!(
        refresh.contains("--chunk-size=128 --chunk-overlap=16"),
        "the command carries the recorded chunk parameters: {refresh}"
    );

    run_refresh(refresh, &path);
    let fresh = find_json(dir.path(), &db, &path, &["--workspace=chunky-ws"]);
    assert_eq!(fresh["freshness"], "fresh", "{fresh}");
}

// _(Scenario: The refresh command handles a workspace identity that starts with a dash)_ — the
// command's workspace argument is not read as a flag, so it runs and the answer becomes fresh.
#[test]
fn the_refresh_command_runs_for_a_workspace_identity_starting_with_a_dash() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".c10r").join("index.db");
    let (stale, path) = stale_rust_workspace(dir.path(), &db, &["--workspace=-foo"]);
    assert_eq!(stale["freshness"], "stale_content", "{stale}");
    let refresh = stale["refresh"].as_str().unwrap();

    run_refresh(refresh, &path);
    let fresh = find_json(dir.path(), &db, &path, &["--workspace=-foo"]);
    assert_eq!(fresh["freshness"], "fresh", "{fresh}");
}

// _(Scenario: An activated environment answers fresh however its path is spelled)_ — the same
// environment named through `$VIRTUAL_ENV` by a symlink at build time and through a `/./` spelling at
// query time is one environment, so the answer is fresh.
#[test]
fn a_virtual_env_spelled_differently_at_build_and_query_answers_fresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"pkg\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let venv = write_fake_venv(dir.path());
    let link = dir.path().join("active-env");
    std::os::unix::fs::symlink(&venv, &link).unwrap();
    let dotted = dir.path().join(".").join(".venv");
    let db = dir.path().join(".c10r").join("index.db");
    let bin_dir = tempfile::tempdir().unwrap();
    write_indexing_stub(bin_dir.path(), "scip-python", "0.6.0");
    let path = hermetic_path(bin_dir.path());
    let root = std::fs::canonicalize(dir.path()).unwrap();

    let build = run_c10r(
        dir.path(),
        &path,
        Some(&link),
        &[
            "build".as_ref(),
            root.as_os_str(),
            "--db".as_ref(),
            db.as_os_str(),
            "--workspace=venv-ws".as_ref(),
        ],
    );
    assert!(
        build.status.success(),
        "the build succeeds: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let query = run_c10r(
        dir.path(),
        &path,
        Some(&dotted),
        &[
            "--db".as_ref(),
            db.as_os_str(),
            "--json".as_ref(),
            "find".as_ref(),
            "hello".as_ref(),
        ],
    );
    assert!(
        query.status.success(),
        "the query succeeds: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&query.stdout).unwrap();
    assert_eq!(value["freshness"], "fresh", "{value}");
}

/// An index over two documents defining `dupcrate::Widget` (a duplicated-descriptor group) and one
/// symbol whose range spells other text (a join discrepancy), under the given document names.
fn status_fixture_index(doc_a: &str, doc_b: &str) -> ExtractedIndex {
    let definition = |doc: &str, range: SourceRange, name: &str| ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new(name, SegmentKind::Type)],
        )),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: doc.to_string(),
            range,
            role: OccurrenceRole::Definition,
        }],
    };
    let mut index = one_doc_index(
        doc_a,
        vec![
            definition(doc_a, SourceRange::new(0, 7, 0, 13), "Widget"),
            definition(doc_b, SourceRange::new(0, 7, 0, 13), "Widget"),
            definition(doc_a, SourceRange::new(1, 4, 1, 5), "Other"),
        ],
    );
    index.documents.push(SourceDocument {
        path: doc_b.to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index
}

fn build_status_fixture(dir: &std::path::Path, workspace: &str, doc_a: &str, doc_b: &str) -> std::path::PathBuf {
    let db = dir.join("index.db");
    silent_cartographer::commands::build_from_index(
        &db,
        workspace,
        dir,
        &status_fixture_index(doc_a, doc_b),
        &[
            (doc_a.to_string(), "struct Widget;\nlet x = 1;\n".to_string()),
            (doc_b.to_string(), "struct Widget;\n".to_string()),
        ],
    )
    .unwrap();
    db
}

fn status_output(db: &std::path::Path, dir: &std::path::Path, json: bool) -> String {
    status_output_in(db, dir, json, silent_cartographer::cli::PositionUnit::Line)
}

fn status_output_in(
    db: &std::path::Path,
    dir: &std::path::Path,
    json: bool,
    position: silent_cartographer::cli::PositionUnit,
) -> String {
    silent_cartographer::commands::run_status(db, dir, "not-a-real-analyzer", json, true, false, true, position)
        .unwrap()
}

// _(Scenario: The status report names the refresh command)_ — a stale index's status carries the same
// refresh command a query answer does, in the machine report and the human one; running it makes the
// status fresh, and a fresh status carries no such field.
#[test]
fn a_stale_status_names_the_refresh_command_and_running_it_makes_status_fresh() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "# fixture manifest\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(support::DOC), support::SOURCE).unwrap();
    let db = support::build_fixture_db(dir.path(), "refresh-ws");
    std::fs::write(
        dir.path().join(support::DOC),
        format!("{}\n// edited\n", support::SOURCE),
    )
    .unwrap();

    let value: serde_json::Value = serde_json::from_str(&status_output(&db, dir.path(), true)).unwrap();
    assert_eq!(value["freshness"], "stale_content", "{value}");
    let refresh = value["refresh"]
        .as_str()
        .expect("a stale status carries a refresh command")
        .to_string();
    assert!(refresh.starts_with("c10r build "), "{refresh}");
    let human = status_output(&db, dir.path(), false);
    assert!(
        human.lines().any(|line| line == format!("refresh: {refresh}")),
        "the human report names the same command: {human}"
    );

    let bin_dir = tempfile::tempdir().unwrap();
    write_indexing_stub(bin_dir.path(), "rust-analyzer", "1.85.0");
    let built_bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_c10r"))
        .parent()
        .unwrap()
        .to_path_buf();
    run_refresh(&refresh, &prepend_path(&[&built_bin_dir, bin_dir.path()]));

    let value: serde_json::Value = serde_json::from_str(&status_output(&db, dir.path(), true)).unwrap();
    assert_eq!(value["freshness"], "fresh", "{value}");
    assert!(
        value.get("refresh").is_none(),
        "a fresh status carries no refresh field: {value}"
    );
    assert!(!status_output(&db, dir.path(), false).contains("refresh:"));
}

// _(Output discipline)_ — the human status report is a projection of the machine one: every string
// and number a top-level field carries, with the optional sections requested, appears in it.
#[test]
fn the_human_status_report_carries_every_value_of_the_machine_report() {
    fn leaves(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Number(n) => out.push(n.to_string()),
            serde_json::Value::Array(items) => items.iter().for_each(|v| leaves(v, out)),
            serde_json::Value::Object(map) => map.values().for_each(|v| leaves(v, out)),
            _ => {}
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let db = build_status_fixture(dir.path(), "status-ws", "a.rs", "b.rs");
    let report: serde_json::Value = serde_json::from_str(&status_output(&db, dir.path(), true)).unwrap();
    // The byte unit names both ends of a span, as the machine report does.
    let human = status_output_in(&db, dir.path(), false, silent_cartographer::cli::PositionUnit::Byte);

    let fields = report.as_object().unwrap();
    for expected in [
        "workspace",
        "provenance",
        "freshness",
        "refresh",
        "join_alignment",
        "duplicated_descriptors",
        "semantic_index",
        "discrepancies",
    ] {
        assert!(
            fields.contains_key(expected),
            "the fixture exercises {expected}: {report}"
        );
    }
    for (field, value) in fields {
        let mut found = Vec::new();
        leaves(value, &mut found);
        for leaf in found {
            assert!(
                human.contains(&leaf),
                "{field} value {leaf:?} missing from the human report:\n{human}"
            );
        }
    }
}

// _(Output discipline)_ — control characters in a workspace identity or a document path never reach
// the human status report.
#[test]
fn the_human_status_report_sanitizes_hostile_values() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_status_fixture(dir.path(), "evil\u{1b}[31m\nws", "a\u{1b}]0;x\u{7}.rs", "b.rs");
    let human = status_output(&db, dir.path(), false);
    assert!(
        !human.contains('\u{1b}') && !human.contains('\u{7}'),
        "no escape reaches the report: {human:?}"
    );
    assert!(
        human.lines().any(|line| line.starts_with("workspace: evil")),
        "{human:?}"
    );
    assert!(
        !human.lines().any(|line| line.starts_with("ws")),
        "the newline did not split the line: {human:?}"
    );
}

// _(Output discipline)_ — under the default line unit, a listed discrepancy row names its location as
// `path:line:column`, the position its machine row reports.
#[test]
fn the_human_status_discrepancy_listing_names_each_row_by_line_and_column() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_status_fixture(dir.path(), "status-lines-ws", "a.rs", "b.rs");
    let run = |json| {
        silent_cartographer::commands::run_status(
            &db,
            dir.path(),
            "not-a-real-analyzer",
            json,
            true,
            true,
            false,
            silent_cartographer::cli::PositionUnit::Line,
        )
        .unwrap()
    };
    let machine: serde_json::Value = serde_json::from_str(&run(true)).unwrap();
    let row = &machine["discrepancies"]["rows"][0];
    assert_eq!(
        (&row["start_line"], &row["start_column"]),
        (&2.into(), &5.into()),
        "{row}"
    );

    let human = run(false);
    assert!(
        human.lines().any(|line| line.ends_with(" at a.rs:2:5")),
        "the discrepancy row is located by line and column: {human}"
    );
}

// ---- Source positions by line and column ----

/// The identities a position-resolved `get` answer names, whether it resolved to one symbol or to a
/// candidate set; `None` for typed absence.
fn position_answer_ids(
    answer: &silent_cartographer::query::output::Answer<silent_cartographer::query::SymbolDetail>,
) -> Option<Vec<CanonicalId>> {
    match &answer.outcome {
        Outcome::Found { results } => Some(results.iter().map(|r| r.symbol.canonical_id.clone()).collect()),
        Outcome::Ambiguous { candidates, .. } => Some(candidates.iter().map(|c| c.canonical_id.clone()).collect()),
        Outcome::Absent => None,
        other => panic!("expected a symbol, a candidate set, or absence, got {other:?}"),
    }
}

/// The line and column (both from 1) of the byte at `offset` in `source`.
fn line_column_of(source: &str, offset: usize) -> SourcePosition {
    let (line, column) = line_col(source, offset);
    SourcePosition::LineColumn(LineColumn {
        line: line + 1,
        column: column + 1,
    })
}

fn function_id(name: &str) -> CanonicalId {
    id_of_pkg("poscrate", &[(name, SegmentKind::Method)])
}

fn get_at(store: &GraphStore, doc: &str, position: SourcePosition) -> Option<Vec<CanonicalId>> {
    let engine = engine_over(store, support::provenance());
    position_answer_ids(
        &engine
            .get_by_position(doc, position, Detail::Location, None, 1)
            .unwrap(),
    )
}

// _(Scenario: A line and column inside a method body)_ — the position resolves to the method.
#[test]
fn a_line_and_column_inside_a_method_body_returns_the_method() {
    for_each_backend(|language, fixture, store| {
        let (needle, path) = method_body(language);
        let inside = fixture.source.find(needle).unwrap() + 1;
        assert_eq!(
            get_at(store, fixture.doc, line_column_of(fixture.source, inside)),
            Some(vec![fixture.id(path)]),
            "{language:?}"
        );
    });
}

/// A body statement of a method in `language`'s header fixture, and the method's descriptor path.
fn method_body(language: Language) -> (&'static str, &'static [&'static str]) {
    match language {
        Language::Rust => ("a + 1", &["P", "get"]),
        Language::Python => ("return a", &["Q", "plain"]),
    }
}

// _(Scenario: A span ending at the line's first column does not cover it)_ — the declaration that
// ends at column 1 of line 2 is not selected for line 2; the one starting there is.
#[test]
fn a_span_ending_at_the_lines_first_column_is_not_selected() {
    assert_adjacent_spans_resolve_by_line("adj.rs");
}

// The same scenario over a Python document. A Python header ends after a colon that closes its line, so
// the rows are stored directly rather than built from Python source.
#[test]
fn a_python_span_ending_at_the_lines_first_column_is_not_selected() {
    assert_adjacent_spans_resolve_by_line("adj.py");
}

/// Store two declarations in `doc`, one ending at column 1 of line 2 where the other starts, and
/// assert that line 2 selects the one starting there and line 1 the one ending there.
fn assert_adjacent_spans_resolve_by_line(doc: &str) {
    let store = GraphStore::open_in_memory().unwrap();
    let row = |name: &str, span: (usize, usize), start: (u32, u32), end: (u32, u32)| SymbolRow {
        canonical_id: function_id(name),
        display_name: name.to_string(),
        kind: "function".to_string(),
        class: PersistedClass::InWorkspace,
        document_path: Some(doc.to_string()),
        span: Some(span),
        line_column: Some(LineColumnSpan {
            start: LineColumn {
                line: start.0,
                column: start.1,
            },
            end: LineColumn {
                line: end.0,
                column: end.1,
            },
        }),
        header_end: Some((
            span.1,
            LineColumn {
                line: end.0,
                column: end.1,
            },
        )),
        span_text: None,
        signature_text: None,
        interface_text: None,
        duplicated: false,
        test_rule: None,
    };
    store.insert_symbol(&row("ended", (0, 10), (1, 1), (2, 1))).unwrap();
    store.insert_symbol(&row("starts", (10, 20), (2, 1), (3, 5))).unwrap();
    assert_eq!(
        get_at(&store, doc, SourcePosition::Line(2)),
        Some(vec![function_id("starts")])
    );
    assert_eq!(
        get_at(&store, doc, SourcePosition::Line(1)),
        Some(vec![function_id("ended")])
    );
}

// _(Scenario: A column past the end of its line)_ — on a middle line of a multi-line function an
// oversized column returns the function; in Rust, on a line where one declaration ends and another
// starts it returns the declaration that encloses the end of the line. A Python declaration always
// begins on its own line, so no Python line ends one declaration and starts another.
#[test]
fn an_oversized_column_denotes_the_end_of_its_line() {
    for language in support::LANGUAGES {
        let (source, doc, expected): (&str, &str, &[(u32, &str)]) = match language {
            Language::Rust => (
                "pub fn first() {\n    let x = 1;\n}  pub fn second() {\n    let y = 2;\n}\n",
                "wide.rs",
                &[(2, "first"), (3, "second")],
            ),
            Language::Python => (
                "def first():\n    x = 1\n    return x\n\n\ndef second():\n    y = 2\n    return y\n",
                "wide.py",
                &[(2, "first"), (7, "second")],
            ),
        };
        let (first, second) = match language {
            Language::Rust => ("fn first", "fn second"),
            Language::Python => ("def first", "def second"),
        };
        let decls = [
            support::Decl {
                segments: &[("first", SegmentKind::Method)],
                kind: SymbolKind::Function,
                within: first,
            },
            support::Decl {
                segments: &[("second", SegmentKind::Method)],
                kind: SymbolKind::Function,
                within: second,
            },
        ];
        let store = support::declared_store(language, doc, source, &decls);
        for (line, name) in expected {
            let oversized = SourcePosition::LineColumn(LineColumn {
                line: *line,
                column: 9999,
            });
            assert_eq!(
                get_at(&store, doc, oversized),
                Some(vec![support::decl_id(&[(name, SegmentKind::Method)])]),
                "{language:?}, line {line}"
            );
        }
    }
}

// _(Scenario: A line past the end of the file; an empty document)_ — typed absence, including for an
// empty document whose zero-width module covers no line.
#[test]
fn a_line_past_the_end_and_an_empty_document_are_typed_absence() {
    for_each_backend(|language, fixture, store| {
        let last = fixture.source.lines().count() as u32;
        assert!(
            get_at(store, fixture.doc, SourcePosition::Line(last)).is_some(),
            "{language:?}: the last line is covered, so absence past it is not vacuous"
        );
        for past in [last + 1, 99] {
            assert_eq!(
                get_at(store, fixture.doc, SourcePosition::Line(past)),
                None,
                "{language:?}, line {past}"
            );
        }

        let doc = if language == Language::Rust {
            "empty.rs"
        } else {
            "empty.py"
        };
        let empty = support::declared_store(language, doc, "", &[]);
        assert!(
            empty.module_of_document(doc).unwrap().is_some(),
            "{language:?}: the empty document holds its module, so absence is not vacuous"
        );
        assert_eq!(get_at(&empty, doc, SourcePosition::Line(1)), None, "{language:?}");
    });
}

/// The sorted identities `get --at path:line` resolves `line` of the fixture to.
fn ids_at_line(fixture: &support::HeaderFixture, store: &GraphStore, line: u32) -> Option<Vec<CanonicalId>> {
    let mut ids = get_at(store, fixture.doc, SourcePosition::Line(line))?;
    ids.sort();
    Some(ids)
}

/// Assert each line in `lines` of `language`'s fixture resolves to exactly `expected`.
fn assert_lines_resolve(
    language: Language,
    fixture: &support::HeaderFixture,
    store: &GraphStore,
    lines: impl IntoIterator<Item = u32>,
    expected: &[CanonicalId],
) {
    let mut expected = expected.to_vec();
    expected.sort();
    for line in lines {
        assert_eq!(
            ids_at_line(fixture, store, line),
            Some(expected.clone()),
            "{language:?}, line {line}"
        );
    }
}

/// Run `check` once for every supported language over that language's header fixture and store.
fn for_each_backend(check: impl Fn(Language, &support::HeaderFixture, &GraphStore)) {
    for language in support::LANGUAGES {
        let fixture = support::header_fixture(language);
        let store = fixture.store(language);
        check(language, &fixture, &store);
    }
}

// _(Scenario: A header line that declares parameters)_ — Python records the parameters as symbols, and
// the method is returned, not a parameter; Rust records no parameter symbols, so its case is a method
// header line that declares parameters.
#[test]
fn a_method_header_line_that_declares_parameters_returns_the_method() {
    for_each_backend(|language, fixture, store| {
        let (needle, path): (&str, &[&str]) = match language {
            Language::Rust => ("pub fn get(&self", &["P", "get"]),
            Language::Python => ("def plain(self", &["Q", "plain"]),
        };
        assert_lines_resolve(language, fixture, store, [fixture.line(needle)], &[fixture.id(path)]);
    });
}

// _(Scenario: Every line of a multi-line header)_ — a decorated Python header and a multi-line
// parameter list in either language return the declaration on every header line. Rust has no
// decorators; its attributes lie outside the definition span (see the attribute test).
#[test]
fn every_line_of_a_multi_line_header_returns_the_declaration() {
    for_each_backend(|language, fixture, store| match language {
        Language::Rust => {
            let first = fixture.line("pub fn wide(");
            assert_lines_resolve(
                language,
                fixture,
                store,
                first..first + 4,
                &[fixture.id(&["P", "wide"])],
            );
        }
        Language::Python => {
            let decorated = fixture.line("@property");
            assert_lines_resolve(
                language,
                fixture,
                store,
                decorated..decorated + 3,
                &[fixture.id(&["Q", "area"])],
            );
            let first = fixture.line("async def wide(");
            assert_lines_resolve(
                language,
                fixture,
                store,
                first..first + 4,
                &[fixture.id(&["Q", "wide"])],
            );
        }
    });
}

// _(Scenario: An attribute outside the declaration)_ — Rust only: an attribute above a type is outside
// the type's definition span, so its line returns the module. Python decorators lie inside the span.
#[test]
fn a_rust_attribute_line_returns_the_enclosing_module() {
    let language = Language::Rust;
    let fixture = support::header_fixture(language);
    let store = fixture.store(language);
    assert_lines_resolve(
        language,
        &fixture,
        &store,
        [fixture.line("#[derive(Debug)]")],
        &[fixture.module_id()],
    );
}

// _(Scenario: A one-line declaration with members)_ — the type is returned, not its members.
#[test]
fn a_one_line_type_with_members_returns_the_type() {
    for_each_backend(|language, fixture, store| {
        let needle = match language {
            Language::Rust => "pub struct P {",
            Language::Python => "class P:",
        };
        assert_lines_resolve(language, fixture, store, [fixture.line(needle)], &[fixture.id(&["P"])]);
    });
}

// _(Scenario: A line declaring a member)_ — the member is returned, not the type around it.
#[test]
fn a_line_declaring_one_member_of_a_multi_line_type_returns_the_member() {
    for_each_backend(|language, fixture, store| {
        let needle = match language {
            Language::Rust => "pub size: u32,",
            Language::Python => "size: int = 0",
        };
        assert_lines_resolve(
            language,
            fixture,
            store,
            [fixture.line(needle)],
            &[fixture.id(&["Q", "size"])],
        );
    });
}

// _(Scenario: A line inside a body)_ — a body line of a method returns the method.
#[test]
fn a_method_body_line_returns_the_method() {
    for_each_backend(|language, fixture, store| {
        let (needle, path): (&str, &[&str]) = match language {
            Language::Rust => ("a + 1", &["P", "get"]),
            Language::Python => ("return a", &["Q", "plain"]),
        };
        assert_lines_resolve(language, fixture, store, [fixture.line(needle)], &[fixture.id(path)]);
    });
}

// _(Scenario: A line without a header)_ — a blank line between members of a container returns the
// innermost symbol that encloses it: the class in Python, the module in Rust (an `impl` block is not
// a symbol).
#[test]
fn a_blank_line_between_members_returns_the_enclosing_container() {
    for_each_backend(|language, fixture, store| {
        let (after, container, offset) = match language {
            Language::Rust => ("a + 1", fixture.module_id(), 2),
            Language::Python => ("size: int = 0", fixture.id(&["Q"]), 1),
        };
        let blank = fixture.line(after) + offset;
        assert_eq!(fixture.source.lines().nth(blank as usize - 1), Some(""), "{language:?}");
        assert_lines_resolve(language, fixture, store, [blank], &[container]);
    });
}

// _(Scenario: A line holding two sibling declarations)_ — Rust: two functions, and a constant beside a
// static; Python cannot hold two compound statements on a line, so its siblings are two assignments.
#[test]
fn two_sibling_declarations_on_one_line_return_the_candidate_set_in_each_language() {
    for_each_backend(|language, fixture, store| match language {
        Language::Rust => {
            assert_lines_resolve(
                language,
                fixture,
                store,
                [fixture.line("pub fn alpha")],
                &[fixture.id(&["alpha"]), fixture.id(&["beta"])],
            );
            assert_lines_resolve(
                language,
                fixture,
                store,
                [fixture.line("pub const K")],
                &[fixture.id(&["K"]), fixture.id(&["S"])],
            );
        }
        Language::Python => assert_lines_resolve(
            language,
            fixture,
            store,
            [fixture.line("ALPHA = 1")],
            &[fixture.id(&["ALPHA"]), fixture.id(&["BETA"])],
        ),
    });
}

// _(Scenario: A header that ends at the line's first column)_ — Rust: a function whose body opens at
// column 1 of line 2 is not selected for that line, so the declaration starting on it is the answer.
// A Python header always ends after a colon that closes its line, so it cannot end at a first column;
// the stored-row test above covers the rule independently of any language.
#[test]
fn a_rust_header_ending_at_the_lines_first_column_is_not_selected() {
    let source = "pub fn outer()\n{ pub fn inner() {} }\n";
    let nested = [
        support::Decl {
            segments: &[("outer", SegmentKind::Method)],
            kind: SymbolKind::Function,
            within: "outer()",
        },
        support::Decl {
            segments: &[("inner", SegmentKind::Method)],
            kind: SymbolKind::Function,
            within: "inner()",
        },
    ];
    let store = support::declared_store(Language::Rust, "nest.rs", source, &nested);
    let inner = support::decl_id(&[("inner", SegmentKind::Method)]);
    let outer = support::decl_id(&[("outer", SegmentKind::Method)]);
    assert_eq!(get_at(&store, "nest.rs", SourcePosition::Line(2)), Some(vec![inner]));
    assert_eq!(get_at(&store, "nest.rs", SourcePosition::Line(1)), Some(vec![outer]));
}

/// `get --at <at>` through the command handler, as a machine answer, under `unit`.
fn get_at_json(
    db: &std::path::Path,
    root: &std::path::Path,
    at: &str,
    unit: silent_cartographer::cli::PositionUnit,
) -> serde_json::Value {
    let out = silent_cartographer::commands::run_get(
        db,
        root,
        "not-a-real-analyzer",
        None,
        Some(at),
        Detail::Location,
        0,
        1,
        false,
        false,
        25,
        None,
        true,
        false,
        unit,
    )
    .unwrap();
    serde_json::from_str(&out).unwrap()
}

// _(Scenario: The byte unit reads a byte offset)_ — under `--position byte`, `path:byte_offset` inside
// a method body returns the method.
#[test]
fn the_byte_unit_reads_a_byte_offset() {
    for language in support::LANGUAGES {
        let fixture = support::header_fixture(language);
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.build_db(language, dir.path());
        let (needle, path) = method_body(language);
        let inside = fixture.source.find(needle).unwrap() + 1;
        let answer = get_at_json(
            &db,
            dir.path(),
            &format!("{}:{inside}", fixture.doc),
            silent_cartographer::cli::PositionUnit::Byte,
        );
        assert_eq!(
            answer["outcome"]["results"][0]["symbol"]["name"],
            *path.last().unwrap(),
            "{language:?}: {answer}"
        );
    }
}

// _(Scenario: A line and column inside a method body)_ — through the `--at` parser, a non-zero
// `path:line:column` inside a method body returns the method.
#[test]
fn an_at_argument_of_line_and_column_inside_a_method_body_returns_the_method() {
    for language in support::LANGUAGES {
        let fixture = support::header_fixture(language);
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.build_db(language, dir.path());
        let (needle, path) = method_body(language);
        let start = fixture.source.find(needle).unwrap();
        let (line, column) = line_col(fixture.source, start + 1);
        assert!(column > 0, "the position is not on a line's first column");
        let answer = get_at_json(
            &db,
            dir.path(),
            &format!("{}:{}:{}", fixture.doc, line + 1, column + 1),
            silent_cartographer::cli::PositionUnit::Line,
        );
        assert_eq!(
            answer["outcome"]["results"][0]["symbol"]["name"],
            *path.last().unwrap(),
            "{language:?}: {answer}"
        );
    }
}

// _(Scenario: A cursor is bound to the position unit)_ — a `get --at` answer never spans pages, so no
// token is issued for it; a token minted over the same query identity under the line unit is accepted
// under that unit and refused under the byte unit.
#[test]
fn a_get_at_cursor_is_refused_under_the_other_position_unit() {
    use silent_cartographer::cli::PositionUnit;
    use silent_cartographer::graph::store::Freshness;
    use silent_cartographer::query::output::{Answer, Provenance};
    use silent_cartographer::query::page::{PageIdentity, apply_pagination};

    for language in support::LANGUAGES {
        let fixture = support::header_fixture(language);
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.build_db(language, dir.path());
        let (needle, _) = method_body(language);
        let at = format!("{}:{}", fixture.doc, fixture.line(needle));

        let metadata = GraphStore::open(&db).unwrap().read_metadata().unwrap().unwrap();
        let identity = PageIdentity {
            command: "get",
            reference: at.clone(),
            relation: None,
            detail: Some("location"),
            depth: None,
            ordering: None,
            limit: Some(1),
            max_lines: None,
            from: Some(1),
            position_unit: Some("line"),
            index_hash: format!(
                "{}\u{1f}{}\u{1f}{}",
                metadata.content_hash, metadata.provenance.analyzer_name, metadata.provenance.analyzer_version
            ),
        };
        let provenance = Provenance {
            analyzer_name: metadata.provenance.analyzer_name.clone(),
            analyzer_version: metadata.provenance.analyzer_version.clone(),
        };
        let paged = apply_pagination(
            Answer::found(vec!["a", "b"], provenance, Freshness::Fresh),
            Some(1),
            None,
            &identity,
        )
        .unwrap();
        let cursor = paged.page.and_then(|page| page.cursor).expect("a truncated page");

        let run = |unit| {
            silent_cartographer::commands::run_get(
                &db,
                dir.path(),
                "not-a-real-analyzer",
                None,
                Some(&at),
                Detail::Location,
                0,
                1,
                false,
                false,
                1,
                Some(&cursor),
                true,
                false,
                unit,
            )
            .expect_err("the token resumes no page of a single-result answer")
            .to_string()
        };
        assert!(
            !run(PositionUnit::Line).contains("different query parameters"),
            "{language:?}: the token is accepted under the unit it was issued under"
        );
        assert!(
            run(PositionUnit::Byte).contains("different query parameters"),
            "{language:?}: the token is refused under the other unit"
        );
    }
}

// _(Scenario: A stale position reads the indexed text)_ — after lines are inserted above a method,
// the line the method had when the index was built still returns it, and the answer is stale.
#[test]
fn a_stale_position_reads_the_indexed_text() {
    for language in support::LANGUAGES {
        let fixture = support::header_fixture(language);
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.build_db(language, dir.path());
        let (header, path) = match language {
            Language::Rust => ("pub fn get", &["P", "get"]),
            Language::Python => ("def plain", &["Q", "plain"]),
        };
        let comment = if language == Language::Rust { "//" } else { "#" };
        std::fs::write(
            dir.path().join(fixture.doc),
            format!("{comment} one\n{comment} two\n{comment} three\n{}", fixture.source),
        )
        .unwrap();

        let line = fixture.line(header);
        let answer = get_at_json(
            &db,
            dir.path(),
            &format!("{}:{line}", fixture.doc),
            silent_cartographer::cli::PositionUnit::Line,
        );
        assert_eq!(answer["freshness"], "stale_content", "{language:?}: {answer}");
        let result = &answer["outcome"]["results"][0];
        assert_eq!(
            result["symbol"]["name"],
            *path.last().unwrap(),
            "{language:?}: {answer}"
        );
        assert_eq!(
            result["payload"]["location"]["start_line"], line,
            "{language:?}: {answer}"
        );
    }
}

// _(Scenario: A line position on a Python method)_ — `similar --at path:line` takes the method as the
// subject: the answer is the one `similar` gives for the method itself.
#[test]
fn similar_at_a_line_inside_a_python_method_takes_the_method() {
    let store = py_store();
    let engine = py_engine(&store);
    let source = py_source("pkg/shapes.py");
    let (line, _) = line_col(&source, source.find("return \"widget\"").unwrap());
    let by_line = engine
        .similar_by_position("pkg/shapes.py", SourcePosition::Line(line + 1), Detail::Location, None)
        .unwrap();
    let by_name = engine.similar(py_render_id().as_str(), Detail::Location, None).unwrap();
    assert!(
        matches!(by_name.outcome, Outcome::Found { .. }),
        "the method has neighbors"
    );
    assert_eq!(by_line.outcome, by_name.outcome);
}

// _(Scenario: A line position on a Rust method)_ — the Rust counterpart: `similar --at path:line` on a
// method's header line takes the method.
#[test]
fn similar_at_a_line_on_a_rust_method_header_takes_the_method() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let (line, _) = line_col(support::SOURCE, support::SOURCE.find("pub fn connect").unwrap());
    let by_line = engine
        .similar_by_position(support::DOC, SourcePosition::Line(line + 1), Detail::Location, None)
        .unwrap();
    let by_name = engine.similar(connect_id().as_str(), Detail::Location, None).unwrap();
    assert!(
        matches!(by_name.outcome, Outcome::Found { .. }),
        "the method has neighbors"
    );
    assert_eq!(by_line.outcome, by_name.outcome);
}
