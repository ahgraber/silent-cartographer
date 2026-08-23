//! Command-surface contract tests: the exit-code taxonomy, stream discipline, the closed flag
//! vocabulary, enum-value rejection with valid alternatives, and non-interactive operation.
//!
//! These drive the built `c10r` binary through `std::process::Command` so the process-level
//! contract (exit codes, stdout/stderr separation) is observed as a caller would.

mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::CommandFactory;

/// Build the exemplar Rust fixture into a store at `dir/index.db` and return its path.
fn build_fixture_db(dir: &Path) -> PathBuf {
    support::build_fixture_db(dir, "op-ws")
}

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`).
fn c10r() -> Command {
    Command::new(env!("CARGO_BIN_EXE_c10r"))
}

// _(Exit-code taxonomy: success on a non-empty answer)_ — a query returning results exits with the
// success code.
#[test]
fn success_code_on_a_non_empty_answer() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client::connect", "--detail", "location"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty(), "the answer is on standard output");
}

// _(Exit-code taxonomy: a typed-empty answer is still success)_ — a relation that resolves but stands
// in no instance exits with the success code, not a failure code.
#[test]
fn success_code_on_a_typed_empty_answer() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // A method contains no declarations: a definite empty relation, not a failure. The machine
    // answer's outcome tag pins the emptiness; the exit code is the load-bearing check.
    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "trace", "net::Client::connect", "--relation", "contains"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    assert_eq!(
        answer["outcome"]["outcome"], "empty",
        "the answer is a typed-empty outcome"
    );
}

// _(Symbol search by name fragment — bounded answer through the binary)_ — `find` honors `--limit`,
// disclosing truncation over the deterministic order, and a fragment matching nothing is a
// success-coded typed-absence answer, not a failure.
#[test]
fn find_honors_limit_and_reports_typed_absence() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // "conn" matches both `connect` and `disconnect` in the fixture; capped to one result, the
    // answer discloses truncation and carries a continuation token.
    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "conn", "--limit", "1"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    assert_eq!(answer["outcome"]["outcome"], "found");
    assert_eq!(
        answer["outcome"]["results"].as_array().unwrap().len(),
        1,
        "the result set is capped to --limit"
    );
    assert_eq!(answer["page"]["total"], 2, "both matches are counted in the total");
    assert_eq!(answer["page"]["truncated"], true, "truncation is disclosed");

    // A fragment matching nothing is typed absence, still exit-code success.
    let none = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "zzz_no_such_fragment"])
        .output()
        .unwrap();
    assert_eq!(none.status.code(), Some(0), "typed absence is still success");
    let none_answer: serde_json::Value = serde_json::from_slice(&none.stdout).expect("the --json answer parses");
    assert_eq!(
        none_answer["outcome"]["outcome"], "absent",
        "no match is typed absence, distinct from empty or a failure"
    );
}

// _(Exit-code taxonomy: usage error is distinct)_ — an unknown flag is a usage error, distinct from
// success and failure.
#[test]
fn usage_code_on_a_bad_flag() {
    let out = c10r()
        .arg("--db")
        .arg("/nonexistent/index.db")
        .args(["get", "x", "--not-a-flag"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "clap reports an unknown flag as a usage error"
    );
}

// _(Rejections name valid alternatives: validation precedes side effects — precedence over
// no-index)_ — a `get` with neither a reference nor `--at` is malformed regardless of index state,
// so it is a usage error even when no index exists.
#[test]
fn usage_error_takes_precedence_over_a_missing_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("get")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "a malformed invocation is a usage error before the index is consulted: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the rejected invocation created no index");
}

// _(Scenario: An overlap that is not smaller than the chunk size is refused)_ — the pair is
// validated before anything is resolved or analyzed, so the refusal is a usage error and no build
// is performed.
#[test]
fn an_overlap_not_smaller_than_the_chunk_size_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["build", "--chunk-size", "128", "--chunk-overlap", "128"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "an overlap equal to the chunk size is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--chunk-overlap"),
        "the refusal names the malformed pair: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the rejected invocation performed no build");
}

// _(Scenario: A chunk size leaving no room for content is refused)_ — a size that cannot admit
// content beside a passage's header is rejected as a usage error before any build begins.
#[test]
fn a_chunk_size_leaving_no_room_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["build", "--chunk-size", "1"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "a chunk size leaving no room for content is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the rejected invocation performed no build");
}

// _(Exit-code taxonomy: absent index is distinct)_ — a query against a directory with no built index
// exits with the no-index code, and creates no index as a side effect.
#[test]
fn no_index_code_on_an_unbuilt_directory() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(3), "an absent index is the no-index code");
    assert!(!db.exists(), "a query did not bring an empty index into being");
}

// _(Exit-code taxonomy: a never-built store is absent)_ — a store stamped with the current schema
// version but carrying no build metadata (its tables created, never populated) is reported as an
// absent index (exit 3), not answered as an empty one.
#[test]
fn no_index_code_on_a_schema_stamped_metadata_less_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // Opening a store at the path creates the schema and stamps the version, but writes no metadata —
    // the shape a build leaves if it never completes.
    {
        let _store = silent_cartographer::graph::store::GraphStore::open_or_replace(&db).unwrap();
    }
    assert!(db.exists(), "the stamped-but-empty store file exists");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(3),
        "a metadata-less store is the no-index code, not a success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("incomplete") || stderr.contains("no build metadata"),
        "the diagnostic explains the store is incomplete: {stderr}"
    );
}

// _(Source-faithful content rendering: stderr diagnostics)_ — an error whose message embeds a
// user-supplied path carrying an ANSI escape (a hostile `--db` path) is sanitized before reaching the
// terminal, so no raw escape byte lands on standard error.
#[test]
fn stderr_diagnostics_sanitize_a_hostile_path() {
    let dir = tempfile::tempdir().unwrap();
    // A `--db` path embedding an ANSI escape; the file does not exist, so the query fails with a
    // no-index diagnostic that names the path.
    let hostile_db = dir.path().join("\u{1b}[31mindex.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&hostile_db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(3), "a missing index is the no-index code");
    assert!(
        !out.stderr.contains(&0x1b),
        "the diagnostic carries no raw ANSI escape byte: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// _(Exit-code taxonomy: incompatible store is distinct)_ — a store c10r created, stamped with a
// different schema version, exits with the incompatible-store code, distinct from the no-index code.
#[test]
fn incompatible_store_code_on_a_wrong_user_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // A store carrying c10r's ownership marker — so it is c10r's own to replace — at a schema
    // version this binary does not recognize. Without the marker the file would be refused for
    // ownership instead, which is a different category with the opposite remedy.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(
        None,
        "application_id",
        silent_cartographer::graph::store::APPLICATION_ID,
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 999i64).unwrap();
    drop(conn);

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(4),
        "a wrong-version store is the incompatible-store code"
    );
}

// _(Exit-code taxonomy: indexer setup failure is distinct)_ — a build whose required indexer is
// absent exits with the indexer/setup-failure code.
#[test]
fn indexer_setup_code_on_a_missing_indexer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let db = dir.path().join("index.db");
    let missing_analyzer = dir.path().join("no-such-rust-analyzer");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("build")
        .arg(dir.path())
        .arg("--rust-analyzer")
        .arg(&missing_analyzer)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(5),
        "a missing indexer is the indexer/setup-failure code"
    );
}

// _(Output stream discipline)_ — a successful answer lands on standard output with a clean standard
// error; a failing invocation lands its diagnostic on standard error with a clean standard output.
#[test]
fn answer_on_stdout_diagnostic_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let ok = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client::connect", "--detail", "location"])
        .output()
        .unwrap();
    assert_eq!(ok.status.code(), Some(0));
    assert!(!ok.stdout.is_empty(), "the answer is on standard output");
    assert!(
        ok.stderr.is_empty(),
        "no diagnostic on a successful answer: {}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let missing = dir.path().join("absent.db");
    let err = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&missing)
        .args(["get", "net::Client"])
        .output()
        .unwrap();
    assert_ne!(err.status.code(), Some(0));
    assert!(
        err.stdout.is_empty(),
        "no answer on standard output when the query fails"
    );
    assert!(!err.stderr.is_empty(), "the diagnostic is on standard error");
}

/// Collect every long-flag spelling accepted across the command tree — canonical names and every
/// alias, hidden or visible — excluding clap's auto-generated `--help` and `--version`.
///
/// Aliases are collected because clap accepts them at runtime exactly like the canonical spelling:
/// an alias that never entered this set would pass the vocabulary assertion while silently widening
/// the accepted surface.
fn collect_long_flags(cmd: &clap::Command, out: &mut BTreeSet<String>) {
    for arg in cmd.get_arguments() {
        if let Some(long) = arg.get_long()
            && long != "help"
            && long != "version"
        {
            out.insert(long.to_string());
        }
        if let Some(aliases) = arg.get_all_aliases() {
            for alias in aliases {
                out.insert(alias.to_string());
            }
        }
    }
    for sub in cmd.get_subcommands() {
        collect_long_flags(sub, out);
    }
}

// _(Closed flag vocabulary)_ — walking the built clap command tree yields exactly the recorded flag
// vocabulary, and the known banned alias spellings are undefined.
#[test]
fn clap_tree_matches_the_recorded_flag_vocabulary() {
    let cmd = silent_cartographer::cli::Cli::command();
    let mut flags = BTreeSet::new();
    collect_long_flags(&cmd, &mut flags);

    let expected: BTreeSet<String> = [
        // Global option surface, shared across commands.
        "db",
        "workspace",
        "json",
        "color",
        // Query commands. `--limit`/`--max-lines`/`--from`/`--cursor` are per-command bounds on the
        // query commands, not globals, so a command they are meaningless for rejects them as unknown.
        "limit",
        "max-lines",
        "from",
        "cursor",
        "detail",
        "at",
        "relation",
        "depth",
        // The dependents order selector on `trace` and `impact`.
        "order",
        // `impact`'s seed-mode selector.
        "staged",
        // Operational commands.
        "rust-analyzer",
        "language",
        "environment",
        // `build`'s override for a store that already describes the workspace.
        "force",
        // `build`'s chunk parameters: the bound on each embedded chunk, and the overlap between
        // adjacent chunks of one passage.
        "chunk-size",
        "chunk-overlap",
        "discrepancies",
        "all",
        "duplicates",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    assert_eq!(flags, expected, "the flag vocabulary drifted from the recorded set");

    // `max-chars` is the retired character-based content bound, superseded by `--max-lines`; it is
    // banned so it cannot creep back alongside the line-based flag.
    for banned in ["format", "output", "top-k", "no-color", "max-chars"] {
        assert!(!flags.contains(banned), "banned alias `--{banned}` is defined");
    }
}

// _(Rejections name valid alternatives: unknown enum value enumerates the valid set)_ — an
// out-of-set `--relation` value is rejected with a message enumerating the valid relations.
#[test]
fn unknown_relation_value_enumerates_valid_relations() {
    let out = c10r()
        .arg("--db")
        .arg("/nonexistent/index.db")
        .args(["trace", "foo", "--relation", "bogus"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "an out-of-set enum value is a usage error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for relation in [
        "containers",
        "contains",
        "references",
        "dependents",
        "importers",
        "implementers",
    ] {
        assert!(
            stderr.contains(relation),
            "the diagnostic enumerates `{relation}`: {stderr}"
        );
    }
}

// _(Rejections name valid alternatives: validation precedes side effects)_ — an invalid argument to a
// mutating command is rejected before any state changes: no index is created.
#[test]
fn invalid_argument_to_a_mutating_command_changes_no_state() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("build")
        .arg(dir.path())
        .args(["--language", "bogus"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "an out-of-set enum value is a usage error");
    assert!(!db.exists(), "the rejected mutating invocation created no index");
}

// _(Bounding flags are query-command-local: an operational command rejects them before any side
// effect)_ — `--limit`/`--max-lines`/`--cursor` are defined only on the query commands, so `cache`
// rejects them as unknown arguments (usage error) before removing anything: a `cache` carrying a
// bounding flag leaves the index in place.
#[test]
fn cache_rejects_bounding_flags_and_removes_nothing() {
    for bounding in [["--cursor", "garbage"], ["--limit", "5"]] {
        let dir = tempfile::tempdir().unwrap();
        let db = build_fixture_db(dir.path());
        assert!(db.exists(), "the fixture index is in place before the rejected cache");

        let out = c10r()
            .current_dir(dir.path())
            .arg("--db")
            .arg(&db)
            .arg("cache")
            .args(bounding)
            .output()
            .unwrap();

        assert_eq!(
            out.status.code(),
            Some(2),
            "cache rejects a query-command bounding flag as an unknown argument: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            db.exists(),
            "the rejected cache removed no index (validation precedes the side effect)"
        );
    }
}

// _(Usage precedes no-index: a malformed cursor against an absent index)_ — a malformed `--cursor`
// is a usage error decided before the store is consulted, so it exits with the usage code even when
// no index exists, and creates none.
#[test]
fn malformed_cursor_is_a_usage_error_before_the_index_is_consulted() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "trace",
            "net::Client",
            "--relation",
            "references",
            "--limit",
            "1",
            "--cursor",
            "not-a-token",
        ])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "a malformed cursor is a usage error even against an absent index: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the rejected invocation created no index");
}

// _(Usage precedes no-index: a modal content-bound misuse against an absent index)_ — explicitly
// passing `--max-lines` when the detail is not content-bearing is a modal usage error decided before
// the store is consulted, so it exits with the usage code even when no index exists, and creates none.
#[test]
fn modal_content_bound_misuse_is_a_usage_error_before_the_index_is_consulted() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // `get` defaults to the `location` detail, which carries no content, so an explicit `--max-lines`
    // is meaningless — a modal usage error ahead of the no-index outcome.
    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client", "--max-lines", "5"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "an explicit --max-lines on a non-content detail is a usage error even against an absent index: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--max-lines") && stderr.contains("signature"),
        "the modal error names the flag and the accepting details: {stderr}"
    );
    assert!(!db.exists(), "the rejected invocation created no index");
}

// _(A positional reference and `--at` are mutually exclusive)_ — supplying both a reference and `--at`
// is a usage error naming both, rather than silently discarding the reference.
#[test]
fn get_reference_and_at_together_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client", "--at", "src/lib.rs:10"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "a reference together with --at is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        stderr.contains("--at") && stderr.contains("reference"),
        "the conflict diagnostic names both the reference and --at: {stderr}"
    );
}

// _(Rejections name valid alternatives: malformed `--at` position)_ — a `--at` value that is not
// `path:byte_offset` — missing the colon separator, or a non-numeric offset — is a usage error
// naming the expected form, decided independently of whether an index exists.
#[test]
fn malformed_at_position_is_a_usage_error_naming_the_expected_form() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    let no_colon = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "--at", "no-colon-here"])
        .output()
        .unwrap();
    assert_eq!(
        no_colon.status.code(),
        Some(2),
        "a --at value with no colon separator is a usage error: {}",
        String::from_utf8_lossy(&no_colon.stderr)
    );
    let no_colon_stderr = String::from_utf8_lossy(&no_colon.stderr);
    assert!(
        no_colon_stderr.contains("path:byte_offset")
            || no_colon_stderr.contains("path") && no_colon_stderr.contains("byte_offset"),
        "the diagnostic names the expected path:byte_offset form: {no_colon_stderr}"
    );

    let bad_offset = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "--at", "file.rs:notanumber"])
        .output()
        .unwrap();
    assert_eq!(
        bad_offset.status.code(),
        Some(2),
        "a --at value with a non-numeric offset is a usage error: {}",
        String::from_utf8_lossy(&bad_offset.stderr)
    );
    let bad_offset_stderr = String::from_utf8_lossy(&bad_offset.stderr).to_lowercase();
    assert!(
        bad_offset_stderr.contains("byte offset") || bad_offset_stderr.contains("byte_offset"),
        "the diagnostic names the offset that is invalid: {bad_offset_stderr}"
    );
    assert!(!db.exists(), "the rejected invocation created no index");
}

// _(Store ownership recognition: a `--db` path is a filename, never a SQLite URI)_ — SQLite reads a
// filename beginning `file:` as a URI naming a different file. The ownership guard inspects the path
// as a literal filename, so if a connection resolved it as a URI the guard would clear one file
// while the command read or wrote another — the guarantee inverted rather than merely bypassed.
// Both halves are checked here: a query answers from the literal file, and a build leaves the file a
// URI would have redirected to byte-identical.
//
// The spelling only bites for a *relative* path (an absolute one cannot begin with `file:`), so this
// runs through the binary with a controlled working directory rather than as a unit test.
#[cfg(unix)]
#[test]
fn a_db_path_spelled_like_a_uri_names_the_file_with_that_literal_name() {
    let dir = tempfile::tempdir().unwrap();

    // The file a URI-parsing open would land on: another application's database, holding its data.
    let redirect_target = dir.path().join("target.db");
    {
        let conn = rusqlite::Connection::open(&redirect_target).unwrap();
        conn.execute_batch("CREATE TABLE payroll (x); INSERT INTO payroll VALUES (1);")
            .unwrap();
        conn.pragma_update(None, "user_version", 4i64).unwrap();
    }
    let before = std::fs::read(&redirect_target).unwrap();

    // The file the caller actually named: a store c10r built, whose identity is distinguishable.
    let literal = dir.path().join("file:target.db");
    silent_cartographer::commands::build_from_index(
        &literal,
        "literal-store",
        dir.path(),
        &support::fixture_index(),
        &support::sources(),
    )
    .unwrap();

    let queried = c10r()
        .current_dir(dir.path())
        .args(["--json", "--db", "file:target.db", "status"])
        .output()
        .unwrap();
    assert_eq!(
        queried.status.code(),
        Some(0),
        "the literal store answers: {}",
        String::from_utf8_lossy(&queried.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&queried.stdout).expect("status emits JSON");
    assert_eq!(
        report["workspace"], "literal-store",
        "the answer came from the file the guard inspected, not a URI redirect: {report}"
    );
    assert_eq!(
        std::fs::read(&redirect_target).unwrap(),
        before,
        "the redirect target is untouched by the query"
    );

    // The same spelling on the write path: `cache` removes the literal store and nothing else.
    let removed = c10r()
        .current_dir(dir.path())
        .args(["--db", "file:target.db", "cache"])
        .output()
        .unwrap();
    assert_eq!(
        removed.status.code(),
        Some(0),
        "the literal store is removable: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!literal.exists(), "the literal store was removed");
    assert_eq!(
        std::fs::read(&redirect_target).unwrap(),
        before,
        "the redirect target survives the removal byte-identical"
    );
}

// _(Exit-code taxonomy: ownership refusal is distinct)_ — a valid SQLite file at the `--db` path
// that c10r did not create is refused with the unrecognized-store code, distinct from both the
// no-index code (nothing is there) and the incompatible-store code (rebuild would fix it) — the two
// it must not be confused with, since the remedy here is the opposite: do not build here at all.
#[test]
fn unrecognized_store_is_its_own_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("CREATE TABLE t (x)").unwrap();
    drop(conn);

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(6),
        "a file c10r did not create is the unrecognized-store code: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// _(Closed flag vocabulary: alias-rejection literal against the binary)_ — the banned `--format`
// alias, run against the actual built binary rather than only inferred from the closed clap-tree
// vocabulary walk, is rejected as a usage error.
#[test]
fn banned_format_alias_is_rejected_by_the_binary() {
    let out = c10r()
        .args(["--db", "/nonexistent/index.db", "--format=json", "find", "x"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "the banned --format alias is rejected as a usage error, not silently honored: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// _(Indexer/setup-failure class: an unresolvable Python environment)_ — a Python-manifested workspace
// with no resolvable interpreter environment (no `--environment`, no `$VIRTUAL_ENV`, no `.venv`/`venv`
// directory) is a setup failure — the same recovery class as an unavailable adapter — so it exits
// with the indexer/setup-failure code, not a generic failure.
#[test]
fn unresolvable_python_environment_is_the_indexer_setup_code() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let db = dir.path().join("index.db");

    let out = c10r()
        .current_dir(dir.path())
        .env_remove("VIRTUAL_ENV")
        .arg("--db")
        .arg(&db)
        .arg("build")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(5),
        "an unresolvable Python environment is the indexer/setup-failure code: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "the failed build created no index");
}

// _(Non-interactive operation)_ — a command invoked with standard input and output not attached to a
// terminal runs to completion without waiting for interactive input.
#[test]
fn runs_to_completion_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // stdin closed (null), stdout/stderr piped: a fully headless environment. `output()` returns only
    // if the process completes rather than blocking on a prompt.
    let out = c10r()
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();

    assert!(out.status.code().is_some(), "the headless invocation ran to completion");
}
