//! Process-level tests for `hooks install`, `hooks uninstall`, and the refresh hooks they write,
//! through real git repositories with the built binary first on `PATH`. Covers: writing the four
//! refresh-event hooks, keeping a hook already present rather than refusing, disagreement and
//! collision refusals, the `--overwrite` flag, removal, coexistence with a supported hook manager
//! (a captured prek shim, or a stand-in stub that mimics prek's observed install/uninstall/migration
//! behavior), and the refresh-timing rules across commits, amends, rebases (both backends), checkouts,
//! merges, and a failed refresh.
//!
//! A refresh is counted by the one-line build report `hooks run` prints when it decides to build
//! (`built: ...` or `already current: ...`, from `build_outcome_line` in `src/commands.rs`); a skipped
//! event prints nothing. Git redirects a hook's standard output to its own standard error, so every
//! helper here scans both streams of the triggering git invocation. Whether the index matches the
//! checked-out commit is read from `c10r status`'s `freshness` field, which falls back to comparing
//! recorded provenance when no live analyzer is on `PATH` and so needs no analyzer to answer truthfully
//! about content staleness.
//!
//! A hermetic `rust-analyzer` stub ([`Toolchain`], reusing the recipe `tests/code_navigation.rs` calls
//! `write_indexing_stub`) makes real hook-triggered builds fast and dependency-free: `--version`
//! answers with a fixed string, and `scip <root> --output <path>` writes an empty (but validly
//! parsing) SCIP index rather than analyzing anything. These tests assert whether and how often a
//! build ran, never what it found, so an empty index is sufficient. Every `TestRepo` is a minimal real
//! Cargo package (a `Cargo.toml` and `src/lib.rs`), so `detect_language` and source collection work
//! against real files on disk; `write_source_change` edits `src/lib.rs` to give each commit a distinct
//! content hash.
//!
//! Every git invocation *this file* makes to set up a throwaway repository is isolated from the
//! developer's own git config (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/`GIT_CONFIG_NOSYSTEM`, plus an
//! explicit `-c user.*`/`commit.gpgsign=false` on every commit), exactly as `tests/impact.rs` does. The
//! `c10r` invocations under test drive the built binary directly
//! (`Command::new(env!("CARGO_BIN_EXE_c10r"))`), never the ambient `c10r` on `PATH` — and once hooks are
//! installed, every git invocation that could fire one also runs with the built binary's directory
//! first on `PATH`, so an installed hook's bare `c10r` never resolves to a stray system installation.

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use support::{prepend_path, write_indexing_stub};

/// Apply the developer's own git config isolation (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`/
/// `GIT_CONFIG_NOSYSTEM`) to `cmd`.
fn isolate_git_config(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

/// A throwaway git repository for one test: a root commit carrying a minimal, real Cargo package (a
/// `Cargo.toml` and `src/lib.rs`), so a hook-triggered build has real sources to analyze and a real
/// manifest for `detect_language` to find.
struct TestRepo {
    dir: tempfile::TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = TestRepo { dir };
        repo.git(&["init", "-q"]);
        std::fs::write(
            repo.path().join("Cargo.toml"),
            "[package]\nname = \"hooks-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        repo.write_source_change(0);
        repo.git(&["add", "-A"]);
        repo.git(&[
            "-c",
            "user.email=hooks-test@example.com",
            "-c",
            "user.name=Hooks Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "root",
        ]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// This repository's root, canonicalized — matching the absolute form
    /// [`silent_cartographer::git::GitRepo::hooks_dir`] reports, so a string comparison against the
    /// binary's own output does not trip on a symlinked temp root (`/tmp` -> `/private/tmp` on macOS).
    fn canonical_path(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    /// Overwrite `src/lib.rs` so its content differs by `marker`, giving each call a distinct content
    /// hash — the input a build's freshness comparison, and `hooks run`'s "already current" skip, both
    /// turn on.
    fn write_source_change(&self, marker: u32) {
        std::fs::write(
            self.path().join("src").join("lib.rs"),
            format!("pub fn marker() -> u32 {{\n    {marker}\n}}\n"),
        )
        .unwrap();
    }

    /// The branch `HEAD` currently names.
    fn current_branch(&self) -> String {
        self.git_output_in(self.path(), &["symbolic-ref", "--short", "HEAD"])
    }

    /// Run `git <args>` in this repository, isolated from the developer's own config.
    fn git(&self, args: &[&str]) {
        self.git_in(self.path(), args);
    }

    /// Run `git <args>` in `dir`, isolated from the developer's own config — used to drive git from a
    /// linked worktree's own directory rather than this repository's primary one.
    fn git_in(&self, dir: &Path, args: &[&str]) {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir);
        isolate_git_config(&mut cmd);
        let status = cmd.status().expect("git runs");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    /// The output of `git <args>` run in `dir`, isolated from the developer's own config.
    fn git_output_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir);
        isolate_git_config(&mut cmd);
        let output = cmd.output().expect("git runs");
        assert!(output.status.success(), "git {args:?} failed in {dir:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Run `git <args>` in this repository with `path` as `PATH`, capturing status/stdout/stderr
    /// rather than asserting success — used to drive a git operation that fires the installed hooks,
    /// where the caller inspects the hooks' build reports, a deliberate non-zero exit, or a conflict.
    fn git_output_with_path(&self, args: &[&str], path: &std::ffi::OsStr) -> Output {
        self.git_output_with_path_in(self.path(), args, path)
    }

    fn git_output_with_path_in(&self, dir: &Path, args: &[&str], path: &std::ffi::OsStr) -> Output {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir);
        isolate_git_config(&mut cmd)
            // Committer identity by environment rather than `-c user.*`: several callers pass
            // operations (an amend, a merge, `rebase --continue`) that may create a commit, and the
            // isolated config above leaves no identity configured otherwise.
            .env("GIT_AUTHOR_NAME", "Hooks Test")
            .env("GIT_AUTHOR_EMAIL", "hooks-test@example.com")
            .env("GIT_COMMITTER_NAME", "Hooks Test")
            .env("GIT_COMMITTER_EMAIL", "hooks-test@example.com")
            .env("PATH", path)
            .output()
            .expect("git runs")
    }

    /// Commit staged changes with `path` as `PATH`, so an installed post-commit hook's bare `c10r`
    /// resolves to the built binary.
    fn commit_with_path(&self, path: &std::ffi::OsStr, message: &str) -> Output {
        self.git_output_with_path(
            &[
                "-c",
                "user.email=hooks-test@example.com",
                "-c",
                "user.name=Hooks Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
            path,
        )
    }

    /// A fresh invocation of the built binary, run in this repository's own directory, isolated from
    /// the developer's own git config exactly as the `git` helpers above are — `run_hooks_install` and
    /// `run_hooks_uninstall` themselves invoke git (`rev-parse --git-path`), and a host's own
    /// `core.hooksPath` would otherwise point them somewhere other than the repository this test set
    /// up.
    fn c10r(&self) -> Command {
        self.c10r_in(self.path())
    }

    /// A fresh invocation of the built binary, run in `dir` — used to drive `hooks install` from a
    /// linked worktree.
    fn c10r_in(&self, dir: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
        cmd.current_dir(dir);
        isolate_git_config(&mut cmd);
        cmd
    }

    /// A fresh invocation of the built binary with `toolchain`'s stub `rust-analyzer` (and this
    /// binary's own directory) first on `PATH` — used for a direct `c10r build`/`c10r status` call that
    /// needs a working analyzer, outside of a hook.
    fn c10r_with_toolchain(&self, toolchain: &Toolchain) -> Command {
        let mut cmd = self.c10r();
        cmd.env("PATH", toolchain.path_for_hooks());
        cmd
    }

    /// The absolute path git resolves `hooks/<event>` to when invoked from `dir`, anchored the way
    /// [`silent_cartographer::git::GitRepo::hooks_dir`] anchors it: joined against the canonicalized
    /// invocation directory, so a relative answer lands where git meant it and an absolute one (a
    /// relocated `core.hooksPath`) replaces the base outright.
    fn resolved_event_path(&self, dir: &Path, event: &str) -> PathBuf {
        let raw = self.git_output_in(dir, &["rev-parse", "--git-path", &format!("hooks/{event}")]);
        dir.canonicalize().unwrap().join(raw)
    }

    /// Whether `c10r status` (with the default `--db`/`--workspace`, run in this repository) reports
    /// the index fresh. Run with `toolchain`'s stub `rust-analyzer` on `PATH` — the same stub a build
    /// used — so an ambient, real `rust-analyzer` elsewhere on the developer's `PATH` cannot be probed
    /// instead and read as a version drift that never actually happened.
    fn is_fresh_with(&self, toolchain: &Toolchain) -> bool {
        let out = self
            .c10r_with_toolchain(toolchain)
            .args(["--json", "status"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "status failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("status answer parses");
        value["freshness"] == "fresh"
    }
}

/// A directory holding a stub `c10r` that records the argv it was invoked with, for placing early on
/// `PATH` so an executed hook's `c10r hooks run` line is observable. A real build needs a language
/// indexer that is not available in this test environment; what the hook must carry is its argument
/// vector, and the stub is what makes that observable.
#[cfg(unix)]
struct StubBinary {
    dir: tempfile::TempDir,
}

#[cfg(unix)]
impl StubBinary {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("argv");
        let stub = dir.path().join("c10r");
        // A tempdir path carries no single quote, so embedding it in a single-quoted shell word is
        // sound here.
        std::fs::write(
            &stub,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", record.display()),
        )
        .unwrap();
        make_executable(&stub);
        StubBinary { dir }
    }

    /// `PATH` with this stub's directory first, so the stub shadows any ambient `c10r`.
    fn prepended_path(&self) -> std::ffi::OsString {
        prepend_path(&[self.dir.path()])
    }

    /// The argv the stub last recorded, one argument per element.
    fn recorded_argv(&self) -> Vec<String> {
        let recorded = std::fs::read_to_string(self.dir.path().join("argv")).expect("the stub recorded its argv");
        recorded.lines().map(str::to_string).collect()
    }
}

/// A directory holding a fast, hermetic `rust-analyzer` stub, for driving real hook-triggered builds
/// end to end without a real Rust indexer: `--version` answers with a fixed string, and `scip <root>
/// --output <path>` writes an empty SCIP index and exits 0 rather than analyzing anything.
struct Toolchain {
    dir: tempfile::TempDir,
}

impl Toolchain {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        write_indexing_stub(dir.path(), "rust-analyzer", "rust-analyzer 9.9.9");
        Toolchain { dir }
    }

    /// A `rust-analyzer` stub whose `scip` subcommand always fails (after a succeeding `--version`
    /// probe), for driving a refresh that fails end to end.
    fn failing() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rust-analyzer");
        std::fs::write(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'rust-analyzer 9.9.9'; exit 0; fi\nexit 1\n",
        )
        .unwrap();
        make_executable(&path);
        Toolchain { dir }
    }

    /// `PATH` with the built `c10r` binary's directory and this stub's directory first, in that order,
    /// ahead of the inherited value — so a hook's bare `c10r` resolves to the binary under test and its
    /// `rust-analyzer` resolves to the stub, never to an ambient installation of either.
    fn path_for_hooks(&self) -> std::ffi::OsString {
        prepend_path(&[&c10r_bin_dir(), self.dir.path()])
    }
}

/// The directory the built `c10r` binary lives in, for placing first on `PATH` so an installed hook's
/// bare `c10r` resolves to it.
fn c10r_bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_c10r"))
        .parent()
        .unwrap()
        .to_path_buf()
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The combined standard output and standard error of a git invocation, in that order — git redirects
/// a hook's own standard output to its standard error, so a build report line can land in either
/// stream depending on git's plumbing for the triggering command.
fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The number of refreshes a triggering git invocation's hooks caused: one per `hooks run` build
/// report line (`built: ...` or `already current: ...`, see `build_outcome_line` in
/// `src/commands.rs`) found in either stream. An event the refresh rules skipped prints no such line.
fn refresh_count(output: &Output) -> usize {
    combined_output(output)
        .lines()
        .filter(|line| line.starts_with("built:") || line.starts_with("already current:"))
        .count()
}

/// Insert one more `c10r hooks run <event> --root ... --db ... --workspace <workspace> -- "$@"` line
/// into the c10r file at `path`, ahead of its `exit $status` — used to hand-craft a disagreement
/// between events' installed workspace sets without going through `install` (which refuses to write a
/// disagreement in the first place).
fn insert_workspace_line(path: &Path, root: &Path, db: &Path, workspace: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let event = path.file_name().unwrap().to_str().unwrap();
    let line = format!(
        "c10r hooks run {event} --root='{}' --db='{}' --workspace='{workspace}' -- \"$@\"\n",
        root.display(),
        db.display(),
    );
    let updated = text.replacen("exit $status\n", &format!("{line}exit $status\n"), 1);
    assert_ne!(
        updated, text,
        "the file carries the exit line this helper inserts before"
    );
    std::fs::write(path, updated).unwrap();
}

/// One directory entry as installation could change it: a symbolic link is recorded by its target,
/// never by the file it points to, so a link replaced by a copy of its target reads as a change.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Link(PathBuf),
    File(Vec<u8>),
}

/// Every entry directly under `dir`, by name — the before/after view a "no file changed" assertion
/// compares.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, Entry> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let entry = if std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink() {
                Entry::Link(std::fs::read_link(&path).unwrap())
            } else {
                Entry::File(std::fs::read(&path).unwrap())
            };
            (name, entry)
        })
        .collect()
}

const PREK_FIXTURE_POST_COMMIT: &str = include_str!("fixtures/prek-shims/0.5.3/post-commit");
const PREK_FIXTURE_POST_REWRITE: &str = include_str!("fixtures/prek-shims/0.5.3/post-rewrite");
const PREK_FIXTURE_POST_CHECKOUT: &str = include_str!("fixtures/prek-shims/0.5.3/post-checkout");
const PREK_FIXTURE_POST_MERGE: &str = include_str!("fixtures/prek-shims/0.5.3/post-merge");

/// The captured prek 0.5.3 shim text for `event` (verbatim, ending with a trailing blank line as
/// captured).
fn prek_fixture(event: &str) -> &'static str {
    match event {
        "post-commit" => PREK_FIXTURE_POST_COMMIT,
        "post-rewrite" => PREK_FIXTURE_POST_REWRITE,
        "post-checkout" => PREK_FIXTURE_POST_CHECKOUT,
        "post-merge" => PREK_FIXTURE_POST_MERGE,
        other => panic!("no captured prek fixture for {other}"),
    }
}

/// `event`'s captured prek shim text with its `PREK="..."` line pointed at `stub` instead of the real
/// prek binary the fixture was captured against — every other byte, including the fixture's own
/// trailing blank line, is left exactly as captured, so [`silent_cartographer::hooks::
/// is_recognized_prek_shim`]'s whole-file match still succeeds.
fn prek_shim_text(event: &str, stub: &Path) -> String {
    let fixture = prek_fixture(event);
    let start = fixture.find("PREK=\"").expect("the fixture carries a PREK= line");
    let value_start = start + "PREK=\"".len();
    let end = value_start + fixture[value_start..].find('"').expect("the PREK= value is quoted");
    format!(
        "{}PREK=\"{}\"{}",
        &fixture[..start],
        stub.display(),
        &fixture[end + 1..]
    )
}

fn hook_event(event: &str) -> silent_cartographer::hooks::HookEvent {
    use clap::ValueEnum;
    use silent_cartographer::hooks::HookEvent;
    HookEvent::from_str(event, false).unwrap_or_else(|_| panic!("unknown event {event}"))
}

/// A stand-in for prek's own binary, invoked as `$PREK hook-impl --hook-dir "$HERE" --script-version 4
/// --hook-type=<event> -- "$@"` by a shim written from [`prek_shim_text`]. Mirrors prek's observed
/// behavior: appends `manager <event>` to `log`; when `PREK_RUNNING_LEGACY` is already set, prints
/// prek's own migration-mode error to standard error and exits 0 without running anything; otherwise
/// runs `<hook-dir>/<event>.legacy`, if executable, with `PREK_RUNNING_LEGACY=1` exported, passing the
/// trailing arguments and standard input through, and exits with its status.
fn write_prek_manager_stub(dir: &Path, log: &Path) -> PathBuf {
    let path = dir.join("prek-stub");
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" != \"hook-impl\" ]; then exit 1; fi\n\
         shift\n\
         hook_dir=\"\"\n\
         hook_type=\"\"\n\
         while [ $# -gt 0 ]; do\n\
         \x20 case \"$1\" in\n\
         \x20   --hook-dir) hook_dir=\"$2\"; shift 2 ;;\n\
         \x20   --script-version) shift 2 ;;\n\
         \x20   --hook-type=*) hook_type=\"${{1#--hook-type=}}\"; shift ;;\n\
         \x20   --) shift; break ;;\n\
         \x20   *) shift ;;\n\
         \x20 esac\n\
         done\n\
         printf 'manager %s\\n' \"$hook_type\" >> '{log}'\n\
         if [ -n \"$PREK_RUNNING_LEGACY\" ]; then\n\
         \x20 echo \"error: prek's Git shim is installed in migration mode\" >&2\n\
         \x20 exit 0\n\
         fi\n\
         legacy=\"$hook_dir/$hook_type.legacy\"\n\
         if [ -x \"$legacy\" ]; then\n\
         \x20 PREK_RUNNING_LEGACY=1 \"$legacy\" \"$@\"\n\
         \x20 exit $?\n\
         fi\n\
         exit 0\n",
        log = log.display()
    );
    std::fs::write(&path, script).unwrap();
    make_executable(&path);
    path
}

/// Mimics `prek install`'s observed behavior for one event: if the file at `E` is not already a
/// recognized shim, move it to `E.legacy` (replacing any existing `E.legacy`), then write the shim at
/// `E`. A no-op when the shim is already at `E` — `prek install` run again changes nothing.
fn prek_install(hooks_dir: &Path, event: &str, stub: &Path) {
    let e = hooks_dir.join(event);
    let legacy = hooks_dir.join(format!("{event}.legacy"));
    let already_shim = std::fs::read_to_string(&e)
        .ok()
        .is_some_and(|text| silent_cartographer::hooks::is_recognized_prek_shim(&text, hook_event(event)));
    if already_shim {
        return;
    }
    if e.exists() {
        if legacy.exists() {
            std::fs::remove_file(&legacy).unwrap();
        }
        std::fs::rename(&e, &legacy).unwrap();
    }
    std::fs::write(&e, prek_shim_text(event, stub)).unwrap();
    make_executable(&e);
}

/// Mimics `prek uninstall`'s observed behavior for one event: remove the shim at `E`, then move
/// `E.legacy`, if present, back to `E`.
fn prek_uninstall(hooks_dir: &Path, event: &str) {
    let e = hooks_dir.join(event);
    let legacy = hooks_dir.join(format!("{event}.legacy"));
    let _ = std::fs::remove_file(&e);
    if legacy.exists() {
        std::fs::rename(&legacy, &e).unwrap();
    }
}

// ---------------------------------------------------------------------------------------------
// Refresh-hook installation: hook content and executability (the four-event layout).
// ---------------------------------------------------------------------------------------------

// _(Hooks installed where none exist)_ — `hooks install` writes an executable hook for each refresh
// event that runs `c10r hooks run`, reports each path written (matching where git actually resolves
// it), and exits with the success code. Exercised under both the human rendering and `--json`.
#[test]
fn installs_an_executable_hook_that_refreshes_the_index() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let expected_path = repo.resolved_event_path(repo.path(), event);
        assert!(expected_path.exists(), "the hook was written to {expected_path:?}");

        let contents = std::fs::read_to_string(&expected_path).unwrap();
        assert!(
            contents.contains("c10r hooks run"),
            "the hook invokes the refresh rules: {contents}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&expected_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "{event}'s hook is executable");
        }
        assert!(
            stdout.contains(expected_path.display().to_string().as_str()),
            "the human report names the path written: {stdout}"
        );
    }

    // A second, freshly discovered repository confirms the same under `--json`.
    let repo = TestRepo::new();
    let expected_path = repo.resolved_event_path(repo.path(), "post-commit");

    let out = repo.c10r().args(["--json", "hooks", "install"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    let events = answer["events"].as_array().expect("events is an array");
    let post_commit = events
        .iter()
        .find(|e| e["event"] == "post-commit")
        .expect("post-commit is reported");
    assert_eq!(
        post_commit["written"],
        expected_path.display().to_string(),
        "the JSON answer reports the path written"
    );
}

// _(existing hooks keep running, baseline)_ — a hook already present at the event path is kept and
// chained: `hooks install` moves it to `<event>.c10r-prior`, and it keeps running on its event with the
// refresh.
#[test]
fn keeps_an_existing_hook_and_runs_it() {
    let repo = TestRepo::new();
    let hook_path = repo.resolved_event_path(repo.path(), "post-commit");
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let marker = repo.path().join("existing-hook-ran");
    let sentinel = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    std::fs::write(&hook_path, &sentinel).unwrap();
    make_executable(&hook_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let prior_path = hook_path.parent().unwrap().join("post-commit.c10r-prior");
    assert_eq!(
        std::fs::read_to_string(&prior_path).unwrap(),
        sentinel,
        "the existing hook is kept, byte-for-byte"
    );

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "source change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(marker.exists(), "the kept hook still ran on its event");
    assert_eq!(refresh_count(&output), 1, "the refresh also ran");
}

// _(Installing outside a git worktree is a typed environment failure)_ — a plain, non-repository
// directory exits with the environment/setup-failure code (5), the same code `impact` raises for the
// same condition.
#[test]
fn outside_a_git_worktree_is_the_environment_setup_failure_code() {
    let dir = tempfile::tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_c10r"))
        .current_dir(dir.path())
        .args(["hooks", "install"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(5),
        "a non-worktree directory is the environment/setup-failure code: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("worktree"),
        "the diagnostic states the directory is not a git worktree: {stderr}"
    );
}

// _(The hooks are installed where this worktree resolves its hooks)_ — a linked worktree keeps its
// hooks in the repository's common directory, not in a `.git/hooks` directory beside its own working
// files. Running `hooks install` from inside the linked worktree must land the hook at the path that
// worktree actually resolves `git rev-parse --git-path hooks/post-commit` to, not at an assumed
// `.git/hooks` location the linked worktree does not even have.
#[test]
fn linked_worktree_installs_the_hook_where_it_actually_resolves() {
    let repo = TestRepo::new();
    // A dedicated tempdir for the linked worktree's parent, so its fixed subdirectory name cannot
    // collide with another test's linked worktree sharing the same system temp root.
    let linked_parent = tempfile::tempdir().unwrap();
    let linked_dir = linked_parent.path().join("linked-worktree");
    repo.git(&["worktree", "add", "-q", linked_dir.to_str().unwrap(), "-b", "linked"]);

    // The premise the boundary method exists for: a linked worktree's `.git` is a *file* pointing at
    // the repository's common directory, not a directory. A hard-coded `.git/hooks/post-commit` join
    // could not even be created here, and the hooks git actually reads live in the common directory.
    assert!(
        linked_dir.join(".git").is_file(),
        "a linked worktree's .git is a file, so an assumed .git/hooks path could not be written"
    );
    assert!(
        !linked_dir.join(".git").join("hooks").exists(),
        "a linked worktree does not carry its own hooks directory"
    );

    let resolved = PathBuf::from(repo.git_output_in(&linked_dir, &["rev-parse", "--git-path", "hooks/post-commit"]));
    let expected_path = linked_dir.join(resolved);

    let out = repo.c10r_in(&linked_dir).args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed where the linked worktree resolves its hooks: {expected_path:?}"
    );
    // The expected path is not under the linked worktree's own (nonexistent) `.git/hooks` — it is the
    // primary repository's shared hooks directory.
    assert!(
        !expected_path.starts_with(linked_dir.join(".git").join("hooks")),
        "the hook is not written to a `.git/hooks` directory beside the linked worktree: {expected_path:?}"
    );
    // It landed in the primary repository's own directory — the one place git will read it from for
    // commits made in either worktree.
    assert!(
        expected_path.starts_with(repo.canonical_path()),
        "the hook landed under the primary repository, not beside the linked worktree: {expected_path:?}"
    );
}

// _(Hooks installed where none exist)_ — "refreshes the index" means *this* caller's index, under
// *this* caller's workspace identity, over *this* caller's workspace root. Git runs each event with the
// working directory at the worktree's top level, which is neither the caller's invocation directory
// nor — for a workspace below the repository root — the workspace at all, so all three have to be
// written into every hook file as resolved absolute values rather than left to defaults.
#[test]
fn the_hook_carries_the_callers_db_workspace_and_root() {
    // A non-default `--db` and an explicit `--workspace`, installed from the repository root.
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args([
            "--db",
            "custom/store.db",
            "--workspace",
            "chosen-identity",
            "hooks",
            "install",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected_root = repo.canonical_path();
    let expected_db = expected_root.join("custom/store.db");
    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let hook_path = repo.resolved_event_path(repo.path(), event);
        let contents = std::fs::read_to_string(&hook_path).unwrap();
        assert!(
            contents.contains(expected_root.display().to_string().as_str()),
            "{event} names the resolved absolute workspace root: {contents}"
        );
        assert!(
            contents.contains(expected_db.display().to_string().as_str()),
            "{event} names the caller's --db, resolved absolute: {contents}"
        );
        assert!(
            contents.contains("--workspace"),
            "{event} pins the workspace identity: {contents}"
        );
        assert!(
            contents.contains("chosen-identity"),
            "{event} names the caller's workspace identity: {contents}"
        );
    }

    // A workspace in a subdirectory of the repository, with the default `--db` and a derived
    // identity: every hook must index the subdirectory, not the worktree top level git will run it
    // from, and must namespace symbols under the subdirectory's own derived identity.
    let repo = TestRepo::new();
    let workspace = repo.path().join("nested-workspace");
    std::fs::create_dir(&workspace).unwrap();
    let out = repo.c10r_in(&workspace).args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected_root = workspace.canonicalize().unwrap();
    let expected_db = expected_root.join(".c10r/index.db");
    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let hook_path = repo.resolved_event_path(&workspace, event);
        let contents = std::fs::read_to_string(&hook_path).unwrap();
        assert!(
            contents.contains(expected_root.display().to_string().as_str()),
            "{event} names the workspace subdirectory, not the worktree top level: {contents}"
        );
        assert!(
            contents.contains(expected_db.display().to_string().as_str()),
            "{event} names the index under the workspace subdirectory: {contents}"
        );
        assert!(
            contents.contains("--workspace='nested-workspace'"),
            "{event} pins the identity derived from the workspace root's own name: {contents}"
        );
        assert!(
            !contents.contains(&format!("'{}'", repo.canonical_path().display())),
            "{event} must not index the worktree top level for a workspace below it: {contents}"
        );
    }
}

// _(Hooks installed where none exist)_ — each written file is a hook git can actually run, and running
// it invokes `c10r hooks run <event>` with the arguments the script carries. Asserting the file's text
// alone leaves the shebang and the script's executability-as-a-program untested, so each hook is
// executed here the way git runs it: as a program, with the working directory at the worktree's top
// level. A real build needs a language indexer this environment does not have, so a stub `c10r` early
// on `PATH` records the argv instead.
#[cfg(unix)]
#[test]
fn the_installed_hook_runs_c10r_build_with_the_resolved_arguments() {
    let repo = TestRepo::new();

    let out = repo
        .c10r()
        .args([
            "--db",
            "custom/store.db",
            "--workspace",
            "chosen-identity",
            "hooks",
            "install",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let expected_root = repo.canonical_path().display().to_string();
    let expected_db = repo.canonical_path().join("custom/store.db").display().to_string();

    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let hook_path = repo.resolved_event_path(repo.path(), event);
        let contents = std::fs::read_to_string(&hook_path).unwrap();
        assert!(
            contents.starts_with("#!/bin/sh\n"),
            "{event} carries an interpreter line, without which git cannot execute it: {contents}"
        );

        let stub = StubBinary::new();
        let status = Command::new(&hook_path)
            // Git invokes a refresh hook with the working directory at the worktree's top level.
            .current_dir(repo.canonical_path())
            .env("PATH", stub.prepended_path())
            .status()
            .expect("the installed hook is executable as a program");
        assert!(status.success(), "{event} ran to completion: {status:?}");

        let argv = stub.recorded_argv();
        assert_eq!(
            argv,
            vec![
                "hooks".to_string(),
                "run".to_string(),
                event.to_string(),
                format!("--root={expected_root}"),
                format!("--db={expected_db}"),
                "--workspace=chosen-identity".to_string(),
                "--".to_string(),
            ],
            "{event}'s hook invoked `c10r hooks run` with the caller's root, --db, and --workspace"
        );
    }
}

// _(The hooks are installed where this worktree resolves its hooks)_ — a repository whose
// `core.hooksPath` relocates hooks outside `.git` entirely. `git rev-parse --git-path hooks/post-commit`
// is the only authority on where that lands, so the expectation is taken from git itself rather than
// reconstructed.
#[test]
fn a_relocated_hooks_path_installs_the_hook_where_git_resolves_it() {
    let repo = TestRepo::new();
    let relocated = repo.canonical_path().join("ops").join("githooks");
    repo.git(&["config", "core.hooksPath", relocated.to_str().unwrap()]);

    let expected_path = repo.resolved_event_path(repo.path(), "post-commit");
    assert!(
        expected_path.starts_with(&relocated),
        "git resolves hooks to the relocated directory: {expected_path:?}"
    );
    assert!(
        !expected_path.starts_with(repo.canonical_path().join(".git")),
        "the relocated hook path is outside `.git`: {expected_path:?}"
    );

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed at the relocated hook path: {expected_path:?}"
    );
    assert!(
        !repo.canonical_path().join(".git/hooks/post-commit").exists(),
        "nothing was written to the assumed `.git/hooks` location git would never read"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(expected_path.display().to_string().as_str()),
        "the report names the relocated path written: {stdout}"
    );
}

// _(The hooks are installed where this worktree resolves its hooks)_ — installing from a subdirectory
// rather than the worktree top level. `git rev-parse --git-path` answers relative to the invocation
// directory, so a subdirectory invocation is where anchoring that answer against the wrong base would
// put the hook somewhere git never reads.
#[test]
fn installing_from_a_subdirectory_lands_where_git_resolves_hooks() {
    let repo = TestRepo::new();
    let subdir = repo.path().join("crates").join("inner");
    std::fs::create_dir_all(&subdir).unwrap();

    let expected_path = repo.resolved_event_path(&subdir, "post-commit");
    let out = repo
        .c10r_in(&subdir)
        .args(["--json", "hooks", "install"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        expected_path.exists(),
        "the hook landed where git resolves hooks from the subdirectory: {expected_path:?}"
    );
    // Independently of git's own relativity: this repository keeps its hooks in the primary
    // `.git/hooks`, and that is the single file the hook must be.
    assert!(
        repo.canonical_path().join(".git/hooks/post-commit").exists(),
        "the hook is the repository's own post-commit hook, not one nested under the subdirectory"
    );
    assert!(
        !subdir.join(".git").exists(),
        "no `.git` directory was fabricated beside the subdirectory"
    );

    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    let events = answer["events"].as_array().expect("events is an array");
    let post_commit = events
        .iter()
        .find(|e| e["event"] == "post-commit")
        .expect("post-commit is reported");
    assert_eq!(
        post_commit["written"],
        expected_path.display().to_string(),
        "the reported path is the absolute path git resolved, not a subdirectory-relative one"
    );
}

// _(Kept-hook collision)_ — a dangling symlink at the hook path is something already present, and
// following it would write outside the hooks directory: neither refusing nor overwriting, but
// clobbering a third location. It is refused like any other occupant.
#[cfg(unix)]
#[test]
fn a_dangling_symlink_at_the_hook_path_is_refused() {
    let repo = TestRepo::new();
    let hook_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(hook_path.parent().unwrap()).unwrap();
    let target = repo.canonical_path().join("absent-target");
    std::os::unix::fs::symlink(&target, &hook_path).unwrap();
    assert!(!target.exists(), "the symlink's target does not exist");

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(
        out.status.code(),
        Some(0),
        "a dangling symlink at the hook path is refused, not written through"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(hook_path.display().to_string().as_str()),
        "the refusal names the path it found: {stderr}"
    );
    assert!(!target.exists(), "nothing was written through the link to {target:?}");
    assert!(
        std::fs::symlink_metadata(&hook_path)
            .expect("the link itself is still there")
            .file_type()
            .is_symlink(),
        "the link itself is left intact"
    );
}

// ---------------------------------------------------------------------------------------------
// Refresh-hook installation: the installation-plan-level scenarios.
// ---------------------------------------------------------------------------------------------

// _(Hooks installed where none exist)_
#[test]
fn hooks_installed_where_none_exist() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let path = repo.resolved_event_path(repo.path(), event);
        assert!(path.exists(), "{event}'s hook is written");
    }
}

// _(A second workspace joins the installed hooks)_
#[test]
fn a_second_workspace_joins_the_installed_hooks() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "first", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = repo
        .c10r()
        .args(["--workspace", "second", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let path = repo.resolved_event_path(repo.path(), "post-commit");
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("--workspace='first'"), "{contents}");
    assert!(contents.contains("--workspace='second'"), "{contents}");

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 2, "a commit refreshes both workspaces");
}

// _(Reinstalling a workspace changes nothing)_
#[test]
fn reinstalling_a_workspace_changes_nothing() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let before: Vec<(PathBuf, Vec<u8>)> = ["post-commit", "post-rewrite", "post-checkout", "post-merge"]
        .iter()
        .map(|e| {
            let p = repo.resolved_event_path(repo.path(), e);
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for (path, bytes) in &before {
        assert_eq!(
            &std::fs::read(path).unwrap(),
            bytes,
            "{path:?} is byte-for-byte unchanged"
        );
    }
}

// _(A removed refresh hook is restored for every workspace)_
#[test]
fn a_removed_refresh_hook_is_restored_for_every_workspace() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = repo
        .c10r()
        .args(["--workspace", "beta", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());

    let merge_path = repo.resolved_event_path(repo.path(), "post-merge");
    std::fs::remove_file(&merge_path).unwrap();

    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(&merge_path).unwrap();
    assert!(contents.contains("--workspace='alpha'"));
    assert!(
        contents.contains("--workspace='beta'"),
        "the restored file names both workspaces: {contents}"
    );
}

// _(Disagreeing refresh hooks are refused)_
#[test]
fn disagreeing_refresh_hooks_are_refused() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());

    // Hand-edit one event's file to name a different workspace than the rest.
    let merge_path = repo.resolved_event_path(repo.path(), "post-merge");
    let contents = std::fs::read_to_string(&merge_path).unwrap();
    std::fs::write(&merge_path, contents.replace("'alpha'", "'other'")).unwrap();
    let merge_before = std::fs::read(&merge_path).unwrap();
    let commit_path = repo.resolved_event_path(repo.path(), "post-commit");
    let commit_before = std::fs::read(&commit_path).unwrap();

    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(merge_path.display().to_string().as_str()), "{stderr}");

    assert_eq!(std::fs::read(&merge_path).unwrap(), merge_before, "no file changed");
    assert_eq!(std::fs::read(&commit_path).unwrap(), commit_before);
}

// _(A workspace path with a line break is refused)_ — a worktree root containing a line break cannot
// be represented in the hook file's one-line-per-workspace format, so installation refuses before
// writing anything.
#[cfg(unix)]
#[test]
fn a_workspace_root_with_a_line_break_is_refused() {
    let parent = tempfile::tempdir().unwrap();
    let repo_dir = parent.path().join("repo\nwith-a-line-break");
    std::fs::create_dir(&repo_dir).unwrap();
    let mut git_init = Command::new("git");
    git_init.args(["init", "-q"]).current_dir(&repo_dir);
    let status = isolate_git_config(&mut git_init).status().unwrap();
    assert!(
        status.success(),
        "git init succeeds in a directory named with a line break"
    );

    let mut c10r = Command::new(env!("CARGO_BIN_EXE_c10r"));
    c10r.current_dir(&repo_dir);
    let out = isolate_git_config(&mut c10r)
        .args(["hooks", "install"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "installation refuses");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("line break"),
        "the refusal states the path contains a line break: {stderr}"
    );
    assert!(
        !repo_dir.join(".git/hooks/post-commit").exists(),
        "no hook file was written"
    );
}

// _(Installation leaves git configuration unchanged)_
#[test]
fn installation_leaves_git_configuration_unchanged() {
    let repo = TestRepo::new();
    let config_path = repo.canonical_path().join(".git").join("config");
    let before = std::fs::read(&config_path).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let after = std::fs::read(&config_path).unwrap();
    assert_eq!(before, after, "git configuration is byte-for-byte unchanged");
}

// ---------------------------------------------------------------------------------------------
// Refresh timing.
// ---------------------------------------------------------------------------------------------

// _(A commit refreshes the index)_
#[test]
fn a_commit_refreshes_the_index() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1);
    assert!(repo.is_fresh_with(&toolchain), "the index matches the new commit");
}

// A workspace identity starting with `-` still refreshes: the hook line carries it as
// `--workspace=-foo`, so the value stays attached to the flag rather than being read as another
// flag by the `hooks run` invocation the hook line makes.
#[test]
fn a_workspace_named_with_a_leading_dash_refreshes_on_a_commit() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let out = repo
        .c10r()
        .args(["--workspace=-foo", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "{}", combined_output(&output));

    let status = repo
        .c10r_with_toolchain(&toolchain)
        .args(["--workspace=-foo", "--json", "status"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).expect("status answer parses");
    assert_eq!(value["freshness"], "fresh", "the index matches the new commit: {value}");
}

// _(An amended commit refreshes the index once)_
#[test]
fn an_amended_commit_refreshes_once() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let setup = repo.commit_with_path(&path, "wip");
    assert!(setup.status.success(), "{}", combined_output(&setup));

    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    let output = repo.git_output_with_path(&["commit", "--amend", "-q", "--no-edit"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "exactly one refresh ran");
    assert!(repo.is_fresh_with(&toolchain), "the index matches the amended commit");
}

// _(A rebase that replays several commits refreshes once)_
#[test]
fn a_rebase_that_replays_several_commits_refreshes_once() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "feature 1"], &path);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "feature 2"], &path);
    repo.git(&["checkout", "-q", &base]);
    std::fs::write(repo.path().join("README.md"), "base advances\n").unwrap();
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "base advances"], &path);
    repo.git(&["checkout", "-q", "feature"]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let output = repo.git_output_with_path(&["rebase", "-q", &base], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "exactly one refresh ran");
    assert!(repo.is_fresh_with(&toolchain), "the index matches the rebased branch");
}

// _(A rebase with nothing to replay refreshes once)_
#[test]
fn a_rebase_with_nothing_to_replay_refreshes_once() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.git(&["checkout", "-q", &base]);
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "base advances"], &path);
    repo.git(&["checkout", "-q", "feature"]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let output = repo.git_output_with_path(&["rebase", "-q", &base], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "exactly one refresh ran");
    assert!(
        repo.is_fresh_with(&toolchain),
        "the index matches the new checked-out commit"
    );
}

// _(An apply-backend rebase refreshes at most twice)_ — once selected by `--apply`, once by
// `rebase.backend=apply`.
#[test]
fn an_apply_backend_rebase_refreshes_at_most_twice() {
    for use_flag in [true, false] {
        let repo = TestRepo::new();
        let toolchain = Toolchain::new();
        let path = toolchain.path_for_hooks();
        let base = repo.current_branch();

        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write_source_change(1);
        repo.git(&["add", "-A"]);
        repo.git_output_with_path(&["commit", "-q", "-m", "feature 1"], &path);
        repo.write_source_change(2);
        repo.git(&["add", "-A"]);
        repo.git_output_with_path(&["commit", "-q", "-m", "feature 2"], &path);
        repo.git(&["checkout", "-q", &base]);
        std::fs::write(repo.path().join("README.md"), "base advances\n").unwrap();
        repo.git(&["add", "-A"]);
        repo.git_output_with_path(&["commit", "-q", "-m", "base advances"], &path);
        repo.git(&["checkout", "-q", "feature"]);

        let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
        assert!(out.status.success());

        let output = if use_flag {
            repo.git_output_with_path(&["rebase", "-q", "--apply", &base], &path)
        } else {
            repo.git_output_with_path(&["-c", "rebase.backend=apply", "rebase", "-q", &base], &path)
        };
        assert!(
            output.status.success(),
            "use_flag={use_flag}: {}",
            combined_output(&output)
        );
        let count = refresh_count(&output);
        assert!(
            count <= 2,
            "use_flag={use_flag}: at most two refreshes ran, got {count}"
        );
        assert!(repo.is_fresh_with(&toolchain), "use_flag={use_flag}");
    }
}

// _(A paused rebase does not refresh)_
#[test]
fn a_paused_rebase_does_not_refresh() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    // Both branches edit the same line differently, so the rebase stops on a conflict.
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "feature change"], &path);
    repo.git(&["checkout", "-q", &base]);
    repo.write_source_change(3);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "base change"], &path);
    repo.git(&["checkout", "-q", "feature"]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let rebase_output = repo.git_output_with_path(&["rebase", "-q", &base], &path);
    assert!(!rebase_output.status.success(), "the rebase stops on a conflict");

    repo.write_source_change(4); // resolve by replacing the conflicted file wholesale
    repo.git(&["add", "-A"]);
    let commit_output = repo.git_output_with_path(&["commit", "-q", "-m", "resolved"], &path);
    assert!(commit_output.status.success(), "{}", combined_output(&commit_output));
    assert_eq!(
        refresh_count(&commit_output),
        0,
        "no refresh runs while the rebase is still in progress"
    );

    let continue_output = repo.git_output_with_path(&["rebase", "--continue"], &path);
    assert!(
        continue_output.status.success(),
        "{}",
        combined_output(&continue_output)
    );
    assert_eq!(refresh_count(&continue_output), 1, "the rebase's completion refreshes");
    assert!(repo.is_fresh_with(&toolchain));
}

// _(An aborted rebase leaves the index at the original commit)_
#[test]
fn an_aborted_rebase_leaves_the_index_at_the_original_commit() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "feature change"], &path);
    repo.git(&["checkout", "-q", &base]);
    repo.write_source_change(3);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "base change"], &path);
    repo.git(&["checkout", "-q", "feature"]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let commit_before_rebase = repo.git_output_in(repo.path(), &["rev-parse", "HEAD"]);
    let build = repo.c10r_with_toolchain(&toolchain).args(["build"]).output().unwrap();
    assert!(build.status.success(), "{}", combined_output(&build));
    assert!(repo.is_fresh_with(&toolchain));

    let rebase_output = repo.git_output_with_path(&["rebase", "-q", &base], &path);
    assert!(!rebase_output.status.success(), "the rebase stops on a conflict");
    assert_eq!(refresh_count(&rebase_output), 0);

    let abort_output = repo.git_output_with_path(&["rebase", "--abort"], &path);
    assert!(abort_output.status.success(), "{}", combined_output(&abort_output));
    assert_eq!(refresh_count(&abort_output), 0, "no refresh ran for the abort");

    let commit_after = repo.git_output_in(repo.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        commit_after, commit_before_rebase,
        "HEAD is back at the original commit"
    );
    assert!(
        repo.is_fresh_with(&toolchain),
        "the index still matches the checked-out commit"
    );
}

// _(A branch checkout refreshes the index)_
#[test]
fn a_branch_checkout_refreshes_the_index() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "other change"], &path);
    repo.git(&["checkout", "-q", &base]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let output = repo.git_output_with_path(&["switch", "-q", "other"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1);
    assert!(
        repo.is_fresh_with(&toolchain),
        "the index matches the newly checked-out branch"
    );
}

// _(A merge refreshes the index)_ — a fast-forward, and a merge commit.
#[test]
fn a_merge_refreshes_the_index() {
    // Fast-forward: merging a branch strictly ahead moves HEAD without a merge commit.
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "ahead"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "ahead change"], &path);
    repo.git(&["checkout", "-q", &base]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let output = repo.git_output_with_path(&["merge", "-q", "ahead"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "the fast-forward merge refreshes once");
    assert!(repo.is_fresh_with(&toolchain));

    // A merge commit: divergent, non-conflicting branches, merged with --no-ff.
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "other change"], &path);
    repo.git(&["checkout", "-q", &base]);
    std::fs::write(repo.path().join("README.md"), "base change\n").unwrap();
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "base readme"], &path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let output = repo.git_output_with_path(&["merge", "-q", "--no-ff", "-m", "merge other", "other"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "the merge commit refreshes once");
    assert!(repo.is_fresh_with(&toolchain));
}

// _(A squash merge does not refresh)_
#[test]
fn a_squash_merge_does_not_refresh() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "other change"], &path);
    repo.git(&["checkout", "-q", &base]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let head_before = repo.git_output_in(repo.path(), &["rev-parse", "HEAD"]);
    let output = repo.git_output_with_path(&["merge", "-q", "--squash", "other"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 0, "no refresh runs for the squash merge itself");
    let head_after = repo.git_output_in(repo.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "a squash merge leaves the checked-out commit unchanged"
    );

    let commit_output = repo.git_output_with_path(&["commit", "-q", "-m", "squashed"], &path);
    assert!(commit_output.status.success(), "{}", combined_output(&commit_output));
    assert_eq!(
        refresh_count(&commit_output),
        1,
        "the commit that follows the squash refreshes"
    );
}

// _(A file checkout does not refresh)_
#[test]
fn a_file_checkout_does_not_refresh() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();

    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "first change"], &path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    // Dirty the working tree without committing, then restore the file from HEAD — a file checkout
    // that does not change the checked-out commit.
    repo.write_source_change(999);
    let output = repo.git_output_with_path(&["checkout", "--", "src/lib.rs"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 0, "no refresh runs for a file checkout");
}

// _(A failed refresh does not fail a branch checkout)_
#[test]
fn a_failed_refresh_does_not_fail_a_branch_checkout() {
    let repo = TestRepo::new();
    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    let base = repo.current_branch();

    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write_source_change(2);
    repo.git(&["add", "-A"]);
    repo.git_output_with_path(&["commit", "-q", "-m", "other change"], &path);
    repo.git(&["checkout", "-q", &base]);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let failing = Toolchain::failing();
    let output = repo.git_output_with_path(&["switch", "-q", "other"], &failing.path_for_hooks());
    assert_eq!(
        output.status.code(),
        Some(0),
        "the checkout still exits with the success code: {}",
        combined_output(&output)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("post-checkout"),
        "the refresh failure names the event: {stderr}"
    );
    assert!(stderr.contains("failed"), "{stderr}");
}

// _(Hook builds and the stale refresh command use the backend the index records)_ — a workspace with
// both a `Cargo.toml` and a `pyproject.toml` is ambiguous to manifest detection. Building it once with
// an explicit `--language` records the backend, and both a later hook-triggered refresh and a stale
// answer's own refresh command reuse that recorded backend rather than needing `--language` again.
#[cfg(unix)]
#[test]
fn hook_refresh_and_the_stale_refresh_command_use_the_recorded_language() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("pyproject.toml"), "# fixture manifest\n").unwrap();

    let toolchain = Toolchain::new();
    let build = repo
        .c10r_with_toolchain(&toolchain)
        .args(["build", "--language", "rust"])
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let install = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(install.status.success(), "{}", String::from_utf8_lossy(&install.stderr));

    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(
        refresh_count(&output),
        1,
        "the hook build succeeds over the ambiguous manifests, using the recorded language: {}",
        combined_output(&output)
    );
    assert!(repo.is_fresh_with(&toolchain), "the index matches the new commit");

    // A stale answer's own refresh command also names the recorded backend, so running it rebuilds
    // an ambiguous workspace without `--language` typed by hand.
    repo.write_source_change(2);
    let query = repo
        .c10r_with_toolchain(&toolchain)
        .args(["--json", "find", "x"])
        .output()
        .unwrap();
    assert!(query.status.success(), "{}", String::from_utf8_lossy(&query.stderr));
    let value: serde_json::Value = serde_json::from_slice(&query.stdout).expect("the --json answer parses");
    assert_eq!(value["freshness"], "stale_content", "{value}");
    let refresh = value["refresh"].as_str().unwrap().to_string();
    assert!(refresh.contains("--language=rust"), "{refresh}");

    let status = Command::new("sh")
        .arg("-c")
        .arg(&refresh)
        .env("PATH", toolchain.path_for_hooks())
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(status.success(), "the refresh command runs to completion: {refresh}");
    assert!(repo.is_fresh_with(&toolchain), "the rebuilt index answers fresh");
}

// ---------------------------------------------------------------------------------------------
// Existing hooks keep running.
// ---------------------------------------------------------------------------------------------

// _(A kept hook receives the event's standard input)_ — an amend fires both `post-commit` and
// `post-rewrite`; the kept `post-rewrite` hook here captures git's rewritten-commit list.
#[test]
fn a_kept_hook_receives_the_events_standard_input() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-rewrite");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let captured = repo.path().join("captured-stdin");
    std::fs::write(&event_path, format!("#!/bin/sh\ncat > '{}'\n", captured.display())).unwrap();
    make_executable(&event_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let toolchain = Toolchain::new();
    let path = toolchain.path_for_hooks();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.git_output_with_path(&["commit", "--amend", "-q", "--no-edit"], &path);
    assert!(output.status.success(), "{}", combined_output(&output));

    let recorded = std::fs::read_to_string(&captured).unwrap();
    assert!(
        !recorded.trim().is_empty(),
        "the kept post-rewrite hook received git's rewritten-commit stdin: {recorded:?}"
    );
}

// _(A kept hook's failure still fails the checkout)_
#[test]
fn a_kept_hooks_failure_still_fails_the_checkout() {
    let repo = TestRepo::new();
    let base = repo.current_branch();
    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.git(&["checkout", "-q", &base]);

    let event_path = repo.resolved_event_path(repo.path(), "post-checkout");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    std::fs::write(&event_path, "#!/bin/sh\nexit 7\n").unwrap();
    make_executable(&event_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let toolchain = Toolchain::new();
    let output = repo.git_output_with_path(&["switch", "-q", "other"], &toolchain.path_for_hooks());
    // Confirmed empirically (git 2.54.0): a failing post-checkout hook fails `git switch`, but git
    // normalizes the process exit status to 1 rather than passing the hook's own status through —
    // the generated script's `exit $status` line carries the kept hook's exact code (7) to git's own
    // hook-invocation machinery, but it is that machinery's own nonzero/zero distinction, not the
    // literal number, that reaches the calling process.
    assert!(
        !output.status.success(),
        "the kept hook's failure still fails the checkout: {output:?}"
    );
    assert_eq!(
        refresh_count(&output),
        1,
        "the index still refreshed despite the kept hook's failure"
    );
}

// _(A non-executable kept file is not run)_
#[test]
fn a_non_executable_kept_file_is_not_run() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let marker = repo.path().join("non-executable-ran");
    let content = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    std::fs::write(&event_path, &content).unwrap();
    // Deliberately not made executable.

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let prior_path = event_path.parent().unwrap().join("post-commit.c10r-prior");
    assert_eq!(std::fs::read_to_string(&prior_path).unwrap(), content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&prior_path).unwrap().permissions().mode() & 0o111;
        assert_eq!(mode, 0, "the kept file is still not executable");
    }

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(!marker.exists(), "the non-executable kept file was not run");
    assert_eq!(refresh_count(&output), 1, "the index still refreshed");

    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&event_path).unwrap(),
        content,
        "removing the refresh hooks puts the file back unchanged"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&event_path).unwrap().permissions().mode() & 0o111;
        assert_eq!(mode, 0, "the restored file is still not executable");
    }
}

// _(An existing directory at the prior-hook path can fail a checkout)_ — a directory happens to sit at
// `<event>.c10r-prior`, something installation itself never creates. The generated script tests `-f`
// before `-x`, so it is not run, and a branch checkout using the installed hooks still succeeds.
#[cfg(unix)]
#[test]
fn a_directory_at_the_prior_path_does_not_fail_a_checkout() {
    let repo = TestRepo::new();
    repo.git(&["branch", "other"]);

    let event_path = repo.resolved_event_path(repo.path(), "post-checkout");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    std::fs::create_dir(event_path.parent().unwrap().join("post-checkout.c10r-prior")).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let toolchain = Toolchain::new();
    let output = repo.git_output_with_path(&["checkout", "-q", "other"], &toolchain.path_for_hooks());
    assert!(
        output.status.success(),
        "the checkout succeeds even though a directory sits at the prior-hook path: {}",
        combined_output(&output)
    );
    assert_eq!(
        refresh_count(&output),
        1,
        "the refresh still ran: {}",
        combined_output(&output)
    );
    assert_eq!(repo.current_branch(), "other");
}

// _(A kept symbolic link keeps its target)_
#[cfg(unix)]
#[test]
fn a_kept_symbolic_link_keeps_its_target() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let marker = repo.path().join("linked-hook-ran");
    let real_hook = hooks_dir.join("real-hook");
    std::fs::write(&real_hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    make_executable(&real_hook);
    std::os::unix::fs::symlink("real-hook", &event_path).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let prior_path = hooks_dir.join("post-commit.c10r-prior");
    assert!(std::fs::symlink_metadata(&prior_path).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_link(&prior_path).unwrap(), PathBuf::from("real-hook"));

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(marker.exists(), "the linked hook ran");
    assert_eq!(
        std::fs::read_link(&prior_path).unwrap(),
        PathBuf::from("real-hook"),
        "the kept link still points to the same target"
    );
}

// _(An unrecognized hook at the event path is chained)_
#[test]
fn an_unrecognized_hook_at_the_event_path_is_chained() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let marker = repo.path().join("foreign-ran");
    let foreign = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    std::fs::write(&event_path, &foreign).unwrap();
    make_executable(&event_path);
    let legacy_path = event_path.parent().unwrap().join("post-commit.legacy");
    std::fs::write(&legacy_path, "#!/bin/sh\necho separate\n").unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(marker.exists(), "the foreign hook at E ran");
    assert_eq!(refresh_count(&output), 1);
    assert_eq!(
        std::fs::read_to_string(&legacy_path).unwrap(),
        "#!/bin/sh\necho separate\n",
        "the legacy file is untouched"
    );
}

// _(A pre-existing hook survives the tools installed around it)_ — every order of installing prek and
// c10r's refresh hooks, prek reinstalled, and both then removed in either order. H's own log line
// asserts it ran; the final state asserts it is back byte-for-byte with no leftovers.
#[test]
fn a_pre_existing_hook_survives_the_tools_installed_around_it() {
    for install_prek_first in [true, false] {
        for remove_prek_first in [true, false] {
            let repo = TestRepo::new();
            let event = "post-commit";
            let event_path = repo.resolved_event_path(repo.path(), event);
            let hooks_dir = event_path.parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&hooks_dir).unwrap();
            let h_content = "#!/bin/sh\necho H-ran\nexit 0\n";
            std::fs::write(&event_path, h_content).unwrap();
            make_executable(&event_path);

            let stub_dir = tempfile::tempdir().unwrap();
            let log = repo.path().join("manager-log");
            let stub = write_prek_manager_stub(stub_dir.path(), &log);

            let toolchain = Toolchain::new();
            let path = toolchain.path_for_hooks();
            let mut marker = 1u32;
            let mut fire = |repo: &TestRepo| -> Output {
                repo.write_source_change(marker);
                marker += 1;
                repo.git(&["add", "-A"]);
                repo.commit_with_path(&path, "change")
            };
            let assert_h_ran = |output: &Output| {
                assert!(
                    combined_output(output).contains("H-ran"),
                    "H ran on the event (install_prek_first={install_prek_first}, \
                     remove_prek_first={remove_prek_first}): {}",
                    combined_output(output)
                );
            };

            // Install both, in the configured order.
            if install_prek_first {
                prek_install(&hooks_dir, event, &stub);
                let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            } else {
                let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
                prek_install(&hooks_dir, event, &stub);
            }
            let output = fire(&repo);
            assert!(output.status.success(), "{}", combined_output(&output));
            assert_h_ran(&output);

            // prek installed again changes nothing about the chain.
            prek_install(&hooks_dir, event, &stub);
            let output = fire(&repo);
            assert!(output.status.success(), "{}", combined_output(&output));
            assert_h_ran(&output);

            // Remove the first of the two: H still runs through whichever tool remains.
            if remove_prek_first {
                prek_uninstall(&hooks_dir, event);
            } else {
                let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            }
            let output = fire(&repo);
            assert!(output.status.success(), "{}", combined_output(&output));
            assert_h_ran(&output);

            // Remove the second.
            if remove_prek_first {
                let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            } else {
                prek_uninstall(&hooks_dir, event);
            }

            assert_eq!(
                std::fs::read_to_string(&event_path).unwrap(),
                h_content,
                "H is back at the event path, byte-for-byte (install_prek_first={install_prek_first}, \
                 remove_prek_first={remove_prek_first})"
            );
            assert!(!hooks_dir.join(format!("{event}.legacy")).exists());
            assert!(!hooks_dir.join(format!("{event}.c10r-prior")).exists());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Existing hooks with a recognized prek shim.
// ---------------------------------------------------------------------------------------------

// _(A supported hook manager installed first keeps running)_
#[test]
fn a_supported_hook_manager_installed_first_keeps_running() {
    let repo = TestRepo::new();
    let event = "post-commit";
    let event_path = repo.resolved_event_path(repo.path(), event);
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let stub_dir = tempfile::tempdir().unwrap();
    let log = repo.path().join("manager-log");
    let stub = write_prek_manager_stub(stub_dir.path(), &log);
    prek_install(&hooks_dir, event, &stub);
    let shim_before = std::fs::read_to_string(&event_path).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));

    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(log_text.lines().count(), 1, "prek's hook ran exactly once: {log_text}");
    assert_eq!(refresh_count(&output), 1, "the index was refreshed");
    assert!(
        !combined_output(&output).contains("migration mode"),
        "no output reports a failure that did not occur"
    );
    assert_eq!(
        std::fs::read_to_string(&event_path).unwrap(),
        shim_before,
        "prek's hook file is unchanged"
    );
}

// _(A supported hook manager installed later keeps both running)_
#[test]
fn a_supported_hook_manager_installed_later_keeps_both_running() {
    let repo = TestRepo::new();
    let event = "post-commit";
    let event_path = repo.resolved_event_path(repo.path(), event);
    let hooks_dir = event_path.parent().unwrap().to_path_buf();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let stub_dir = tempfile::tempdir().unwrap();
    let log = repo.path().join("manager-log");
    let stub = write_prek_manager_stub(stub_dir.path(), &log);
    prek_install(&hooks_dir, event, &stub);

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));

    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(log_text.lines().count(), 1, "prek's hook ran once: {log_text}");
    assert_eq!(refresh_count(&output), 1, "the index was refreshed");
}

// _(A supported hook manager reinstalled runs once)_
#[test]
fn a_supported_hook_manager_reinstalled_runs_once() {
    for install_prek_first in [true, false] {
        let repo = TestRepo::new();
        let event = "post-commit";
        let event_path = repo.resolved_event_path(repo.path(), event);
        let hooks_dir = event_path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&hooks_dir).unwrap();

        let stub_dir = tempfile::tempdir().unwrap();
        let log = repo.path().join("manager-log");
        let stub = write_prek_manager_stub(stub_dir.path(), &log);

        if install_prek_first {
            prek_install(&hooks_dir, event, &stub);
            let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
            assert!(out.status.success());
        } else {
            let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
            assert!(out.status.success());
            prek_install(&hooks_dir, event, &stub);
        }

        // prek is installed again.
        prek_install(&hooks_dir, event, &stub);

        let toolchain = Toolchain::new();
        repo.write_source_change(1);
        repo.git(&["add", "-A"]);
        let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
        assert!(output.status.success(), "{}", combined_output(&output));

        let log_text = std::fs::read_to_string(&log).unwrap_or_default();
        assert_eq!(
            log_text.lines().count(),
            1,
            "install_prek_first={install_prek_first}: prek's hook ran exactly once: {log_text}"
        );
        assert_eq!(
            refresh_count(&output),
            1,
            "install_prek_first={install_prek_first}: the index was refreshed"
        );
        assert!(
            !combined_output(&output).contains("migration mode"),
            "install_prek_first={install_prek_first}: no output reports a failure that did not occur"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Kept-hook collision.
// ---------------------------------------------------------------------------------------------

// _(An identical remaining hook is replaced silently)_
#[test]
fn an_identical_remaining_hook_is_replaced_silently() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let marker = repo.path().join("kept-ran");
    let content = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    std::fs::write(&event_path, &content).unwrap();
    make_executable(&event_path);
    let prior_path = event_path.parent().unwrap().join("post-commit.c10r-prior");
    std::fs::write(&prior_path, &content).unwrap();
    make_executable(&prior_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(marker.exists(), "the kept hook ran once");
    assert_eq!(refresh_count(&output), 1);
}

// _(A different remaining hook stops installation)_
#[test]
fn a_different_remaining_hook_stops_installation() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    std::fs::write(&event_path, "#!/bin/sh\necho now\n").unwrap();
    make_executable(&event_path);
    let prior_path = event_path.parent().unwrap().join("post-commit.c10r-prior");
    std::fs::write(&prior_path, "#!/bin/sh\necho before\n").unwrap();
    make_executable(&prior_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(prior_path.display().to_string().as_str()), "{stderr}");
    assert!(
        stderr.contains("remove it or rename it before installing"),
        "the refusal says the remaining hook must be removed or renamed: {stderr}"
    );
    assert_eq!(std::fs::read_to_string(&event_path).unwrap(), "#!/bin/sh\necho now\n");
    assert_eq!(
        std::fs::read_to_string(&prior_path).unwrap(),
        "#!/bin/sh\necho before\n"
    );
}

// _(Symbolic links with different targets are different)_
#[cfg(unix)]
#[test]
fn symbolic_links_with_different_targets_are_different() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let target_a = hooks_dir.join("hook-a");
    let target_b = hooks_dir.join("hook-b");
    std::fs::write(&target_a, "#!/bin/sh\necho same\n").unwrap();
    std::fs::write(&target_b, "#!/bin/sh\necho same\n").unwrap();
    make_executable(&target_a);
    make_executable(&target_b);
    std::os::unix::fs::symlink(&target_a, &event_path).unwrap();
    let prior_path = hooks_dir.join("post-commit.c10r-prior");
    std::os::unix::fs::symlink(&target_b, &prior_path).unwrap();

    let before = snapshot(&hooks_dir);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(prior_path.display().to_string().as_str()), "{stderr}");
    assert!(
        stderr.contains("remove it or rename it before installing"),
        "the refusal says the remaining hook must be removed or renamed: {stderr}"
    );
    assert_eq!(snapshot(&hooks_dir), before, "no file changed");
}

// A symbolic link at the slot and a regular file with the link target's bytes at the kept-hook path
// are different hooks: the link is never compared by dereferencing it.
#[cfg(unix)]
#[test]
fn a_symbolic_link_and_a_regular_file_with_the_same_bytes_are_different() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let target = hooks_dir.join("real-hook");
    std::fs::write(&target, "#!/bin/sh\necho same\n").unwrap();
    make_executable(&target);
    std::os::unix::fs::symlink("real-hook", &event_path).unwrap();
    let prior_path = hooks_dir.join("post-commit.c10r-prior");
    std::fs::write(&prior_path, "#!/bin/sh\necho same\n").unwrap();
    make_executable(&prior_path);
    let before = snapshot(&hooks_dir);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(prior_path.display().to_string().as_str()), "{stderr}");
    assert_eq!(snapshot(&hooks_dir), before, "no file changed");
}

// ---------------------------------------------------------------------------------------------
// Hook replacement on request.
// ---------------------------------------------------------------------------------------------

// _(Overwrite replaces an existing hook)_
#[test]
fn overwrite_replaces_an_existing_hook() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let marker = repo.path().join("replaced-would-run");
    std::fs::write(&event_path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    make_executable(&event_path);

    let out = repo.c10r().args(["hooks", "install", "--overwrite"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(!marker.exists(), "the replaced hook never ran");
    assert_eq!(refresh_count(&output), 1, "the index still refreshed");
}

// ---------------------------------------------------------------------------------------------
// Refresh-hook removal.
// ---------------------------------------------------------------------------------------------

// _(Removing one of two workspaces)_
#[test]
fn removing_one_of_two_workspaces() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "first", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = repo
        .c10r()
        .args(["--workspace", "second", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());

    let out = repo
        .c10r()
        .args(["--workspace", "first", "hooks", "uninstall"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(refresh_count(&output), 1, "only the remaining workspace refreshes");

    let path = repo.resolved_event_path(repo.path(), "post-commit");
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(!contents.contains("'first'"));
    assert!(contents.contains("'second'"));
}

// _(Removing the last workspace restores a kept hook)_
#[test]
fn removing_the_last_workspace_restores_a_kept_hook() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    std::fs::create_dir_all(event_path.parent().unwrap()).unwrap();
    let kept_content = "#!/bin/sh\necho kept\n";
    std::fs::write(&event_path, kept_content).unwrap();
    make_executable(&event_path);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(!event_path.parent().unwrap().join("post-commit.c10r-prior").exists());
    assert_eq!(
        std::fs::read_to_string(&event_path).unwrap(),
        kept_content,
        "the kept hook is restored byte-for-byte"
    );
}

// _(Removing the last workspace with no kept hook)_
#[test]
fn removing_the_last_workspace_with_no_kept_hook() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        assert!(
            !repo.resolved_event_path(repo.path(), event).exists(),
            "{event}'s hook file is gone"
        );
    }
}

// _(Removing while a supported hook manager is installed)_
#[test]
fn removing_while_a_supported_hook_manager_is_installed() {
    let repo = TestRepo::new();
    let event = "post-commit";
    let event_path = repo.resolved_event_path(repo.path(), event);
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let stub_dir = tempfile::tempdir().unwrap();
    let log = repo.path().join("manager-log");
    let stub = write_prek_manager_stub(stub_dir.path(), &log);
    prek_install(&hooks_dir, event, &stub);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());

    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert!(out.status.success());

    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));

    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(log_text.lines().count(), 1, "prek's hook still ran once: {log_text}");
    assert_eq!(
        refresh_count(&output),
        0,
        "no refresh runs once c10r's hooks are removed"
    );
    assert!(!combined_output(&output).contains("migration mode"));
}

// _(Removing from disagreeing refresh hooks)_
#[test]
fn removing_from_disagreeing_refresh_hooks() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());

    // Hand-craft a disagreement `install` would refuse to write: post-merge additionally names a
    // second workspace the other events don't.
    let merge_path = repo.resolved_event_path(repo.path(), "post-merge");
    insert_workspace_line(
        &merge_path,
        &repo.canonical_path(),
        &repo.canonical_path().join(".c10r/index.db"),
        "beta",
    );

    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "uninstall"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for event in ["post-commit", "post-rewrite", "post-checkout"] {
        assert!(
            !repo.resolved_event_path(repo.path(), event).exists(),
            "{event} had only alpha; removed entirely"
        );
    }
    let contents = std::fs::read_to_string(&merge_path).unwrap();
    assert!(
        !contents.contains("'alpha'"),
        "alpha no longer named anywhere: {contents}"
    );
    assert!(contents.contains("'beta'"), "{contents}");
}

// _(Removing a workspace that is not installed)_
#[test]
fn removing_a_workspace_that_is_not_installed() {
    let repo = TestRepo::new();
    let out = repo
        .c10r()
        .args(["--workspace", "alpha", "hooks", "install"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let before: Vec<Vec<u8>> = ["post-commit", "post-rewrite", "post-checkout", "post-merge"]
        .iter()
        .map(|e| std::fs::read(repo.resolved_event_path(repo.path(), e)).unwrap())
        .collect();

    let out = repo
        .c10r()
        .args(["--workspace", "not-installed", "hooks", "uninstall"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for (event, bytes) in ["post-commit", "post-rewrite", "post-checkout", "post-merge"]
        .iter()
        .zip(before)
    {
        assert_eq!(
            std::fs::read(repo.resolved_event_path(repo.path(), event)).unwrap(),
            bytes,
            "{event} unchanged"
        );
    }
}

// _(Refresh-hook removal)_ — removal is as silent on git configuration as installation.
#[test]
fn removal_leaves_git_configuration_unchanged() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let config_path = repo.canonical_path().join(".git").join("config");
    let before = std::fs::read(&config_path).unwrap();

    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert!(out.status.success());

    let after = std::fs::read(&config_path).unwrap();
    assert_eq!(before, after, "git configuration is byte-for-byte unchanged");
}

// ---------------------------------------------------------------------------------------------
// Overwrite deleting a kept hook, action-scoped arguments, and further refusal and restore paths.
// ---------------------------------------------------------------------------------------------

/// Write an executable kept hook at `<hooks_dir>/post-commit.c10r-prior` that creates `marker` when it
/// runs, and return its path.
#[cfg(unix)]
fn write_marker_prior(hooks_dir: &Path, marker: &Path) -> PathBuf {
    let prior_path = hooks_dir.join("post-commit.c10r-prior");
    std::fs::write(&prior_path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    make_executable(&prior_path);
    prior_path
}

/// Commit a source change with the hooks' toolchain on `PATH` and assert the kept hook at `marker` did
/// not run while the index still refreshed.
#[cfg(unix)]
fn assert_commit_refreshes_without_running(repo: &TestRepo, marker: &Path) {
    let toolchain = Toolchain::new();
    repo.write_source_change(1);
    repo.git(&["add", "-A"]);
    let output = repo.commit_with_path(&toolchain.path_for_hooks(), "change");
    assert!(output.status.success(), "{}", combined_output(&output));
    assert!(!marker.exists(), "the deleted kept hook never ran");
    assert_eq!(refresh_count(&output), 1, "the index still refreshed");
}

// _(Hook replacement on request)_ — with the refresh hook already installed, `--overwrite` deletes the
// hook an earlier install kept.
#[cfg(unix)]
#[test]
fn overwrite_deletes_a_kept_hook_beside_an_installed_refresh_hook() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let hooks_dir = repo
        .resolved_event_path(repo.path(), "post-commit")
        .parent()
        .unwrap()
        .to_path_buf();
    let marker = repo.path().join("kept-ran");
    let prior_path = write_marker_prior(&hooks_dir, &marker);

    let out = repo.c10r().args(["hooks", "install", "--overwrite"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!prior_path.exists(), "the kept hook is deleted");

    assert_commit_refreshes_without_running(&repo, &marker);
}

// _(Hook replacement on request)_ — with the refresh hook deleted by another tool, `--overwrite` still
// deletes the hook an earlier install kept.
#[cfg(unix)]
#[test]
fn overwrite_deletes_a_kept_hook_when_the_refresh_hook_is_missing() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    let marker = repo.path().join("kept-ran");
    let prior_path = write_marker_prior(&hooks_dir, &marker);
    std::fs::remove_file(&event_path).unwrap();

    let out = repo.c10r().args(["hooks", "install", "--overwrite"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!prior_path.exists(), "the kept hook is deleted");
    assert!(event_path.exists(), "the refresh hook is restored");

    assert_commit_refreshes_without_running(&repo, &marker);
}

// _(Refresh-hook event action)_ — an event that calls for no refresh prints nothing and succeeds.
#[test]
fn an_event_that_calls_for_no_refresh_prints_nothing() {
    let repo = TestRepo::new();
    let root = repo.canonical_path();
    let db = root.join(".c10r/index.db");

    let out = repo
        .c10r()
        .args(["hooks", "run", "post-checkout"])
        .arg(format!("--root={}", root.display()))
        .arg(format!("--db={}", db.display()))
        .args(["--workspace=ws", "--", "old", "new", "0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// _(An argument given to the wrong action is a usage error)_ — each misplaced argument exits with the
// usage-error code, names the action it belongs to, and changes no file.
#[test]
fn an_argument_given_to_the_wrong_hooks_action_is_a_usage_error() {
    let repo = TestRepo::new();
    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let hooks_dir = repo
        .resolved_event_path(repo.path(), "post-commit")
        .parent()
        .unwrap()
        .to_path_buf();
    let before = snapshot(&hooks_dir);

    let cases: [(&[&str], &str); 7] = [
        (&["hooks", "install", "post-commit"], "applies only to `hooks run`"),
        (&["hooks", "install", "--root=/tmp/x"], "applies only to `hooks run`"),
        (&["hooks", "install", "--", "x"], "apply only to `hooks run`"),
        (
            &["hooks", "uninstall", "--overwrite"],
            "applies only to `hooks install`",
        ),
        (
            &["hooks", "run", "post-commit", "--root=/tmp/x", "--overwrite"],
            "applies only to `hooks install`",
        ),
        (&["hooks", "run"], "`hooks run` requires an event"),
        (&["hooks", "run", "post-commit"], "`hooks run` requires --root"),
    ];
    for (args, expected) in cases {
        let out = repo.c10r().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?} is a usage error");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expected), "{args:?} names its action: {stderr}");
        assert_eq!(snapshot(&hooks_dir), before, "{args:?} changed no file");
    }
}

// _(Existing hooks keep running)_ — a plain install removes no file: every original hook's bytes
// remain, at their original path or at the kept-hook path.
#[test]
fn a_plain_install_over_existing_hooks_removes_no_file() {
    let repo = TestRepo::new();
    let hooks_dir = repo
        .resolved_event_path(repo.path(), "post-commit")
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    for event in ["post-commit", "post-rewrite", "post-checkout", "post-merge"] {
        let path = hooks_dir.join(event);
        std::fs::write(&path, format!("#!/bin/sh\necho foreign-{event}\n")).unwrap();
        make_executable(&path);
    }
    std::fs::write(hooks_dir.join("post-commit.legacy"), "#!/bin/sh\necho legacy\n").unwrap();
    let before = snapshot(&hooks_dir);

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let after = snapshot(&hooks_dir);
    for (name, original) in &before {
        let kept_name = format!("{name}.c10r-prior");
        assert!(
            after.get(name) == Some(original) || after.get(&kept_name) == Some(original),
            "{name}'s original bytes survive at its path or at {kept_name}"
        );
    }
}

// _(A workspace path with a line break is refused)_ — the same refusal applies to the index path and
// the workspace identity, each before any hook file changes.
#[test]
fn an_index_path_or_workspace_identity_with_a_line_break_is_refused() {
    let repo = TestRepo::new();
    let hooks_dir = repo
        .resolved_event_path(repo.path(), "post-commit")
        .parent()
        .unwrap()
        .to_path_buf();
    let before = snapshot(&hooks_dir);

    for (flag, what) in [("--db", "index path"), ("--workspace", "workspace identity")] {
        let out = repo
            .c10r()
            .args([flag, "one\ntwo", "hooks", "install"])
            .output()
            .unwrap();
        assert_ne!(out.status.code(), Some(0), "{flag} with a line break is refused");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(what) && stderr.contains("line break"),
            "the refusal states the {what} contains a line break: {stderr}"
        );
        assert_eq!(snapshot(&hooks_dir), before, "{flag}: no hook file changed");
    }
}

// _(A retry completes an installation that failed partway)_ — when the first hook file lacks the
// workspace being installed and a later one already names it, installing again completes every file.
#[test]
fn a_retry_completes_an_installation_whose_first_hook_lacks_the_workspace() {
    use silent_cartographer::hooks::{HookEvent, parse, render};

    let repo = TestRepo::new();
    for workspace in ["alpha", "beta"] {
        let out = repo
            .c10r()
            .args(["--workspace", workspace, "hooks", "install"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let commit_path = repo.resolved_event_path(repo.path(), "post-commit");
    let both = parse(&std::fs::read_to_string(&commit_path).unwrap()).unwrap();
    assert_eq!(both.len(), 2, "both workspaces are installed");
    std::fs::write(&commit_path, render(HookEvent::PostCommit, &both[..1])).unwrap();

    let out = repo
        .c10r()
        .args(["--workspace", "beta", "hooks", "install"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for event in HookEvent::ALL {
        let path = repo.resolved_event_path(repo.path(), event.as_str());
        assert_eq!(
            parse(&std::fs::read_to_string(&path).unwrap()),
            Some(both.clone()),
            "{} names both workspaces",
            event.as_str()
        );
    }
}

// _(A kept symbolic link keeps its target)_ — removing the last workspace puts a kept link back as a
// link to the same target.
#[cfg(unix)]
#[test]
fn removing_the_last_workspace_restores_a_kept_symbolic_link() {
    let repo = TestRepo::new();
    let event_path = repo.resolved_event_path(repo.path(), "post-commit");
    let hooks_dir = event_path.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let real_hook = hooks_dir.join("real-hook");
    std::fs::write(&real_hook, "#!/bin/sh\necho real\n").unwrap();
    make_executable(&real_hook);
    std::os::unix::fs::symlink("real-hook", &event_path).unwrap();

    let out = repo.c10r().args(["hooks", "install"]).output().unwrap();
    assert!(out.status.success());
    let out = repo.c10r().args(["hooks", "uninstall"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        std::fs::symlink_metadata(&event_path).unwrap().file_type().is_symlink(),
        "the restored hook is a link"
    );
    assert_eq!(std::fs::read_link(&event_path).unwrap(), PathBuf::from("real-hook"));
}

// _(Refresh-hook event action)_ — a root outside any git worktree is a failed refresh: the action
// still exits with the success code, prints nothing to standard output, and names the event and the
// workspace on standard error.
#[test]
fn a_hooks_run_root_outside_a_git_worktree_fails_softly_naming_event_and_workspace() {
    let not_a_worktree = tempfile::tempdir().unwrap();
    let db = not_a_worktree.path().join("index.db");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
    cmd.current_dir(not_a_worktree.path());

    let out = isolate_git_config(&mut cmd)
        .args(["hooks", "run", "post-commit"])
        .arg(format!("--root={}", not_a_worktree.path().display()))
        .arg(format!("--db={}", db.display()))
        .args(["--workspace=ws-identity", "--"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("post-commit") && stderr.contains("ws-identity"),
        "the failure names the event and the workspace: {stderr}"
    );
}
