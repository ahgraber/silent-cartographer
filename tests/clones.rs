//! `clones`: corpus clone groups, a subject's whole-symbol clones, and the token-equivalence labeling
//! both carry, on every supported language.

mod support;

use std::path::Path;

use serde_json::Value;
use silent_cartographer::cli::PositionUnit;
use silent_cartographer::graph::content_hash;
use silent_cartographer::graph::syntax::Language;
use silent_cartographer::identity::SegmentKind;
use silent_cartographer::query::output::Outcome;
use silent_cartographer::query::{Detail, QueryEngine};
use silent_cartographer::semantic::model::SymbolKind;
use support::{Decl, LANGUAGES, backend_provenance, decl_id};

type Segments = &'static [(&'static str, SegmentKind)];

const ALPHA_1: Segments = &[("m1", SegmentKind::Module), ("alpha", SegmentKind::Method)];
const ALPHA_2: Segments = &[("m2", SegmentKind::Module), ("alpha", SegmentKind::Method)];
const ALPHA_3: Segments = &[("m3", SegmentKind::Module), ("alpha", SegmentKind::Method)];
const BRAVO_1: Segments = &[("n1", SegmentKind::Module), ("bravo", SegmentKind::Method)];
const BRAVO_2: Segments = &[("n2", SegmentKind::Module), ("bravo", SegmentKind::Method)];
const CHARLIE: Segments = &[("charlie", SegmentKind::Method)];
const OMEGA: Segments = &[("omega", SegmentKind::Method)];
const SIGMA: Segments = &[("sigma", SegmentKind::Method)];
const HOLDER: Segments = &[("Holder", SegmentKind::Type)];
const HELD: Segments = &[("Holder", SegmentKind::Type), ("held", SegmentKind::Method)];

/// A source in one language holding: `alpha` three times differing only in layout and comments (one
/// exact group), a longer `bravo` twice plus a consistently renamed `charlie` (one variant group with
/// an exact pair), a unique `omega`, and a container `Holder` whose method `held` is unique.
struct Fixture {
    language: Language,
    doc: &'static str,
    source: &'static str,
    decls: Vec<Decl<'static>>,
    /// A byte offset inside the first `alpha`'s body.
    inside_alpha: usize,
    /// A byte offset inside `held`'s body.
    inside_held: usize,
}

fn decl(segments: Segments, kind: SymbolKind, within: &'static str) -> Decl<'static> {
    Decl { segments, kind, within }
}

fn fixtures() -> Vec<Fixture> {
    LANGUAGES
        .into_iter()
        .map(|language| match language {
            Language::Rust => rust_fixture(),
            Language::Python => python_fixture(),
        })
        .collect()
}

fn rust_fixture() -> Fixture {
    let source = "\
// copy one
pub fn alpha(x: u32, y: u32) -> u32 {
    let total = x + y;
    total * 3
}

// copy two
pub fn alpha(x: u32, y: u32) -> u32 { let total = x + y; total * 3 }

// copy three
pub fn alpha(x: u32, y: u32) -> u32 {
    // sum first
    let total = x + y;
    total * 3
}

// first long
pub fn bravo(items: &[u32], scale: u32) -> u32 {
    let mut total = 0;
    for item in items {
        if *item > scale { total += item * scale; } else { total += item + scale; }
    }
    let result = total * 2 + scale;
    result
}

// second long
pub fn bravo(items: &[u32], scale: u32) -> u32 {
    // accumulate
    let mut total = 0;
    for item in items { if *item > scale { total += item * scale; } else { total += item + scale; } }
    let result = total * 2 + scale;
    result
}

pub fn charlie(values: &[u32], factor: u32) -> u32 {
    let mut acc = 0;
    for elem in values {
        if *elem > factor { acc += elem * factor; } else { acc += elem + factor; }
    }
    let outcome = acc * 2 + factor;
    outcome
}

pub fn omega(text: &str) -> usize { text.len() }

pub trait Holder {
    fn held(&self) -> u32 { 41 }
}
";
    Fixture {
        language: Language::Rust,
        doc: "src/clones.rs",
        source,
        decls: vec![
            decl(ALPHA_1, SymbolKind::Function, "// copy one\npub fn alpha("),
            decl(ALPHA_2, SymbolKind::Function, "// copy two\npub fn alpha("),
            decl(ALPHA_3, SymbolKind::Function, "// copy three\npub fn alpha("),
            decl(BRAVO_1, SymbolKind::Function, "// first long\npub fn bravo("),
            decl(BRAVO_2, SymbolKind::Function, "// second long\npub fn bravo("),
            decl(CHARLIE, SymbolKind::Function, "pub fn charlie("),
            decl(OMEGA, SymbolKind::Function, "pub fn omega("),
            decl(HOLDER, SymbolKind::Trait, "pub trait Holder"),
            decl(HELD, SymbolKind::Method, "fn held(&self)"),
        ],
        inside_alpha: source.find("let total = x + y;").unwrap() + 4,
        inside_held: source.find("{ 41 }").unwrap() + 2,
    }
}

fn python_fixture() -> Fixture {
    let source = "\
# copy one
def alpha(x, y):
    total = x + y
    return total * 3


# copy two
def alpha(x, y):

    total = x + y

    return total * 3


# copy three
def alpha(x, y):
    # sum first
    total = x + y
    return total * 3


# first long
def bravo(items, scale):
    total = 0
    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale
    result = total * 2 + scale
    return result


# second long
def bravo(items, scale):
    # accumulate
    total = 0

    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale
    result = total * 2 + scale
    return result


def charlie(values, factor):
    acc = 0
    for elem in values:
        if elem > factor:
            acc += elem * factor
        else:
            acc += elem + factor
    outcome = acc * 2 + factor
    return outcome


def omega(text):
    return len(text)


class Holder:
    def held(self):
        return 41
";
    Fixture {
        language: Language::Python,
        doc: "clones.py",
        source,
        decls: vec![
            decl(ALPHA_1, SymbolKind::Function, "# copy one\ndef alpha("),
            decl(ALPHA_2, SymbolKind::Function, "# copy two\ndef alpha("),
            decl(ALPHA_3, SymbolKind::Function, "# copy three\ndef alpha("),
            decl(BRAVO_1, SymbolKind::Function, "# first long\ndef bravo("),
            decl(BRAVO_2, SymbolKind::Function, "# second long\ndef bravo("),
            decl(CHARLIE, SymbolKind::Function, "def charlie("),
            decl(OMEGA, SymbolKind::Function, "def omega("),
            decl(HOLDER, SymbolKind::Type, "class Holder:"),
            decl(HELD, SymbolKind::Method, "def held(self)"),
        ],
        inside_alpha: source.find("total = x + y").unwrap() + 4,
        inside_held: source.find("return 41").unwrap() + 2,
    }
}

impl Fixture {
    fn sources(&self) -> Vec<(String, String)> {
        vec![(self.doc.to_string(), self.source.to_string())]
    }

    fn index(&self) -> silent_cartographer::semantic::model::ExtractedIndex {
        support::declared_index(self.language, self.doc, self.source, &self.decls)
    }

    fn store(&self) -> silent_cartographer::graph::store::GraphStore {
        support::declared_store(self.language, self.doc, self.source, &self.decls)
    }

    /// The fixture built to an index on disk under `dir`.
    fn db(&self, dir: &Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &self.index(), &self.sources()).unwrap();
        db
    }

    /// `offset` as the 1-based `path:line:column` the `--at` flag takes.
    fn at(&self, offset: usize) -> String {
        let (line, column) = support::line_col(self.source, offset);
        format!("{}:{}:{}", self.doc, line + 1, column + 1)
    }
}

/// The `clones` answer as JSON through the command handler.
fn clones(
    db: &Path,
    root: &Path,
    reference: Option<&str>,
    at: Option<&str>,
    limit: usize,
    cursor: Option<&str>,
) -> Value {
    let out = silent_cartographer::commands::run_clones(
        db,
        root,
        "rust-analyzer",
        reference,
        at,
        limit,
        cursor,
        true,
        false,
        PositionUnit::Line,
    )
    .unwrap();
    serde_json::from_str(&out).unwrap()
}

fn ids(members: &Value, key: &str) -> Vec<String> {
    members
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member[key]["canonical_id"].as_str().unwrap().to_string())
        .collect()
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

fn id_strings(paths: &[Segments]) -> Vec<String> {
    sorted(paths.iter().map(|path| decl_id(path).as_str().to_string()).collect())
}

fn groups_of(answer: &Value) -> &Vec<Value> {
    answer["outcome"]["results"].as_array().unwrap()
}

fn group_with(answer: &Value, member: Segments) -> &Value {
    let id = decl_id(member);
    groups_of(answer)
        .iter()
        .find(|group| ids(&group["members"], "symbol").contains(&id.as_str().to_string()))
        .unwrap_or_else(|| panic!("{id} is in a group"))
}

// _(Exact copies form an exact group)_ — three functions differing only in layout and comments.
#[test]
fn exact_copies_form_one_exact_group() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), None, None, 0, None);
        let group = group_with(&answer, ALPHA_1);
        assert_eq!(group["certainty"], "exact_clone", "{:?}", fixture.language);
        assert_eq!(
            sorted(ids(&group["members"], "symbol")),
            id_strings(&[ALPHA_1, ALPHA_2, ALPHA_3]),
            "{:?}",
            fixture.language
        );
        assert!(group["members"].as_array().unwrap().iter().all(|m| m["exact_set"] == 1));
    }
}

// _(A variant group shows its exact subsets)_ — two exact copies and a renamed third share a group,
// and the exact pair share an exact set the renamed copy is outside.
#[test]
fn a_variant_group_identifies_its_exact_subset() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), None, None, 0, None);
        let group = group_with(&answer, BRAVO_1);
        assert_eq!(group["certainty"], "variant_clone", "{:?}", fixture.language);
        let members = group["members"].as_array().unwrap();
        let set_of = |path: Segments| {
            let id = decl_id(path);
            members
                .iter()
                .find(|m| m["symbol"]["canonical_id"] == id.as_str())
                .unwrap_or_else(|| panic!("{id} is a member"))["exact_set"]
                .as_u64()
                .unwrap()
        };
        assert_eq!(set_of(BRAVO_1), set_of(BRAVO_2), "{:?}", fixture.language);
        assert_ne!(set_of(BRAVO_1), set_of(CHARLIE), "{:?}", fixture.language);
        // Sets are numbered from 1 in the order their first member appears, members in identity order.
        assert_eq!(members[0]["exact_set"], 1);
        assert_eq!(members.len(), 3);
    }
}

// _(A symbol with no clone is not listed)_ — and no group is a lone symbol.
#[test]
fn a_unique_function_is_in_no_group() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), None, None, 0, None);
        let omega = decl_id(OMEGA);
        for group in groups_of(&answer) {
            assert!(group["members"].as_array().unwrap().len() >= 2);
            assert!(!ids(&group["members"], "symbol").contains(&omega.as_str().to_string()));
        }
    }
}

// _(Longer clones are listed first)_ — the long-function group precedes the short-function group.
#[test]
fn a_long_function_group_precedes_a_short_one() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), None, None, 0, None);
        let groups = groups_of(&answer);
        assert_eq!(groups.len(), 2, "{:?}", fixture.language);
        assert!(groups[0]["token_count"].as_u64() > groups[1]["token_count"].as_u64());
        assert_eq!(groups[0], *group_with(&answer, BRAVO_1));
        assert_eq!(groups[1], *group_with(&answer, ALPHA_1));
    }
}

// _(No shared key is typed absence)_ — a corpus without a shared key is typed-empty and keeps the
// grade.
#[test]
fn a_corpus_with_no_shared_key_is_typed_empty_and_keeps_the_marker() {
    let fixture = rust_fixture();
    let source = "pub fn omega(text: &str) -> usize { text.len() }\npub fn sigma(n: u32) -> u32 { n + 1 }\n";
    let doc = fixture.doc;
    let decls = vec![
        decl(OMEGA, SymbolKind::Function, "pub fn omega("),
        decl(SIGMA, SymbolKind::Function, "pub fn sigma("),
    ];
    let index = support::declared_index(Language::Rust, doc, source, &decls);
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    silent_cartographer::commands::build_from_index(
        &db,
        "test-ws",
        dir.path(),
        &index,
        &[(doc.into(), source.into())],
    )
    .unwrap();
    let answer = clones(&db, dir.path(), None, None, 0, None);
    assert_eq!(answer["outcome"]["outcome"], "empty");
    assert_eq!(answer["grade"], "token_equivalence");
}

// _(Groups page deterministically)_ — paging to the end yields the unbounded answer, and the limit
// counts groups.
#[test]
fn paging_the_groups_to_the_end_yields_the_unbounded_answer() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let whole = clones(&db, dir.path(), None, None, 0, None);
        let mut paged: Vec<Value> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = clones(&db, dir.path(), None, None, 1, cursor.as_deref());
            assert_eq!(page["page"]["total"], groups_of(&whole).len());
            paged.extend(groups_of(&page).clone());
            match page["page"]["cursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
        }
        assert_eq!(&paged, groups_of(&whole), "{:?}", fixture.language);
    }
}

// _(Tokens cannot cross queries)_ — a group token is refused for a subject query.
#[test]
fn a_group_token_is_refused_on_a_subject_query() {
    let fixture = rust_fixture();
    let dir = tempfile::tempdir().unwrap();
    let db = fixture.db(dir.path());
    let page = clones(&db, dir.path(), None, None, 1, None);
    let cursor = page["page"]["cursor"].as_str().unwrap();
    let err = silent_cartographer::commands::run_clones(
        &db,
        dir.path(),
        "rust-analyzer",
        Some("m1::alpha"),
        None,
        1,
        Some(cursor),
        true,
        false,
        PositionUnit::Line,
    )
    .expect_err("the token is bound to its query");
    assert!(err.to_string().contains("different query parameters"), "{err}");
}

// _(Whole-symbol clones)_ — the other symbols sharing the subject's key, exact before variant, the
// subject excluded.
#[test]
fn whole_symbol_clones_are_ordered_exact_first_and_exclude_the_subject() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let answer = clones(&db, dir.path(), Some("n1::bravo"), None, 0, None);
        let rows = groups_of(&answer);
        assert_eq!(
            ids(&Value::Array(rows.clone()), "symbol"),
            vec![
                decl_id(BRAVO_2).as_str().to_string(),
                decl_id(CHARLIE).as_str().to_string()
            ],
            "{:?}",
            fixture.language
        );
        assert_eq!(rows[0]["kind"], "whole_symbol");
        assert_eq!(rows[0]["certainty"], "exact_clone");
        assert_eq!(rows[1]["certainty"], "variant_clone");
        assert!(rows[0]["location"]["document_path"].is_string());
    }
}

// _(A container has no comparable content)_ — a distinct typed outcome that keeps the grade.
#[test]
fn a_container_subject_is_not_comparable_and_keeps_the_marker() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let answer = clones(&db, dir.path(), Some("Holder"), None, 0, None);
        assert_eq!(answer["outcome"]["outcome"], "not_comparable", "{:?}", fixture.language);
        assert_eq!(answer["grade"], "token_equivalence");
        assert!(answer["guaranteed_fragment_length"].is_number());

        // Another command's answer about the same container is never `not_comparable`.
        let store = fixture.store();
        let engine = QueryEngine::new(
            &store,
            backend_provenance(fixture.language),
            content_hash(&fixture.sources()),
            None,
        );
        let similar = engine.similar("Holder", Detail::Location, None).unwrap();
        assert!(!matches!(similar.outcome, Outcome::NotComparable));
    }
}

// _(A subject with keys but no clone is typed-empty)_ — distinct from `not_comparable`.
#[test]
fn a_function_with_no_clone_is_typed_empty_and_keeps_the_marker() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("omega"), None, 0, None);
        assert_eq!(answer["outcome"]["outcome"], "empty", "{:?}", fixture.language);
        assert_eq!(answer["grade"], "token_equivalence");
    }
}

// _(A position takes the enclosing symbol)_ — inside a method the method, never its container.
#[test]
fn at_inside_a_symbol_takes_the_innermost_symbol() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let in_alpha = clones(&db, dir.path(), None, Some(&fixture.at(fixture.inside_alpha)), 0, None);
        assert_eq!(
            sorted(ids(&Value::Array(groups_of(&in_alpha).clone()), "symbol")),
            id_strings(&[ALPHA_2, ALPHA_3]),
            "{:?}",
            fixture.language
        );
        let in_held = clones(&db, dir.path(), None, Some(&fixture.at(fixture.inside_held)), 0, None);
        assert_eq!(
            in_held["outcome"]["outcome"], "empty",
            "{:?}: the method, not its container, is the subject",
            fixture.language
        );
    }
}

// _(An ambiguous or unresolved subject carries no marker)_ — resolution ends before the clone
// evidence is read.
#[test]
fn ambiguous_and_unresolved_subjects_carry_no_marker() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let ambiguous = clones(&db, dir.path(), Some("alpha"), None, 0, None);
        assert_eq!(ambiguous["outcome"]["outcome"], "ambiguous");
        assert_eq!(ambiguous["outcome"]["candidates"].as_array().unwrap().len(), 3);
        let absent = clones(&db, dir.path(), Some("no_such_symbol"), None, 0, None);
        assert_eq!(absent["outcome"]["outcome"], "absent");
        for answer in [&ambiguous, &absent] {
            assert!(answer.get("grade").is_none());
            assert!(answer.get("guaranteed_fragment_length").is_none());
        }
    }
}

// _(The machine answer carries the marker and the length)_ — and no estimation marker, no score.
#[test]
fn answers_carry_the_grade_and_length_and_no_estimation_marker() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        for answer in [
            clones(&db, dir.path(), None, None, 0, None),
            clones(&db, dir.path(), Some("n1::bravo"), None, 0, None),
        ] {
            assert_eq!(answer["grade"], "token_equivalence");
            assert_eq!(
                answer["guaranteed_fragment_length"],
                silent_cartographer::graph::clone::guaranteed_fragment_length(
                    silent_cartographer::graph::clone::FRAGMENT_KGRAM_LENGTH,
                    silent_cartographer::graph::clone::FRAGMENT_WINDOW
                )
            );
            assert!(answer.get("classification").is_none());
            assert!(answer.get("semantic_index").is_none());
        }
    }
}

// _(The length comes from the store)_ — a store recording other parameters reports its own length.
#[test]
fn the_guaranteed_length_comes_from_the_recorded_parameters() {
    let fixture = rust_fixture();
    let dir = tempfile::tempdir().unwrap();
    let db = fixture.db(dir.path());
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE index_metadata SET fragment_kgram_length = 5, fragment_window = 7 WHERE id = 1",
            [],
        )
        .unwrap();
    drop(connection);
    for answer in [
        clones(&db, dir.path(), None, None, 0, None),
        clones(&db, dir.path(), Some("omega"), None, 0, None),
        clones(&db, dir.path(), Some("Holder"), None, 0, None),
    ] {
        assert_eq!(answer["guaranteed_fragment_length"], 11);
    }
}

// _(The marker composes with staleness)_ — a stale answer carries both.
#[test]
fn a_stale_answer_carries_both_the_staleness_flag_and_the_marker() {
    let fixture = rust_fixture();
    let store = fixture.store();
    let engine = QueryEngine::new(
        &store,
        backend_provenance(Language::Rust),
        "sources-changed-since-indexing".to_string(),
        None,
    );
    let answer = engine.clone_groups().unwrap();
    assert!(answer.stale);
    assert!(answer.token_equivalence.is_some());
}

// _(The human render states token equivalence)_ — and shows the guaranteed length.
#[test]
fn the_human_render_states_token_equivalence_and_the_length() {
    let fixture = rust_fixture();
    let dir = tempfile::tempdir().unwrap();
    let db = fixture.db(dir.path());
    for (reference, expected) in [
        (None, "group of 3 symbols, "),
        (Some("n1::bravo"), "exact clone: identical token sequence"),
        (Some("Holder"), "not comparable"),
        (Some("omega"), "empty: no clones found"),
    ] {
        let out = silent_cartographer::commands::run_clones(
            &db,
            dir.path(),
            "rust-analyzer",
            reference,
            None,
            0,
            None,
            false,
            false,
            PositionUnit::Line,
        )
        .unwrap();
        assert!(
            out.contains("identical token structure, not proof of identical behavior"),
            "{out}"
        );
        assert!(out.contains("guaranteed fragment length: 40 tokens"), "{out}");
        assert!(out.contains(expected), "{out}");
    }
}
