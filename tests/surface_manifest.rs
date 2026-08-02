//! `manifest`'s structural surface: the derived command/flag structure enumerates every command
//! with its valid argument and flag values and defaults, and is pinned to a checked-in snapshot
//! paired with the `surface_version` it was taken at, so drift is a checked invariant rather than a
//! silent parser change.
//!
//! Scenario 3 of the command-surface spec — "two builds whose structure differs carry different
//! surface versions" — is exercised here, not by building two binaries: the snapshot test below IS
//! the check that a structural difference without a matching `SURFACE_VERSION` bump fails.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::CommandFactory;
use silent_cartographer::cli::Cli;
use silent_cartographer::manifest::{SURFACE_VERSION, derive_structure};

/// A fresh invocation of the built binary (never the ambient `c10r` on `PATH`).
fn c10r() -> Command {
    Command::new(env!("CARGO_BIN_EXE_c10r"))
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/surface_manifest.snapshot.json")
}

/// Whether regenerating the surface snapshot is permitted, given the fixture's current contents, the
/// freshly derived structure, and the current `SURFACE_VERSION`.
///
/// Regeneration is refused in exactly one case: the existing fixture parses, its recorded
/// `surface_version` equals `current_version`, and its command structure differs from the derived one
/// — a structural change that reuses the same version, which the version exists to prevent. Every
/// other case is allowed: no fixture yet, an unparseable fixture, a different recorded version (a bump
/// is present), or an unchanged structure (a no-op regeneration).
fn regen_allowed(existing: Option<&str>, derived: &str, current_version: u32) -> bool {
    let Some(existing) = existing else {
        return true;
    };
    let Ok(recorded) = serde_json::from_str::<serde_json::Value>(existing) else {
        return true;
    };
    let Ok(derived_json) = serde_json::from_str::<serde_json::Value>(derived) else {
        return true;
    };
    if recorded.get("surface_version").and_then(|v| v.as_u64()) != Some(current_version as u64) {
        return true;
    }
    recorded["commands"] == derived_json["commands"]
}

// _(Versioned structural surface index: enumerates commands with valid values and defaults)_ — the
// derived structure names every command, its positional args, and its flags with their enumerated
// values and defaults; a `surface_version` is carried alongside.
#[test]
fn derived_structure_enumerates_commands_args_flags_and_carries_a_surface_version() {
    let structure = derive_structure(&Cli::command());
    assert_eq!(structure.surface_version, SURFACE_VERSION);

    let names: Vec<&str> = structure.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "get",
            "trace",
            "find",
            "impact",
            "build",
            "status",
            "doctor",
            "cache",
            "hooks",
            "manifest",
            "completions"
        ],
        "every command is enumerated, completions included"
    );

    let trace = structure.commands.iter().find(|c| c.name == "trace").unwrap();
    let relation = trace.flags.iter().find(|f| f.name == "relation").unwrap();
    assert_eq!(
        relation.values,
        [
            "containers",
            "contains",
            "references",
            "dependents",
            "importers",
            "implementers",
            "tests"
        ],
        "a ValueEnum-backed flag lists its valid values"
    );

    let db = trace.flags.iter().find(|f| f.name == "db").unwrap();
    assert_eq!(
        db.default.as_deref(),
        Some(".c10r/index.db"),
        "a global flag's default is carried"
    );
    assert!(db.global, "db is carried as a global flag");

    let get = structure.commands.iter().find(|c| c.name == "get").unwrap();
    let reference = get.args.iter().find(|a| a.name == "reference").unwrap();
    assert!(
        !reference.required,
        "get's reference is optional (omittable in favor of --at)"
    );

    // A ValueEnum-backed positional lists its valid values, exactly as a ValueEnum-backed flag does.
    let completions = structure.commands.iter().find(|c| c.name == "completions").unwrap();
    let shell = completions.args.iter().find(|a| a.name == "shell").unwrap();
    assert_eq!(
        shell.values,
        ["bash", "elvish", "fish", "powershell", "zsh"],
        "the shell positional enumerates its supported shells"
    );
}

// _(Versioned structural surface index: version distinguishes structural revisions)_ — the derived
// structure (commands, args, flags — no index state) is pinned to a checked-in snapshot fixture
// paired with the `surface_version` it was taken at. A structural change without a matching bump
// fails this test; regenerate the fixture and bump `SURFACE_VERSION` together with:
// `UPDATE_SURFACE_SNAPSHOT=1 cargo test --test surface_manifest snapshot -- --nocapture`
#[test]
fn snapshot_matches_the_checked_in_surface_and_its_recorded_version() {
    let structure = derive_structure(&Cli::command());
    let derived = serde_json::to_string_pretty(&structure).expect("structure serializes");

    if std::env::var_os("UPDATE_SURFACE_SNAPSHOT").is_some() {
        // Guard the regeneration itself: a structural change with no matching `SURFACE_VERSION` bump
        // must not silently reuse the recorded version — the whole point of the version is to move when
        // the structure does. Regenerate only when the fixture is fresh, the version was bumped, or the
        // structure is unchanged.
        let existing = std::fs::read_to_string(fixture_path()).ok();
        assert!(
            regen_allowed(existing.as_deref(), &derived, SURFACE_VERSION),
            "structure changed but SURFACE_VERSION was not bumped; bump it first, then regenerate"
        );
        std::fs::write(fixture_path(), format!("{derived}\n")).expect("writing regenerated snapshot fixture");
    }

    let recorded = std::fs::read_to_string(fixture_path()).unwrap_or_else(|e| {
        panic!(
            "no checked-in surface snapshot at {}: {e}; generate it with \
             `UPDATE_SURFACE_SNAPSHOT=1 cargo test --test surface_manifest snapshot -- --nocapture`",
            fixture_path().display()
        )
    });

    assert_eq!(
        derived.trim_end(),
        recorded.trim_end(),
        "the derived command/flag structure drifted from the checked-in snapshot at {}; if this \
         change is intentional, bump SURFACE_VERSION in src/manifest.rs and regenerate the fixture \
         in the same change with `UPDATE_SURFACE_SNAPSHOT=1 cargo test --test surface_manifest \
         snapshot -- --nocapture`",
        fixture_path().display()
    );

    let recorded_json: serde_json::Value = serde_json::from_str(&recorded).expect("the fixture parses as JSON");
    assert_eq!(
        recorded_json["surface_version"],
        SURFACE_VERSION,
        "the checked-in snapshot's recorded surface_version must match SURFACE_VERSION at {}",
        fixture_path().display()
    );
}

// _(Versioned structural surface index: process-level enumeration)_ — `manifest` run through the
// real binary emits JSON enumerating every command, a ValueEnum flag's valid values, a carried
// default, and a `surface_version`, whether or not an index has been built.
#[test]
fn manifest_enumerates_the_surface_through_the_binary_with_and_without_an_index() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");

    // No index has been built: manifest still succeeds, reporting the index as unbuilt rather than
    // exiting with the no-index code — an agent orienting on arrival may not have built yet.
    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("manifest")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "manifest succeeds even with no index built: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!db.exists(), "manifest did not bring an index into being");

    let manifest: serde_json::Value = serde_json::from_slice(&out.stdout).expect("manifest emits JSON");
    assert_eq!(manifest["surface_version"], SURFACE_VERSION);
    assert_eq!(
        manifest["index"]["built"], false,
        "no index built is reported, not a failure"
    );

    let commands = manifest["commands"].as_array().expect("commands is a list");
    let names: Vec<&str> = commands.iter().map(|c| c["name"].as_str().unwrap()).collect();
    for expected in [
        "get",
        "trace",
        "find",
        "impact",
        "build",
        "status",
        "doctor",
        "cache",
        "hooks",
        "manifest",
        "completions",
    ] {
        assert!(names.contains(&expected), "manifest enumerates `{expected}`: {names:?}");
    }

    let trace = commands.iter().find(|c| c["name"] == "trace").unwrap();
    let relation = trace["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "relation")
        .expect("trace carries --relation");
    let values: Vec<&str> = relation["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        values,
        [
            "containers",
            "contains",
            "references",
            "dependents",
            "importers",
            "implementers",
            "tests"
        ],
        "the relation flag enumerates its seven valid values"
    );

    let color = trace["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "color")
        .expect("color is a global flag on every command");
    assert_eq!(color["default"], "auto", "the color default is carried");

    let db_flag = trace["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "db")
        .unwrap();
    assert_eq!(db_flag["default"], ".c10r/index.db", "the db default is carried");

    // `--json` is accepted (it is a global flag) but changes nothing: the answer is always JSON.
    let with_json_flag = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .args(["--json", "manifest"])
        .output()
        .unwrap();
    assert_eq!(with_json_flag.status.code(), Some(0));
    let with_json: serde_json::Value = serde_json::from_slice(&with_json_flag.stdout).unwrap();
    assert_eq!(with_json, manifest, "--json changes nothing for manifest's answer");

    // With a built fixture index, manifest reports its state.
    let sources = vec![(support::DOC.to_string(), support::SOURCE.to_string())];
    silent_cartographer::commands::build_from_index(&db, "op-ws", &support::fixture_index(), &sources).unwrap();

    let built = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("manifest")
        .output()
        .unwrap();
    assert_eq!(built.status.code(), Some(0));
    let built_manifest: serde_json::Value = serde_json::from_slice(&built.stdout).expect("manifest emits JSON");
    assert_eq!(built_manifest["index"]["built"], true, "a built index is reported");
    assert_eq!(built_manifest["index"]["workspace"], "op-ws");
}

// _(Versioned structural surface index: orientation absorbs an unreadable store)_ — a file present
// at the `--db` path that is not a readable index (garbage bytes, not SQLite) does not fail
// `manifest`: unlike a query, orientation needs no index, so the unreadable store collapses to
// `built: false` and the structural answer still lands with the success code.
#[test]
fn manifest_reports_built_false_for_an_unreadable_store() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    std::fs::write(&db, b"this is not a sqlite database").unwrap();

    let out = c10r()
        .current_dir(dir.path())
        .arg("--db")
        .arg(&db)
        .arg("manifest")
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "manifest succeeds over an unreadable store: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest: serde_json::Value = serde_json::from_slice(&out.stdout).expect("manifest emits JSON");
    assert_eq!(manifest["surface_version"], SURFACE_VERSION);
    assert_eq!(
        manifest["index"]["built"], false,
        "an unreadable store reads as no built index, not a failure"
    );
}

// _(Surface-snapshot regeneration guard)_ — the pure decision behind the `UPDATE_SURFACE_SNAPSHOT`
// path: a structural change at the same recorded version is refused (the version must move with the
// structure), while a fresh fixture, a bumped version, or an unchanged structure are all allowed.
#[test]
fn regen_is_refused_only_when_structure_changes_without_a_version_bump() {
    let v2_a = r#"{ "surface_version": 2, "commands": [{ "name": "get" }] }"#;
    let v2_b = r#"{ "surface_version": 2, "commands": [{ "name": "get" }, { "name": "trace" }] }"#;
    let v1_b = r#"{ "surface_version": 1, "commands": [{ "name": "get" }, { "name": "trace" }] }"#;

    // No fixture yet: always allowed.
    assert!(regen_allowed(None, v2_b, 2), "a fresh fixture regenerates");
    // Unparseable fixture: overwrite is allowed.
    assert!(
        regen_allowed(Some("not json"), v2_b, 2),
        "an unparseable fixture regenerates"
    );
    // Same version, unchanged structure: a no-op regeneration is allowed.
    assert!(
        regen_allowed(Some(v2_a), v2_a, 2),
        "an unchanged structure regenerates freely"
    );
    // Same version, changed structure: refused — the version must be bumped first.
    assert!(
        !regen_allowed(Some(v2_a), v2_b, 2),
        "a structural change at the same version is refused"
    );
    // A bumped version accompanies the structural change: allowed.
    assert!(
        regen_allowed(Some(v1_b), v2_b, 2),
        "a structural change with a bumped version regenerates"
    );
}
