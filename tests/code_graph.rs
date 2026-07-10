//! Code-graph tests: the guarded positional join, lossless persistence, enclosure, staleness,
//! reference attribution, and join-alignment accounting.

mod support;

use silent_cartographer::graph::store::{
    DEPENDENTS_HORIZON, EdgeKind, Freshness, GraphStore, PersistedClass, SymbolRow,
};
use silent_cartographer::graph::{content_hash, freshness, ingest, join_guarded};
use silent_cartographer::identity::{CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, DuplicateGroup, EnvironmentFacts, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol,
    OccurrenceRole, PositionEncoding, SourceDocument, SourceRange, SymbolClass, SymbolKind, normalize,
};

fn ws() -> WorkspaceId {
    WorkspaceId::new("test-ws")
}

fn sources() -> Vec<(String, String)> {
    vec![(support::DOC.to_string(), support::SOURCE.to_string())]
}

/// Ingest the standard fixture into a fresh in-memory store, returning the store and identities.
fn ingest_fixture() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::fixture_index();
    let src = sources();
    ingest(&mut store, &ws(), &index, &src).unwrap();
    store
}

/// The canonical identity of a fixture symbol by its descriptor segments.
fn id_of(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    let descriptor = Descriptor::new("mycrate", segs);
    silent_cartographer::identity::project_one(&ws(), &descriptor)
}

fn client_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module), ("Client", SegmentKind::Type)])
}

fn connect_id() -> CanonicalId {
    id_of(&[
        ("net", SegmentKind::Module),
        ("Client", SegmentKind::Type),
        ("connect", SegmentKind::Method),
    ])
}

fn open_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module), ("open", SegmentKind::Method)])
}

fn module_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module)])
}

// _(Deduplicated dependency edges)_ — an edge is a relation instance, unique on (kind, src, dst):
// repeated references from the same declaration to the same symbol collapse to one edge, and
// rebuilding the identical fixture leaves the edge set unchanged.
#[test]
fn edges_are_deduplicated_and_rebuild_is_idempotent() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::fixture_index();
    let src = sources();

    ingest(&mut store, &ws(), &index, &src).unwrap();
    let uses_first = store.edges(EdgeKind::Uses).unwrap();

    // `open` references `Client` twice (return type and `let c = Client`), yet exactly one `uses`
    // edge from `open` to `Client` persists — the per-site detail lives in the occurrences table.
    let open_to_client = uses_first
        .iter()
        .filter(|(s, d)| *s == open_id() && *d == client_id())
        .count();
    assert_eq!(
        open_to_client, 1,
        "repeated references collapse to one uses edge: {uses_first:?}"
    );

    // Every persisted uses edge is unique on (src, dst).
    let distinct: std::collections::HashSet<_> = uses_first.iter().collect();
    assert_eq!(
        distinct.len(),
        uses_first.len(),
        "no duplicate uses rows: {uses_first:?}"
    );

    // Rebuilding the identical fixture into the same store yields the identical edge set.
    ingest(&mut store, &ws(), &index, &src).unwrap();
    let uses_second = store.edges(EdgeKind::Uses).unwrap();
    assert_eq!(uses_first, uses_second, "rebuild is idempotent over the edge set");
}

// _(Whole-build supersession)_ — a build over an existing same-version store wholly supersedes every
// derived table: rebuilding the identical index leaves byte-identical symbol, occurrence, and edge
// row sets (no accumulation), and a symbol absent from the next index leaves no stale row behind.
#[test]
fn rebuilding_over_the_same_store_supersedes_all_derived_rows() {
    let snapshot = |store: &GraphStore| {
        let names = ["net", "Client", "connect", "disconnect", "open"];
        let mut symbols = Vec::new();
        let mut occurrences = Vec::new();
        for name in names {
            for row in store.symbols_by_shortname(name).unwrap() {
                occurrences.extend(store.occurrences_of(&row.canonical_id).unwrap());
                symbols.push(row);
            }
        }
        let mut edges = Vec::new();
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Uses,
            EdgeKind::Imports,
            EdgeKind::TypeHierarchy,
        ] {
            for (src, dst) in store.edges(kind).unwrap() {
                edges.push((kind.tag().to_string(), src, dst));
            }
        }
        (symbols, occurrences, edges)
    };

    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::fixture_index();
    let src = sources();
    ingest(&mut store, &ws(), &index, &src).unwrap();
    let first = snapshot(&store);
    assert!(
        !first.0.is_empty() && !first.1.is_empty() && !first.2.is_empty(),
        "sanity: the first build produced symbol, occurrence, and edge rows"
    );

    // Second build of the identical index over the same store: every derived row set is superseded,
    // not accumulated — equal counts AND equal content.
    ingest(&mut store, &ws(), &index, &src).unwrap();
    let second = snapshot(&store);
    assert_eq!(first.0, second.0, "symbol rows are identical after a rebuild");
    assert_eq!(first.1, second.1, "occurrence rows are identical after a rebuild");
    assert_eq!(first.2, second.2, "edge rows are identical after a rebuild");

    // Staleness clearing, not just dedup: a symbol present only in the earlier index is absent after
    // a build of an index without it.
    let mut smaller = support::fixture_index();
    smaller.symbols.retain(|s| s.terminal_name() != Some("disconnect"));
    ingest(&mut store, &ws(), &smaller, &src).unwrap();
    assert!(
        store.symbols_by_shortname("disconnect").unwrap().is_empty(),
        "the vanished symbol's row is superseded away, not left stale"
    );
    assert!(
        !store.symbols_by_shortname("connect").unwrap().is_empty(),
        "symbols still in the index persist"
    );
}

// _(Declaration-level dependency edges (uses) — call branch)_ — a function calling another yields a
// `uses` edge from caller to callee.
#[test]
fn call_yields_a_uses_edge() {
    let store = ingest_fixture();
    // `open` calls `connect` inside its closure body.
    let uses = store.edges(EdgeKind::Uses).unwrap();
    assert!(uses.contains(&(open_id(), connect_id())), "open uses connect: {uses:?}");
}

// _(Declaration-level dependency edges (uses) — reference-grade branch)_ — a function that only
// mentions a type in its body (never calling it) still yields a `uses` edge to that type.
#[test]
fn type_mention_yields_a_uses_edge() {
    let store = ingest_fixture();
    // `open` mentions `Client` as its return type and constructs it — no method call — yet uses it.
    let uses = store.edges(EdgeKind::Uses).unwrap();
    assert!(
        uses.contains(&(open_id(), client_id())),
        "open uses Client by type mention: {uses:?}"
    );
}

// _(Declaration-level dependency edges (uses) — refusal guard)_ — an occurrence the join refused
// contributes no dependency edge: edges derive exclusively from aligned occurrences.
#[test]
fn refused_occurrence_contributes_no_dependency_edge() {
    // `caller` is a function; `beta` (external) has a reference occurrence inside caller's body that
    // points at a token spelling `x`, not `beta` → refused. No uses edge may derive from it.
    let source = "fn caller() { let x = 1; }\n";
    let x_pos = source.find("x =").unwrap();
    let (xl, xc) = line_col(source, x_pos);
    let caller_pos = source.find("caller").unwrap();
    let (cl, cc) = line_col(source, caller_pos);
    let caller = one_occ_symbol(
        "c",
        &[("caller", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(cl, cc, cl, cc + 6),
        OccurrenceRole::Definition,
    );
    let beta = one_occ_symbol(
        "thirdparty",
        &[("beta", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(xl, xc, xl, xc + 1),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![caller, beta]), &src).unwrap();
    assert_eq!(acc.text_mismatch, 1, "the drifted reference is refused");
    assert!(
        store.edges(EdgeKind::Uses).unwrap().is_empty(),
        "a refused occurrence derives no uses edge"
    );
}

// _(Module-level dependency edges (imports))_ — a use statement at module scope yields an `imports`
// edge from the module to the named symbol.
#[test]
fn use_statement_yields_an_imports_edge() {
    // A module `thing` with `use thing::Thing;` at module scope.
    let source = "\
mod thing {
    pub struct Thing;
}
use thing::Thing;
";
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![DescriptorSegment::new("thing", SegmentKind::Module)],
                )),
                kind: SymbolKind::Module,
                class: SymbolClass::InWorkspace,
                occurrences: vec![ExtractedOccurrence {
                    document_path: "m.rs".to_string(),
                    range: SourceRange::new(0, 4, 0, 9),
                    role: OccurrenceRole::Definition,
                }],
            },
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![
                        DescriptorSegment::new("thing", SegmentKind::Module),
                        DescriptorSegment::new("Thing", SegmentKind::Type),
                    ],
                )),
                kind: SymbolKind::Type,
                class: SymbolClass::InWorkspace,
                occurrences: vec![
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(1, 15, 1, 20),
                        role: OccurrenceRole::Definition,
                    },
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(3, 11, 3, 16),
                        role: OccurrenceRole::Reference,
                    },
                ],
            },
        ],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let module = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("thing", SegmentKind::Module)]),
    );
    let thing = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "c",
            vec![
                DescriptorSegment::new("thing", SegmentKind::Module),
                DescriptorSegment::new("Thing", SegmentKind::Type),
            ],
        ),
    );
    let imports = store.edges(EdgeKind::Imports).unwrap();
    assert!(
        imports.contains(&(module, thing)),
        "the module imports Thing via the use statement: {imports:?}"
    );
}

// _(Module-level dependency edges (imports) — crate-root/multi-document branch)_ — rust-analyzer
// emits one shared module symbol for every crate root (bin, lib, each `tests/*.rs` file), each with
// its own definition occurrence spanning its own document. A module-scope reference in EITHER
// document must resolve to a real importer and yield an `imports` edge, not just the first document
// `module_by_doc` happens to keep.
#[test]
fn shared_module_symbol_maps_to_every_document_it_defines() {
    // Two crate-root documents, `a.rs` and `b.rs`, both carrying a definition occurrence of the same
    // module symbol (module-span rule: the occurrence spans the whole document). Each document also
    // has a module-scope reference to its own external symbol.
    let source_a = "use ext::Alpha;";
    let source_b = "use ext::Beta;";
    let alpha_tok = source_a.find("Alpha").unwrap();
    let (al, ac) = line_col(source_a, alpha_tok);
    let beta_tok = source_b.find("Beta").unwrap();
    let (bl, bc) = line_col(source_b, beta_tok);

    let module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "c",
            vec![DescriptorSegment::new("root", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "a.rs".to_string(),
                range: SourceRange::new(0, 0, 0, source_a.len() as u32),
                role: OccurrenceRole::Definition,
            },
            ExtractedOccurrence {
                document_path: "b.rs".to_string(),
                range: SourceRange::new(0, 0, 0, source_b.len() as u32),
                role: OccurrenceRole::Definition,
            },
        ],
    };
    let alpha = one_occ_symbol(
        "ext",
        &[("Alpha", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::External,
        "a.rs",
        SourceRange::new(al, ac, al, ac + 5),
        OccurrenceRole::Reference,
    );
    let beta = one_occ_symbol(
        "ext",
        &[("Beta", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::External,
        "b.rs",
        SourceRange::new(bl, bc, bl, bc + 4),
        OccurrenceRole::Reference,
    );

    let index = ExtractedIndex {
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
        symbols: vec![module, alpha, beta],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![
        ("a.rs".to_string(), source_a.to_string()),
        ("b.rs".to_string(), source_b.to_string()),
    ];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let root = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("root", SegmentKind::Module)]),
    );
    let alpha_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("ext", vec![DescriptorSegment::new("Alpha", SegmentKind::Type)]),
    );
    let beta_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("ext", vec![DescriptorSegment::new("Beta", SegmentKind::Type)]),
    );

    let imports = store.edges(EdgeKind::Imports).unwrap();
    assert!(
        imports.contains(&(root.clone(), alpha_id)),
        "the module-scope reference in a.rs yields an imports edge: {imports:?}"
    );
    assert!(
        imports.contains(&(root, beta_id)),
        "the module-scope reference in b.rs yields an imports edge too, not silently dropped: {imports:?}"
    );
}

// _(Module-level dependency edges (imports) — boundary)_ — a reference inside a function yields a
// `uses` edge from the function and no `imports` edge from the containing module.
#[test]
fn reference_inside_a_function_is_not_an_import() {
    // The fixture's references all sit inside declarations (impl header, function body), never at
    // module scope, so no imports edge is derived.
    let store = ingest_fixture();
    assert!(
        store.edges(EdgeKind::Imports).unwrap().is_empty(),
        "function-body references produce uses edges, not imports"
    );
    // The connect reference produced a uses edge from open, confirming it was derived as a use.
    assert!(
        store
            .edges(EdgeKind::Uses)
            .unwrap()
            .contains(&(open_id(), connect_id()))
    );
}

/// Build a symbol whose occurrences are given as `(byte_pos, token_len, role)` against `source`,
/// used by the trait-implementation edge fixtures where a symbol both defines and is referenced in
/// an impl header. Byte positions map directly to zero-based `(line, col)` in single-byte sources.
fn sym_multi(
    package: &str,
    segments: &[(&str, SegmentKind)],
    kind: SymbolKind,
    class: SymbolClass,
    doc: &str,
    occs: &[(usize, usize, OccurrenceRole)],
    source: &str,
) -> ExtractedSymbol {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    let occurrences = occs
        .iter()
        .map(|(pos, len, role)| {
            let (l, c) = line_col(source, *pos);
            ExtractedOccurrence {
                document_path: doc.to_string(),
                range: SourceRange::new(l, c, l, c + *len as u32),
                role: *role,
            }
        })
        .collect();
    ExtractedSymbol {
        descriptor: Some(Descriptor::new(package, segs)),
        kind,
        class,
        occurrences,
    }
}

// _(Trait-implementation edges (type_hierarchy) — plain branch)_ — a workspace type implementing a
// workspace trait yields an edge from the type to the trait.
#[test]
fn plain_trait_impl_yields_type_hierarchy_edge() {
    let source = "\
trait Greet {}
struct Person;
impl Greet for Person {}
";
    let greet = sym_multi(
        "c",
        &[("Greet", SegmentKind::Type)],
        SymbolKind::Trait,
        SymbolClass::InWorkspace,
        "m.rs",
        &[
            (source.find("Greet").unwrap(), 5, OccurrenceRole::Definition),
            (source.rfind("Greet").unwrap(), 5, OccurrenceRole::Reference),
        ],
        source,
    );
    let person = sym_multi(
        "c",
        &[("Person", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        &[
            (source.find("Person").unwrap(), 6, OccurrenceRole::Definition),
            (source.rfind("Person").unwrap(), 6, OccurrenceRole::Reference),
        ],
        source,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![greet, person]), &src).unwrap();

    let greet_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("Greet", SegmentKind::Type)]),
    );
    let person_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("Person", SegmentKind::Type)]),
    );
    let edges = store.edges(EdgeKind::TypeHierarchy).unwrap();
    assert_eq!(
        edges,
        vec![(person_id, greet_id)],
        "the implementing type points at the trait: {edges:?}"
    );
}

// _(Trait-implementation edges (type_hierarchy) — generic branch)_ — a trait name carrying generic
// parameters resolves through the name-token occurrence, closing the previously-dropped case.
#[test]
fn generic_trait_impl_yields_type_hierarchy_edge() {
    let source = "\
struct Detail;
struct Wrapper;
impl From<Detail> for Wrapper {}
";
    // `From` is external; its only occurrence is the name token inside `From<Detail>`.
    let from = sym_multi(
        "core",
        &[("convert", SegmentKind::Module), ("From", SegmentKind::Type)],
        SymbolKind::Trait,
        SymbolClass::External,
        "m.rs",
        &[(source.find("From").unwrap(), 4, OccurrenceRole::Reference)],
        source,
    );
    let wrapper = sym_multi(
        "c",
        &[("Wrapper", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        &[
            (source.find("Wrapper").unwrap(), 7, OccurrenceRole::Definition),
            (source.rfind("Wrapper").unwrap(), 7, OccurrenceRole::Reference),
        ],
        source,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![from, wrapper]), &src).unwrap();

    let from_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "core",
            vec![
                DescriptorSegment::new("convert", SegmentKind::Module),
                DescriptorSegment::new("From", SegmentKind::Type),
            ],
        ),
    );
    let wrapper_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("Wrapper", SegmentKind::Type)]),
    );
    let edges = store.edges(EdgeKind::TypeHierarchy).unwrap();
    assert!(
        edges.contains(&(wrapper_id, from_id)),
        "the generic trait name resolves through its name token: {edges:?}"
    );
}

// _(Trait-implementation edges (type_hierarchy) — external-trait branch)_ — an implementation of a
// trait defined outside the workspace yields an edge to that external symbol's persisted identity.
#[test]
fn external_trait_impl_yields_type_hierarchy_edge() {
    let source = "\
struct Widget;
impl Default for Widget {}
";
    let default = sym_multi(
        "core",
        &[("default", SegmentKind::Module), ("Default", SegmentKind::Type)],
        SymbolKind::Trait,
        SymbolClass::External,
        "m.rs",
        &[(source.find("Default").unwrap(), 7, OccurrenceRole::Reference)],
        source,
    );
    let widget = sym_multi(
        "c",
        &[("Widget", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        &[
            (source.find("Widget").unwrap(), 6, OccurrenceRole::Definition),
            (source.rfind("Widget").unwrap(), 6, OccurrenceRole::Reference),
        ],
        source,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![default, widget]), &src).unwrap();

    let default_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "core",
            vec![
                DescriptorSegment::new("default", SegmentKind::Module),
                DescriptorSegment::new("Default", SegmentKind::Type),
            ],
        ),
    );
    let widget_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("Widget", SegmentKind::Type)]),
    );
    let default_row = store.symbol(&default_id).unwrap().expect("external trait persisted");
    assert_eq!(default_row.class, PersistedClass::External);
    let edges = store.edges(EdgeKind::TypeHierarchy).unwrap();
    assert!(
        edges.contains(&(widget_id, default_id)),
        "the edge points at the external trait's persisted identity: {edges:?}"
    );
}

// _(Trait-implementation edges (type_hierarchy) — skip branch)_ — an impl whose trait name token has
// no aligned occurrence yields no edge and fabricates no identity: the skip is a refusal to guess.
#[test]
fn missing_trait_occurrence_skips_the_edge_without_fabrication() {
    let source = "\
struct Thing;
impl Gone for Thing {}
";
    // Only `Thing` is provided; the `Gone` trait name has no aligned occurrence.
    let thing = sym_multi(
        "c",
        &[("Thing", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        &[
            (source.find("Thing").unwrap(), 5, OccurrenceRole::Definition),
            (source.rfind("Thing").unwrap(), 5, OccurrenceRole::Reference),
        ],
        source,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![thing]), &src).unwrap();

    assert!(
        store.edges(EdgeKind::TypeHierarchy).unwrap().is_empty(),
        "no edge is derived when the trait name-token has no aligned occurrence"
    );
    assert!(
        store.symbols_by_shortname("Gone").unwrap().is_empty(),
        "no identity is fabricated for the unresolved trait name"
    );
}

// ---- Dependents traversal ----

/// A synthetic canonical identity for a traversal-graph symbol.
fn sid(name: &str) -> CanonicalId {
    CanonicalId::from_raw(format!("test-ws::{name}"))
}

/// Persist a bare in-workspace symbol so edges referencing it satisfy the foreign-key constraint.
fn put_symbol(store: &GraphStore, name: &str) {
    store
        .insert_symbol(&SymbolRow {
            canonical_id: sid(name),
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

/// Build a store over `symbols`, then insert `edges` as `(kind, src_name, dst_name)`. An edge
/// `(Uses, "a", "b")` means "a uses b", so `b`'s dependents include `a`.
fn graph(symbols: &[&str], edges: &[(EdgeKind, &str, &str)]) -> GraphStore {
    let store = GraphStore::open_in_memory().unwrap();
    for s in symbols {
        put_symbol(&store, s);
    }
    for (kind, src, dst) in edges {
        store.insert_edge(*kind, &sid(src), &sid(dst)).unwrap();
    }
    store
}

// _(Dependents traversal — direct branch)_ — a function using the seed appears at distance 1 with
// kind `uses`.
#[test]
fn dependent_direct_is_distance_one_uses() {
    let store = graph(&["seed", "caller"], &[(EdgeKind::Uses, "caller", "seed")]);
    let deps = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].id, sid("caller"));
    assert_eq!(deps[0].depth, 1);
    assert_eq!(deps[0].kind, "uses");
}

// _(Dependents traversal — transitive branch)_ — a function two hops away appears at distance 2.
#[test]
fn dependent_transitive_is_distance_two() {
    let store = graph(
        &["seed", "mid", "outer"],
        &[(EdgeKind::Uses, "mid", "seed"), (EdgeKind::Uses, "outer", "mid")],
    );
    let deps = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    let outer = deps.iter().find(|d| d.id == sid("outer")).expect("outer reached");
    assert_eq!(outer.depth, 2, "outer is two hops from the seed: {deps:?}");
}

// _(Dependents traversal — imports branch)_ — a module importing the seed appears with kind
// `imports`.
#[test]
fn dependent_via_imports_carries_imports_kind() {
    let store = graph(&["seed", "mod_a"], &[(EdgeKind::Imports, "mod_a", "seed")]);
    let deps = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].kind, "imports");
}

// _(Dependents traversal — type_hierarchy branch)_ — a type implementing the seed trait appears with
// kind `type_hierarchy`.
#[test]
fn dependent_via_type_hierarchy_carries_that_kind() {
    let store = graph(
        &["seed_trait", "impl_type"],
        &[(EdgeKind::TypeHierarchy, "impl_type", "seed_trait")],
    );
    let deps = store.dependents(&sid("seed_trait"), DEPENDENTS_HORIZON).unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].kind, "type_hierarchy");
}

// _(Dependents traversal — enclosure excluded)_ — the seed's containing module is not a dependent by
// containment alone: `contains` supplies attribution, never impact.
#[test]
fn enclosure_never_propagates_dependence() {
    let store = graph(&["seed", "module"], &[(EdgeKind::Contains, "module", "seed")]);
    let deps = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    assert!(
        deps.is_empty(),
        "a contains edge does not make the container a dependent: {deps:?}"
    );
}

// _(Dependents traversal — shortest distance)_ — a symbol reaching the seed both directly and through
// an intermediate is reported once, at distance 1.
#[test]
fn multiple_paths_report_the_shortest_distance() {
    // `outer` uses the seed directly AND uses `mid` which uses the seed → two paths, shortest is 1.
    let store = graph(
        &["seed", "mid", "outer"],
        &[
            (EdgeKind::Uses, "outer", "seed"),
            (EdgeKind::Uses, "mid", "seed"),
            (EdgeKind::Uses, "outer", "mid"),
        ],
    );
    let deps = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    let outer: Vec<_> = deps.iter().filter(|d| d.id == sid("outer")).collect();
    assert_eq!(outer.len(), 1, "outer reported exactly once: {deps:?}");
    assert_eq!(outer[0].depth, 1, "at its shortest distance");
}

// _(Dependents traversal — cycle)_ — two mutually-using functions terminate, each reported at most
// once, with the seed excluded from its own results.
#[test]
fn cyclic_dependencies_terminate() {
    // a uses b and b uses a; seed = a. b depends on a (b uses a → edge b→a). From b, the only edge
    // to b is a→b whose src is the seed, excluded — so the walk terminates.
    let store = graph(&["a", "b"], &[(EdgeKind::Uses, "a", "b"), (EdgeKind::Uses, "b", "a")]);
    let deps = store.dependents(&sid("a"), DEPENDENTS_HORIZON).unwrap();
    assert_eq!(deps.len(), 1, "each symbol reported at most once: {deps:?}");
    assert_eq!(deps[0].id, sid("b"));
    assert!(
        !deps.iter().any(|d| d.id == sid("a")),
        "the seed is never its own dependent"
    );
}

// _(Dependents traversal — determinism)_ — repeated identical queries return byte-identical ordering.
#[test]
fn dependents_ordering_is_deterministic() {
    let store = graph(
        &["seed", "x", "y", "z"],
        &[
            (EdgeKind::Uses, "x", "seed"),
            (EdgeKind::Imports, "y", "seed"),
            (EdgeKind::TypeHierarchy, "z", "seed"),
        ],
    );
    let first = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    let second = store.dependents(&sid("seed"), DEPENDENTS_HORIZON).unwrap();
    assert_eq!(first, second, "identical queries return identical ordering");
    // All at depth 1, so ordering falls to the fixed kind order: uses, imports, type_hierarchy.
    let kinds: Vec<&str> = first.iter().map(|d| d.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["uses", "imports", "type_hierarchy"],
        "fixed kind order: {first:?}"
    );
}

// _(Guarded positional join — canonical write-site)_ — an occurrence whose location spells the
// symbol name is persisted as an aligned attribution.
#[test]
fn aligned_occurrence_is_persisted() {
    let store = ingest_fixture();
    let occs = store.occurrences_of(&connect_id()).unwrap();
    // connect has a definition occurrence (line 3) and a reference (closure, line 8), both aligned.
    assert_eq!(
        occs.len(),
        2,
        "both connect occurrences aligned and persisted: {occs:?}"
    );
    assert!(occs.iter().any(|o| o.role == "definition"));
    assert!(occs.iter().any(|o| o.role == "reference"));
}

// _(Guarded positional join — mismatch branch)_ — an occurrence whose location does not spell the
// expected name is not persisted as aligned and the mismatch is surfaced, incl. a non-ASCII case.
#[test]
fn text_mismatch_is_refused_and_surfaced() {
    // A symbol `alpha` whose sole occurrence points at a location that spells `beta` — a drift.
    let source = "let alpha = 1;\nlet beta = 2;\n";
    // Also a non-ASCII drift: a symbol `wide` pointing at a location spelling an accented identifier.
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new("alpha", SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            // Points at "beta" on line 1 cols 4..8, which does not spell "alpha".
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(1, 4, 1, 8),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let accounting = ingest(&mut store, &ws(), &index, &src).unwrap();
    assert_eq!(accounting.text_mismatch, 1, "mismatch should be counted");
    assert_eq!(accounting.aligned_total(), 0, "mismatched occurrence must not align");

    // Non-ASCII drift: expected name is ASCII but the location spells an accented identifier.
    let nonascii = "let café = 1;\n";
    let index2 = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "n.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new("cafe", SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            // "café" occupies bytes 4..9 (é is 2 bytes) on line 0.
            occurrences: vec![ExtractedOccurrence {
                document_path: "n.rs".to_string(),
                range: SourceRange::new(0, 4, 0, 9),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store2 = GraphStore::open_in_memory().unwrap();
    let src2 = vec![("n.rs".to_string(), nonascii.to_string())];
    let acc2 = ingest(&mut store2, &ws(), &index2, &src2).unwrap();
    assert_eq!(
        acc2.text_mismatch, 1,
        "non-ASCII drift is a surfaced mismatch, not aligned"
    );
    assert_eq!(acc2.aligned_total(), 0);
}

// _(Guarded positional join — SCIP-only branch)_ — a SCIP occurrence with no syntactic construct at
// its location is recorded as unaligned, not misattributed.
#[test]
fn semantic_only_occurrence_is_unaligned() {
    let source = "fn a() {}\n";
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new("ghost", SegmentKind::Method)],
            )),
            kind: SymbolKind::Method,
            class: SymbolClass::InWorkspace,
            // A location far past the end of the source: no construct there.
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(50, 0, 50, 5),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();
    assert_eq!(acc.semantic_only, 1, "occurrence with no syntax is semantic-only");
    assert_eq!(acc.aligned_total(), 0);
    // Not misattributed: the ghost symbol has no aligned occurrence persisted.
    assert!(
        store
            .occurrences_of(&id_of(&[("ghost", SegmentKind::Method)]))
            .unwrap()
            .is_empty()
    );
}

// _(Guarded positional join — AST-only branch)_ — a syntactic construct the backend did not resolve
// is retained structurally without a fabricated identity.
#[test]
fn syntax_only_construct_is_counted_without_fabricated_identity() {
    // The fixture source declares `disconnect`, but drop its symbol so the backend "did not resolve"
    // it. Its declaration remains in the tree → syntax-only.
    let mut index = support::fixture_index();
    index.symbols.retain(|s| s.terminal_name() != Some("disconnect"));
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = sources();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();
    assert!(acc.syntax_only >= 1, "unresolved declaration counted as syntax-only");
    // No fabricated identity: nothing named disconnect is persisted.
    assert!(store.symbols_by_shortname("disconnect").unwrap().is_empty());
}

// _(Lossless symbol persistence)_ — a persisted symbol returns the same identity, occurrence set, and
// exact span text.
#[test]
fn symbol_round_trips_with_exact_span_text() {
    let store = ingest_fixture();
    let sym = store.symbol(&connect_id()).unwrap().expect("connect persisted");
    assert_eq!(sym.canonical_id, connect_id());
    // The span text is the exact function declaration body from SOURCE.
    let text = sym.span_text.expect("connect has a span");
    assert_eq!(text, "pub fn connect(&self) {}");
    // Occurrences round-trip.
    let occs = store.occurrences_of(&connect_id()).unwrap();
    assert_eq!(occs.len(), 2);
}

// _(Enclosure is persisted)_ — a method's enclosing type is returned, and a module's direct contents
// are exactly its children.
#[test]
fn enclosure_is_persisted() {
    let store = ingest_fixture();
    // connect's container is Client.
    let containers = store.containers(&connect_id()).unwrap();
    assert_eq!(containers, vec![client_id()], "connect is contained by Client");
    // The module net directly contains Client and open (its direct members).
    let contents = store.contains(&module_id()).unwrap();
    assert!(contents.contains(&client_id()), "module contains Client: {contents:?}");
    assert!(contents.contains(&open_id()), "module contains open: {contents:?}");
    // Client's methods are Client's children, not the module's.
    assert!(
        !contents.contains(&connect_id()),
        "connect is not a direct child of the module"
    );
}

/// Environment facts differing only by their package fingerprint, for the staleness write-sites.
fn env_facts(fingerprint: &str) -> EnvironmentFacts {
    EnvironmentFacts {
        interpreter_version: "Python 3.12.4".to_string(),
        environment_path: "/ws/.venv".to_string(),
        package_fingerprint: fingerprint.to_string(),
    }
}

// _(Staleness reflects underlying change — fresh branch)_ — unchanged sources and analyzer stay
// fresh, and so does an unchanged recorded environment.
#[test]
fn unchanged_sources_and_analyzer_are_fresh() {
    let store = ingest_fixture();
    let f = freshness(&store, &sources(), &support::provenance(), None).unwrap();
    assert_eq!(f, Some(Freshness::Fresh));

    // A store recorded with environment facts stays fresh while the environment in effect matches.
    let mut store = GraphStore::open_in_memory().unwrap();
    let mut index = support::fixture_index();
    index.environment = Some(env_facts("fp-a"));
    ingest(&mut store, &ws(), &index, &sources()).unwrap();
    let f = freshness(&store, &sources(), &support::provenance(), Some(&env_facts("fp-a"))).unwrap();
    assert_eq!(f, Some(Freshness::Fresh), "an unchanged environment stays fresh");
}

// _(Staleness reflects underlying change — content write-site)_
#[test]
fn changed_content_marks_stale() {
    let store = ingest_fixture();
    let changed = vec![(support::DOC.to_string(), format!("{}\n// edit\n", support::SOURCE))];
    let f = freshness(&store, &changed, &support::provenance(), None).unwrap();
    assert_eq!(f, Some(Freshness::StaleContent));
}

// _(Staleness reflects underlying change — version write-site)_
#[test]
fn changed_analyzer_version_marks_stale_and_flags_reindex() {
    let store = ingest_fixture();
    let newer = AnalyzerProvenance {
        analyzer_name: "rust-analyzer".to_string(),
        analyzer_version: "9.9.9".to_string(),
    };
    let f = freshness(&store, &sources(), &newer, None).unwrap();
    assert_eq!(f, Some(Freshness::StaleVersion));
    assert!(f.unwrap().is_stale(), "version drift flags reindex");
}

// _(Staleness reflects underlying change — environment write-site; scenario: Changed environment
// marks stale)_ — a recorded interpreter environment differing from the one in effect (here by its
// installed-package fingerprint) marks results stale and flags reindex.
#[test]
fn changed_environment_marks_stale() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let mut index = support::fixture_index();
    index.environment = Some(env_facts("fp-a"));
    ingest(&mut store, &ws(), &index, &sources()).unwrap();

    let drifted = env_facts("fp-b");
    let f = freshness(&store, &sources(), &support::provenance(), Some(&drifted)).unwrap();
    assert_eq!(f, Some(Freshness::StaleEnvironment));
    assert!(f.unwrap().is_stale(), "environment drift flags reindex");

    // An environment that can no longer be resolved is drift too, never reported fresh.
    let f = freshness(&store, &sources(), &support::provenance(), None).unwrap();
    assert_eq!(
        f,
        Some(Freshness::StaleEnvironment),
        "an unresolvable environment is drift"
    );
}

// _(Scenario: Declared environment facts ride the provenance)_ — environment facts persist on the
// index metadata and round-trip whole beside the analyzer identity; a backend that declares none
// round-trips as typed absence.
#[test]
fn environment_provenance_round_trips_through_metadata() {
    use silent_cartographer::graph::store::IndexMetadata;

    let store = GraphStore::open_in_memory().unwrap();
    let meta = IndexMetadata {
        workspace_id: ws(),
        provenance: support::provenance(),
        content_hash: "hash".to_string(),
        accounting: Default::default(),
        environment: Some(env_facts("fp-a")),
    };
    store.write_metadata(&meta).unwrap();
    let read = store.read_metadata().unwrap().expect("metadata present");
    assert_eq!(read.environment, Some(env_facts("fp-a")), "the facts round-trip whole");
    assert_eq!(
        read.provenance,
        support::provenance(),
        "the analyzer identity rides alongside"
    );

    // A backend that declares no environment (the Rust adapter) round-trips as absent.
    let meta = IndexMetadata {
        environment: None,
        ..meta
    };
    store.write_metadata(&meta).unwrap();
    let read = store.read_metadata().unwrap().expect("metadata present");
    assert_eq!(read.environment, None, "absence is typed, not defaulted");
}

// _(Join alignment accounting — the module-name bucket rides metadata)_ — a metadata round-trip
// preserves the `aligned_module_name` count alongside every other per-rule count.
#[test]
fn module_name_count_rides_metadata() {
    use silent_cartographer::graph::join::JoinAccounting;
    use silent_cartographer::graph::store::IndexMetadata;

    let store = GraphStore::open_in_memory().unwrap();
    let accounting = JoinAccounting {
        aligned_exact: 1,
        aligned_crate_root: 2,
        aligned_operator_desugar: 3,
        aligned_module_span: 4,
        aligned_self_keyword: 5,
        aligned_module_name: 6,
        text_mismatch: 7,
        semantic_only: 8,
        duplicate_ambiguous: 9,
        syntax_only: 10,
    };
    let meta = IndexMetadata {
        workspace_id: ws(),
        provenance: support::provenance(),
        content_hash: "hash".to_string(),
        accounting,
        environment: None,
    };
    store.write_metadata(&meta).unwrap();
    let read = store.read_metadata().unwrap().expect("metadata present");
    assert_eq!(
        read.accounting, accounting,
        "every per-rule count, the module-name bucket included, round-trips whole"
    );
}

// _(Reference occurrences carry enclosing-declaration attribution)_ — inside a method.
#[test]
fn reference_inside_method_attributes_to_method() {
    // Build an index where a reference to Client sits inside connect's body.
    let source = "\
struct Client;
impl Client {
    fn connect(&self) { let _c: Client = Client; }
}
";
    // The reference `Client` inside connect's body — the second `Client` on line 2 (0-based).
    let client_ref_col = source.lines().nth(2).unwrap().rfind("Client").unwrap() as u32;
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![DescriptorSegment::new("Client", SegmentKind::Type)],
                )),
                kind: SymbolKind::Type,
                class: SymbolClass::InWorkspace,
                occurrences: vec![
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(0, 7, 0, 13),
                        role: OccurrenceRole::Definition,
                    },
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(2, client_ref_col, 2, client_ref_col + 6),
                        role: OccurrenceRole::Reference,
                    },
                ],
            },
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![
                        DescriptorSegment::new("Client", SegmentKind::Type),
                        DescriptorSegment::new("connect", SegmentKind::Method),
                    ],
                )),
                kind: SymbolKind::Method,
                class: SymbolClass::InWorkspace,
                occurrences: vec![ExtractedOccurrence {
                    document_path: "m.rs".to_string(),
                    range: SourceRange::new(2, 7, 2, 14),
                    role: OccurrenceRole::Definition,
                }],
            },
        ],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let connect = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "c",
            vec![
                DescriptorSegment::new("Client", SegmentKind::Type),
                DescriptorSegment::new("connect", SegmentKind::Method),
            ],
        ),
    );
    let client = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("Client", SegmentKind::Type)]),
    );
    let refs = store.references_of(&client).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0].enclosing_id,
        Some(connect),
        "reference inside connect attributes to connect"
    );
}

// _(Reference occurrences carry enclosing-declaration attribution — closure branch)_
#[test]
fn reference_inside_closure_attributes_to_declaring_function() {
    let store = ingest_fixture();
    // connect is referenced inside a closure in `open`; attribution is `open`, not the closure.
    let refs = store.references_of(&connect_id()).unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(
        refs[0].enclosing_id,
        Some(open_id()),
        "closure reference attributes to open"
    );
}

// _(Reference occurrences carry enclosing-declaration attribution — module branch)_
#[test]
fn module_level_reference_attributes_to_module() {
    // A crate-root `use` references `Thing` at module scope, enclosed by no narrower declaration.
    let source = "\
mod thing {
    pub struct Thing;
}
use thing::Thing;
";
    // Line 3 (0-based) is `use thing::Thing;`; the `Thing` reference is at cols 11..16.
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![DescriptorSegment::new("thing", SegmentKind::Module)],
                )),
                kind: SymbolKind::Module,
                class: SymbolClass::InWorkspace,
                occurrences: vec![ExtractedOccurrence {
                    document_path: "m.rs".to_string(),
                    range: SourceRange::new(0, 4, 0, 9),
                    role: OccurrenceRole::Definition,
                }],
            },
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![
                        DescriptorSegment::new("thing", SegmentKind::Module),
                        DescriptorSegment::new("Thing", SegmentKind::Type),
                    ],
                )),
                kind: SymbolKind::Type,
                class: SymbolClass::InWorkspace,
                occurrences: vec![
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(1, 15, 1, 20),
                        role: OccurrenceRole::Definition,
                    },
                    // Module-scope reference in the `use` at line 3.
                    ExtractedOccurrence {
                        document_path: "m.rs".to_string(),
                        range: SourceRange::new(3, 11, 3, 16),
                        role: OccurrenceRole::Reference,
                    },
                ],
            },
        ],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let thing = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "c",
            vec![
                DescriptorSegment::new("thing", SegmentKind::Module),
                DescriptorSegment::new("Thing", SegmentKind::Type),
            ],
        ),
    );
    let refs = store.references_of(&thing).unwrap();
    assert_eq!(refs.len(), 1, "one module-scope reference");
    // Enclosed by no narrower declaration: attributed to the module (None), never a fabricated one.
    assert_eq!(
        refs[0].enclosing_id, None,
        "module-scope reference attributes to the module"
    );
}

// _(design.md ingest policy — local symbols)_ — a parameter or let-binding does not appear in the
// persisted symbol table.
#[test]
fn local_symbols_are_excluded() {
    // Add a local symbol (no descriptor) to the fixture; it must not be persisted.
    let mut index = support::fixture_index();
    index.symbols.push(ExtractedSymbol {
        descriptor: None,
        kind: SymbolKind::Other,
        class: SymbolClass::Local,
        occurrences: vec![ExtractedOccurrence {
            document_path: support::DOC.to_string(),
            range: SourceRange::new(7, 12, 7, 13), // the `c` let-binding
            role: OccurrenceRole::Definition,
        }],
    });
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = sources();
    ingest(&mut store, &ws(), &index, &src).unwrap();
    // The local binding `c` is not a persisted symbol.
    assert!(
        store.symbols_by_shortname("c").unwrap().is_empty(),
        "local binding must not be persisted"
    );
}

// _(design.md ingest policy — external symbols)_ — a reference to a third-party symbol persists under
// the external class with no fabricated definition span.
#[test]
fn external_symbol_persists_without_definition_span() {
    // A symbol referenced but never defined in the workspace → external.
    let source = "fn f() { g(); }\n";
    let g_col = source.find("g(").unwrap() as u32;
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "c",
                    vec![DescriptorSegment::new("f", SegmentKind::Method)],
                )),
                kind: SymbolKind::Function,
                class: SymbolClass::InWorkspace,
                occurrences: vec![ExtractedOccurrence {
                    document_path: "m.rs".to_string(),
                    range: SourceRange::new(0, 3, 0, 4),
                    role: OccurrenceRole::Definition,
                }],
            },
            ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "thirdparty",
                    vec![DescriptorSegment::new("g", SegmentKind::Method)],
                )),
                kind: SymbolKind::Function,
                class: SymbolClass::External,
                occurrences: vec![ExtractedOccurrence {
                    document_path: "m.rs".to_string(),
                    range: SourceRange::new(0, g_col, 0, g_col + 1),
                    role: OccurrenceRole::Reference,
                }],
            },
        ],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let g = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("thirdparty", vec![DescriptorSegment::new("g", SegmentKind::Method)]),
    );
    let sym = store.symbol(&g).unwrap().expect("external symbol persisted");
    assert_eq!(sym.class, PersistedClass::External);
    assert!(sym.span.is_none(), "external symbol has no fabricated definition span");
    assert!(sym.span_text.is_none());
}

// _(Join alignment accounting)_ — a build records all four join-outcome counts.
#[test]
fn build_records_four_outcome_counts() {
    let store = ingest_fixture();
    let meta = store.read_metadata().unwrap().expect("metadata recorded");
    let acc = meta.accounting;
    // The fixture is all-aligned; other counts are zero but present/retrievable.
    assert!(acc.aligned_total() > 0);
    assert_eq!(acc.text_mismatch, 0);
    assert_eq!(acc.semantic_only, 0);
    // Some declarations may be syntax-only (e.g. the impl block), which is fine; the count exists.
    let _ = acc.syntax_only;
}

// _(Join alignment accounting — conservation)_ — aligned + text-mismatch + semantic-only == total
// semantic occurrences processed.
#[test]
fn accounting_conserves_occurrence_total() {
    // A mixed fixture: the all-aligned base plus one text-mismatch occurrence (a location spelling
    // a different token) and one semantic-only occurrence (a location with no syntax), so every
    // semantic-side outcome contributes a non-zero term to the sum.
    let mut index = support::fixture_index();
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("drifted", SegmentKind::Term)],
        )),
        kind: SymbolKind::Constant,
        class: SymbolClass::InWorkspace,
        // Points at "Client" on line 1 (cols 15..21), which does not spell "drifted" → mismatch.
        occurrences: vec![ExtractedOccurrence {
            document_path: support::DOC.to_string(),
            range: SourceRange::new(1, 15, 1, 21),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("ghost", SegmentKind::Method)],
        )),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        // Far past the end of the source: no syntactic construct → semantic-only.
        occurrences: vec![ExtractedOccurrence {
            document_path: support::DOC.to_string(),
            range: SourceRange::new(90, 0, 90, 5),
            role: OccurrenceRole::Reference,
        }],
    });
    let total_occurrences: u64 = index.symbols.iter().map(|s| s.occurrences.len() as u64).sum();

    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), &index, &sources()).unwrap();
    let meta = store.read_metadata().unwrap().unwrap();

    // Every semantic-side term is non-zero, so the conservation claim is exercised across a real mix.
    assert!(
        meta.accounting.aligned_total() > 0,
        "mixed fixture has aligned occurrences"
    );
    assert!(
        meta.accounting.text_mismatch > 0,
        "mixed fixture has a text-mismatch occurrence"
    );
    assert!(
        meta.accounting.semantic_only > 0,
        "mixed fixture has a semantic-only occurrence"
    );
    assert_eq!(
        meta.accounting.total_semantic(),
        total_occurrences,
        "three semantic counts conserve the total"
    );

    // The module-name bucket is a write-site of the same conserved sum: a Python index mixing a
    // module-name acceptance with a text mismatch still conserves its occurrence total.
    let py_source = "shapes\npkg\n";
    let py_module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg.shapes", SegmentKind::Module),
                DescriptorSegment::new("__init__", SegmentKind::Meta),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            // "shapes" (line 0) aligns under the module-name rule; "pkg" (line 1) is refused.
            py_occ("m.py", 0, 0, 6, OccurrenceRole::Reference),
            py_occ("m.py", 1, 0, 3, OccurrenceRole::Reference),
        ],
    };
    let py_index = py_synthetic_index(vec![py_module]);
    let py_total: u64 = py_index.symbols.iter().map(|s| s.occurrences.len() as u64).sum();
    let mut py_store = GraphStore::open_in_memory().unwrap();
    let py_src = vec![("m.py".to_string(), py_source.to_string())];
    ingest(&mut py_store, &ws(), &py_index, &py_src).unwrap();
    let py_acc = py_store.read_metadata().unwrap().unwrap().accounting;
    assert!(
        py_acc.aligned_module_name > 0,
        "the module-name bucket contributes a non-zero term"
    );
    assert_eq!(
        py_acc.total_semantic(),
        py_total,
        "the sum including aligned_module_name conserves the occurrence total"
    );
}

// _(Workspace-namespaced identity — persisted path)_ — the same descriptor ingested under two
// workspaces persists two distinct symbols, and queries do not conflate them. A store holds exactly
// one build (whole-build supersession), so each workspace builds into its own store; the identities
// stay distinct across them, which is what keeps the future multi-workspace surface designable.
#[test]
fn two_workspace_ingest_persists_distinct_symbols() {
    use silent_cartographer::query::resolve::{Resolution, resolve};

    let index = support::fixture_index();
    let src = sources();
    let ws_a = WorkspaceId::new("workspace-a");
    let ws_b = WorkspaceId::new("workspace-b");
    let mut store_a = GraphStore::open_in_memory().unwrap();
    let mut store_b = GraphStore::open_in_memory().unwrap();
    ingest(&mut store_a, &ws_a, &index, &src).unwrap();
    ingest(&mut store_b, &ws_b, &index, &src).unwrap();

    let descriptor = Descriptor::new(
        "mycrate",
        vec![
            DescriptorSegment::new("net", SegmentKind::Module),
            DescriptorSegment::new("Client", SegmentKind::Type),
            DescriptorSegment::new("connect", SegmentKind::Method),
        ],
    );
    let id_a = silent_cartographer::identity::project_one(&ws_a, &descriptor);
    let id_b = silent_cartographer::identity::project_one(&ws_b, &descriptor);
    assert_ne!(id_a, id_b, "the two workspace projections are distinct identities");

    // Each is persisted as its own row, under its own workspace-namespaced identity.
    let row_a = store_a.symbol(&id_a).unwrap().expect("workspace-a symbol persisted");
    let row_b = store_b.symbol(&id_b).unwrap().expect("workspace-b symbol persisted");
    assert_ne!(row_a.canonical_id, row_b.canonical_id);

    // Identity-tier resolution round-trips each to exactly its own symbol — never the other's.
    match resolve(&store_a, id_a.as_str()).unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_a),
        other => panic!("expected unique for identity a, got {other:?}"),
    }
    match resolve(&store_b, id_b.as_str()).unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_b),
        other => panic!("expected unique for identity b, got {other:?}"),
    }
    // No conflation: neither store answers for the other workspace's identity.
    match resolve(&store_a, id_b.as_str()).unwrap() {
        Resolution::None => {}
        other => panic!("workspace-a's store must not answer for workspace-b's identity: {other:?}"),
    }

    // The shared qualified name resolves within each store to exactly that workspace's symbol.
    match resolve(&store_a, "net::Client::connect").unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_a),
        other => panic!("expected workspace-a's own symbol, got {other:?}"),
    }
    match resolve(&store_b, "net::Client::connect").unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_b),
        other => panic!("expected workspace-b's own symbol, got {other:?}"),
    }
}

/// A source with two structs the semantic backend describes with the identical resolved descriptor
/// `dupcrate::Widget` — the rust-analyzer true-duplicate defect — plus a reference to that descriptor.
const DUP_SOURCE: &str = "\
struct Widget;
struct Widget;
fn use_widget() {
    let _w: Widget = Widget;
}
";

/// An index over [`DUP_SOURCE`]: two `Widget` definitions sharing one descriptor, and one reference.
///
/// The reference is attached to the first twin symbol (as a backend would, unable to disambiguate);
/// the join must nonetheless refuse to attribute it to either twin.
fn duplicate_index() -> ExtractedIndex {
    let widget_descriptor = || Descriptor::new("dupcrate", vec![DescriptorSegment::new("Widget", SegmentKind::Type)]);
    // "Widget" name tokens: def at line 0 cols 7..13, def at line 1 cols 7..13; the reference on
    // line 3 is the second "Widget" there (cols 21..27).
    let twin_a = ExtractedSymbol {
        descriptor: Some(widget_descriptor()),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "dup.rs".to_string(),
                range: SourceRange::new(0, 7, 0, 13),
                role: OccurrenceRole::Definition,
            },
            // A reference the backend arbitrarily hung on twin_a — must not be attributed to it.
            ExtractedOccurrence {
                document_path: "dup.rs".to_string(),
                range: SourceRange::new(3, 21, 3, 27),
                role: OccurrenceRole::Reference,
            },
        ],
    };
    let twin_b = ExtractedSymbol {
        descriptor: Some(widget_descriptor()),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "dup.rs".to_string(),
            range: SourceRange::new(1, 7, 1, 13),
            role: OccurrenceRole::Definition,
        }],
    };
    ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "dup.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![twin_a, twin_b],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

// _(Occurrences of duplicated descriptors are never arbitrarily attributed — definition branch)_ —
// each definition occurrence of a duplicated descriptor attaches to the definition at its own
// location.
#[test]
fn duplicate_definitions_attach_by_co_location() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = duplicate_index();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    // Two definitions aligned (each at its own location); the lone reference did not.
    assert_eq!(
        acc.aligned_total(),
        2,
        "both duplicate definitions align by co-location"
    );

    // The two twins carry distinct identities and each persists its own definition span.
    let twins = store.symbols_by_shortname("Widget").unwrap();
    assert_eq!(twins.len(), 2, "two distinct Widget symbols persisted: {twins:?}");
    for twin in &twins {
        let def_occs = store.occurrences_of(&twin.canonical_id).unwrap();
        // Each twin owns exactly one definition occurrence at its own location; no reference.
        assert_eq!(def_occs.len(), 1, "twin owns only its own definition: {def_occs:?}");
        assert_eq!(def_occs[0].role, "definition");
    }
    // The two definition spans are distinct (line 0 vs line 1) — co-location, not conflation.
    let spans: std::collections::HashSet<_> = twins
        .iter()
        .flat_map(|t| store.occurrences_of(&t.canonical_id).unwrap())
        .map(|o| o.span)
        .collect();
    assert_eq!(spans.len(), 2, "definitions attach to distinct locations");
}

// _(Occurrences of duplicated descriptors are never arbitrarily attributed — reference branch)_ — a
// reference of a duplicated descriptor is recorded duplicate-ambiguous and attributed to no twin.
#[test]
fn duplicate_reference_is_ambiguous_and_unattributed() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = duplicate_index();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(acc.duplicate_ambiguous, 1, "the reference is typed duplicate-ambiguous");

    // No twin has a persisted reference occurrence — the reference is attributed to none.
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for twin in &twins {
        let refs = store.references_of(&twin.canonical_id).unwrap();
        assert!(refs.is_empty(), "no twin owns the ambiguous reference: {refs:?}");
    }

    // It is persisted as an inspectable discrepancy of kind duplicate_ambiguous instead.
    let rows = store.all_discrepancies().unwrap();
    assert!(
        rows.iter()
            .any(|r| r.outcome == "duplicate_ambiguous" && r.expected_name == "Widget"),
        "the ambiguous reference is recorded as a discrepancy: {rows:?}"
    );
}

// _(Occurrences of duplicated descriptors are never arbitrarily attributed — unduplicated branch)_ —
// references of a single-definition descriptor attribute through the ordinary guarded join.
#[test]
fn unduplicated_references_are_never_marked_ambiguous() {
    // The standard fixture has no duplicated descriptors; `connect` is referenced once inside a
    // closure and must attribute through the guarded join, never duplicate-ambiguous.
    let store = ingest_fixture();
    let meta = store.read_metadata().unwrap().unwrap();
    assert_eq!(
        meta.accounting.duplicate_ambiguous, 0,
        "no duplicate-ambiguous outcomes on an unduplicated fixture"
    );
    // connect's reference attributed normally (to `open`).
    let refs = store.references_of(&connect_id()).unwrap();
    assert_eq!(refs.len(), 1, "the single-definition reference attributes normally");
    assert_eq!(refs[0].enclosing_id, Some(open_id()));
    // No discrepancy rows for the all-aligned fixture.
    assert!(
        store.all_discrepancies().unwrap().is_empty(),
        "no discrepancies on an aligned build"
    );
}

// _(Join alignment accounting)_ — a build records the duplicate-ambiguous count alongside the other
// three semantic-side counts and syntax-only.
#[test]
fn build_records_duplicate_ambiguous_count() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = duplicate_index();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();
    let acc = store.read_metadata().unwrap().unwrap().accounting;
    // All four semantic-side buckets plus syntax-only are recorded and retrievable.
    assert_eq!(acc.aligned_total(), 2);
    assert_eq!(acc.text_mismatch, 0);
    assert_eq!(acc.semantic_only, 0);
    assert_eq!(acc.duplicate_ambiguous, 1);
    let _ = acc.syntax_only;
}

// _(Join alignment accounting — conservation)_ — with non-zero counts in all four semantic-side
// buckets, their sum equals the total occurrences processed.
#[test]
fn accounting_conserves_with_four_buckets() {
    // Extend the duplicate fixture with a text-mismatch and a semantic-only occurrence so every one
    // of the four semantic-side buckets carries a non-zero term.
    let mut index = duplicate_index();
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new("drifted", SegmentKind::Term)],
        )),
        kind: SymbolKind::Constant,
        class: SymbolClass::InWorkspace,
        // Points at "Widget" on line 0, which does not spell "drifted" → text-mismatch.
        occurrences: vec![ExtractedOccurrence {
            document_path: "dup.rs".to_string(),
            range: SourceRange::new(0, 7, 0, 13),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new("ghost", SegmentKind::Method)],
        )),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        // Far past the end of the source → semantic-only.
        occurrences: vec![ExtractedOccurrence {
            document_path: "dup.rs".to_string(),
            range: SourceRange::new(90, 0, 90, 5),
            role: OccurrenceRole::Definition,
        }],
    });
    let total: u64 = index.symbols.iter().map(|s| s.occurrences.len() as u64).sum();

    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();
    let acc = store.read_metadata().unwrap().unwrap().accounting;

    assert!(acc.aligned_total() > 0, "aligned term non-zero");
    assert!(acc.text_mismatch > 0, "text-mismatch term non-zero");
    assert!(acc.semantic_only > 0, "semantic-only term non-zero");
    assert!(acc.duplicate_ambiguous > 0, "duplicate-ambiguous term non-zero");
    assert_eq!(
        acc.total_semantic(),
        total,
        "the four semantic counts conserve the occurrence total"
    );
}

// _(Join discrepancies are inspectable)_ — a text-mismatch occurrence persists location, kind,
// expected name, and found source text, retrievable after the build.
#[test]
fn text_mismatch_detail_is_persisted() {
    // `alpha` points at a location spelling `beta` → text-mismatch with a found token.
    let source = "let alpha = 1;\nlet beta = 2;\n";
    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new("alpha", SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(1, 4, 1, 8),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let rows = store.all_discrepancies().unwrap();
    let row = rows
        .iter()
        .find(|r| r.expected_name == "alpha")
        .expect("the mismatch is persisted");
    assert_eq!(row.outcome, "text_mismatch");
    assert_eq!(row.document_path, "m.rs");
    assert_eq!(row.found_text.as_deref(), Some("beta"), "the found token is persisted");
    let span = row
        .span
        .expect("a mismatch at a normalized location carries a real span");
    assert!(span.1 > span.0, "a real location span is persisted: {row:?}");
}

// _(Join discrepancies are inspectable — supersession branch)_ — discrepancy rows from a prior build
// are absent after a new build of changed sources.
#[test]
fn discrepancies_are_superseded_per_build() {
    let mut store = GraphStore::open_in_memory().unwrap();

    // First build: a text-mismatch on `alpha`.
    let source1 = "let alpha = 1;\nlet beta = 2;\n";
    let index1 = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new("alpha", SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(1, 4, 1, 8),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    ingest(&mut store, &ws(), &index1, &[("m.rs".to_string(), source1.to_string())]).unwrap();
    assert!(
        store
            .all_discrepancies()
            .unwrap()
            .iter()
            .any(|r| r.expected_name == "alpha"),
        "first build's discrepancy is present"
    );

    // Second build over an all-aligned source: the prior discrepancy must not survive.
    let index2 = support::fixture_index();
    let src2 = vec![(support::DOC.to_string(), support::SOURCE.to_string())];
    ingest(&mut store, &ws(), &index2, &src2).unwrap();
    let rows = store.all_discrepancies().unwrap();
    assert!(
        !rows.iter().any(|r| r.expected_name == "alpha"),
        "the prior build's discrepancy is wholly superseded: {rows:?}"
    );
}

// _(Join discrepancies are inspectable — unnormalizable branch)_ — a discrepancy whose coordinates
// cannot be normalized persists its span as typed absence, never a fabricated location; a
// duplicate-ambiguous row, whose coordinates do normalize, carries its real span.
#[test]
fn unnormalizable_span_is_typed_absence_and_ambiguous_span_is_real() {
    // The duplicate fixture yields one duplicate-ambiguous reference with a real location; add a
    // symbol whose occurrence lies far past the end of the source, so its coordinates cannot
    // normalize.
    let mut index = duplicate_index();
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new("ghost", SegmentKind::Method)],
        )),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "dup.rs".to_string(),
            range: SourceRange::new(90, 0, 90, 5),
            role: OccurrenceRole::Definition,
        }],
    });
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let rows = store.all_discrepancies().unwrap();

    // The unnormalizable occurrence's span is typed absence — no fabricated (0, 0).
    let ghost = rows
        .iter()
        .find(|r| r.expected_name == "ghost")
        .expect("the unnormalizable occurrence is persisted");
    assert_eq!(ghost.outcome, "semantic_only");
    assert_eq!(ghost.span, None, "unnormalizable coordinates persist as typed absence");

    // The duplicate-ambiguous reference carries its real normalized byte span: the second `Widget`
    // on the `let` line, i.e. the fourth `Widget` token in the source.
    let widget_ref = DUP_SOURCE.match_indices("Widget").nth(3).expect("reference token").0;
    let ambiguous = rows
        .iter()
        .find(|r| r.outcome == "duplicate_ambiguous")
        .expect("the ambiguous reference is persisted");
    assert_eq!(
        ambiguous.span,
        Some((widget_ref, widget_ref + "Widget".len())),
        "the duplicate-ambiguous row carries its real normalized span"
    );
}

// _(Join discrepancies are inspectable — truncation bound)_ — a mismatch whose found text exceeds
// the persistence bound is classified on the full bytes and truncated on a UTF-8 char boundary.
#[test]
fn oversized_found_text_is_classified_on_full_bytes_and_truncated_on_a_boundary() {
    use silent_cartographer::graph::store::FOUND_TEXT_MAX_BYTES;

    // An identifier of 1 ASCII byte + 60 two-byte `é`s = 121 bytes: one past the bound, with the
    // 60th `é` straddling the boundary byte. The expected name is exactly the first 119 bytes — so
    // if classification ran on the truncated text the two would compare equal and the occurrence
    // would silently align; classification on the full bytes keeps it a mismatch.
    let found_ident = format!("a{}", "é".repeat(60));
    assert_eq!(
        found_ident.len(),
        FOUND_TEXT_MAX_BYTES + 1,
        "test setup: one byte past the bound"
    );
    let expected_name = format!("a{}", "é".repeat(59));
    let source = format!("let {found_ident} = 1;\n");

    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "m.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "c",
                vec![DescriptorSegment::new(expected_name.clone(), SegmentKind::Term)],
            )),
            kind: SymbolKind::Constant,
            class: SymbolClass::InWorkspace,
            // The occurrence covers the oversized identifier (UTF-8 columns are bytes).
            occurrences: vec![ExtractedOccurrence {
                document_path: "m.rs".to_string(),
                range: SourceRange::new(0, 4, 0, 4 + found_ident.len() as u32),
                role: OccurrenceRole::Definition,
            }],
        }],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source)];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    // Classified on the full bytes: a mismatch, not a silent alignment against the truncated text.
    assert_eq!(
        acc.text_mismatch, 1,
        "the equality check runs on full bytes before truncation"
    );
    assert_eq!(acc.aligned_total(), 0);

    let rows = store.all_discrepancies().unwrap();
    let row = rows
        .iter()
        .find(|r| r.expected_name == expected_name)
        .expect("the mismatch is persisted");
    let found = row.found_text.as_deref().expect("found text persisted");
    // Truncated to the bound on a UTF-8 boundary: the straddling `é` is dropped whole, leaving the
    // 119-byte prefix, and the persisted text is valid UTF-8 by construction (it is a `str`).
    assert!(
        found.len() <= FOUND_TEXT_MAX_BYTES,
        "found text is bounded: {} bytes",
        found.len()
    );
    assert_eq!(
        found, expected_name,
        "truncation cut on the char boundary before the straddling char"
    );
}

// content-hash gate: a non-matching tree is refused (design.md guard).
#[test]
fn content_hash_gate_refuses_non_matching_sources() {
    let index = support::fixture_index();
    let src = sources();
    let expected = content_hash(&src);
    let mut store = GraphStore::open_in_memory().unwrap();
    // Matching sources: accepted.
    assert!(join_guarded(&mut store, &ws(), &index, &src, &expected).is_ok());
    // Non-matching sources against the same expected hash: refused.
    let drifted = vec![(support::DOC.to_string(), format!("{}// drift\n", support::SOURCE))];
    let err = join_guarded(&mut store, &ws(), &index, &drifted, &expected);
    assert!(err.is_err(), "content-hash gate must refuse a non-matching tree");
}

// ---- Typed alignment rules ----

/// A one-document index over `path`.
fn one_doc_index(path: &str, symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
    ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: path.to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: symbols.to_vec(),
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

/// A symbol with one occurrence, for rule fixtures.
fn one_occ_symbol(
    package: &str,
    segments: &[(&str, SegmentKind)],
    kind: SymbolKind,
    class: SymbolClass,
    doc: &str,
    range: SourceRange,
    role: OccurrenceRole,
) -> ExtractedSymbol {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    ExtractedSymbol {
        descriptor: Some(Descriptor::new(package, segs)),
        kind,
        class,
        occurrences: vec![ExtractedOccurrence {
            document_path: doc.to_string(),
            range,
            role,
        }],
    }
}

/// The zero-based `(line, col)` of the byte at `pos` in single-byte-per-char test sources.
fn line_col(source: &str, pos: usize) -> (u32, u32) {
    let line = source[..pos].matches('\n').count() as u32;
    let line_start = source[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    (line, (pos - line_start) as u32)
}

// _(Guarded positional join — crate-root branch)_ — a use-site reference to an external crate root,
// spelled with the package name (underscored where the package is hyphenated), aligns under the
// crate-root rule.
#[test]
fn crate_root_reference_aligns_on_package_name() {
    let source = "use ext_pkg::Thing;\nfn f() {}\n";
    let tok = source.find("ext_pkg").unwrap();
    let (line, col) = line_col(source, tok);
    // The package is `ext-pkg` (hyphen), the source token `ext_pkg` (underscore) — the crates.io
    // equivalence the rule normalizes.
    let root = one_occ_symbol(
        "ext-pkg",
        &[("crate", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(line, col, line, col + 7),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![root]), &src).unwrap();

    assert_eq!(acc.aligned_crate_root, 1, "package-name token accepted by crate-root");
    assert_eq!(acc.text_mismatch, 0, "not refused as a mismatch");
    let id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("ext-pkg", vec![DescriptorSegment::new("crate", SegmentKind::Module)]),
    );
    let occs = store.occurrences_of(&id).unwrap();
    assert_eq!(occs.len(), 1, "the crate-root reference is persisted");
    assert_eq!(occs[0].rule, "crate_root", "the attribution carries its rule");
    // The package-name carve-out is scoped to duplicated groups: a unique crate root's package-name
    // reference flows through the ordinary join with no locality tag.
    assert_eq!(
        occs[0].locality, None,
        "an unduplicated crate-root attribution carries no locality provenance"
    );
}

// _(Guarded positional join — crate-root branch)_ — a `crate::` path segment aligns under the
// crate-root rule via the `crate` keyword's own node kind.
#[test]
fn crate_keyword_reference_aligns_under_crate_root() {
    let source = "mod thing { pub struct Thing; }\nuse crate::thing::Thing;\n";
    let tok = source.find("crate::").unwrap();
    let (line, col) = line_col(source, tok);
    let root = one_occ_symbol(
        "mycrate",
        &[("crate", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(line, col, line, col + 5),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![root]), &src).unwrap();
    assert_eq!(
        acc.aligned_crate_root, 1,
        "the `crate` keyword is accepted by crate-root"
    );
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch)_ — a try-expression occurrence for `branch` aligns
// under the operator-desugar rule.
#[test]
fn try_expression_aligns_for_branch() {
    let source = "fn f(x: Option<u8>) -> Option<u8> { Some(x?) }\n";
    let q = source.find('?').unwrap();
    let (line, col) = line_col(source, q);
    let branch = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Try", SegmentKind::Type),
            ("branch", SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(line, col, line, col + 1),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![branch]), &src).unwrap();
    assert_eq!(acc.aligned_operator_desugar, 1, "`?` accepted for `branch`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch)_ — a binary-operator occurrence whose single-byte
// span sits on the whitespace beside the sigil (the live rust-analyzer shape) still aligns via the
// syntax-tree construct.
#[test]
fn operator_span_adjacent_to_sigil_aligns() {
    let source = "fn f(a: u8, b: u8) -> u8 { a + b }\n";
    let space = source.find(" + ").unwrap(); // the whitespace before `+`, not the sigil itself
    let (line, col) = line_col(source, space);
    let add = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Add", SegmentKind::Type),
            ("add", SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(line, col, line, col + 1),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![add]), &src).unwrap();
    assert_eq!(acc.aligned_operator_desugar, 1, "adjacent span matched by construct");
    assert_eq!(
        acc.text_mismatch, 0,
        "byte-equality against the sigil would have refused this"
    );
}

// _(Guarded positional join — refusal branch)_ — a method outside the correspondence, failing
// name-token equality, stays refused: the rules extend the guard, they do not loosen it.
#[test]
fn method_outside_correspondence_stays_refused() {
    let source = "fn frobnicate() {}\nfn g() { frobnicate(); }\n";
    let call = source.rfind("frobnicate").unwrap();
    let (line, col) = line_col(source, call);
    let compute = one_occ_symbol(
        "c",
        &[("compute", SegmentKind::Method)],
        SymbolKind::Method,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(line, col, line, col + 10),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![compute]), &src).unwrap();
    assert_eq!(acc.text_mismatch, 1, "no rule accepts a name-shaped drift");
    assert_eq!(acc.aligned_total(), 0);
    assert_eq!(
        acc.aligned_operator_desugar, 0,
        "`compute` is not in the correspondence"
    );
}

// _(Guarded positional join — module-span branch)_ — a module definition spanning its whole
// document aligns under the module-span rule.
#[test]
fn module_definition_spanning_whole_document_aligns() {
    let source = "fn a() {}\n";
    let module = one_occ_symbol(
        "mycrate",
        &[("mymod", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        "mymod.rs",
        // Line 1, char 0 is one past the final newline: the whole document.
        SourceRange::new(0, 0, 1, 0),
        OccurrenceRole::Definition,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("mymod.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("mymod.rs", vec![module]), &src).unwrap();
    assert_eq!(acc.aligned_module_span, 1, "whole-document module definition accepted");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — module-span negative branch)_ — a whole-document span on a non-module
// is not accepted by the module-span rule.
#[test]
fn whole_document_span_on_non_module_stays_refused() {
    let source = "fn a() {}\n";
    let not_a_module = one_occ_symbol(
        "mycrate",
        &[("Thing", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(0, 0, 1, 0),
        OccurrenceRole::Definition,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![not_a_module]), &src).unwrap();
    assert_eq!(acc.aligned_module_span, 0, "module-span is gated on the module kind");
    assert_eq!(acc.aligned_total(), 0);
    assert_eq!(acc.text_mismatch, 1, "the non-module whole-document span is refused");
}

// _(Guarded positional join — provenance)_ — attributions accepted under the default rule and under
// a kind-scoped rule each carry their rule tag.
#[test]
fn attributions_carry_their_accepting_rule() {
    let source = "fn add2(a: u8, b: u8) -> u8 { a + b }\n";
    let name = source.find("add2").unwrap();
    let (nl, nc) = line_col(source, name);
    let plus = source.find('+').unwrap();
    let (pl, pc) = line_col(source, plus);

    let add2 = one_occ_symbol(
        "c",
        &[("add2", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(nl, nc, nl, nc + 4),
        OccurrenceRole::Definition,
    );
    let add = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Add", SegmentKind::Type),
            ("add", SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(pl, pc, pl, pc + 1),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![add2, add]), &src).unwrap();

    let exact_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("c", vec![DescriptorSegment::new("add2", SegmentKind::Method)]),
    );
    let exact_occs = store.occurrences_of(&exact_id).unwrap();
    assert_eq!(exact_occs.len(), 1);
    assert_eq!(exact_occs[0].rule, "exact", "default-rule attribution carries `exact`");

    let op_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "core",
            vec![
                DescriptorSegment::new("ops", SegmentKind::Module),
                DescriptorSegment::new("Add", SegmentKind::Type),
                DescriptorSegment::new("add", SegmentKind::Method),
            ],
        ),
    );
    let op_occs = store.occurrences_of(&op_id).unwrap();
    assert_eq!(op_occs.len(), 1);
    assert_eq!(
        op_occs[0].rule, "operator_desugar",
        "kind-scoped attribution carries its rule"
    );
}

// _(Guarded positional join — query-surface consequence)_ — `trace` over `references` returns a
// site aligned under the operator-desugar rule: the silent under-reporting fix.
#[test]
fn trace_references_includes_operator_aligned_site() {
    use silent_cartographer::query::output::Outcome;
    use silent_cartographer::query::{QueryEngine, Relation, TraceItem};

    let source = "fn f(a: u8, b: u8) -> u8 { a + b }\n";
    let plus = source.find('+').unwrap();
    let (pl, pc) = line_col(source, plus);
    let add = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Add", SegmentKind::Type),
            ("add", SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(pl, pc, pl, pc + 1),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![add]), &src).unwrap();

    let engine = QueryEngine::new(&store, support::provenance(), content_hash(&src), None);
    let answer = engine.trace("ops::Add::add", Relation::References).unwrap();
    match answer.outcome {
        Outcome::Found { results } => {
            assert_eq!(results.len(), 1, "the operator-aligned reference is reported");
            match &results[0] {
                TraceItem::Reference { location, .. } => {
                    assert_eq!(location.document_path, "m.rs");
                }
                other => panic!("expected a reference item, got {other:?}"),
            }
        }
        other => panic!("references must include the rule-aligned site, got {other:?}"),
    }
}

// _(Join alignment accounting — conservation)_ — per-rule acceptance buckets and refusal buckets
// are recorded and sum to the total semantic occurrences, with every bucket non-zero.
#[test]
fn per_rule_buckets_conserve_the_total() {
    let source = "\
use ext_pkg::Thing;
struct Widget;
struct Widget;
fn f(a: u8, b: u8) -> u8 { a + b }
fn g() { let _w: Widget = Widget; }
struct Holder;
impl Holder { fn h() -> Self { Holder } }
";
    let doc = "m.rs";

    // exact: the definition of `f`.
    let f_name = source.find("fn f(").unwrap() + 3;
    let (fl, fc) = line_col(source, f_name);
    let f_sym = one_occ_symbol(
        "mycrate",
        &[("f", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(fl, fc, fl, fc + 1),
        OccurrenceRole::Definition,
    );
    // crate-root: the `ext_pkg` use-site token.
    let ext = source.find("ext_pkg").unwrap();
    let (el, ec) = line_col(source, ext);
    let root = one_occ_symbol(
        "ext-pkg",
        &[("crate", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::External,
        doc,
        SourceRange::new(el, ec, el, ec + 7),
        OccurrenceRole::Reference,
    );
    // operator-desugar: `add` at the `+`.
    let plus = source.find('+').unwrap();
    let (pl, pc) = line_col(source, plus);
    let add = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Add", SegmentKind::Type),
            ("add", SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        doc,
        SourceRange::new(pl, pc, pl, pc + 1),
        OccurrenceRole::Reference,
    );
    // module-span: a module definition spanning the whole document (7 lines + final newline).
    let module = one_occ_symbol(
        "mycrate",
        &[("m", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(0, 0, 7, 0),
        OccurrenceRole::Definition,
    );
    // self-keyword: a `Holder` reference at the `Self` token inside `impl Holder`.
    let self_tok = source.find("Self").unwrap();
    let (hl, hc) = line_col(source, self_tok);
    let holder_self = one_occ_symbol(
        "mycrate",
        &[("Holder", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(hl, hc, hl, hc + 4),
        OccurrenceRole::Reference,
    );
    // text-mismatch: `drifted` pointing at the `Thing` token.
    let thing = source.find("Thing").unwrap();
    let (tl, tc) = line_col(source, thing);
    let drifted = one_occ_symbol(
        "mycrate",
        &[("drifted", SegmentKind::Term)],
        SymbolKind::Constant,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(tl, tc, tl, tc + 5),
        OccurrenceRole::Definition,
    );
    // semantic-only: a ghost far past the end of the source.
    let ghost = one_occ_symbol(
        "mycrate",
        &[("ghost", SegmentKind::Method)],
        SymbolKind::Method,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(90, 0, 90, 5),
        OccurrenceRole::Reference,
    );
    // duplicate-ambiguous: two `Widget` twins plus one reference.
    let w1 = source.find("Widget").unwrap();
    let (w1l, w1c) = line_col(source, w1);
    let w2 = source[w1 + 1..].find("Widget").unwrap() + w1 + 1;
    let (w2l, w2c) = line_col(source, w2);
    let wref = source.rfind("Widget").unwrap();
    let (wrl, wrc) = line_col(source, wref);
    let widget_descriptor = &[("Widget", SegmentKind::Type)][..];
    let mut twin_a = one_occ_symbol(
        "mycrate",
        widget_descriptor,
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(w1l, w1c, w1l, w1c + 6),
        OccurrenceRole::Definition,
    );
    twin_a.occurrences.push(ExtractedOccurrence {
        document_path: doc.to_string(),
        range: SourceRange::new(wrl, wrc, wrl, wrc + 6),
        role: OccurrenceRole::Reference,
    });
    let twin_b = one_occ_symbol(
        "mycrate",
        widget_descriptor,
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(w2l, w2c, w2l, w2c + 6),
        OccurrenceRole::Definition,
    );

    let index = one_doc_index(
        doc,
        vec![f_sym, root, add, module, holder_self, drifted, ghost, twin_a, twin_b],
    );
    let total: u64 = index.symbols.iter().map(|s| s.occurrences.len() as u64).sum();
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![(doc.to_string(), source.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let acc = store.read_metadata().unwrap().unwrap().accounting;
    assert!(acc.aligned_exact > 0, "exact bucket non-zero");
    assert!(acc.aligned_crate_root > 0, "crate-root bucket non-zero");
    assert!(acc.aligned_operator_desugar > 0, "operator bucket non-zero");
    assert!(acc.aligned_module_span > 0, "module-span bucket non-zero");
    assert!(acc.aligned_self_keyword > 0, "self-keyword bucket non-zero");
    assert!(acc.text_mismatch > 0, "text-mismatch bucket non-zero");
    assert!(acc.semantic_only > 0, "semantic-only bucket non-zero");
    assert!(acc.duplicate_ambiguous > 0, "duplicate-ambiguous bucket non-zero");
    assert_eq!(
        acc.total_semantic(),
        total,
        "rule buckets plus refusal buckets conserve the total"
    );
}

// _(Guarded positional join — self-keyword branch)_ — a `Self` return-type reference inside an impl
// of the expected type aligns under the self-keyword rule and carries its tag.
#[test]
fn self_in_own_impl_aligns_with_rule_tag() {
    let source = "\
struct GraphStore;
impl GraphStore {
    fn open() -> Self { GraphStore }
}
";
    let self_tok = source.find("Self").unwrap();
    let (sl, sc) = line_col(source, self_tok);
    let store_ref = one_occ_symbol(
        "mycrate",
        &[("GraphStore", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![store_ref]), &src).unwrap();

    assert_eq!(acc.aligned_self_keyword, 1, "`Self` in its own impl accepted");
    assert_eq!(acc.text_mismatch, 0);
    let id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("mycrate", vec![DescriptorSegment::new("GraphStore", SegmentKind::Type)]),
    );
    let occs = store.occurrences_of(&id).unwrap();
    assert_eq!(occs.len(), 1, "the Self reference is persisted");
    assert_eq!(occs[0].rule, "self_keyword", "the attribution carries its rule");
}

// _(Guarded positional join — self-keyword branch)_ — the `Self` segment of a `Self::method(...)`
// call inside an impl aligns under the self-keyword rule.
#[test]
fn self_path_segment_aligns() {
    let source = "\
struct Widget2;
impl Widget2 {
    fn new() -> Widget2 { Widget2 }
    fn wrap() -> Widget2 { Self::new() }
}
";
    let self_tok = source.find("Self::").unwrap();
    let (sl, sc) = line_col(source, self_tok);
    let widget_ref = one_occ_symbol(
        "mycrate",
        &[("Widget2", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![widget_ref]), &src).unwrap();
    assert_eq!(acc.aligned_self_keyword, 1, "`Self::` path segment accepted");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — self-keyword generic branch)_ — an expected name carrying generic
// arguments aligns at a `Self` token via base-name comparison on both sides.
#[test]
fn generic_self_type_compares_by_base_name() {
    let source = "\
struct Answer<T> {
    v: T,
}
impl<T> Answer<T> {
    fn id(self) -> Self {
        self
    }
}
";
    let self_tok = source.find("-> Self").unwrap() + 3;
    let (sl, sc) = line_col(source, self_tok);
    // The expected name carries generic arguments, as the live dogfood showed (`Answer<T>`); the
    // impl header is `impl<T> Answer<T>` — both compare as `Answer`.
    let answer_ref = one_occ_symbol(
        "mycrate",
        &[("Answer<T>", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![answer_ref]), &src).unwrap();
    assert_eq!(acc.aligned_self_keyword, 1, "generic self type accepted by base name");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — self-keyword negative branch)_ — a `Self` token whose enclosing impl
// names a different base type stays refused: the impl cross-check, not token presence, is the rule.
#[test]
fn self_in_foreign_impl_stays_refused() {
    let source = "\
struct A;
struct B;
impl A {
    fn f() -> Self { A }
}
";
    let self_tok = source.find("Self").unwrap();
    let (sl, sc) = line_col(source, self_tok);
    // A reference resolving to `B`, drifted onto the `Self` inside `impl A`.
    let b_ref = one_occ_symbol(
        "mycrate",
        &[("B", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![b_ref]), &src).unwrap();
    assert_eq!(acc.aligned_self_keyword, 0, "a foreign impl's Self is never accepted");
    assert_eq!(acc.aligned_total(), 0);
    assert_eq!(acc.text_mismatch, 1, "the drifted occurrence is refused and surfaced");
}

// _(Guarded positional join — self-keyword branch, live shape)_ — rust-analyzer resolves `Self` to
// the impl symbol (descriptor carrying an `impl` path segment, non-Type kind), not the plain type;
// the rule accepts it through the same impl-header cross-check. The terminal carries generics to
// prove base-name comparison holds on this shape too.
#[test]
fn self_resolving_to_impl_symbol_aligns() {
    let source = "\
struct Answer<T> {
    v: T,
}
impl<T> Answer<T> {
    fn id(self) -> Self {
        self
    }
}
";
    let self_tok = source.find("-> Self").unwrap() + 3;
    let (sl, sc) = line_col(source, self_tok);
    // The live shape: terminal named for the type (generics included), sitting behind an `impl`
    // path segment, with the symbol kind mapped to Other — not Type.
    let impl_symbol = one_occ_symbol(
        "mycrate",
        &[
            ("output", SegmentKind::Module),
            ("impl", SegmentKind::Meta),
            ("Answer<T>", SegmentKind::Type),
        ],
        SymbolKind::Other,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![impl_symbol]), &src).unwrap();

    assert_eq!(
        acc.aligned_self_keyword, 1,
        "the impl-symbol resolution is accepted by the self-keyword rule"
    );
    assert_eq!(
        acc.text_mismatch, 0,
        "the live Self shape no longer lands in text_mismatch"
    );
    let id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new(
            "mycrate",
            vec![
                DescriptorSegment::new("output", SegmentKind::Module),
                DescriptorSegment::new("impl", SegmentKind::Meta),
                DescriptorSegment::new("Answer<T>", SegmentKind::Type),
            ],
        ),
    );
    let occs = store.occurrences_of(&id).unwrap();
    assert_eq!(occs.len(), 1);
    assert_eq!(occs[0].rule, "self_keyword", "the attribution carries its rule");
}

// ---- Operator-desugar family coverage: one acceptance test per correspondence family ----

/// Ingest one reference occurrence of desugar-correspondence `method` at the first occurrence of
/// `token` in `source`, returning the build's accounting. Each family test pins its own method and
/// construct so a regression narrowing any single family's match fails independently.
fn desugar_case(source: &str, token: &str, method: &str) -> silent_cartographer::graph::join::JoinAccounting {
    let pos = source.find(token).unwrap();
    let (line, col) = line_col(source, pos);
    let sym = one_occ_symbol(
        "core",
        &[
            ("ops", SegmentKind::Module),
            ("Op", SegmentKind::Type),
            (method, SegmentKind::Method),
        ],
        SymbolKind::Method,
        SymbolClass::External,
        "m.rs",
        SourceRange::new(line, col, line, col + token.len() as u32),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![sym]), &src).unwrap()
}

// _(Guarded positional join — operator branch, equality family)_
#[test]
fn equality_operator_aligns_for_eq() {
    let acc = desugar_case("fn f(a: u8, b: u8) -> bool { a == b }\n", "==", "eq");
    assert_eq!(acc.aligned_operator_desugar, 1, "`==` accepted for `eq`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, ordered-comparison family)_
#[test]
fn ordered_comparison_aligns_for_lt() {
    let acc = desugar_case("fn f(a: u8, b: u8) -> bool { a < b }\n", "<", "lt");
    assert_eq!(acc.aligned_operator_desugar, 1, "`<` accepted for `lt`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, compound-assignment family)_
#[test]
fn compound_assignment_aligns_for_add_assign() {
    let acc = desugar_case("fn f(mut a: u8) { a += 1; }\n", "+=", "add_assign");
    assert_eq!(acc.aligned_operator_desugar, 1, "`+=` accepted for `add_assign`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, bitwise and shift families)_ — both parse as binary
// expressions; each sub-family is sampled at its own sigil.
#[test]
fn bitwise_and_shift_align_for_bitand_and_shl() {
    let bitand = desugar_case("fn f(a: u8, b: u8) -> u8 { a & b }\n", "&", "bitand");
    assert_eq!(bitand.aligned_operator_desugar, 1, "`&` accepted for `bitand`");
    assert_eq!(bitand.text_mismatch, 0);

    let shl = desugar_case("fn f(a: u8) -> u8 { a << 1 }\n", "<<", "shl");
    assert_eq!(shl.aligned_operator_desugar, 1, "`<<` accepted for `shl`");
    assert_eq!(shl.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, unary family)_
#[test]
fn unary_not_aligns_for_not() {
    let acc = desugar_case("fn f(a: bool) -> bool { !a }\n", "!", "not");
    assert_eq!(acc.aligned_operator_desugar, 1, "`!` accepted for `not`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, index family)_
#[test]
fn index_expression_aligns_for_index() {
    let acc = desugar_case("fn f(v: &[u8]) -> u8 { v[0] }\n", "[0]", "index");
    assert_eq!(
        acc.aligned_operator_desugar, 1,
        "the index expression accepted for `index`"
    );
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, deref family)_ — the explicit unary `*` only.
#[test]
fn explicit_deref_aligns_for_deref() {
    let acc = desugar_case("fn f(p: &u8) -> u8 { *p }\n", "*", "deref");
    assert_eq!(acc.aligned_operator_desugar, 1, "explicit `*` accepted for `deref`");
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, call family)_ — a call expression on a callable
// value.
#[test]
fn call_expression_aligns_for_call() {
    let acc = desugar_case("fn f(g: fn(u8) -> u8) -> u8 { g(1) }\n", "g(1)", "call");
    assert_eq!(
        acc.aligned_operator_desugar, 1,
        "the call expression accepted for `call`"
    );
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — operator branch, for-loop family)_ — accepted only on the loop
// construct itself.
#[test]
fn for_loop_aligns_for_into_iter() {
    let acc = desugar_case("fn f(v: Vec<u8>) { for _x in v {} }\n", "for", "into_iter");
    assert_eq!(
        acc.aligned_operator_desugar, 1,
        "the `for` construct accepted for `into_iter`"
    );
    assert_eq!(acc.text_mismatch, 0);
}

// _(Guarded positional join — self-keyword branch, qualifier arm)_ — an impl header spelling a
// path-qualified self type compares by base name: `impl output::Answer` accepts expected `Answer`.
#[test]
fn path_qualified_impl_header_compares_by_base_name() {
    let source = "\
mod output {
    pub struct Answer;
}
impl output::Answer {
    fn wrap() -> Self {
        output::Answer
    }
}
";
    let self_tok = source.find("Self").unwrap();
    let (sl, sc) = line_col(source, self_tok);
    let answer_ref = one_occ_symbol(
        "mycrate",
        &[("output", SegmentKind::Module), ("Answer", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::InWorkspace,
        "m.rs",
        SourceRange::new(sl, sc, sl, sc + 4),
        OccurrenceRole::Reference,
    );
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &one_doc_index("m.rs", vec![answer_ref]), &src).unwrap();
    assert_eq!(
        acc.aligned_self_keyword, 1,
        "the qualified impl header compares by base name"
    );
    assert_eq!(acc.text_mismatch, 0);
}

// ---- Locality attribution ----

/// A twin symbol carrying only its own definition occurrence, for a group-addressed duplicate index.
fn twin(descriptor: Descriptor, kind: SymbolKind, doc: &str, def_range: SourceRange) -> ExtractedSymbol {
    ExtractedSymbol {
        descriptor: Some(descriptor),
        kind,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: doc.to_string(),
            range: def_range,
            role: OccurrenceRole::Definition,
        }],
    }
}

/// A group reference occurrence at `doc`/`range`, for a [`DuplicateGroup`].
fn group_ref(doc: &str, range: SourceRange) -> ExtractedOccurrence {
    ExtractedOccurrence {
        document_path: doc.to_string(),
        range,
        role: OccurrenceRole::Reference,
    }
}

/// A multi-document index carrying `symbols` and one duplicate group over `descriptor` with
/// `group_occurrences`.
fn duplicate_group_index(
    documents: &[&str],
    symbols: Vec<ExtractedSymbol>,
    descriptor: Descriptor,
    group_occurrences: Vec<ExtractedOccurrence>,
) -> ExtractedIndex {
    ExtractedIndex {
        provenance: support::provenance(),
        documents: documents
            .iter()
            .map(|p| SourceDocument {
                path: p.to_string(),
                encoding: PositionEncoding::Utf8,
            })
            .collect(),
        symbols,
        duplicate_groups: vec![DuplicateGroup {
            descriptor,
            occurrences: group_occurrences,
        }],
        library_roots: Default::default(),
        environment: None,
    }
}

fn widget_descriptor() -> Descriptor {
    Descriptor::new("dupcrate", vec![DescriptorSegment::new("Widget", SegmentKind::Type)])
}

// _(Reference in one duplicate's territory is attributed to it — defining-document branch)_ — a
// group reference sitting in a twin's own defining document attributes to that twin, carrying
// `defining_document` locality provenance.
#[test]
fn reference_in_twins_defining_document_attributes_via_defining_document_locality() {
    // twin_a defines and is referenced again in a.rs; twin_b defines in b.rs.
    let a_source = "struct Widget;\nfn use_a() { let _w: Widget = Widget; }\n";
    let b_source = "struct Widget;\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    // The reference: the second "Widget" token on line 1 of a.rs.
    let ref_pos = a_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(a_source, ref_pos);
    let group_occ = group_ref("a.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = duplicate_group_index(
        &["a.rs", "b.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let src = vec![
        ("a.rs".to_string(), a_source.to_string()),
        ("b.rs".to_string(), b_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(acc.duplicate_ambiguous, 0, "the reference is settled, not ambiguous");
    assert_eq!(
        acc.aligned_exact, 3,
        "two definitions plus the settled reference align exactly"
    );

    // The reference attributed to the a.rs twin specifically, carrying defining_document provenance.
    let twins = store.symbols_by_shortname("Widget").unwrap();
    let a_twin = twins
        .iter()
        .find(|t| t.document_path.as_deref() == Some("a.rs"))
        .expect("the a.rs twin is persisted");
    let b_twin = twins
        .iter()
        .find(|t| t.document_path.as_deref() == Some("b.rs"))
        .expect("the b.rs twin is persisted");
    let a_refs = store.references_of(&a_twin.canonical_id).unwrap();
    let b_refs = store.references_of(&b_twin.canonical_id).unwrap();
    assert_eq!(
        a_refs.len(),
        1,
        "the reference attributes to the twin in whose document it sits"
    );
    assert_eq!(
        a_refs[0].locality.as_deref(),
        Some("defining_document"),
        "the attribution carries defining_document locality provenance: {a_refs:?}"
    );
    assert!(b_refs.is_empty(), "the other twin owns no reference");
}

// _(Reference in one duplicate's territory is attributed to it — provenance round-trip)_ — the
// locality rule that selected a group reference's twin is persisted alongside the attribution and
// retrieved unchanged; an ordinary (non-duplicated) attribution's locality is absent.
#[test]
fn locality_provenance_round_trips_through_the_store() {
    let a_source = "struct Widget;\nfn use_a() { let _w: Widget = Widget; }\n";
    let b_source = "struct Widget;\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let ref_pos = a_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(a_source, ref_pos);
    let group_occ = group_ref("a.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = duplicate_group_index(
        &["a.rs", "b.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let src = vec![
        ("a.rs".to_string(), a_source.to_string()),
        ("b.rs".to_string(), b_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let twins = store.symbols_by_shortname("Widget").unwrap();
    let a_twin = twins
        .iter()
        .find(|t| t.document_path.as_deref() == Some("a.rs"))
        .unwrap();
    let refs = store.references_of(&a_twin.canonical_id).unwrap();
    assert_eq!(
        refs[0].locality.as_deref(),
        Some("defining_document"),
        "the locality provenance is retrieved unchanged after persistence: {refs:?}"
    );

    // An ordinary attribution (no duplicated descriptor involved) carries no locality tag.
    let ordinary_store = ingest_fixture();
    let connect_refs = ordinary_store.references_of(&connect_id()).unwrap();
    assert_eq!(connect_refs.len(), 1);
    assert_eq!(
        connect_refs[0].locality, None,
        "an ordinary attribution's locality is absent, not a stray tag"
    );
}

// _(Reference in one duplicate's territory is attributed to it — module-chain branch)_ — a group
// reference sitting in a document reachable only through one twin's module-declaration chain
// attributes to that twin, carrying `module_chain` locality provenance.
#[test]
fn reference_reachable_only_through_one_twins_module_chain_attributes_via_module_chain_locality() {
    // Two Widget twins define from their own crate-root documents; crate_a.rs additionally declares
    // a submodule `sub` living in sub_a.rs. The group reference sits in sub_a.rs, reachable only
    // through crate_a.rs's module chain — crate_b.rs has no such submodule.
    let crate_a_source = "struct Widget;\nmod sub;\n";
    let crate_b_source = "struct Widget;\n";
    let sub_a_source = "fn use_widget() { let _w: Widget = Widget; }\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "crate_a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "crate_b.rs",
        SourceRange::new(0, 7, 0, 13),
    );

    // The module-declaration evidence `parent_document_map` walks: module `sub`'s definition
    // document is sub_a.rs, and its one reference occurrence (the `mod sub;` declaration) sits in
    // crate_a.rs, so `parent_of["sub_a.rs"] == "crate_a.rs"`. The definition occurrence spans the
    // whole sub_a.rs document (the module-span alignment rule's shape); the reference occurrence
    // sits at `sub` in line 1 of crate_a.rs (`mod sub;`, after the `struct Widget;` line).
    let sub_descriptor = Descriptor::new("dupcrate", vec![DescriptorSegment::new("sub", SegmentKind::Module)]);
    let sub_a_lines = sub_a_source.matches('\n').count() as u32;
    let sub_def = ExtractedSymbol {
        descriptor: Some(sub_descriptor),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "sub_a.rs".to_string(),
                range: SourceRange::new(0, 0, sub_a_lines, 0),
                role: OccurrenceRole::Definition,
            },
            ExtractedOccurrence {
                document_path: "crate_a.rs".to_string(),
                range: SourceRange::new(1, 4, 1, 7),
                role: OccurrenceRole::Reference,
            },
        ],
    };

    let ref_pos = sub_a_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(sub_a_source, ref_pos);
    let group_occ = group_ref("sub_a.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec!["crate_a.rs", "crate_b.rs", "sub_a.rs"]
            .into_iter()
            .map(|p| SourceDocument {
                path: p.to_string(),
                encoding: PositionEncoding::Utf8,
            })
            .collect(),
        symbols: vec![twin_a, twin_b, sub_def],
        duplicate_groups: vec![DuplicateGroup {
            descriptor: widget_descriptor(),
            occurrences: vec![group_occ],
        }],
        library_roots: Default::default(),
        environment: None,
    };

    let src = vec![
        ("crate_a.rs".to_string(), crate_a_source.to_string()),
        ("crate_b.rs".to_string(), crate_b_source.to_string()),
        ("sub_a.rs".to_string(), sub_a_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 0,
        "the reference resolves through the module chain, not ambiguous"
    );

    let twins = store.symbols_by_shortname("Widget").unwrap();
    let a_twin = twins
        .iter()
        .find(|t| t.document_path.as_deref() == Some("crate_a.rs"))
        .expect("the crate_a.rs Widget twin is persisted");
    let b_twin = twins
        .iter()
        .find(|t| t.document_path.as_deref() == Some("crate_b.rs"))
        .expect("the crate_b.rs Widget twin is persisted");
    let a_refs = store.references_of(&a_twin.canonical_id).unwrap();
    let b_refs = store.references_of(&b_twin.canonical_id).unwrap();
    assert_eq!(
        a_refs.len(),
        1,
        "the reference attributes to the twin reachable through the module chain"
    );
    assert_eq!(
        a_refs[0].locality.as_deref(),
        Some("module_chain"),
        "the attribution carries module_chain locality provenance: {a_refs:?}"
    );
    assert!(
        b_refs.is_empty(),
        "the other twin, unreachable from sub_a.rs, owns no reference"
    );
}

// _(Module-chain evidence is declaration-site only)_ — a module referenced only through use-style
// path segments (no `mod name;` declaration site anywhere) derives no parent document, so a group
// reference in that module's document stays duplicate-ambiguous instead of walking an arbitrary
// reference to the wrong twin.
#[test]
fn use_style_module_references_derive_no_parent_and_group_reference_stays_ambiguous() {
    // twin_a's own defining document (crate_a.rs) carries a use-style reference to module `sub` —
    // the misattribution bait: a first-reference-wins derivation would walk sub.rs → crate_a.rs and
    // hand the reference to twin_a. Neither reference is a `mod sub;` declaration.
    let crate_a_source = "struct Widget;\nuse sub::thing;\n";
    let crate_b_source = "struct Widget;\n";
    let other_source = "use sub::other;\n";
    let sub_source = "fn use_widget() { let _w: Widget = Widget; }\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "crate_a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "crate_b.rs",
        SourceRange::new(0, 7, 0, 13),
    );

    // Module `sub` defined in sub.rs, referenced from crate_a.rs and other.rs — both use-style path
    // segments, neither a declaration site.
    let a_ref_pos = crate_a_source.find("sub").unwrap();
    let (al, ac) = line_col(crate_a_source, a_ref_pos);
    let o_ref_pos = other_source.find("sub").unwrap();
    let (ol, oc) = line_col(other_source, o_ref_pos);
    let sub_lines = sub_source.matches('\n').count() as u32;
    let sub_def = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "dupcrate",
            vec![DescriptorSegment::new("sub", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "sub.rs".to_string(),
                range: SourceRange::new(0, 0, sub_lines, 0),
                role: OccurrenceRole::Definition,
            },
            ExtractedOccurrence {
                document_path: "crate_a.rs".to_string(),
                range: SourceRange::new(al, ac, al, ac + 3),
                role: OccurrenceRole::Reference,
            },
            ExtractedOccurrence {
                document_path: "other.rs".to_string(),
                range: SourceRange::new(ol, oc, ol, oc + 3),
                role: OccurrenceRole::Reference,
            },
        ],
    };

    let ref_pos = sub_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(sub_source, ref_pos);
    let group_occ = group_ref("sub.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec!["crate_a.rs", "crate_b.rs", "other.rs", "sub.rs"]
            .into_iter()
            .map(|p| SourceDocument {
                path: p.to_string(),
                encoding: PositionEncoding::Utf8,
            })
            .collect(),
        symbols: vec![twin_a, twin_b, sub_def],
        duplicate_groups: vec![DuplicateGroup {
            descriptor: widget_descriptor(),
            occurrences: vec![group_occ],
        }],
        library_roots: Default::default(),
        environment: None,
    };

    let src = vec![
        ("crate_a.rs".to_string(), crate_a_source.to_string()),
        ("crate_b.rs".to_string(), crate_b_source.to_string()),
        ("other.rs".to_string(), other_source.to_string()),
        ("sub.rs".to_string(), sub_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "no declaration-site evidence connects sub.rs to any twin, so the reference is refused"
    );
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(
            store.references_of(&t.canonical_id).unwrap().is_empty(),
            "no twin is attributed through use-style reference evidence"
        );
    }
}

// _(Package-name reference without target metadata is typed ambiguous)_ — a package-name token
// denotes the package's library target no matter which document it sits in, so containing-document
// locality must never hand it to the containing file's own crate root; with no authoritative target
// description available (the index's library-root map is empty), it is refused to
// duplicate-ambiguous. The `crate`-keyword form in the same document is target-relative by
// construction and keeps its locality attribution.
#[test]
fn package_name_reference_among_duplicated_crate_roots_is_ambiguous_while_crate_keyword_attributes() {
    // Two crate-root twins sharing descriptor `dupcrate` + terminal segment `crate` (package name
    // differs from the terminal name). root_a.rs carries BOTH reference forms. `library_roots` is
    // empty (the `duplicate_group_index` helper's default): no target metadata is available.
    let root_a_source = "use dupcrate::thing;\nuse crate::other;\n";
    let root_b_source = "fn placeholder() {}\n";

    let crate_descriptor = || Descriptor::new("dupcrate", vec![DescriptorSegment::new("crate", SegmentKind::Module)]);
    let a_lines = root_a_source.matches('\n').count() as u32;
    let b_lines = root_b_source.matches('\n').count() as u32;
    // Crate-root twin definitions span their whole documents (the module-span rule's shape).
    let twin_a = twin(
        crate_descriptor(),
        SymbolKind::Module,
        "root_a.rs",
        SourceRange::new(0, 0, a_lines, 0),
    );
    let twin_b = twin(
        crate_descriptor(),
        SymbolKind::Module,
        "root_b.rs",
        SourceRange::new(0, 0, b_lines, 0),
    );

    // Group references, both in twin_a's own defining document: the package-name token and the
    // `crate` keyword token.
    let pkg_pos = root_a_source.find("dupcrate").unwrap();
    let (pl, pc) = line_col(root_a_source, pkg_pos);
    let pkg_occ = group_ref("root_a.rs", SourceRange::new(pl, pc, pl, pc + "dupcrate".len() as u32));
    let kw_pos = root_a_source.find("crate::other").unwrap();
    let (kl, kc) = line_col(root_a_source, kw_pos);
    let kw_occ = group_ref("root_a.rs", SourceRange::new(kl, kc, kl, kc + "crate".len() as u32));

    let index = duplicate_group_index(
        &["root_a.rs", "root_b.rs"],
        vec![twin_a, twin_b],
        crate_descriptor(),
        vec![pkg_occ, kw_occ],
    );
    let src = vec![
        ("root_a.rs".to_string(), root_a_source.to_string()),
        ("root_b.rs".to_string(), root_b_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "the package-name token is refused to duplicate-ambiguous, never attributed by locality"
    );
    assert_eq!(
        acc.aligned_crate_root, 1,
        "the crate-keyword token still attributes under the crate-root rule"
    );

    let roots = store.symbols_by_shortname("crate").unwrap();
    let a_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("root_a.rs"))
        .expect("the root_a.rs twin is persisted");
    let b_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("root_b.rs"))
        .expect("the root_b.rs twin is persisted");
    let a_refs = store.references_of(&a_root.canonical_id).unwrap();
    assert_eq!(
        a_refs.len(),
        1,
        "only the crate-keyword reference attributes to the containing twin: {a_refs:?}"
    );
    assert_eq!(a_refs[0].rule, "crate_root");
    assert_eq!(
        a_refs[0].locality.as_deref(),
        Some("defining_document"),
        "the keyword attribution keeps its locality provenance: {a_refs:?}"
    );
    assert!(
        store.references_of(&b_root.canonical_id).unwrap().is_empty(),
        "the other twin owns nothing"
    );

    // The refused package-name occurrence is inspectable as a duplicate-ambiguous discrepancy.
    let rows = store.all_discrepancies().unwrap();
    assert!(
        rows.iter()
            .any(|r| r.outcome == "duplicate_ambiguous" && r.expected_name == "crate"),
        "the package-name refusal is surfaced: {rows:?}"
    );
}

// _(Package-name reference resolves to the library target)_ — with the build system's target
// description available, a package-name token attributes to the twin defined at the package's
// library root — never to the containing document's own crate root — under `target_metadata`
// locality provenance; the `crate`-keyword token in the same document keeps ordinary locality.
#[test]
fn package_name_reference_resolves_to_the_library_twin_via_target_metadata() {
    // Two crate-root twins; the build metadata names root_b.rs as the library target's root. The
    // package-name token sits in root_a.rs — the OTHER twin's document, the misattribution bait.
    let root_a_source = "use dupcrate::thing;\nuse crate::other;\n";
    let root_b_source = "fn placeholder() {}\n";

    let crate_descriptor = || Descriptor::new("dupcrate", vec![DescriptorSegment::new("crate", SegmentKind::Module)]);
    let a_lines = root_a_source.matches('\n').count() as u32;
    let b_lines = root_b_source.matches('\n').count() as u32;
    let twin_a = twin(
        crate_descriptor(),
        SymbolKind::Module,
        "root_a.rs",
        SourceRange::new(0, 0, a_lines, 0),
    );
    let twin_b = twin(
        crate_descriptor(),
        SymbolKind::Module,
        "root_b.rs",
        SourceRange::new(0, 0, b_lines, 0),
    );

    let pkg_pos = root_a_source.find("dupcrate").unwrap();
    let (pl, pc) = line_col(root_a_source, pkg_pos);
    let pkg_occ = group_ref("root_a.rs", SourceRange::new(pl, pc, pl, pc + "dupcrate".len() as u32));
    let kw_pos = root_a_source.find("crate::other").unwrap();
    let (kl, kc) = line_col(root_a_source, kw_pos);
    let kw_occ = group_ref("root_a.rs", SourceRange::new(kl, kc, kl, kc + "crate".len() as u32));

    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec!["root_a.rs", "root_b.rs"]
            .into_iter()
            .map(|p| SourceDocument {
                path: p.to_string(),
                encoding: PositionEncoding::Utf8,
            })
            .collect(),
        symbols: vec![twin_a, twin_b],
        duplicate_groups: vec![DuplicateGroup {
            descriptor: crate_descriptor(),
            occurrences: vec![pkg_occ, kw_occ],
        }],
        library_roots: [("dupcrate".to_string(), "root_b.rs".to_string())].into(),
        environment: None,
    };
    let src = vec![
        ("root_a.rs".to_string(), root_a_source.to_string()),
        ("root_b.rs".to_string(), root_b_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 0,
        "both reference forms resolve — nothing is left ambiguous"
    );
    assert_eq!(
        acc.aligned_crate_root, 2,
        "package-name and crate-keyword tokens both align under the crate-root rule"
    );

    let roots = store.symbols_by_shortname("crate").unwrap();
    let a_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("root_a.rs"))
        .expect("the root_a.rs twin is persisted");
    let b_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("root_b.rs"))
        .expect("the root_b.rs twin is persisted");

    // The package-name token attributes to the LIBRARY twin (root_b.rs), never to the containing
    // document's own crate root, and carries the target-description evidence as provenance.
    let b_refs = store.references_of(&b_root.canonical_id).unwrap();
    assert_eq!(
        b_refs.len(),
        1,
        "the package-name reference attributes to the library twin: {b_refs:?}"
    );
    assert_eq!(b_refs[0].rule, "crate_root");
    assert_eq!(
        b_refs[0].locality.as_deref(),
        Some("target_metadata"),
        "the attribution carries the target-metadata evidence: {b_refs:?}"
    );

    // The crate-keyword token keeps ordinary locality: it attributes to its containing twin.
    let a_refs = store.references_of(&a_root.canonical_id).unwrap();
    assert_eq!(
        a_refs.len(),
        1,
        "the crate-keyword reference attributes to the containing twin: {a_refs:?}"
    );
    assert_eq!(
        a_refs[0].locality.as_deref(),
        Some("defining_document"),
        "the keyword attribution keeps ordinary locality provenance: {a_refs:?}"
    );
}

// _(Locality does not bypass the guarded join)_ — a group reference in exactly one twin's
// defining document, whose source text does not spell the expected name, is refused as
// text-mismatch rather than attributed.
#[test]
fn locality_selected_occurrence_with_mismatched_text_is_refused_not_attributed() {
    let a_source = "struct Widget;\nfn use_a() { let _w = OTHER; }\n";
    let b_source = "struct Widget;\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    // The group occurrence's range points at "OTHER", not "Widget" — a text mismatch even though
    // a.rs is uniquely twin_a's defining document.
    let other_pos = a_source.find("OTHER").unwrap();
    let (rl, rc) = line_col(a_source, other_pos);
    let group_occ = group_ref("a.rs", SourceRange::new(rl, rc, rl, rc + "OTHER".len() as u32));

    let index = duplicate_group_index(
        &["a.rs", "b.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let src = vec![
        ("a.rs".to_string(), a_source.to_string()),
        ("b.rs".to_string(), b_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.text_mismatch, 1,
        "locality selects the twin but the text still refuses"
    );
    assert_eq!(
        acc.duplicate_ambiguous, 0,
        "locality found a unique twin, so it is not ambiguous"
    );

    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(
            store.references_of(&t.canonical_id).unwrap().is_empty(),
            "no twin is attributed the mismatched reference"
        );
    }
    let rows = store.all_discrepancies().unwrap();
    assert!(
        rows.iter()
            .any(|r| r.outcome == "text_mismatch" && r.expected_name == "Widget"),
        "the refusal is surfaced as a text-mismatch discrepancy: {rows:?}"
    );
}

/// The canonical identity of the fixture's `size`-property `self` param twin at collision rank
/// `rank` (the getter's twin ranks 0 — its definition precedes the setter's in `pkg/shapes.py`).
fn py_size_self_twin_id(rank: u32) -> CanonicalId {
    CanonicalId::from_raw(format!(
        "py-ws::python-conformance::pkg.shapes::Widget::size::self#{rank}"
    ))
}

// _(Scenario: Same-document reference inside exactly one twin's scope is attributed to it)_ — the
// fixture's `size` property getter/setter pair share one `self`-param descriptor (a real
// same-document twin group in the committed index); the getter-body `self` reference attributes to
// the getter's twin and the setter-body `self` reference to the setter's twin, each carrying
// `declaration_scope` locality provenance.
#[test]
fn same_document_reference_inside_one_twin_scope_attributes() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::python_fixture_index();
    let sources = support::python_fixture_sources();
    let acc = ingest(&mut store, &py_ws(), &index, &sources).unwrap();

    // Both twin-group references resolve by declaration scope: the ambiguous bucket is empty, and
    // the duplicated-group disclosure still reports the twin group itself (attribution settles
    // references, it does not un-disclose the duplicate).
    assert_eq!(
        acc.duplicate_ambiguous, 0,
        "the scope-resolved references left the ambiguous bucket"
    );
    let groups = store.duplicated_groups().unwrap();
    assert_eq!(
        groups.len(),
        1,
        "the size-property self twin group is still disclosed: {groups:?}"
    );

    // Getter twin (#0): its definition is the `self` in `def size(self):`; the scope-attributed
    // reference is the `self` in the getter body (`return self._size`).
    let getter_refs = store.references_of(&py_size_self_twin_id(0)).unwrap();
    assert_eq!(
        getter_refs.len(),
        1,
        "the getter-body self reference attributes to the getter twin: {getter_refs:?}"
    );
    assert_eq!(getter_refs[0].document_path, "pkg/shapes.py");
    assert_eq!(
        getter_refs[0].locality.as_deref(),
        Some("declaration_scope"),
        "the attribution carries declaration_scope locality provenance: {getter_refs:?}"
    );

    // Setter twin (#1): the `self` in the setter body (`self._size = value`).
    let setter_refs = store.references_of(&py_size_self_twin_id(1)).unwrap();
    assert_eq!(
        setter_refs.len(),
        1,
        "the setter-body self reference attributes to the setter twin: {setter_refs:?}"
    );
    assert_eq!(setter_refs[0].document_path, "pkg/shapes.py");
    assert_eq!(
        setter_refs[0].locality.as_deref(),
        Some("declaration_scope"),
        "the attribution carries declaration_scope locality provenance: {setter_refs:?}"
    );

    // The two references are distinct sites (getter body precedes setter body).
    assert_ne!(getter_refs[0].span, setter_refs[0].span, "distinct reference sites");
    assert!(
        getter_refs[0].span.0 < setter_refs[0].span.0,
        "the getter-body reference precedes the setter-body reference"
    );
}

// _(Scenario: Same-document reference whose deciding scope holds several twins stays ambiguous)_ —
// two twin definitions inside one declaration (a module block) and a reference in a sibling
// function of that block: the innermost twin-bearing declaration (the block) holds both twins, so
// the reference stays duplicate-ambiguous.
#[test]
fn same_document_reference_with_shared_deciding_scope_stays_ambiguous() {
    let source = "\
mod holder {
    struct Widget;
    struct Widget;
    fn sib() { let _w: Widget = Widget; }
}
";
    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(1, 11, 1, 17),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(2, 11, 2, 17),
    );
    // The reference: the first "Widget" token inside `sib` (line 3).
    let group_occ = group_ref("dup.rs", SourceRange::new(3, 23, 3, 29));

    let index = duplicate_group_index(&["dup.rs"], vec![twin_a, twin_b], widget_descriptor(), vec![group_occ]);
    let src = vec![("dup.rs".to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "the deciding scope holds both twins, so the reference stays ambiguous"
    );
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(
            store.references_of(&t.canonical_id).unwrap().is_empty(),
            "no twin is attributed the ambiguous reference"
        );
    }
}

// _(Scenario: Same-document reference enclosed by no twin-bearing declaration stays ambiguous)_ —
// two twins at document top level and a reference inside a function: no enclosing declaration
// contains a twin definition (the document itself is not a deciding scope), so the reference stays
// duplicate-ambiguous.
#[test]
fn same_document_reference_outside_any_twin_scope_stays_ambiguous() {
    let source = "\
struct Widget;
struct Widget;
fn use_widget() { let _w: Widget = Widget; }
";
    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(1, 7, 1, 13),
    );
    // The reference: the first "Widget" token inside `use_widget` (line 2).
    let group_occ = group_ref("dup.rs", SourceRange::new(2, 26, 2, 32));

    let index = duplicate_group_index(&["dup.rs"], vec![twin_a, twin_b], widget_descriptor(), vec![group_occ]);
    let src = vec![("dup.rs".to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "no enclosing declaration contains a twin, so the reference stays ambiguous"
    );
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(
            store.references_of(&t.canonical_id).unwrap().is_empty(),
            "no twin is attributed the ambiguous reference"
        );
    }
}

// _(Scenario: Scope locality does not bypass the guarded join)_ — a reference inside exactly one
// twin's scope whose source token satisfies no alignment rule is refused with zero aligned rows;
// scope locality selects the target, it never overrides a text refusal.
#[test]
fn scope_locality_does_not_bypass_alignment() {
    let source = "\
fn a() {
    struct Widget;
    let _x = OTHER;
}
fn b() {
    struct Widget;
}
";
    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(1, 11, 1, 17),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "dup.rs",
        SourceRange::new(5, 11, 5, 17),
    );
    // The reference: the "OTHER" token inside `a` — exactly one twin (twin_a) is in scope, but the
    // token does not spell "Widget".
    let group_occ = group_ref("dup.rs", SourceRange::new(2, 13, 2, 18));

    let index = duplicate_group_index(&["dup.rs"], vec![twin_a, twin_b], widget_descriptor(), vec![group_occ]);
    let src = vec![("dup.rs".to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.text_mismatch, 1,
        "scope locality selects the twin but the text still refuses"
    );
    assert_eq!(
        acc.duplicate_ambiguous, 0,
        "scope locality found a unique twin, so it is not ambiguous"
    );
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(
            store.references_of(&t.canonical_id).unwrap().is_empty(),
            "no twin is attributed the mismatched reference"
        );
    }
}

// _(Reference outside every duplicate's territory is typed ambiguous)_ — a group reference whose
// document is associated with no twin (no defining-document match, no module-chain path) is typed
// duplicate-ambiguous.
#[test]
fn reference_outside_every_twins_territory_is_ambiguous() {
    let a_source = "struct Widget;\n";
    let b_source = "struct Widget;\n";
    let c_source = "fn use_c() { let _w: Widget = Widget; }\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    // c.rs is neither twin's defining document, and no module-chain evidence connects it to either.
    let ref_pos = c_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(c_source, ref_pos);
    let group_occ = group_ref("c.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = duplicate_group_index(
        &["a.rs", "b.rs", "c.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let src = vec![
        ("a.rs".to_string(), a_source.to_string()),
        ("b.rs".to_string(), b_source.to_string()),
        ("c.rs".to_string(), c_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "the reference lies outside every twin's territory"
    );
    let twins = store.symbols_by_shortname("Widget").unwrap();
    for t in &twins {
        assert!(store.references_of(&t.canonical_id).unwrap().is_empty());
    }
}

// _(Reference outside every duplicate's territory is typed ambiguous — inspectability)_ — the
// surfaced group-ambiguous discrepancy carries the group's shared identity base (the twins' common
// identity with the `#<rank>` disambiguator stripped), never an empty symbol, so an inspecting
// consumer sees which descriptor group the ambiguity belongs to.
#[test]
fn group_ambiguous_discrepancy_names_the_group_identity() {
    use silent_cartographer::graph::join::{JoinOutcome, SourceCorpus, join};

    let a_source = "struct Widget;\n";
    let b_source = "struct Widget;\n";
    let c_source = "fn use_c() { let _w: Widget = Widget; }\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let ref_pos = c_source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(c_source, ref_pos);
    let group_occ = group_ref("c.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = duplicate_group_index(
        &["a.rs", "b.rs", "c.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let corpus = SourceCorpus::new([("a.rs", a_source), ("b.rs", b_source), ("c.rs", c_source)]);
    // The twins' identities as identity projection would assign them: shared base, `#<rank>` each.
    let identities = vec![
        Some(CanonicalId::from_raw("test-ws::dupcrate::Widget#0")),
        Some(CanonicalId::from_raw("test-ws::dupcrate::Widget#1")),
    ];

    let result = join(
        &index,
        &corpus,
        &identities,
        silent_cartographer::graph::syntax::Language::Rust,
    );
    let ambiguous = result
        .unaligned
        .iter()
        .find(|u| u.outcome == JoinOutcome::DuplicateAmbiguous)
        .expect("the outside-territory reference is surfaced as duplicate-ambiguous");
    assert!(
        !ambiguous.symbol.as_str().is_empty(),
        "the discrepancy carries a non-empty symbol: {ambiguous:?}"
    );
    assert_eq!(
        ambiguous.symbol.as_str(),
        "test-ws::dupcrate::Widget",
        "the discrepancy names the group's shared identity base: {ambiguous:?}"
    );

    // Degenerate branch: no twin was persisted at all (identities withheld). The discrepancy still
    // names the group non-emptily, falling back to the descriptor's terminal name.
    let no_identities = vec![None, None];
    let result = join(
        &index,
        &corpus,
        &no_identities,
        silent_cartographer::graph::syntax::Language::Rust,
    );
    let ambiguous = result
        .unaligned
        .iter()
        .find(|u| u.outcome == JoinOutcome::DuplicateAmbiguous)
        .expect("the group reference is still surfaced without persisted twins");
    assert!(
        !ambiguous.symbol.as_str().is_empty(),
        "even with no persisted twins the discrepancy names the group: {ambiguous:?}"
    );
}

// _(Reference in shared territory is typed ambiguous)_ — a group reference whose document is the
// defining document of more than one twin (a genuinely shared document) is typed duplicate-ambiguous.
#[test]
fn reference_in_shared_territory_is_ambiguous() {
    // Both twins define from the same document (a file compiled into two targets, the design's
    // known hard case) — the defining-document rule finds two matches, not one.
    let source = "struct Widget;\nfn use_shared() { let _w: Widget = Widget; }\n";
    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "shared.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "shared.rs",
        SourceRange::new(0, 7, 0, 13),
    );

    let ref_pos = source.match_indices("Widget").nth(1).unwrap().0;
    let (rl, rc) = line_col(source, ref_pos);
    let group_occ = group_ref("shared.rs", SourceRange::new(rl, rc, rl, rc + "Widget".len() as u32));

    let index = duplicate_group_index(
        &["shared.rs"],
        vec![twin_a, twin_b],
        widget_descriptor(),
        vec![group_occ],
    );
    let src = vec![("shared.rs".to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.duplicate_ambiguous, 1,
        "a document that is both twins' defining document settles to ambiguous, not a guess"
    );
}

// _(Join alignment accounting — conservation, locality-attributed input shape)_ — with a mix of
// locality-attributed group references and duplicate-ambiguous refusals, the per-rule acceptance
// counts and refusal counts still conserve the total occurrence count.
#[test]
fn conservation_holds_with_locality_attributed_and_ambiguous_group_references() {
    let a_source = "struct Widget;\nfn use_a() { let _w: Widget = Widget; }\n";
    let b_source = "struct Widget;\n";
    let c_source = "fn use_c() { let _w: Widget = Widget; }\n";

    let twin_a = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "a.rs",
        SourceRange::new(0, 7, 0, 13),
    );
    let twin_b = twin(
        widget_descriptor(),
        SymbolKind::Type,
        "b.rs",
        SourceRange::new(0, 7, 0, 13),
    );

    let a_ref_pos = a_source.match_indices("Widget").nth(1).unwrap().0;
    let (arl, arc) = line_col(a_source, a_ref_pos);
    let settled_occ = group_ref("a.rs", SourceRange::new(arl, arc, arl, arc + "Widget".len() as u32));

    let c_ref_pos = c_source.match_indices("Widget").nth(1).unwrap().0;
    let (crl, crc) = line_col(c_source, c_ref_pos);
    let ambiguous_occ = group_ref("c.rs", SourceRange::new(crl, crc, crl, crc + "Widget".len() as u32));

    let index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec!["a.rs", "b.rs", "c.rs"]
            .into_iter()
            .map(|p| SourceDocument {
                path: p.to_string(),
                encoding: PositionEncoding::Utf8,
            })
            .collect(),
        symbols: vec![twin_a, twin_b],
        duplicate_groups: vec![DuplicateGroup {
            descriptor: widget_descriptor(),
            occurrences: vec![settled_occ, ambiguous_occ],
        }],
        library_roots: Default::default(),
        environment: None,
    };
    let total_occurrences = 2 /* definitions */ + 2 /* group references */;

    let src = vec![
        ("a.rs".to_string(), a_source.to_string()),
        ("b.rs".to_string(), b_source.to_string()),
        ("c.rs".to_string(), c_source.to_string()),
    ];
    let mut store = GraphStore::open_in_memory().unwrap();
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(acc.duplicate_ambiguous, 1, "the unsettled reference is ambiguous");
    assert_eq!(
        acc.aligned_exact, 3,
        "two definitions plus the locality-settled reference align"
    );
    assert_eq!(
        acc.total_semantic(),
        total_occurrences,
        "per-rule acceptances plus refusals conserve the total occurrence count"
    );
}

// _(Store & Schema — per-target imports edges)_ — after normalization splits twin crate roots into
// distinct symbols, each target's own module-scope reference produces an `imports` edge from that
// target's own crate-root identity, not a shared/collapsed one (the blast-radius dedup-collapse
// caveat the crate-root workaround existed for).
#[test]
fn twin_crate_roots_produce_per_target_imports_edges_after_normalization() {
    // Two crate roots sharing an identical descriptor (the rust-analyzer true-duplicate defect),
    // each with its own module-scope `use` reference to a distinct external symbol.
    let source_a = "use ext::Alpha;";
    let source_b = "use ext::Beta;";
    let alpha_tok = source_a.find("Alpha").unwrap();
    let (al, ac) = line_col(source_a, alpha_tok);
    let beta_tok = source_b.find("Beta").unwrap();
    let (bl, bc) = line_col(source_b, beta_tok);

    let crate_descriptor = || Descriptor::new("dupcrate", vec![DescriptorSegment::new("crate", SegmentKind::Module)]);
    // Both definition occurrences land on the same merged symbol, as a backend that has not yet
    // learned to split twins would emit — `normalize` is what performs the split under test.
    let merged_root = ExtractedSymbol {
        descriptor: Some(crate_descriptor()),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            ExtractedOccurrence {
                document_path: "a.rs".to_string(),
                range: SourceRange::new(0, 0, 0, source_a.len() as u32),
                role: OccurrenceRole::Definition,
            },
            ExtractedOccurrence {
                document_path: "b.rs".to_string(),
                range: SourceRange::new(0, 0, 0, source_b.len() as u32),
                role: OccurrenceRole::Definition,
            },
        ],
    };
    let alpha = one_occ_symbol(
        "ext",
        &[("Alpha", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::External,
        "a.rs",
        SourceRange::new(al, ac, al, ac + 5),
        OccurrenceRole::Reference,
    );
    let beta = one_occ_symbol(
        "ext",
        &[("Beta", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::External,
        "b.rs",
        SourceRange::new(bl, bc, bl, bc + 4),
        OccurrenceRole::Reference,
    );

    let raw_index = ExtractedIndex {
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
        symbols: vec![merged_root, alpha, beta],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    // The normalization pass every backend flows through: splits the merged crate root into two
    // distinct twin symbols, one per definition document.
    let index = normalize(raw_index);
    assert_eq!(
        index.symbols.iter().filter(|s| s.kind == SymbolKind::Module).count(),
        2,
        "the merged crate root split into two distinct module symbols"
    );

    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![
        ("a.rs".to_string(), source_a.to_string()),
        ("b.rs".to_string(), source_b.to_string()),
    ];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let roots = store.symbols_by_shortname("crate").unwrap();
    assert_eq!(roots.len(), 2, "two distinct crate-root symbols persisted: {roots:?}");
    let a_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("a.rs"))
        .expect("the a.rs crate root is persisted");
    let b_root = roots
        .iter()
        .find(|r| r.document_path.as_deref() == Some("b.rs"))
        .expect("the b.rs crate root is persisted");

    let alpha_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("ext", vec![DescriptorSegment::new("Alpha", SegmentKind::Type)]),
    );
    let beta_id = silent_cartographer::identity::project_one(
        &ws(),
        &Descriptor::new("ext", vec![DescriptorSegment::new("Beta", SegmentKind::Type)]),
    );

    let imports = store.edges(EdgeKind::Imports).unwrap();
    assert!(
        imports.contains(&(a_root.canonical_id.clone(), alpha_id.clone())),
        "a.rs's own crate root imports Alpha: {imports:?}"
    );
    assert!(
        imports.contains(&(b_root.canonical_id.clone(), beta_id.clone())),
        "b.rs's own crate root imports Beta: {imports:?}"
    );
    assert!(
        !imports.contains(&(a_root.canonical_id.clone(), beta_id)),
        "a.rs's crate root does not import b.rs's target: {imports:?}"
    );
    assert!(
        !imports.contains(&(b_root.canonical_id.clone(), alpha_id)),
        "b.rs's crate root does not import a.rs's target: {imports:?}"
    );
}

// ---- Duplicated-descriptor disclosure ----

// _(Duplicated descriptors are disclosed — retrievable branch, store surface)_ — a store with a
// duplicated descriptor's twins returns one group naming the shared descriptor's base and both
// definitions.
#[test]
fn store_duplicated_groups_returns_the_shared_descriptor_and_its_definitions() {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = duplicate_index();
    let src = vec![("dup.rs".to_string(), DUP_SOURCE.to_string())];
    ingest(&mut store, &ws(), &index, &src).unwrap();

    let groups = store.duplicated_groups().unwrap();
    assert_eq!(groups.len(), 1, "one duplicated-descriptor group: {groups:?}");
    let group = &groups[0];
    assert_eq!(group.definitions.len(), 2, "both twins listed: {group:?}");
    for d in &group.definitions {
        assert_eq!(
            d.display_name, "Widget",
            "each group member is a Widget definition: {group:?}"
        );
    }
}

// _(Duplicated descriptors are disclosed — no-duplicates branch, store surface)_ — a store with no
// duplicated descriptor returns a definite empty set, not a failure.
#[test]
fn store_duplicated_groups_is_a_definite_empty_set_without_duplicates() {
    let store = ingest_fixture();
    let groups = store.duplicated_groups().unwrap();
    assert!(
        groups.is_empty(),
        "no duplicated descriptors on the standard fixture: {groups:?}"
    );
}

// _(Duplicated descriptors are disclosed — collision-vs-duplicate boundary)_ — two DISTINCT
// descriptors whose canonical base projections collide (same names, different segment kinds) receive
// `#<rank>` disambiguators from identity projection but are NOT duplicated descriptors: their
// references attribute normally, and `duplicated_groups` must not report them as twins.
#[test]
fn canonical_collision_groups_are_not_reported_as_duplicated_descriptors() {
    // `m::f` as a Term and `m::f` as a Method: distinct descriptors, identical base projection.
    let source = "const f: u8 = 1;\nfn f() {}\n";
    let const_pos = source.find('f').unwrap();
    let (cl, cc) = line_col(source, const_pos);
    let fn_pos = source
        .match_indices('f')
        .find(|(i, _)| source[..*i].ends_with("fn "))
        .unwrap()
        .0;
    let (fl, fc) = line_col(source, fn_pos);

    let as_term = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "c",
            vec![
                DescriptorSegment::new("m", SegmentKind::Module),
                DescriptorSegment::new("f", SegmentKind::Term),
            ],
        )),
        kind: SymbolKind::Constant,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "m.rs".to_string(),
            range: SourceRange::new(cl, cc, cl, cc + 1),
            role: OccurrenceRole::Definition,
        }],
    };
    let as_method = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "c",
            vec![
                DescriptorSegment::new("m", SegmentKind::Module),
                DescriptorSegment::new("f", SegmentKind::Method),
            ],
        )),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "m.rs".to_string(),
            range: SourceRange::new(fl, fc, fl, fc + 1),
            role: OccurrenceRole::Definition,
        }],
    };

    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    ingest(
        &mut store,
        &ws(),
        &one_doc_index("m.rs", vec![as_term, as_method]),
        &src,
    )
    .unwrap();

    // Sanity: the pair really collided in projection (both carry a disambiguator) — otherwise this
    // test would be vacuous.
    let members = store.symbols_by_shortname("f").unwrap();
    assert_eq!(members.len(), 2, "both colliding symbols persisted: {members:?}");
    assert!(
        members.iter().all(|m| m.canonical_id.as_str().contains('#')),
        "the canonical collision was disambiguated: {members:?}"
    );

    // The contract: canonical-collision groups are not duplicated descriptors.
    let groups = store.duplicated_groups().unwrap();
    assert!(
        groups.is_empty(),
        "a canonical-collision pair must not be reported as a duplicated-descriptor group: {groups:?}"
    );
}

// ---------------------------------------------------------------------------
// Python (fixture-level): the guarded join and derived edges over the committed
// python-conformance fixture — no live tool.
// ---------------------------------------------------------------------------

fn py_ws() -> WorkspaceId {
    WorkspaceId::new("py-ws")
}

fn py_id(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    silent_cartographer::identity::project_one(&py_ws(), &Descriptor::new("python-conformance", segs))
}

fn py_widget_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("Widget", SegmentKind::Type)])
}

fn py_base_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("Base", SegmentKind::Type)])
}

fn py_mixin_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("Mixin", SegmentKind::Type)])
}

fn py_gadget_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("Gadget", SegmentKind::Type)])
}

fn py_consumer_module_id() -> CanonicalId {
    py_id(&[("pkg.consumer", SegmentKind::Module), ("__init__", SegmentKind::Meta)])
}

fn py_shapes_module_id() -> CanonicalId {
    py_id(&[("pkg.shapes", SegmentKind::Module), ("__init__", SegmentKind::Meta)])
}

fn ingest_python_fixture() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::python_fixture_index();
    let sources = support::python_fixture_sources();
    ingest(&mut store, &py_ws(), &index, &sources).unwrap();
    store
}

// _(Scenario: Python name token accepted under the default rule)_ — a Python occurrence whose
// location spells the symbol's own name aligns, and its rule provenance is the default rule.
#[test]
fn python_name_token_aligns_under_default_rule() {
    let store = ingest_python_fixture();
    let occs = store.occurrences_of(&py_widget_id()).unwrap();
    assert!(!occs.is_empty(), "Widget occurrences aligned");
    assert!(
        occs.iter().any(|o| o.role == "definition"),
        "the definition site aligns: {occs:?}"
    );
    assert!(
        occs.iter().any(|o| o.role == "reference"),
        "reference sites align: {occs:?}"
    );
    for occ in &occs {
        assert_eq!(occ.rule, "exact", "every acceptance carries the default rule: {occ:?}");
    }
}

// _(Scenario: Python occurrence outside every rule stays refused)_ — the consumer module's own
// zero-width definition marker (scip-python's document-origin marker, no identifier at that span)
// satisfies neither the default rule (no name token to compare) nor the module-name rule (the
// structural name-node gate finds nothing there); it lands in a typed discrepancy, never an aligned
// attribution.
#[test]
fn python_occurrence_outside_every_rule_stays_refused() {
    let store = ingest_python_fixture();

    // Not aligned: the consumer module symbol has no aligned occurrence anywhere.
    let module_occs = store.occurrences_of(&py_consumer_module_id()).unwrap();
    assert!(
        module_occs.is_empty(),
        "module occurrences stay refused: {module_occs:?}"
    );

    // Surfaced: the refusal is a typed discrepancy at the zero-width definition marker.
    let rows = store.all_discrepancies().unwrap();
    let refused = rows
        .iter()
        .find(|r| r.document_path == "pkg/consumer.py" && r.expected_name == "__init__" && r.span == Some((0, 0)))
        .unwrap_or_else(|| panic!("zero-width module definition marker surfaces as a discrepancy: {rows:?}"));
    assert!(
        refused.outcome == "text_mismatch" || refused.outcome == "semantic_only",
        "typed refusal outcome: {refused:?}"
    );
}

// _(Scenario: Python import produces an imports edge)_ — a module-scope reference occurrence (the
// `from pkg.shapes import Widget` line) is attributed to the importing module itself and yields an
// `imports` edge from that module to the symbol.
#[test]
fn python_import_produces_imports_edge() {
    let store = ingest_python_fixture();
    let imports = store.edges(EdgeKind::Imports).unwrap();
    assert!(
        imports.contains(&(py_consumer_module_id(), py_widget_id())),
        "imports edge from the importing module to Widget: {imports:?}"
    );
}

// _(Scenario: Python base class produces an edge)_ — the base-name token in the class definition
// header resolves through the aligned occurrence at exactly that location (as Rust impl headers
// do), yielding a `type_hierarchy` edge from the subclass to its base.
#[test]
fn python_base_class_produces_type_hierarchy_edge() {
    let store = ingest_python_fixture();
    let edges = store.edges(EdgeKind::TypeHierarchy).unwrap();
    assert!(
        edges.contains(&(py_widget_id(), py_base_id())),
        "Widget(Base) yields Widget → Base: {edges:?}"
    );
}

// _(Scenario: Multiple bases each produce an edge)_ — `class Gadget(Base, Mixin)` yields one edge
// per declared base; and the whole fixture yields exactly one edge per declared base across the
// file (skip-never-guess: nothing fabricated beyond the declared bases).
#[test]
fn python_multiple_bases_produce_one_edge_each() {
    let store = ingest_python_fixture();
    let mut edges = store.edges(EdgeKind::TypeHierarchy).unwrap();
    edges.sort();
    let mut expected = vec![
        (py_widget_id(), py_base_id()),
        (py_gadget_id(), py_base_id()),
        (py_gadget_id(), py_mixin_id()),
    ];
    expected.sort();
    assert_eq!(edges, expected, "exactly one edge per declared base");
}

// _(Scenario: Python module reference accepted under the module-name rule)_ — the `from pkg.shapes
// import Widget` line's module token (`shapes`, the terminal component of `pkg.shapes`) aligns under
// the module-name rule, with that rule as its persisted provenance.
#[test]
fn python_module_reference_aligns_under_module_name_rule() {
    let store = ingest_python_fixture();
    let occs = store.occurrences_of(&py_shapes_module_id()).unwrap();
    assert!(
        occs.iter().any(|o| o.role == "reference" && o.rule == "module_name"),
        "the import-site module reference aligns under the module-name rule: {occs:?}"
    );
}

// _(Module kind classification)_ — every `__init__`-terminal symbol in the committed fixture index
// carries `SymbolKind::Module`, and no class/function/parameter symbol does: classification keys off
// kind everywhere downstream, not scip-python's naming convention.
#[test]
fn python_module_symbols_classify_as_module_kind() {
    let index = support::python_fixture_index();
    for symbol in &index.symbols {
        let is_init_terminal = symbol
            .descriptor
            .as_ref()
            .and_then(|d| d.segments.last())
            .is_some_and(|seg| seg.name == "__init__");
        if is_init_terminal {
            assert_eq!(
                symbol.kind,
                SymbolKind::Module,
                "__init__-terminal symbol classifies as module kind: {symbol:?}"
            );
        } else {
            assert_ne!(
                symbol.kind,
                SymbolKind::Module,
                "non-__init__-terminal symbol does not classify as module kind: {symbol:?}"
            );
        }
    }
}

// _(Scenario: Python module import produces a module-to-module edge)_ — `from pkg import shapes`
// names another module by its terminal component; the aligned module-name occurrence is attributed
// to the importing module itself, yielding an `imports` edge from `pkg.consumer` to `pkg.shapes`.
#[test]
fn python_module_import_produces_module_to_module_edge() {
    let store = ingest_python_fixture();
    let imports = store.edges(EdgeKind::Imports).unwrap();
    assert!(
        imports.contains(&(py_consumer_module_id(), py_shapes_module_id())),
        "imports edge from the importing module to the imported module: {imports:?}"
    );
}

/// A synthetic Python-language index: one symbol, `provenance.analyzer_name` set to the Python
/// adapter's name so `ingest` selects `Language::Python` for the join.
fn py_synthetic_index(symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
    ExtractedIndex {
        provenance: AnalyzerProvenance {
            analyzer_name: silent_cartographer::semantic::python_adapter::PythonAdapter::analyzer_name().to_string(),
            analyzer_version: "0".to_string(),
        },
        documents: vec![SourceDocument {
            path: "m.py".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

fn py_occ(document_path: &str, line: u32, start: u32, end: u32, role: OccurrenceRole) -> ExtractedOccurrence {
    ExtractedOccurrence {
        document_path: document_path.to_string(),
        range: SourceRange::new(line, start, line, end),
        role,
    }
}

// _(Scenario: Nested module accepted at trailing component-runs only — bare-terminal instance)_ —
// in one build, a reference occurrence of a two-component module (`pkg.shapes`) at a token spelling
// the bare terminal component (`shapes`) aligns under the module-name rule while an occurrence of
// the same module at a leading-component token (`pkg`) is refused. The full-dotted-path acceptance
// and the standalone prefix refusal are pinned by `python_module_dotted_path_aligns_under_module_name_rule`
// and `python_module_prefix_token_stays_refused`.
#[test]
fn nested_module_bare_terminal_aligns_and_prefix_refused() {
    let source = "shapes\npkg\n";
    let module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg.shapes", SegmentKind::Module),
                DescriptorSegment::new("__init__", SegmentKind::Meta),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            // "shapes" at line 0, cols 0..6 — the terminal component.
            py_occ("m.py", 0, 0, 6, OccurrenceRole::Reference),
            // "pkg" at line 1, cols 0..3 — a non-terminal component.
            py_occ("m.py", 1, 0, 3, OccurrenceRole::Reference),
        ],
    };
    let index = py_synthetic_index(vec![module]);
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_name, 1,
        "exactly the terminal-component token aligns"
    );
    assert_eq!(acc.text_mismatch, 1, "the non-terminal-component token is refused");
}

// A module-kind symbol whose descriptor lacks the `__init__`/meta terminal is outside the
// module-name rule: the rule reconciles exactly the shape `classify_module_kinds` classifies on,
// and any other descriptor shape refuses rather than reading a component from the wrong segment.
#[test]
fn module_kind_without_init_terminal_is_outside_module_name_rule() {
    let source = "pkg\n";
    let module = ExtractedSymbol {
        // Two plain segments, no `__init__` terminal: were the rule to read the second-to-last
        // segment without checking the terminal shape, the `pkg` token would misalign here.
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg", SegmentKind::Module),
                DescriptorSegment::new("shapes", SegmentKind::Module),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        // "pkg" at line 0, cols 0..3 — spells neither "shapes" (the default rule's expectation)
        // nor any admissible module-name component (the descriptor lacks the module shape).
        occurrences: vec![py_occ("m.py", 0, 0, 3, OccurrenceRole::Reference)],
    };
    let index = py_synthetic_index(vec![module]);
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_name, 0,
        "the module-name rule refuses a descriptor without the __init__ terminal"
    );
    assert_eq!(acc.aligned_total(), 0, "no rule accepts the occurrence");
    assert_eq!(acc.text_mismatch, 1, "the occurrence is refused");
}

// _(Scenario: Non-module occurrence is outside the module-name rule)_ — a class-kind occurrence at a
// non-matching token is refused, not rescued by the module-name rule (which is gated on module kind).
#[test]
fn non_module_occurrence_is_outside_module_name_rule() {
    let source = "shapes\n";
    let class_symbol = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![DescriptorSegment::new("Widget", SegmentKind::Type)],
        )),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        // Points at "shapes", which spells neither "Widget" (the default rule) nor any module-name
        // component (the symbol is not module-kind).
        occurrences: vec![py_occ("m.py", 0, 0, 6, OccurrenceRole::Reference)],
    };
    let index = py_synthetic_index(vec![class_symbol]);
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_name, 0,
        "the module-name rule never fires for a non-module symbol"
    );
    assert_eq!(acc.aligned_total(), 0, "no rule accepts the mismatched token");
    assert_eq!(acc.text_mismatch, 1, "the occurrence is refused");
}

// _(Kind-scoped rules are language-gated)_ — the four Rust rules never evaluate for a Python
// document. The pinned leak: a zero-width Python module marker on an EMPTY document vacuously spans
// the whole (empty) document, which the un-gated module-span rule accepted; gated, it refuses.
#[test]
fn rust_kind_scoped_rules_do_not_fire_for_python() {
    let source = "";
    let module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg", SegmentKind::Module),
                DescriptorSegment::new("__init__", SegmentKind::Meta),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        // scip-python's zero-width definition marker at the origin of an empty __init__.py.
        occurrences: vec![py_occ("pkg/__init__.py", 0, 0, 0, OccurrenceRole::Definition)],
    };
    let mut index = py_synthetic_index(vec![module]);
    index.documents[0].path = "pkg/__init__.py".to_string();
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("pkg/__init__.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_span, 0,
        "the Rust module-span rule never accepts a Python occurrence"
    );
    assert_eq!(acc.aligned_total(), 0, "the zero-width marker aligns under no rule");
    assert_eq!(
        acc.text_mismatch + acc.semantic_only,
        1,
        "the marker is refused with a typed outcome"
    );
}

// _(Scenario: Relative-import module reference accepted)_ — a module occurrence whose span covers a
// relative-import token (leading dots plus a trailing component-run, `.shapes` for module
// `pkg.shapes`) aligns under the module-name rule.
#[test]
fn python_module_relative_import_aligns_under_module_name_rule() {
    let source = "from .shapes import Widget\n";
    let module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg.shapes", SegmentKind::Module),
                DescriptorSegment::new("__init__", SegmentKind::Meta),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        // The `.shapes` token: bytes 5..12 on line 0 (the dot plus the identifier — no single
        // identifier node contains this span, so the raw-span-text gate carries it).
        occurrences: vec![py_occ("m.py", 0, 5, 12, OccurrenceRole::Reference)],
    };
    let index = py_synthetic_index(vec![module]);
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_name, 1,
        "the relative-import token aligns under the module-name rule"
    );
    assert_eq!(acc.text_mismatch, 0, "nothing refused");
}

// _(Scenario: Nested module accepted at trailing component-runs only — full-dotted-path half)_ —
// the fixture's `from pkg.shapes import Widget` line carries a module occurrence spanning the full
// dotted path `pkg.shapes`; it aligns under the module-name rule.
#[test]
fn python_module_dotted_path_aligns_under_module_name_rule() {
    let store = ingest_python_fixture();
    let occs = store.occurrences_of(&py_shapes_module_id()).unwrap();
    let dotted = occs
        .iter()
        .find(|o| o.document_path == "pkg/consumer.py" && o.span == (5, 15))
        .unwrap_or_else(|| panic!("the full-dotted-path import occurrence aligns: {occs:?}"));
    assert_eq!(dotted.role, "reference");
    assert_eq!(
        dotted.rule, "module_name",
        "the dotted-path acceptance carries the module-name rule: {dotted:?}"
    );
}

// _(Scenario: Nested module accepted at trailing component-runs only — refusal half)_ — a token
// spelling only a leading component of the module's dotted name (`pkg` for module `pkg.shapes`)
// breaks the trailing-run condition and is refused.
#[test]
fn python_module_prefix_token_stays_refused() {
    let source = "pkg\n";
    let module = ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "synthetic",
            vec![
                DescriptorSegment::new("pkg.shapes", SegmentKind::Module),
                DescriptorSegment::new("__init__", SegmentKind::Meta),
            ],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        // "pkg" at line 0, cols 0..3 — a leading component, not a trailing run.
        occurrences: vec![py_occ("m.py", 0, 0, 3, OccurrenceRole::Reference)],
    };
    let index = py_synthetic_index(vec![module]);
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.py".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();

    assert_eq!(
        acc.aligned_module_name, 0,
        "a leading-component token is not evidence for the module"
    );
    assert_eq!(acc.aligned_total(), 0, "no rule accepts the prefix token");
    assert_eq!(acc.text_mismatch, 1, "the occurrence is refused");
}
