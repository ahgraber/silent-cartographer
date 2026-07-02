//! Code-graph tests: the guarded positional join, lossless persistence, enclosure, staleness,
//! reference attribution, and join-alignment accounting.

mod support;

use silent_cartographer::graph::store::{Freshness, GraphStore, PersistedClass};
use silent_cartographer::graph::{content_hash, freshness, ingest, join_guarded};
use silent_cartographer::identity::{CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
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
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let accounting = ingest(&mut store, &ws(), &index, &src).unwrap();
    assert_eq!(accounting.text_mismatch, 1, "mismatch should be counted");
    assert_eq!(accounting.aligned, 0, "mismatched occurrence must not align");

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
    };
    let mut store2 = GraphStore::open_in_memory().unwrap();
    let src2 = vec![("n.rs".to_string(), nonascii.to_string())];
    let acc2 = ingest(&mut store2, &ws(), &index2, &src2).unwrap();
    assert_eq!(
        acc2.text_mismatch, 1,
        "non-ASCII drift is a surfaced mismatch, not aligned"
    );
    assert_eq!(acc2.aligned, 0);
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
    };
    let mut store = GraphStore::open_in_memory().unwrap();
    let src = vec![("m.rs".to_string(), source.to_string())];
    let acc = ingest(&mut store, &ws(), &index, &src).unwrap();
    assert_eq!(acc.semantic_only, 1, "occurrence with no syntax is semantic-only");
    assert_eq!(acc.aligned, 0);
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

// _(Staleness reflects underlying change — fresh branch)_
#[test]
fn unchanged_sources_and_analyzer_are_fresh() {
    let store = ingest_fixture();
    let f = freshness(&store, &sources(), &support::provenance()).unwrap();
    assert_eq!(f, Some(Freshness::Fresh));
}

// _(Staleness reflects underlying change — content write-site)_
#[test]
fn changed_content_marks_stale() {
    let store = ingest_fixture();
    let changed = vec![(support::DOC.to_string(), format!("{}\n// edit\n", support::SOURCE))];
    let f = freshness(&store, &changed, &support::provenance()).unwrap();
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
    let f = freshness(&store, &sources(), &newer).unwrap();
    assert_eq!(f, Some(Freshness::StaleVersion));
    assert!(f.unwrap().is_stale(), "version drift flags reindex");
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
    assert!(acc.aligned > 0);
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
    assert!(meta.accounting.aligned > 0, "mixed fixture has aligned occurrences");
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
}

// _(Workspace-namespaced identity — persisted path)_ — the same descriptor ingested under two
// workspaces persists two distinct symbols, and queries do not conflate them.
#[test]
fn two_workspace_ingest_persists_distinct_symbols() {
    use silent_cartographer::query::resolve::{Resolution, resolve};

    // One store, two builds of the identical index under distinct workspace identities. Symbols are
    // keyed by canonical identity, which is workspace-namespaced, so both sets coexist.
    let mut store = GraphStore::open_in_memory().unwrap();
    let index = support::fixture_index();
    let src = sources();
    let ws_a = WorkspaceId::new("workspace-a");
    let ws_b = WorkspaceId::new("workspace-b");
    ingest(&mut store, &ws_a, &index, &src).unwrap();
    ingest(&mut store, &ws_b, &index, &src).unwrap();

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

    // Both are persisted as their own rows.
    let row_a = store.symbol(&id_a).unwrap().expect("workspace-a symbol persisted");
    let row_b = store.symbol(&id_b).unwrap().expect("workspace-b symbol persisted");
    assert_ne!(row_a.canonical_id, row_b.canonical_id);

    // Identity-tier resolution round-trips each to exactly its own symbol — never the other's.
    match resolve(&store, id_a.as_str()).unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_a),
        other => panic!("expected unique for identity a, got {other:?}"),
    }
    match resolve(&store, id_b.as_str()).unwrap() {
        Resolution::Unique(row) => assert_eq!(row.canonical_id, id_b),
        other => panic!("expected unique for identity b, got {other:?}"),
    }

    // A qualified name shared across the workspaces resolves to a typed candidate set carrying both
    // identities — surfaced ambiguity, not a silent conflation into one.
    match resolve(&store, "net::Client::connect").unwrap() {
        Resolution::Ambiguous(rows) => {
            let ids: Vec<&str> = rows.iter().map(|r| r.canonical_id.as_str()).collect();
            assert!(ids.contains(&id_a.as_str()), "candidates include workspace-a: {ids:?}");
            assert!(ids.contains(&id_b.as_str()), "candidates include workspace-b: {ids:?}");
        }
        other => panic!("expected ambiguous across workspaces, got {other:?}"),
    }
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
