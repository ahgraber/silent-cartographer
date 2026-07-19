//! Output-bounding contract tests: the result-set cap with its truncation disclosure and opaque
//! continuation token, deterministic resume across pages, the per-result content cap with its
//! disclosure, and the usage-error rejection of a token presented against parameters or an index it
//! was not issued for.
//!
//! These drive the built `c10r` binary through `std::process::Command` so the bounding contract
//! (JSON disclosure shape, exit codes, diagnostics) is observed as a caller would. The continuation
//! token is treated as opaque throughout: it is only ever read from one answer and passed back
//! through `--cursor`.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use silent_cartographer::graph::store::{EdgeKind, GraphStore, PersistedClass, SymbolRow};
use silent_cartographer::identity::CanonicalId;

/// The single fixture source, paired with its workspace-relative path.
fn sources() -> Vec<(String, String)> {
    vec![(support::DOC.to_string(), support::SOURCE.to_string())]
}

/// Build the exemplar Rust fixture into a store at `dir/index.db` and return its path.
fn build_fixture_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    silent_cartographer::commands::build_from_index(&db, "bound-ws", &support::fixture_index(), &sources()).unwrap();
    db
}

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`), run in an empty
/// directory so freshness scanning stays cheap and deterministic.
fn c10r(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
    cmd.current_dir(dir);
    cmd
}

/// Parse a `--json` answer from stdout, asserting the invocation succeeded.
fn json_answer(out: &std::process::Output) -> serde_json::Value {
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("the --json answer parses")
}

/// The `canonical_id` of each result in a found answer, in serialized order.
fn result_ids(answer: &serde_json::Value) -> Vec<String> {
    answer["outcome"]["results"]
        .as_array()
        .expect("a found result set")
        .iter()
        .map(|r| {
            // A trace reference row nests its identity under `subject`; a symbol row flattens it.
            let id = r["canonical_id"].as_str().or(r["subject"]["canonical_id"].as_str());
            format!(
                "{}@{}",
                id.expect("a canonical_id"),
                // References share a subject; the span disambiguates rows.
                r["location"]["span_start"].as_u64().unwrap_or(0)
            )
        })
        .collect()
}

/// The full `trace references` query the paging tests slice into pages: the fixture holds several
/// reference sites of `net::Client`, in a deterministic order.
const PAGED_QUERY: [&str; 4] = ["trace", "net::Client", "--relation", "references"];

// _(Result set capped and truncation disclosed)_ — a query whose results exceed `--limit` returns at
// most that many results, and the answer discloses the truncation with a continuation token.
#[test]
fn limit_caps_the_result_set_and_discloses_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // The unbounded set (`--limit 0`) establishes how many results exist and carries no page block.
    let full = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--limit", "0"])
        .output()
        .unwrap();
    let full = json_answer(&full);
    let total = result_ids(&full).len();
    assert!(total >= 2, "the fixture yields several references: {total}");
    assert!(
        full.get("page").is_none(),
        "--limit 0 is unbounded: no paging disclosure"
    );

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--limit", "1"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(result_ids(&answer).len(), 1, "at most --limit results are returned");
    let page = &answer["page"];
    assert_eq!(page["truncated"], true, "the truncation is disclosed: {answer}");
    assert_eq!(page["total"].as_u64(), Some(total as u64), "the total is disclosed");
    assert!(
        page["cursor"].as_str().is_some_and(|c| !c.is_empty()),
        "a truncated answer carries the continuation token: {answer}"
    );
}

// _(Continuation resumes deterministically)_ — resuming with the token returns the next page exactly
// where the prior page ended, with no overlap and no gap, and re-issuing the same page is identical.
#[test]
fn cursor_resumes_the_next_page_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let full = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .output()
        .unwrap();
    let all_ids = result_ids(&json_answer(&full));
    assert!(all_ids.len() >= 3, "enough rows for two pages at limit 2: {all_ids:?}");

    let page = |cursor: Option<&str>| {
        let mut cmd = c10r(dir.path());
        cmd.arg("--db").arg(&db).arg("--json");
        cmd.args(PAGED_QUERY).args(["--limit", "2"]);
        if let Some(token) = cursor {
            cmd.args(["--cursor", token]);
        }
        cmd.output().unwrap()
    };

    let first = json_answer(&page(None));
    let token = first["page"]["cursor"]
        .as_str()
        .expect("page 1 is truncated")
        .to_string();
    let first_ids = result_ids(&first);

    let second = json_answer(&page(Some(&token)));
    let second_ids = result_ids(&second);

    // The two pages tile the full ordered set: page 2 starts exactly where page 1 ended.
    let stitched: Vec<String> = first_ids.iter().chain(second_ids.iter()).cloned().collect();
    assert_eq!(
        stitched, all_ids,
        "the pages resume with no overlap and no gap over the deterministic order"
    );

    // Determinism: re-issuing the same page yields byte-identical output.
    let again = page(Some(&token));
    let second_raw = page(Some(&token));
    assert_eq!(
        String::from_utf8_lossy(&again.stdout),
        String::from_utf8_lossy(&second_raw.stdout),
        "re-issuing the same page is identical"
    );
    assert_eq!(second_ids, result_ids(&json_answer(&again)));
}

// _(Content bounded with truncation disclosed: a `get` body)_ — a body exceeding `--max-lines` is
// capped to the first N lines and the per-result truncation is disclosed, with the line window in the
// JSON answer and named in the human render alike.
#[test]
fn max_lines_caps_a_get_body_and_discloses_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // `net::open`'s body spans five lines; a two-line cap keeps the first two and discloses the rest.
    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "net::open", "--detail", "body", "--max-lines", "2"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let result = &answer["outcome"]["results"][0];
    let body = result["payload"]["body"].as_str().expect("a body");
    assert_eq!(
        body.lines().count(),
        2,
        "the body is capped to the first two lines: {body:?}"
    );
    assert_eq!(
        result["content_truncated"], true,
        "the per-result truncation is disclosed: {answer}"
    );
    assert_eq!(
        result["content_lines"]["start"], 1,
        "the window starts at line 1: {answer}"
    );
    assert_eq!(result["content_lines"]["end"], 2, "the window ends at line 2: {answer}");
    let total = result["content_lines"]["total"].as_u64().expect("a total line count");
    assert!(total > 2, "the total exceeds the window: {answer}");

    // The human render of the same answer names the line window and the next fetch.
    let human = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::open", "--detail", "body", "--max-lines", "2"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(
        text.contains(&format!("lines 1-2 of {total}")) && text.contains("--from 3"),
        "the human render names the window and the next fetch: {text}"
    );

    // Content within the bound passes through whole, with no disclosure.
    let within = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "net::open", "--detail", "body", "--max-lines", "0"])
        .output()
        .unwrap();
    let within = json_answer(&within);
    assert!(
        within["outcome"]["results"][0].get("content_truncated").is_none(),
        "an untruncated result carries no disclosure: {within}"
    );
    assert!(
        within["outcome"]["results"][0].get("content_lines").is_none(),
        "the whole content carries no line-window block: {within}"
    );
}

// _(Content bounded with truncation disclosed: a `trace` content row)_ — the cap holds on the trace
// path too: a reference row's projected content is capped to the first N lines with the row's own
// disclosure, independently of `--limit`.
#[test]
fn max_lines_caps_a_trace_content_row_and_discloses_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // `--limit 0` keeps the answer unbounded so the content cap is observed without any page block.
    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--detail", "body", "--max-lines", "1", "--limit", "0"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let results = answer["outcome"]["results"].as_array().expect("found rows");
    let capped: Vec<&serde_json::Value> = results.iter().filter(|r| r["content"].as_str().is_some()).collect();
    assert!(!capped.is_empty(), "some rows project content: {answer}");
    for row in &capped {
        let content = row["content"].as_str().unwrap();
        assert!(
            content.split('\n').count() <= 1,
            "every content row is capped to the first line: {content:?}"
        );
    }
    // At least one enclosing declaration's body spans more than one line, so its row is truncated.
    assert!(
        capped.iter().any(|r| r["content_truncated"] == true),
        "a multi-line enclosing body's row discloses its truncation: {answer}"
    );
    // `--limit 0`: the content cap applied without any result-set paging.
    assert!(answer.get("page").is_none(), "--max-lines is independent of --limit");
}

// _(Mismatched continuation token refused: different parameters)_ — a token issued for one query is
// rejected as a usage error when presented with different query parameters, naming the recovery.
#[test]
fn token_with_different_parameters_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let first = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--limit", "1"])
        .output()
        .unwrap();
    let token = json_answer(&first)["page"]["cursor"]
        .as_str()
        .expect("a continuation token")
        .to_string();

    // Same token, different subject and relation: refused, not silently mis-paged.
    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "trace",
            "net::Client",
            "--relation",
            "contains",
            "--limit",
            "1",
            "--cursor",
            &token,
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "a mismatched token is a usage error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("re-issue the query without --cursor"),
        "the rejection names the recovery: {stderr}"
    );

    // Same query except the projection detail: still a different identity, still refused — the token
    // binds every parameter, not just the subject and relation.
    let detail_varied = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(PAGED_QUERY)
        .args(["--detail", "signature", "--limit", "1", "--cursor", &token])
        .output()
        .unwrap();
    assert_eq!(
        detail_varied.status.code(),
        Some(2),
        "a token presented at a different --detail is a usage error"
    );
    assert!(
        String::from_utf8_lossy(&detail_varied.stderr).contains("re-issue the query without --cursor"),
        "the rejection names the recovery"
    );

    // A cursor against an unbounded set (`--limit 0`) has no page to resume: a usage error naming the
    // requirement to re-issue with a positive limit.
    let unbounded = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(PAGED_QUERY)
        .args(["--limit", "0", "--cursor", &token])
        .output()
        .unwrap();
    assert_eq!(
        unbounded.status.code(),
        Some(2),
        "--cursor with --limit 0 (unbounded) is a usage error"
    );
    assert!(
        String::from_utf8_lossy(&unbounded.stderr).contains("--limit"),
        "the rejection names the positive-limit requirement"
    );
}

// _(Mismatched continuation token refused: rebuilt index)_ — a token issued before the index was
// rebuilt over changed sources is rejected as a usage error rather than resumed against shifted
// results.
#[test]
fn token_after_an_index_rebuild_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let first = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--limit", "1"])
        .output()
        .unwrap();
    let token = json_answer(&first)["page"]["cursor"]
        .as_str()
        .expect("a continuation token")
        .to_string();

    // Rebuild the index over changed source content: the recorded content hash shifts.
    let changed = vec![(support::DOC.to_string(), format!("{}\n// changed\n", support::SOURCE))];
    silent_cartographer::commands::build_from_index(&db, "bound-ws", &support::fixture_index(), &changed).unwrap();

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(PAGED_QUERY)
        .args(["--limit", "1", "--cursor", &token])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "a token from before the rebuild is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("re-issue the query without --cursor"),
        "the rejection names the recovery"
    );
}

// _(Mismatched continuation token refused: rebuilt under a different analyzer)_ — a token binds to
// the recorded analyzer provenance as well as the source content-hash, so a rebuild under a different
// analyzer version — where extraction, and thus result order, may shift even over identical sources —
// invalidates the token even though the content-hash is unchanged.
#[test]
fn token_after_an_analyzer_version_change_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let first = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(PAGED_QUERY)
        .args(["--limit", "1"])
        .output()
        .unwrap();
    let token = json_answer(&first)["page"]["cursor"]
        .as_str()
        .expect("a continuation token")
        .to_string();

    // Rewrite the recorded metadata with a different analyzer version, leaving the content-hash
    // untouched — the same source, a different extractor build.
    {
        let store = GraphStore::open(&db).unwrap();
        let mut meta = store.read_metadata().unwrap().expect("the fixture recorded metadata");
        meta.provenance.analyzer_version = format!("{}-changed", meta.provenance.analyzer_version);
        store.write_metadata(&meta).unwrap();
    }

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(PAGED_QUERY)
        .args(["--limit", "1", "--cursor", &token])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "a token from before the analyzer-version change is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("re-issue the query without --cursor"),
        "the rejection names the recovery"
    );
}

// _(Mismatched continuation token refused: tampered token)_ — a cursor that is not a well-formed
// token at all — including multibyte text whose byte length looks plausible — is a usage error
// naming the recovery, never a crash.
#[test]
fn malformed_cursor_is_a_usage_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // Multibyte text (non-ASCII, even byte length), plain garbage, odd-length hex, and hex that
    // decodes to no `hash:page` shape.
    for bogus in ["😀😀", "not-a-token", "abc", "deadbeef"] {
        let out = c10r(dir.path())
            .arg("--db")
            .arg(&db)
            .args(PAGED_QUERY)
            .args(["--limit", "1", "--cursor", bogus])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "the malformed cursor {bogus:?} is a usage error, not a crash: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("re-issue the query without --cursor"),
            "the rejection names the recovery for {bogus:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Bounding defaults and windowing
// ---------------------------------------------------------------------------

/// A symbol identity under the fixture workspace, for directly-built stores.
fn ws_id(name: &str) -> CanonicalId {
    CanonicalId::from_raw(format!("bound-ws::{name}"))
}

/// Insert a bare in-workspace symbol carrying `body` as its body/signature/interface tiers.
fn put_symbol(store: &GraphStore, id: &str, display: &str, body: Option<&str>) {
    store
        .insert_symbol(&SymbolRow {
            canonical_id: ws_id(id),
            display_name: display.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("m.rs".to_string()),
            span: Some((0, 1)),
            span_text: body.map(str::to_string),
            signature_text: body.map(str::to_string),
            interface_text: body.map(str::to_string),
            duplicated: false,
        })
        .unwrap();
}

/// A body of `n` lines, one `lineN` per line.
fn lines_body(n: usize) -> String {
    (1..=n).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n")
}

/// A store at `dir/index.db` holding `count` symbols named `widgetNN`, for result-set bounding.
fn build_widget_db(dir: &Path, count: usize) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    for i in 0..count {
        let name = format!("widget{i:02}");
        put_symbol(&store, &name, &name, None);
    }
    support::stamp_metadata(&store, "bound-ws");
    db
}

// _(Default bounding: a documented default bound applies with no limit requested)_ — a result set
// larger than the default limit is bounded to it, the truncation and total disclosed, and the
// continuation token resumes the remainder.
#[test]
fn default_limit_bounds_a_large_result_set_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_widget_db(dir.path(), 30);

    // No `--limit`: the default of 25 bounds the 30-symbol result set.
    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "widget"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(
        answer["outcome"]["results"].as_array().unwrap().len(),
        25,
        "the default limit caps the set at 25: {answer}"
    );
    assert_eq!(
        answer["page"]["truncated"], true,
        "the truncation is disclosed: {answer}"
    );
    assert_eq!(answer["page"]["total"], 30, "the total is disclosed: {answer}");
    let token = answer["page"]["cursor"]
        .as_str()
        .expect("a truncated default-bounded answer carries a cursor")
        .to_string();

    // Resuming (the default limit still in force) returns the remaining five.
    let next = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "widget", "--cursor", &token])
        .output()
        .unwrap();
    let next = json_answer(&next);
    assert_eq!(
        next["outcome"]["results"].as_array().unwrap().len(),
        5,
        "the second page holds the remaining five: {next}"
    );
}

// _(Explicit unbounded request is honored)_ — `--limit 0` returns the whole set with no page block.
#[test]
fn limit_zero_returns_the_whole_set_with_no_page_block() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_widget_db(dir.path(), 30);

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "widget", "--limit", "0"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(
        answer["outcome"]["results"].as_array().unwrap().len(),
        30,
        "--limit 0 returns every result: {answer}"
    );
    assert!(
        answer.get("page").is_none(),
        "an unbounded set carries no page block: {answer}"
    );
}

// _(Bounded and resumable answers: page block suppressed when a positive limit is not exhausted)_ —
// a `--limit` larger than the result set returns the whole set on the first page, carrying no `page`
// block: the same suppression the unbounded (`--limit 0`) case gets, but reached through a positive,
// non-exhausting limit rather than the explicit-unbounded early return.
#[test]
fn page_block_is_suppressed_when_a_positive_limit_is_not_exhausted() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_widget_db(dir.path(), 10);

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "find", "widget", "--limit", "50"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(
        answer["outcome"]["results"].as_array().unwrap().len(),
        10,
        "every result is returned: {answer}"
    );
    assert!(
        answer.get("page").is_none(),
        "a positive limit that the set fits within carries no page block: {answer}"
    );
}

// _(Per-command content default: `get` body caps at 100 lines)_ — a `get` body longer than the
// per-command default is capped to the first 100 lines and the window disclosed, with no flag typed.
#[test]
fn get_body_default_max_lines_caps_at_one_hundred() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = GraphStore::open(&db).unwrap();
        put_symbol(&store, "bigfn", "bigfn", Some(&lines_body(150)));
        support::stamp_metadata(&store, "bound-ws");
    }

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "bound-ws::bigfn", "--detail", "body"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let result = &answer["outcome"]["results"][0];
    let body = result["payload"]["body"].as_str().expect("a body");
    assert_eq!(
        body.lines().count(),
        100,
        "the default get bound is 100 lines: {}",
        body.lines().count()
    );
    assert_eq!(
        result["content_truncated"], true,
        "the truncation is disclosed: {answer}"
    );
    assert_eq!(
        result["content_lines"]["end"], 100,
        "the window ends at line 100: {answer}"
    );
    assert_eq!(
        result["content_lines"]["total"], 150,
        "the full line count is disclosed: {answer}"
    );
}

// _(Per-command content default: a `trace` row caps at 10 lines)_ — a trace row's projected content
// is capped to the first 10 lines by the per-command default, with no flag typed. `dependents` shares
// this default, since it runs through the `trace` command.
#[test]
fn trace_row_default_max_lines_caps_at_ten() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = GraphStore::open(&db).unwrap();
        put_symbol(&store, "container", "container", None);
        put_symbol(&store, "member", "member", Some(&lines_body(20)));
        store
            .insert_edge(EdgeKind::Contains, &ws_id("container"), &ws_id("member"))
            .unwrap();
        support::stamp_metadata(&store, "bound-ws");
    }

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "--json",
            "trace",
            "bound-ws::container",
            "--relation",
            "contains",
            "--detail",
            "body",
        ])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let row = &answer["outcome"]["results"][0];
    let content = row["content"].as_str().expect("the member row projects its body");
    assert_eq!(
        content.lines().count(),
        10,
        "the default trace row bound is 10 lines: {content:?}"
    );
    assert_eq!(
        row["content_truncated"], true,
        "the row's truncation is disclosed: {answer}"
    );
}

// _(Windowed content access on `get`)_ — `--from` selects a line window with its position and total
// disclosed; a window past the end returns empty content naming the total, not an error.
#[test]
fn from_windows_get_content_and_handles_past_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = GraphStore::open(&db).unwrap();
        put_symbol(&store, "win", "win", Some(&lines_body(5)));
        support::stamp_metadata(&store, "bound-ws");
    }

    // A middle window: two lines from line 2.
    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "--json",
            "get",
            "bound-ws::win",
            "--detail",
            "body",
            "--from",
            "2",
            "--max-lines",
            "2",
        ])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let result = &answer["outcome"]["results"][0];
    assert_eq!(
        result["payload"]["body"], "line2\nline3",
        "the window is lines 2-3: {answer}"
    );
    assert_eq!(
        result["content_lines"]["start"], 2,
        "the window start is disclosed: {answer}"
    );
    assert_eq!(
        result["content_lines"]["end"], 3,
        "the window end is disclosed: {answer}"
    );
    assert_eq!(result["content_lines"]["total"], 5, "the total is disclosed: {answer}");
    assert_eq!(
        result["content_truncated"], true,
        "a partial window is truncated: {answer}"
    );

    // The human render names the next window fragment.
    let human = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "get",
            "bound-ws::win",
            "--detail",
            "body",
            "--from",
            "2",
            "--max-lines",
            "2",
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("--from 4"),
        "the human render names the next window: {}",
        String::from_utf8_lossy(&human.stdout)
    );

    // Past the end: empty content, disclosure names the total; a data-dependent outcome, not an error.
    let past = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "bound-ws::win", "--detail", "body", "--from", "9"])
        .output()
        .unwrap();
    let past = json_answer(&past);
    let past_result = &past["outcome"]["results"][0];
    assert_eq!(
        past_result["payload"]["body"], "",
        "a window past the end is empty content: {past}"
    );
    assert_eq!(
        past_result["content_lines"]["total"], 5,
        "the total line count is disclosed: {past}"
    );
    assert_eq!(
        past_result["content_truncated"], true,
        "empty content does not cover the whole text"
    );
}

// _(Modal teaching errors)_ — explicitly passing a content bound (`--max-lines`/`--from`) when no
// content-bearing detail is in play is a usage error naming the accepting details, on `get` and
// `trace` alike; a defaulted value stays dormant.
#[test]
fn explicit_content_bounds_on_non_content_details_are_modal_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    // Each of these types a content bound where the detail carries no content.
    let cases: [(&[&str], &str); 4] = [
        (&["get", "net::Client", "--max-lines", "3"], "--max-lines"),
        (&["get", "net::Client", "--from", "2"], "--from"),
        (
            &["get", "net::Client", "--detail", "location", "--max-lines", "3"],
            "--max-lines",
        ),
        (
            &["trace", "net::Client", "--relation", "contains", "--max-lines", "3"],
            "--max-lines",
        ),
    ];
    for (args, flag) in cases {
        let out = c10r(dir.path()).arg("--db").arg(&db).args(args).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "an explicit {flag} without a content detail is a usage error: {:?} → {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(flag) && stderr.contains("signature"),
            "the modal error names {flag} and the accepting details: {stderr}"
        );
    }

    // A defaulted bound stays dormant: a plain `get` at the location default is not an error.
    let dormant = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::Client"])
        .output()
        .unwrap();
    assert_eq!(
        dormant.status.code(),
        Some(0),
        "a defaulted content bound raises no error"
    );
}

// _(Ambiguous-candidate cap)_ — an ambiguous reference's candidate list is capped at the effective
// limit and the total disclosed; a candidate set is a refusal, so it carries no cursor.
#[test]
fn ambiguous_candidates_capped_with_disclosed_remainder() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = GraphStore::open(&db).unwrap();
        // Four distinct symbols sharing the shortname `dup`.
        for i in 0..4 {
            put_symbol(&store, &format!("dup{i}"), "dup", None);
        }
        support::stamp_metadata(&store, "bound-ws");
    }

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "dup", "--limit", "2"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(
        answer["outcome"]["outcome"], "ambiguous",
        "a shared shortname is ambiguous: {answer}"
    );
    assert_eq!(
        answer["outcome"]["candidates"].as_array().unwrap().len(),
        2,
        "the candidate list is capped at the effective limit: {answer}"
    );
    assert_eq!(
        answer["outcome"]["candidates_total"], 4,
        "the total candidate count is disclosed when capped: {answer}"
    );
    assert!(
        answer.get("page").is_none(),
        "a refusal carries no page/cursor: {answer}"
    );

    // The human render names the withheld remainder.
    let human = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "dup", "--limit", "2"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&human.stdout).contains("and 2 more — narrow the reference"),
        "the human render names the withheld candidates: {}",
        String::from_utf8_lossy(&human.stdout)
    );
}

/// A store at `dir/index.db` for dependents paging: a subject with `direct` direct dependents (each a
/// `uses` edge into the subject) plus one dependent two hops out (it uses `dep00`), so a default
/// depth-1 impact answer holds `direct` detailed rows and a nonempty beyond-bound aggregate.
fn build_dependents_db(dir: &Path, direct: usize) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    put_symbol(&store, "sub", "sub", None);
    for i in 0..direct {
        let name = format!("dep{i:02}");
        // A multi-line body on every direct dependent, so a content-bearing detail has a tail to cap.
        put_symbol(&store, &name, &name, Some(&lines_body(20)));
        store.insert_edge(EdgeKind::Uses, &ws_id(&name), &ws_id("sub")).unwrap();
    }
    put_symbol(&store, "deep", "deep", None);
    store
        .insert_edge(EdgeKind::Uses, &ws_id("deep"), &ws_id("dep00"))
        .unwrap();
    support::stamp_metadata(&store, "bound-ws");
    db
}

/// The detailed dependent identities of a dependents answer, in serialized order.
fn dependent_ids(answer: &serde_json::Value) -> Vec<String> {
    answer["outcome"]["results"][0]["detail"]
        .as_array()
        .expect("a dependents report with detail rows")
        .iter()
        .map(|r| {
            r["symbol"]["canonical_id"]
                .as_str()
                .expect("a canonical_id")
                .to_string()
        })
        .collect()
}

// _(Dependents detail rows are bounded and resumable)_ — a subject whose direct dependents exceed the
// result limit details at most the limit's rows, discloses the truncation with a continuation token,
// and repeats the beyond-bound aggregate and horizon disclosure on every page.
#[test]
fn dependents_detail_rows_are_paged_and_the_summary_repeats_per_page() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_dependents_db(dir.path(), 30);

    let page = |cursor: Option<&str>| {
        let mut cmd = c10r(dir.path());
        cmd.arg("--db").arg(&db).arg("--json");
        cmd.args(["trace", "bound-ws::sub", "--relation", "dependents", "--limit", "5"]);
        if let Some(token) = cursor {
            cmd.args(["--cursor", token]);
        }
        cmd.output().unwrap()
    };

    let first = json_answer(&page(None));
    let report = &first["outcome"]["results"][0];
    let first_ids = dependent_ids(&first);
    assert_eq!(
        first_ids.len(),
        5,
        "the first page details at most --limit rows: {first}"
    );
    assert_eq!(first["page"]["truncated"], true, "the truncation is disclosed: {first}");
    assert_eq!(first["page"]["total"], 30, "the detail-row total is disclosed: {first}");
    // The summary metadata is context, present on the first page alongside the rows.
    assert_eq!(
        report["disclosure"], "beyond_bound",
        "the horizon disclosure accompanies the page"
    );
    assert!(
        !report["beyond_bound"].as_array().unwrap().is_empty(),
        "the beyond-bound aggregate accompanies the page: {first}"
    );
    let token = first["page"]["cursor"]
        .as_str()
        .expect("a cursor on a truncated page")
        .to_string();

    let second = json_answer(&page(Some(&token)));
    let second_report = &second["outcome"]["results"][0];
    let second_ids = dependent_ids(&second);
    assert_eq!(
        second["page"]["page_index"], 1,
        "the cursor resumes the second page: {second}"
    );
    // The rows resume with no overlap; the summary is repeated on the resumed page.
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "the second page's rows do not overlap the first: {first_ids:?} / {second_ids:?}"
    );
    assert_eq!(
        second_report["disclosure"], "beyond_bound",
        "the horizon disclosure is repeated on page two: {second}"
    );
    assert!(
        !second_report["beyond_bound"].as_array().unwrap().is_empty(),
        "the beyond-bound aggregate is repeated on page two: {second}"
    );
}

// _(Dependents detail rows are bounded by default)_ — with no explicit limit, the default caps the
// detailed rows and discloses the truncation, exactly as a row-bearing answer does.
#[test]
fn default_limit_binds_dependents_detail_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_dependents_db(dir.path(), 30);

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "trace", "bound-ws::sub", "--relation", "dependents"])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    assert_eq!(
        dependent_ids(&answer).len(),
        25,
        "the default limit of 25 caps the detailed dependents: {answer}"
    );
    assert_eq!(
        answer["page"]["truncated"], true,
        "the truncation is disclosed: {answer}"
    );
    assert_eq!(
        answer["page"]["total"], 30,
        "the detail-row total is disclosed: {answer}"
    );
}

// _(Bounded and resumable answers: dependents content cap)_ — a `dependents` detail row's projected
// content is capped by `--max-lines` exactly as any other content-bearing row, with the truncation
// disclosed on the row itself.
#[test]
fn dependents_detail_row_content_is_capped_and_disclosed() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_dependents_db(dir.path(), 3);

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args([
            "trace",
            "bound-ws::sub",
            "--relation",
            "dependents",
            "--detail",
            "body",
            "--max-lines",
            "2",
        ])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let report = &answer["outcome"]["results"][0];
    let detail = report["detail"].as_array().expect("dependents detail rows");
    assert!(!detail.is_empty(), "the direct dependents are detailed: {answer}");
    for row in detail {
        let content = row["content"].as_str().expect("the row projects its content");
        assert_eq!(
            content.lines().count(),
            2,
            "the row's content is capped to --max-lines: {content:?}"
        );
        assert_eq!(
            row["content_truncated"], true,
            "the row's truncation is disclosed: {row}"
        );
    }
}

// _(Windowed content arithmetic does not overflow)_ — a `--max-lines` near `usize::MAX` combined with
// a `--from` past line 1 must not overflow the window's end computation: the window simply runs to the
// end of the content, exit 0.
#[test]
fn a_huge_max_lines_with_a_nonzero_from_does_not_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    {
        let store = GraphStore::open(&db).unwrap();
        put_symbol(&store, "win", "win", Some(&lines_body(5)));
        support::stamp_metadata(&store, "bound-ws");
    }

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "--json",
            "get",
            "bound-ws::win",
            "--detail",
            "body",
            "--from",
            "2",
            "--max-lines",
            "18446744073709551615",
        ])
        .output()
        .unwrap();
    let answer = json_answer(&out);
    let result = &answer["outcome"]["results"][0];
    assert_eq!(
        result["payload"]["body"], "line2\nline3\nline4\nline5",
        "the window runs from line 2 to the end without overflowing: {answer}"
    );
    assert_eq!(
        result["content_lines"]["end"], 5,
        "the window reaches the last line: {answer}"
    );
}

// _(Ranged `--from` parser)_ — `--from 0` is rejected at parse time as a usage error naming the
// 1-based constraint, rather than silently coerced to line 1.
#[test]
fn from_zero_is_a_parse_time_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::open", "--detail", "body", "--from", "0"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "--from 0 is a usage error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("0") && (stderr.contains("1..") || stderr.contains("not in")),
        "the diagnostic names the 1-based constraint: {stderr}"
    );
}

// _(Content bound is query-command-local: `find` carries no content)_ — `find` defines no
// `--max-lines`, so passing it is an unknown-argument usage error.
#[test]
fn find_rejects_max_lines_as_an_unknown_argument() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["find", "conn", "--max-lines", "5"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "find has no --max-lines, so it is an unknown argument: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Write an executable stub named `name` into `dir` that hangs on any invocation, standing in for a
/// wedged indexer on `PATH`.
#[cfg(unix)]
fn write_hanging_stub(dir: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    // A standard PATH inside the stub so `sleep` resolves even though the probe runs it under the
    // test's restricted PATH (which holds only the stub itself).
    std::fs::write(&path, "#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec sleep 300\n").unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
}

// _(Query freshness probe is bounded and its timeout disclosed)_ — a `get` against a built index with a
// hanging `rust-analyzer` on `PATH` still answers: the freshness/version probe times out, so the
// answer is served from recorded provenance on standard output while a timeout warning is disclosed on
// standard error — all within a bounded time, never an indefinite hang.
#[cfg(unix)]
#[test]
fn get_answers_despite_a_hanging_analyzer_and_discloses_the_timeout() {
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());
    // A wedged rust-analyzer resolved by the query's freshness probe.
    let bin_dir = tempfile::tempdir().unwrap();
    write_hanging_stub(bin_dir.path(), "rust-analyzer");

    let start = Instant::now();
    let out = c10r(dir.path())
        .env("PATH", bin_dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "net::open"])
        .output()
        .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(60),
        "the freshness probe is bounded, not waited on indefinitely: {elapsed:?}"
    );

    assert_eq!(
        out.status.code(),
        Some(0),
        "the query still answers from recorded state; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    assert!(
        answer["outcome"]["results"].as_array().is_some_and(|r| !r.is_empty()),
        "the answer is intact on standard output: {answer}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("rust-analyzer") && stderr.to_lowercase().contains("timed out"),
        "the timeout is disclosed on standard error: {stderr}"
    );
}
