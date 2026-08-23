//! Build-side semantic-corpus tests: corpus membership and content through a real build, the
//! persisted vector and lexical representations, carry-forward across rebuilds, clone-equivalence
//! keys, failed-build supersession, and semantic-index provenance.

mod support;

use silent_cartographer::graph::chunk::ChunkParams;
use silent_cartographer::graph::corpus::CORPUS_DEFINITION_VERSION;
use silent_cartographer::graph::embed::MODEL_ID;
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::graph::{ingest, ingest_with_params};
use silent_cartographer::identity::{CanonicalId, Descriptor, DescriptorSegment, SegmentKind};

use crate::support::{DOC, SOURCE, fixture_index, sources, ws};

const WS_ROOT: &str = "/test-ws";

fn ingest_fixture() -> GraphStore {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    store
}

fn id_of(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    silent_cartographer::identity::project_one(&ws(), &Descriptor::new("mycrate", segs))
}

fn module_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module)])
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

fn disconnect_id() -> CanonicalId {
    id_of(&[
        ("net", SegmentKind::Module),
        ("Client", SegmentKind::Type),
        ("disconnect", SegmentKind::Method),
    ])
}

fn open_id() -> CanonicalId {
    id_of(&[("net", SegmentKind::Module), ("open", SegmentKind::Method)])
}

// _(Every passage is represented)_ — after a build, each in-workspace symbol with persisted tier
// content contributes exactly one passage, and each passage carries at least one vector
// representation (256 little-endian f32s each) and a lexical row its words are retrievable through.
#[test]
fn every_passage_carries_both_representations() {
    let store = ingest_fixture();
    let reps = store.semantic_representations().unwrap();
    let expected = [module_id(), client_id(), connect_id(), disconnect_id(), open_id()];
    assert_eq!(reps.len(), expected.len());
    for id in &expected {
        let rep = reps
            .get(id.as_str())
            .unwrap_or_else(|| panic!("{} contributes a passage", id.as_str()));
        assert!(!rep.render.is_empty());
        assert!(!rep.embeddings.is_empty(), "at least one chunk vector per passage");
        for embedding in &rep.embeddings {
            assert_eq!(embedding.len(), 256 * 4, "one f32 vector of the model's dimension");
        }
    }
    assert_eq!(store.corpus_size().unwrap(), expected.len() as u64);
    // The lexical representation is retrievable through an FTS match on a render word.
    let hits = store.lexical_neighbors("\"disconnect\"", 10).unwrap();
    assert!(
        hits.iter().any(|(id, _)| *id == disconnect_id()),
        "the lexical signal indexes the passage's words"
    );
}

// _(Leaf own-content, container interface-only, module excludes member bodies)_ — through a real
// build: a leaf method's entry carries its own source; the containing type's and module's entries
// derive from their interface tiers, so no entry carries member bodies through a container.
#[test]
fn corpus_content_splits_leaf_and_container() {
    let store = ingest_fixture();
    let reps = store.semantic_representations().unwrap();
    let connect = &reps.get(connect_id().as_str()).unwrap().render;
    assert!(connect.contains("pub fn connect(&self) {}"));
    let client = &reps.get(client_id().as_str()).unwrap().render;
    assert!(client.contains("pub struct Client"));
    assert!(
        !client.contains("pub fn connect"),
        "no member body rides through the type"
    );
    let module = &reps.get(module_id().as_str()).unwrap().render;
    assert!(
        !module.contains("pub fn open"),
        "no member body rides through the module"
    );
    assert!(!module.contains("connect(&c)"));
}

// _(Rebuild over unchanged sources is idempotent)_ — a second build over identical sources yields
// byte-identical representations. The second build necessarily runs through the carry-forward path:
// every render is unchanged, so every vector is carried rather than re-embedded, and the contract
// makes that unobservable.
#[test]
fn rebuild_over_unchanged_sources_is_idempotent_through_carry_forward() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    let first = store.semantic_representations().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    let second = store.semantic_representations().unwrap();
    assert_eq!(first, second);
}

// _(An edited symbol is re-represented; the skip is bypassed)_ — editing one leaf's body re-derives
// that entry from the new content in the same build where an unchanged sibling's representation is
// carried forward byte-identically.
#[test]
fn edited_symbol_reembeds_while_unchanged_sibling_carries_forward() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    let first = store.semantic_representations().unwrap();
    let open_keys_before = store.clone_keys_of(&open_id()).unwrap().expect("a leaf carries keys");

    // Edit `open`'s body on a line carrying no fixture occurrence, leaving every occurrence range
    // valid: the trailing expression `c` gains a method call.
    let edited = SOURCE.replace("\n        c\n", "\n        c.reconnect_timeout()\n");
    assert_ne!(edited, SOURCE, "the edit applies");
    let edited_sources = vec![(DOC.to_string(), edited)];
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &edited_sources).unwrap();
    let second = store.semantic_representations().unwrap();

    // The persisted clone keys are the new build's: the edited source spells a different token
    // sequence, so both keys change — the prior build's keys are wholly superseded.
    let open_keys_after = store.clone_keys_of(&open_id()).unwrap().expect("a leaf carries keys");
    assert_ne!(open_keys_before, open_keys_after, "keys follow the build");

    let open_before = first.get(open_id().as_str()).unwrap();
    let open_after = second.get(open_id().as_str()).unwrap();
    assert!(
        open_after.render.contains("reconnect_timeout"),
        "derived from the new content"
    );
    assert_ne!(
        open_before.embeddings, open_after.embeddings,
        "re-embedded, not carried"
    );

    let connect_before = first.get(connect_id().as_str()).unwrap();
    let connect_after = second.get(connect_id().as_str()).unwrap();
    assert_eq!(
        connect_before, connect_after,
        "the unchanged sibling is carried forward"
    );
}

// _(Representations of vanished symbols do not linger)_ — rebuilding from sources that no longer
// contain a symbol leaves no representation for it retrievable.
#[test]
fn vanished_symbol_representations_do_not_linger() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    assert!(
        store
            .semantic_representations()
            .unwrap()
            .contains_key(open_id().as_str())
    );

    let mut without_open = fixture_index();
    without_open.symbols.retain(|sym| {
        sym.descriptor
            .as_ref()
            .is_none_or(|d| d.terminal_name() != Some("open"))
    });
    ingest(&mut store, &ws(), Some(WS_ROOT), &without_open, &sources()).unwrap();
    let reps = store.semantic_representations().unwrap();
    assert!(
        !reps.contains_key(open_id().as_str()),
        "the vanished symbol left nothing"
    );
    assert!(reps.contains_key(connect_id().as_str()), "surviving symbols remain");
}

// _(A build that fails mid-way leaves the prior build's representations intact)_ — the semantic
// tables live inside the build transaction: a transaction abandoned after clearing rolls back, and
// the prior build's representations stay authoritative.
//
// The failure is simulated by dropping the transaction guard rather than injected through
// `ingest`, deliberately: after the clear, every fallible call in `ingest` is a store write no
// public input can make fail on a healthy store, and dropping the guard is byte-for-byte what
// every `?` inside `ingest` does — the rollback-on-drop the contract rides on.
#[test]
fn failed_build_leaves_prior_representations_authoritative() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    let before = store.semantic_representations().unwrap();
    assert!(!before.is_empty());

    {
        let tx = store.begin_build().unwrap();
        store.clear_derived().unwrap();
        assert!(store.semantic_representations().unwrap().is_empty());
        drop(tx); // The build fails: no commit.
    }

    assert_eq!(store.semantic_representations().unwrap(), before);
}

// _(Identity recorded and retrievable; defaults apply when unsupplied)_ — after a build, the
// store's provenance carries the embedding model identity, the corpus definition version, and the
// chunk parameters that produced the representations; a build run without explicit parameters
// records the recommended defaults.
#[test]
fn semantic_index_identity_is_recorded() {
    let store = ingest_fixture();
    let identity = store
        .semantic_index_identity()
        .unwrap()
        .expect("a build records identity");
    assert_eq!(identity.model_identity, MODEL_ID);
    assert_eq!(identity.corpus_definition_version, CORPUS_DEFINITION_VERSION);
    assert_eq!(
        identity.chunk_params,
        ChunkParams::default(),
        "an unparameterized build records the recommended defaults"
    );
}

// _(A name-only symbol contributes nothing)_ — a parameter-like symbol, whose name span matches no
// declaration so every tier degrades to its bare name token, yields no passage and no clone
// keys, while the real declaration beside it still contributes.
#[test]
fn a_name_only_symbol_contributes_no_passage_and_no_keys() {
    use silent_cartographer::semantic::model::{OccurrenceRole, SourceRange, SymbolClass, SymbolKind};

    use crate::support::{id_of_pkg, line_col, one_doc_index, one_occ_symbol};

    let doc = "shout.rs";
    let source = "pub fn shout(word: &str) -> String {\n    word.to_uppercase()\n}\n";
    let sym = |name: &str, kind, offset: usize| {
        let (line, col) = line_col(source, offset);
        one_occ_symbol(
            "paramcrate",
            &[(name, silent_cartographer::identity::SegmentKind::Method)],
            kind,
            SymbolClass::InWorkspace,
            doc,
            SourceRange::new(line, col, line, col + name.len() as u32),
            OccurrenceRole::Definition,
        )
    };
    let shout = sym("shout", SymbolKind::Function, source.find("shout").unwrap());
    // The parameter token inside the signature: no declaration has this name span, so its content
    // degrades to the bare name.
    let word = sym("word", SymbolKind::Other, source.find("word").unwrap());
    let index = one_doc_index(doc, vec![shout, word]);
    let srcs = vec![(doc.to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();

    let seg = silent_cartographer::identity::SegmentKind::Method;
    let shout_id = id_of_pkg("paramcrate", &[("shout", seg)]);
    let word_id = id_of_pkg("paramcrate", &[("word", seg)]);
    let reps = store.semantic_representations().unwrap();
    let shout = reps.get(shout_id.as_str()).expect("the real declaration contributes");
    assert!(
        shout.render.contains("to_uppercase"),
        "the enclosing function stays a leaf: containment over a name-only symbol does not evict its body"
    );
    assert!(
        !reps.contains_key(word_id.as_str()),
        "the name-only symbol contributes nothing"
    );
    assert!(store.clone_keys_of(&shout_id).unwrap().is_some());
    assert_eq!(
        store.clone_keys_of(&word_id).unwrap(),
        None,
        "no keys for a name-only symbol"
    );
}

// _(Leaves carry clone keys; a container carries none)_ — the fixture's methods are leaves and
// carry both keys; the containing type and module carry none. `connect` and `disconnect` are
// consistent renamings of each other, so they share the substitution-insensitive key while their
// formatting-insensitive keys differ.
#[test]
fn clone_keys_cover_leaves_and_never_containers() {
    let store = ingest_fixture();
    let connect = store
        .clone_keys_of(&connect_id())
        .unwrap()
        .expect("a leaf carries keys");
    let disconnect = store
        .clone_keys_of(&disconnect_id())
        .unwrap()
        .expect("a leaf carries keys");
    assert_ne!(
        connect.0, disconnect.0,
        "different spellings, different formatting keys"
    );
    assert_eq!(
        connect.1, disconnect.1,
        "a consistent renaming shares the substitution key"
    );
    assert_eq!(
        store.clone_keys_of(&client_id()).unwrap(),
        None,
        "a type container carries no key"
    );
    assert_eq!(
        store.clone_keys_of(&module_id()).unwrap(),
        None,
        "a module carries no key"
    );
}

// _(Container contributes its interface only — equal-span boundary)_ — a file module whose span
// exactly equals its sole declaration's span (a file with no trailing newline) is still the
// container: its entry derives from its interface tier, never the whole document, and the
// declaration stays a leaf carrying its own body.
#[test]
fn an_equal_span_module_still_contributes_interface_only() {
    use silent_cartographer::identity::SegmentKind;
    use silent_cartographer::semantic::model::{OccurrenceRole, SourceRange, SymbolClass, SymbolKind};

    use crate::support::{id_of_pkg, line_col, one_doc_index, one_occ_symbol};

    // No trailing newline: the function's full span is the whole document, equal to the module's.
    let source = "fn only_child() { leak_marker_body() }";
    let doc = "solo_mod.rs";
    let len = source.len() as u32;
    let (l, c) = line_col(source, source.find("only_child").unwrap());
    let module = one_occ_symbol(
        "probecrate",
        &[("solo_mod", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(0, 0, 0, len),
        OccurrenceRole::Definition,
    );
    let child = one_occ_symbol(
        "probecrate",
        &[("solo_mod", SegmentKind::Module), ("only_child", SegmentKind::Method)],
        SymbolKind::Function,
        SymbolClass::InWorkspace,
        doc,
        SourceRange::new(l, c, l, c + 10),
        OccurrenceRole::Definition,
    );
    let index = one_doc_index(doc, vec![module, child]);
    let srcs = vec![(doc.to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();

    let reps = store.semantic_representations().unwrap();
    let module_render = &reps
        .get(id_of_pkg("probecrate", &[("solo_mod", SegmentKind::Module)]).as_str())
        .expect("the module contributes a passage")
        .render;
    assert!(
        !module_render.contains("leak_marker_body"),
        "the equal-span module contributes its interface, not the document: {module_render}"
    );
    let child_render = &reps
        .get(
            id_of_pkg(
                "probecrate",
                &[("solo_mod", SegmentKind::Module), ("only_child", SegmentKind::Method)],
            )
            .as_str(),
        )
        .expect("the declaration contributes a passage")
        .render;
    assert!(
        child_render.contains("leak_marker_body"),
        "the declaration stays a leaf carrying its own body: {child_render}"
    );
}

/// A chunk size small enough that the fixture's `open` function splits into several chunks, yet
/// large enough that its identity head fits the header budget untrimmed.
fn small_params() -> ChunkParams {
    ChunkParams {
        chunk_size: 32,
        overlap: 0,
    }
}

// _(Parameters recorded with the build)_ — a build run with explicit chunk parameters records the
// supplied values in the semantic-index identity, and its vectors derive from them: under a size
// the long leaf exceeds, that passage is represented by several chunks.
#[test]
fn explicit_parameters_are_recorded_and_shape_the_vectors() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest_with_params(
        &mut store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();

    let identity = store.semantic_index_identity().unwrap().expect("identity recorded");
    assert_eq!(
        identity.chunk_params,
        small_params(),
        "the supplied values are recorded"
    );
    let open_vectors = store.chunk_vectors_of(&open_id()).unwrap();
    assert!(open_vectors.len() > 1, "the long leaf splits under the small size");
    assert!(
        store.chunk_count().unwrap() > store.corpus_size().unwrap(),
        "the vector table holds more chunks than passages"
    );
}

// _(Changed parameters re-derive every vector)_ — rebuilding the same sources at a different chunk
// size yields exactly the representations a fresh build at that size yields: no vector from the
// prior regime survives, observable because a fresh store never held one.
#[test]
fn a_changed_chunk_size_rederives_every_vector() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &fixture_index(), &sources()).unwrap();
    let before = store.semantic_representations().unwrap();
    ingest_with_params(
        &mut store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();
    let after = store.semantic_representations().unwrap();

    let mut fresh_store = GraphStore::open_in_memory().unwrap();
    ingest_with_params(
        &mut fresh_store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();
    let fresh = fresh_store.semantic_representations().unwrap();

    assert_eq!(after, fresh, "the rebuild equals a from-scratch build at the new size");
    let open_after = after.get(open_id().as_str()).unwrap();
    let open_before = before.get(open_id().as_str()).unwrap();
    assert!(
        open_after.embeddings.len() > 1,
        "the long leaf splits under the new size"
    );
    assert_ne!(
        open_before.embeddings, open_after.embeddings,
        "the passage's vectors derive from the new chunk size"
    );
}

// _(Rebuild over unchanged sources is idempotent — explicit-parameter arm)_ — a second build under
// the same explicit parameters yields byte-identical representations through carry-forward.
#[test]
fn rebuild_under_unchanged_explicit_parameters_is_identical() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest_with_params(
        &mut store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();
    let first = store.semantic_representations().unwrap();
    ingest_with_params(
        &mut store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();
    let second = store.semantic_representations().unwrap();
    assert_eq!(first, second);
}

// _(Every chunk carries its passage's header)_ — the persisted vectors of a split passage are
// exactly the embeddings of the header-carrying chunks the splitter derives from the same tiers,
// and each of those chunks opens with the passage's identity head.
#[test]
fn persisted_chunk_vectors_match_the_header_carrying_chunks() {
    use silent_cartographer::graph::chunk::{PassageHeader, split_passage};
    use silent_cartographer::graph::corpus::{CorpusSource, assemble};
    use silent_cartographer::graph::embed::{embed, vector_bytes};
    use silent_cartographer::graph::range::ByteSpan;
    use silent_cartographer::graph::syntax::{Language, SyntaxTree};

    let mut store = GraphStore::open_in_memory().unwrap();
    ingest_with_params(
        &mut store,
        &ws(),
        Some(WS_ROOT),
        &fixture_index(),
        &sources(),
        &small_params(),
    )
    .unwrap();

    // Recompute the passage from the persisted tiers, exactly as the build assembles it.
    let row = store.symbol(&open_id()).unwrap().expect("the leaf is persisted");
    let source = CorpusSource {
        canonical_id: &row.canonical_id,
        display_name: &row.display_name,
        kind: &row.kind,
        document_path: row.document_path.as_deref(),
        span: row.span.map(|(start, end)| ByteSpan { start, end }),
        signature_text: row.signature_text.as_deref(),
        interface_text: row.interface_text.as_deref(),
        span_text: row.span_text.as_deref(),
        contains_persisted: false,
    };
    let passages = assemble(&[source]);
    let passage = &passages[0];
    let tree = SyntaxTree::parse(SOURCE, Language::Rust).unwrap();
    let (_, span) = passage
        .content_location
        .clone()
        .expect("a leaf's content has a location");
    let header = PassageHeader {
        identity: &passage.header_identity,
        documentation: passage.documentation.as_deref(),
    };
    let chunks = split_passage(&header, &passage.content, Some((&tree, span)), &small_params());

    let stored = store.chunk_vectors_of(&open_id()).unwrap();
    assert!(chunks.len() > 1, "the passage splits");
    assert_eq!(stored.len(), chunks.len(), "one persisted vector per chunk");
    for (chunk, stored_bytes) in chunks.iter().zip(&stored) {
        assert!(
            chunk.text.starts_with(&passage.header_identity),
            "every chunk opens with the passage's identity head"
        );
        assert!(
            silent_cartographer::graph::embed::count_tokens(&chunk.text) <= small_params().chunk_size,
            "no chunk the build embeds exceeds the supplied size, header included"
        );
        assert_eq!(
            *stored_bytes,
            vector_bytes(&embed(&chunk.text)),
            "the persisted vector is the embedding of the header-carrying chunk"
        );
    }
}
