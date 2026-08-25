//! Code-navigation tests: reference resolution across its forms, `get` at each detail, `trace` over the
//! supported relations, and the calibrated output contract.

mod support;

use silent_cartographer::graph::ingest;
use silent_cartographer::graph::store::{
    DEPENDENTS_HORIZON, DependencyKind, EdgeKind, GraphStore, OccurrenceRow, PersistedClass, SymbolRow,
};
use silent_cartographer::identity::{
    CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one,
};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::resolve::Resolution;
use silent_cartographer::query::{Detail, OrderMode, QueryEngine, Relation, TraceRelation};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};

use crate::support::{id_of_pkg, line_col, one_doc_index, one_occ_symbol, sources, ws};

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
        .get_by_position(support::DOC, inside, Detail::Location, None, 1)
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
    let human = silent_cartographer::render::to_human(&answer, false);
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
        .get_by_position("pkg/shapes.py", inside_render, Detail::Location, None, 1)
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
