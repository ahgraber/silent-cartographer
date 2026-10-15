//! The Rust semantic backend: `rust-analyzer scip` for cross-file identity/resolution/types,
//! translated into the engine-neutral model.
//!
//! The heavy, environment-dependent step — invoking `rust-analyzer scip` — is isolated in
//! [`RustAdapter::analyze`]. The translation from a SCIP index into the model is a pure function
//! ([`translate_index`]) so it is unit-testable without a live analyzer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use scip::types as scip_types;

use super::model::{AnalyzerProvenance, BuildTarget, ExtractedIndex, InventoryFailure, TargetInventory, normalize};
use super::probe::{PROBE_DEADLINE, ProbeOutcome, run_bounded};
pub use super::scip::translate_index;
use super::{Capabilities, SemanticEngine, SemanticError};

/// The remedy for a `rust-analyzer` that cannot be started.
const INSTALL_STEP: &str = "Install it with `rustup component add rust-analyzer`, or pass `--rust-analyzer <path>` if it is installed elsewhere.";

/// The Rust adapter, wrapping `rust-analyzer scip` and translating its output into the model.
pub struct RustAdapter {
    /// The `rust-analyzer` executable to invoke.
    executable: String,
    /// The analyzer version, discovered once at construction.
    version: String,
}

impl RustAdapter {
    /// Construct an adapter for the given `rust-analyzer` executable, discovering its version.
    ///
    /// The version probe is bounded: a missing executable is [`SemanticError::Unavailable`], and one
    /// that is present but does not answer within the probe deadline is [`SemanticError::Timeout`], so
    /// the caller learns explicitly rather than by a later opaque failure or an indefinite hang.
    pub fn new(executable: impl Into<String>) -> Result<Self, SemanticError> {
        let executable = executable.into();
        let mut command = Command::new(&executable);
        command.arg("--version");
        let capture = match run_bounded(command, PROBE_DEADLINE) {
            Ok(ProbeOutcome::Completed(capture)) => capture,
            Ok(ProbeOutcome::TimedOut) => {
                return Err(SemanticError::Timeout(format!(
                    "{executable} did not answer `--version` within {} s, so no index was built. \
                     Run `{executable} --version` to see why, or pass `--rust-analyzer <path>` to use another build.",
                    PROBE_DEADLINE.as_secs()
                )));
            }
            Err(e) => {
                return Err(SemanticError::Unavailable(format!(
                    "could not run `{executable}` ({e}), so no index was built. {INSTALL_STEP}"
                )));
            }
        };
        if !capture.status.success() {
            return Err(SemanticError::Unavailable(format!(
                "`{executable} --version` failed ({}), so no index was built. Reinstall with \
                 `rustup component add rust-analyzer`, or pass `--rust-analyzer <path>` to use another build.",
                capture.failure_detail()
            )));
        }
        let version = String::from_utf8_lossy(&capture.stdout).trim().to_string();
        Ok(Self { executable, version })
    }

    /// The analyzer name this adapter reports as provenance.
    pub fn analyzer_name() -> &'static str {
        "rust-analyzer"
    }
}

impl SemanticEngine for RustAdapter {
    fn provenance(&self) -> AnalyzerProvenance {
        AnalyzerProvenance {
            analyzer_name: Self::analyzer_name().to_string(),
            analyzer_version: self.version.clone(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        // The Rust adapter pairs SCIP with tree-sitter for enclosure and is a full batch index,
        // so it declares enclosure support and base-index eligibility; it is not a live backend.
        Capabilities::none().with_enclosure().with_base_index()
    }

    fn analyze(&self, project_root: &Path) -> Result<ExtractedIndex, SemanticError> {
        let output_path = project_root.join("index.scip");
        let status = Command::new(&self.executable)
            .arg("scip")
            .arg(project_root)
            .arg("--output")
            .arg(&output_path)
            .status()
            .map_err(|e| {
                SemanticError::Unavailable(format!(
                    "could not start `{} scip` ({e}). {INSTALL_STEP}",
                    self.executable
                ))
            })?;
        if !status.success() {
            return Err(SemanticError::Analysis(format!(
                "`{} scip` failed ({status}). Its own error output is printed above.",
                self.executable
            )));
        }
        let bytes = std::fs::read(&output_path).map_err(|e| {
            SemanticError::Analysis(format!(
                "`{} scip` finished, but its output {} could not be read ({e}).",
                self.executable,
                output_path.display()
            ))
        })?;
        let index: scip_types::Index = protobuf::Message::parse_from_bytes(&bytes).map_err(|e| {
            SemanticError::Analysis(format!(
                "`{} scip` finished, but its output {} is not a valid SCIP index ({e}).",
                self.executable,
                output_path.display()
            ))
        })?;
        let mut extracted = translate_index(&index, &self.provenance());
        extracted.targets = cargo_targets(project_root);
        Ok(normalize(extracted))
    }
}

/// The build targets of the project's packages, from `cargo metadata --no-deps`.
///
/// This is enrichment, never a build gate: every failure — no manifest, a command that cannot be
/// spawned, a timeout, a non-zero exit, unparsable output — is reported as
/// [`TargetInventory::Unavailable`] with its cause, and the index is still built.
pub fn cargo_targets(project_root: &Path) -> TargetInventory {
    match cargo_inventory(project_root) {
        TargetInventory::Available { targets, .. } => TargetInventory::Available {
            manifests: manifest_digests(project_root, &targets),
            targets,
        },
        other => other,
    }
}

/// The inventory `cargo metadata` yields, before any manifest digest is attached.
fn cargo_inventory(project_root: &Path) -> TargetInventory {
    if !project_root.join("Cargo.toml").is_file() {
        return TargetInventory::Unavailable(InventoryFailure::NoManifest);
    }
    let mut command = Command::new("cargo");
    command
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(project_root);
    inventory_from_probe(run_bounded(command, PROBE_DEADLINE))
}

/// The workspace root's manifest, which every inventory depends on.
const MANIFEST: &str = "Cargo.toml";

/// The manifests an inventory of `targets` depends on: the workspace root's and every package's,
/// each once.
pub fn required_manifests(targets: &[BuildTarget]) -> BTreeSet<&str> {
    std::iter::once(MANIFEST)
        .chain(targets.iter().map(|target| target.manifest.as_str()))
        .collect()
}

/// The text digest of each manifest the targets were read from. A manifest that cannot be read is
/// left out, so a later comparison sees it as changed.
fn manifest_digests(project_root: &Path, targets: &[BuildTarget]) -> BTreeMap<String, String> {
    required_manifests(targets)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(project_root.join(path)).ok()?;
            Some((path.to_string(), crate::graph::text_hash(&text)))
        })
        .collect()
}

/// The inventory a `cargo metadata` probe yields.
fn inventory_from_probe(probe: std::io::Result<ProbeOutcome>) -> TargetInventory {
    match probe {
        Ok(ProbeOutcome::Completed(capture)) if capture.status.success() => parse_targets(&capture.stdout),
        Ok(ProbeOutcome::Completed(capture)) => {
            let stderr = String::from_utf8_lossy(&capture.stderr);
            let detail = match stderr.lines().next().map(str::trim) {
                Some(line) if !line.is_empty() => line.to_string(),
                _ => capture.status.to_string(),
            };
            TargetInventory::Unavailable(InventoryFailure::MetadataFailed { detail })
        }
        Ok(ProbeOutcome::TimedOut) => TargetInventory::Unavailable(InventoryFailure::MetadataTimeout),
        Err(e) => TargetInventory::Unavailable(InventoryFailure::MetadataFailed {
            detail: format!("could not run `cargo` ({e})"),
        }),
    }
}

/// Parse `cargo metadata --no-deps --format-version 1` output into the target inventory.
///
/// External boundary: output that is not the expected shape is
/// [`InventoryFailure::MetadataFailed`]; a package or target with a surprising shape is skipped
/// rather than failing the rest. Every target is kept, whatever its kind. A target's kind is the
/// first of the kinds Cargo lists (`["lib", "rlib"]` is `lib`). The absolute `src_path` is made
/// workspace-relative against the metadata's own `workspace_root` so it matches SCIP document
/// paths; a target rooted outside the workspace is skipped.
fn parse_targets(metadata_json: &[u8]) -> TargetInventory {
    let unreadable = || {
        TargetInventory::Unavailable(InventoryFailure::MetadataFailed {
            detail: "the output of `cargo metadata` could not be read".to_string(),
        })
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(metadata_json) else {
        return unreadable();
    };
    let (Some(workspace_root), Some(packages)) = (
        value.get("workspace_root").and_then(|v| v.as_str()),
        value.get("packages").and_then(|v| v.as_array()),
    ) else {
        return unreadable();
    };

    let mut targets = Vec::new();
    for package in packages {
        let Some(package_name) = package.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(package_targets) = package.get("targets").and_then(|v| v.as_array()) else {
            continue;
        };
        for target in package_targets {
            let (Some(kind), Some(name), Some(src_path)) = (
                target
                    .get("kind")
                    .and_then(|v| v.as_array())
                    .and_then(|kinds| kinds.first())
                    .and_then(|k| k.as_str()),
                target.get("name").and_then(|v| v.as_str()),
                target.get("src_path").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let Ok(relative) = Path::new(src_path).strip_prefix(workspace_root) else {
                continue;
            };
            let manifest = package
                .get("manifest_path")
                .and_then(|v| v.as_str())
                .and_then(|path| Path::new(path).strip_prefix(workspace_root).ok())
                .map_or_else(
                    || MANIFEST.to_string(),
                    |path| path.to_string_lossy().replace('\\', "/"),
                );
            targets.push(BuildTarget {
                package: package_name.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                root: relative.to_string_lossy().replace('\\', "/"),
                manifest,
            });
        }
    }
    TargetInventory::Available {
        targets,
        manifests: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(package, kind, name, root)` of every target in an inventory that is available.
    fn available(inventory: TargetInventory) -> Vec<(String, String, String, String)> {
        let TargetInventory::Available { targets, .. } = inventory else {
            panic!("the inventory is available: {inventory:?}");
        };
        targets
            .into_iter()
            .map(|t| (t.package, t.kind, t.name, t.root))
            .collect()
    }

    fn row(package: &str, kind: &str, name: &str, root: &str) -> (String, String, String, String) {
        (package.into(), kind.into(), name.into(), root.into())
    }

    // Every target is kept with its package, kind, name, and workspace-relative root, including a
    // default binary named like the package; a multi-kind library keeps its first kind.
    #[test]
    fn parse_targets_keeps_every_target_kind_and_name() {
        let json = r#"{
            "workspace_root": "/ws/proj",
            "packages": [
                {
                    "name": "libbin",
                    "targets": [
                        {"name": "libbin", "kind": ["lib", "rlib"], "src_path": "/ws/proj/src/lib.rs"},
                        {"name": "libbin", "kind": ["bin"], "src_path": "/ws/proj/src/main.rs"},
                        {"name": "tool", "kind": ["bin"], "src_path": "/ws/proj/src/bin/tool.rs"},
                        {"name": "a", "kind": ["test"], "src_path": "/ws/proj/tests/a.rs"},
                        {"name": "demo", "kind": ["example"], "src_path": "/ws/proj/examples/demo.rs"},
                        {"name": "speed", "kind": ["bench"], "src_path": "/ws/proj/benches/speed.rs"},
                        {"name": "build-script-build", "kind": ["custom-build"], "src_path": "/ws/proj/build.rs"}
                    ]
                }
            ]
        }"#;
        assert_eq!(
            available(parse_targets(json.as_bytes())),
            vec![
                row("libbin", "lib", "libbin", "src/lib.rs"),
                row("libbin", "bin", "libbin", "src/main.rs"),
                row("libbin", "bin", "tool", "src/bin/tool.rs"),
                row("libbin", "test", "a", "tests/a.rs"),
                row("libbin", "example", "demo", "examples/demo.rs"),
                row("libbin", "bench", "speed", "benches/speed.rs"),
                row("libbin", "custom-build", "build-script-build", "build.rs"),
            ]
        );
    }

    // Library kinds are the package's default unit; every other kind is not.
    #[test]
    fn library_kinds_are_the_default_unit() {
        let target = |kind: &str| BuildTarget {
            package: "p".into(),
            kind: kind.into(),
            name: "p".into(),
            root: "src/lib.rs".into(),
            manifest: "Cargo.toml".into(),
        };
        for kind in ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"] {
            assert!(target(kind).is_library(), "{kind}");
        }
        for kind in ["bin", "test", "example", "bench", "custom-build"] {
            assert!(!target(kind).is_library(), "{kind}");
        }
    }

    // A package rooted outside the workspace is skipped; a proc-macro target is kept.
    #[test]
    fn parse_targets_skips_out_of_workspace_targets() {
        let json = r#"{
            "workspace_root": "/ws/proj",
            "packages": [
                {
                    "name": "my-macros",
                    "targets": [{"name": "my-macros", "kind": ["proc-macro"], "src_path": "/ws/proj/macros/src/lib.rs"}]
                },
                {
                    "name": "vendored",
                    "targets": [{"name": "vendored", "kind": ["lib"], "src_path": "/elsewhere/src/lib.rs"}]
                }
            ]
        }"#;
        assert_eq!(
            available(parse_targets(json.as_bytes())),
            vec![row("my-macros", "proc-macro", "my-macros", "macros/src/lib.rs")]
        );
    }

    // The parser is an external boundary: unusable input is a typed failure, never a panic.
    #[test]
    fn parse_targets_reports_unreadable_output() {
        for input in [
            &b"not json at all"[..],
            b"{}",
            br#"{"workspace_root": "/ws", "packages": "not-an-array"}"#,
        ] {
            let inventory = parse_targets(input);
            assert!(
                matches!(
                    inventory,
                    TargetInventory::Unavailable(InventoryFailure::MetadataFailed { .. })
                ),
                "{inventory:?}"
            );
        }
    }

    // A directory with no manifest has no targets to describe, without running cargo.
    #[test]
    fn missing_manifest_is_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            cargo_targets(dir.path()),
            TargetInventory::Unavailable(InventoryFailure::NoManifest)
        );
    }

    // A probe that timed out or failed maps to its own cause; a failure carries the first line of
    // cargo's error output.
    #[test]
    fn probe_failures_map_to_their_causes() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            inventory_from_probe(Ok(ProbeOutcome::TimedOut)),
            TargetInventory::Unavailable(InventoryFailure::MetadataTimeout)
        );
        let failed = ProbeOutcome::Completed(super::super::probe::ProbeCapture {
            stdout: Vec::new(),
            stderr: b"error: failed to parse manifest\n\nCaused by: x\n".to_vec(),
            status: std::process::ExitStatus::from_raw(1 << 8),
        });
        assert_eq!(
            inventory_from_probe(Ok(failed)),
            TargetInventory::Unavailable(InventoryFailure::MetadataFailed {
                detail: "error: failed to parse manifest".to_string()
            })
        );
        let spawn = inventory_from_probe(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert!(
            matches!(
                spawn,
                TargetInventory::Unavailable(InventoryFailure::MetadataFailed { .. })
            ),
            "{spawn:?}"
        );
    }

    /// An executable shell stub named `name` in `dir` running `body`.
    fn write_stub(dir: &Path, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    // A tool that cannot be started names the install command, the flag that points elsewhere, and
    // the cause the operating system gave.
    #[test]
    fn missing_tool_message_names_install_command_flag_and_cause() {
        let err = RustAdapter::new("/nonexistent/rust-analyzer")
            .err()
            .expect("a missing tool refuses");
        assert!(matches!(err, SemanticError::Unavailable(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("/nonexistent/rust-analyzer"), "{text}");
        assert!(text.contains("No such file or directory"), "{text}");
        assert!(text.contains("no index was built"), "{text}");
        assert!(text.contains("rustup component add rust-analyzer"), "{text}");
        assert!(text.contains("--rust-analyzer"), "{text}");
    }

    // A tool that ran but failed its version probe reports the exit status and the error output it
    // wrote.
    #[test]
    fn failed_version_probe_message_carries_exit_status_and_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(dir.path(), "rust-analyzer", "echo 'toolchain is broken' >&2\nexit 3");
        let err = RustAdapter::new(exe).err().expect("a failing probe refuses");
        let text = err.to_string();
        assert!(text.contains("exit status: 3"), "{text}");
        assert!(text.contains("toolchain is broken"), "{text}");
        assert!(text.contains("--version"), "{text}");
    }

    // A tool that is present but silent past the probe deadline is reported as not answering, never
    // as missing.
    #[test]
    fn unresponsive_tool_message_is_not_called_missing() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(dir.path(), "rust-analyzer", "sleep 30");
        let err = RustAdapter::new(exe).err().expect("an unresponsive tool refuses");
        assert!(matches!(err, SemanticError::Timeout(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("did not answer"), "{text}");
        assert!(!text.contains("rustup component add"), "{text}");
        assert!(!text.contains("could not run"), "{text}");
    }

    // An executable that answered `--version` but cannot be started for `scip` names the tool, the
    // operating system's cause, and the install step.
    #[test]
    fn an_analyzer_that_cannot_start_for_scip_names_the_cause_and_install_step() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(dir.path(), "rust-analyzer", "echo 1.0");
        let adapter = RustAdapter::new(exe.clone()).unwrap();
        std::fs::remove_file(&exe).unwrap();
        let err = adapter.analyze(dir.path()).expect_err("the analyzer is gone");
        assert!(matches!(err, SemanticError::Unavailable(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains(&exe) && text.contains("scip"), "{text}");
        assert!(text.contains("No such file or directory"), "{text}");
        assert!(text.contains("rustup component add rust-analyzer"), "{text}");
    }

    // An analyzer that exits 0 without writing its output names the tool and the unreadable output
    // path.
    #[test]
    fn an_analyzer_that_writes_no_output_names_the_missing_output() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(dir.path(), "rust-analyzer", "exit 0");
        let adapter = RustAdapter::new(exe.clone()).unwrap();
        let err = adapter.analyze(dir.path()).expect_err("no output was written");
        assert!(matches!(err, SemanticError::Analysis(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains(&exe) && text.contains("index.scip"), "{text}");
        assert!(text.contains("could not be read"), "{text}");
    }

    // An analyzer that exits 0 with output that is not a SCIP index names the tool and the output
    // path.
    #[test]
    fn an_analyzer_that_writes_garbage_names_the_invalid_output() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(
            dir.path(),
            "rust-analyzer",
            "if [ \"$1\" = \"--version\" ]; then echo 1.0; exit 0; fi\nprintf '\\377\\377\\377\\377' > \"$4\"",
        );
        let adapter = RustAdapter::new(exe.clone()).unwrap();
        let err = adapter.analyze(dir.path()).expect_err("the output is not SCIP");
        assert!(matches!(err, SemanticError::Analysis(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains(&exe) && text.contains("index.scip"), "{text}");
        assert!(text.contains("not a valid SCIP index"), "{text}");
    }
}
