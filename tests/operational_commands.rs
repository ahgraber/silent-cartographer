//! Operational-command contract tests: `doctor`'s indexer readiness report, `doctor`'s refresh-hook
//! status report, and `cache clear`'s index reset.
//!
//! These drive the built `c10r` binary through `std::process::Command` so the process-level contract
//! (exit codes, stdout/stderr separation) is observed as a caller would. `doctor` probes tools by
//! name on `PATH`, so its tests control `PATH` for the spawned process rather than depending on
//! whichever indexers happen to be installed on the host running the suite. The hook-status tests
//! isolate the git invocations they use to set up a throwaway repository from the developer's own git
//! config, the same way `tests/hooks.rs` does; git itself must still be reachable on `PATH` for
//! `doctor`'s own probing of the repository, so those tests prepend their indexer-stub directory to
//! the inherited `PATH` rather than replacing it outright.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use silent_cartographer::hooks::{HookEvent, WorkspaceLine, render};

/// The generous window a hanging probe must complete within: the production 10s deadline plus ample
/// slack, kept well under a minute so a genuine hang fails loudly rather than stalling the suite.
#[cfg(unix)]
const HANG_ASSERT_WINDOW: Duration = Duration::from_secs(60);

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`).
fn c10r() -> Command {
    Command::new(env!("CARGO_BIN_EXE_c10r"))
}

/// Write an executable shell-script stub named `name` into `dir`, responding to `--version` with
/// `version_line` on standard output and exiting successfully; any other invocation fails.
#[cfg(unix)]
fn write_stub(dir: &Path, name: &str, version_line: &str) {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(
        &path,
        format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"{version_line}\"\n  exit 0\nfi\nexit 1\n"),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
}

// _(Indexer readiness report: all indexers present)_ — with both required indexers on `PATH`, each is
// reported present with its version and the process exits with the success code.
#[cfg(unix)]
#[test]
fn doctor_reports_present_indexers_with_version_and_exits_success() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_stub(bin_dir.path(), "rust-analyzer", "rust-analyzer 1.99.0-stub");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let out = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "both indexers present is the success code; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "no diagnostic when every indexer is present");

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let statuses = report["indexers"]
        .as_array()
        .expect("the report's indexers field is a list of indexer statuses");
    assert_eq!(statuses.len(), 2, "both required indexers are reported");
    for status in statuses {
        assert_eq!(status["presence"], "present", "reported present: {status}");
        assert!(
            status["version"].as_str().is_some_and(|v| v.contains("stub")),
            "the stub's version is reported: {status}"
        );
        assert!(
            status["hint"].is_null(),
            "a present indexer carries no install hint: {status}"
        );
    }
}

// _(Indexer readiness report: a missing indexer is reported with a hint)_ — with neither required
// indexer on `PATH`, each is reported absent with an install hint and the process exits with the
// indexer/setup-failure code.
#[cfg(unix)]
#[test]
fn doctor_reports_absent_indexers_with_hint_and_exits_setup_failure() {
    let empty_bin_dir = tempfile::tempdir().unwrap();

    let out = c10r()
        .env("PATH", empty_bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(5),
        "a missing required indexer is the indexer/setup-failure code"
    );
    // The report still lands on standard output even though the process exits with a failure code.
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let statuses = report["indexers"]
        .as_array()
        .expect("the report's indexers field is a list of indexer statuses");
    assert_eq!(statuses.len(), 2, "both required indexers are reported");
    for status in statuses {
        assert_eq!(status["presence"], "absent", "reported absent: {status}");
        assert!(
            status["version"].is_null(),
            "an absent indexer carries no version: {status}"
        );
        let hint = status["hint"]
            .as_str()
            .expect("an absent indexer carries an install hint");
        assert!(!hint.is_empty(), "the install hint is actionable, not empty");
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.is_empty(),
        "the indexer/setup failure carries a diagnostic on standard error"
    );
    for name in ["rust-analyzer", "scip-python"] {
        assert!(stderr.contains(name), "the diagnostic names {name}: {stderr}");
    }
}

// _(Indexer readiness report — human rendering)_ — the default human report is a fixed-width line per
// indexer naming each one and carrying an install hint for the absent one.
#[cfg(unix)]
#[test]
fn doctor_human_report_names_each_indexer_with_a_hint_when_absent() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_stub(bin_dir.path(), "rust-analyzer", "rust-analyzer 1.99.0-stub");
    // scip-python left absent.

    let out = c10r().env("PATH", bin_dir.path()).arg("doctor").output().unwrap();

    assert_eq!(out.status.code(), Some(5));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rust-analyzer"), "names rust-analyzer: {stdout}");
    assert!(stdout.contains("present"), "reports rust-analyzer present: {stdout}");
    assert!(stdout.contains("scip-python"), "names scip-python: {stdout}");
    assert!(stdout.contains("absent"), "reports scip-python absent: {stdout}");
    assert!(
        stdout.to_lowercase().contains("install"),
        "the absent line carries an install hint: {stdout}"
    );
}

// _(Source-faithful content rendering: doctor's human report)_ — a stub tool whose `--version` output
// carries an ANSI escape sequence is reported present, but the escape reaches neither the human
// report (sanitized) nor is it dropped from `--json` (preserved byte-exactly, JSON's own escaping the
// guard).
#[cfg(unix)]
#[test]
fn doctor_sanitizes_a_hostile_version_string_in_the_human_report_but_not_json() {
    let bin_dir = tempfile::tempdir().unwrap();
    let hostile_version = "rust-analyzer \x1b[31m1.99.0-stub";
    write_stub(bin_dir.path(), "rust-analyzer", hostile_version);
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let human = c10r().env("PATH", bin_dir.path()).arg("doctor").output().unwrap();
    assert_eq!(
        human.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human_stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        !human_stdout.contains('\u{1b}'),
        "the human doctor report carries no ANSI escape: {human_stdout:?}"
    );
    assert!(
        human_stdout.contains("rust-analyzer"),
        "the tool is still named: {human_stdout}"
    );

    let json = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).expect("the --json report parses");
    let rust_analyzer = report["indexers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "rust-analyzer")
        .unwrap();
    assert_eq!(
        rust_analyzer["version"], hostile_version,
        "--json preserves the hostile version byte-exactly: {rust_analyzer}"
    );
}

/// A stub-script preamble setting a standard `PATH`, so the stub can call `sleep`/`head`/`tr` even
/// though the probe runs it under the test's restricted `PATH` (which holds only the stub itself).
#[cfg(unix)]
const STUB_PREAMBLE: &str = "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\n";

/// Write an executable stub named `name` into `dir` that hangs on any invocation (a stuck indexer).
#[cfg(unix)]
fn write_hanging_stub(dir: &Path, name: &str) {
    write_script(dir, name, &format!("{STUB_PREAMBLE}exec sleep 300\n"));
}

/// Write an executable stub named `name` into `dir` that backgrounds a grandchild inheriting its
/// stdout, then waits on it: killing the direct child leaves the grandchild holding the stdout pipe
/// write-end open — the case that would hang a probe joining its reader thread.
#[cfg(unix)]
fn write_pipe_holding_stub(dir: &Path, name: &str) {
    write_script(dir, name, &format!("{STUB_PREAMBLE}sleep 300 &\nwait\n"));
}

/// Write an executable stub named `name` into `dir` that answers `--version` with a version line
/// followed by output far exceeding the probe's capture cap, then exits — exercising the drain caps.
#[cfg(unix)]
fn write_flooding_stub(dir: &Path, name: &str, version_line: &str) {
    write_script(
        dir,
        name,
        &format!(
            "{STUB_PREAMBLE}if [ \"$1\" = \"--version\" ]; then\n  echo \"{version_line}\"\n  \
             head -c 200000 /dev/zero | tr '\\0' 'x'\n  exit 0\nfi\nexit 1\n"
        ),
    );
}

/// Write `body` as an executable file named `name` into `dir`.
#[cfg(unix)]
fn write_script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
}

// _(Indexer readiness report: a present-but-unresponsive indexer)_ — a `rust-analyzer` stub that hangs
// when probed is reported unresponsive (distinct from absent) with an investigation hint, and the
// process exits with the indexer/setup-failure code within a bounded time.
#[cfg(unix)]
#[test]
fn doctor_reports_a_hanging_indexer_unresponsive_within_a_bounded_time() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_hanging_stub(bin_dir.path(), "rust-analyzer");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let start = Instant::now();
    let out = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < HANG_ASSERT_WINDOW,
        "the hanging probe is bounded, not waited on indefinitely: {elapsed:?}"
    );

    assert_eq!(
        out.status.code(),
        Some(5),
        "an unresponsive required indexer is the indexer/setup-failure code; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = report["indexers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "rust-analyzer")
        .unwrap();
    assert_eq!(
        rust["presence"], "unresponsive",
        "the hanging indexer is unresponsive, not absent: {rust}"
    );
    let hint = rust["hint"].as_str().expect("an unresponsive indexer carries a hint");
    assert!(
        hint.contains("did not respond") && hint.contains("stuck"),
        "the investigation hint points at a stuck process, distinct from an install hint: {hint}"
    );

    // The human render (the same stubs, no fresh tempdir/setup) names the unresponsive status word
    // and the investigation hint alongside the present `scip-python`.
    let human = c10r().env("PATH", bin_dir.path()).arg("doctor").output().unwrap();
    assert_eq!(
        human.status.code(),
        Some(5),
        "an unresponsive required indexer is the indexer/setup-failure code in the human render too"
    );
    let human_stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        human_stdout.contains("rust-analyzer") && human_stdout.contains("unresponsive"),
        "the human report names the unresponsive status: {human_stdout}"
    );
    assert!(
        human_stdout.to_lowercase().contains("stuck"),
        "the human report carries the investigation hint: {human_stdout}"
    );
}

// _(Indexer readiness report: scip-python present-but-unresponsive)_ — the unresponsive treatment
// applies to `scip-python`, not just `rust-analyzer`: a hanging `scip-python` stub alongside a
// healthy `rust-analyzer` is reported unresponsive with an investigation hint, and the process exits
// with the indexer/setup-failure code within a bounded time.
#[cfg(unix)]
#[test]
fn doctor_reports_a_hanging_scip_python_unresponsive_within_a_bounded_time() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_stub(bin_dir.path(), "rust-analyzer", "rust-analyzer 1.99.0-stub");
    write_hanging_stub(bin_dir.path(), "scip-python");

    let start = Instant::now();
    let out = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < HANG_ASSERT_WINDOW,
        "the hanging probe is bounded, not waited on indefinitely: {elapsed:?}"
    );

    assert_eq!(
        out.status.code(),
        Some(5),
        "an unresponsive required indexer is the indexer/setup-failure code; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let scip = report["indexers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "scip-python")
        .unwrap();
    assert_eq!(
        scip["presence"], "unresponsive",
        "the hanging scip-python indexer is unresponsive, not absent: {scip}"
    );
    let hint = scip["hint"].as_str().expect("an unresponsive indexer carries a hint");
    assert!(
        hint.contains("did not respond") && hint.contains("stuck"),
        "the investigation hint points at a stuck process: {hint}"
    );
}

// _(Indexer readiness report: a descendant holding the pipe open does not hang doctor)_ — the critical
// regression: a stub whose backgrounded grandchild keeps the stdout pipe open after the direct child
// is killed must not wedge the probe. Doctor completes within the window and reports the indexer
// unresponsive, proving the drain threads are abandoned rather than joined on the timeout path.
#[cfg(unix)]
#[test]
fn doctor_survives_a_descendant_holding_the_probe_pipe_open() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_pipe_holding_stub(bin_dir.path(), "rust-analyzer");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let start = Instant::now();
    let out = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < HANG_ASSERT_WINDOW,
        "a descendant-held pipe must not hang doctor: {elapsed:?}"
    );

    assert_eq!(
        out.status.code(),
        Some(5),
        "an unresponsive indexer is the setup-failure code"
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = report["indexers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "rust-analyzer")
        .unwrap();
    assert_eq!(
        rust["presence"], "unresponsive",
        "the pipe-holding indexer is reported unresponsive: {rust}"
    );
}

// _(Bounded probe: a flood of output past the capture cap is handled)_ — a `rust-analyzer` stub that
// prints a version line then floods stdout well past the cap still completes; the probe reports it
// present and parses its version, proving the capped, drained pipe neither blocks the child nor grows
// unbounded.
#[cfg(unix)]
#[test]
fn doctor_handles_an_indexer_that_floods_its_version_output() {
    let bin_dir = tempfile::tempdir().unwrap();
    write_flooding_stub(bin_dir.path(), "rust-analyzer", "9.9.9-stub");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let out = c10r()
        .env("PATH", bin_dir.path())
        .args(["--json", "doctor"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "a flooding-but-exiting indexer is present, not a failure; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = report["indexers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "rust-analyzer")
        .unwrap();
    assert_eq!(
        rust["presence"], "present",
        "the flooding indexer still reports present: {rust}"
    );
    assert!(
        rust["version"].as_str().is_some_and(|v| v.contains("9.9.9-stub")),
        "the version line survives the cap: {rust}"
    );
}

// _(Refresh-hook status report)_ — `doctor` reports each refresh event's hook status for the git
// worktree it runs in: installed, present with activation unverified, or not installed with a fix,
// plus whether the present hook files disagree on the installed workspaces. Most of these tests write
// the hooks directory's fixture files directly (the same way `src/hooks.rs`'s own unit tests do)
// rather than through `hooks install`, since the states under test — a disagreement, a hook another
// tool removed, a kept hook a later write has diverged from — are states installation itself refuses
// to produce or does not otherwise construct in one step.

/// Isolate a spawned `git` invocation from the developer's own config, matching `tests/hooks.rs`.
fn isolate_git_config(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

/// A throwaway git repository for one hook-status test, with a root commit so `HEAD` always resolves.
struct HookStatusRepo {
    dir: tempfile::TempDir,
}

impl HookStatusRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = HookStatusRepo { dir };
        repo.git(&["init", "-q"]);
        repo.git(&[
            "-c",
            "user.email=hook-status-test@example.com",
            "-c",
            "user.name=Hook Status Test",
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

    fn git(&self, args: &[&str]) {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(self.path());
        isolate_git_config(&mut cmd);
        let status = cmd.status().expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    /// The hooks directory `doctor` looks at for this repository: the path `git rev-parse --git-path
    /// hooks` names under the same git config isolation the `c10r` under test runs with, so a host's
    /// `core.hooksPath` cannot relocate the fixture files.
    fn hooks_dir(&self) -> PathBuf {
        let mut cmd = Command::new("git");
        cmd.args(["rev-parse", "--git-path", "hooks"]).current_dir(self.path());
        isolate_git_config(&mut cmd);
        let output = cmd.output().expect("git runs");
        assert!(output.status.success(), "git rev-parse --git-path hooks failed");
        let raw = String::from_utf8(output.stdout).unwrap();
        self.path().canonicalize().unwrap().join(raw.trim())
    }

    /// A fresh invocation of the built binary in this repository, with `PATH` set so `git` (needed by
    /// `doctor`'s own hook-status probing) is still found alongside `bin_dir`, which shadows anything
    /// of the same name already on `PATH` — the indexer stubs the doctor tests above already use.
    /// Isolated from the developer's own git config, matching the `git` helper above: `doctor` itself
    /// invokes git to resolve the hooks directory, and a host's own `core.hooksPath` would otherwise
    /// point it somewhere other than the repository this test set up.
    fn c10r(&self, bin_dir: &Path) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
        cmd.current_dir(self.path()).env("PATH", prepend_to_path(bin_dir));
        isolate_git_config(&mut cmd);
        cmd
    }
}

/// `PATH` with `bin_dir` prepended to the inherited `PATH`.
fn prepend_to_path(bin_dir: &Path) -> std::ffi::OsString {
    let mut path = std::ffi::OsString::from(bin_dir);
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    path
}

/// A `WorkspaceLine` naming `workspace`, rooted at `repo`.
fn workspace_line(repo: &HookStatusRepo, workspace: &str) -> WorkspaceLine {
    let root = repo.path().canonicalize().unwrap();
    let db = root.join(".c10r/index.db");
    WorkspaceLine {
        root,
        db,
        workspace: workspace.to_string(),
    }
}

/// Write a c10r file for every refresh event in `hooks_dir`, all naming `line`, executable exactly as
/// `hooks install` leaves it — `doctor` now reports a non-executable c10r file as not installed, since
/// git would not run it either.
fn write_installed_hooks(hooks_dir: &Path, line: &WorkspaceLine) {
    std::fs::create_dir_all(hooks_dir).unwrap();
    for event in HookEvent::ALL {
        let path = hooks_dir.join(event.as_str());
        std::fs::write(&path, render(event, std::slice::from_ref(line))).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

/// The captured prek shim fixture for `event`, the same fixtures `src/hooks.rs` recognizes.
fn prek_fixture(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PostCommit => include_str!("fixtures/prek-shims/0.5.3/post-commit"),
        HookEvent::PostRewrite => include_str!("fixtures/prek-shims/0.5.3/post-rewrite"),
        HookEvent::PostCheckout => include_str!("fixtures/prek-shims/0.5.3/post-checkout"),
        HookEvent::PostMerge => include_str!("fixtures/prek-shims/0.5.3/post-merge"),
    }
}

/// This event's entry in a `doctor --json` report's `hooks.events` array.
fn find_event(events: &[serde_json::Value], event: HookEvent) -> &serde_json::Value {
    events
        .iter()
        .find(|e| e["event"] == event.as_str())
        .unwrap_or_else(|| panic!("{} is reported: {events:?}", event.as_str()))
}

// _(Installed hooks are reported)_ — a workspace installed for every refresh event is reported
// installed with that workspace, for every event.
#[test]
fn installed_hooks_are_reported() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    write_installed_hooks(&repo.hooks_dir(), &line);

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let hooks = &report["hooks"];
    assert_eq!(
        hooks["disagreement"], false,
        "a single workspace never disagrees: {hooks}"
    );
    let events = hooks["events"]
        .as_array()
        .expect("hook status applies inside a git worktree");
    assert_eq!(events.len(), 4, "every refresh event is reported: {hooks}");
    for event in events {
        assert_eq!(event["state"], "installed", "{event}");
        assert_eq!(event["workspaces"], serde_json::json!(["alpha"]), "{event}");
    }
}

// _(A non-executable hook is not reported installed)_ — git skips a hook file that is not executable,
// so a c10r file that lost its executable bit must not be reported installed either.
#[cfg(unix)]
#[test]
fn a_non_executable_hook_is_not_reported_installed() {
    use std::os::unix::fs::PermissionsExt;

    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_installed_hooks(&hooks_dir, &line);
    std::fs::set_permissions(hooks_dir.join("post-merge"), std::fs::Permissions::from_mode(0o644)).unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_merge = find_event(events, HookEvent::PostMerge);
    assert_eq!(post_merge["state"], "not_installed", "{post_merge}");
    let fix = post_merge["fix"].as_str().expect("a not-installed event carries a fix");
    assert!(
        fix.contains(&hooks_dir.join("post-merge").display().to_string()) && fix.contains("make it executable"),
        "the fix names the file and says to make it executable: {fix}"
    );
    for other in [HookEvent::PostCommit, HookEvent::PostRewrite, HookEvent::PostCheckout] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

// _(A missing hook is reported with a fix)_ — the install command `doctor` suggests for an event whose
// refresh hook is gone restores it, including for a workspace identity that starts with `-`.
#[cfg(unix)]
#[test]
fn the_suggested_install_command_restores_a_missing_hook_for_a_dash_workspace() {
    let repo = HookStatusRepo::new();
    let exe_dir = Path::new(env!("CARGO_BIN_EXE_c10r")).parent().unwrap().to_path_buf();
    let install = repo
        .c10r(&exe_dir)
        .args(["hooks", "install", "--workspace=-foo"])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "install succeeds: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let post_merge = repo.hooks_dir().join("post-merge");
    std::fs::remove_file(&post_merge).unwrap();

    let out = repo.c10r(&exe_dir).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();
    let fix = find_event(events, HookEvent::PostMerge)["fix"]
        .as_str()
        .unwrap()
        .to_string();

    let mut cmd = Command::new("sh");
    cmd.args(["-c", &fix])
        .current_dir(repo.path())
        .env("PATH", prepend_to_path(&exe_dir));
    isolate_git_config(&mut cmd);
    let run = cmd.output().unwrap();
    assert!(
        run.status.success(),
        "the suggested command runs: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(post_merge.is_file(), "the missing hook is restored");
}

// _(A hand-edited hook is reported modified)_ — a command added by hand to an installed c10r file is
// still run by git, but the file no longer matches what installation would write, so it is reported
// modified rather than installed, naming the kept hook as where the user's own commands belong.
#[test]
fn a_hand_edited_hook_is_reported_modified() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_installed_hooks(&hooks_dir, &line);

    let path = hooks_dir.join("post-commit");
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replacen("exit $status\n", "echo added-by-hand\nexit $status\n", 1);
    std::fs::write(&path, edited).unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_commit = find_event(events, HookEvent::PostCommit);
    assert_eq!(post_commit["state"], "modified", "{post_commit}");
    assert_eq!(
        post_commit["workspaces_named"],
        serde_json::json!(["alpha"]),
        "{post_commit}"
    );
    assert!(post_commit.get("workspaces").is_none(), "{post_commit}");
    let note = post_commit["note"].as_str().expect("a modified event carries a note");
    assert!(
        note.contains("post-commit.c10r-prior"),
        "the note names the kept hook: {note}"
    );
    for other in [HookEvent::PostRewrite, HookEvent::PostCheckout, HookEvent::PostMerge] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

// _(Hooks under a supported hook manager are reported installed)_ — installing under a recognized
// prek shim places c10r at the legacy path; `doctor` reports that event installed too, exercising
// `hooks install` itself rather than a hand-written fixture.
#[cfg(unix)]
#[test]
fn hooks_under_a_supported_hook_manager_are_reported_installed() {
    use std::os::unix::fs::PermissionsExt;

    let repo = HookStatusRepo::new();
    let hooks_dir = repo.hooks_dir();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    for event in HookEvent::ALL {
        let path = hooks_dir.join(event.as_str());
        std::fs::write(&path, prek_fixture(event)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let bin_dir = tempfile::tempdir().unwrap();
    let install = repo
        .c10r(bin_dir.path())
        .args(["hooks", "install", "--workspace", "alpha"])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "hooks install under a recognized shim succeeds: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    for event in events {
        assert_eq!(
            event["state"], "installed",
            "a c10r file placed at the legacy path under a recognized shim is installed: {event}"
        );
    }
}

// _(Hooks under an unrecognized file are reported unverified)_ — a c10r file at the legacy path under
// a file this build does not recognize as a hook manager is reported present with activation
// unverified, and its workspaces are not reported as refreshed.
#[test]
fn hooks_under_an_unrecognized_file_are_reported_unverified() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_installed_hooks(&hooks_dir, &line);

    let e_path = hooks_dir.join("post-commit");
    let legacy_path = hooks_dir.join("post-commit.legacy");
    std::fs::rename(&e_path, &legacy_path).unwrap();
    std::fs::write(&e_path, "#!/bin/sh\necho unrelated\n").unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_commit = find_event(events, HookEvent::PostCommit);
    assert_eq!(post_commit["state"], "unverified", "{post_commit}");
    assert_eq!(
        post_commit["workspaces_named"],
        serde_json::json!(["alpha"]),
        "{post_commit}"
    );
    assert!(
        post_commit.get("workspaces").is_none(),
        "an unverified hook's workspaces are not reported as refreshed: {post_commit}"
    );
    for other in [HookEvent::PostRewrite, HookEvent::PostCheckout, HookEvent::PostMerge] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

// _(A removed hook is reported with its fix)_ — a c10r file another tool removed for one event is
// reported not installed with the install command as its fix, and hook status leaves the exit code
// exactly what indexer readiness alone determines.
#[cfg(unix)]
#[test]
fn a_removed_hook_is_reported_with_its_fix() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_installed_hooks(&hooks_dir, &line);
    std::fs::remove_file(hooks_dir.join("post-merge")).unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    write_stub(bin_dir.path(), "rust-analyzer", "rust-analyzer 1.99.0-stub");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");

    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "both indexers present is the success code regardless of hook status: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_merge = find_event(events, HookEvent::PostMerge);
    assert_eq!(post_merge["state"], "not_installed", "{post_merge}");
    let fix = post_merge["fix"].as_str().expect("a not-installed event carries a fix");
    assert!(
        fix.contains("c10r hooks install"),
        "the fix is the install command: {fix}"
    );
    assert!(
        fix.contains("alpha"),
        "the suggested command names the workspace already installed for the other events: {fix}"
    );
    for other in [HookEvent::PostCommit, HookEvent::PostRewrite, HookEvent::PostCheckout] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

// _(A remaining kept hook is reported with its path)_ — a hook kept from an earlier install that
// remains without a refresh hook, and that differs from the file now at the event's path, is reported
// not installed naming the kept hook's path and the need to remove or rename it.
#[test]
fn a_remaining_kept_hook_is_reported_with_its_path() {
    let repo = HookStatusRepo::new();
    let hooks_dir = repo.hooks_dir();
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let prior_path = hooks_dir.join("post-commit.c10r-prior");
    std::fs::write(&prior_path, "#!/bin/sh\necho original\n").unwrap();
    std::fs::write(hooks_dir.join("post-commit"), "#!/bin/sh\necho different\n").unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_commit = find_event(events, HookEvent::PostCommit);
    assert_eq!(post_commit["state"], "not_installed", "{post_commit}");
    let fix = post_commit["fix"]
        .as_str()
        .expect("a not-installed event carries a fix");
    assert!(
        fix.contains(&prior_path.display().to_string()),
        "the fix names the remaining kept hook's path: {fix}"
    );
    assert!(
        fix.contains("remove") && fix.contains("rename"),
        "the fix states the need to remove or rename it: {fix}"
    );
}

// _(Disagreeing hooks are reported)_ — refresh events whose c10r files name different workspace sets
// are each reported with their own file's workspaces, and the report notes the disagreement.
#[test]
fn disagreeing_hooks_are_reported() {
    let repo = HookStatusRepo::new();
    let hooks_dir = repo.hooks_dir();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let alpha = workspace_line(&repo, "alpha");
    let beta = workspace_line(&repo, "beta");
    let commit_path = hooks_dir.join("post-commit");
    std::fs::write(
        &commit_path,
        render(HookEvent::PostCommit, std::slice::from_ref(&alpha)),
    )
    .unwrap();
    let merge_path = hooks_dir.join("post-merge");
    std::fs::write(&merge_path, render(HookEvent::PostMerge, std::slice::from_ref(&beta))).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&commit_path, &merge_path] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let hooks = &report["hooks"];
    assert_eq!(hooks["disagreement"], true, "{hooks}");
    let events = hooks["events"].as_array().unwrap();
    assert_eq!(
        find_event(events, HookEvent::PostCommit)["workspaces"],
        serde_json::json!(["alpha"])
    );
    assert_eq!(
        find_event(events, HookEvent::PostMerge)["workspaces"],
        serde_json::json!(["beta"])
    );
}

// _(Outside a git worktree)_ — `doctor` run outside a git worktree states that hook status does not
// apply, and still exits with the code indexer readiness alone determines.
#[cfg(unix)]
#[test]
fn hook_status_outside_a_git_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = tempfile::tempdir().unwrap();
    write_stub(bin_dir.path(), "rust-analyzer", "rust-analyzer 1.99.0-stub");
    write_stub(bin_dir.path(), "scip-python", "0.9.9-stub");
    let doctor = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_c10r"))
            .current_dir(dir.path())
            .env("PATH", prepend_to_path(bin_dir.path()))
            .args(args)
            .output()
            .unwrap()
    };

    let out = doctor(&["--json", "doctor"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let hooks = &report["hooks"];
    assert!(
        hooks["reason"].as_str().is_some_and(|r| !r.is_empty()),
        "hook status names why it does not apply: {hooks}"
    );
    assert!(
        hooks["events"].is_null(),
        "no per-event status outside a git worktree: {hooks}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "both indexers present is the success code regardless of hook status: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let human = doctor(&["doctor"]);
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(
        stdout.lines().any(|l| l.starts_with("hooks: not applicable (")),
        "the human report states hook status does not apply: {stdout}"
    );
}

/// `doctor`'s human (non-`--json`) output lines for `repo`.
fn doctor_human_lines(repo: &HookStatusRepo, bin_dir: &Path) -> Vec<String> {
    let out = repo.c10r(bin_dir).arg("doctor").output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The human report line for `event`, which starts with the event's name after two spaces.
fn human_event_line(lines: &[String], event: HookEvent) -> &str {
    let prefix = format!("  {}  ", event.as_str());
    lines
        .iter()
        .find(|l| l.starts_with(&prefix))
        .unwrap_or_else(|| panic!("{} has a line: {lines:?}", event.as_str()))
}

// _(Installed hooks are reported, A hand-edited hook is reported modified, Hooks under an unrecognized
// file are reported unverified, A removed hook is reported with its fix)_ — the human `hooks:` section
// gives each event one line whose wording depends on its state.
#[test]
fn the_human_report_states_each_hook_state() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_installed_hooks(&hooks_dir, &line);

    let rewrite_path = hooks_dir.join("post-rewrite");
    let text = std::fs::read_to_string(&rewrite_path).unwrap();
    std::fs::write(
        &rewrite_path,
        text.replacen("exit $status\n", "echo added-by-hand\nexit $status\n", 1),
    )
    .unwrap();

    let checkout_path = hooks_dir.join("post-checkout");
    std::fs::rename(&checkout_path, hooks_dir.join("post-checkout.legacy")).unwrap();
    std::fs::write(&checkout_path, "#!/bin/sh\necho unrelated\n").unwrap();

    std::fs::remove_file(hooks_dir.join("post-merge")).unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let lines = doctor_human_lines(&repo, bin_dir.path());
    assert!(lines.iter().any(|l| l == "hooks:"), "the section is headed: {lines:?}");

    let commit = human_event_line(&lines, HookEvent::PostCommit);
    assert!(commit.contains("installed"), "{commit}");
    assert!(commit.contains("workspaces: alpha"), "{commit}");
    assert!(
        commit.contains(&hooks_dir.join("post-commit").display().to_string()),
        "{commit}"
    );

    let rewrite = human_event_line(&lines, HookEvent::PostRewrite);
    assert!(rewrite.contains("modified"), "{rewrite}");
    assert!(rewrite.contains("workspaces named: alpha"), "{rewrite}");
    assert!(
        rewrite.contains("installation and removal replace this file"),
        "{rewrite}"
    );
    assert!(rewrite.contains("post-rewrite.c10r-prior"), "{rewrite}");

    let checkout = human_event_line(&lines, HookEvent::PostCheckout);
    assert!(checkout.contains("present, activation unverified"), "{checkout}");
    assert!(!checkout.contains("workspaces"), "{checkout}");

    let merge = human_event_line(&lines, HookEvent::PostMerge);
    assert!(merge.contains("not installed"), "{merge}");
    assert!(merge.contains("fix: cd "), "{merge}");
    assert!(merge.contains("c10r hooks install"), "{merge}");
    assert!(
        !lines.iter().any(|l| l.contains("disagree")),
        "one workspace set never disagrees: {lines:?}"
    );
}

// _(Disagreeing hooks are reported)_ — the human report gives each event its own file's workspaces and
// adds a line stating that the files disagree.
#[test]
fn the_human_report_states_disagreeing_hooks() {
    let repo = HookStatusRepo::new();
    let hooks_dir = repo.hooks_dir();
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let alpha = workspace_line(&repo, "alpha");
    let beta = workspace_line(&repo, "beta");
    let commit_path = hooks_dir.join("post-commit");
    std::fs::write(
        &commit_path,
        render(HookEvent::PostCommit, std::slice::from_ref(&alpha)),
    )
    .unwrap();
    let merge_path = hooks_dir.join("post-merge");
    std::fs::write(&merge_path, render(HookEvent::PostMerge, std::slice::from_ref(&beta))).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&commit_path, &merge_path] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let bin_dir = tempfile::tempdir().unwrap();
    let lines = doctor_human_lines(&repo, bin_dir.path());
    assert!(
        human_event_line(&lines, HookEvent::PostCommit).contains("workspaces: alpha"),
        "{lines:?}"
    );
    assert!(
        human_event_line(&lines, HookEvent::PostMerge).contains("workspaces: beta"),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("disagree")),
        "the report states the files disagree: {lines:?}"
    );
}

/// Write the captured prek shim for every event as an executable file in `hooks_dir`, and beside it a
/// refresh hook for `line` at each event's `.legacy` path, executable exactly as `hooks install`
/// leaves it.
#[cfg(unix)]
fn write_shims_over_legacy_hooks(hooks_dir: &Path, line: &WorkspaceLine) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(hooks_dir).unwrap();
    for event in HookEvent::ALL {
        let shim = hooks_dir.join(event.as_str());
        std::fs::write(&shim, prek_fixture(event)).unwrap();
        let legacy = hooks_dir.join(format!("{}.legacy", event.as_str()));
        std::fs::write(&legacy, render(event, std::slice::from_ref(line))).unwrap();
        for path in [&shim, &legacy] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

// _(A non-executable hook is not reported installed)_ — under a recognized shim git runs the shim and
// the shim runs the legacy file, so an event is installed only when both are executable.
#[cfg(unix)]
#[test]
fn a_legacy_hook_is_not_reported_installed_when_the_shim_or_the_hook_is_not_executable() {
    use std::os::unix::fs::PermissionsExt;

    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_shims_over_legacy_hooks(&hooks_dir, &line);
    std::fs::set_permissions(hooks_dir.join("post-commit"), std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(
        hooks_dir.join("post-rewrite.legacy"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    for event in [HookEvent::PostCommit, HookEvent::PostRewrite] {
        let status = find_event(events, event);
        assert_eq!(status["state"], "not_installed", "{status}");
        assert!(
            status["fix"].as_str().unwrap().contains("not executable"),
            "the fix says the file is not executable: {status}"
        );
    }
    for other in [HookEvent::PostCheckout, HookEvent::PostMerge] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

// _(A hand-edited hook is reported modified)_ — a command added by hand to a refresh hook at the legacy
// path, under an executable recognized shim, is reported modified rather than installed.
#[cfg(unix)]
#[test]
fn a_hand_edited_legacy_hook_is_reported_modified() {
    let repo = HookStatusRepo::new();
    let line = workspace_line(&repo, "alpha");
    let hooks_dir = repo.hooks_dir();
    write_shims_over_legacy_hooks(&hooks_dir, &line);

    let legacy = hooks_dir.join("post-commit.legacy");
    let text = std::fs::read_to_string(&legacy).unwrap();
    std::fs::write(
        &legacy,
        text.replacen("exit $status\n", "echo added-by-hand\nexit $status\n", 1),
    )
    .unwrap();

    let bin_dir = tempfile::tempdir().unwrap();
    let out = repo.c10r(bin_dir.path()).args(["--json", "doctor"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let events = report["hooks"]["events"].as_array().unwrap();

    let post_commit = find_event(events, HookEvent::PostCommit);
    assert_eq!(post_commit["state"], "modified", "{post_commit}");
    assert_eq!(
        post_commit["workspaces_named"],
        serde_json::json!(["alpha"]),
        "{post_commit}"
    );
    assert!(post_commit.get("workspaces").is_none(), "{post_commit}");
    for other in [HookEvent::PostRewrite, HookEvent::PostCheckout, HookEvent::PostMerge] {
        assert_eq!(find_event(events, other)["state"], "installed");
    }
}

/// Write a database at `path` carrying c10r's ownership marker at the given schema version — a store
/// c10r recognizes as its own.
fn write_c10r_store(path: &Path, user_version: i64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE t (x)").unwrap();
    conn.pragma_update(
        None,
        "application_id",
        silent_cartographer::graph::store::APPLICATION_ID,
    )
    .unwrap();
    conn.pragma_update(None, "user_version", user_version).unwrap();
    drop(conn);
}

/// Write a database at `path` that another application created: a real SQLite file carrying its own
/// version stamp and no c10r marker. Version stamps are what nearly every application puts in
/// `user_version`, so this is what a mis-pointed `--db` most plausibly lands on.
fn write_foreign_store(path: &Path, user_version: i64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE t (x)").unwrap();
    conn.pragma_update(None, "user_version", user_version).unwrap();
    drop(conn);
}

// _(Index reset: a symbolic link at the store path is removed and disclosed)_ — unlinking a symlink
// never reaches the file it names, so `cache clear` over one removes the link and leaves the store on
// disk. The answer says so on both renderings, because a bare "removed index" would report success
// for an index that is still there.
#[cfg(unix)]
#[test]
fn cache_clear_over_a_symlink_removes_the_link_and_discloses_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("real-store.db");
    write_c10r_store(&store, silent_cartographer::graph::schema::SCHEMA_VERSION);
    let link = dir.path().join("index.db");
    std::os::unix::fs::symlink(&store, &link).unwrap();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&link)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "removing a symlinked store path succeeds; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        store.exists(),
        "the store the link named is left in place, because only the link was unlinked"
    );
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link itself is gone from the store path"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("symbolic link"),
        "standard error warns that a link, not the index, was removed: {stderr}"
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["symlink"], true,
        "the machine answer discloses the link, so a caller reading it alone still learns the index survives: {report}"
    );

    // The human rendering carries the same disclosure, on a fresh link over the surviving store.
    std::os::unix::fs::symlink(&store, &link).unwrap();
    let human = c10r()
        .args(["--db"])
        .arg(&link)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&human.stdout).trim(),
        format!(
            "removed the symbolic link at {}; the index it names is still there",
            link.display()
        ),
        "the human line says a link went, not the index"
    );
}

// _(Index reset: existing index removed)_ — `cache` removes a recognizable index store (a SQLite
// database carrying a nonzero `user_version` stamp) and reports the affected path, exiting with the
// success code.
#[test]
fn cache_removes_a_valid_index_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, 11);

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the index file was removed");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["removed"], true, "the report discloses removal: {report}");
    assert_eq!(
        report["path"].as_str().map(PathBuf::from).as_deref(),
        Some(db.as_path()),
        "the report names the affected path: {report}"
    );
}

// _(Index reset: human success line)_ — removing a valid index store without `--json` reports the
// removal on standard output, naming the (sanitized) affected path, and exits with the success code.
#[test]
fn cache_human_success_line_names_the_removed_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, 11);

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the index file was removed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("removed") && stdout.contains(&db.display().to_string()),
        "the human success line names the removed path: {stdout}"
    );
}

// _(Index reset: removal applies only to a recognizable index — recovery case)_ — a store stamped with
// a `user_version` other than the current schema version is still a c10r index and remains removable,
// so a store left by an older build can be reset.
#[test]
fn cache_removes_a_store_with_an_older_user_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, 999);

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "a nonzero (if wrong) version stamp is still an index; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the older-version index was removed");
}

// _(Index reset: a foreign versioned database is refused)_ — a database another application created
// and stamped with its own version carries no c10r marker, so `cache` refuses to remove it, leaving
// the file intact and exiting with a failure code rather than the usage code. A version stamp alone
// is what nearly every SQLite application writes, so it can never stand in for ownership.
#[test]
fn cache_refuses_a_foreign_versioned_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_foreign_store(&db, 7);

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(6),
        "another application's database is refused in the ownership category, like every other target \
         the guard cannot confirm is c10r's own"
    );
    assert!(db.exists(), "the refused file is left intact");
    assert!(
        out.stdout.is_empty(),
        "no answer on standard output when the command refuses"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a c10r index") && stderr.contains("rm "),
        "the refusal teaches the manual alternative: {stderr}"
    );
}

// _(Index reset: a non-index target is refused — text file)_ — a plain text file at the `--db` path is
// not a SQLite database, so `cache` refuses to unlink it and names the manual `rm` alternative,
// leaving the file intact.
#[test]
fn cache_refuses_a_non_sqlite_text_file_naming_the_rm_alternative() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::write(&db, b"not a real sqlite file, cache must not remove it").unwrap();

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(6),
        "a foreign file is refused in the ownership category, not silently removed"
    );
    assert!(db.exists(), "the foreign file is left intact");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a c10r index"),
        "the refusal explains why it will not remove the file: {stderr}"
    );
    assert!(
        stderr.contains(&format!("rm {}", db.display())),
        "the refusal names the manual `rm` alternative on the path: {stderr}"
    );
}

// _(Store ownership recognition: an unexaminable path is refused without an ownership claim)_ — a
// directory at the `--db` path cannot be examined, so `cache` refuses it naming the path and the
// cause, leaves it in place, and exits with the ownership code. The raw OS error never reaches the
// caller alone, and the refusal never claims the target is somebody else's — the read that would have
// established that never completed.
#[test]
fn cache_refuses_a_path_it_cannot_examine() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(".c10r");
    std::fs::create_dir(&db).unwrap();

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(6),
        "an unexaminable path exits with the ownership code"
    );
    assert!(db.is_dir(), "the directory is left where it was");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot read") && stderr.contains(&db.display().to_string()),
        "the refusal names the path and that it could not be read: {stderr}"
    );
    assert!(
        !stderr.contains("is not a c10r index store"),
        "no ownership verdict is claimed: {stderr}"
    );
}

// _(Index reset: a non-index target is refused — empty file)_ — a zero-byte file at the `--db` path
// carries no ownership marker, so `cache` refuses it and leaves it intact. Store creation is atomic
// (built beside the path, renamed into place), so an empty file there is never c10r's own leftover —
// it is somebody else's file, and unlinking it on a guess is exactly what the guard exists to stop.
#[test]
fn cache_refuses_an_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::write(&db, b"").unwrap();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(6),
        "an empty file is refused in the ownership category, not unlinked on a guess"
    );
    assert!(db.exists(), "the refused file is left intact");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not a c10r index") && stderr.contains(&format!("rm {}", db.display())),
        "the refusal names the manual alternative: {stderr}"
    );
}

// _(Index reset: nothing to remove is success)_ — `cache` against a workspace with no stored index
// exits with the success code, not a failure.
#[test]
fn cache_is_success_when_there_is_nothing_to_remove() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    assert!(!db.exists());

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an already-absent index is success, not a failure; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["removed"], false, "nothing was removed: {report}");
}

// _(Index reset: removal error names the path)_ — an OS error removing the index is reported as a
// failure naming the affected path, exiting with a failure code.
//
// The store here is genuinely c10r's own and clears the ownership guard; what fails is the unlink
// itself, because the directory holding it is not writable. That ordering is the point: a target the
// guard refuses never reaches `remove_file`, so a fixture the guard rejects (a directory, a foreign
// file) cannot exercise this scenario at all.
#[cfg(unix)]
#[test]
fn cache_removal_os_error_names_the_path() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let holder = dir.path().join("held");
    std::fs::create_dir(&holder).unwrap();
    let db = holder.join("index.db");
    write_c10r_store(&db, 11);
    std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o500)).unwrap();

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("clear")
        .output()
        .unwrap();

    // Root unlinks straight through the directory's write bit, so the failure is unobservable there.
    let unlinked = !db.exists();
    std::fs::set_permissions(&holder, std::fs::Permissions::from_mode(0o700)).unwrap();
    if unlinked {
        return;
    }

    assert_eq!(
        out.status.code(),
        Some(1),
        "a failed unlink is an operational failure, not an ownership refusal — the store was c10r's own"
    );
    assert!(
        out.stdout.is_empty(),
        "no answer on standard output when the command fails"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&db.display().to_string()),
        "the failure diagnostic names the affected path: {stderr}"
    );
    assert!(db.exists(), "the store the unlink failed on is still there");
}

/// Write an executable stub `rust-analyzer` that answers `--version` on standard output and, for a
/// `scip <root> --output <path>` invocation, writes an empty SCIP index to `<path>` and exits
/// successfully — so a `build` invocation runs to completion without a real analyzer.
#[cfg(unix)]
fn write_rust_analyzer_stub(dir: &Path, version_line: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("rust-analyzer-stub");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then\n  echo \"{version_line}\"\n  exit 0\nfi\n\
             while [ $# -gt 0 ]; do\n\
             \x20 if [ \"$1\" = \"--output\" ]; then\n    : > \"$2\"\n    exit 0\n  fi\n\
             \x20 shift\n\
             done\n\
             exit 1\n"
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

// _(Source discovery excludes undecodable files — process level)_ — `build`, driven through the
// built binary over a workspace holding a source file whose bytes are not valid UTF-8, exits with
// the success code and names the excluded file on standard error, never on standard output.
//
// The stub analyzer writes nothing itself, so standard output here is exactly `build`'s own
// `--json` answer — this test's narrow claim is that the exclusion diagnostic does not corrupt it,
// not that `--json build` output always parses. A real analyzer's own progress output can land on
// the same stream (`rust_adapter.rs`/`python_adapter.rs` run their child processes without
// redirecting standard output), a pre-existing gap this change neither introduces nor fixes.
#[cfg(unix)]
#[test]
fn build_excludes_an_undecodable_file_and_still_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "# fixture manifest\n").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(dir.path().join("src/undecodable.rs"), [b'/', b'/', 0xCF, 0xF0, b'\n']).unwrap();

    let stub = write_rust_analyzer_stub(dir.path(), "rust-analyzer 1.99.0-stub");
    let db = dir.path().join("index.db");

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("--json")
        .arg("build")
        .arg(dir.path())
        .args(["--language", "rust", "--rust-analyzer"])
        .arg(&stub)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an undecodable file does not stop the build; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("undecodable.rs"),
        "the diagnostic names the excluded file: {stderr}"
    );

    // With this stub, standard output is exactly the `--json` answer: the exclusion diagnostic
    // never leaks onto it, so it parses undisturbed.
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "with a silent stub analyzer, stdout is the --json answer undisturbed by the \
             exclusion diagnostic: {e}\nstdout: {}",
            String::from_utf8_lossy(&out.stdout)
        )
    });
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("undecodable.rs"),
        "the exclusion diagnostic appears only on standard error, never standard output: {report}"
    );
    assert_eq!(report["rebuilt"], true, "the build actually ran: {report}");
}

// _(Naming no action removes nothing)_ — `cache` invoked without an action is rejected as a usage
// error naming all three valid actions, and the index at `--db` is left in place.
#[test]
fn cache_with_no_action_is_a_usage_error_and_removes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);

    let out = c10r().args(["--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(out.status.code(), Some(2), "no action named is the usage code");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for action in ["clear", "dir", "size"] {
        assert!(stderr.contains(action), "the usage error names {action}: {stderr}");
    }
    assert!(db.exists(), "the index is untouched when no action is named");
}

// _(Index store location report: directory reported for an existing store)_ — `cache dir` over a
// workspace holding a stored index reports the directory containing it and exits with the success
// code; the machine-readable answer also carries the `--db` path alongside the directory.
#[test]
fn cache_dir_reports_the_containing_directory_of_an_existing_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("dir")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["dir"].as_str().map(PathBuf::from).as_deref(),
        Some(dir.path()),
        "the report names the containing directory: {report}"
    );
    assert_eq!(
        report["db"].as_str().map(PathBuf::from).as_deref(),
        Some(db.as_path()),
        "the report also carries the --db path: {report}"
    );
}

// _(Index store location report: directory reported when no store exists)_ — `cache dir` against a
// `--db` path where no store exists still reports the directory the store would occupy, and exits
// with the success code.
#[test]
fn cache_dir_reports_the_directory_when_no_store_exists() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    assert!(!db.exists());

    let out = c10r().args(["--db"]).arg(&db).arg("cache").arg("dir").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an absent store still reports its directory; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim(),
        dir.path().display().to_string(),
        "the directory alone is reported: {stdout}"
    );
}

// _(Index store size report: size of a recognized store)_ — `cache size --json` over a store the
// system recognizes as its own reports the store's own byte count under the recognized state, and
// exits with the success code.
#[test]
fn cache_size_reports_the_byte_count_of_a_recognized_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);
    let expected_bytes = std::fs::metadata(&db).unwrap().len();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["state"], "recognized", "a c10r store is recognized: {report}");
    assert_eq!(
        report["bytes"].as_u64(),
        Some(expected_bytes),
        "the byte count matches the store file's own length: {report}"
    );
}

// _(Index store size report: auxiliary files count toward the total)_ — `cache size --json` over a
// recognized store accompanied by its `-wal` and `-shm` sidecar files reports a total covering the
// store and both sidecars together, not the store file alone.
#[test]
fn cache_size_sums_the_store_and_its_wal_shm_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);
    let wal = dir.path().join("index.db-wal");
    let shm = dir.path().join("index.db-shm");
    std::fs::write(&wal, b"wal sidecar contents").unwrap();
    std::fs::write(&shm, b"shm").unwrap();
    let db_only_bytes = std::fs::metadata(&db).unwrap().len();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["state"], "recognized", "a c10r store is recognized: {report}");
    // Opening the store to classify it touches the sidecars, so their post-run sizes rather than the
    // bytes this test wrote are the ones the total must match; the point under test is that they are
    // summed in at all, evidenced by the total exceeding the store file alone.
    let expected_bytes = std::fs::metadata(&db).unwrap().len()
        + std::fs::metadata(&wal).unwrap().len()
        + std::fs::metadata(&shm).unwrap().len();
    assert_eq!(
        report["bytes"].as_u64(),
        Some(expected_bytes),
        "the total covers the store and both sidecars together: {report}"
    );
    assert!(
        report["bytes"].as_u64().unwrap() > db_only_bytes,
        "the sidecars contribute to the total, beyond the store file alone: {report}"
    );
}

// _(Index store size report: an incompatible store reports a size)_ — `cache size --json` over a
// store carrying an older schema version reports its byte count, names the incompatible state, and
// exits with the success code.
#[test]
fn cache_size_reports_bytes_and_incompatible_state_for_an_older_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, 11);
    let expected_bytes = std::fs::metadata(&db).unwrap().len();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an incompatible store is still a success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["state"], "incompatible",
        "the older version is incompatible: {report}"
    );
    assert_eq!(
        report["bytes"].as_u64(),
        Some(expected_bytes),
        "an incompatible store still reports its byte count: {report}"
    );
}

// _(Index store size report: a recognized store that cannot be measured reports no size)_ — a
// sidecar that exists but cannot be examined leaves part of the total unknown. The answer keeps the
// state the classification reached and omits the figure, because a total missing one file understates
// what is at the path. A symlink loop is the fixture: it is a file that is present and unstattable,
// which an absent sidecar (the resting state, worth zero) is not.
#[cfg(unix)]
#[test]
fn cache_size_reports_no_bytes_when_a_sidecar_cannot_be_examined() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);
    std::os::unix::fs::symlink(dir.path().join("index.db-shm"), dir.path().join("index.db-wal")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("index.db-wal"), dir.path().join("index.db-shm")).unwrap();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an unmeasurable store is still a success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["state"], "recognized",
        "the classification still stands; only the measurement failed: {report}"
    );
    assert!(
        report.get("bytes").is_none(),
        "a total that cannot cover every file is withheld rather than understated: {report}"
    );

    let human = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&human.stdout).trim(),
        format!("cannot measure the store at {}", db.display()),
        "the human rendering names the path without a figure"
    );
}

// _(Index store size report: the human rendering states the size)_ — the default rendering of a
// recognized store is its byte count against the path, not the JSON answer.
#[test]
fn cache_size_human_line_states_the_byte_count_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_c10r_store(&db, silent_cartographer::graph::schema::SCHEMA_VERSION);
    let expected_bytes = std::fs::metadata(&db).unwrap().len();

    let out = c10r()
        .args(["--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("{expected_bytes} bytes at {}", db.display()),
        "the human line carries the count and the path"
    );
}

// _(Index store size report: an absent store reports no size)_ — `cache size --json` against a
// `--db` path where no store exists names the absent state, carries no `bytes` key, and exits with
// the success code.
#[test]
fn cache_size_reports_no_bytes_for_an_absent_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    assert!(!db.exists());

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an absent store is still a success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["state"], "absent", "no store exists at the path: {report}");
    assert!(
        report.get("bytes").is_none(),
        "no byte count for an absent store: {report}"
    );
}

// _(Index store size report: an unrecognized file reports no size)_ — `cache size --json` against a
// file the system does not recognize as its own store names the unrecognized state, carries no
// `bytes` key, exits with the success code, and leaves the file's original bytes untouched.
#[test]
fn cache_size_reports_no_bytes_for_a_foreign_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let original = b"not a real sqlite file, cache size must not remove it".to_vec();
    std::fs::write(&db, &original).unwrap();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an unrecognized file is still a success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["state"], "unrecognized",
        "a file with no c10r marker is unrecognized: {report}"
    );
    assert!(
        report.get("bytes").is_none(),
        "no byte count for an unrecognized file: {report}"
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        original,
        "the foreign file's bytes are untouched by a size read"
    );
}

// _(Index store size report: an unexaminable path reports no size)_ — `cache size --json` against a
// directory at the `--db` path names the unreadable state, distinct from absent and unrecognized,
// carries no `bytes` key, and exits with the success code.
#[test]
fn cache_size_reports_no_bytes_for_an_unreadable_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::create_dir(&db).unwrap();

    let out = c10r()
        .args(["--json", "--db"])
        .arg(&db)
        .arg("cache")
        .arg("size")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an unreadable path is still a success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["state"], "unreadable",
        "a directory at the store path is unreadable, not absent or unrecognized: {report}"
    );
    assert!(
        report.get("bytes").is_none(),
        "no byte count for an unreadable path: {report}"
    );
}
