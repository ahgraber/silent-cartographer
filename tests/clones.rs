//! `clones`: corpus clone groups, a subject's whole-symbol clones and shared fragments, and the
//! token-equivalence labeling they carry, on every supported language.

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
        let length = silent_cartographer::graph::clone::guaranteed_fragment_length(
            silent_cartographer::graph::clone::FRAGMENT_KGRAM_LENGTH,
            silent_cartographer::graph::clone::FRAGMENT_WINDOW,
        );
        assert!(
            out.contains(&format!("guaranteed fragment length: {length} tokens")),
            "{out}"
        );
        assert!(out.contains(expected), "{out}");
    }
}

const F1_SHORTER: Segments = &[("f1", SegmentKind::Module), ("shorter", SegmentKind::Method)];
const F2_SHORTER: Segments = &[("f2", SegmentKind::Module), ("shorter", SegmentKind::Method)];
const LEDGER: Segments = &[("Ledger", SegmentKind::Type)];
const LONGER: Segments = &[("Ledger", SegmentKind::Type), ("longer", SegmentKind::Method)];
const RENAMED: Segments = &[("renamed", SegmentKind::Method)];
const MERGED: Segments = &[("merged", SegmentKind::Method)];
const BRIEF: Segments = &[("brief", SegmentKind::Method)];
const TWICE: Segments = &[("twice", SegmentKind::Method)];
const ALSO_TWICE: Segments = &[("also_twice", SegmentKind::Method)];
const D1_DOCUMENTED: Segments = &[("d1", SegmentKind::Module), ("documented", SegmentKind::Method)];
const D2_DOCUMENTED: Segments = &[("d2", SegmentKind::Module), ("documented", SegmentKind::Method)];

/// A source in one language for the shared-fragment answer. `f1::shorter` holds a block longer than
/// the guaranteed length that `Ledger.longer` holds exactly inside a longer method, `renamed` holds
/// consistently renamed, and `merged` holds with two identifiers merged into one name;
/// `f2::shorter` is an exact whole copy of `f1::shorter`, and `brief` shares only a short run with
/// it. `twice` holds a second block twice over, which `also_twice` holds renamed, twice over. The
/// two `documented` functions share a run that their differing documentation sits inside.
struct FragmentFixture {
    language: Language,
    doc: &'static str,
    source: &'static str,
    decls: Vec<Decl<'static>>,
    /// The first and last line of the shared block in `f1::shorter`.
    subject_block: (u64, u64),
    /// The first and last line of the shared block in `Ledger.longer`.
    longer_block: (u64, u64),
}

fn fragment_fixtures() -> Vec<FragmentFixture> {
    LANGUAGES
        .into_iter()
        .map(|language| match language {
            Language::Rust => rust_fragment_fixture(),
            Language::Python => python_fragment_fixture(),
        })
        .collect()
}

fn rust_fragment_fixture() -> FragmentFixture {
    let source = "\
// subject
pub fn shorter(items: &[u32], scale: u32) -> u32 {
    let mut total = 0;
    for item in items {
        if *item > scale { total += item * scale; } else { total += item + scale; }
    }
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    bonus + result
}

// exact copy
pub fn shorter(items: &[u32], scale: u32) -> u32 {
    let mut total = 0;
    for item in items {
        if *item > scale { total += item * scale; } else { total += item + scale; }
    }
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    bonus + result
}

pub struct Ledger;

impl Ledger {
    pub fn longer(&self, items: &[u32], scale: u32, label: &str) -> usize {
        println!(\"{}\", label);
        let width = label.len();
        let mut total = 0;
        for item in items {
            if *item > scale { total += item * scale; } else { total += item + scale; }
        }
        let result = total * 2 + scale;
        let bonus = result / 3 + scale * 5;
        eprintln!(\"{} {}\", width, bonus);
        width + (bonus + result) as usize
    }
}

pub fn renamed(values: &[u32], factor: u32, tag: &str) -> usize {
    let size = tag.len();
    let mut acc = 0;
    for elem in values {
        if *elem > factor { acc += elem * factor; } else { acc += elem + factor; }
    }
    let outcome = acc * 2 + factor;
    let extra = outcome / 3 + factor * 5;
    size + (extra + outcome) as usize
}

pub fn merged(xs: &[u32], k: u32, tag: &str) -> usize {
    let size = tag.len();
    let mut v = 0;
    for v in xs {
        if *v > k { v += v * k; } else { v += v + k; }
    }
    let r = v * 2 + k;
    let b = r / 3 + k * 5;
    size + (b + r) as usize
}

pub fn brief(items: &[u32], scale: u32, names: &[&str]) -> usize {
    let mut total = 0;
    for item in items { total -= item / scale; }
    let mut count = names.len();
    while count > 10 { count -= 3; println!(\"{}\", count); }
    count + total as usize
}

pub fn twice(rows: &mut Vec<String>, limit: usize) {
    rows.retain(|row| !row.is_empty() && row.len() < limit);
    rows.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    rows.retain(|row| !row.is_empty() && row.len() < limit);
    rows.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
}

pub fn also_twice(lines: &mut Vec<String>, cap: usize, extra: bool) {
    lines.retain(|line| !line.is_empty() && line.len() < cap);
    lines.sort_by(|p, q| q.len().cmp(&p.len()).then_with(|| p.cmp(q)));
    lines.retain(|line| !line.is_empty() && line.len() < cap);
    lines.sort_by(|p, q| q.len().cmp(&p.len()).then_with(|| p.cmp(q)));
    if extra { lines.clear(); }
}

// doc one
pub fn documented(items: &[u32], scale: u32, offset: u32, limit: u32) -> u32 {
    /// Scale the items.
    let mut total = offset + limit * scale;
    for item in items { total += item * scale; }
    total = total % (limit + 1);
    total
}

// doc two
pub fn documented(items: &[u32], scale: u32, offset: u32, limit: u32) -> u32 {
    /// Sum the items under a different description.
    let mut total = offset + limit * scale;
    for item in items { total += item * scale; }
    total = total % (limit + 1);
    println!(\"{}\", total);
    total
}
";
    let decls = vec![
        decl(F1_SHORTER, SymbolKind::Function, "// subject\npub fn shorter("),
        decl(F2_SHORTER, SymbolKind::Function, "// exact copy\npub fn shorter("),
        decl(LEDGER, SymbolKind::Type, "pub struct Ledger"),
        decl(LONGER, SymbolKind::Method, "pub fn longer("),
        decl(RENAMED, SymbolKind::Function, "pub fn renamed("),
        decl(MERGED, SymbolKind::Function, "pub fn merged("),
        decl(BRIEF, SymbolKind::Function, "pub fn brief("),
        decl(TWICE, SymbolKind::Function, "pub fn twice("),
        decl(ALSO_TWICE, SymbolKind::Function, "pub fn also_twice("),
        decl(D1_DOCUMENTED, SymbolKind::Function, "// doc one\npub fn documented("),
        decl(D2_DOCUMENTED, SymbolKind::Function, "// doc two\npub fn documented("),
    ];
    FragmentFixture {
        language: Language::Rust,
        doc: "src/fragments.rs",
        source,
        decls,
        subject_block: (3, 8),
        longer_block: (29, 34),
    }
}

fn python_fragment_fixture() -> FragmentFixture {
    let source = "\
# subject
def shorter(items, scale):
    total = 0
    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    spare = bonus - result * 2
    return bonus + result + spare


# exact copy
def shorter(items, scale):
    total = 0
    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    spare = bonus - result * 2
    return bonus + result + spare


class Ledger:
    def longer(self, items, scale, label):
        print(label)
        width = len(label)
        total = 0
        for item in items:
            if item > scale:
                total += item * scale
            else:
                total += item + scale
        result = total * 2 + scale
        bonus = result // 3 + scale * 5
        spare = bonus - result * 2
        print(width, bonus)
        return width + bonus + result + spare


def renamed(values, factor, tag):
    size = len(tag)
    acc = 0
    for elem in values:
        if elem > factor:
            acc += elem * factor
        else:
            acc += elem + factor
    outcome = acc * 2 + factor
    extra = outcome // 3 + factor * 5
    rest = extra - outcome * 2
    print(size)
    return size


def merged(xs, k, tag):
    size = len(tag)
    v = 0
    for v in xs:
        if v > k:
            v += v * k
        else:
            v += v + k
    r = v * 2 + k
    b = r // 3 + k * 5
    s = b - r * 2
    print(size)
    return size


def brief(items, scale, names):
    total = 0
    for item in items:
        total -= item / scale
    count = len(names)
    while count > 10:
        count -= 3
        print(count)
    return count + total


def twice(rows, limit):
    rows = [row for row in rows if row and len(row) < limit]
    rows.sort(key=lambda row: (-len(row), row))
    rows = [row for row in rows if row and len(row) < limit]
    rows.sort(key=lambda row: (-len(row), row))


def also_twice(lines, cap, extra):
    lines = [line for line in lines if line and len(line) < cap]
    lines.sort(key=lambda line: (-len(line), line))
    lines = [line for line in lines if line and len(line) < cap]
    lines.sort(key=lambda line: (-len(line), line))
    if extra:
        lines.clear()


# doc one
def documented(items, scale, offset, limit, width, height):
    \"\"\"Scale the items.\"\"\"
    total = offset + limit * width - height
    for item in items:
        total += item * scale
    total = total % (limit + 1)
    return total


# doc two
def documented(items, scale, offset, limit, width, height):
    \"\"\"Sum the items under a different description.\"\"\"
    total = offset + limit * width - height
    for item in items:
        total += item * scale
    total = total % (limit + 1)
    print(total)
    return total
";
    let decls = vec![
        decl(F1_SHORTER, SymbolKind::Function, "# subject\ndef shorter("),
        decl(F2_SHORTER, SymbolKind::Function, "# exact copy\ndef shorter("),
        decl(LEDGER, SymbolKind::Type, "class Ledger:"),
        decl(LONGER, SymbolKind::Method, "def longer("),
        decl(RENAMED, SymbolKind::Function, "def renamed("),
        decl(MERGED, SymbolKind::Function, "def merged("),
        decl(BRIEF, SymbolKind::Function, "def brief("),
        decl(TWICE, SymbolKind::Function, "def twice("),
        decl(ALSO_TWICE, SymbolKind::Function, "def also_twice("),
        decl(D1_DOCUMENTED, SymbolKind::Function, "# doc one\ndef documented("),
        decl(D2_DOCUMENTED, SymbolKind::Function, "# doc two\ndef documented("),
    ];
    FragmentFixture {
        language: Language::Python,
        doc: "fragments.py",
        source,
        decls,
        subject_block: (3, 11),
        longer_block: (33, 41),
    }
}

impl FragmentFixture {
    fn sources(&self) -> Vec<(String, String)> {
        vec![(self.doc.to_string(), self.source.to_string())]
    }

    /// The fixture built to an index on disk under `dir`.
    fn db(&self, dir: &Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        let index = support::declared_index(self.language, self.doc, self.source, &self.decls);
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &index, &self.sources()).unwrap();
        db
    }
}

/// The rows of a subject answer naming `other`.
fn rows_naming(answer: &Value, other: Segments) -> Vec<&Value> {
    let id = decl_id(other);
    groups_of(answer)
        .iter()
        .filter(|row| row["symbol"]["canonical_id"] == id.as_str())
        .collect()
}

/// The fragment rows of a subject answer naming `other`.
fn fragments_naming(answer: &Value, other: Segments) -> Vec<&Value> {
    rows_naming(answer, other)
        .into_iter()
        .filter(|row| row["kind"] == "fragment")
        .collect()
}

/// A run's first and last line.
fn lines_of(run: &Value) -> (u64, u64) {
    (run["start_line"].as_u64().unwrap(), run["end_line"].as_u64().unwrap())
}

// _(Shared lines inside a longer method are found)_ — from the shorter function, the method's run
// covers the copied block.
#[test]
fn a_block_copied_into_a_longer_method_is_found_from_the_shorter_function() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("f1::shorter"), None, 0, None);
        let rows = fragments_naming(&answer, LONGER);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        let fragment = &rows[0]["fragment"];
        assert_eq!(rows[0]["certainty"], "exact_clone");
        assert_eq!(
            lines_of(&fragment["subject_run"]),
            fixture.subject_block,
            "{:?}",
            fixture.language
        );
        assert_eq!(
            lines_of(&fragment["other_run"]),
            fixture.longer_block,
            "{:?}",
            fixture.language
        );
        assert_eq!(fragment["subject_run"]["document_path"], fixture.doc);
        assert!(fragment["token_length"].as_u64() >= answer["guaranteed_fragment_length"].as_u64());
    }
}

// _(A renamed block is a variant fragment)_
#[test]
fn a_renamed_block_is_a_variant_fragment() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("f1::shorter"), None, 0, None);
        let rows = fragments_naming(&answer, RENAMED);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        assert_eq!(rows[0]["certainty"], "variant_clone");
        assert_eq!(lines_of(&rows[0]["fragment"]["subject_run"]), fixture.subject_block);
    }
}

// _(An inconsistently renamed block is not reported as one run)_ — the merged copy shares the whole
// placeholder run, but no consistent run of the guaranteed length.
#[test]
fn an_inconsistently_renamed_block_is_not_reported() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("f1::shorter"), None, 0, None);
        assert!(
            rows_naming(&answer, MERGED).is_empty(),
            "{:?}: {answer}",
            fixture.language
        );
    }
}

// _(A block shorter than the guaranteed length is not reported)_ — from either side, so a subject
// whose only shared runs are short is typed-empty.
#[test]
fn a_shorter_shared_run_is_not_reported() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let from_shorter = clones(&db, dir.path(), Some("f1::shorter"), None, 0, None);
        assert!(rows_naming(&from_shorter, BRIEF).is_empty(), "{:?}", fixture.language);
        let from_brief = clones(&db, dir.path(), Some("brief"), None, 0, None);
        assert_eq!(
            from_brief["outcome"]["outcome"], "empty",
            "{:?}: {from_brief}",
            fixture.language
        );
    }
}

// _(A whole-symbol clone has no fragment rows)_ — and whole-symbol rows precede fragments, which
// run longest first.
#[test]
fn an_exact_whole_copy_has_no_fragment_rows() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("f1::shorter"), None, 0, None);
        let rows = rows_naming(&answer, F2_SHORTER);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        assert_eq!(rows[0]["kind"], "whole_symbol");
        assert_eq!(rows[0]["certainty"], "exact_clone");
        assert!(rows[0].get("fragment").is_none());
        let results = groups_of(&answer);
        assert_eq!(results[0]["kind"], "whole_symbol");
        let lengths: Vec<u64> = results[1..]
            .iter()
            .map(|row| row["fragment"]["token_length"].as_u64().unwrap())
            .collect();
        assert!(lengths.windows(2).all(|pair| pair[0] >= pair[1]), "{lengths:?}");
    }
}

// _(A docstring does not break a run)_ — nor does a Rust doc comment: the shared run spans the
// documentation, and neither side of it alone reaches the guaranteed length.
#[test]
fn documentation_does_not_break_a_run() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(
            &fixture.db(dir.path()),
            dir.path(),
            Some("d1::documented"),
            None,
            0,
            None,
        );
        let rows = fragments_naming(&answer, D2_DOCUMENTED);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        assert_eq!(rows[0]["certainty"], "exact_clone");
        let header = u64::from(support::line_number(fixture.source, "doc one\n")) + 1;
        let (start, end) = lines_of(&rows[0]["fragment"]["subject_run"]);
        assert_eq!(start, header, "{:?}: the run starts at the header", fixture.language);
        assert!(
            end > header + 1,
            "{:?}: the run continues past the documentation",
            fixture.language
        );
    }
}

// _(No reported pair is contained in another)_ — a block repeated twice in both symbols reports the
// doubled run once, not the shifted single-block pairs inside it; the subject never names itself.
#[test]
fn no_reported_pair_is_contained_in_another_for_the_same_symbols() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("twice"), None, 0, None);
        assert!(rows_naming(&answer, TWICE).is_empty());
        assert_eq!(
            fragments_naming(&answer, ALSO_TWICE).len(),
            1,
            "{:?}: {answer}",
            fixture.language
        );
        let span = |row: &Value, side: &str| {
            let run = &row["fragment"][side];
            (run["span_start"].as_u64().unwrap(), run["span_end"].as_u64().unwrap())
        };
        let fragments: Vec<&Value> = groups_of(&answer)
            .iter()
            .filter(|row| row["kind"] == "fragment")
            .collect();
        for (i, inner) in fragments.iter().enumerate() {
            for (j, outer) in fragments.iter().enumerate() {
                if i == j || inner["symbol"] != outer["symbol"] {
                    continue;
                }
                let within = |side: &str| {
                    let (inner, outer) = (span(inner, side), span(outer, side));
                    outer.0 <= inner.0 && inner.1 <= outer.1
                };
                assert!(
                    !(within("subject_run") && within("other_run")),
                    "{:?}",
                    fixture.language
                );
            }
        }
    }
}

// _(Runs come from the indexed text)_ — after the source changes on disk without a rebuild, the
// answer reports the same runs and is marked stale.
#[test]
fn a_changed_source_still_answers_from_the_indexed_text_and_is_stale() {
    let fixture = python_fragment_fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(fixture.doc);
    std::fs::write(&path, fixture.source).unwrap();
    let db = fixture.db(dir.path());
    let before = clones(&db, dir.path(), Some("f1::shorter"), None, 0, None);
    std::fs::write(
        &path,
        format!("import os\n\n\n{}", fixture.source.replace("total", "sum_")),
    )
    .unwrap();
    let after = clones(&db, dir.path(), Some("f1::shorter"), None, 0, None);
    assert_eq!(after["stale"], true);
    assert_eq!(after["outcome"], before["outcome"]);
    let rows = fragments_naming(&after, LONGER);
    assert_eq!(lines_of(&rows[0]["fragment"]["subject_run"]), fixture.subject_block);
}

// _(The answer applies the length the store records)_ — a recorded length one past the shared
// block filters it out, and the answer reports the recorded length.
#[test]
fn the_fragment_answer_applies_the_recorded_length() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let built = clones(&db, dir.path(), Some("f1::shorter"), None, 0, None);
        let block = fragments_naming(&built, LONGER)[0]["fragment"]["token_length"]
            .as_u64()
            .unwrap() as usize;
        let k = silent_cartographer::graph::clone::FRAGMENT_KGRAM_LENGTH;
        let window = block + 2 - k;
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute(
                "UPDATE index_metadata SET fragment_window = ?1 WHERE id = 1",
                [window as i64],
            )
            .unwrap();
        drop(connection);
        let answer = clones(&db, dir.path(), Some("f1::shorter"), None, 0, None);
        assert_eq!(
            answer["guaranteed_fragment_length"],
            block + 1,
            "{:?}",
            fixture.language
        );
        assert!(
            fragments_naming(&answer, LONGER).is_empty(),
            "{:?}: {answer}",
            fixture.language
        );
        assert_eq!(rows_naming(&answer, F2_SHORTER).len(), 1);
    }
}

// _(The human render locates both runs)_ — in the selected position unit.
#[test]
fn the_human_render_locates_both_runs_of_a_fragment() {
    let fixture = rust_fragment_fixture();
    let dir = tempfile::tempdir().unwrap();
    let db = fixture.db(dir.path());
    let human = |unit: PositionUnit| {
        silent_cartographer::commands::run_clones(
            &db,
            dir.path(),
            "rust-analyzer",
            Some("f1::shorter"),
            None,
            0,
            None,
            false,
            false,
            unit,
        )
        .unwrap()
    };
    let by_line = human(PositionUnit::Line);
    assert!(
        by_line.contains("54 tokens: subject src/fragments.rs:3:5-8:40 = other src/fragments.rs:29:9-34:44"),
        "{by_line}"
    );
    let by_byte = human(PositionUnit::Byte);
    let start = fixture.source.find("let mut total").unwrap();
    assert!(
        by_byte.contains(&format!("subject src/fragments.rs:{start}-")),
        "{by_byte}"
    );
}

const PLAIN: Segments = &[("plain", SegmentKind::Method)];
const TWIN: Segments = &[("twin", SegmentKind::Method)];
const HOST: Segments = &[("host", SegmentKind::Method)];
const HELPER: Segments = &[("host", SegmentKind::Method), ("helper", SegmentKind::Method)];

/// A source in one language for comparable content. `plain` holds two blocks back to back, each
/// longer than the guaranteed length; `twin` is a renamed whole copy of `plain`. `host` holds both
/// blocks in its own code with a nested function `helper` between them, and a header and a tail of
/// its own.
struct NestedFixture {
    language: Language,
    doc: &'static str,
    source: &'static str,
    decls: Vec<Decl<'static>>,
    /// The first and last line of each block in `host`.
    host_blocks: [(u64, u64); 2],
}

fn nested_fixtures() -> Vec<NestedFixture> {
    LANGUAGES
        .into_iter()
        .map(|language| match language {
            Language::Rust => rust_nested_fixture(),
            Language::Python => python_nested_fixture(),
        })
        .collect()
}

fn rust_nested_fixture() -> NestedFixture {
    let source = "\
pub fn plain(items: &[u32], scale: u32) -> u32 {
    let mut total = 0;
    let count = items.len() as u32 + scale;
    for item in items {
        if *item > scale { total += item * scale; } else { total += item + scale; }
    }
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    let spare = bonus - result * 2;
    let extra = spare % 7 + bonus + count;
    extra
}

pub fn twin(values: &[u32], factor: u32) -> u32 {
    let mut acc = 0;
    let size = values.len() as u32 + factor;
    for elem in values {
        if *elem > factor { acc += elem * factor; } else { acc += elem + factor; }
    }
    let outcome = acc * 2 + factor;
    let gain = outcome / 3 + factor * 5;
    let rest = gain - outcome * 2;
    let more = rest % 7 + gain + size;
    more
}

pub fn host(items: &[u32], scale: u32, label: &str) -> usize {
    let width = label.chars().count();
    let mut total = 0;
    let count = items.len() as u32 + scale;
    for item in items {
        if *item > scale { total += item * scale; } else { total += item + scale; }
    }
    #[inline]
    fn helper(values: &[u32]) -> u32 {
        let doubled: u32 = values.iter().map(|v| v * 7 + 3).sum();
        doubled / 2 + values.len() as u32
    }
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    let spare = bonus - result * 2;
    let extra = spare % 7 + bonus + count;
    width + extra as usize + helper(items) as usize
}
";
    NestedFixture {
        language: Language::Rust,
        doc: "src/nested.rs",
        source,
        decls: vec![
            decl(PLAIN, SymbolKind::Function, "pub fn plain("),
            decl(TWIN, SymbolKind::Function, "pub fn twin("),
            decl(HOST, SymbolKind::Function, "pub fn host("),
            decl(HELPER, SymbolKind::Function, "fn helper("),
        ],
        host_blocks: [(29, 33), (39, 42)],
    }
}

fn python_nested_fixture() -> NestedFixture {
    let source = "\
def plain(items, scale):
    total = 0
    count = len(items) + scale
    limit = count * 2 - 1
    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    spare = bonus - result * 2
    extra = spare % 7 + bonus + limit
    final = extra * count
    return final


def twin(values, factor):
    acc = 0
    size = len(values) + factor
    bound = size * 2 - 1
    for elem in values:
        if elem > factor:
            acc += elem * factor
        else:
            acc += elem + factor
    outcome = acc * 2 + factor
    gain = outcome // 3 + factor * 5
    rest = gain - outcome * 2
    more = rest % 7 + gain + bound
    last = more * size
    return last


def host(items, scale, label):
    width = len(label.strip())
    total = 0
    count = len(items) + scale
    limit = count * 2 - 1
    for item in items:
        if item > scale:
            total += item * scale
        else:
            total += item + scale

    def helper(values):
        doubled = sum(v * 7 + 3 for v in values)
        doubled -= len(values) * 3
        return doubled // 2 + len(values) - 1

    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    spare = bonus - result * 2
    extra = spare % 7 + bonus + limit
    final = extra * count
    print(width, helper(items))
    return width
";
    NestedFixture {
        language: Language::Python,
        doc: "nested.py",
        source,
        decls: vec![
            decl(PLAIN, SymbolKind::Function, "def plain("),
            decl(TWIN, SymbolKind::Function, "def twin("),
            decl(HOST, SymbolKind::Function, "def host("),
            decl(HELPER, SymbolKind::Function, "def helper("),
        ],
        host_blocks: [(37, 44), (51, 55)],
    }
}

impl NestedFixture {
    fn sources(&self) -> Vec<(String, String)> {
        vec![(self.doc.to_string(), self.source.to_string())]
    }

    /// The fixture built to an index on disk under `dir`.
    fn db(&self, dir: &Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        let index = support::declared_index(self.language, self.doc, self.source, &self.decls);
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &index, &self.sources()).unwrap();
        db
    }

    fn store(&self) -> silent_cartographer::graph::store::GraphStore {
        support::declared_store(self.language, self.doc, self.source, &self.decls)
    }
}

/// A run's byte span.
fn byte_span(run: &Value) -> (usize, usize) {
    (
        run["span_start"].as_u64().unwrap() as usize,
        run["span_end"].as_u64().unwrap() as usize,
    )
}

// _(A function that contains a nested function contributes evidence for its own code)_ — its token
// count is its own tokens plus one boundary for the nested function, which keeps evidence of its own;
// the function carries no clone keys.
#[test]
fn a_function_with_a_nested_function_contributes_evidence_for_its_own_code() {
    for fixture in nested_fixtures() {
        let store = fixture.store();
        let (host, helper) = (decl_id(HOST), decl_id(HELPER));
        let span_of = |id| {
            let (start, end) = store.symbol(id).unwrap().unwrap().span.unwrap();
            silent_cartographer::graph::range::ByteSpan { start, end }
        };
        let tree = silent_cartographer::graph::syntax::SyntaxTree::parse(fixture.source, fixture.language).unwrap();
        let own = tree.spelled_tokens(span_of(&host)).len() - tree.spelled_tokens(span_of(&helper)).len();
        assert_eq!(
            store.clone_token_count_of(&host).unwrap(),
            Some(own + 1),
            "{:?}",
            fixture.language
        );
        assert!(!store.clone_fingerprints_of(&host).unwrap().is_empty());
        assert_eq!(store.clone_keys_of(&host).unwrap(), None);
        assert!(store.clone_keys_of(&helper).unwrap().is_some());
        assert!(!store.clone_fingerprints_of(&helper).unwrap().is_empty());
    }
}

// _(Shared lines in the own code of a function with a nested function are found)_ and _(A run does
// not include a nested symbol)_ — from either side, the two blocks `host` shares with `plain` are two
// fragments, one on each side of `helper`, and neither run in `host` covers `helper`'s tokens.
#[test]
fn blocks_around_a_nested_function_are_found_and_no_run_covers_it() {
    for fixture in nested_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let store = silent_cartographer::graph::store::GraphStore::open(&db).unwrap();
        let helper = store.symbol(&decl_id(HELPER)).unwrap().unwrap().span.unwrap();
        for (subject, other, host_side) in [("plain", HOST, "other_run"), ("host", PLAIN, "subject_run")] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            let rows = fragments_naming(&answer, other);
            let mut host_runs: Vec<(u64, u64)> =
                rows.iter().map(|row| lines_of(&row["fragment"][host_side])).collect();
            host_runs.sort();
            assert_eq!(
                host_runs,
                fixture.host_blocks.to_vec(),
                "{:?} from {subject}: {answer}",
                fixture.language
            );
            for row in rows {
                let (start, end) = byte_span(&row["fragment"][host_side]);
                assert!(
                    end <= helper.0 || helper.1 <= start,
                    "{:?} from {subject}: a run covers the nested function: {row}",
                    fixture.language
                );
            }
        }
    }
}

// _(The comparable sequence the query re-spells is the one the build fingerprinted)_ — for every
// symbol with a token count, the re-spelled sequence has exactly that length.
#[test]
fn the_respelled_comparable_sequence_has_the_stored_token_count() {
    let mut built = Vec::new();
    for fixture in nested_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        built.push((fixture.language, fixture.sources(), fixture.db(dir.path()), dir));
    }
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        built.push((fixture.language, fixture.sources(), fixture.db(dir.path()), dir));
    }
    for (language, sources, db, _dir) in built {
        let counts: Vec<(String, usize)> = {
            let connection = rusqlite::Connection::open(&db).unwrap();
            let mut stmt = connection
                .prepare("SELECT canonical_id, clone_token_count FROM symbols WHERE clone_token_count IS NOT NULL")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert!(!counts.is_empty());
        let store = silent_cartographer::graph::store::GraphStore::open(&db).unwrap();
        let engine = QueryEngine::new(&store, backend_provenance(language), content_hash(&sources), None);
        for (id, count) in counts {
            let row = store
                .symbol(&silent_cartographer::identity::CanonicalId::from_raw(id.clone()))
                .unwrap()
                .unwrap();
            assert_eq!(
                engine.comparable_tokens_of(&row).unwrap().len(),
                count,
                "{language:?}: {id}"
            );
        }
    }
}

// _(Clone keys stay leaf-only)_ — `similar` for every symbol and the corpus clone groups answer the
// same with and without the comparable-content evidence of the function holding a nested function.
#[test]
fn comparable_content_leaves_similar_and_clone_groups_unchanged() {
    for fixture in nested_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let answers = || {
            let groups = clones(&db, dir.path(), None, None, 0, None)["outcome"].clone();
            let similar: Vec<Value> = ["plain", "twin", "host", "helper"]
                .into_iter()
                .map(|reference| {
                    let out = silent_cartographer::commands::run_similar(
                        &db,
                        dir.path(),
                        "rust-analyzer",
                        Some(reference),
                        None,
                        Detail::Location,
                        10,
                        false,
                        &[],
                        0,
                        None,
                        true,
                        false,
                        PositionUnit::Line,
                    )
                    .unwrap();
                    serde_json::from_str::<Value>(&out).unwrap()["outcome"].clone()
                })
                .collect();
            (groups, similar)
        };
        let with_evidence = answers();
        assert!(!with_evidence.0["results"].as_array().unwrap().is_empty());

        // Remove the evidence of every symbol carrying no keys: the store as the leaf-only rule
        // builds it.
        let connection = rusqlite::Connection::open(&db).unwrap();
        let removed = connection
            .execute(
                "DELETE FROM clone_fingerprints WHERE passage_id IN (
                     SELECT c.id FROM semantic_corpus c JOIN symbols s ON s.canonical_id = c.symbol_id
                     WHERE s.clone_formatting_key IS NULL)",
                [],
            )
            .unwrap();
        assert!(removed > 0, "{:?}", fixture.language);
        connection
            .execute(
                "UPDATE symbols SET clone_token_count = NULL WHERE clone_formatting_key IS NULL",
                [],
            )
            .unwrap();
        drop(connection);

        assert_eq!(answers(), with_evidence, "{:?}", fixture.language);
    }
}

const REPEAT_ALPHA: Segments = &[("repeat_alpha", SegmentKind::Method)];
const REPEAT_BETA: Segments = &[("repeat_beta", SegmentKind::Method)];
const REPEAT_GAMMA: Segments = &[("repeat_gamma", SegmentKind::Method)];
const BLOCK_USER: Segments = &[("block_user", SegmentKind::Method)];

/// Each repetitive function holds this many statements of one shape; `repeat_beta` holds
/// `REPEAT_EXTRA` more, so it is no whole-symbol clone of the others. A function's fingerprints
/// then share one hash about once per statement, and a pair of them has more candidate seeds than
/// the work limit allows.
const REPEAT_LENGTH: usize = 1500;
const REPEAT_EXTRA: usize = 100;

/// A source in one language for the work limit. `repeat_alpha` and `repeat_gamma` hold the same
/// run of repeated statements, with consistently renamed variables, followed by the same block of
/// statements, so each is a variant whole-symbol clone of the other; `repeat_beta` holds a longer
/// run of other variables and no block; `block_user` holds the block alone.
struct RepeatFixture {
    language: Language,
    doc: &'static str,
    source: String,
    decls: Vec<Decl<'static>>,
}

/// `count` statements of one shape, each assigning a new variable named with `prefix` from the
/// previous one, in `language`.
fn repeated_statements(language: Language, prefix: &str, count: usize) -> String {
    (0..count)
        .map(|i| {
            let previous = if i == 0 {
                "scale".to_string()
            } else {
                format!("{prefix}{}", i - 1)
            };
            match language {
                Language::Rust => format!("    let {prefix}{i} = {previous} + scale;\n"),
                Language::Python => format!("    {prefix}{i} = {previous} + scale\n"),
            }
        })
        .collect()
}

fn rust_repeat_fixture() -> RepeatFixture {
    let function = |name: &str, prefix: &str, count: usize, block: bool| {
        let tail = if block {
            "    let mut total = scale * 2;
    for name in names.iter() { if name.len() > 3 { total += scale; } else { total += 2; } }
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    bonus + result
"
        } else {
            "    scale\n"
        };
        format!(
            "pub fn {name}(names: &[&str], scale: u32) -> u32 {{\n{}{tail}}}\n\n",
            repeated_statements(Language::Rust, prefix, count)
        )
    };
    let source = format!(
        "{}{}{}pub fn block_user(names: &[&str], scale: u32) -> u32 {{
    let mut total = scale * 2;
    for name in names.iter() {{ if name.len() > 3 {{ total += scale; }} else {{ total += 2; }} }}
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    bonus + result
}}
",
        function("repeat_alpha", "a", REPEAT_LENGTH, true),
        function("repeat_beta", "b", REPEAT_LENGTH + REPEAT_EXTRA, false),
        function("repeat_gamma", "c", REPEAT_LENGTH, true),
    );
    RepeatFixture {
        language: Language::Rust,
        doc: "src/repeats.rs",
        source,
        decls: vec![
            decl(REPEAT_ALPHA, SymbolKind::Function, "pub fn repeat_alpha("),
            decl(REPEAT_BETA, SymbolKind::Function, "pub fn repeat_beta("),
            decl(REPEAT_GAMMA, SymbolKind::Function, "pub fn repeat_gamma("),
            decl(BLOCK_USER, SymbolKind::Function, "pub fn block_user("),
        ],
    }
}

fn python_repeat_fixture() -> RepeatFixture {
    let function = |name: &str, prefix: &str, count: usize, block: bool| {
        let tail = if block {
            "    total = scale * 2
    for name in names:
        if len(name) > 3:
            total += scale
        else:
            total += 2
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    return bonus + result
"
        } else {
            "    return scale\n"
        };
        format!(
            "def {name}(names, scale):\n{}{tail}\n\n",
            repeated_statements(Language::Python, prefix, count)
        )
    };
    let source = format!(
        "{}{}{}def block_user(names, scale):
    total = scale * 2
    for name in names:
        if len(name) > 3:
            total += scale
        else:
            total += 2
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    return bonus + result
",
        function("repeat_alpha", "a", REPEAT_LENGTH, true),
        function("repeat_beta", "b", REPEAT_LENGTH + REPEAT_EXTRA, false),
        function("repeat_gamma", "c", REPEAT_LENGTH, true),
    );
    RepeatFixture {
        language: Language::Python,
        doc: "repeats.py",
        source,
        decls: vec![
            decl(REPEAT_ALPHA, SymbolKind::Function, "def repeat_alpha("),
            decl(REPEAT_BETA, SymbolKind::Function, "def repeat_beta("),
            decl(REPEAT_GAMMA, SymbolKind::Function, "def repeat_gamma("),
            decl(BLOCK_USER, SymbolKind::Function, "def block_user("),
        ],
    }
}

fn repeat_fixtures() -> Vec<RepeatFixture> {
    LANGUAGES
        .into_iter()
        .map(|language| match language {
            Language::Rust => rust_repeat_fixture(),
            Language::Python => python_repeat_fixture(),
        })
        .collect()
}

impl RepeatFixture {
    fn db(&self, dir: &Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        let index = support::declared_index(self.language, self.doc, &self.source, &self.decls);
        let sources = vec![(self.doc.to_string(), self.source.clone())];
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &index, &sources).unwrap();
        db
    }
}

/// The identities the answer lists as not compared completely, in the order listed.
fn not_compared(answer: &Value) -> Vec<String> {
    answer["not_compared_completely"]
        .as_array()
        .unwrap_or_else(|| panic!("no not_compared_completely field: {answer}"))
        .iter()
        .map(|view| view["canonical_id"].as_str().unwrap().to_string())
        .collect()
}

// _(A repetitive pair is named, not reported in part)_ — the repetitive pair is named, has no
// fragment rows, and the other symbols' rows are still reported.
#[test]
fn a_repetitive_pair_is_named_and_reports_no_fragment_rows() {
    for fixture in repeat_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let language = fixture.language;
        let answer = clones(&db, dir.path(), Some("repeat_alpha"), None, 0, None);
        assert_eq!(
            not_compared(&answer),
            vec![decl_id(REPEAT_BETA).as_str().to_string()],
            "{language:?}: {answer}"
        );
        assert!(rows_naming(&answer, REPEAT_BETA).is_empty(), "{language:?}: {answer}");
        let gamma = rows_naming(&answer, REPEAT_GAMMA);
        assert_eq!(gamma.len(), 1, "{language:?}: {answer}");
        assert_eq!(gamma[0]["kind"], "whole_symbol");
        assert_eq!(fragments_naming(&answer, BLOCK_USER).len(), 1, "{language:?}: {answer}");

        // From the longer function, both other repetitive functions are over the limit, in
        // identity order.
        let from_beta = clones(&db, dir.path(), Some("repeat_beta"), None, 0, None);
        let mut expected = vec![
            decl_id(REPEAT_ALPHA).as_str().to_string(),
            decl_id(REPEAT_GAMMA).as_str().to_string(),
        ];
        expected.sort();
        assert_eq!(not_compared(&from_beta), expected, "{language:?}: {from_beta}");
    }
}

// An answer in which no pair reaches the limit carries no such field.
#[test]
fn the_field_is_absent_when_no_pair_reaches_the_limit() {
    for fixture in fragment_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("f1::shorter"), None, 0, None);
        assert!(
            answer.get("not_compared_completely").is_none(),
            "{:?}: {answer}",
            fixture.language
        );
    }
}

// The limited symbols ride on every page of the answer.
#[test]
fn a_limited_answer_carries_the_field_on_every_page() {
    for fixture in repeat_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let whole = clones(&db, dir.path(), Some("repeat_alpha"), None, 0, None);
        assert_eq!(groups_of(&whole).len(), 2, "{:?}", fixture.language);
        assert!(!not_compared(&whole).is_empty(), "{:?}: {whole}", fixture.language);
        let mut pages = 0;
        let mut cursor: Option<String> = None;
        loop {
            let page = clones(&db, dir.path(), Some("repeat_alpha"), None, 1, cursor.as_deref());
            assert_eq!(
                page["not_compared_completely"], whole["not_compared_completely"],
                "{:?}: {page}",
                fixture.language
            );
            pages += 1;
            match page["page"]["cursor"].as_str() {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
        }
        assert_eq!(pages, 2, "{:?}", fixture.language);
    }
}

/// The human rendering of a `clones` answer for `reference`.
fn human_clones(db: &Path, root: &Path, reference: &str) -> String {
    silent_cartographer::commands::run_clones(
        db,
        root,
        "rust-analyzer",
        Some(reference),
        None,
        0,
        None,
        false,
        false,
        PositionUnit::Line,
    )
    .unwrap()
}

#[test]
fn the_human_render_names_the_symbols_not_compared_completely() {
    for fixture in repeat_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let out = human_clones(&db, dir.path(), "repeat_alpha");
        let expected = format!(
            "not compared completely (too many candidate positions): {}",
            decl_id(REPEAT_BETA).as_str()
        );
        assert!(out.contains(&expected), "{:?}: {out}", fixture.language);
    }
}

// An empty answer whose other symbols were not all compared says so, rather than the plain empty
// line: `repeat_beta` has no clone but the two functions it was not compared with.
#[test]
fn an_empty_answer_with_symbols_not_compared_is_scoped_to_the_symbols_compared() {
    for fixture in repeat_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let answer = clones(&db, dir.path(), Some("repeat_beta"), None, 0, None);
        assert_eq!(
            answer["outcome"]["outcome"], "empty",
            "{:?}: {answer}",
            fixture.language
        );
        let out = human_clones(&db, dir.path(), "repeat_beta");
        assert!(
            out.contains("empty: no clones found among the symbols compared"),
            "{:?}: {out}",
            fixture.language
        );
    }
}

const TABLE_ONE: Segments = &[("table_one", SegmentKind::Method)];
const TABLE_TWO: Segments = &[("table_two", SegmentKind::Method)];
const TABLE_HOST: Segments = &[("table_host", SegmentKind::Method)];
const RECORDS_ONE: Segments = &[("records_one", SegmentKind::Method)];
const RECORDS_TWO: Segments = &[("records_two", SegmentKind::Method)];
const CONSTANT_ONE: Segments = &[("constant_one", SegmentKind::Method)];
const CONSTANT_TWO: Segments = &[("constant_two", SegmentKind::Method)];
const PADDED_ONE: Segments = &[("padded_one", SegmentKind::Method)];
const PADDED_TWO: Segments = &[("padded_two", SegmentKind::Method)];
const NEGATIVES_ONE: Segments = &[("negatives_one", SegmentKind::Method)];
const NEGATIVES_TWO: Segments = &[("negatives_two", SegmentKind::Method)];
const EDITED_HOST: Segments = &[("edited_host", SegmentKind::Method)];

/// The row of `table_one`'s table that `edited_host` holds with another value.
const EDITED_ROW: usize = 30;

/// Rows in each table; the second table of each pair holds `DATA_EXTRA` more, so neither is a
/// whole-symbol clone of the other.
const DATA_ROWS: usize = 60;
const DATA_EXTRA: usize = 5;

/// A source in one language for data stretches. `table_one` and `table_two` each hold a table of
/// distinct literal values, the values different; `table_host` holds `table_one`'s table between
/// code of its own. `records_one` and `records_two` each hold a table of record literals with the
/// same field or key names and different values. `constant_one` holds a block of statements that
/// `constant_two` holds with one numeric constant changed, followed by a statement of its own.
/// `padded_one` and `padded_two` each hold two tables, the values different, around one shared
/// statement shorter than the guaranteed length. `negatives_one` and `negatives_two` each hold a
/// table of distinct negative numbers, the values different. `edited_host` holds `table_one`'s
/// table with the value of row `EDITED_ROW` changed.
struct DataFixture {
    language: Language,
    doc: &'static str,
    source: String,
    decls: Vec<Decl<'static>>,
    /// The first and last line of `table_one`'s table inside `table_host`.
    host_table: (u64, u64),
    /// The byte spans of the first, edited, and last value of the table inside `edited_host`.
    edited_table: [(usize, usize); 3],
}

/// The source of the padded, negative, and edited tables in `language`, appended to the data
/// fixture.
fn more_data_source(language: Language) -> String {
    let strings = |prefix: &'static str| move |i: usize| format!("\"{prefix}{i:03}\"");
    let negatives = |step: usize| move |i: usize| format!("-{}", i * step + 1);
    let edited = |i: usize| {
        if i == EDITED_ROW {
            "\"edited\"".to_string()
        } else {
            format!("\"one{i:03}\"")
        }
    };
    let indent = "        ";
    match language {
        Language::Rust => format!(
            "
pub fn padded_one(scale: usize) -> usize {{
    let first = [
{}    ];
    let total = first.len() + scale;
    let second = [
{}    ];
    total + second.len()
}}

pub fn padded_two(scale: usize) -> usize {{
    let first = [
{}    ];
    let total = first.len() + scale;
    let second = [
{}    ];
    total + second.len()
}}

pub fn negatives_one() -> usize {{
    let values = [
{}    ];
    values.len()
}}

pub fn negatives_two() -> usize {{
    let values = [
{}    ];
    values.len()
}}

pub fn edited_host(scale: usize) -> usize {{
    let width = scale * 2 + 1;
    let values = [
{}    ];
    values.len() + width
}}
",
            rows(DATA_ROWS, indent, strings("pa")),
            rows(DATA_ROWS, indent, strings("pb")),
            rows(DATA_ROWS + DATA_EXTRA, indent, strings("qa")),
            rows(DATA_ROWS, indent, strings("qb")),
            rows(DATA_ROWS, indent, negatives(3)),
            rows(DATA_ROWS + DATA_EXTRA, indent, negatives(5)),
            rows(DATA_ROWS, indent, edited),
        ),
        Language::Python => format!(
            "

def padded_one(scale):
    first = [
{}    ]
    total = len(first) + scale
    second = [
{}    ]
    return total + len(second)


def padded_two(scale):
    first = [
{}    ]
    total = len(first) + scale
    second = [
{}    ]
    return total + len(second)


def negatives_one():
    values = [
{}    ]
    return len(values)


def negatives_two():
    values = [
{}    ]
    return len(values)


def edited_host(scale):
    width = scale * 2 + 1
    values = [
{}    ]
    return len(values) + width
",
            rows(DATA_ROWS, indent, strings("pa")),
            rows(DATA_ROWS, indent, strings("pb")),
            rows(DATA_ROWS + DATA_EXTRA, indent, strings("qa")),
            rows(DATA_ROWS, indent, strings("qb")),
            rows(DATA_ROWS, indent, negatives(3)),
            rows(DATA_ROWS + DATA_EXTRA, indent, negatives(5)),
            rows(DATA_ROWS, indent, edited),
        ),
    }
}

/// The byte spans of the first, edited, and last value of the table inside `edited_host` in
/// `source`, which declares it after `opener`.
fn edited_values(source: &str, opener: &str) -> [(usize, usize); 3] {
    let host = source.find(&format!("{opener}edited_host")).unwrap();
    let last = format!("\"one{:03}\"", DATA_ROWS - 1);
    ["\"one000\"", "\"edited\"", last.as_str()].map(|value| {
        let start = host + source[host..].find(value).unwrap();
        (start, start + value.len())
    })
}

/// `count` lines, each `row` applied to its index and indented by `indent`.
fn rows(count: usize, indent: &str, row: impl Fn(usize) -> String) -> String {
    (0..count).map(|i| format!("{indent}{},\n", row(i))).collect()
}

/// The 1-based line on which `needle` first occurs in `source`.
fn line_of(source: &str, needle: &str) -> u64 {
    let at = source.find(needle).unwrap_or_else(|| panic!("{needle} occurs"));
    source[..at].matches('\n').count() as u64 + 1
}

fn rust_data_fixture() -> DataFixture {
    let strings = |prefix: &'static str| move |i: usize| format!("\"{prefix}{i:03}\"");
    let records =
        |prefix: &'static str| move |i: usize| format!("Row {{ name: \"{prefix}{i:03}\", size: {} }}", i * 7);
    let table_one = rows(DATA_ROWS, "        ", strings("one"));
    let source = format!(
        "pub fn table_one() -> usize {{
    let values = [
{table_one}    ];
    values.len()
}}

pub fn table_two() -> usize {{
    let values = [
{}    ];
    values.len()
}}

pub fn table_host(scale: usize) -> usize {{
    let width = scale * 2 + 1;
    let values = [
{table_one}    ];
    values.len() + width
}}

pub fn records_one() -> usize {{
    let values = [
{}    ];
    values.len()
}}

pub fn records_two() -> usize {{
    let values = [
{}    ];
    values.len()
}}

pub fn constant_one(items: &[u32], scale: u32) -> u32 {{
    let mut total = 0;
    for item in items {{
        if *item > scale {{ total += item * 7; }} else {{ total += item + scale; }}
    }}
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    bonus + result
}}

pub fn constant_two(items: &[u32], scale: u32) -> u32 {{
    let mut total = 0;
    for item in items {{
        if *item > scale {{ total += item * 9; }} else {{ total += item + scale; }}
    }}
    let result = total * 2 + scale;
    let bonus = result / 3 + scale * 5;
    println!(\"{{}}\", bonus);
    bonus + result
}}
",
        rows(DATA_ROWS + DATA_EXTRA, "        ", strings("two")),
        rows(DATA_ROWS, "        ", records("one")),
        rows(DATA_ROWS + DATA_EXTRA, "        ", records("two")),
    );
    let source = source + &more_data_source(Language::Rust);
    let edited_table = edited_values(&source, "pub fn ");
    let host = &source[source.find("pub fn table_host").unwrap()..];
    let offset = line_of(&source, "pub fn table_host") - 1;
    let host_table = (
        offset + line_of(host, "\"one000\""),
        offset + line_of(host, &format!("\"one{:03}\"", DATA_ROWS - 1)),
    );
    DataFixture {
        language: Language::Rust,
        doc: "src/data.rs",
        decls: data_decls("pub fn "),
        source,
        host_table,
        edited_table,
    }
}

fn python_data_fixture() -> DataFixture {
    let strings = |prefix: &'static str| move |i: usize| format!("\"{prefix}{i:03}\"");
    let records =
        |prefix: &'static str| move |i: usize| format!("{{\"name\": \"{prefix}{i:03}\", \"size\": {}}}", i * 7);
    let table_one = rows(DATA_ROWS, "        ", strings("one"));
    let source = format!(
        "def table_one():
    values = [
{table_one}    ]
    return len(values)


def table_two():
    values = [
{}    ]
    return len(values)


def table_host(scale):
    width = scale * 2 + 1
    values = [
{table_one}    ]
    return len(values) + width


def records_one():
    values = [
{}    ]
    return len(values)


def records_two():
    values = [
{}    ]
    return len(values)


def constant_one(items, scale):
    total = 0
    for item in items:
        if item > scale:
            total += item * 7
        else:
            total += item + scale
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    return bonus + result


def constant_two(items, scale):
    total = 0
    for item in items:
        if item > scale:
            total += item * 9
        else:
            total += item + scale
    result = total * 2 + scale
    bonus = result // 3 + scale * 5
    print(bonus)
    return bonus + result
",
        rows(DATA_ROWS + DATA_EXTRA, "        ", strings("two")),
        rows(DATA_ROWS, "        ", records("one")),
        rows(DATA_ROWS + DATA_EXTRA, "        ", records("two")),
    );
    let source = source + &more_data_source(Language::Python);
    let edited_table = edited_values(&source, "def ");
    let host = &source[source.find("def table_host").unwrap()..];
    let offset = line_of(&source, "def table_host") - 1;
    let host_table = (
        offset + line_of(host, "\"one000\""),
        offset + line_of(host, &format!("\"one{:03}\"", DATA_ROWS - 1)),
    );
    DataFixture {
        language: Language::Python,
        doc: "data.py",
        decls: data_decls("def "),
        source,
        host_table,
        edited_table,
    }
}

/// The data fixture's declarations, each found by `opener` followed by its name.
fn data_decls(opener: &str) -> Vec<Decl<'static>> {
    [
        TABLE_ONE,
        TABLE_TWO,
        TABLE_HOST,
        RECORDS_ONE,
        RECORDS_TWO,
        CONSTANT_ONE,
        CONSTANT_TWO,
        PADDED_ONE,
        PADDED_TWO,
        NEGATIVES_ONE,
        NEGATIVES_TWO,
        EDITED_HOST,
    ]
    .into_iter()
    .map(|segments| {
        let within: &'static str = Box::leak(format!("{opener}{}(", segments[0].0).into_boxed_str());
        decl(segments, SymbolKind::Function, within)
    })
    .collect()
}

fn data_fixtures() -> Vec<DataFixture> {
    LANGUAGES
        .into_iter()
        .map(|language| match language {
            Language::Rust => rust_data_fixture(),
            Language::Python => python_data_fixture(),
        })
        .collect()
}

impl DataFixture {
    fn db(&self, dir: &Path) -> std::path::PathBuf {
        let db = dir.join("index.db");
        let index = support::declared_index(self.language, self.doc, &self.source, &self.decls);
        let sources = vec![(self.doc.to_string(), self.source.clone())];
        silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &index, &sources).unwrap();
        db
    }
}

/// Whether any row of a subject answer, which may be empty, names `other`.
fn names(answer: &Value, other: Segments) -> bool {
    let id = decl_id(other);
    answer["outcome"]["results"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["symbol"]["canonical_id"] == id.as_str()))
}

// _(Different tables of literal values share no fragment)_
#[test]
fn different_literal_tables_share_no_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        for (subject, other) in [("table_one", TABLE_TWO), ("table_two", TABLE_ONE)] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            assert!(!names(&answer, other), "{:?}: {answer}", fixture.language);
        }
    }
}

// _(A copied table is an exact fragment)_
#[test]
fn a_copied_table_is_an_exact_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("table_one"), None, 0, None);
        let rows = fragments_naming(&answer, TABLE_HOST);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        assert_eq!(rows[0]["certainty"], "exact_clone", "{:?}: {answer}", fixture.language);
        let (first, last) = lines_of(&rows[0]["fragment"]["other_run"]);
        assert!(
            first <= fixture.host_table.0 && fixture.host_table.1 <= last,
            "{:?}: {answer}",
            fixture.language
        );
    }
}

// _(Different tables of records share no fragment)_ — Rust classifies record-literal structure;
// in Python a table of dictionaries with string keys is literals and punctuation throughout.
#[test]
fn different_record_tables_share_no_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        for (subject, other) in [("records_one", RECORDS_TWO), ("records_two", RECORDS_ONE)] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            assert!(!names(&answer, other), "{:?}: {answer}", fixture.language);
        }
    }
}

// _(A changed constant in code remains a variant)_
#[test]
fn a_changed_constant_in_code_remains_a_variant_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("constant_one"), None, 0, None);
        let rows = fragments_naming(&answer, CONSTANT_TWO);
        assert_eq!(rows.len(), 1, "{:?}: {answer}", fixture.language);
        assert_eq!(
            rows[0]["certainty"], "variant_clone",
            "{:?}: {answer}",
            fixture.language
        );
        assert!(
            rows[0]["fragment"]["token_length"].as_u64() >= answer["guaranteed_fragment_length"].as_u64(),
            "{:?}: {answer}",
            fixture.language
        );
    }
}

// _(Different table values do not extend a fragment)_ — the one shared statement between the
// tables is shorter than the guaranteed length, and the differing values around it do not pad it.
#[test]
fn different_table_values_do_not_extend_a_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        for (subject, other) in [("padded_one", PADDED_TWO), ("padded_two", PADDED_ONE)] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            assert!(!names(&answer, other), "{:?}: {answer}", fixture.language);
        }
    }
}

// _(Different tables of negative numbers share no fragment)_
#[test]
fn different_tables_of_negative_numbers_share_no_fragment() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        for (subject, other) in [("negatives_one", NEGATIVES_TWO), ("negatives_two", NEGATIVES_ONE)] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            assert!(!names(&answer, other), "{:?}: {answer}", fixture.language);
        }
    }
}

// A copied table with one edited value still reports its identical parts: one exact fragment runs
// from the table's first value up to the edited value, another from after it through the last
// value, and none includes it.
#[test]
fn a_copied_table_with_one_edited_value_reports_its_identical_parts() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let answer = clones(&fixture.db(dir.path()), dir.path(), Some("table_one"), None, 0, None);
        let rows = fragments_naming(&answer, EDITED_HOST);
        let [first, edited, last] = fixture.edited_table;
        let runs: Vec<(usize, usize)> = rows
            .iter()
            .map(|row| byte_span(&row["fragment"]["other_run"]))
            .collect();
        assert!(
            rows.iter().all(|row| row["certainty"] == "exact_clone"),
            "{:?}: {answer}",
            fixture.language
        );
        assert!(
            runs.iter().all(|&(start, end)| end <= edited.0 || edited.1 <= start),
            "{:?}: {runs:?}",
            fixture.language
        );
        // Only layout lies between the two offsets.
        let adjacent = |from: usize, to: usize| from <= to && fixture.source[from..to].trim().is_empty();
        assert!(
            runs.iter()
                .any(|&(start, end)| start <= first.0 && adjacent(end, edited.0)),
            "{:?}: {runs:?}",
            fixture.language
        );
        assert!(
            runs.iter()
                .any(|&(start, end)| adjacent(edited.1, start) && last.1 <= end),
            "{:?}: {runs:?}",
            fixture.language
        );
    }
}

// _(The data-stretch length comes from the store)_ — a lone differing constant is a variant at the
// built length, and a break at a recorded length of 1, so no run includes it.
#[test]
fn the_fragment_answer_applies_the_recorded_data_stretch_length() {
    for fixture in data_fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let db = fixture.db(dir.path());
        let constant = fixture.source.find("item * 9").unwrap() + "item * ".len();
        let covers = |row: &&Value| {
            let (start, end) = byte_span(&row["fragment"]["other_run"]);
            start <= constant && constant < end
        };
        let built = clones(&db, dir.path(), Some("constant_one"), None, 0, None);
        assert!(fragments_naming(&built, CONSTANT_TWO).iter().any(covers));
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute("UPDATE index_metadata SET fragment_kgram_length = 1 WHERE id = 1", [])
            .unwrap();
        drop(connection);
        let answer = clones(&db, dir.path(), Some("constant_one"), None, 0, None);
        let rows = fragments_naming(&answer, CONSTANT_TWO);
        assert!(!rows.is_empty(), "{:?}: {answer}", fixture.language);
        assert!(!rows.iter().any(covers), "{:?}: {answer}", fixture.language);
    }
}

/// A build of `source` in `language`, declaring `decls`, under `dir`.
fn build_source(language: Language, doc: &str, source: &str, decls: &[Decl], dir: &Path) -> std::path::PathBuf {
    let db = dir.join("index.db");
    let index = support::declared_index(language, doc, source, decls);
    silent_cartographer::commands::build_from_index(&db, "test-ws", dir, &index, &[(doc.into(), source.into())])
        .unwrap();
    db
}

// _(Documentation is disregarded by the clone key)_ — two functions differing only in a docstring
// (Python) or doc comments (Rust) are exact clones, in the corpus groups and from either subject.
#[test]
fn functions_differing_only_in_documentation_are_exact_clones() {
    for language in LANGUAGES {
        let (doc, source, withins) = match language {
            Language::Rust => (
                "src/docs.rs",
                "\
/// Adds the pair.
pub fn alpha(x: u32, y: u32) -> u32 {
    /// Inner note.
    let total = x + y;
    total * 3
}

/// Sums two numbers and scales the result, described differently.
pub fn alpha(x: u32, y: u32) -> u32 {
    /// Another inner note.
    let total = x + y;
    total * 3
}
",
                [
                    "/// Adds the pair.\npub fn alpha(",
                    "described differently.\npub fn alpha(",
                ],
            ),
            Language::Python => (
                "docs.py",
                "\
def alpha(x, y):
    \"\"\"Adds the pair.\"\"\"
    total = x + y
    return total * 3


def alpha(x, y):
    \"\"\"Sums two numbers and scales the result, described differently.\"\"\"
    total = x + y
    return total * 3
",
                ["def alpha(x, y):\n    \"\"\"Adds", "def alpha(x, y):\n    \"\"\"Sums"],
            ),
        };
        let decls = vec![
            decl(ALPHA_1, SymbolKind::Function, withins[0]),
            decl(ALPHA_2, SymbolKind::Function, withins[1]),
        ];
        let dir = tempfile::tempdir().unwrap();
        let db = build_source(language, doc, source, &decls, dir.path());
        let corpus = clones(&db, dir.path(), None, None, 0, None);
        let group = group_with(&corpus, ALPHA_1);
        assert_eq!(group["certainty"], "exact_clone", "{language:?}: {corpus}");
        assert_eq!(
            sorted(ids(&group["members"], "symbol")),
            id_strings(&[ALPHA_1, ALPHA_2]),
            "{language:?}"
        );
        for (subject, other) in [("m1::alpha", ALPHA_2), ("m2::alpha", ALPHA_1)] {
            let answer = clones(&db, dir.path(), Some(subject), None, 0, None);
            let rows = rows_naming(&answer, other);
            assert_eq!(rows.len(), 1, "{language:?}: {answer}");
            assert_eq!(rows[0]["certainty"], "exact_clone", "{language:?}");
        }
    }
}

// _(The marker composes with staleness)_ — through the command, a changed source leaves the answer
// stale and still graded.
#[test]
fn a_stale_clones_command_answer_carries_the_flag_and_the_marker() {
    for fixture in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(fixture.doc);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, fixture.source).unwrap();
        let db = fixture.db(dir.path());
        std::fs::write(&path, format!("{}\n// edited\n", fixture.source)).unwrap();
        for answer in [
            clones(&db, dir.path(), None, None, 0, None),
            clones(&db, dir.path(), Some("n1::bravo"), None, 0, None),
        ] {
            assert_eq!(answer["stale"], true, "{:?}: {answer}", fixture.language);
            assert_eq!(answer["grade"], "token_equivalence", "{:?}", fixture.language);
            assert!(answer["guaranteed_fragment_length"].is_u64());
        }
    }
}

const PLUS_Z1: Segments = &[("z1", SegmentKind::Module), ("plus", SegmentKind::Method)];
const PLUS_Z2: Segments = &[("z2", SegmentKind::Module), ("plus", SegmentKind::Method)];
const PLUS_Z3: Segments = &[("z3", SegmentKind::Module), ("plus", SegmentKind::Method)];
const MINUS_A1: Segments = &[("a1", SegmentKind::Module), ("minus", SegmentKind::Method)];
const MINUS_A2: Segments = &[("a2", SegmentKind::Module), ("minus", SegmentKind::Method)];
const DIVIDE_B1: Segments = &[("b1", SegmentKind::Module), ("divide", SegmentKind::Method)];
const DIVIDE_B2: Segments = &[("b2", SegmentKind::Module), ("divide", SegmentKind::Method)];
const TIMES_C1: Segments = &[("c1", SegmentKind::Module), ("times", SegmentKind::Method)];
const TIMES_C2: Segments = &[("c2", SegmentKind::Module), ("times", SegmentKind::Method)];

// _(Groups of equal length order by member count, then lowest identity)_ — four groups of one token
// count: the three-member group first although its lowest identity sorts last, then the two-member
// groups by lowest identity.
#[test]
fn groups_of_equal_token_count_order_by_member_count_then_lowest_identity() {
    for language in LANGUAGES {
        let (doc, opener, operations) = match language {
            Language::Rust => ("src/order.rs", "pub fn ", "{ x OP y }"),
            Language::Python => ("order.py", "def ", ":\n    return x OP y"),
        };
        let signature = match language {
            Language::Rust => "(x: u32, y: u32) -> u32 ",
            Language::Python => "(x, y)",
        };
        let members: [(Segments, &str, &str); 9] = [
            (PLUS_Z1, "plus", "+"),
            (PLUS_Z2, "plus", "+"),
            (PLUS_Z3, "plus", "+"),
            (MINUS_A1, "minus", "-"),
            (MINUS_A2, "minus", "-"),
            (DIVIDE_B1, "divide", "/"),
            (DIVIDE_B2, "divide", "/"),
            (TIMES_C1, "times", "*"),
            (TIMES_C2, "times", "*"),
        ];
        let mut source = String::new();
        let mut decls = Vec::new();
        for (index, (segments, name, operator)) in members.into_iter().enumerate() {
            let within: &'static str = Box::leak(format!("// m{index}\n{opener}{name}(").into_boxed_str());
            let comment = if language == Language::Rust { "//" } else { "#" };
            source.push_str(&format!(
                "{comment} m{index}\n{opener}{name}{signature}{}\n\n",
                operations.replace("OP", operator)
            ));
            let within = if language == Language::Rust {
                within
            } else {
                Box::leak(format!("# m{index}\n{opener}{name}(").into_boxed_str())
            };
            decls.push(decl(segments, SymbolKind::Function, within));
        }
        let dir = tempfile::tempdir().unwrap();
        let db = build_source(language, doc, &source, &decls, dir.path());
        let answer = clones(&db, dir.path(), None, None, 0, None);
        let groups = groups_of(&answer);
        let counts: Vec<u64> = groups.iter().map(|g| g["token_count"].as_u64().unwrap()).collect();
        assert_eq!(counts.len(), 4, "{language:?}: {answer}");
        assert!(
            counts.windows(2).all(|pair| pair[0] == pair[1]),
            "{language:?}: {counts:?}"
        );
        let lowest: Vec<String> = groups
            .iter()
            .map(|group| sorted(ids(&group["members"], "symbol")).remove(0))
            .collect();
        let expected: Vec<String> = [PLUS_Z1, MINUS_A1, DIVIDE_B1, TIMES_C1]
            .iter()
            .map(|segments| decl_id(segments).as_str().to_string())
            .collect();
        assert_eq!(lowest, expected, "{language:?}: {answer}");
    }
}
