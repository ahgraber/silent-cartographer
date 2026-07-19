//! Human-render contract tests: the default rendering is a faithful projection of the same answer the
//! `--json` path serializes — same results in the same order — content-bearing tiers render as
//! multi-line source text, and no styling ever reaches a redirected stream or the machine answer.
//!
//! These drive the built `c10r` binary so the rendered bytes are observed exactly as a caller sees
//! them (test processes are not attached to a terminal, so the default `--color=auto` renders plain).

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use silent_cartographer::graph::store::{EdgeKind, GraphStore, OccurrenceRow, PersistedClass, SymbolRow};
use silent_cartographer::identity::CanonicalId;

/// The single fixture source, paired with its workspace-relative path.
fn sources() -> Vec<(String, String)> {
    vec![(support::DOC.to_string(), support::SOURCE.to_string())]
}

/// Build the exemplar Rust fixture into a store at `dir/index.db` and return its path.
fn build_fixture_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    silent_cartographer::commands::build_from_index(&db, "render-ws", &support::fixture_index(), &sources()).unwrap();
    db
}

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`), run in an empty
/// directory so freshness scanning stays cheap and deterministic.
fn c10r(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_c10r"));
    cmd.current_dir(dir);
    cmd
}

// _(Source-faithful content rendering: body renders as source)_ — a multi-line body is shown as
// multi-line source text, not an escaped single-line scalar.
#[test]
fn multi_line_body_renders_as_multi_line_source() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::open", "--detail", "body"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    // The body's own source lines appear verbatim, on separate physical lines.
    assert!(
        stdout.contains("pub fn open() -> Client {"),
        "the body opening line: {stdout}"
    );
    assert!(
        stdout.contains("        let c = Client;"),
        "an interior body line: {stdout}"
    );
    // Not a Debug/JSON-escaped single-line scalar: no literal backslash-n sequence stands in for the
    // real newlines, and the body genuinely spans several physical lines.
    assert!(
        !stdout.contains("\\n"),
        "the body is not escaped into one line: {stdout}"
    );
    assert!(
        stdout.lines().count() >= 5,
        "the body renders across multiple physical lines: {stdout}"
    );
}

/// The `canonical_id` of each result in a `--json` answer, in serialized order.
fn json_result_ids(stdout: &[u8]) -> Vec<String> {
    let answer: serde_json::Value = serde_json::from_slice(stdout).expect("the --json answer parses");
    answer["outcome"]["results"]
        .as_array()
        .expect("a found result set")
        .iter()
        .map(|r| r["canonical_id"].as_str().expect("a canonical_id").to_string())
        .collect()
}

// _(Output stream discipline: human render matches the JSON answer)_ — the same query rendered with
// `--json` and with the default human render presents the same results in the same order.
#[test]
fn human_and_json_present_the_same_results_in_the_same_order() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let query = ["trace", "net::Client", "--relation", "contains"];

    let json = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("--json")
        .args(query)
        .output()
        .unwrap();
    let human = c10r(dir.path()).arg("--db").arg(&db).args(query).output().unwrap();

    let ids = json_result_ids(&json.stdout);
    assert!(ids.len() >= 2, "the query returns several results: {ids:?}");

    let human_text = String::from_utf8_lossy(&human.stdout);
    // Each identity appears in the human render, and their first appearances follow the JSON order.
    let mut last = 0usize;
    for id in &ids {
        let at = human_text
            .find(id.as_str())
            .unwrap_or_else(|| panic!("human render is missing `{id}`: {human_text}"));
        assert!(
            at >= last,
            "human render presents `{id}` out of the JSON order: {human_text}"
        );
        last = at;
    }
}

// _(Source-faithful content rendering: no styling to a redirected stream)_ — piped (non-terminal)
// output carries no color/styling control sequences under the default `--color=auto`.
#[test]
fn piped_output_carries_no_styling() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "net::open", "--detail", "body"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "piped output carries no ANSI escape sequences: {stdout:?}"
    );
}

// _(Source-faithful content rendering: styling is structural only)_ — under `--color=always`, which
// forces styling even piped, ANSI escapes land on structural lines (the header, the symbol identity
// line) and never inside the tier/source text: the source block always stays plain.
#[test]
fn forced_styling_marks_structure_but_never_source_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--color", "always", "get", "net::open", "--detail", "body"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    let styled_lines: Vec<&str> = stdout.lines().filter(|l| l.contains('\u{1b}')).collect();
    assert!(
        !styled_lines.is_empty(),
        "--color=always styles the structural lines even when piped: {stdout:?}"
    );
    // Every styled line is structural — the header or the symbol identity line — never source text.
    for line in &styled_lines {
        assert!(
            line.contains("rust-analyzer") || line.contains("net::open"),
            "only structural lines carry styling: {line:?}"
        );
    }
    // The body's own source lines are present and carry no escapes.
    for source_line in ["pub fn open() -> Client {", "        let c = Client;"] {
        let line = stdout
            .lines()
            .find(|l| l.contains(source_line))
            .unwrap_or_else(|| panic!("the body line {source_line:?} renders: {stdout}"));
        assert!(!line.contains('\u{1b}'), "source text carries no styling: {line:?}");
    }
}

// _(Source-faithful content rendering: no styling under JSON)_ — the machine answer carries no color
// or styling control sequences.
#[test]
fn json_output_carries_no_styling() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_fixture_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", "net::open", "--detail", "body"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "the --json answer carries no ANSI escape sequences: {stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// Source-faithful content rendering — the sanitization clause: structural fields render safe,
// content and JSON render verbatim.
// ---------------------------------------------------------------------------

/// The hostile symbol's clean canonical identity — used to address it directly, so a test does not
/// depend on passing raw control bytes through argv.
const HOSTILE_ID: &str = "hostile-ws::evil";
/// The clean identity of the symbol that `contains` the hostile one, for the `trace` sanitization test.
const CONTAINER_ID: &str = "hostile-ws::container";
/// A `display_name` carrying the terminal-control injection payloads: an ESC-introduced ANSI escape
/// sequence, a C1 CSI (U+009B — a one-character control-sequence introducer that C1-honoring
/// terminals execute with no ESC byte), and a bidirectional override (U+202E, the Trojan-Source
/// display-spoofing class).
const HOSTILE_NAME: &str = "evil\x1b[31m\u{9b}31m\u{202e}name";
/// A `document_path` carrying an ANSI escape sequence and a raw newline — the newline is the row-
/// forging payload a structural field must never let through, since a row-bearing answer is one line
/// per result.
const HOSTILE_DOC: &str = "src/evil\x1b[31m\ninjected.rs";
/// Body/signature/interface tier text carrying an ANSI escape, a C1 CSI, and a bidirectional
/// override alongside a raw newline, to prove content sanitization neutralizes all three
/// terminal-control classes while the source text's own line break survives.
const HOSTILE_BODY: &str = "line one\x1b[31m\u{9b}31m\u{202e}\nline two";

/// A store at `dir/index.db` holding one in-workspace symbol whose `display_name`, `document_path`,
/// and tier text all carry terminal-control bytes, inserted directly through the store API (bypassing
/// ingest/join, which is irrelevant to rendering), plus a clean container symbol that `contains` it —
/// so a `trace --relation contains` row projects the hostile fields too.
fn build_hostile_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: CanonicalId::from_raw(HOSTILE_ID.to_string()),
            display_name: HOSTILE_NAME.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some(HOSTILE_DOC.to_string()),
            span: Some((0, 4)),
            span_text: Some(HOSTILE_BODY.to_string()),
            signature_text: Some(HOSTILE_BODY.to_string()),
            interface_text: Some(HOSTILE_BODY.to_string()),
            duplicated: false,
        })
        .unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: CanonicalId::from_raw(CONTAINER_ID.to_string()),
            display_name: "container".to_string(),
            kind: "type".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("src/container.rs".to_string()),
            span: Some((0, 1)),
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
        })
        .unwrap();
    store
        .insert_edge(
            EdgeKind::Contains,
            &CanonicalId::from_raw(CONTAINER_ID.to_string()),
            &CanonicalId::from_raw(HOSTILE_ID.to_string()),
        )
        .unwrap();
    // A reference site of the hostile symbol itself, attributed to the hostile symbol's own
    // declaration — so a `trace --relation references` row projects both the hostile structural
    // fields (subject identity, location) and, at a content-bearing detail, the hostile tier text.
    store
        .insert_occurrence(&OccurrenceRow {
            symbol_id: CanonicalId::from_raw(HOSTILE_ID.to_string()),
            document_path: HOSTILE_DOC.to_string(),
            span: (0, 4),
            role: "reference".to_string(),
            rule: "exact".to_string(),
            enclosing_id: Some(CanonicalId::from_raw(HOSTILE_ID.to_string())),
            locality: None,
        })
        .unwrap();
    support::stamp_metadata(&store, "hostile-ws");
    db
}

// _(Source-faithful content rendering: `get` structural output)_ — a `get` at `location` detail (header
// plus identity plus location line — no content) renders no ANSI escape and no forged extra row: the
// hostile display name's escape and the hostile document path's raw newline are both replaced.
#[test]
fn get_sanitizes_hostile_name_and_path_in_structural_output() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", HOSTILE_ID, "--detail", "location"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "no ANSI escape reaches structural output: {stdout:?}"
    );
    assert!(
        !stdout.contains('\u{9b}'),
        "no C1 CSI reaches structural output: {stdout:?}"
    );
    assert!(
        !stdout.contains('\u{202e}'),
        "no bidirectional override reaches structural output: {stdout:?}"
    );
    // header line + identity line + location line: the raw newline in the hostile path did not forge
    // a fourth line.
    assert_eq!(
        stdout.lines().count(),
        3,
        "the embedded newline forges no extra row: {stdout:?}"
    );
}

// _(Source-faithful content rendering: `find` and `trace` row output)_ — a row-bearing answer over the
// hostile symbol renders no ANSI escape and no forged row in either command.
#[test]
fn find_and_trace_sanitize_hostile_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_db(dir.path());

    let find_out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["find", "evil"])
        .output()
        .unwrap();
    assert_eq!(find_out.status.code(), Some(0));
    let find_stdout = String::from_utf8_lossy(&find_out.stdout);
    assert!(
        !find_stdout.contains('\u{1b}'),
        "find carries no ANSI escape: {find_stdout:?}"
    );
    assert!(
        !find_stdout.contains('\u{9b}') && !find_stdout.contains('\u{202e}'),
        "find carries no C1 CSI and no bidirectional override: {find_stdout:?}"
    );
    // header line + "N results" line + one row: the embedded newline forges no extra row.
    assert_eq!(find_stdout.lines().count(), 3, "no forged row: {find_stdout:?}");

    let trace_out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["trace", CONTAINER_ID, "--relation", "contains"])
        .output()
        .unwrap();
    assert_eq!(
        trace_out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&trace_out.stderr)
    );
    let trace_stdout = String::from_utf8_lossy(&trace_out.stdout);
    assert!(
        !trace_stdout.contains('\u{1b}'),
        "trace carries no ANSI escape: {trace_stdout:?}"
    );
    assert!(
        !trace_stdout.contains('\u{9b}') && !trace_stdout.contains('\u{202e}'),
        "trace carries no C1 CSI and no bidirectional override: {trace_stdout:?}"
    );
    // header line + "N results" line + one row (with its `at <path:span>` location suffix): the
    // embedded newline in the hostile document path forges no extra row.
    assert_eq!(trace_stdout.lines().count(), 3, "no forged row: {trace_stdout:?}");
    assert!(
        trace_stdout.contains(" at "),
        "the symbol row still names its (sanitized) location: {trace_stdout:?}"
    );
}

// _(Source-faithful content rendering: `--json` stays byte-exact)_ — the machine answer is exempt: the
// hostile name and path round-trip exactly through JSON's own escaping, never lossily substituted.
#[test]
fn json_output_preserves_the_hostile_name_and_path_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", HOSTILE_ID, "--detail", "location"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("the --json answer parses");
    let result = &answer["outcome"]["results"][0];
    assert_eq!(
        result["symbol"]["name"], HOSTILE_NAME,
        "the hostile display name round-trips byte-exactly: {result}"
    );
    assert_eq!(
        result["payload"]["location"]["document_path"], HOSTILE_DOC,
        "the hostile document path round-trips byte-exactly: {result}"
    );
}

// _(Source-faithful content rendering: content is not a terminal-injection vector)_ —
// a content-bearing detail (signature/interface/body) renders its source text with its own newline
// preserved, but the terminal-control class embedded in it is made visible as a replacement character
// so verbatim source cannot command the reader's terminal. The machine (`--json`) answer stays
// byte-exact.
#[test]
fn body_content_sanitizes_terminal_controls_but_json_round_trips_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", HOSTILE_ID, "--detail", "body"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The escape byte in the body is neutralized to a replacement character, never executed.
    assert!(
        !stdout.contains('\u{1b}'),
        "the body's ANSI escape is made visible, not executed: {stdout:?}"
    );
    assert!(
        stdout.contains('\u{FFFD}'),
        "the neutralized control renders as a replacement character: {stdout:?}"
    );
    // The source text and its own newline survive: the body still reads as its two lines.
    assert!(
        stdout.contains("line one") && stdout.contains("line two"),
        "the source text itself is preserved: {stdout:?}"
    );

    // The machine answer is exempt: the hostile body round-trips byte-exactly through JSON's escaping.
    let json = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "get", HOSTILE_ID, "--detail", "body"])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0));
    let answer: serde_json::Value = serde_json::from_slice(&json.stdout).expect("the --json answer parses");
    assert_eq!(
        answer["outcome"]["results"][0]["payload"]["body"], HOSTILE_BODY,
        "the hostile body round-trips byte-exactly under --json: {answer}"
    );
}

/// A store at `dir/index.db` holding one in-workspace symbol whose body is a CRLF-terminated source
/// block carrying no injection-hazard characters, to prove content sanitization leaves `\r` intact.
fn build_crlf_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: CanonicalId::from_raw("crlf-ws::win".to_string()),
            display_name: "win".to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("src/win.rs".to_string()),
            span: Some((0, 4)),
            span_text: Some("line one\r\nline two\r\n".to_string()),
            signature_text: None,
            interface_text: None,
            duplicated: false,
        })
        .unwrap();
    support::stamp_metadata(&store, "crlf-ws");
    db
}

// _(Source-faithful content rendering: CRLF source survives)_ — a CRLF-terminated body renders with no
// replacement characters: `\r`, like `\n` and `\t`, is exempt from content sanitization, so a
// Windows-line-ending source file does not sprout replacement characters.
#[test]
fn crlf_body_renders_without_replacement_characters() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_crlf_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["get", "crlf-ws::win", "--detail", "body"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{FFFD}'),
        "a CRLF source body sprouts no replacement characters: {stdout:?}"
    );
    assert!(
        stdout.contains('\r'),
        "the carriage return is preserved verbatim: {stdout:?}"
    );
}

// _(Source-faithful content rendering: `references` row human output)_ — a `references` row over the
// hostile symbol (added by `build_hostile_db` as its own reference site) renders no raw terminal
// control at the default (no-content) detail, with no forged row from the hostile document path's
// embedded newline; at a content-bearing detail the row's projected content — carrying all three
// injection classes (ANSI escape, C1 CSI, bidirectional override) — renders neutralized to
// replacement characters while the machine answer round-trips the same content byte-exactly.
#[test]
fn references_rows_sanitize_hostile_fields_in_human_output() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["trace", HOSTILE_ID, "--relation", "references"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}') && !stdout.contains('\u{9b}') && !stdout.contains('\u{202e}'),
        "no raw terminal control reaches the references row: {stdout:?}"
    );
    // header + "N results" + one row: the hostile document path's embedded newline forges no extra
    // row.
    assert_eq!(stdout.lines().count(), 3, "no forged row: {stdout:?}");

    // At signature detail, the row's projected content — the hostile symbol's own signature tier,
    // attributed as the reference site's enclosing declaration — carries all three control classes,
    // neutralized in the human render.
    let human = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["trace", HOSTILE_ID, "--relation", "references", "--detail", "signature"])
        .output()
        .unwrap();
    assert_eq!(
        human.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    let human_stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        !human_stdout.contains('\u{1b}') && !human_stdout.contains('\u{9b}') && !human_stdout.contains('\u{202e}'),
        "no raw terminal control reaches the signature-detail row: {human_stdout:?}"
    );
    assert!(
        human_stdout.contains('\u{FFFD}'),
        "the neutralized controls render as replacement characters: {human_stdout:?}"
    );

    // The machine answer stays byte-exact.
    let json = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args([
            "--json",
            "trace",
            HOSTILE_ID,
            "--relation",
            "references",
            "--detail",
            "signature",
        ])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(0));
    let answer: serde_json::Value = serde_json::from_slice(&json.stdout).expect("the --json answer parses");
    assert_eq!(
        answer["outcome"]["results"][0]["content"], HOSTILE_BODY,
        "the hostile content round-trips byte-exactly under --json: {answer}"
    );
}

/// A hostile shortname carrying an ANSI escape and a raw newline, shared by two distinct symbols so
/// resolving it is ambiguous — the newline is the same row-forging payload a candidate line must not
/// let through.
const AMBIGUOUS_HOSTILE_NAME: &str = "dup\x1b[31m\ninjected";

/// A store at `dir/index.db` holding two in-workspace symbols that share [`AMBIGUOUS_HOSTILE_NAME`]
/// as their display name, so resolving it by shortname is ambiguous.
fn build_ambiguous_hostile_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    for suffix in ["a", "b"] {
        store
            .insert_symbol(&SymbolRow {
                canonical_id: CanonicalId::from_raw(format!("hostile-ws::dup_{suffix}")),
                display_name: AMBIGUOUS_HOSTILE_NAME.to_string(),
                kind: "function".to_string(),
                class: PersistedClass::InWorkspace,
                document_path: None,
                span: None,
                span_text: None,
                signature_text: None,
                interface_text: None,
                duplicated: false,
            })
            .unwrap();
    }
    support::stamp_metadata(&store, "hostile-ws");
    db
}

// _(Source-faithful content rendering: ambiguous candidate lines sanitize hostile names)_ — two
// symbols sharing a hostile shortname resolve ambiguously; the human candidate list carries no raw
// terminal control and the embedded newline forges no extra candidate row.
#[test]
fn ambiguous_candidate_lines_sanitize_hostile_names() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_ambiguous_hostile_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("get")
        .arg(AMBIGUOUS_HOSTILE_NAME)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "no ANSI escape reaches the candidate list: {stdout:?}"
    );
    // header + "ambiguous: 2 candidates" + one line per candidate: the embedded newline in the shared
    // hostile display name forges no extra candidate row.
    assert_eq!(
        stdout.lines().count(),
        4,
        "the embedded newline forges no extra candidate row: {stdout:?}"
    );
}

/// A store at `dir/index.db` holding a subject and one dependent whose `display_name` and
/// `document_path` carry terminal-control bytes, connected by a `uses` edge, so a `dependents`
/// impact answer's detail row and summary project the hostile fields.
fn build_hostile_dependents_db(dir: &Path) -> PathBuf {
    let db = dir.join("index.db");
    let store = GraphStore::open(&db).unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: CanonicalId::from_raw("hostile-ws::subject".to_string()),
            display_name: "subject".to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some("src/subject.rs".to_string()),
            span: Some((0, 1)),
            span_text: None,
            signature_text: None,
            interface_text: None,
            duplicated: false,
        })
        .unwrap();
    store
        .insert_symbol(&SymbolRow {
            canonical_id: CanonicalId::from_raw(HOSTILE_ID.to_string()),
            display_name: HOSTILE_NAME.to_string(),
            kind: "function".to_string(),
            class: PersistedClass::InWorkspace,
            document_path: Some(HOSTILE_DOC.to_string()),
            span: Some((0, 4)),
            span_text: Some(HOSTILE_BODY.to_string()),
            signature_text: Some(HOSTILE_BODY.to_string()),
            interface_text: Some(HOSTILE_BODY.to_string()),
            duplicated: false,
        })
        .unwrap();
    store
        .insert_edge(
            EdgeKind::Uses,
            &CanonicalId::from_raw(HOSTILE_ID.to_string()),
            &CanonicalId::from_raw("hostile-ws::subject".to_string()),
        )
        .unwrap();
    support::stamp_metadata(&store, "hostile-ws");
    db
}

// _(Source-faithful content rendering: dependents rows sanitize hostile fields)_ — a dependent
// symbol's hostile display name and document path render sanitized in the human `dependents`
// rendering: the summary and the detail row carry no raw terminal control, and the hostile path's
// embedded newline forges no extra row.
#[test]
fn dependents_rows_render_sanitized_in_human_output() {
    let dir = tempfile::tempdir().unwrap();
    let db = build_hostile_dependents_db(dir.path());

    let out = c10r(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["trace", "hostile-ws::subject", "--relation", "dependents"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains('\u{1b}') && !stdout.contains('\u{9b}') && !stdout.contains('\u{202e}'),
        "no raw terminal control reaches the dependents rendering: {stdout:?}"
    );
    // header + summary line + "1 detailed" + one row: the hostile path's embedded newline forges no
    // extra row.
    assert_eq!(stdout.lines().count(), 4, "no forged row: {stdout:?}");
}
