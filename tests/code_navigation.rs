//! Code-navigation tests: reference resolution across tiers, `get` at each detail, `trace` over the
//! supported relations, and the calibrated output contract.

mod support;

use silent_cartographer::graph::ingest;
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::identity::{
    CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one,
};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::resolve::Resolution;
use silent_cartographer::query::{Detail, QueryEngine, Relation};
use silent_cartographer::semantic::model::AnalyzerProvenance;

fn ws() -> WorkspaceId {
    WorkspaceId::new("test-ws")
}

fn sources() -> Vec<(String, String)> {
    vec![(support::DOC.to_string(), support::SOURCE.to_string())]
}

fn built_store() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), &support::fixture_index(), &sources()).unwrap();
    store
}

fn engine_over<'a>(store: &'a GraphStore, provenance: AnalyzerProvenance) -> QueryEngine<'a> {
    let hash = silent_cartographer::graph::content_hash(&sources());
    QueryEngine::new(store, provenance, hash)
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
    ingest(&mut store, &ws(), &index, &sources()).unwrap();
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
    let answer = engine.get("net::Client::connect", Detail::Location).unwrap();
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
    let answer = engine.get("net::Client::connect", Detail::Body).unwrap();
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
    let answer = engine.get("net::Client::connect", Detail::Signature).unwrap();
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

// _(Symbol retrieval at a chosen detail — position write-site)_ — `get` for a position returns the
// enclosing symbol.
#[test]
fn get_by_position_returns_enclosing_symbol() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // A byte offset inside connect's body. Find "connect(&self) {}" and point just after the brace.
    let idx = support::SOURCE.find("pub fn connect").unwrap();
    let inside = idx + "pub fn connect(&self) {".len(); // inside the method body span
    let answer = engine.get_by_position(support::DOC, inside, Detail::Location).unwrap();
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
    let answer = engine.trace("net::Client", Relation::Contains).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let names: Vec<&str> = results
        .iter()
        .filter_map(|item| match item {
            silent_cartographer::query::TraceItem::Symbol(v) => Some(v.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"connect"), "Client contains connect: {names:?}");
    assert!(names.contains(&"disconnect"), "Client contains disconnect: {names:?}");
}

// _(Relationship trace — containers write-site)_ — `trace` over `containers` for a method returns its
// enclosing type.
#[test]
fn trace_containers_returns_enclosing_type() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.trace("net::Client::connect", Relation::Containers).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    match &results[0] {
        silent_cartographer::query::TraceItem::Symbol(v) => assert_eq!(v.canonical_id, client_id()),
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
    let answer = engine.trace("net::Client", Relation::References).unwrap();
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
    let answer = engine.trace("net::Client", Relation::References).unwrap();
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

// _(Relationship trace — empty branch)_ — a subject with no instances of a relation returns typed
// absence.
#[test]
fn trace_empty_relation_is_typed_absence() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    // disconnect has no references in the fixture.
    let answer = engine.trace("net::Client::disconnect", Relation::References).unwrap();
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "empty relation is a typed Empty, not a failure: {:?}",
        answer.outcome
    );
}

// _(Calibrated output contract — get write-site)_ — a fresh result carries provenance and is marked
// fresh.
#[test]
fn fresh_result_carries_provenance_and_is_fresh() {
    use silent_cartographer::query::output::FreshnessLabel;
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let answer = engine.get("net::Client::connect", Detail::Location).unwrap();
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
    let answer = engine.get("net::Client::connect", Detail::Location).unwrap();
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
    let engine = QueryEngine::new(&store, support::provenance(), "different-hash".to_string());

    let get_answer = engine.get("net::Client::connect", Detail::Location).unwrap();
    assert!(get_answer.stale, "get result over changed sources is stale");

    let trace_answer = engine.trace("net::Client", Relation::Contains).unwrap();
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
    ingest(&mut store, &ws(), &index, &sources()).unwrap();
    let engine = engine_over(&store, support::provenance());

    let answer = engine.get("connect", Detail::Location).unwrap();
    let Outcome::Ambiguous { candidates } = &answer.outcome else {
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
        .get("no_such_symbol_anywhere", Detail::Location)
        .expect("absence is a successful typed answer, not a failure");
    assert!(
        matches!(answer.outcome, Outcome::Absent),
        "expected absent, got {:?}",
        answer.outcome
    );

    // Absent is distinct from an empty relation: a subject with no instances of a relation yields
    // Empty, and the two render with different outcome tags.
    let empty = engine.trace("net::Client::disconnect", Relation::References).unwrap();
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

// _(Calibrated output contract)_ — repeated identical queries return locations in the same order.
#[test]
fn deterministic_ordering_across_repeated_queries() {
    let store = built_store();
    let engine = engine_over(&store, support::provenance());
    let first = engine.trace("net::Client", Relation::References).unwrap();
    let second = engine.trace("net::Client", Relation::References).unwrap();
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
