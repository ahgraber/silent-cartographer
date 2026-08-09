//! Process-level tests for `impact`: seeding from a git diff (working tree, staged, and revision
//! range modes, plus rename/delete resolution at the pre-change path), the exit taxonomy for the git
//! boundary, the freshness/recovery grading of the answer, and the shared output contract (JSON/human
//! parity, result-set bounding, and the uncapped diff capture).
//!
//! These drive the built `c10r` binary through `std::process::Command`, exactly as
//! `tests/output_bounding.rs` and `tests/surface_manifest.rs` do (`Command::new(env!("CARGO_BIN_EXE_c10r"))`,
//! never the ambient `c10r` on `PATH`). A live `rust-analyzer` is not available in this environment,
//! so every index here is built the way the existing tests build one: `build_from_index` over a
//! hand-built `ExtractedIndex`, with source text written to disk so the diff's byte offsets and the
//! index's persisted spans agree.
//!
//! Every git invocation *this file* makes to set up a throwaway repository is isolated from the
//! developer's own git config (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/`GIT_CONFIG_NOSYSTEM`, plus an
//! explicit `-c user.*`/`commit.gpgsign=false` on every commit). The `c10r` invocations under test
//! deliberately do NOT stub git config: they exercise the real git boundary as a caller's own
//! environment would present it, and none of the operations `run_impact` performs (`rev-parse`,
//! `diff`, `show`, `ls-files`) needs a committer identity or a signature.

mod support;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use silent_cartographer::commands::{build_from_index, collect_rust_sources, resolve_workspace};
use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind};
use silent_cartographer::semantic::model::{
    ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding, SourceDocument,
    SourceRange, SymbolClass, SymbolKind,
};

// ---------------------------------------------------------------------------
// A throwaway git repository, isolated from the developer's own git config.
// ---------------------------------------------------------------------------

/// A throwaway git repository for one test, with a root commit so `HEAD` always resolves.
struct TestRepo {
    dir: tempfile::TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = TestRepo { dir };
        repo.git(&["init", "-q"]);
        repo.git(&[
            "-c",
            "user.email=impact-test@example.com",
            "-c",
            "user.name=Impact Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "root",
        ]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Run `git <args>` in this repository, isolated from the developer's own config.
    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed in {:?}", self.path());
    }

    /// Write `rel` (workspace-relative, `/`-separated) with `content`, creating parent directories.
    fn write(&self, rel: &str, content: &str) {
        let path = self.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&[
            "-c",
            "user.email=impact-test@example.com",
            "-c",
            "user.name=Impact Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            message,
        ]);
    }

    fn head(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// A fresh invocation of the built binary, run in this repository. Deliberately does not stub
    /// git config: the invocation exercises the real git boundary `run_impact` drives, none of whose
    /// operations (`rev-parse`, `diff`, `show`, `ls-files`) needs a committer identity.
    fn c10r(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
        cmd.current_dir(self.path());
        cmd
    }
}

/// The workspace identity `build_from_index`/`run_impact` must agree on: the same derivation
/// `resolve_workspace(None, root)` performs (the canonicalized root's directory name).
fn workspace_id(repo: &TestRepo) -> String {
    resolve_workspace(None, repo.path()).unwrap().as_str().to_string()
}

// ---------------------------------------------------------------------------
// The index fixture: the shared `net.rs` fixture plus a second, independent module.
// ---------------------------------------------------------------------------

/// A second, independent module (`other/extra.rs`), alongside the shared `net.rs` fixture, so a
/// path-narrowing test has a second directory whose edit would otherwise also seed something.
fn extra_source() -> &'static str {
    "mod extra {\n    pub fn helper() {}\n}\n"
}

/// The shared `net.rs` fixture's index, extended with `extra::helper`.
fn extended_fixture_index() -> ExtractedIndex {
    let mut index = support::fixture_index();
    index.documents.push(SourceDocument {
        path: "other/extra.rs".to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("extra", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "other/extra.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 9),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![
                DescriptorSegment::new("extra", SegmentKind::Module),
                DescriptorSegment::new("helper", SegmentKind::Method),
            ],
        )),
        kind: SymbolKind::Function,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "other/extra.rs".to_string(),
            range: SourceRange::new(1, 11, 1, 17),
            role: OccurrenceRole::Definition,
        }],
    });
    index
}

/// The sources matching [`extended_fixture_index`].
fn base_sources() -> Vec<(String, String)> {
    vec![
        (support::DOC.to_string(), support::SOURCE.to_string()),
        ("other/extra.rs".to_string(), extra_source().to_string()),
    ]
}

/// A repository with the fixture sources committed and an index built to match them exactly:
/// `(repo, db path, workspace identity)`.
fn setup_repo_with_index() -> (TestRepo, PathBuf, String) {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add fixture sources");

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &base_sources()).unwrap();
    (repo, db, ws)
}

/// Replace 1-based `line` of `source` with `new_line`.
fn edit_line(source: &str, line: usize, new_line: &str) -> String {
    let mut lines: Vec<&str> = source.lines().collect();
    lines[line - 1] = new_line;
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Running `impact` and reading its answer.
// ---------------------------------------------------------------------------

/// Run `impact --json <args>` against `db` in `repo`.
fn run_impact(repo: &TestRepo, db: &Path, args: &[&str]) -> std::process::Output {
    repo.c10r()
        .arg("--db")
        .arg(db)
        .arg("--json")
        .arg("impact")
        .args(args)
        .output()
        .unwrap()
}

/// Run `impact <args>` without `--json`, returning the human projection on standard output. Styling
/// is disabled so an assertion reads the text rather than the escape sequences wrapping it.
fn run_impact_human(repo: &TestRepo, db: &Path, args: &[&str]) -> String {
    let out = repo
        .c10r()
        .arg("--db")
        .arg(db)
        .args(["--color", "never", "impact"])
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("the human render is UTF-8")
}

/// Run `impact --json <args>` and parse the answer, asserting success.
fn run_impact_json(repo: &TestRepo, db: &Path, args: &[&str]) -> serde_json::Value {
    let out = run_impact(repo, db, args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the --json answer parses")
}

/// The sole report of a found `impact` answer.
fn report(answer: &serde_json::Value) -> &serde_json::Value {
    &answer["outcome"]["results"][0]
}

/// The `name` of every seed in a report, in serialized order.
fn seed_names(report: &serde_json::Value) -> Vec<String> {
    report["seeds"]
        .as_array()
        .expect("a seeds array")
        .iter()
        .map(|s| s["symbol"]["name"].as_str().unwrap().to_string())
        .collect()
}

/// The `name` of every detailed dependent in a report, in serialized order.
fn dependent_names(report: &serde_json::Value) -> Vec<String> {
    report["dependents"]["detail"]
        .as_array()
        .expect("a dependents detail array")
        .iter()
        .map(|d| d["symbol"]["name"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Seeding and modes
// ---------------------------------------------------------------------------

// _(Working-tree default mode)_ — editing an indexed symbol in the working tree returns that
// symbol's dependents in the default mode. `Client` is referenced from `open` (a `uses` edge, from
// both its return type and its constructor call), so `open` appears.
#[test]
fn working_tree_default_mode_returns_dependents() {
    let (repo, db, _ws) = setup_repo_with_index();
    let edited = edit_line(support::SOURCE, 2, "    pub struct Client; // touched");
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "working_tree");
    assert_eq!(seed_names(report), vec!["Client".to_string()]);
    assert_eq!(dependent_names(report), vec!["open".to_string()]);
}

// _(Signature-only edit)_ — editing `connect`'s own declaration line seeds `connect`, whose
// dependents are exactly its callers (`open`). This scenario also has nothing else touched, so it
// doubles as the "index matches the diff-reverted set" exactness check: the answer is graded exact.
#[test]
fn signature_only_edit_returns_callers() {
    let (repo, db, _ws) = setup_repo_with_index();
    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(dependent_names(report), vec!["open".to_string()]);
    assert_eq!(
        report["exactness"], "exact",
        "the reverted source set matches what the index was built from: {report}"
    );
}

// _(Staged mode)_ — with `connect`'s edit staged and a separate, unstaged edit to `disconnect` in
// the same file, `--staged` seeds only the staged change.
#[test]
fn staged_mode_seeds_only_the_staged_edit() {
    let (repo, db, _ws) = setup_repo_with_index();

    let connect_edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &connect_edited);
    repo.git(&["add", support::DOC]);

    let both_edited = edit_line(&connect_edited, 5, "        pub fn disconnect(&self, extra: bool) {}");
    repo.write(support::DOC, &both_edited);

    let answer = run_impact_json(&repo, &db, &["--staged"]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "staged");
    assert_eq!(
        seed_names(report),
        vec!["connect".to_string()],
        "the unstaged disconnect edit is invisible to --staged: {report}"
    );
}

// _(Revision range)_ — a two-dot revision range seeds the symbol changed across it.
#[test]
fn revision_range_seeds_the_symbol_changed_across_the_range() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("commit1: fixture sources");
    let sha1 = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("commit2: edit connect");

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &base_sources()).unwrap();

    let range = format!("{sha1}..HEAD");
    let answer = run_impact_json(&repo, &db, &[&range]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "range");
    assert_eq!(report["base_revision"], sha1);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
}

// _(Path narrowing)_ — trailing `--` path narrowing restricts the seed to the named directory: with
// both `net.rs` (under `src/`) and `extra.rs` (under `other/`) edited, narrowing to `src` seeds only
// `connect`.
#[test]
fn path_narrowing_restricts_the_seed_to_the_named_directory() {
    let (repo, db, _ws) = setup_repo_with_index();
    let net_edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &net_edited);
    repo.write("other/extra.rs", "mod extra {\n    pub fn helper(extra: bool) {}\n}\n");

    let answer = run_impact_json(&repo, &db, &["--", "src"]);
    let report = report(&answer);
    assert_eq!(
        seed_names(report),
        vec!["connect".to_string()],
        "narrowing to src/ excludes the other/extra.rs edit: {report}"
    );
}

// _(Rename plus edit)_ — with an index matching the pre-change state, a change that renames a file
// and edits a symbol inside it resolves that symbol at its pre-change path and returns its
// dependents.
#[test]
fn rename_plus_edit_resolves_at_the_pre_change_path() {
    let (repo, db, _ws) = setup_repo_with_index();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    std::fs::remove_file(repo.path().join(support::DOC)).unwrap();
    repo.write("src/net_renamed.rs", &edited);
    // Stage the rename: git only performs rename detection between paths visible to the diff it is
    // computing, and an untracked new path is invisible to a plain `git diff` regardless of mode.
    // Staging it is still observed by the working-tree default mode, since `git diff HEAD` reports
    // the total change (staged and unstaged together) against `HEAD`.
    repo.git(&["add", "-A"]);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(dependent_names(report), vec!["open".to_string()]);
}

// _(Deleted symbol)_ — with an index matching the pre-change state, deleting the indexed file still
// resolves every declaration it held from the pre-change side and reports their dependents.
#[test]
fn deleted_symbol_resolves_from_the_pre_change_side() {
    let (repo, db, _ws) = setup_repo_with_index();
    std::fs::remove_file(repo.path().join(support::DOC)).unwrap();

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    let mut seeds = seed_names(report);
    seeds.sort();
    assert_eq!(
        seeds,
        vec![
            "Client".to_string(),
            "connect".to_string(),
            "disconnect".to_string(),
            "open".to_string(),
        ],
        "the whole deleted file's declarations resolve from the pre-change side: {report}"
    );
    assert!(
        dependent_names(report).contains(&"open".to_string()),
        "connect's and Client's dependent (open) is reported: {report}"
    );
}

// ---------------------------------------------------------------------------
// Exit taxonomy
// ---------------------------------------------------------------------------

// _(Absent git)_ — with no `git` reachable on `PATH`, `impact` exits with the indexer/setup-failure
// code and names the missing tool.
#[test]
fn absent_git_exits_setup_failure() {
    let repo = TestRepo::new();
    let empty_path = tempfile::tempdir().unwrap();

    let out = repo
        .c10r()
        .env("PATH", empty_path.path())
        .arg("--db")
        .arg(repo.path().join(".c10r/index.db"))
        .arg("impact")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(5), "absent git is a setup failure");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The distinctive part of the absent-git diagnostic, not the bare word `git`: the
    // not-a-worktree message names `git` too, so matching on that alone would pass even if an
    // unspawnable executable were reclassified as "this directory is not a worktree".
    assert!(
        stderr.contains("git could not be run"),
        "the diagnostic names the missing tool as unrunnable, not the directory as a non-worktree: {stderr}"
    );
}

// _(Not a git worktree)_ — a directory outside any git repository exits with the same setup-failure
// code.
#[test]
fn non_worktree_directory_exits_setup_failure() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_c10r"))
        .current_dir(dir.path())
        .arg("--db")
        .arg(dir.path().join(".c10r/index.db"))
        .arg("impact")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(5), "not a worktree is a setup failure");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("worktree"),
        "the diagnostic names the problem: {stderr}"
    );
}

/// Write an executable shell script named `git` into `dir` that sleeps far past the bounded
/// deadline, standing in for a wedged `git` on `PATH`.
#[cfg(unix)]
fn write_hanging_git_stub(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("git");
    std::fs::write(&path, "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec sleep 300\n").unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
}

// _(Hanging git)_ — a `git` that never answers still exits with the setup-failure code within a
// bounded window, never an indefinite hang.
#[cfg(unix)]
#[test]
fn hanging_git_exits_setup_failure_within_a_bounded_window() {
    let repo = TestRepo::new();
    let bin_dir = tempfile::tempdir().unwrap();
    write_hanging_git_stub(bin_dir.path());

    let start = Instant::now();
    let out = repo
        .c10r()
        .env("PATH", bin_dir.path())
        .arg("--db")
        .arg(repo.path().join(".c10r/index.db"))
        .arg("impact")
        .output()
        .unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(60),
        "the git boundary is bounded, not waited on indefinitely: {elapsed:?}"
    );
    assert_eq!(out.status.code(), Some(5), "a hanging git is a setup failure");
    // The exit code alone does not separate a wedged `git` from the other two setup failures, both
    // of which exit 5 as well: a regression that reported a stub which never answers as "not a git
    // worktree" would keep this test green without the diagnostic assertion.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bounded deadline"),
        "the diagnostic reports a bounded timeout, not a misclassified worktree failure: {stderr}"
    );
}

// _(Bad revspec)_ — a malformed revision spec exits with the usage code, decided before the store
// (here, absent) is ever opened: a nonexistent `--db` would surface as the no-index code if the
// store were opened first.
#[test]
fn bad_revspec_exits_usage_before_any_traversal() {
    let repo = TestRepo::new();
    let out = run_impact(
        &repo,
        &repo.path().join(".c10r/index.db"),
        &["not-a-real-revision-at-all"],
    );
    assert_eq!(
        out.status.code(),
        Some(2),
        "a bad revspec is a usage error, decided before the absent index would otherwise fire: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// _(Comment-only change)_ — a change that adds a comment line in the gap between declarations exits
// 0 and reports the typed-empty answer, not the file module's whole dependent set.
#[test]
fn comment_only_change_exits_success_with_typed_empty_answer() {
    let (repo, db, _ws) = setup_repo_with_index();

    // Inserted right after the `impl Client { ... }` block's closing brace (line 6): the insertion
    // point's anchor sits in the gap between declarations, so the only span it overlaps is the
    // file-spanning module's, which the whole-file-span exclusion drops.
    //
    // The source keeps its natural trailing newline, so the module's parsed span stops one byte
    // short of the document's length — a declaration's span ends at its last token, and whether the
    // file's final newline falls inside it is a property of the grammar. That one byte must not
    // decide whether a comment edit reports the file's entire dependent set, which is why the
    // exclusion compares the tail after trimming rather than by exact length.
    let mut lines: Vec<&str> = support::SOURCE.lines().collect();
    lines.insert(6, "    // just a comment");
    let mut edited = lines.join("\n");
    edited.push('\n');
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(report["seed_outcome"], "no_indexed_symbol_touched");
    assert!(seed_names(report).is_empty(), "{report}");
    assert!(
        dependent_names(report).is_empty(),
        "a definite none carries no dependents: {report}"
    );
}

// _(A change touching no indexed symbol is typed absence — files the graph does not track)_ — a
// change confined to files source discovery would never collect (a README, a JSON fixture) reaches
// the definite-none answer, and those files are not disclosed as regions the index cannot resolve:
// the graph never had an opinion about them, so calling them unresolved would dress a non-question
// as a resolution gap.
#[test]
fn change_to_untracked_file_kinds_is_typed_absence_not_unresolvable() {
    let (repo, db, _ws) = setup_repo_with_index();
    repo.write("README.md", "# project\n");
    repo.write("fixtures/data.json", "{}\n");
    repo.commit_all("add non-source files");

    repo.write("README.md", "# project\n\nnow with prose.\n");
    repo.write("fixtures/data.json", "{\"key\": 1}\n");

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(
        report["seed_outcome"], "no_indexed_symbol_touched",
        "a change to files the graph does not track is a definite none: {report}"
    );
    assert!(
        report["unmappable"].is_null(),
        "a non-source file is not a region the index failed to resolve: {report}"
    );
}

// ---------------------------------------------------------------------------
// Freshness and recovery
// ---------------------------------------------------------------------------

// _(Unrelated drift without untracked sources)_ — with `connect`'s edit staged and a separate,
// unrelated file (`extra.rs`) edited but not staged, `--staged` seeds only `connect` while the
// answer is graded approximate (the unrelated drift is real, even though it is not part of the seeded
// diff) — and, since nothing here is untracked, the recovery recipe carries no untracked-copy step.
#[test]
fn unrelated_drift_since_the_build_is_approximate_without_an_untracked_copy_step() {
    let (repo, db, ws) = setup_repo_with_index();

    let connect_edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &connect_edited);
    repo.git(&["add", support::DOC]);
    repo.write("other/extra.rs", "mod extra {\n    pub fn helper(extra: bool) {}\n}\n");

    let answer = run_impact_json(&repo, &db, &["--staged"]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(
        report["exactness"], "approximate",
        "the unstaged, unrelated extra.rs edit is real drift the diff never reverts: {report}"
    );
    let recovery = &report["recovery"];
    assert!(!recovery.is_null(), "an approximate answer carries a recipe: {report}");
    assert_eq!(recovery["workspace"], ws);
    let steps = recovery["steps"].as_array().unwrap();
    assert!(
        !steps.iter().any(|s| s.as_str().unwrap().starts_with("for f in")),
        "no untracked sources exist, so the recipe carries no copy step: {steps:?}"
    );
}

// _(The critical regression: an untracked source, unedited since the build, stays exact)_ — a
// discovered-but-untracked source present at build time, with no edits since apart from the change
// under assessment, still grades the answer exact. A base-git-tree definition of the pre-change side
// would instead see this file as permanently absent from the tree and grade every answer
// approximate forever.
#[test]
fn untracked_source_present_at_build_unedited_stays_exact() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add tracked fixture sources");
    // Never `git add`ed: present when the index is built, but untracked.
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &sources).unwrap();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(
        report["exactness"], "exact",
        "the untracked, unedited source contributes identically to both hashes: {report}"
    );
}

// _(Untracked source edited after the build)_ — the same untracked source, edited after the build,
// grades the answer approximate; the recovery recipe carries the substituted base revision and
// `--workspace` identity, builds into a throwaway index rather than the queried one, and includes
// the untracked-copy step because an untracked discovered source exists.
#[test]
fn untracked_source_edited_after_the_build_is_approximate_with_the_copy_step() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add tracked fixture sources");
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &sources).unwrap();

    // Edit connect (the seed) and the untracked scratch file (drift the untracked-source hash).
    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.write("src/scratch.rs", "pub fn scratch_edited() {}\n");

    let head = repo.head();
    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(
        report["exactness"], "approximate",
        "the untracked source drifted since the build: {report}"
    );

    let recovery = &report["recovery"];
    assert_eq!(recovery["base_revision"], head);
    assert_ne!(
        recovery["db"],
        db.display().to_string(),
        "the recipe must never name the queried index: {recovery}"
    );
    assert_eq!(recovery["workspace"], ws);
    let steps: Vec<&str> = recovery["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(
        steps.iter().any(|s| s.starts_with("for f in")),
        "an untracked discovered source exists, so the recipe includes the copy step: {steps:?}"
    );
    assert!(
        !steps
            .iter()
            .any(|s| s.starts_with("c10r build") && s.contains(&db.display().to_string())),
        "the build step must not name the queried index: {steps:?}"
    );
}

// _(Staged mode cannot reconstruct a further unstaged edit to the same file)_ — substituting the
// staged file's base content also discards its own further unstaged edit, landing on exactly the
// base state, which an index built at the base then matches by hash alone. Reconstructibility gates
// the exact claim independently of the hash comparison, so this grades approximate — agreeing with
// the grade the same kind of drift in a file the change never touched already produces (see
// `unrelated_drift_since_the_build_is_approximate_without_an_untracked_copy_step` above, where the
// drift lands in `extra.rs` instead of the staged file itself).
#[test]
fn staged_edit_with_a_further_unstaged_edit_to_the_same_file_is_approximate() {
    let (repo, db, _ws) = setup_repo_with_index();

    let connect_edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &connect_edited);
    repo.git(&["add", support::DOC]);

    // A further, unstaged edit to the SAME file the staged diff already touches.
    let both_edited = edit_line(&connect_edited, 5, "        pub fn disconnect(&self, extra: bool) {}");
    repo.write(support::DOC, &both_edited);

    let answer = run_impact_json(&repo, &db, &["--staged"]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(
        report["exactness"], "approximate",
        "the base-content substitution silently discards the unstaged edit too, so a hash match \
         alone must not be trusted here: {report}"
    );
}

// _(A range whose head is not the checkout cannot be reconstructed)_ — reverting `A..B`'s diff from
// a checkout that is not `B` yields a hybrid no index can ever match, so the range grades
// approximate even when the reverted hash happens to agree with the index. The extra commit below
// touches only a file source discovery never collects, so the discovered `.rs` tree at HEAD stays
// byte-for-byte identical to `B`'s — isolating that the reconstructibility gate, not a hash
// mismatch, drives the grade here.
#[test]
fn range_whose_head_is_not_the_checkout_is_approximate_even_when_the_hash_matches() {
    let (repo, db, _ws) = setup_repo_with_index();
    let sha1 = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("commit2: edit connect");
    let sha2 = repo.head();

    // Advance past B with a change to a file source discovery never collects, so the discovered
    // `.rs` sources at HEAD stay identical to B's tree.
    repo.write("README.md", "# advanced past B\n");
    repo.commit_all("commit3: advance past B, touching only a non-source file");

    let range = format!("{sha1}..{sha2}");
    let answer = run_impact_json(&repo, &db, &[&range]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "range");
    assert_eq!(report["base_revision"], sha1);
    assert_eq!(
        report["exactness"], "approximate",
        "HEAD is commit3, not the range's head (commit2), so the pre-change side cannot be \
         reconstructed even though the reverted hash matches the index: {report}"
    );
}

// _(Unmappable regions disclosed on an approximate answer)_ — a change to a tracked file the index
// never indexed produces a disclosed unmappable region, alongside a real seed and an approximate
// grade — the region is disclosed, not dropped, even though the answer is already approximate for an
// unrelated reason.
#[test]
fn unmappable_regions_are_disclosed_on_an_approximate_answer() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    // Tracked, but never referenced by the index at all.
    repo.write("src/untouched.rs", "pub fn untouched() {}\n");
    repo.commit_all("add fixture plus an unindexed tracked file");
    // Untracked at build time, to drive the approximate grade.
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/untouched.rs".to_string(), "pub fn untouched() {}\n".to_string()));
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &sources).unwrap();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.write("src/untouched.rs", "pub fn untouched_edited() {}\n");
    repo.write("src/scratch.rs", "pub fn scratch_edited() {}\n");

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(report["exactness"], "approximate");
    let unmappable = report["unmappable"].as_array().expect("unmappable regions disclosed");
    assert!(
        unmappable.iter().any(|u| u["document_path"] == "src/untouched.rs"),
        "the region in the unindexed tracked file is disclosed: {report}"
    );
}

// ---------------------------------------------------------------------------
// Output contract
// ---------------------------------------------------------------------------

// _(JSON/human parity)_ — `--json` and the human render present the same seeds and the same
// dependents, in the same order.
#[test]
fn json_and_human_render_present_the_same_seeds_and_dependents_in_the_same_order() {
    let (repo, db, _ws) = setup_repo_with_index();
    std::fs::remove_file(repo.path().join(support::DOC)).unwrap();

    let json_answer = run_impact_json(&repo, &db, &[]);
    let report = report(&json_answer);
    let json_seeds = seed_names(report);
    let json_deps = dependent_names(report);

    let human = repo.c10r().arg("--db").arg(&db).arg("impact").output().unwrap();
    assert_eq!(human.status.code(), Some(0));
    let text = String::from_utf8_lossy(&human.stdout);

    // Every seed and every dependent name appears in the human text, in the same relative order as
    // the JSON answer.
    let mut cursor = 0usize;
    for name in json_seeds.iter().chain(json_deps.iter()) {
        let pos = text[cursor..]
            .find(name.as_str())
            .unwrap_or_else(|| panic!("{name} appears in the human render after position {cursor}: {text}"));
        cursor += pos + name.len();
    }
}

// _(Result-set bounding)_ — an over-limit impact answer is capped, discloses truncation, and its
// cursor resumes exactly. Read the cursor from one answer and pass it straight back through
// `--cursor`, treating it as opaque throughout.
#[test]
fn over_limit_answer_is_capped_and_the_cursor_resumes_exactly() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());

    let caller_count = 30usize;
    let (callers_source, caller_symbols, connect_refs) = build_callers(caller_count);
    repo.write("src/callers.rs", &callers_source);
    repo.commit_all("add fixture sources plus many callers");

    let mut index = extended_fixture_index();
    index.documents.push(SourceDocument {
        path: "src/callers.rs".to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("callers", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/callers.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 11),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.extend(caller_symbols);
    for sym in index.symbols.iter_mut() {
        if sym.terminal_name() == Some("connect") {
            sym.occurrences.extend(connect_refs);
            break;
        }
    }

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/callers.rs".to_string(), callers_source));
    build_from_index(&db, &ws, repo.path(), &index, &sources).unwrap();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    // 31 total dependents (open, plus 30 generated callers); the default limit of 25 caps the first
    // page.
    let first = run_impact_json(&repo, &db, &[]);
    let report_first = report(&first);
    let first_detail = dependent_names(report_first);
    assert_eq!(first_detail.len(), 25, "{report_first}");
    assert_eq!(first["page"]["truncated"], true);
    assert_eq!(first["page"]["total"], 31);
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    assert_eq!(report_first["exactness"], "exact", "{report_first}");

    let second = run_impact_json(&repo, &db, &["--cursor", &cursor]);
    let second_detail = dependent_names(report(&second));
    assert_eq!(second_detail.len(), 6, "the remaining six resume: {second}");
    assert!(
        first_detail.iter().all(|n| !second_detail.contains(n)),
        "the second page does not overlap the first: {first_detail:?} / {second_detail:?}"
    );
    assert_eq!(
        report(&second)["exactness"],
        "exact",
        "the freshness label accompanies every page, not just the first: {second}"
    );
}

/// Build `count` caller functions (`caller00`, `caller01`, …) inside `mod callers { ... }`, each
/// calling `connect()`, together with their persisted symbols and their reference occurrences onto
/// `connect`. Returns `(source_text, caller_symbols, connect_reference_occurrences)`.
fn build_callers(count: usize) -> (String, Vec<ExtractedSymbol>, Vec<ExtractedOccurrence>) {
    assert!(count < 100, "the fixed two-digit width assumes count < 100");
    let mut source = String::from("mod callers {\n");
    let mut symbols = Vec::with_capacity(count);
    let mut references = Vec::with_capacity(count);
    for i in 0..count {
        let name = format!("caller{i:02}");
        source.push_str(&format!("    pub fn {name}() {{ connect(); }}\n"));
        let line = (i + 1) as u32;
        let name_start = "    pub fn ".len() as u32;
        let name_end = name_start + name.len() as u32;
        let connect_start = name_end + "() { ".len() as u32;
        let connect_end = connect_start + "connect".len() as u32;
        symbols.push(ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "mycrate",
                vec![
                    DescriptorSegment::new("callers", SegmentKind::Module),
                    DescriptorSegment::new(&name, SegmentKind::Method),
                ],
            )),
            kind: SymbolKind::Function,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: "src/callers.rs".to_string(),
                range: SourceRange::new(line, name_start, line, name_end),
                role: OccurrenceRole::Definition,
            }],
        });
        references.push(ExtractedOccurrence {
            document_path: "src/callers.rs".to_string(),
            range: SourceRange::new(line, connect_start, line, connect_end),
            role: OccurrenceRole::Reference,
        });
    }
    source.push_str("}\n");
    (source, symbols, references)
}

// _(Uncapped diff capture)_ — a diff larger than 64 KiB seeds every touched symbol, including one
// whose hunk falls well past that point in the patch: a capture-capped diff would silently drop it.
#[test]
fn diff_larger_than_64kib_seeds_every_touched_symbol() {
    let repo = TestRepo::new();

    // A tiny symbol at the top, a single huge placeholder line in the middle (short in the committed
    // version, ~70 KiB in the edited working-tree version — comfortably past the 64 KiB capture cap
    // on its own), and a tiny symbol at the bottom, so the bottom symbol's hunk sits well past the
    // cap in the emitted patch.
    let committed = "mod giant {\n    pub fn sym_first() {}\n    // filler: short\n    pub fn sym_last() {}\n}\n";
    repo.write("src/giant.rs", committed);
    repo.commit_all("add the giant fixture");

    let mut index = ExtractedIndex {
        provenance: support::provenance(),
        documents: vec![SourceDocument {
            path: "src/giant.rs".to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: Vec::new(),
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    };
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("giant", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/giant.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 9),
            role: OccurrenceRole::Definition,
        }],
    });
    for (name, line) in [("sym_first", 1u32), ("sym_last", 3u32)] {
        let name_start = "    pub fn ".len() as u32;
        let name_end = name_start + name.len() as u32;
        index.symbols.push(ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "mycrate",
                vec![
                    DescriptorSegment::new("giant", SegmentKind::Module),
                    DescriptorSegment::new(name, SegmentKind::Method),
                ],
            )),
            kind: SymbolKind::Function,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: "src/giant.rs".to_string(),
                range: SourceRange::new(line, name_start, line, name_end),
                role: OccurrenceRole::Definition,
            }],
        });
    }

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    build_from_index(
        &db,
        &ws,
        repo.path(),
        &index,
        &[("src/giant.rs".to_string(), committed.to_string())],
    )
    .unwrap();

    let filler = format!("    // filler: {}", "x".repeat(70_000));
    let edited = format!(
        "mod giant {{\n    pub fn sym_first() {{}} // touched\n{filler}\n    pub fn sym_last() {{}} // touched\n}}\n"
    );
    assert!(
        edited.len() > 64 * 1024,
        "the edited file exceeds 64 KiB: {} bytes",
        edited.len()
    );
    repo.write("src/giant.rs", &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    let seeds = seed_names(report);
    assert!(
        seeds.contains(&"sym_first".to_string()),
        "the symbol before the giant hunk is seeded: {report}"
    );
    assert!(
        seeds.contains(&"sym_last".to_string()),
        "the symbol after the giant hunk (past the 64 KiB mark) is still seeded — a capped capture \
         would silently drop it: {report}"
    );
}

// _(Path narrowing reaches a renamed file by its post-change path)_ — narrowing is applied to the
// parsed change set, not handed to `git diff` as a pathspec. A pathspec is applied before rename
// detection, so narrowing to a renamed file's post-change directory would leave git nothing to pair
// the new path against and it would degrade to an addition with no pre-change side, silently seeding
// nothing for exactly the file the caller asked about.
#[test]
fn narrowing_to_a_renamed_files_post_change_path_still_seeds_it() {
    let (repo, db, _ws) = setup_repo_with_index();

    // Edit one line and move the file into a new directory. The fixture is many lines long, so the
    // single-line edit leaves it far above git's default 50% rename-similarity threshold; a one-line
    // file edited on its only line would be reported as a delete plus an add instead.
    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    std::fs::remove_file(repo.path().join(support::DOC)).unwrap();
    repo.write("moved/net.rs", &edited);
    repo.git(&["add", "-A"]);

    // Guard the premise: this test is only meaningful if git really did detect a rename.
    let patch = Command::new("git")
        .args(["diff", "--find-renames", "--no-prefix", "HEAD"])
        .current_dir(repo.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    let patch = String::from_utf8_lossy(&patch.stdout);
    assert!(
        patch.contains("rename from"),
        "the fixture must produce a detected rename, not a delete plus an add: {patch}"
    );

    // Narrowed to the POST-change directory, which the pre-change path is not under.
    let answer = run_impact_json(&repo, &db, &["--", "moved"]);
    let narrowed = report(&answer);
    assert_eq!(
        seed_names(narrowed),
        vec!["connect".to_string()],
        "the renamed file's edit seeds from its pre-change side: {narrowed}"
    );
    assert_eq!(dependent_names(narrowed), vec!["open".to_string()]);

    // And by its pre-change directory, which must keep working.
    let by_pre_path = run_impact_json(&repo, &db, &["--", "src"]);
    assert_eq!(seed_names(report(&by_pre_path)), vec!["connect".to_string()]);
}

/// A repository whose fixture sources gain `count` extra callers of `connect`, indexed to match:
/// an edit to `connect` then yields enough dependents to truncate the default-bounded page, which
/// is what the continuation-token tests need a cursor from.
fn repo_with_many_connect_callers(count: usize) -> (TestRepo, PathBuf) {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());

    let (callers_source, caller_symbols, connect_refs) = build_callers(count);
    repo.write("src/callers.rs", &callers_source);
    repo.commit_all("add fixture sources plus many callers");

    let mut index = extended_fixture_index();
    index.documents.push(SourceDocument {
        path: "src/callers.rs".to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("callers", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/callers.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 11),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.extend(caller_symbols);
    for sym in index.symbols.iter_mut() {
        if sym.terminal_name() == Some("connect") {
            sym.occurrences.extend(connect_refs);
            break;
        }
    }

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/callers.rs".to_string(), callers_source));
    build_from_index(&db, &ws, repo.path(), &index, &sources).unwrap();
    (repo, db)
}

// _(Mismatched continuation token refused)_ — an impact answer is a function of the index *and* the
// diff, and the diff is not a flag. A token minted against one change must not resume against a
// different one: the working tree can move between two pages, which would silently drop or duplicate
// rows under a stable-looking token.
#[test]
fn a_cursor_is_refused_after_the_seeding_change_moves() {
    let (repo, db) = repo_with_many_connect_callers(30);

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    let first = run_impact_json(&repo, &db, &[]);
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    // Move the seeding change: same flags, same base revision, different diff.
    let edited_again = edit_line(&edited, 5, "        pub fn disconnect(&self, more: bool) {}");
    repo.write(support::DOC, &edited_again);

    let out = run_impact(&repo, &db, &["--cursor", &cursor]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a token issued against a different change is refused, not resumed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// _(Dependents order selector: a continuation token binds its ordering — impact)_ — `impact`
// populates the token identity at its own site, apart from `trace`'s, so the binding is pinned
// here through the real command: a cursor minted under the ranked default is refused when
// presented with `--order unranked`, while the same cursor under the same ordering resumes.
#[test]
fn an_impact_cursor_issued_under_ranked_is_refused_under_unranked() {
    let (repo, db) = repo_with_many_connect_callers(30);

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    let first = run_impact_json(&repo, &db, &[]);
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    let out = run_impact(&repo, &db, &["--order", "unranked", "--cursor", &cursor]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "an ordering switch refuses the token as a usage error: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("different query parameters"),
        "the refusal names the identity mismatch: {stderr}"
    );

    // The same token under the same ordering resumes — the refusal above is the ordering switch,
    // not the token itself.
    let resumed = run_impact_json(&repo, &db, &["--cursor", &cursor]);
    assert_eq!(
        resumed["page"]["page_index"], 1,
        "the unswitched cursor resumes the second page: {resumed}"
    );
}

// _(Range answers disclose which snapshot their dependents reflect)_ — there is one index, so a
// range-seeded answer resolves its seeds against the range's pre-change side while drawing its
// dependents from whatever the index holds now. The machine answer names that snapshot on every
// answer; the human render says so only for a range, where the straddle is the surprising part.
#[test]
fn a_range_answer_discloses_the_dependents_snapshot() {
    let (repo, db, _ws) = setup_repo_with_index();
    let base = repo.head();
    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("edit connect");
    let head = repo.head();

    let range = format!("{base}..{head}");
    let answer = run_impact_json(&repo, &db, &[&range]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "range");
    assert_eq!(
        report["dependents_snapshot"], "current_index",
        "the machine answer names the snapshot the dependents came from: {report}"
    );

    let human = run_impact_human(&repo, &db, &[&range]);
    assert!(
        human.contains("current index"),
        "a range answer's render discloses the snapshot: {human}"
    );

    // The working-tree mode's base revision already is the snapshot its dependents reflect, so the
    // line would be noise there.
    let plain = run_impact_human(&repo, &db, &[]);
    assert!(
        !plain.contains("current index"),
        "a working-tree answer carries no snapshot disclosure: {plain}"
    );
}

// _(Following the recovery recipe never touches the queried index)_ — the recipe's own steps are
// executed for real, exactly as a caller pasting the block would run them, and the queried index's
// bytes are compared before and after.
//
// The `c10r build`/`c10r impact` steps are skipped: this test suite's own convention (see the file
// header) is that a live `rust-analyzer` is not exercised by these process tests — every index here
// is built via `build_from_index` over a hand-built `ExtractedIndex` instead, and the throwaway
// worktree's stub `.rs` files carry no `Cargo.toml` a real `rust-analyzer` run could resolve against.
// Every other step — both `git worktree add`s, the untracked-copy loop, `git worktree remove`, and
// the final `rm -f` — runs for real, which is exactly the machinery Fix B/D/E's contract is about:
// none of it may read or write the path the answer was queried from.
#[test]
fn following_the_recipe_leaves_the_queried_index_byte_for_byte_unchanged() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add tracked fixture sources");
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &sources).unwrap();

    // Edit connect (the seed) and the untracked scratch file (drift the untracked-source hash), so
    // the answer is approximate and its recipe carries the untracked-copy step.
    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.write("src/scratch.rs", "pub fn scratch_edited() {}\n");

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(report["exactness"], "approximate", "{report}");
    let steps: Vec<String> = report["recovery"]["steps"]
        .as_array()
        .expect("recovery steps")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .filter(|s| !s.starts_with("c10r build") && !s.contains("c10r impact"))
        .collect();

    let before = std::fs::read(&db).unwrap();

    // `set -e` so a failing step aborts the script immediately: without it, `sh`'s exit status is
    // just the last command's, and a comment-only or `rm -f` final line would report success even if
    // an earlier `git worktree add` failed.
    let script = format!("set -e\n{}", steps.join("\n"));
    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(repo.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("sh runs");
    assert!(status.success(), "the recipe's non-c10r steps failed to run:\n{script}");

    // Read rather than unwrap blindly: the defect this pins removed the queried index outright, so
    // the absence must read as "the recipe destroyed it" rather than as a bare I/O panic.
    let after = std::fs::read(&db).unwrap_or_else(|e| {
        panic!(
            "the recipe removed the index it was queried from ({}): {e}",
            db.display()
        )
    });
    assert_eq!(
        before, after,
        "the recipe must never write the index it was queried from"
    );
}

// ---------------------------------------------------------------------------
// Following the recovery recipe to convergence
// ---------------------------------------------------------------------------
//
// The delta requires that following an approximate answer's recipe *yields an exact answer for the
// seed mode it was emitted for*. That is a claim about the recipe as a whole, so the tests below run
// it as a whole: every emitted step executes for real through `sh`, except the single `c10r build`
// line, which cannot run here (no live `rust-analyzer` — see the file header). The substitute is
// driven entirely by the recipe's own text: the root, `--db`, and `--workspace` the substitute uses
// are parsed out of the emitted build step and its own variable assignments, so a recipe that names
// the wrong tree, the wrong index, or the wrong workspace identity builds the wrong index and the
// re-run reports approximate.

/// Split a shell command line into its words, respecting single quotes, double quotes, and
/// backslash escapes. Quotes are kept in the returned words; [`expand_word`] removes them.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single => {
                current.push(c);
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
                started = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(c);
                started = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(c);
                started = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// Expand one shell word to the literal string `sh` would pass as an argument: strip quoting, honor
/// backslash escapes, and substitute `$NAME` / `${NAME}` / `${NAME:-default}` from `vars`.
///
/// # Panics
///
/// Panics when the word expands a variable `vars` does not carry and that has no default — a recipe
/// referring to a variable it never set would otherwise silently expand to nothing.
fn expand_word(word: &str, vars: &BTreeMap<String, String>) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < chars.len() {
        match chars[i] {
            '\\' if !in_single => {
                i += 1;
                if i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '\'' if !in_double => {
                in_single = !in_single;
                i += 1;
            }
            '"' if !in_single => {
                in_double = !in_double;
                i += 1;
            }
            '$' if !in_single => {
                i += 1;
                let (name, default) = if chars.get(i) == Some(&'{') {
                    i += 1;
                    let mut body = String::new();
                    while i < chars.len() && chars[i] != '}' {
                        body.push(chars[i]);
                        i += 1;
                    }
                    i += 1; // past the closing brace
                    match body.split_once(":-") {
                        Some((name, default)) => (name.to_string(), Some(default.to_string())),
                        None => (body, None),
                    }
                } else {
                    let mut name = String::new();
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        name.push(chars[i]);
                        i += 1;
                    }
                    (name, None)
                };
                let value = vars
                    .get(&name)
                    .cloned()
                    .or(default)
                    .unwrap_or_else(|| panic!("the recipe expands ${name}, which no earlier step sets"));
                out.push_str(&value);
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Whether `step` is a plain shell variable assignment (`NAME=value`).
fn is_assignment(step: &str) -> bool {
    match step.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        }
        None => false,
    }
}

/// The shell variables the recipe's own assignment steps define, resolved in order against `tmpdir`
/// as `TMPDIR` — the same value the executed steps run with, so the paths this test resolves and the
/// paths the recipe creates are the same paths.
fn recipe_vars(steps: &[String], tmpdir: &Path) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert("TMPDIR".to_string(), tmpdir.display().to_string());
    for step in steps {
        if !is_assignment(step) {
            continue;
        }
        let (name, rhs) = step.split_once('=').expect("is_assignment checked the shape");
        let value = expand_word(rhs, &vars);
        vars.insert(name.to_string(), value);
    }
    vars
}

/// The root, index path, and workspace identity the recipe's `c10r build` step names, with every
/// shell word unquoted and expanded.
struct BuildStep {
    root: PathBuf,
    db: PathBuf,
    workspace: String,
}

fn parse_build_step(step: &str, vars: &BTreeMap<String, String>) -> BuildStep {
    let words: Vec<String> = split_words(step).iter().map(|w| expand_word(w, vars)).collect();
    assert!(words.len() >= 3, "the build step has a positional root: {step}");
    assert_eq!(words[0], "c10r", "{step}");
    assert_eq!(words[1], "build", "{step}");
    assert!(
        !words[2].starts_with('-'),
        "the build step's third word is its positional root: {step}"
    );
    let mut db = None;
    let mut workspace = None;
    let mut i = 3;
    while i < words.len() {
        let value = words
            .get(i + 1)
            .unwrap_or_else(|| panic!("{} carries a value in the build step: {step}", words[i]));
        match words[i].as_str() {
            "--db" => db = Some(value.clone()),
            "--workspace" => workspace = Some(value.clone()),
            other => panic!("unexpected flag {other} in the recipe's build step: {step}"),
        }
        i += 2;
    }
    BuildStep {
        root: PathBuf::from(&words[2]),
        db: PathBuf::from(db.expect("the build step names --db")),
        workspace: workspace.expect("the build step names --workspace"),
    }
}

/// Put the binary under test on a `PATH` under `dir` as plain `c10r`, so the recipe's own
/// `c10r impact` line runs verbatim against it rather than against whatever `c10r` the developer
/// happens to have installed. Returns the `PATH` value to run the recipe's steps with.
fn path_with_c10r(dir: &Path) -> OsString {
    let exe = Path::new(env!("CARGO_BIN_EXE_c10r"));
    let linked = dir.join("c10r");
    if std::fs::hard_link(exe, &linked).is_err() {
        std::fs::copy(exe, &linked).expect("the binary under test can be placed on the recipe's PATH");
    }
    let mut path = dir.as_os_str().to_os_string();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    path
}

/// Run `lines` as one `sh` script under `set -e`, in `dir`, with the recipe's `TMPDIR` and the
/// binary under test on `PATH`.
fn run_recipe_steps(lines: &[String], dir: &Path, tmpdir: &Path, path: &OsString) -> std::process::Output {
    let script = format!("set -e\n{}", lines.join("\n"));
    let out = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(dir)
        .env("TMPDIR", tmpdir)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("sh runs");
    assert!(
        out.status.success(),
        "a recipe step failed:\n{script}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Follow `report`'s recovery recipe end to end and return the answer its own re-run step produces.
///
/// Only the `c10r build` line is substituted, and the substitution reads its root, index path, and
/// workspace identity out of that very line: `build_from_index` stands in for the analyzer, over the
/// sources actually discovered on disk at the root the recipe named. Every other step — the worktree
/// materializations, the untracked-copy loop, the re-run, and the cleanup — is executed by `sh`
/// exactly as emitted, from the directory the recipe's own text dictates.
fn follow_recipe(
    report: &serde_json::Value,
    workspace_dir: &Path,
    tmpdir: &Path,
    index: &ExtractedIndex,
) -> serde_json::Value {
    let steps: Vec<String> = report["recovery"]["steps"]
        .as_array()
        .expect("an approximate answer carries recovery steps")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();

    let vars = recipe_vars(&steps, tmpdir);
    let build_at = steps
        .iter()
        .position(|s| s.starts_with("c10r build"))
        .expect("the recipe builds a throwaway index");
    let rerun_at = steps
        .iter()
        .position(|s| s.contains("c10r impact"))
        .expect("the recipe re-runs the assessment");
    assert!(build_at < rerun_at, "the recipe builds before it re-queries: {steps:?}");

    let assignments: Vec<String> = steps.iter().filter(|s| is_assignment(s)).cloned().collect();
    let bin_dir = tempfile::tempdir().unwrap();
    let path = path_with_c10r(bin_dir.path());

    // Everything up to the build step: the variable assignments, the worktree materializations, and
    // the untracked-copy loop when one was emitted.
    run_recipe_steps(&steps[..build_at], workspace_dir, tmpdir, &path);

    // The one substituted step, driven by the emitted build step's own words.
    let build = parse_build_step(&steps[build_at], &vars);
    let sources = collect_rust_sources(&build.root)
        .unwrap_or_else(|e| panic!("the recipe's build root {} is readable: {e}", build.root.display()));
    assert!(
        !sources.is_empty(),
        "the recipe's build root {} holds the workspace's sources",
        build.root.display()
    );
    build_from_index(&build.db, &build.workspace, &build.root, index, &sources).unwrap();

    // The re-run, verbatim apart from `--json`: the answer has to be machine-readable to be asserted
    // on, and `--json` is a global flag, so it goes in front of the subcommand without disturbing
    // the step's own arguments or its surrounding subshell.
    let rerun = steps[rerun_at].replace("c10r impact", "c10r --json impact");
    let mut rerun_script = assignments.clone();
    rerun_script.push(rerun);
    let out = run_recipe_steps(&rerun_script, workspace_dir, tmpdir, &path);
    let answer: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("the recipe's re-run emits a parseable --json answer");

    // The cleanup steps, also for real: the recipe promises to leave nothing behind.
    let mut cleanup = assignments;
    cleanup.extend(steps[rerun_at + 1..].iter().cloned());
    run_recipe_steps(&cleanup, workspace_dir, tmpdir, &path);

    answer
}

// _(Following the recipe converges — working-tree mode)_ — an approximate working-tree answer's
// recipe, run end to end, produces an exact answer. This is the executable evidence for "following
// it SHALL yield an exact answer for the seed mode it was emitted for": the recipe's throwaway index
// is built at the base revision *with the untracked source copied in*, which is exactly what makes
// the re-run's reverted hash match — drop the copy step and the re-run stays approximate forever.
#[test]
fn following_the_recipe_converges_to_an_exact_working_tree_answer() {
    let tmpdir = tempfile::tempdir().unwrap();
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add tracked fixture sources");
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &sources).unwrap();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.write("src/scratch.rs", "pub fn scratch_edited() {}\n");

    let answer = run_impact_json(&repo, &db, &[]);
    let first = report(&answer);
    assert_eq!(first["exactness"], "approximate", "{first}");

    let rerun = follow_recipe(first, repo.path(), tmpdir.path(), &extended_fixture_index());
    let converged = report(&rerun);
    assert_eq!(converged["seed_mode"], "working_tree", "{rerun}");
    assert_eq!(
        converged["exactness"], "exact",
        "following the recipe yields the exact answer it promises: {rerun}"
    );
}

// _(Following the recipe converges — revision-range mode)_ — the same end-to-end run for a range,
// whose recipe additionally materializes the range's head and re-queries from it. Reverting an
// `A..B` diff reconstructs `A` only from a tree at `B`, so a recipe that re-ran from the original
// workspace would recompute the same unreconstructable hybrid and never converge.
//
// The checkout is deliberately advanced past the range's head, which is what makes the head-side
// worktree load-bearing: with the workspace sitting at `B` the re-run would converge from there too,
// and the test would pass even for a recipe that never materialized the head at all.
#[test]
fn following_the_recipe_converges_to_an_exact_range_answer() {
    let tmpdir = tempfile::tempdir().unwrap();
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("commit1: fixture sources");
    let sha1 = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("commit2: edit connect");
    let sha2 = repo.head();

    // Advance past the range's head with a change to a file source discovery never collects, so the
    // checkout is no longer the range's head while the discovered `.rs` tree still is.
    repo.write("README.md", "# advanced past the range head\n");
    repo.commit_all("commit3: advance past the range head, touching only a non-source file");
    assert_ne!(repo.head(), sha2, "the checkout has moved past the range's head");

    // Built over the range's HEAD-side tree, so the index does not match the range's pre-change side
    // and the answer is approximate for a reason the recipe can actually repair.
    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let head_sources = vec![
        (support::DOC.to_string(), edited.clone()),
        ("other/extra.rs".to_string(), extra_source().to_string()),
    ];
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &head_sources).unwrap();

    let range = format!("{sha1}..{sha2}");
    let answer = run_impact_json(&repo, &db, &[&range]);
    let first = report(&answer);
    assert_eq!(first["seed_mode"], "range");
    assert_eq!(first["exactness"], "approximate", "{first}");

    let rerun = follow_recipe(first, repo.path(), tmpdir.path(), &extended_fixture_index());
    let converged = report(&rerun);
    assert_eq!(converged["seed_mode"], "range", "{rerun}");
    assert_eq!(
        converged["exactness"], "exact",
        "the range recipe converges from a worktree at the range's head: {rerun}"
    );
}

// _(A range recipe converges for a workspace below the repository root)_ — the re-run step must
// `cd` to the workspace's own directory inside the head worktree, not to the worktree's root.
// Landing on the repository root makes the re-run's discovered set the whole repository rather than
// the workspace subtree, so its pre-change hash can never match an index built over that subtree and
// the procedure never converges — permanently approximate for every workspace that is not the
// repository root.
#[test]
fn a_range_recipe_converges_for_a_workspace_below_the_repository_root() {
    let tmpdir = tempfile::tempdir().unwrap();
    let repo = TestRepo::new();
    repo.write("sub/src/net.rs", support::SOURCE);
    repo.write("sub/other/extra.rs", extra_source());
    repo.commit_all("commit1: fixture sources under sub/");
    let sha1 = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write("sub/src/net.rs", &edited);
    repo.commit_all("commit2: edit connect");
    let sha2 = repo.head();

    // Advance past the range's head, so the re-run genuinely has to happen in the head-side worktree
    // — and therefore in that worktree's `sub/` directory rather than at its root.
    repo.write("README.md", "# advanced past the range head\n");
    repo.commit_all("commit3: advance past the range head, touching only a non-source file");

    let workspace_dir = repo.path().join("sub");
    let db = workspace_dir.join(".c10r/index.db");
    let ws = resolve_workspace(None, &workspace_dir).unwrap().as_str().to_string();
    let head_sources = vec![
        (support::DOC.to_string(), edited.clone()),
        ("other/extra.rs".to_string(), extra_source().to_string()),
    ];
    build_from_index(&db, &ws, &workspace_dir, &extended_fixture_index(), &head_sources).unwrap();

    let range = format!("{sha1}..{sha2}");
    let out = Command::new(env!("CARGO_BIN_EXE_c10r"))
        .current_dir(&workspace_dir)
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .arg("impact")
        .arg(&range)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    let first = report(&answer);
    assert_eq!(first["seed_mode"], "range");
    assert_eq!(first["exactness"], "approximate", "{first}");

    // The emitted step itself, before running anything: the re-run's directory carries the
    // workspace's repository-relative prefix.
    let steps: Vec<String> = first["recovery"]["steps"]
        .as_array()
        .expect("recovery steps")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let rerun = steps
        .iter()
        .find(|s| s.starts_with("( cd "))
        .expect("a range recipe re-runs in a subshell cd'd to the head worktree");
    assert!(
        rerun.contains("/sub/"),
        "the re-run's directory carries the workspace prefix, not the worktree root: {rerun}"
    );

    let rerun_answer = follow_recipe(first, &workspace_dir, tmpdir.path(), &extended_fixture_index());
    let converged = report(&rerun_answer);
    assert_eq!(converged["seed_mode"], "range", "{rerun_answer}");
    assert_eq!(
        converged["exactness"], "exact",
        "a workspace below the repository root converges too: {rerun_answer}"
    );
}

// ---------------------------------------------------------------------------
// Reconstructibility partitions
// ---------------------------------------------------------------------------

// _(A bare single-revision spec names no range head)_ — `git diff <rev>` spans that revision to the
// worktree exactly as the working-tree mode spans `HEAD` to the worktree, so nothing sits outside
// the selected diff and the pre-change side is reconstructible. HEAD here is deliberately *not*
// `<rev>`: treating a bare spec as a range would compare its head against the checkout, find them
// different, and grade this approximate.
#[test]
fn a_bare_single_revision_spec_is_reconstructible() {
    let (repo, db, _ws) = setup_repo_with_index();
    let base = repo.head();

    // Advance HEAD past the base with a change to a file source discovery never collects, so the
    // discovered `.rs` tree is untouched and only the checkout's identity has moved.
    repo.write("README.md", "# advanced past the base\n");
    repo.commit_all("commit2: advance past the base, touching only a non-source file");
    assert_ne!(repo.head(), base, "HEAD has moved past the spec's revision");

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[&base]);
    let report = report(&answer);
    assert_eq!(report["seed_mode"], "range");
    assert_eq!(report["base_revision"], base);
    assert_eq!(seed_names(report), vec!["connect".to_string()]);
    assert_eq!(
        report["exactness"], "exact",
        "a bare spec names no range head, so nothing sits outside the diff it selects: {report}"
    );
    assert!(
        report["recovery"].is_null(),
        "an exact answer carries no recipe: {report}"
    );
}

// _(A range at the checkout with an uncommitted edit to a discovered source)_ — the range's head
// *is* what is checked out, so the head check passes; what disqualifies the answer is the
// uncommitted edit. The edit deliberately lands in the file the range's own diff already touches, so
// the wholesale base-content substitution discards it too and the reverted hash still matches the
// index — leaving the uncommitted-source check as the only thing that can catch it. The clean run
// first pins that the very same range grades exact without the edit, so the grade change traces to
// the edit alone.
#[test]
fn a_range_at_the_checkout_with_an_uncommitted_source_edit_is_approximate() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("commit1: fixture sources");
    let sha1 = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("commit2: edit connect");
    let sha2 = repo.head();

    // Built at the range's pre-change side, so the reverted hash matches.
    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &base_sources()).unwrap();

    let range = format!("{sha1}..{sha2}");
    let clean = run_impact_json(&repo, &db, &[&range]);
    assert_eq!(
        report(&clean)["exactness"],
        "exact",
        "with the checkout clean at the range's head, the range reconstructs: {clean}"
    );

    // An uncommitted edit to a discovered source — in the very file the range's diff touches, so the
    // reverted content, and therefore the hash, is unchanged.
    let both_edited = edit_line(&edited, 5, "        pub fn disconnect(&self, extra: bool) {}");
    repo.write(support::DOC, &both_edited);

    let dirty = run_impact_json(&repo, &db, &[&range]);
    let dirty_report = report(&dirty);
    assert_eq!(seed_names(dirty_report), vec!["connect".to_string()]);
    assert_eq!(
        dirty_report["exactness"], "approximate",
        "an uncommitted edit to a discovered source is lost by the substitution, so the hash match \
         alone must not be trusted: {dirty_report}"
    );
    assert!(
        !dirty_report["recovery"].is_null(),
        "an approximate answer carries a recipe: {dirty_report}"
    );
}

// ---------------------------------------------------------------------------
// Continuation tokens, forward reach, and per-page disclosures
// ---------------------------------------------------------------------------

/// Extend `index` with the generated `src/callers.rs` module: `count` functions calling `connect`,
/// their persisted symbols, and their reference occurrences onto `connect`. Returns the source text
/// to write to disk.
fn add_callers_module(index: &mut ExtractedIndex, count: usize) -> String {
    let (source, symbols, references) = build_callers(count);
    index.documents.push(SourceDocument {
        path: "src/callers.rs".to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("callers", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/callers.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 11),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.extend(symbols);
    for sym in index.symbols.iter_mut() {
        if sym.terminal_name() == Some("connect") {
            sym.occurrences.extend(references);
            break;
        }
    }
    source
}

// _(Two ranges sharing a base do not share a continuation token)_ — `A..B` and `A..C` present the
// same flags and resolve to the same base revision, so nothing but the patch itself distinguishes
// them. A token bound only to the flags plus the resolved revision would resume `A..B`'s page
// against `A..C`'s results, silently serving rows from a change the caller never asked about.
#[test]
fn a_cursor_is_refused_across_two_ranges_sharing_a_base() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());

    let mut index = extended_fixture_index();
    let callers_source = add_callers_module(&mut index, 30);
    repo.write("src/callers.rs", &callers_source);
    repo.commit_all("commit1: fixture sources plus many callers");
    let sha_a = repo.head();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.commit_all("commit2: edit connect");
    let sha_b = repo.head();

    let edited_again = edit_line(&edited, 5, "        pub fn disconnect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited_again);
    repo.commit_all("commit3: edit disconnect");
    let sha_c = repo.head();

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/callers.rs".to_string(), callers_source));
    build_from_index(&db, &ws, repo.path(), &index, &sources).unwrap();

    let range_ab = format!("{sha_a}..{sha_b}");
    let range_ac = format!("{sha_a}..{sha_c}");

    let first = run_impact_json(&repo, &db, &[&range_ab]);
    assert_eq!(report(&first)["base_revision"], sha_a);
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    // Guard the premise: the other range really does resolve to the same base revision, so the two
    // are told apart by nothing but the patch digest.
    let other = run_impact_json(&repo, &db, &[&range_ac]);
    assert_eq!(
        report(&other)["base_revision"],
        sha_a,
        "the two ranges share a base revision: {other}"
    );

    let out = run_impact(&repo, &db, &[&range_ac, "--cursor", &cursor]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a token issued for A..B is refused against A..C, not resumed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// _(The impact set carries no forward reach)_ — the requirement makes the impact set exactly the
// reverse-reachability closure of the seeds. The seed here deliberately *uses* two other symbols
// (`Client` and `connect`) while being used by one (`use_open`): a regression that also unioned the
// forward, callee-direction reach would report the two callees alongside the one real dependent.
#[test]
fn the_impact_set_carries_no_callee_direction_reach() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    let user_source = "mod user {\n    pub fn use_open() { open(); }\n}\n";
    repo.write("src/user.rs", user_source);
    repo.commit_all("add fixture sources plus a caller of open");

    let mut index = extended_fixture_index();
    index.documents.push(SourceDocument {
        path: "src/user.rs".to_string(),
        encoding: PositionEncoding::Utf8,
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![DescriptorSegment::new("user", SegmentKind::Module)],
        )),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/user.rs".to_string(),
            range: SourceRange::new(0, 4, 0, 8),
            role: OccurrenceRole::Definition,
        }],
    });
    index.symbols.push(ExtractedSymbol {
        descriptor: Some(Descriptor::new(
            "mycrate",
            vec![
                DescriptorSegment::new("user", SegmentKind::Module),
                DescriptorSegment::new("use_open", SegmentKind::Method),
            ],
        )),
        kind: SymbolKind::Function,
        class: SymbolClass::InWorkspace,
        // `    pub fn use_open() { open(); }` — the name occupies columns 11..19 of line 1.
        occurrences: vec![ExtractedOccurrence {
            document_path: "src/user.rs".to_string(),
            range: SourceRange::new(1, 11, 1, 19),
            role: OccurrenceRole::Definition,
        }],
    });
    // The `open()` call inside `use_open`, at columns 24..28 of the same line: a reference onto
    // `open`, which is what makes `use_open` depend on it.
    for sym in index.symbols.iter_mut() {
        if sym.terminal_name() == Some("open") {
            sym.occurrences.push(ExtractedOccurrence {
                document_path: "src/user.rs".to_string(),
                range: SourceRange::new(1, 24, 1, 28),
                role: OccurrenceRole::Reference,
            });
            break;
        }
    }

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/user.rs".to_string(), user_source.to_string()));
    build_from_index(&db, &ws, repo.path(), &index, &sources).unwrap();

    // Guard the premise: the seed really does have outgoing dependency edges, so a forward-reach
    // regression would have something to find. Seeding `Client` and `connect` each reports `open` as
    // a dependent, which is exactly the `open → Client` and `open → connect` uses edges — the edges a
    // forward walk from `open` would traverse in the other direction.
    for (line, replacement) in [
        (2usize, "    pub struct Client; // touched"),
        (4, "        pub fn connect(&self, extra: bool) {}"),
    ] {
        repo.write(support::DOC, &edit_line(support::SOURCE, line, replacement));
        let premise = run_impact_json(&repo, &db, &[]);
        assert!(
            dependent_names(report(&premise)).contains(&"open".to_string()),
            "the seed has an outgoing edge onto this symbol: {premise}"
        );
    }

    // `open`'s own declaration line: `open` uses `Client` (its return type) and `connect` (inside
    // its closure), and is used by `use_open`.
    let edited = edit_line(support::SOURCE, 7, "    pub fn open() -> Client { // touched");
    repo.write(support::DOC, &edited);

    let answer = run_impact_json(&repo, &db, &[]);
    let report = report(&answer);
    assert_eq!(seed_names(report), vec!["open".to_string()]);
    let dependents = dependent_names(report);
    assert_eq!(
        dependents,
        vec!["use_open".to_string()],
        "the impact set is the reverse-reachability closure and nothing else: {report}"
    );
    for callee in ["Client", "connect"] {
        assert!(
            !dependents.contains(&callee.to_string()),
            "{callee} is what the seed uses, not what uses the seed: {report}"
        );
    }
}

// _(Every page repeats the answer's disclosures)_ — bounding splits the dependent rows across pages,
// but the exactness label, the recovery recipe, and the disclosed unmappable regions are properties
// of the whole answer. A caller who only ever reads the second page must still see that the answer
// is approximate, how to make it exact, and which changed regions this index could not resolve.
#[test]
fn every_page_of_a_capped_answer_repeats_its_disclosures() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    // Tracked, but never referenced by the index at all, so editing it discloses an unmappable region.
    repo.write("src/untouched.rs", "pub fn untouched() {}\n");

    let mut index = extended_fixture_index();
    let callers_source = add_callers_module(&mut index, 30);
    repo.write("src/callers.rs", &callers_source);
    repo.commit_all("add fixture sources, many callers, and an unindexed tracked file");

    // Untracked at build time, to drive the approximate grade and the untracked-copy step.
    let scratch_content = "pub fn scratch() {}\n";
    repo.write("src/scratch.rs", scratch_content);

    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);
    let mut sources = base_sources();
    sources.push(("src/callers.rs".to_string(), callers_source));
    sources.push(("src/untouched.rs".to_string(), "pub fn untouched() {}\n".to_string()));
    sources.push(("src/scratch.rs".to_string(), scratch_content.to_string()));
    build_from_index(&db, &ws, repo.path(), &index, &sources).unwrap();

    let edited = edit_line(support::SOURCE, 4, "        pub fn connect(&self, extra: bool) {}");
    repo.write(support::DOC, &edited);
    repo.write("src/untouched.rs", "pub fn untouched_edited() {}\n");
    repo.write("src/scratch.rs", "pub fn scratch_edited() {}\n");
    let head = repo.head();

    let first = run_impact_json(&repo, &db, &[]);
    assert_eq!(first["page"]["truncated"], true, "{first}");
    assert_eq!(dependent_names(report(&first)).len(), 25, "{first}");
    let cursor = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    let second = run_impact_json(&repo, &db, &["--cursor", &cursor]);
    assert_eq!(
        dependent_names(report(&second)).len(),
        6,
        "the remaining six resume: {second}"
    );

    for (label, answer) in [("first", &first), ("second", &second)] {
        let report = report(answer);
        assert_eq!(
            report["exactness"], "approximate",
            "the {label} page carries the freshness label: {report}"
        );
        let recovery = &report["recovery"];
        assert!(
            !recovery.is_null(),
            "the {label} page carries the recovery recipe: {report}"
        );
        assert_eq!(recovery["base_revision"], head, "the {label} page: {recovery}");
        assert_eq!(recovery["workspace"], ws, "the {label} page: {recovery}");
        assert!(
            recovery["steps"]
                .as_array()
                .expect("recovery steps")
                .iter()
                .any(|s| s.as_str().unwrap().starts_with("c10r build")),
            "the {label} page's recipe is complete, not a stub: {recovery}"
        );
        let unmappable = report["unmappable"]
            .as_array()
            .unwrap_or_else(|| panic!("the {label} page discloses unmappable regions: {report}"));
        assert!(
            unmappable.iter().any(|u| u["document_path"] == "src/untouched.rs"),
            "the {label} page discloses the region in the unindexed tracked file: {report}"
        );
    }
}

// _(Stores record and disclose their workspace: an impact answer discloses too)_ — `impact` assembles
// its own answer, so the workspace disclosure has to reach it as well: an assessment read from a
// store recorded for another workspace carries the mismatch marker, and one read from the workspace
// its store describes carries none.
#[test]
fn an_impact_answer_discloses_a_workspace_mismatch() {
    let repo = TestRepo::new();
    repo.write(support::DOC, support::SOURCE);
    repo.write("other/extra.rs", extra_source());
    repo.commit_all("add fixture sources");
    let db = repo.path().join(".c10r/index.db");
    let ws = workspace_id(&repo);

    // The index records a workspace that is not the one the assessment is run from.
    let elsewhere = tempfile::tempdir().unwrap();
    build_from_index(&db, &ws, elsewhere.path(), &extended_fixture_index(), &base_sources()).unwrap();

    repo.write(
        support::DOC,
        &edit_line(support::SOURCE, 4, "        pub fn connect(&self, x: u8) {}"),
    );
    let out = run_impact(&repo, &db, &[]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("impact emits JSON");
    assert_eq!(
        report["workspace_relation"]["state"], "mismatched",
        "the impact answer discloses the mismatch: {report}"
    );

    // Rebuilt for this workspace, the same assessment carries no marker.
    build_from_index(&db, &ws, repo.path(), &extended_fixture_index(), &base_sources()).unwrap();
    let out = run_impact(&repo, &db, &[]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("impact emits JSON");
    assert!(
        report.get("workspace_relation").is_none(),
        "a matched workspace carries no marker: {report}"
    );
}

// _(Ordering disclosure: impact answers carry it too)_ — the impact answer envelope discloses the
// ordering in effect through the same single structural field a `dependents` trace carries:
// `ranked` by default, `unranked` on request — a separate wiring site from `trace`, checked
// separately.
#[test]
fn impact_answers_disclose_the_ordering_in_effect() {
    let (repo, db, _ws) = setup_repo_with_index();
    repo.write(
        support::DOC,
        &edit_line(support::SOURCE, 4, "        pub fn connect(&self, x: u8) {}"),
    );

    let ranked = run_impact_json(&repo, &db, &[]);
    assert_eq!(ranked["ordering"], "ranked", "the default is disclosed: {ranked}");

    let unranked = run_impact_json(&repo, &db, &["--order", "unranked"]);
    assert_eq!(unranked["ordering"], "unranked", "the request is disclosed: {unranked}");
}
