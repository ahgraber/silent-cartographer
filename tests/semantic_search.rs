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
use silent_cartographer::query::output::{Classification, Outcome};
use silent_cartographer::query::resolve::SourcePosition;
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
    store_over_with_params(doc, source, names, &Default::default())
}

/// [`store_over`] under explicit chunk parameters, for fixtures whose passages must split.
fn store_over_with_params(
    doc: &str,
    source: &str,
    names: &[&str],
    params: &silent_cartographer::graph::chunk::ChunkParams,
) -> (GraphStore, Vec<(String, String)>) {
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
    silent_cartographer::graph::ingest_with_params(&mut store, &ws(), Some(WS_ROOT), &index, &srcs, params).unwrap();
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
    let Some(Classification::Estimation { semantic_index }) = answer.classification else {
        panic!("the empty answer carries the estimation marker");
    };
    assert_eq!(semantic_index.model_identity, MODEL_ID);
}

// _(The marker composes with staleness)_ — an engine told a different current hash grades the
// answer stale, and the estimation marker rides independently beside it.
#[test]
fn the_estimation_marker_composes_with_staleness() {
    let (store, _) = clones_store();
    let engine = QueryEngine::new(&store, provenance(), "a-different-hash".to_string(), None);
    let answer = engine.similar("m1::alpha", Detail::Signature, None).unwrap();
    assert!(answer.stale, "a changed hash grades the answer stale");
    assert!(
        matches!(answer.classification, Some(Classification::Estimation { .. })),
        "the estimation marker rides independently beside staleness"
    );
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
        .similar_by_position(CLONES_DOC, SourcePosition::Byte(inside_alpha), Detail::Signature, None)
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

    let subject_vectors = store.chunk_vectors_of(&subject).unwrap();
    assert!(!subject_vectors.is_empty(), "the subject is in the corpus");
    let corpus_size = store.corpus_size().unwrap() as usize;
    let chunk_count = store.chunk_count().unwrap() as usize;
    // The dense leg as the engine derives it: each subject chunk's neighbors (already best-chunk
    // per candidate), min-merged across subject chunks — the best pair.
    let mut best: std::collections::HashMap<CanonicalId, f64> = std::collections::HashMap::new();
    for subject_vector in &subject_vectors {
        for (id, distance) in store.vector_neighbors(subject_vector, chunk_count).unwrap() {
            let entry = best.entry(id).or_insert(distance);
            if distance < *entry {
                *entry = distance;
            }
        }
    }
    let mut nearest: Vec<(CanonicalId, f64)> = best.into_iter().collect();
    nearest.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let vector_ranks: Vec<CanonicalId> = nearest.into_iter().map(|(id, _)| id).collect();
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
    // The certainty class reorders across classes but preserves the fused order within each class;
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
    // The refusal contains no estimation-derived content: no estimation marker, and so no
    // semantic-index provenance either — the marker asserts a derivation, never the command
    // invoked, and the provenance rides inside the marker rather than beside it.
    assert_eq!(answer.classification, None);
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
        &[],
        0,
        None,
        true,
        false,
        silent_cartographer::cli::PositionUnit::Line,
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
// contributes no passage (an external symbol) has no representation to compare. The corpus
// holds other symbols, so a ranking pass would have produced found rows: the empty answer can only
// come from the no-representation return, and it carries the marker and provenance.
#[test]
fn a_subject_with_no_passage_is_typed_absence_with_the_marker() {
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
    // The marker and its provenance are one value, so carrying the marker is carrying the
    // provenance — there is no longer a second thing to assert.
    assert!(
        matches!(answer.classification, Some(Classification::Estimation { .. })),
        "the empty answer carries the marker and its provenance"
    );
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
    assert!(
        matches!(answer.classification, Some(Classification::Estimation { .. })),
        "a definite empty still carries the estimation marker"
    );
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
                line_column: None,
                header_end: None,
                span_text: Some("fn f() {}".to_string()),
                signature_text: Some("fn f()".to_string()),
                interface_text: Some("fn f()".to_string()),
                duplicated: false,
                test_rule: None,
            })
            .unwrap();
        // A distinct unit vector per passage, varying in its first two components.
        let mut vector = vec![0.0f32; 256];
        vector[0] = (i as f32).cos();
        vector[1] = (i as f32).sin();
        let bytes = silent_cartographer::graph::embed::vector_bytes(&vector);
        store.insert_passage(&id, "render", "words", &[bytes]).unwrap();
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
        silent_cartographer::cli::PositionUnit::Line,
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
    // The whole recorded identity rides the machine answer: the chunk parameters are the
    // operator-variable part of the regime, so without them two stores built at different
    // parameters would carry identical provenance.
    assert!(answer["semantic_index"]["chunk_size"].is_u64());
    assert!(answer["semantic_index"]["chunk_overlap"].is_u64());
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
        silent_cartographer::cli::PositionUnit::Line,
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
        silent_cartographer::cli::PositionUnit::Line,
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
        &[],
        0,
        None,
        false,
        false,
        silent_cartographer::cli::PositionUnit::Line,
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
            &[],
            2,
            cursor,
            true,
            false,
            silent_cartographer::cli::PositionUnit::Line,
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

// _(A cursor is bound to the position unit)_ — `--at src/net.rs:4` names a line under one unit and a
// byte offset under the other, so a token issued under the line unit is refused under the byte unit.
#[test]
fn a_cursor_from_an_at_query_is_refused_under_the_other_position_unit() {
    use silent_cartographer::cli::PositionUnit;

    let dir = tempfile::tempdir().unwrap();
    let db = fixture_db(dir.path());
    let at = format!("{}:4", support::DOC);
    let run = |unit: PositionUnit, cursor: Option<&str>| {
        silent_cartographer::commands::run_similar(
            &db,
            dir.path(),
            "rust-analyzer",
            None,
            Some(&at),
            Detail::Signature,
            10,
            false,
            &[],
            1,
            cursor,
            true,
            false,
            unit,
        )
    };
    let first: serde_json::Value = serde_json::from_str(&run(PositionUnit::Line, None).unwrap()).unwrap();
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a truncated page carries a cursor");

    assert!(
        run(PositionUnit::Line, Some(cursor)).is_ok(),
        "the token resumes under the unit it was issued under"
    );
    let err = run(PositionUnit::Byte, Some(cursor)).expect_err("the token is refused under the other unit");
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

/// The multi-chunk fixture: long functions that split at a small chunk size. `alpha_walk` carries
/// distinctive vocabulary only near its end; `beta_walk` and `gamma_walk` coincide in one region
/// only; `plain_helper` is short and disjoint.
const LONG_DOC: &str = "long.rs";

fn long_source() -> String {
    let filler = |tag: &str| -> String {
        (0..40)
            .map(|i| format!("    let {tag}_line_{i} = {i};\n"))
            .collect::<String>()
    };
    let shared = "    let shared_quantile_sketch = merge_digest_centroids();\n    \
                  let shared_rank_estimate = interpolate_quantile_rank();\n";
    format!(
        "pub fn alpha_walk() {{\n{a}    let glacier_melt_telemetry = 9;\n}}\n\n\
         pub fn beta_walk() {{\n{b}{shared}}}\n\n\
         pub fn gamma_walk() {{\n{g}{shared}}}\n\n\
         pub fn plain_helper() {{ let tiny = 1; }}\n",
        a = filler("alpha_step"),
        b = filler("beta_hop"),
        g = filler("gamma_leap"),
    )
}

fn long_store() -> (GraphStore, Vec<(String, String)>) {
    let source = long_source();
    let (store, srcs) = store_over_with_params(
        LONG_DOC,
        &source,
        &["alpha_walk", "beta_walk", "gamma_walk", "plain_helper"],
        &silent_cartographer::graph::chunk::ChunkParams {
            chunk_size: 64,
            overlap: 0,
        },
    );
    // The fixture only tests what it claims when the long passages actually split.
    for name in ["alpha_walk", "beta_walk", "gamma_walk"] {
        assert!(
            store.chunk_vectors_of(&sem_id(name)).unwrap().len() > 1,
            "{name} splits into several chunks under the small size"
        );
    }
    (store, srcs)
}

// _(Scenario: Content late in a long symbol is findable)_ — distinctive content near the end of a
// passage that exceeds the chunk size still surfaces the symbol, because a later chunk carries it.
#[test]
fn content_late_in_a_long_symbol_is_findable() {
    let (store, srcs) = long_store();
    let order = search_order(&store, &srcs, "glacier melt telemetry", Detail::Signature);
    assert!(
        order.contains(&sem_id("alpha_walk").as_str().to_string()),
        "the long symbol surfaces for words drawn from its tail: {order:?}"
    );
}

// _(Scenario: A multi-chunk symbol appears at most once)_ — a symbol whose several chunks all
// match a query still contributes exactly one row.
#[test]
fn a_multi_chunk_symbol_appears_at_most_once() {
    let (store, srcs) = long_store();
    // "alpha step" words appear in every chunk of alpha_walk's body.
    let order = search_order(&store, &srcs, "alpha step walk", Detail::Signature);
    let alpha = sem_id("alpha_walk").as_str().to_string();
    assert_eq!(
        order.iter().filter(|id| **id == alpha).count(),
        1,
        "the multi-chunk symbol appears exactly once: {order:?}"
    );
    let mut unique = order.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), order.len(), "no symbol appears twice: {order:?}");
}

// _(Scenario: A multi-chunk subject excludes only itself)_ — no returned row is the subject,
// however many chunks represent it, and every other corpus symbol is still ranked.
#[test]
fn a_multi_chunk_subject_excludes_only_itself() {
    let (store, srcs) = long_store();
    let rows = similar_rows(&store, &srcs, "alpha_walk");
    let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    assert!(
        !ids.contains(&sem_id("alpha_walk").as_str()),
        "the subject is not among its own neighbors: {ids:?}"
    );
    for other in ["beta_walk", "gamma_walk", "plain_helper"] {
        assert!(
            ids.contains(&sem_id(other).as_str()),
            "{other} is still ranked: {ids:?}"
        );
    }
}

// _(Scenario: Symbols sharing one region rank as similar)_ — two passages that exceed the chunk
// size and coincide in one region only rank as similar: the best pair is the shared region's
// chunk pair, so the sharer ranks above a disjoint symbol.
#[test]
fn symbols_sharing_one_region_rank_as_similar() {
    let (store, srcs) = long_store();
    let rows = similar_rows(&store, &srcs, "beta_walk");
    let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    let gamma = ids.iter().position(|id| *id == sem_id("gamma_walk").as_str());
    let helper = ids.iter().position(|id| *id == sem_id("plain_helper").as_str());
    let (gamma, helper) = (
        gamma.expect("the region-sharer is among the rows"),
        helper.expect("the disjoint symbol is among the rows"),
    );
    assert!(
        gamma < helper,
        "the symbol sharing a region ranks above the disjoint one: {ids:?}"
    );
}

/// Two functions with the same body, the second followed by unrelated padding that adds chunks but
/// no matching content, plus a third function with the same body as the subject of a `similar`
/// query. `(single, multi)` name the one-chunk and many-chunk functions, so a test can swap them and
/// show the order does not follow the names.
fn chunk_count_store(single: &str, multi: &str) -> (GraphStore, Vec<(String, String)>) {
    let body = "    let quartz_resonator_value = tune_quartz_resonator();\n";
    let padding: String = (0..40).map(|i| format!("    let pad_line_{i} = {i};\n")).collect();
    let source = format!(
        "pub fn {single}() {{\n{body}}}\n\npub fn {multi}() {{\n{body}{padding}}}\n\npub fn probe_subject() {{\n{body}}}\n"
    );
    let (store, srcs) = store_over_with_params(
        LONG_DOC,
        &source,
        &[single, multi, "probe_subject"],
        &silent_cartographer::graph::chunk::ChunkParams {
            chunk_size: 64,
            overlap: 0,
        },
    );
    assert_eq!(store.chunk_vectors_of(&sem_id(single)).unwrap().len(), 1);
    assert!(
        store.chunk_vectors_of(&sem_id(multi)).unwrap().len() > 1,
        "{multi} splits into several chunks"
    );
    (store, srcs)
}

/// Position of `name` in `ids`.
fn position_of(ids: &[String], name: &str) -> usize {
    ids.iter()
        .position(|id| id == sem_id(name).as_str())
        .unwrap_or_else(|| panic!("{name} is among the rows: {ids:?}"))
}

// _(Search by meaning — chunk-count neutrality)_ — a symbol represented by many chunks does not rank
// above an otherwise equal symbol represented by one, whichever of the two names each carries.
#[test]
fn search_does_not_rank_a_symbol_higher_for_having_more_chunks() {
    for (single, multi) in [
        ("calibrate_first", "calibrate_second"),
        ("calibrate_second", "calibrate_first"),
    ] {
        let (store, srcs) = chunk_count_store(single, multi);
        let order = search_order(&store, &srcs, "tune quartz resonator value", Detail::Signature);
        assert!(
            position_of(&order, single) < position_of(&order, multi),
            "the one-chunk {single} ranks above the many-chunk {multi}: {order:?}"
        );
    }
}

// _(Similar-code lookup — chunk-count neutrality)_ — the same holds for `similar`, whether the
// many-chunk candidate or the one-chunk candidate sorts first by name.
#[test]
fn similar_does_not_rank_a_symbol_higher_for_having_more_chunks() {
    for (single, multi) in [
        ("calibrate_first", "calibrate_second"),
        ("calibrate_second", "calibrate_first"),
    ] {
        let (store, srcs) = chunk_count_store(single, multi);
        let ids: Vec<String> = similar_rows(&store, &srcs, "probe_subject")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(
            position_of(&ids, single) < position_of(&ids, multi),
            "the one-chunk {single} ranks above the many-chunk {multi}: {ids:?}"
        );
    }
}

// ---- Relation labels and exclusion ----

mod relations {
    use super::*;
    use silent_cartographer::cli::PositionUnit;
    use silent_cartographer::graph::store::EdgeKind;
    use silent_cartographer::graph::syntax::Language;
    use silent_cartographer::query::search::{RelationLabel, similar_candidates};
    use silent_cartographer::semantic::model::{ExtractedIndex, ExtractedOccurrence};
    use support::{Decl, decl_id};

    type Path = &'static [(&'static str, SegmentKind)];

    const SHAPE: Path = &[("Shape", SegmentKind::Type)];
    const SHAPE_DESCRIBE: Path = &[("Shape", SegmentKind::Type), ("describe", SegmentKind::Method)];
    const SQUARE: Path = &[("Square", SegmentKind::Type)];
    const SQUARE_DESCRIBE: Path = &[("Square", SegmentKind::Type), ("describe", SegmentKind::Method)];
    const SQUARE_AREA: Path = &[("Square", SegmentKind::Type), ("area", SegmentKind::Method)];
    const CIRCLE: Path = &[("Circle", SegmentKind::Type)];
    const CIRCLE_DESCRIBE: Path = &[("Circle", SegmentKind::Type), ("describe", SegmentKind::Method)];
    const LOOSE: Path = &[("Loose", SegmentKind::Type)];
    const LOOSE_DESCRIBE: Path = &[("Loose", SegmentKind::Type), ("describe", SegmentKind::Method)];
    const CORE: Path = &[("core", SegmentKind::Method)];
    const CALLEE: Path = &[("callee", SegmentKind::Method)];
    const FACADE: Path = &[("facade", SegmentKind::Method)];
    const CHECKS: Path = &[("verifies_core", SegmentKind::Method)];
    const PLAIN: Path = &[("plain", SegmentKind::Method)];
    const LEFT: Path = &[("Left", SegmentKind::Type)];
    const LEFT_RUN: Path = &[("Left", SegmentKind::Type), ("run", SegmentKind::Method)];
    const RIGHT: Path = &[("Right", SegmentKind::Type)];
    const RIGHT_RUN: Path = &[("Right", SegmentKind::Type), ("run", SegmentKind::Method)];
    const EXTERNAL_BASE: Path = &[("Base", SegmentKind::Type)];
    const EXTERNAL_MARKER: Path = &[("Marker", SegmentKind::Type)];

    /// A reference to the declaration at `to`, spelled as `name` inside the unique `within` text.
    struct Reference {
        to: Path,
        within: &'static str,
        name: &'static str,
    }

    /// A source in one language with the declarations and references its relations are drawn from.
    struct Fixture {
        language: Language,
        doc: &'static str,
        source: &'static str,
        decls: Vec<Decl<'static>>,
        references: Vec<Reference>,
        /// An external type referenced at its name inside each unique `within` text.
        external: Option<(Path, Vec<&'static str>)>,
    }

    fn decl(segments: Path, kind: SymbolKind, within: &'static str) -> Decl<'static> {
        Decl { segments, kind, within }
    }

    fn reference(to: Path, within: &'static str, name: &'static str) -> Reference {
        Reference { to, within, name }
    }

    impl Fixture {
        fn index(&self) -> ExtractedIndex {
            let mut index = support::declared_index(self.language, self.doc, self.source, &self.decls);
            let occurrence_at = |within: &str, name: &str| {
                assert_eq!(self.source.matches(within).count(), 1, "{within:?} must occur once");
                let at = self.source.find(within).unwrap() + within.find(name).unwrap();
                let (line, col) = line_col(self.source, at);
                ExtractedOccurrence {
                    document_path: self.doc.to_string(),
                    range: SourceRange::new(line, col, line, col + name.len() as u32),
                    role: OccurrenceRole::Reference,
                }
            };
            // `declared_index` lists the symbols in declaration order.
            for reference in &self.references {
                let target = self.decls.iter().position(|d| d.segments == reference.to).unwrap();
                index.symbols[target]
                    .occurrences
                    .push(occurrence_at(reference.within, reference.name));
            }
            if let Some((path, withins)) = &self.external {
                let name = path[0].0;
                let mut symbol = support::one_occ_symbol(
                    "poscrate",
                    path,
                    SymbolKind::Type,
                    SymbolClass::External,
                    self.doc,
                    SourceRange::new(0, 0, 0, 0),
                    OccurrenceRole::Reference,
                );
                symbol.occurrences = withins.iter().map(|within| occurrence_at(within, name)).collect();
                index.symbols.push(symbol);
            }
            index
        }

        fn sources(&self) -> Vec<(String, String)> {
            vec![(self.doc.to_string(), self.source.to_string())]
        }

        fn store(&self) -> GraphStore {
            let mut store = GraphStore::open_in_memory().unwrap();
            ingest(&mut store, &ws(), Some(WS_ROOT), &self.index(), &self.sources()).unwrap();
            store
        }
    }

    fn rust_relations() -> Fixture {
        let source = "\
pub trait Shape {
    fn describe(&self) -> u32 { 1 }
}
pub struct Square;
pub struct Circle;
pub struct Loose;
impl Shape for Square {
    fn describe(&self) -> u32 { 2 }
    fn area(&self) -> u32 { 4 }
}
impl Shape for Circle {
    fn describe(&self) -> u32 { 3 }
}
impl Loose {
    fn describe(&self) -> u32 { 5 }
}
pub fn core(x: u32) -> u32 { callee(x) + 1 }
pub fn callee(x: u32) -> u32 { x * 2 }
pub fn facade(x: u32) -> u32 { core(x) }
#[test]
fn verifies_core() { core(1); }
pub fn plain(x: u32) -> u32 { x - 7 }
";
        Fixture {
            language: Language::Rust,
            doc: "src/rel.rs",
            source,
            decls: vec![
                decl(SHAPE, SymbolKind::Trait, "trait Shape"),
                decl(SHAPE_DESCRIBE, SymbolKind::Method, "fn describe(&self) -> u32 { 1 }"),
                decl(SQUARE, SymbolKind::Type, "struct Square"),
                decl(CIRCLE, SymbolKind::Type, "struct Circle"),
                decl(LOOSE, SymbolKind::Type, "struct Loose"),
                decl(SQUARE_DESCRIBE, SymbolKind::Method, "fn describe(&self) -> u32 { 2 }"),
                decl(SQUARE_AREA, SymbolKind::Method, "fn area(&self)"),
                decl(CIRCLE_DESCRIBE, SymbolKind::Method, "fn describe(&self) -> u32 { 3 }"),
                decl(LOOSE_DESCRIBE, SymbolKind::Method, "fn describe(&self) -> u32 { 5 }"),
                decl(CORE, SymbolKind::Function, "pub fn core(x: u32)"),
                decl(CALLEE, SymbolKind::Function, "pub fn callee(x: u32)"),
                decl(FACADE, SymbolKind::Function, "pub fn facade(x: u32)"),
                decl(CHECKS, SymbolKind::Function, "fn verifies_core()"),
                decl(PLAIN, SymbolKind::Function, "pub fn plain(x: u32)"),
            ],
            references: vec![
                reference(SHAPE, "impl Shape for Square", "Shape"),
                reference(SHAPE, "impl Shape for Circle", "Shape"),
                reference(SQUARE, "impl Shape for Square", "Square"),
                reference(CIRCLE, "impl Shape for Circle", "Circle"),
                reference(LOOSE, "impl Loose {", "Loose"),
                reference(CALLEE, "callee(x) + 1", "callee"),
                reference(CORE, "{ core(x) }", "core"),
                reference(CORE, "{ core(1); }", "core"),
            ],
            external: None,
        }
    }

    fn python_relations() -> Fixture {
        let source = "\
class Shape:
    def describe(self):
        return 1


class Square(Shape):
    def describe(self):
        return 2

    def area(self):
        return 4


class Circle(Shape):
    def describe(self):
        return 3


class Loose:
    def describe(self):
        return 5


def core(x):
    return callee(x) + 1


def callee(x):
    return x * 2


def facade(x):
    return core(x)


def plain(x):
    return x - 7
";
        Fixture {
            language: Language::Python,
            doc: "rel.py",
            source,
            decls: vec![
                decl(SHAPE, SymbolKind::Type, "class Shape:"),
                decl(
                    SHAPE_DESCRIBE,
                    SymbolKind::Method,
                    "def describe(self):\n        return 1",
                ),
                decl(SQUARE, SymbolKind::Type, "class Square(Shape)"),
                decl(CIRCLE, SymbolKind::Type, "class Circle(Shape)"),
                decl(LOOSE, SymbolKind::Type, "class Loose:"),
                decl(
                    SQUARE_DESCRIBE,
                    SymbolKind::Method,
                    "def describe(self):\n        return 2",
                ),
                decl(SQUARE_AREA, SymbolKind::Method, "def area(self)"),
                decl(
                    CIRCLE_DESCRIBE,
                    SymbolKind::Method,
                    "def describe(self):\n        return 3",
                ),
                decl(
                    LOOSE_DESCRIBE,
                    SymbolKind::Method,
                    "def describe(self):\n        return 5",
                ),
                decl(CORE, SymbolKind::Function, "def core(x)"),
                decl(CALLEE, SymbolKind::Function, "def callee(x)"),
                decl(FACADE, SymbolKind::Function, "def facade(x)"),
                decl(PLAIN, SymbolKind::Function, "def plain(x)"),
            ],
            references: vec![
                reference(SHAPE, "class Square(Shape)", "Shape"),
                reference(SHAPE, "class Circle(Shape)", "Shape"),
                reference(CALLEE, "return callee(x) + 1", "callee"),
                reference(CORE, "return core(x)", "core"),
            ],
            external: None,
        }
    }

    /// A Python source whose file name classifies its symbols as test code, with a test that calls
    /// `core`. A Rust test is classified by attribute, inside [`rust_relations`].
    fn python_test_relations() -> Fixture {
        Fixture {
            language: Language::Python,
            doc: "test_core.py",
            source: "\
def core(x):
    return x + 1


def verifies_core():
    return core(1)


def plain(x):
    return x - 7
",
            decls: vec![
                decl(CORE, SymbolKind::Function, "def core(x)"),
                decl(CHECKS, SymbolKind::Function, "def verifies_core()"),
                decl(PLAIN, SymbolKind::Function, "def plain(x)"),
            ],
            references: vec![reference(CORE, "return core(1)", "core")],
            external: None,
        }
    }

    /// Two types whose only common supertype is defined outside the workspace.
    fn external_base(language: Language) -> Fixture {
        match language {
            Language::Rust => Fixture {
                language,
                doc: "src/ext.rs",
                source: "\
pub struct Left;
pub struct Right;
impl Marker for Left {
    fn run(&self) -> u32 { 1 }
}
impl Marker for Right {
    fn run(&self) -> u32 { 2 }
}
",
                decls: vec![
                    decl(LEFT, SymbolKind::Type, "struct Left"),
                    decl(RIGHT, SymbolKind::Type, "struct Right"),
                    decl(LEFT_RUN, SymbolKind::Method, "fn run(&self) -> u32 { 1 }"),
                    decl(RIGHT_RUN, SymbolKind::Method, "fn run(&self) -> u32 { 2 }"),
                ],
                references: vec![
                    reference(LEFT, "impl Marker for Left", "Left"),
                    reference(RIGHT, "impl Marker for Right", "Right"),
                ],
                external: Some((EXTERNAL_MARKER, vec!["impl Marker for Left", "impl Marker for Right"])),
            },
            Language::Python => Fixture {
                language,
                doc: "ext.py",
                source: "\
class Left(Base):
    def run(self):
        return 1


class Right(Base):
    def run(self):
        return 2
",
                decls: vec![
                    decl(LEFT, SymbolKind::Type, "class Left(Base)"),
                    decl(RIGHT, SymbolKind::Type, "class Right(Base)"),
                    decl(LEFT_RUN, SymbolKind::Method, "def run(self):\n        return 1"),
                    decl(RIGHT_RUN, SymbolKind::Method, "def run(self):\n        return 2"),
                ],
                references: Vec::new(),
                external: Some((EXTERNAL_BASE, vec!["class Left(Base)", "class Right(Base)"])),
            },
        }
    }

    /// The labels, as `label` or `label:rule`, on the row for `row` in `subject`'s answer.
    fn labels_on(store: &GraphStore, fixture: &Fixture, subject: Path, row: Path) -> Vec<String> {
        let engine = engine_over(store, &fixture.sources());
        let answer = engine
            .similar(decl_id(subject).as_str(), Detail::Location, None)
            .unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found results");
        };
        let item = results
            .iter()
            .find(|item| item.symbol.canonical_id == decl_id(row))
            .unwrap_or_else(|| panic!("{row:?} is a row of the answer"));
        item.relations
            .iter()
            .map(|relation| match &relation.rule {
                Some(rule) => format!("{}:{rule}", relation.label.tag()),
                None => relation.label.tag().to_string(),
            })
            .collect()
    }

    fn fixtures() -> [Fixture; 2] {
        [rust_relations(), python_relations()]
    }

    // _(A callee is labeled used by the subject; a caller is labeled as using the subject; an
    // unrelated row carries an empty set)_ — on every backend.
    #[test]
    fn calls_in_either_direction_are_labeled() {
        for fixture in fixtures() {
            let store = fixture.store();
            let lang = fixture.language;
            assert_eq!(
                labels_on(&store, &fixture, CORE, CALLEE),
                ["used_by_subject"],
                "{lang:?}"
            );
            assert_eq!(labels_on(&store, &fixture, CORE, FACADE), ["uses_subject"], "{lang:?}");
            assert!(labels_on(&store, &fixture, CORE, PLAIN).is_empty(), "{lang:?}");
        }
    }

    // _(A test that calls the subject carries both labels)_ — the test label names the convention
    // rule: an attribute in Rust, the file name in Python.
    #[test]
    fn a_test_that_calls_the_subject_carries_the_test_label_with_its_rule() {
        let rust = rust_relations();
        assert_eq!(
            labels_on(&rust.store(), &rust, CORE, CHECKS),
            ["uses_subject", "tests_subject:test_attribute"]
        );
        let python = python_test_relations();
        let store = python.store();
        assert_eq!(
            labels_on(&store, &python, CORE, CHECKS),
            ["uses_subject", "tests_subject:test_file"]
        );
        // Test code that does not use the subject carries neither label.
        assert!(labels_on(&store, &python, CORE, PLAIN).is_empty());
    }

    // _(Sibling implementations, and a base method and its override, are labeled type family;
    // methods of one type and unrelated types are not)_ — on every backend.
    #[test]
    fn related_types_are_labeled_type_family() {
        for fixture in fixtures() {
            let store = fixture.store();
            let lang = fixture.language;
            let family = ["type_family"];
            let on = |row| labels_on(&store, &fixture, SQUARE_DESCRIBE, row);
            assert_eq!(on(CIRCLE_DESCRIBE), family, "{lang:?} sibling");
            assert_eq!(on(SHAPE_DESCRIBE), family, "{lang:?} base");
            assert!(on(SQUARE_AREA).is_empty(), "{lang:?} same type");
            assert!(on(LOOSE_DESCRIBE).is_empty(), "{lang:?} unrelated");
        }
    }

    // _(An external common supertype is not a type family)_ — on every backend.
    #[test]
    fn an_external_common_supertype_is_not_a_type_family() {
        for language in support::LANGUAGES {
            let fixture = external_base(language);
            let store = fixture.store();
            let hierarchy = store.edges(EdgeKind::TypeHierarchy).unwrap();
            let subtypes: Vec<_> = hierarchy.iter().map(|(sub, _)| sub.clone()).collect();
            assert!(
                subtypes.contains(&decl_id(LEFT)) && subtypes.contains(&decl_id(RIGHT)),
                "{language:?}: both types declare the external base: {hierarchy:?}"
            );
            assert!(
                labels_on(&store, &fixture, LEFT_RUN, RIGHT_RUN).is_empty(),
                "{language:?}"
            );
        }
    }

    // _(Labels do not change the answer)_ — the rows and their order equal the ranked candidates.
    #[test]
    fn labels_leave_the_rows_and_their_order_unchanged() {
        let fixture = rust_relations();
        let store = fixture.store();
        let subject = store.symbol(&decl_id(CORE)).unwrap().unwrap();
        let candidates: Vec<CanonicalId> = similar_candidates(&store, &subject)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.id)
            .collect();
        let engine = engine_over(&store, &fixture.sources());
        let answer = engine.similar(decl_id(CORE).as_str(), Detail::Location, None).unwrap();
        let Outcome::Found { results } = answer.outcome else {
            panic!("expected found results");
        };
        assert!(
            results.iter().any(|item| !item.relations.is_empty()),
            "some row carries labels"
        );
        let rows: Vec<CanonicalId> = results.into_iter().map(|item| item.symbol.canonical_id).collect();
        assert_eq!(rows, candidates);
    }

    /// `similar` for `core` over the built Rust relations store.
    fn run_core(
        db: &std::path::Path,
        root: &std::path::Path,
        exclude: &[RelationLabel],
        limit: usize,
        cursor: Option<&str>,
        json: bool,
    ) -> anyhow::Result<String> {
        silent_cartographer::commands::run_similar(
            db,
            root,
            "rust-analyzer",
            Some(decl_id(CORE).as_str()),
            None,
            Detail::Location,
            10,
            false,
            exclude,
            limit,
            cursor,
            json,
            false,
            PositionUnit::Line,
        )
    }

    fn core_json(
        db: &std::path::Path,
        root: &std::path::Path,
        exclude: &[RelationLabel],
        limit: usize,
        cursor: Option<&str>,
    ) -> serde_json::Value {
        serde_json::from_str(&run_core(db, root, exclude, limit, cursor, true).unwrap()).unwrap()
    }

    fn built_db(dir: &std::path::Path) -> std::path::PathBuf {
        let fixture = rust_relations();
        let db = dir.join("index.db");
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &fixture.index(), &fixture.sources())
            .unwrap();
        db
    }

    /// The row identities of `answer` whose labels include none of `without`, in answer order.
    fn ids_without(answer: &serde_json::Value, without: &str) -> Vec<String> {
        answer["outcome"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                !row["relations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["label"] == without)
            })
            .map(|row| row["symbol"]["canonical_id"].as_str().unwrap().to_string())
            .collect()
    }

    // _(The human rendering shows each row's labels)_ — including the rule of a test label.
    #[test]
    fn the_human_render_shows_the_labels() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let out = run_core(&db, dir.path(), &[], 0, None, false).unwrap();
        for shown in [
            "[uses_subject]",
            "[used_by_subject]",
            "[tests_subject (rule: test_attribute)]",
        ] {
            assert!(out.contains(shown), "{shown} is rendered: {out}");
        }
    }

    // _(The human rendering names the excluded labels)_ — sorted, once each.
    #[test]
    fn the_human_render_names_the_excluded_relations() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let out = run_core(
            &db,
            dir.path(),
            &[RelationLabel::TypeFamily, RelationLabel::TestsSubject],
            0,
            None,
            false,
        )
        .unwrap();
        assert!(out.contains("excluded relations: tests_subject, type_family"), "{out}");
        let none = run_core(&db, dir.path(), &[], 0, None, false).unwrap();
        assert!(!none.contains("excluded relations"), "{none}");
    }

    // _(Excluded rows are omitted and the order is kept; the exclusion is disclosed)_ — a repeated
    // label counts once, and the disclosed set is sorted.
    #[test]
    fn an_excluded_label_removes_its_rows_and_keeps_the_order() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let all = core_json(&db, dir.path(), &[], 0, None);
        assert!(all.get("excluded_relations").is_none(), "no exclusion, no disclosure");
        let expected = ids_without(&all, "tests_subject");
        assert!(
            expected.len() < ids_without(&all, "").len(),
            "the test row was among the rows"
        );

        let excluded = core_json(
            &db,
            dir.path(),
            &[
                RelationLabel::TypeFamily,
                RelationLabel::TestsSubject,
                RelationLabel::TestsSubject,
            ],
            0,
            None,
        );
        assert_eq!(ids_without(&excluded, "type_family"), expected);
        assert_eq!(
            excluded["excluded_relations"],
            serde_json::json!(["tests_subject", "type_family"])
        );
    }

    // _(The limit counts remaining rows; a token under another exclusion is refused)_.
    #[test]
    fn the_limit_counts_remaining_rows_and_the_token_binds_the_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let remaining = ids_without(&core_json(&db, dir.path(), &[], 0, None), "uses_subject");
        let limit = remaining.len() - 1;
        let exclude = [RelationLabel::UsesSubject];
        let page = core_json(&db, dir.path(), &exclude, limit, None);
        assert_eq!(
            ids_without(&page, ""),
            remaining[..limit],
            "the page holds only remaining rows"
        );
        assert_eq!(page["page"]["total"], remaining.len());

        let cursor = page["page"]["cursor"]
            .as_str()
            .expect("a truncated page carries a cursor");
        let err = run_core(&db, dir.path(), &[], limit, Some(cursor), true)
            .expect_err("the token is bound to its exclusion");
        assert_eq!(classify(&err), ExitCode::Usage);
        assert!(err.to_string().contains("different query parameters"), "{err}");
        let resumed = core_json(&db, dir.path(), &exclude, limit, Some(cursor));
        assert_eq!(ids_without(&resumed, ""), remaining[limit..]);
    }

    // ---- Batch ----

    /// A store built from `fixture` at `dir`, with the subject reference for each backend's `core`.
    fn db_for(fixture: &Fixture, dir: &std::path::Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &fixture.index(), &fixture.sources())
            .unwrap();
        db
    }

    fn single_json(
        db: &std::path::Path,
        root: &std::path::Path,
        reference: &str,
        exclude: &[RelationLabel],
        limit: usize,
        cursor: Option<&str>,
    ) -> serde_json::Value {
        let out = silent_cartographer::commands::run_similar(
            db,
            root,
            "rust-analyzer",
            Some(reference),
            None,
            Detail::Location,
            10,
            false,
            exclude,
            limit,
            cursor,
            true,
            false,
            PositionUnit::Line,
        )
        .unwrap();
        serde_json::from_str(&out).unwrap()
    }

    fn batch(
        db: &std::path::Path,
        root: &std::path::Path,
        references: &[&str],
        exclude: &[RelationLabel],
        limit: usize,
        json: bool,
    ) -> String {
        let references: Vec<String> = references.iter().map(|r| r.to_string()).collect();
        silent_cartographer::commands::run_similar_batch(
            db,
            root,
            "rust-analyzer",
            &references,
            Detail::Location,
            10,
            false,
            exclude,
            limit,
            json,
            false,
            PositionUnit::Line,
        )
        .unwrap()
    }

    fn batch_json(
        db: &std::path::Path,
        root: &std::path::Path,
        references: &[&str],
        exclude: &[RelationLabel],
        limit: usize,
    ) -> Vec<serde_json::Value> {
        let answer: serde_json::Value =
            serde_json::from_str(&batch(db, root, references, exclude, limit, true)).unwrap();
        answer["answers"].as_array().unwrap().clone()
    }

    // _(Each subject gets its own answer in order; a per-subject answer equals the single answer; an
    // unresolved or ambiguous reference does not fail the batch)_ — a resolved, an ambiguous, and an
    // unresolved reference on every backend. The nested answer is re-indented, so the parsed values
    // are compared.
    #[test]
    fn a_batch_answers_each_subject_as_a_single_invocation_does() {
        for fixture in fixtures() {
            let dir = tempfile::tempdir().unwrap();
            let db = db_for(&fixture, dir.path());
            let core = decl_id(CORE);
            let references = [core.as_str(), "describe", "no_such_symbol_anywhere"];
            let answers = batch_json(&db, dir.path(), &references, &[], 0);
            assert_eq!(answers.len(), 3);
            let outcomes: Vec<&str> = answers
                .iter()
                .map(|entry| entry["answer"]["outcome"]["outcome"].as_str().unwrap())
                .collect();
            assert_eq!(outcomes, ["found", "ambiguous", "absent"], "{:?}", fixture.language);
            for (entry, reference) in answers.iter().zip(references) {
                assert_eq!(entry["reference"], reference);
                assert_eq!(
                    entry["answer"],
                    single_json(&db, dir.path(), reference, &[], 0, None),
                    "{:?}: {reference}",
                    fixture.language
                );
            }
        }
    }

    // _(A per-subject token resumes a single query)_.
    #[test]
    fn a_batch_continuation_token_resumes_a_single_subject_query() {
        let dir = tempfile::tempdir().unwrap();
        let db = db_for(&rust_relations(), dir.path());
        let core = decl_id(CORE);
        let answers = batch_json(&db, dir.path(), &[core.as_str()], &[], 2);
        let page = &answers[0]["answer"];
        let cursor = page["page"]["cursor"]
            .as_str()
            .expect("a truncated page carries a token");
        let resumed = single_json(&db, dir.path(), core.as_str(), &[], 2, Some(cursor));
        let all = ids_without(&single_json(&db, dir.path(), core.as_str(), &[], 0, None), "");
        let resumed = ids_without(&resumed, "");
        assert!(!resumed.is_empty());
        assert_eq!(resumed, all[2..2 + resumed.len()]);
    }

    // _(Rows carry labels and `--exclude` removes them, on the batch path)_.
    #[test]
    fn the_batch_path_labels_rows_and_honors_exclusion() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let core = decl_id(CORE);
        let all = batch_json(&db, dir.path(), &[core.as_str()], &[], 0);
        let labeled = all[0]["answer"]["outcome"]["results"].as_array().unwrap();
        assert!(
            labeled
                .iter()
                .any(|row| !row["relations"].as_array().unwrap().is_empty())
        );
        let excluded = batch_json(&db, dir.path(), &[core.as_str()], &[RelationLabel::TestsSubject], 0);
        assert_eq!(
            ids_without(&excluded[0]["answer"], ""),
            ids_without(&all[0]["answer"], "tests_subject")
        );
        assert_eq!(
            excluded[0]["answer"]["excluded_relations"],
            serde_json::json!(["tests_subject"])
        );
    }

    /// Non-default controls shared by a single and a batch invocation.
    const BODY_CONTROLS: (Detail, usize, usize) = (Detail::Body, 1, 1);

    /// `similar` for `reference` under [`BODY_CONTROLS`] and `exclude`, as a single invocation.
    fn single_under_controls(
        db: &std::path::Path,
        root: &std::path::Path,
        reference: &str,
        exclude: &[RelationLabel],
    ) -> serde_json::Value {
        let (detail, max_lines, limit) = BODY_CONTROLS;
        let out = silent_cartographer::commands::run_similar(
            db,
            root,
            "rust-analyzer",
            Some(reference),
            None,
            detail,
            max_lines,
            true,
            exclude,
            limit,
            None,
            true,
            false,
            PositionUnit::Line,
        )
        .unwrap();
        serde_json::from_str(&out).unwrap()
    }

    /// `similar` for `references` under [`BODY_CONTROLS`] and `exclude`, as a batch.
    fn batch_under_controls(
        db: &std::path::Path,
        root: &std::path::Path,
        references: &[&str],
        exclude: &[RelationLabel],
    ) -> Vec<serde_json::Value> {
        let (detail, max_lines, limit) = BODY_CONTROLS;
        let references: Vec<String> = references.iter().map(|r| r.to_string()).collect();
        let out = silent_cartographer::commands::run_similar_batch(
            db,
            root,
            "rust-analyzer",
            &references,
            detail,
            max_lines,
            true,
            exclude,
            limit,
            true,
            false,
            PositionUnit::Line,
        )
        .unwrap();
        let answer: serde_json::Value = serde_json::from_str(&out).unwrap();
        answer["answers"].as_array().unwrap().clone()
    }

    /// The `clone_certainty` of the row for `row` in `answer`, if it has one.
    fn certainty_of(answer: &serde_json::Value, row: Path) -> Option<&str> {
        answer["outcome"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["symbol"]["canonical_id"] == decl_id(row).as_str())
            .unwrap_or_else(|| panic!("{row:?} is a row of the answer"))["clone_certainty"]
            .as_str()
    }

    // _(A clone row carries its marker, singly and in a batch)_ — an exact sibling and a variant
    // base method, and the batch answer equals the single answer, on every backend.
    #[test]
    fn a_clone_row_carries_its_marker_in_a_single_and_a_batch_answer() {
        for language in support::LANGUAGES {
            let dir = tempfile::tempdir().unwrap();
            let db = db_for(&mutual::clone_family(language), dir.path());
            let subject = decl_id(SQUARE_DESCRIBE);
            let single = single_json(&db, dir.path(), subject.as_str(), &[], 0, None);
            assert_eq!(
                certainty_of(&single, CIRCLE_DESCRIBE),
                Some("exact_clone"),
                "{language:?}"
            );
            assert_eq!(
                certainty_of(&single, SHAPE_DESCRIBE),
                Some("variant_clone"),
                "{language:?}"
            );
            let answers = batch_json(&db, dir.path(), &[subject.as_str()], &[], 0);
            assert_eq!(answers[0]["answer"], single, "{language:?}");
        }
    }

    // _(A batch answers as a single invocation does under every control)_ — body detail, a line
    // bound, an exclusion, and a limit, for a clone subject, a subject with no clone key, and an
    // unresolved reference, on every backend.
    #[test]
    fn a_batch_equals_single_answers_under_non_default_controls() {
        for language in support::LANGUAGES {
            let dir = tempfile::tempdir().unwrap();
            let db = db_for(&mutual::clone_family(language), dir.path());
            let (clone, keyless) = (decl_id(SQUARE_DESCRIBE), decl_id(SQUARE));
            let references = [clone.as_str(), keyless.as_str(), "no_such_symbol_anywhere"];
            for exclude in [&[][..], &[RelationLabel::TypeFamily][..]] {
                let answers = batch_under_controls(&db, dir.path(), &references, exclude);
                for (entry, reference) in answers.iter().zip(references) {
                    assert_eq!(
                        entry["answer"],
                        single_under_controls(&db, dir.path(), reference, exclude),
                        "{language:?}: {reference} excluding {exclude:?}"
                    );
                }
                let rows = answers[0]["answer"]["outcome"]["results"].as_array().unwrap();
                assert_eq!(rows.len(), 1, "{language:?}: the limit applies");
                assert!(rows[0].get("content").is_some(), "{language:?}: body detail applies");
            }
            let keyless_answer = single_under_controls(&db, dir.path(), keyless.as_str(), &[]);
            assert!(
                keyless_answer["outcome"]["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|row| row.get("clone_certainty").is_none()),
                "{language:?}: a subject with no clone key marks no row"
            );
        }
    }

    // _(An empty list is typed absence)_ — a definite none, not a failure, in both renderings.
    #[test]
    fn an_empty_batch_is_a_definite_none() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        assert!(batch_json(&db, dir.path(), &[], &[], 0).is_empty());
        assert!(batch(&db, dir.path(), &[], &[], 0, false).starts_with("empty: no subjects"));
    }

    #[test]
    fn the_human_batch_headings_precede_each_answer() {
        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let out = batch(
            &db,
            dir.path(),
            &["no_such_symbol_anywhere", "also_missing"],
            &[],
            0,
            false,
        );
        let first = out.find("# no_such_symbol_anywhere\n").expect("first heading");
        let second = out.find("\n\n# also_missing\n").expect("second heading");
        assert!(first < second, "{out}");
        assert_eq!(out.matches("absent: nothing resolved").count(), 2, "{out}");
    }

    // _(Standard input supplies the list; an unreadable list is a usage error naming the source)_ —
    // through the built binary.
    #[test]
    fn the_list_comes_from_standard_input_and_a_missing_path_is_a_usage_error() {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let dir = tempfile::tempdir().unwrap();
        let db = built_db(dir.path());
        let core = decl_id(CORE);
        let run = |source: &str, input: &str| {
            let mut child = Command::new(env!("CARGO_BIN_EXE_c10r"))
                .current_dir(dir.path())
                .args([
                    "--db",
                    db.to_str().unwrap(),
                    "--json",
                    "similar",
                    "--subjects-from",
                    source,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output().unwrap()
        };
        let input = format!("  {}  \n\n{}\n", core.as_str(), "no_such_symbol_anywhere");
        let out = run("-", &input);
        assert_eq!(out.status.code(), Some(0));
        let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(answer["answers"].as_array().unwrap().len(), 2);
        assert_eq!(answer["answers"][0]["reference"], core.as_str());

        let out = run("no/such/list.txt", "");
        assert_eq!(out.status.code(), Some(ExitCode::Usage.code()));
        assert!(String::from_utf8_lossy(&out.stderr).contains("no/such/list.txt"));
    }

    // _(`-` is refused on a terminal; a list that is not UTF-8 is refused naming standard input)_ —
    // through the reader's seam, since a test cannot attach a terminal.
    #[test]
    fn the_list_reader_refuses_a_terminal_and_invalid_utf8() {
        use silent_cartographer::commands::read_subject_list;

        let err = read_subject_list("-", &mut std::io::empty(), true).expect_err("a terminal is refused");
        assert_eq!(classify(&err), ExitCode::Usage);
        assert!(err.to_string().contains("terminal"), "{err}");

        let err = read_subject_list("-", &mut &[0xff, 0xfe][..], false).expect_err("invalid UTF-8 is refused");
        assert_eq!(classify(&err), ExitCode::Usage);
        assert!(err.to_string().contains("standard input"), "{err}");
    }

    // ---- Mutual nearest pairs ----

    mod mutual {
        use super::*;
        use silent_cartographer::query::search::MutualWalk;

        /// Two sibling methods with identical bodies beside a base method that differs only in a
        /// literal: the siblings are exact clones and type-family relatives of each other, the base
        /// method a variant clone of both.
        pub(super) fn clone_family(language: Language) -> Fixture {
            match language {
                Language::Rust => Fixture {
                    language,
                    doc: "src/fam.rs",
                    source: "\
pub trait Shape {
    fn describe(&self) -> u32 { 1 }
}
pub struct Square;
pub struct Circle;
impl Shape for Square {
    fn describe(&self) -> u32 { 7 }
}
impl Shape for Circle {
    fn describe(&self) -> u32 { 7 }
}
pub fn plain(x: u32) -> u32 { x - 7 }
",
                    decls: vec![
                        decl(SHAPE, SymbolKind::Trait, "trait Shape"),
                        decl(SHAPE_DESCRIBE, SymbolKind::Method, "fn describe(&self) -> u32 { 1 }"),
                        decl(SQUARE, SymbolKind::Type, "struct Square"),
                        decl(CIRCLE, SymbolKind::Type, "struct Circle"),
                        decl(
                            SQUARE_DESCRIBE,
                            SymbolKind::Method,
                            "impl Shape for Square {\n    fn describe(&self) -> u32 { 7 }",
                        ),
                        decl(
                            CIRCLE_DESCRIBE,
                            SymbolKind::Method,
                            "impl Shape for Circle {\n    fn describe(&self) -> u32 { 7 }",
                        ),
                        decl(PLAIN, SymbolKind::Function, "pub fn plain(x: u32)"),
                    ],
                    references: vec![
                        reference(SHAPE, "impl Shape for Square", "Shape"),
                        reference(SHAPE, "impl Shape for Circle", "Shape"),
                        reference(SQUARE, "impl Shape for Square", "Square"),
                        reference(CIRCLE, "impl Shape for Circle", "Circle"),
                    ],
                    external: None,
                },
                Language::Python => Fixture {
                    language,
                    doc: "fam.py",
                    source: "\
class Shape:
    def describe(self):
        return 1


class Square(Shape):
    def describe(self):
        return 7


class Circle(Shape):
    def describe(self):
        return 7


def plain(x):
    return x - 7
",
                    decls: vec![
                        decl(SHAPE, SymbolKind::Type, "class Shape:"),
                        decl(
                            SHAPE_DESCRIBE,
                            SymbolKind::Method,
                            "def describe(self):\n        return 1",
                        ),
                        decl(SQUARE, SymbolKind::Type, "class Square(Shape)"),
                        decl(CIRCLE, SymbolKind::Type, "class Circle(Shape)"),
                        decl(
                            SQUARE_DESCRIBE,
                            SymbolKind::Method,
                            "class Square(Shape):\n    def describe(self):\n        return 7",
                        ),
                        decl(
                            CIRCLE_DESCRIBE,
                            SymbolKind::Method,
                            "class Circle(Shape):\n    def describe(self):\n        return 7",
                        ),
                        decl(PLAIN, SymbolKind::Function, "def plain(x)"),
                    ],
                    references: vec![
                        reference(SHAPE, "class Square(Shape)", "Shape"),
                        reference(SHAPE, "class Circle(Shape)", "Shape"),
                    ],
                    external: None,
                },
            }
        }

        /// [`python_test_relations`] in Rust, where an attribute classifies the test.
        fn rust_test_relations() -> Fixture {
            Fixture {
                language: Language::Rust,
                doc: "src/test_core.rs",
                source: "\
pub fn core(x: u32) -> u32 { x + 1 }
#[test]
fn verifies_core() { core(1); }
pub fn plain(text: &str) -> usize { text.len() }
",
                decls: vec![
                    decl(CORE, SymbolKind::Function, "pub fn core(x: u32)"),
                    decl(CHECKS, SymbolKind::Function, "fn verifies_core()"),
                    decl(PLAIN, SymbolKind::Function, "pub fn plain(text: &str)"),
                ],
                references: vec![reference(CORE, "{ core(1); }", "core")],
                external: None,
            }
        }

        fn walk(
            fixture: &Fixture,
            after: Option<&CanonicalId>,
            limit: Option<usize>,
            excluded: &[RelationLabel],
        ) -> MutualWalk {
            let store = fixture.store();
            engine_over(&store, &fixture.sources())
                .mutual_pairs(after, limit, excluded, Detail::Location, None)
                .unwrap()
        }

        /// The `(lower, upper)` identities of the pairs of a found walk, in answer order.
        fn pair_ids(walk: &MutualWalk) -> Vec<(CanonicalId, CanonicalId)> {
            match &walk.answer.outcome {
                Outcome::Found { results } => results
                    .iter()
                    .map(|pair| {
                        (
                            pair.lower.symbol.canonical_id.clone(),
                            pair.upper.symbol.canonical_id.clone(),
                        )
                    })
                    .collect(),
                other => panic!("expected found pairs, got {other:?}"),
            }
        }

        fn pair_of(path_a: Path, path_b: Path) -> (CanonicalId, CanonicalId) {
            let mut ids = [decl_id(path_a), decl_id(path_b)];
            ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            let [lower, upper] = ids;
            (lower, upper)
        }

        // _(A mutual pair is listed once; a pair carries its marker and labels; a one-sided
        // nearest neighbor is not a pair)_ — two exact-clone siblings are each other's first row;
        // the base method's first row is one of them, whose own first row is the other sibling.
        #[test]
        fn a_mutual_pair_is_listed_once_with_its_marker_and_both_label_sets() {
            for language in support::LANGUAGES {
                let fixture = clone_family(language);
                let walk = walk(&fixture, None, None, &[]);
                let (lower, upper) = pair_of(SQUARE_DESCRIBE, CIRCLE_DESCRIBE);
                let pairs = pair_ids(&walk);
                assert_eq!(
                    pairs
                        .iter()
                        .filter(|pair| **pair == (lower.clone(), upper.clone()))
                        .count(),
                    1,
                    "{language:?}: {pairs:?}"
                );
                let Outcome::Found { results } = &walk.answer.outcome else {
                    unreachable!()
                };
                let sibling = results
                    .iter()
                    .find(|pair| pair.lower.symbol.canonical_id == lower)
                    .unwrap();
                assert_eq!(
                    sibling.clone_certainty,
                    Some(CloneCertainty::ExactClone),
                    "{language:?}"
                );
                for member in [&sibling.lower, &sibling.upper] {
                    let labels: Vec<_> = member.relations.iter().map(|r| r.label).collect();
                    assert_eq!(labels, [RelationLabel::TypeFamily], "{language:?}");
                }

                let base = decl_id(SHAPE_DESCRIBE);
                let store = fixture.store();
                let engine = engine_over(&store, &fixture.sources());
                let first_of = |id: &CanonicalId| {
                    let answer = engine.similar(id.as_str(), Detail::Location, None).unwrap();
                    let Outcome::Found { results } = answer.outcome else {
                        panic!("expected found rows")
                    };
                    results[0].symbol.canonical_id.clone()
                };
                let base_first = first_of(&base);
                assert_ne!(
                    first_of(&base_first),
                    base,
                    "{language:?}: the base's neighbor is one-sided"
                );
                assert!(
                    pairs.iter().all(|(a, b)| *a != base && *b != base),
                    "{language:?}: no pair joins the base method: {pairs:?}"
                );
            }
        }

        // _(An exclusion changes the first rows)_ — a test and the subject it calls are each
        // other's first row until `tests_subject` is excluded.
        #[test]
        fn excluding_tests_subject_drops_a_test_and_subject_pair() {
            for fixture in [python_test_relations(), rust_test_relations()] {
                let lang = fixture.language;
                let pair = pair_of(CORE, CHECKS);
                assert!(
                    pair_ids(&walk(&fixture, None, None, &[])).contains(&pair),
                    "{lang:?}: the test and its subject are a mutual pair"
                );
                assert!(
                    !pair_ids(&walk(&fixture, None, None, &[RelationLabel::TestsSubject])).contains(&pair),
                    "{lang:?}: the exclusion drops the pair"
                );
            }
        }

        // _(A member carries the labels its row has in the other's answer)_ — a test that calls its
        // subject is labeled as using it, and the subject as used by it, whichever sorts first.
        #[test]
        fn each_member_of_a_pair_carries_its_own_direction_of_labels() {
            for fixture in [python_test_relations(), rust_test_relations()] {
                let lang = fixture.language;
                let walk = walk(&fixture, None, None, &[]);
                let Outcome::Found { results } = &walk.answer.outcome else {
                    panic!("{lang:?}: expected found pairs")
                };
                let (lower, upper) = pair_of(CORE, CHECKS);
                let pair = results
                    .iter()
                    .find(|pair| pair.lower.symbol.canonical_id == lower && pair.upper.symbol.canonical_id == upper)
                    .unwrap_or_else(|| panic!("{lang:?}: the test and its subject are a pair"));
                let labels_of = |id: &CanonicalId| -> Vec<RelationLabel> {
                    let member = if pair.lower.symbol.canonical_id == *id {
                        &pair.lower
                    } else {
                        &pair.upper
                    };
                    member.relations.iter().map(|r| r.label).collect()
                };
                assert_eq!(
                    labels_of(&decl_id(CHECKS)),
                    [RelationLabel::UsesSubject, RelationLabel::TestsSubject],
                    "{lang:?}"
                );
                assert_eq!(labels_of(&decl_id(CORE)), [RelationLabel::UsedBySubject], "{lang:?}");
            }
        }

        // _(The exclusion is disclosed on a mutual answer)_ — a repeated label counts once, sorted.
        #[test]
        fn a_mutual_answer_discloses_the_excluded_labels() {
            let dir = tempfile::tempdir().unwrap();
            let db = db_for(&python_relations(), dir.path());
            let all = mutual_json(&db, dir.path(), &[], 0, None);
            assert!(all.get("excluded_relations").is_none(), "no exclusion, no disclosure");
            let excluded = mutual_json(
                &db,
                dir.path(),
                &[
                    RelationLabel::TypeFamily,
                    RelationLabel::TestsSubject,
                    RelationLabel::TestsSubject,
                ],
                0,
                None,
            );
            assert_eq!(
                excluded["excluded_relations"],
                serde_json::json!(["tests_subject", "type_family"])
            );
        }

        // _(The human rendering shows each pair's labels)_ — each member's own labels, with the rule of
        // a test label.
        #[test]
        fn the_human_render_shows_each_pairs_labels() {
            let dir = tempfile::tempdir().unwrap();
            let db = db_for(&rust_test_relations(), dir.path());
            let out = mutual(&db, dir.path(), &[], 0, None, false).unwrap();
            let pair = out
                .lines()
                .find(|line| line.contains("  <->  "))
                .unwrap_or_else(|| panic!("a pair line: {out}"));
            for shown in [
                "[uses_subject]",
                "[used_by_subject]",
                "[tests_subject (rule: test_attribute)]",
            ] {
                assert!(pair.contains(shown), "{shown} is rendered: {out}");
            }
        }

        /// Run `similar --mutual` over `db`.
        fn mutual(
            db: &std::path::Path,
            root: &std::path::Path,
            exclude: &[RelationLabel],
            limit: usize,
            cursor: Option<&str>,
            json: bool,
        ) -> anyhow::Result<String> {
            silent_cartographer::commands::run_similar_mutual(
                db,
                root,
                "rust-analyzer",
                Detail::Location,
                10,
                false,
                exclude,
                limit,
                cursor,
                json,
                false,
                PositionUnit::Line,
            )
        }

        fn mutual_json(
            db: &std::path::Path,
            root: &std::path::Path,
            exclude: &[RelationLabel],
            limit: usize,
            cursor: Option<&str>,
        ) -> serde_json::Value {
            serde_json::from_str(&mutual(db, root, exclude, limit, cursor, true).unwrap()).unwrap()
        }

        fn json_pairs(answer: &serde_json::Value) -> Vec<serde_json::Value> {
            answer["outcome"]["results"].as_array().unwrap().clone()
        }

        fn member_id(pair: &serde_json::Value, side: &str) -> String {
            pair[side]["symbol"]["canonical_id"].as_str().unwrap().to_string()
        }

        // _(Pages resume deterministically)_ — the pages carry no total, each carries a walk token,
        // and their concatenation is the unbounded answer.
        #[test]
        fn paging_to_the_end_yields_the_unbounded_answer() {
            let dir = tempfile::tempdir().unwrap();
            let db = built_db(dir.path());
            let all = mutual_json(&db, dir.path(), &[], 0, None);
            assert!(all.get("page").is_none(), "an unbounded answer carries no page block");
            let expected = json_pairs(&all);
            assert!(expected.len() > 2, "the fixture has more pairs than the page limit");

            let mut pages = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let page = mutual_json(&db, dir.path(), &[], 2, cursor.as_deref());
                pages.extend(json_pairs(&page));
                let info = page.get("page");
                assert!(
                    info.is_none_or(|info| info.get("total").is_none()),
                    "no total: {info:?}"
                );
                cursor = info.and_then(|info| info["cursor"].as_str()).map(str::to_string);
                if cursor.is_none() {
                    break;
                }
            }
            assert_eq!(pages, expected);
        }

        // _(The answer is labeled as estimation)_.
        #[test]
        fn the_answer_carries_the_estimation_marker_and_the_semantic_index_identity() {
            let dir = tempfile::tempdir().unwrap();
            let db = built_db(dir.path());
            let answer = mutual_json(&db, dir.path(), &[], 0, None);
            assert_eq!(answer["classification"], "estimation");
            assert_eq!(answer["semantic_index"]["model_identity"], MODEL_ID);
            assert_eq!(answer["outcome"]["outcome"], "found");
            for pair in json_pairs(&answer) {
                assert!(pair.get("score").is_none() && pair["lower"].get("score").is_none());
            }
        }

        // _(An empty result is typed-empty)_ — a corpus offering no neighbor yields no pair, and the
        // answer keeps the marker.
        #[test]
        fn a_corpus_with_no_pair_is_typed_empty_with_the_marker() {
            let (store, srcs) = store_over("one.rs", "pub fn only() -> u32 { 1 }\n", &["only"]);
            let walk = engine_over(&store, &srcs)
                .mutual_pairs(None, None, &[], Detail::Location, None)
                .unwrap();
            assert_eq!(walk.answer.outcome, Outcome::Empty);
            assert!(matches!(
                walk.answer.classification,
                Some(Classification::Estimation { .. })
            ));
            assert!(walk.resume_after.is_none());
        }

        // _(A token under another exclusion is refused)_ — the token binds the exclusion set, and a
        // page token from a single-subject query is not a walk token.
        #[test]
        fn a_token_issued_under_one_exclusion_is_refused_under_another() {
            let dir = tempfile::tempdir().unwrap();
            let db = built_db(dir.path());
            let page = mutual_json(&db, dir.path(), &[], 1, None);
            let cursor = page["page"]["cursor"]
                .as_str()
                .expect("a truncated page carries a cursor");

            let err = mutual(&db, dir.path(), &[RelationLabel::TestsSubject], 1, Some(cursor), true)
                .expect_err("the token is bound to its exclusion");
            assert_eq!(classify(&err), ExitCode::Usage);
            assert!(err.to_string().contains("different query parameters"), "{err}");

            let single = single_json(&db, dir.path(), decl_id(CORE).as_str(), &[], 1, None);
            let page_token = single["page"]["cursor"].as_str().unwrap();
            let err = mutual(&db, dir.path(), &[], 1, Some(page_token), true)
                .expect_err("a single-subject token does not resume a walk");
            assert_eq!(classify(&err), ExitCode::Usage);
        }

        // _(Each member is the other's first row)_ — checked through the single-subject path under
        // the same exclusion, on every backend.
        #[test]
        fn each_member_of_a_pair_is_the_first_row_of_the_other_through_the_single_path() {
            let fixtures = [
                rust_relations(),
                python_relations(),
                python_test_relations(),
                clone_family(Language::Rust),
                clone_family(Language::Python),
            ];
            for fixture in fixtures {
                let dir = tempfile::tempdir().unwrap();
                let db = db_for(&fixture, dir.path());
                for exclude in [&[][..], &[RelationLabel::TestsSubject][..]] {
                    let answer = mutual_json(&db, dir.path(), exclude, 0, None);
                    let pairs = json_pairs(&answer);
                    if exclude.is_empty() {
                        assert!(!pairs.is_empty(), "{:?}: a corpus has a nearest pair", fixture.language);
                    }
                    for pair in pairs {
                        let (lower, upper) = (member_id(&pair, "lower"), member_id(&pair, "upper"));
                        assert!(lower < upper);
                        for (subject, expected) in [(&lower, &upper), (&upper, &lower)] {
                            let single = single_json(&db, dir.path(), subject, exclude, 1, None);
                            let first = &single["outcome"]["results"][0]["symbol"]["canonical_id"];
                            assert_eq!(first, expected.as_str(), "{:?}: {subject}", fixture.language);
                        }
                    }
                }
            }
        }

        // _(The human rendering)_ — one line per pair naming both members, and a page disclosure
        // without a total.
        #[test]
        fn the_human_render_lists_one_line_per_pair_and_discloses_no_total() {
            let dir = tempfile::tempdir().unwrap();
            let db = built_db(dir.path());
            let out = mutual(&db, dir.path(), &[], 1, None, false).unwrap();
            assert_eq!(out.lines().filter(|line| line.contains("  <->  ")).count(), 1, "{out}");
            assert!(out.contains("page 0: 1 results (truncated)"), "{out}");
            assert!(out.contains("resume with --cursor"), "{out}");
        }

        // _(`--mutual` takes no subject)_ — it conflicts with a reference, `--at`, and `--subjects-from`.
        #[test]
        fn mutual_conflicts_with_every_subject_form() {
            let dir = tempfile::tempdir().unwrap();
            let db = built_db(dir.path());
            for extra in [
                &["core"][..],
                &["--at", "src/rel.rs:1"][..],
                &["--subjects-from", "-"][..],
            ] {
                let out = c10r(&db)
                    .current_dir(dir.path())
                    .args(["similar", "--mutual"])
                    .args(extra)
                    .output()
                    .unwrap();
                assert_eq!(out.status.code(), Some(ExitCode::Usage.code()), "{extra:?}");
            }
        }
    }
}
