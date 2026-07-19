//! `completions` contract tests: a supported shell emits a non-empty script on standard output with
//! no diagnostic, an unsupported shell is a usage error naming the accepted set, `--json` is rejected
//! as meaningless for a script answer, and the command needs no index.
//!
//! These drive the built `c10r` binary through `std::process::Command` so the process-level contract
//! (exit codes, stdout/stderr separation) is observed as a caller would.

use std::process::Command;

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`).
fn c10r() -> Command {
    Command::new(env!("CARGO_BIN_EXE_c10r"))
}

/// The top-level command names the emitted script must name — the observable drift guard: if the
/// script were hand-written or generated from a stale command tree rather than `Cli::command()`, a
/// renamed or removed command would not show up here. The assertions anchor on the structural
/// spelling each generator uses for a command word (`'<name>:` in zsh's `_describe` entries,
/// `c10r,<name>)` in bash's case arms), not a bare substring a help sentence could mask.
const TOP_LEVEL_COMMANDS: [&str; 9] = [
    "get",
    "trace",
    "find",
    "build",
    "status",
    "doctor",
    "cache",
    "manifest",
    "completions",
];

// _(Shell completion scripts: a supported shell)_ — `completions zsh` succeeds, emits a non-empty
// script on standard output with no diagnostic, and the script names every current top-level command.
#[test]
fn completions_zsh_emits_a_script_naming_the_current_commands() {
    // No `--db`/index anywhere: completions needs no index, so an empty temp directory is orientation
    // enough (the same "runs before build" guarantee `manifest` and `doctor` carry).
    let dir = tempfile::tempdir().unwrap();

    let out = c10r()
        .current_dir(dir.path())
        .args(["completions", "zsh"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "no diagnostic on a successful script emission");
    let script = String::from_utf8(out.stdout).expect("the script is valid UTF-8");
    assert!(!script.is_empty(), "a non-empty script lands on standard output");
    for command in TOP_LEVEL_COMMANDS {
        // The `_describe` command-word entry (`'get:Retrieve …'`): a structural anchor a help
        // sentence cannot fake, so a renamed or removed command fails here.
        let anchor = format!("'{command}:");
        assert!(
            script.contains(&anchor),
            "the zsh script carries the command-word entry for `{command}` (anchor `{anchor}`)"
        );
    }
}

// _(Shell completion scripts: a second supported shell)_ — `completions bash` succeeds with a
// non-empty script distinct from the zsh script (each shell's generator produces its own syntax).
#[test]
fn completions_bash_emits_a_script_distinct_from_zsh() {
    let dir = tempfile::tempdir().unwrap();

    let zsh = c10r()
        .current_dir(dir.path())
        .args(["completions", "zsh"])
        .output()
        .unwrap();
    let bash = c10r()
        .current_dir(dir.path())
        .args(["completions", "bash"])
        .output()
        .unwrap();

    assert_eq!(bash.status.code(), Some(0));
    assert!(bash.stderr.is_empty(), "no diagnostic on a successful script emission");
    let script = String::from_utf8(bash.stdout.clone()).expect("the script is valid UTF-8");
    assert!(!script.is_empty(), "a non-empty script lands on standard output");
    for command in TOP_LEVEL_COMMANDS {
        // The generator's case arm for each command word (`c10r,get)`): the structural anchor a
        // help sentence cannot fake.
        let anchor = format!("c10r,{command})");
        assert!(
            script.contains(&anchor),
            "the bash script carries the case arm for `{command}` (anchor `{anchor}`)"
        );
    }
    assert_ne!(zsh.stdout, bash.stdout, "the bash script differs from the zsh script");
}

// _(Shell completion scripts: an unknown shell is a usage error)_ — an unsupported shell value is
// rejected as a usage error, and the diagnostic lists the accepted shells.
#[test]
fn unknown_shell_is_a_usage_error_listing_the_accepted_shells() {
    let dir = tempfile::tempdir().unwrap();

    let out = c10r()
        .current_dir(dir.path())
        .args(["completions", "nosuchshell"])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2), "an out-of-set shell value is a usage error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        assert!(stderr.contains(shell), "the diagnostic lists `{shell}`: {stderr}");
    }
    assert!(out.stdout.is_empty(), "no script is emitted for a rejected shell value");
}

// _(Shell completion scripts: an explicit `--json` is rejected)_ — a completion script is not a
// machine answer, so an explicit `--json` is rejected as a usage error naming why, rather than
// silently ignored, mirroring the modal content-bound teaching-error convention.
#[test]
fn explicit_json_with_completions_is_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();

    let out = c10r()
        .current_dir(dir.path())
        .args(["--json", "completions", "zsh"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "an explicit --json on completions is a usage error"
    );
    assert!(
        out.stdout.is_empty(),
        "no script is emitted when the invocation is rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--json") && stderr.contains("completions"),
        "the diagnostic names --json and completions: {stderr}"
    );
}

// _(Shell completion scripts: completion precedes any build)_ — an empty directory with no `.c10r`
// database still succeeds, and the invocation creates no index as a side effect.
#[test]
fn completions_succeeds_in_an_empty_directory_with_no_index() {
    let dir = tempfile::tempdir().unwrap();

    let out = c10r()
        .current_dir(dir.path())
        .args(["completions", "zsh"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty());
    assert!(
        !dir.path().join(".c10r").exists(),
        "completions created no index as a side effect"
    );
}
