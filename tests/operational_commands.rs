//! Operational-command contract tests: `doctor`'s indexer readiness report and `cache`'s index reset.
//!
//! These drive the built `c10r` binary through `std::process::Command` so the process-level contract
//! (exit codes, stdout/stderr separation) is observed as a caller would. `doctor` probes tools by
//! name on `PATH`, so its tests control `PATH` for the spawned process rather than depending on
//! whichever indexers happen to be installed on the host running the suite.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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

    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let statuses = statuses.as_array().expect("the report is a list of indexer statuses");
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
    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let statuses = statuses.as_array().expect("the report is a list of indexer statuses");
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
    let statuses: serde_json::Value = serde_json::from_slice(&json.stdout).expect("the --json report parses");
    let rust_analyzer = statuses
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
    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = statuses
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
    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let scip = statuses
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
    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = statuses
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
    let statuses: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    let rust = statuses
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

/// Write a SQLite database file at `path` carrying the SQLite magic header and the given
/// `user_version` stamp — a nonzero stamp marks it a recognizable c10r index store, a zero stamp a
/// bare SQLite file that is not one.
fn write_sqlite_store(path: &Path, user_version: i64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE t (x)").unwrap();
    conn.pragma_update(None, "user_version", user_version).unwrap();
    drop(conn);
}

// _(Index reset: existing index removed)_ — `cache` removes a recognizable index store (a SQLite
// database carrying a nonzero `user_version` stamp) and reports the affected path, exiting with the
// success code.
#[test]
fn cache_removes_a_valid_index_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_sqlite_store(&db, 11);

    let out = c10r().args(["--json", "--db"]).arg(&db).arg("cache").output().unwrap();

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
    write_sqlite_store(&db, 11);

    let out = c10r().args(["--db"]).arg(&db).arg("cache").output().unwrap();

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
    write_sqlite_store(&db, 999);

    let out = c10r().args(["--json", "--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "a nonzero (if wrong) version stamp is still an index; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the older-version index was removed");
}

// _(Index reset: a non-index target is refused)_ — a SQLite file whose `user_version` is zero carries
// no build stamp, so `cache` refuses to remove it, leaving the file intact and exiting with a failure
// code rather than the usage code.
#[test]
fn cache_refuses_a_sqlite_file_with_a_zero_user_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    write_sqlite_store(&db, 0);

    let out = c10r().args(["--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "an unstamped SQLite file is refused as a plain operational failure, not a usage error"
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

    let out = c10r().args(["--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(1),
        "a foreign file is refused, not silently removed"
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

// _(Index reset: an empty file is a failed-create artifact)_ — a zero-byte file at the `--db` path is a
// failed-create artifact, not a foreign file, so `cache` removes it as a success.
#[test]
fn cache_removes_an_empty_file_as_success() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::write(&db, b"").unwrap();

    let out = c10r().args(["--json", "--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an empty failed-create artifact is removed as success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the empty artifact was removed");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(
        report["removed"], true,
        "the empty artifact's removal is disclosed: {report}"
    );
}

// _(Index reset: nothing to remove is success)_ — `cache` against a workspace with no stored index
// exits with the success code, not a failure.
#[test]
fn cache_is_success_when_there_is_nothing_to_remove() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    assert!(!db.exists());

    let out = c10r().args(["--json", "--db"]).arg(&db).arg("cache").output().unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "an already-absent index is success, not a failure; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json report parses");
    assert_eq!(report["removed"], false, "nothing was removed: {report}");
}

// _(Index reset: removal error names the path)_ — an OS error removing the index (the path is a
// directory, not a file) is reported as a failure naming the affected path, exiting with a failure
// code.
#[test]
fn cache_removal_os_error_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::create_dir(&db).unwrap(); // a directory at the db path: remove_file errors on it

    let out = c10r().args(["--db"]).arg(&db).arg("cache").output().unwrap();

    assert_ne!(out.status.code(), Some(0), "an OS removal error is not success");
    assert!(
        out.stdout.is_empty(),
        "no answer on standard output when the command fails"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&db.display().to_string()),
        "the failure diagnostic names the affected path: {stderr}"
    );
    assert!(db.exists(), "the directory at the db path is untouched");
}
