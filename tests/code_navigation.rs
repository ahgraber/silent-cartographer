//! Code-navigation tests: reference resolution across tiers, `get` at each detail, `trace` over the
//! supported relations, and the calibrated output contract.

mod support;

use silent_cartographer::graph::ingest;
use silent_cartographer::graph::store::{DEPENDENTS_HORIZON, EdgeKind, GraphStore, PersistedClass, SymbolRow};
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
    let engine = QueryEngine::new(&store, support::provenance(), "different-hash".to_string(), None);

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

// ---- Dependents: the depth-bounded impact answer ----

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
            duplicated: false,
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
    let answer = engine.dependents("test-ws::seed", 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.detail.len(), 1);
    assert_eq!(report.detail[0].symbol.canonical_id, dep_id("caller"));
    assert_eq!(report.detail[0].kind, "uses", "detail carries the connecting kind");
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
    let answer = engine.dependents("test-ws::seed", 1).unwrap();
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
    let answer = engine.dependents("test-ws::seed", 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found");
    };
    let report: &DependentsReport = &results[0];
    assert_eq!(report.disclosure, HorizonDisclosure::BeyondBound);
    assert_eq!(report.detail.len(), 1, "only the direct dependent is detailed");
    assert_eq!(report.detail[0].symbol.canonical_id, dep_id("mid"));
    assert_eq!(report.beyond_bound.len(), 1, "the deeper dependent is aggregated");
    let agg = &report.beyond_bound[0];
    assert_eq!(agg.kind, "uses");
    assert_eq!(agg.distance, 2);
    assert_eq!(agg.count, 1);
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
    let answer = engine.dependents("test-ws::f0", 1).unwrap();
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
    let answer = engine.dependents("test-ws::f0", DEPENDENTS_HORIZON).unwrap();
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
    let answer = engine.dependents("test-ws::f0", DEPENDENTS_HORIZON + 5).unwrap();
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
    let answer = engine.dependents("test-ws::seed", DEPENDENTS_HORIZON).unwrap();
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
    let answer = engine.dependents("test-ws::seed", 0).unwrap();
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
        .dependents("test-ws::lonely", 1)
        .expect("no dependents is a successful typed answer, not a failure");
    assert!(
        matches!(answer.outcome, Outcome::Empty),
        "a symbol with no dependents is typed Empty: {:?}",
        answer.outcome
    );
    assert!(answer.to_json().contains("\"empty\""), "renders the empty outcome tag");
}

// _(`trace` refuses the dependents relation)_ — `trace` cannot express the depth-bounded, horizon
// aggregated dependents payload, so it errors rather than returning a confident but misleading empty
// answer for a subject that may have many dependents.
#[test]
fn trace_with_dependents_relation_errors_instead_of_empty() {
    let store = dep_graph(&["seed", "caller"], &[(EdgeKind::Uses, "caller", "seed")]);
    let engine = dep_engine(&store);
    let err = engine
        .trace("test-ws::seed", Relation::Dependents)
        .expect_err("trace must refuse the dependents relation, not answer empty");
    assert!(
        matches!(err, silent_cartographer::query::QueryError::DependentsNotTraceable),
        "expected DependentsNotTraceable, got {err:?}"
    );
}

// _(Depth-bounded impact answer — teaching error)_ — supplying `--depth` with a relation other than
// `dependents` fails with a teaching error naming the flag, the relation, and the accepting relation.
#[test]
fn depth_with_non_dependents_relation_is_a_teaching_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    silent_cartographer::commands::build_from_index(&db, "op-ws", &support::fixture_index(), &sources()).unwrap();

    let err = silent_cartographer::commands::run_trace(
        &db,
        dir.path(),
        "not-a-real-analyzer",
        "net::Client",
        Relation::Contains,
        Some(2),
        false,
    )
    .expect_err("--depth with contains must fail");
    let msg = format!("{err:#}");
    assert!(msg.contains("--depth"), "names the flag: {msg}");
    assert!(msg.contains("contains"), "names the offending relation: {msg}");
    assert!(msg.contains("dependents"), "names the relation that accepts it: {msg}");
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

// ---------------------------------------------------------------------------
// Python (fixture-level): navigation over the committed python-conformance
// fixture — no live tool.
// ---------------------------------------------------------------------------

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
    let answer = engine.get(py_render_id().as_str(), Detail::Body).unwrap();
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
        .get_by_position("pkg/shapes.py", inside_render, Detail::Location)
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
    let answer = engine.dependents(py_widget_id().as_str(), 1).unwrap();
    let Outcome::Found { results } = &answer.outcome else {
        panic!("expected found, got {:?}", answer.outcome);
    };
    let report: &DependentsReport = &results[0];

    let uses = report
        .detail
        .iter()
        .find(|d| d.symbol.canonical_id == py_build_id())
        .unwrap_or_else(|| panic!("build is a dependent: {:?}", report.detail));
    assert_eq!(uses.kind, "uses", "the function that calls Widget is a uses dependent");
    assert_eq!(uses.distance, 1);

    let imports = report
        .detail
        .iter()
        .find(|d| d.symbol.canonical_id == py_consumer_module_id())
        .unwrap_or_else(|| panic!("the importing module is a dependent: {:?}", report.detail));
    assert_eq!(imports.kind, "imports", "the importing module is an imports dependent");
    assert_eq!(imports.distance, 1);
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
