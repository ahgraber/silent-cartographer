//! `search` and `similar` tests: search-by-meaning over the semantic corpus, similar-code lookup
//! with clone-certainty markers, the estimation labeling every semantic answer carries, and the
//! inherited envelope (bounding, continuation tokens, exit taxonomy) through the new commands.

mod support;

use std::path::Path;

use silent_cartographer::exit::{ExitCode, classify};
use silent_cartographer::graph::embed::MODEL_ID;
use silent_cartographer::graph::ingest;
use silent_cartographer::graph::store::GraphStore;
use silent_cartographer::identity::{CanonicalId, SegmentKind};
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::search::{CloneCertainty, SimilarItem};
use silent_cartographer::query::{Detail, QueryEngine};
use silent_cartographer::semantic::model::{OccurrenceRole, SourceRange, SymbolClass, SymbolKind};

use crate::support::{id_of_pkg, line_col, one_doc_index, one_occ_symbol, provenance, sources, ws};

const WS_ROOT: &str = "/test-ws";

/// An engine over `store` that reads as fresh: the current hash is the hash of `srcs`.
fn engine_over<'a>(store: &'a GraphStore, srcs: &[(String, String)]) -> QueryEngine<'a> {
    let hash = silent_cartographer::graph::content_hash(srcs);
    QueryEngine::new(store, provenance(), hash, None)
}

/// A one-document store over Rust functions, each `(package-qualified name, whole source)` pair
/// derived from `source` by locating each name's first occurrence.
fn store_over(doc: &str, source: &str, names: &[&str]) -> (GraphStore, Vec<(String, String)>) {
    let symbols = names
        .iter()
        .map(|name| {
            let offset = source
                .find(name)
                .unwrap_or_else(|| panic!("{name} appears in the fixture"));
            let (line, col) = line_col(source, offset);
            one_occ_symbol(
                "semcrate",
                &[(name, SegmentKind::Method)],
                SymbolKind::Function,
                SymbolClass::InWorkspace,
                doc,
                SourceRange::new(line, col, line, col + name.len() as u32),
                OccurrenceRole::Definition,
            )
        })
        .collect();
    let index = one_doc_index(doc, symbols);
    let srcs = vec![(doc.to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();
    (store, srcs)
}

fn sem_id(name: &str) -> CanonicalId {
    id_of_pkg("semcrate", &[(name, SegmentKind::Method)])
}

/// The relevance fixture: three documented functions whose names and documentation are disjoint
/// enough that a query aimed at one should rank it first.
const SEARCH_DOC: &str = "search.rs";
const SEARCH_SOURCE: &str = "\
/// Retries a flaky call, sleeping longer between attempts.
pub fn retryWithBackoff(tries: u32) -> u32 {
    tries + 1
}

/// Reads the settings file into a struct.
pub fn parse_config(text: &str) -> u32 {
    text.len() as u32
}

/// Formats a greeting for display.
pub fn greet_user(name: &str) -> u32 {
    name.len() as u32
}
";

fn search_store() -> (GraphStore, Vec<(String, String)>) {
    store_over(
        SEARCH_DOC,
        SEARCH_SOURCE,
        &["retryWithBackoff", "parse_config", "greet_user"],
    )
}

/// The ranked symbol identities of a found search answer.
fn search_order(store: &GraphStore, srcs: &[(String, String)], query: &str, detail: Detail) -> Vec<String> {
    let engine = engine_over(store, srcs);
    let answer = engine.search(query, detail, None).unwrap();
    match answer.outcome {
        Outcome::Found { results } => results
            .into_iter()
            .map(|item| item.symbol.canonical_id.as_str().to_string())
            .collect(),
        other => panic!("expected found results, got {other:?}"),
    }
}

// _(Documentation words match without the name)_ — a query drawn from a function's documentation,
// none of whose words appear in its name, ranks that function first.
#[test]
fn documentation_words_match_without_the_name() {
    let (store, srcs) = search_store();
    let order = search_order(
        &store,
        &srcs,
        "sleeping between attempts on a flaky call",
        Detail::Signature,
    );
    assert_eq!(order[0], sem_id("retryWithBackoff").as_str());
}

// _(Name words match as natural language)_ — a compound name's words, given as separate
// natural-language words, rank the compound-named function first.
#[test]
fn split_name_words_match_as_natural_language() {
    let (store, srcs) = search_store();
    let order = search_order(&store, &srcs, "retry with backoff", Detail::Signature);
    assert_eq!(order[0], sem_id("retryWithBackoff").as_str());
}

// _(Detail does not change the result set)_ — the same query at two detail levels returns the same
// symbols in the same order.
#[test]
fn detail_never_changes_the_result_set_or_order() {
    let (store, srcs) = search_store();
    let at_location = search_order(&store, &srcs, "settings file", Detail::Location);
    let at_body = search_order(&store, &srcs, "settings file", Detail::Body);
    assert_eq!(at_location, at_body);
}

// _(A symbol appears at most once)_ — a function relevant through both its name and its
// documentation appears exactly once in the answer.
#[test]
fn a_symbol_appears_at_most_once() {
    let (store, srcs) = search_store();
    let order = search_order(&store, &srcs, "retry backoff sleeping attempts", Detail::Signature);
    let hits = order
        .iter()
        .filter(|id| **id == sem_id("retryWithBackoff").as_str())
        .count();
    assert_eq!(hits, 1);
}

// _(An empty corpus is typed absence)_ — a built store whose corpus holds no entries answers a
// definite empty, still carrying the estimation marker and the semantic-index provenance.
#[test]
fn an_empty_corpus_is_typed_absence_with_marker_and_provenance() {
    let doc = "empty.rs";
    let index = one_doc_index(doc, Vec::new());
    let srcs = vec![(doc.to_string(), "// nothing declared\n".to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();
    let engine = engine_over(&store, &srcs);
    let answer = engine.search("anything", Detail::Signature, None).unwrap();
    assert!(matches!(answer.outcome, Outcome::Empty));
    assert_eq!(answer.classification, Some("estimation"));
    let semantic = answer.semantic_index.expect("the empty answer carries provenance");
    assert_eq!(semantic.model_identity, MODEL_ID);
}

// _(The marker composes with staleness)_ — an engine told a different current hash grades the
// answer stale, and the estimation marker rides independently beside it.
#[test]
fn the_estimation_marker_composes_with_staleness() {
    let (store, _) = clones_store();
    let engine = QueryEngine::new(&store, provenance(), "a-different-hash".to_string(), None);
    let answer = engine.similar("m1::alpha", Detail::Signature, None).unwrap();
    assert!(answer.stale, "a changed hash grades the answer stale");
    assert_eq!(answer.classification, Some("estimation"));
}

/// The clone fixture: `m1::alpha` with a same-named copy in another module differing only in
/// layout (`m2::alpha` — the declaration's own name is a token, so only a same-named copy can be
/// token-identical), a consistently renamed copy (`gamma`), a literal-substituted copy (`delta`),
/// an edited copy (`epsilon`), and unrelated code (`omega`).
const CLONES_DOC: &str = "clones.rs";
const CLONES_SOURCE: &str = "\
pub fn alpha(x: u32, y: u32) -> u32 {
    let total = x + y;
    total * 3
}

pub fn alpha(x: u32, y: u32) -> u32 { let total = x + y; total * 3 }

pub fn gamma(a: u32, b: u32) -> u32 {
    let sum = a + b;
    sum * 3
}

pub fn delta(x: u32, y: u32) -> u32 {
    let total = x + y;
    total * 5
}

pub fn epsilon(x: u32, y: u32) -> u32 {
    let total = x + y;
    let extra = total + 1;
    extra * 3
}

pub fn omega(text: &str) -> usize {
    text.len()
}
";

/// The clone fixture's symbols: the two same-named `alpha` copies live under distinct module
/// segments so their identities differ while their token sequences agree.
fn clone_symbols() -> Vec<silent_cartographer::semantic::model::ExtractedSymbol> {
    let sym = |segments: &[(&str, SegmentKind)], offset: usize, name_len: u32| {
        let (line, col) = line_col(CLONES_SOURCE, offset);
        one_occ_symbol(
            "semcrate",
            segments,
            SymbolKind::Function,
            SymbolClass::InWorkspace,
            CLONES_DOC,
            SourceRange::new(line, col, line, col + name_len),
            OccurrenceRole::Definition,
        )
    };
    let first_alpha = CLONES_SOURCE.find("alpha").unwrap();
    let second_alpha = CLONES_SOURCE.rfind("alpha").unwrap();
    let mut symbols = vec![
        sym(
            &[("m1", SegmentKind::Module), ("alpha", SegmentKind::Method)],
            first_alpha,
            5,
        ),
        sym(
            &[("m2", SegmentKind::Module), ("alpha", SegmentKind::Method)],
            second_alpha,
            5,
        ),
    ];
    for name in ["gamma", "delta", "epsilon", "omega"] {
        symbols.push(sym(
            &[(name, SegmentKind::Method)],
            CLONES_SOURCE.find(name).unwrap(),
            name.len() as u32,
        ));
    }
    symbols
}

fn m1_alpha() -> CanonicalId {
    id_of_pkg(
        "semcrate",
        &[("m1", SegmentKind::Module), ("alpha", SegmentKind::Method)],
    )
}

fn m2_alpha() -> CanonicalId {
    id_of_pkg(
        "semcrate",
        &[("m2", SegmentKind::Module), ("alpha", SegmentKind::Method)],
    )
}

fn clones_store() -> (GraphStore, Vec<(String, String)>) {
    let index = one_doc_index(CLONES_DOC, clone_symbols());
    let srcs = vec![(CLONES_DOC.to_string(), CLONES_SOURCE.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();
    (store, srcs)
}

/// The `similar` rows for a subject, as `(identity, marker)` pairs in answer order.
fn similar_rows(
    store: &GraphStore,
    srcs: &[(String, String)],
    subject: &str,
) -> Vec<(String, Option<CloneCertainty>)> {
    let engine = engine_over(store, srcs);
    let answer = engine.similar(subject, Detail::Signature, None).unwrap();
    match answer.outcome {
        Outcome::Found { results } => results
            .into_iter()
            .map(|item: SimilarItem| (item.symbol.canonical_id.as_str().to_string(), item.clone_certainty))
            .collect(),
        other => panic!("expected found results, got {other:?}"),
    }
}

// _(Neighbors ranked with the subject excluded)_ — every other corpus symbol is returned; the
// subject is not among them.
#[test]
fn neighbors_are_ranked_with_the_subject_excluded() {
    let (store, srcs) = clones_store();
    let rows = similar_rows(&store, &srcs, "m1::alpha");
    assert_eq!(rows.len(), 5, "every other corpus symbol is a candidate");
    assert!(rows.iter().all(|(id, _)| *id != m1_alpha().as_str()));
}

// _(A verbatim copy is marked exact and ranks first)_ — the copy differing only in layout carries
// the exact-clone marker and precedes every unmarked row.
#[test]
fn a_formatting_variant_copy_is_marked_exact_and_first() {
    let (store, srcs) = clones_store();
    let rows = similar_rows(&store, &srcs, "m1::alpha");
    assert_eq!(rows[0].0, m2_alpha().as_str());
    assert_eq!(rows[0].1, Some(CloneCertainty::ExactClone));
}

// _(A renamed copy is marked and ordered between)_ — the consistently renamed copy carries the
// variant-clone marker, follows the exact-clone row, and precedes every unmarked row.
#[test]
fn a_renamed_copy_is_marked_variant_and_ordered_between() {
    let (store, srcs) = clones_store();
    let rows = similar_rows(&store, &srcs, "m1::alpha");
    let gamma_pos = rows.iter().position(|(id, _)| *id == sem_id("gamma").as_str()).unwrap();
    assert_eq!(rows[gamma_pos].1, Some(CloneCertainty::VariantClone));
    let exact_pos = rows.iter().position(|(id, _)| *id == m2_alpha().as_str()).unwrap();
    assert!(exact_pos < gamma_pos, "the exact clone precedes the variant clone");
    let first_unmarked = rows.iter().position(|(_, marker)| marker.is_none()).unwrap();
    assert!(
        gamma_pos < first_unmarked,
        "every marked row precedes every unmarked row"
    );
    // Within the variant class the estimated order applies: `delta` differs from the subject's
    // content by one literal character while `gamma` renames three identifiers, so any content
    // estimate ranks `delta` nearer — the class lift never scrambles the estimate inside a class.
    let delta_pos = rows.iter().position(|(id, _)| *id == sem_id("delta").as_str()).unwrap();
    assert!(
        delta_pos < gamma_pos,
        "the nearer variant precedes the farther within its class"
    );
}

// _(Detail does not change the result set — similar)_ — the same subject at two detail levels
// returns the same symbols in the same order.
#[test]
fn similar_detail_never_changes_the_result_set_or_order() {
    let (store, srcs) = clones_store();
    let engine = engine_over(&store, &srcs);
    let ids = |detail: Detail| -> Vec<String> {
        match engine.similar("m1::alpha", detail, None).unwrap().outcome {
            Outcome::Found { results } => results
                .into_iter()
                .map(|item| item.symbol.canonical_id.as_str().to_string())
                .collect(),
            other => panic!("expected found results, got {other:?}"),
        }
    };
    assert_eq!(ids(Detail::Location), ids(Detail::Body));
}

// _(A literal-substituted copy is marked as a variant)_ — the copy differing only in a literal
// value carries the variant-clone marker, never the exact-clone marker.
#[test]
fn a_literal_substituted_copy_is_variant_not_exact() {
    let (store, srcs) = clones_store();
    let rows = similar_rows(&store, &srcs, "m1::alpha");
    let delta = rows.iter().find(|(id, _)| *id == sem_id("delta").as_str()).unwrap();
    assert_eq!(delta.1, Some(CloneCertainty::VariantClone));
}

// _(An edited copy carries no marker)_ — a copy with a statement added shares neither key and is
// ordered by estimated similarity alone.
#[test]
fn an_edited_copy_carries_no_marker() {
    let (store, srcs) = clones_store();
    let rows = similar_rows(&store, &srcs, "m1::alpha");
    let epsilon = rows.iter().find(|(id, _)| *id == sem_id("epsilon").as_str()).unwrap();
    assert_eq!(epsilon.1, None);
}

// _(A keyless subject yields no markers)_ — a container carries no equivalence keys, so no row in
// its answer carries a clone marker, whatever keys the rows themselves hold.
#[test]
fn a_keyless_subject_yields_no_markers() {
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &support::fixture_index(), &sources()).unwrap();
    let engine = engine_over(&store, &sources());
    let answer = engine.similar("Client", Detail::Signature, None).unwrap();
    let Outcome::Found { results } = answer.outcome else {
        panic!("the fixture type has corpus neighbors");
    };
    assert!(results.iter().all(|row| row.clone_certainty.is_none()));
}

// _(A position resolves the enclosing symbol as subject)_ — a byte offset inside `alpha`'s body
// takes `alpha` as the subject: `alpha` is excluded and its exact clone ranks first.
#[test]
fn a_position_resolves_the_enclosing_symbol_as_subject() {
    let (store, srcs) = clones_store();
    let engine = engine_over(&store, &srcs);
    let inside_alpha = CLONES_SOURCE.find("let total = x + y;").unwrap() + 4;
    let answer = engine
        .similar_by_position(CLONES_DOC, inside_alpha, Detail::Signature, None)
        .unwrap();
    let Outcome::Found { results } = answer.outcome else {
        panic!("a position inside a body resolves a subject");
    };
    assert!(results.iter().all(|row| row.symbol.canonical_id != m1_alpha()));
    assert_eq!(results[0].symbol.canonical_id, m2_alpha());
    assert_eq!(results[0].clone_certainty, Some(CloneCertainty::ExactClone));
}

// _(Similar ranks by the two-signal hybrid)_ — the answer's row order is the RRF fusion of the
// subject's vector neighbors with BM25 over the subject's own render words, beneath the certainty
// classes. Recomputing the fusion from the store's two signals must reproduce the answer's order —
// evidence the ranking is fused, which a vector-only ranking would fail.
#[test]
fn similar_order_is_the_fused_two_leg_ranking() {
    use silent_cartographer::query::search::{rrf_fuse, subject_word_expr};

    let (store, srcs) = clones_store();
    let subject = m1_alpha();

    let subject_vector = store.semantic_vector_of(&subject).unwrap().unwrap();
    let corpus_size = store.corpus_size().unwrap() as usize;
    let vector_ranks: Vec<CanonicalId> = store
        .vector_neighbors(&subject_vector, corpus_size)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let render = store.semantic_render_of(&subject).unwrap().unwrap();
    let expr = subject_word_expr(&render).expect("a render spells words");
    let lexical_ranks: Vec<CanonicalId> = store
        .lexical_neighbors(&expr, corpus_size)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let expected: Vec<String> = rrf_fuse(&[vector_ranks, lexical_ranks])
        .into_iter()
        .filter(|id| *id != subject)
        .map(|id| id.as_str().to_string())
        .collect();

    let rows = similar_rows(&store, &srcs, "m1::alpha");
    assert_eq!(rows.len(), expected.len(), "the answer spans the fused candidate set");
    // The certainty tier reorders across classes but preserves the fused order within each class;
    // restricted to the unmarked rows, the answer's order is exactly the fused order.
    let unmarked_in_answer: Vec<&String> = rows.iter().filter(|(_, m)| m.is_none()).map(|(id, _)| id).collect();
    let unmarked_ids: std::collections::HashSet<&String> = unmarked_in_answer.iter().copied().collect();
    let unmarked_expected: Vec<&String> = expected.iter().filter(|id| unmarked_ids.contains(id)).collect();
    assert_eq!(unmarked_in_answer, unmarked_expected);
}

// _(An ambiguous reference yields candidates)_ — a shortname shared by two indexed symbols returns
// the typed candidate set rather than an arbitrary choice.
#[test]
fn an_ambiguous_reference_yields_candidates() {
    let doc = "dups.rs";
    let source = "\
pub fn dup(x: u32) -> u32 {
    x + 1
}

pub fn dup(y: u32) -> u32 {
    y + 2
}
";
    let first = source.find("dup").unwrap();
    let second = source.rfind("dup").unwrap();
    let mk = |offset: usize, module: &str| {
        let (line, col) = line_col(source, offset);
        one_occ_symbol(
            "semcrate",
            &[(module, SegmentKind::Module), ("dup", SegmentKind::Method)],
            SymbolKind::Function,
            SymbolClass::InWorkspace,
            doc,
            SourceRange::new(line, col, line, col + 3),
            OccurrenceRole::Definition,
        )
    };
    let index = one_doc_index(doc, vec![mk(first, "m1"), mk(second, "m2")]);
    let srcs = vec![(doc.to_string(), source.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();
    let engine = engine_over(&store, &srcs);
    let answer = engine.similar("dup", Detail::Signature, None).unwrap();
    // The refusal terminates before any ranking is derived: no estimation marker, no
    // semantic-index provenance — the marker asserts a derivation, never the command invoked.
    assert_eq!(answer.classification, None);
    assert!(answer.semantic_index.is_none());
    let Outcome::Ambiguous { candidates, .. } = answer.outcome else {
        panic!("a shared shortname is ambiguous");
    };
    assert_eq!(candidates.len(), 2);
}

// _(An unresolved subject carries no marker)_ — a reference denoting no symbol is typed absence
// with neither the estimation marker nor the semantic-index provenance: nothing was derived.
#[test]
fn an_unresolved_subject_carries_no_marker() {
    let (store, srcs) = clones_store();
    let engine = engine_over(&store, &srcs);
    let answer = engine
        .similar("no_such_symbol_anywhere", Detail::Signature, None)
        .unwrap();
    assert!(matches!(answer.outcome, Outcome::Absent));
    assert_eq!(answer.classification, None);
    assert!(answer.semantic_index.is_none());
}

// _(An unresolved subject carries no marker — position arm)_ — a source position enclosed by no
// indexed symbol is the same typed absence with no marker and no provenance, through the command
// handler's `--at` branch.
#[test]
fn an_unresolved_position_subject_is_absent_with_no_marker() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let out = silent_cartographer::commands::run_similar(
        &db,
        dir.path(),
        "rust-analyzer",
        None,
        Some("unknown.rs:3"),
        Detail::Signature,
        10,
        false,
        0,
        None,
        true,
        false,
    )
    .unwrap();
    let answer: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(answer["outcome"]["outcome"], "absent");
    assert!(answer.get("classification").is_none(), "no estimation marker: {answer}");
    assert!(
        answer.get("semantic_index").is_none(),
        "no semantic provenance: {answer}"
    );
}

// _(A subject outside the corpus is typed absence with the marker)_ — a resolved subject that
// contributes no corpus entry (an external symbol) has no representation to compare. The corpus
// holds other symbols, so a ranking pass would have produced found rows: the empty answer can only
// come from the no-representation return, and it carries the marker and provenance.
#[test]
fn a_subject_with_no_corpus_entry_is_typed_absence_with_the_marker() {
    let mut symbols = clone_symbols();
    symbols.push(one_occ_symbol(
        "depcrate",
        &[("Widget", SegmentKind::Type)],
        SymbolKind::Type,
        SymbolClass::External,
        CLONES_DOC,
        SourceRange::new(0, 7, 0, 12),
        OccurrenceRole::Reference,
    ));
    let index = one_doc_index(CLONES_DOC, symbols);
    let srcs = vec![(CLONES_DOC.to_string(), CLONES_SOURCE.to_string())];
    let mut store = GraphStore::open_in_memory().unwrap();
    ingest(&mut store, &ws(), Some(WS_ROOT), &index, &srcs).unwrap();
    assert!(store.corpus_size().unwrap() > 0, "the corpus holds other symbols");

    let engine = engine_over(&store, &srcs);
    let answer = engine.similar("Widget", Detail::Signature, None).unwrap();
    assert!(matches!(answer.outcome, Outcome::Empty));
    assert_eq!(answer.classification, Some("estimation"));
    assert!(answer.semantic_index.is_some(), "the empty answer carries provenance");
}

// _(Token overlap is not a precondition)_ — a query sharing no token with any render still
// returns every corpus symbol as a candidate: membership never requires lexical overlap.
#[test]
fn token_overlap_is_not_a_precondition_for_membership() {
    let (store, srcs) = search_store();
    let order = search_order(&store, &srcs, "zzqx vvwq kkjy", Detail::Location);
    assert_eq!(order.len(), 3, "every corpus symbol is still a candidate");
}

// _(No other corpus symbol is typed absence)_ — a corpus holding only the subject answers a
// definite empty, still carrying the estimation marker.
#[test]
fn a_corpus_with_only_the_subject_is_typed_absence() {
    let doc = "solo.rs";
    let source = "pub fn lonely(x: u32) -> u32 {\n    x\n}\n";
    let (store, srcs) = store_over(doc, source, &["lonely"]);
    let engine = engine_over(&store, &srcs);
    let answer = engine.similar("lonely", Detail::Signature, None).unwrap();
    assert!(matches!(answer.outcome, Outcome::Empty));
    assert_eq!(answer.classification, Some("estimation"));
}

// A corpus larger than the vector extension's KNN ceiling (4096) still answers: the vector signal
// clamps its request and ranks the nearest-candidate pool instead of erroring. Regression coverage
// for the repo-scale corpus that first hit the ceiling; exercised at the store layer, where the
// clamp lives, so the fixture needs no 4100-symbol build.
#[test]
fn a_corpus_larger_than_the_knn_cap_still_answers() {
    use silent_cartographer::graph::store::{PersistedClass, SymbolRow};

    let store = GraphStore::open_in_memory().unwrap();
    for i in 0..4100u32 {
        let id = CanonicalId::from_raw(format!("test-ws::many::f{i}"));
        store
            .insert_symbol(&SymbolRow {
                canonical_id: id.clone(),
                display_name: format!("f{i}"),
                kind: "function".to_string(),
                class: PersistedClass::InWorkspace,
                document_path: Some("many.rs".to_string()),
                span: Some((0, 1)),
                span_text: Some("fn f() {}".to_string()),
                signature_text: Some("fn f()".to_string()),
                interface_text: Some("fn f()".to_string()),
                duplicated: false,
                test_rule: None,
            })
            .unwrap();
        // A distinct unit vector per entry, varying in its first two components.
        let mut vector = vec![0.0f32; 256];
        vector[0] = (i as f32).cos();
        vector[1] = (i as f32).sin();
        let bytes = silent_cartographer::graph::embed::vector_bytes(&vector);
        store.insert_corpus_entry(&id, "render", "words", &bytes).unwrap();
    }

    let query = silent_cartographer::graph::embed::vector_bytes(&{
        let mut v = vec![0.0f32; 256];
        v[0] = 1.0;
        v
    });
    let neighbors = store.vector_neighbors(&query, 5000).unwrap();
    assert_eq!(
        neighbors.len(),
        4096,
        "the pool is clamped to the KNN ceiling, not an error"
    );
}

// ---- Command-level: labeling, bounding, and continuation tokens through the real handlers ----

/// A fixture store on disk (the standard 5-symbol workspace), for the command handlers.
fn fixture_db(dir: &Path) -> std::path::PathBuf {
    support::build_fixture_db(dir, "test-ws")
}

fn run_search_json(db: &Path, root: &Path, query: &str, limit: usize, cursor: Option<&str>) -> serde_json::Value {
    let out = silent_cartographer::commands::run_search(
        db,
        root,
        "rust-analyzer",
        query,
        Detail::Signature,
        10,
        false,
        limit,
        cursor,
        true,
        false,
    )
    .unwrap();
    serde_json::from_str(&out).unwrap()
}

// _(Machine answer carries the marker and provenance)_ — the JSON answer carries the structural
// estimation marker and the semantic-index identity.
#[test]
fn the_machine_answer_carries_the_marker_and_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let answer = run_search_json(&db, dir.path(), "connect a client", 0, None);
    assert_eq!(answer["classification"], "estimation");
    assert_eq!(answer["semantic_index"]["model_identity"], MODEL_ID);
    assert!(answer["semantic_index"]["corpus_definition_version"].is_u64());
    assert_eq!(answer["outcome"]["outcome"], "found");
}

// _(Human render frames results as candidates)_ — the human rendering presents rows as the nearest
// candidates by estimation, never as the complete set of matches.
#[test]
fn the_human_render_frames_results_as_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let out = silent_cartographer::commands::run_search(
        &db,
        dir.path(),
        "rust-analyzer",
        "connect a client",
        Detail::Signature,
        10,
        false,
        0,
        None,
        false,
        false,
    )
    .unwrap();
    assert!(
        out.contains("nearest candidates by model-derived estimation"),
        "the render frames candidates: {out}"
    );
    assert!(
        out.contains("semantic index:"),
        "the render names the provenance: {out}"
    );
}

// _(An empty answer keeps the marker and scoped absence)_ — a search against an empty corpus keeps
// the estimation marker, and the human render never presents the absence as proof.
#[test]
fn an_empty_answer_keeps_the_marker_and_scoped_absence() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let doc = "empty.rs";
    let index = one_doc_index(doc, Vec::new());
    let srcs = vec![(doc.to_string(), "// nothing declared\n".to_string())];
    silent_cartographer::commands::build_from_index(&db, "test-ws", dir.path(), &index, &srcs).unwrap();

    let machine = run_search_json(&db, dir.path(), "anything", 0, None);
    assert_eq!(machine["classification"], "estimation");
    assert_eq!(machine["outcome"]["outcome"], "empty");

    let human = silent_cartographer::commands::run_search(
        &db,
        dir.path(),
        "rust-analyzer",
        "anything",
        Detail::Signature,
        10,
        false,
        0,
        None,
        false,
        false,
    )
    .unwrap();
    assert!(
        human.contains("not proof that no relevant code exists"),
        "the absence is scoped, never proof: {human}"
    );
}

// _(Clone markers are presented as fact)_ — the human render states an exact clone as
// deterministic equivalence, distinct from the estimation note.
#[test]
fn clone_markers_render_as_fact_distinct_from_the_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let index = one_doc_index(CLONES_DOC, clone_symbols());
    let srcs = vec![(CLONES_DOC.to_string(), CLONES_SOURCE.to_string())];
    silent_cartographer::commands::build_from_index(&db, "test-ws", dir.path(), &index, &srcs).unwrap();

    let out = silent_cartographer::commands::run_similar(
        &db,
        dir.path(),
        "rust-analyzer",
        Some("m1::alpha"),
        None,
        Detail::Signature,
        10,
        false,
        0,
        None,
        false,
        false,
    )
    .unwrap();
    assert!(out.contains("[exact clone"), "the exact clone is stated as fact: {out}");
    assert!(
        out.contains("not behavioral equivalence"),
        "the variant marker disclaims behavior: {out}"
    );
    assert!(
        out.contains("nearest candidates by model-derived estimation"),
        "the estimation note rides alongside: {out}"
    );
}

// _(Result limit and truncation disclosure; continuation token resume)_ — the inherited bounded
// answer contract holds through `search`: a limited page discloses truncation and a cursor, and
// the cursor resumes the next page with the remaining rows.
#[test]
fn search_pages_disclose_truncation_and_resume_by_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());

    let first = run_search_json(&db, dir.path(), "client connection", 2, None);
    assert_eq!(first["page"]["truncated"], true);
    assert_eq!(first["page"]["returned"], 2);
    assert_eq!(first["page"]["total"], 5);
    // The estimation marker and semantic-index provenance survive the pagination rebuild, on the
    // truncated first page and on the resumed page alike.
    assert_eq!(first["classification"], "estimation");
    assert!(first["semantic_index"]["model_identity"].is_string());
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a truncated page carries a cursor");

    let second = run_search_json(&db, dir.path(), "client connection", 2, Some(cursor));
    assert_eq!(second["page"]["page_index"], 1);
    assert_eq!(second["classification"], "estimation");
    assert!(second["semantic_index"]["model_identity"].is_string());
    let ids = |answer: &serde_json::Value| -> Vec<String> {
        answer["outcome"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["symbol"]["canonical_id"].as_str().unwrap().to_string())
            .collect()
    };
    let first_ids = ids(&first);
    let second_ids = ids(&second);
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "pages do not overlap"
    );
}

// _(Mismatched continuation token refused)_ — a token issued for one `similar` subject is refused
// as a usage error when presented with another subject.
#[test]
fn a_mismatched_token_is_refused_on_similar() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let run = |subject: &str, cursor: Option<&str>| {
        silent_cartographer::commands::run_similar(
            &db,
            dir.path(),
            "rust-analyzer",
            Some(subject),
            None,
            Detail::Signature,
            10,
            false,
            2,
            cursor,
            true,
            false,
        )
    };
    let first: serde_json::Value = serde_json::from_str(&run("connect", None).unwrap()).unwrap();
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a truncated page carries a cursor");

    let err = run("open", Some(cursor)).expect_err("a token from another subject is refused");
    assert_eq!(classify(&err), ExitCode::Usage);
    assert!(
        err.to_string().contains("different query parameters"),
        "the refusal names the mismatch: {err}"
    );
}

// ---- Process-level: default detail and the exit taxonomy through the real binary ----

fn c10r(db: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_c10r"));
    cmd.arg("--db").arg(db);
    cmd
}

// _(Rows default to signature detail)_ — `search` with no detail requested carries each row's
// signature tier.
#[test]
fn search_rows_default_to_signature_detail() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let out = c10r(&db)
        .args(["search", "connect a client", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = answer["outcome"]["results"].as_array().unwrap();
    let connect = rows
        .iter()
        .find(|row| row["symbol"]["name"] == "connect")
        .expect("the fixture method is a candidate");
    assert_eq!(connect["content"], "pub fn connect(&self)");
}

// _(Similar rows default to signature detail)_ — `similar` with no detail requested carries each
// row's signature tier, through the binary's flag default.
#[test]
fn similar_rows_default_to_signature_detail() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let out = c10r(&db).args(["similar", "connect", "--json"]).output().unwrap();
    assert!(out.status.success());
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = answer["outcome"]["results"].as_array().unwrap();
    let disconnect = rows
        .iter()
        .find(|row| row["symbol"]["name"] == "disconnect")
        .expect("the sibling method is a neighbor");
    assert_eq!(disconnect["content"], "pub fn disconnect(&self)");
}

// _(Exit taxonomy through the new commands)_ — success on results, success on a typed-empty
// answer, the usage code on an out-of-set detail value, and the no-index code with no store.
#[test]
fn exit_codes_cover_success_empty_usage_and_no_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());

    // Success on results.
    let ok = c10r(&db).args(["search", "client", "--json"]).output().unwrap();
    assert_eq!(ok.status.code(), Some(0));

    // Success on a typed-empty answer: an empty-corpus store.
    let empty_db = dir.path().join("empty.db");
    let index = one_doc_index("empty.rs", Vec::new());
    let srcs = vec![("empty.rs".to_string(), "// nothing\n".to_string())];
    silent_cartographer::commands::build_from_index(&empty_db, "test-ws", dir.path(), &index, &srcs).unwrap();
    let empty = c10r(&empty_db).args(["search", "anything", "--json"]).output().unwrap();
    assert_eq!(empty.status.code(), Some(0), "a typed-empty answer is success");

    // Usage on an out-of-set detail value.
    let usage = c10r(&db)
        .args(["search", "client", "--detail", "bogus"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));

    // No index at the store path.
    let missing = dir.path().join("missing.db");
    let no_index = c10r(&missing).args(["similar", "connect"]).output().unwrap();
    assert_eq!(no_index.status.code(), Some(3));
}
