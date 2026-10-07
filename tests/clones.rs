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
        assert!(out.contains("guaranteed fragment length: 40 tokens"), "{out}");
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
