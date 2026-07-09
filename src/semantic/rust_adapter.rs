//! The Rust semantic backend: `rust-analyzer scip` for cross-file identity/resolution/types,
//! translated into the engine-neutral model.
//!
//! The heavy, environment-dependent step — invoking `rust-analyzer scip` — is isolated in
//! [`RustAdapter::analyze`]. The translation from a SCIP index into the model is a pure function
//! ([`translate_index`]) so it is unit-testable without a live analyzer.

use std::path::Path;
use std::process::Command;

use scip::types as scip_types;

use super::model::{AnalyzerProvenance, ExtractedIndex, normalize};
pub use super::scip::translate_index;
use super::{Capabilities, SemanticEngine, SemanticError};

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
    /// Fails if the executable cannot be run — a backend whose analyzer is unavailable is not
    /// usable, and the caller learns so explicitly rather than by a later opaque failure.
    pub fn new(executable: impl Into<String>) -> Result<Self, SemanticError> {
        let executable = executable.into();
        let output = Command::new(&executable)
            .arg("--version")
            .output()
            .map_err(|e| SemanticError::Unavailable(format!("cannot run {executable}: {e}")))?;
        if !output.status.success() {
            return Err(SemanticError::Unavailable(format!("{executable} --version failed")));
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
            .map_err(|e| SemanticError::Unavailable(format!("cannot run {} scip: {e}", self.executable)))?;
        if !status.success() {
            return Err(SemanticError::Analysis(format!(
                "rust-analyzer scip exited with status {status}"
            )));
        }
        let bytes = std::fs::read(&output_path)
            .map_err(|e| SemanticError::Analysis(format!("cannot read {}: {e}", output_path.display())))?;
        let index: scip_types::Index = protobuf::Message::parse_from_bytes(&bytes)
            .map_err(|e| SemanticError::Analysis(format!("cannot parse SCIP index: {e}")))?;
        let mut extracted = translate_index(&index, &self.provenance());
        extracted.library_roots = library_roots(project_root);
        Ok(normalize(extracted))
    }
}

/// The build system's authoritative library-target roots for the project: package name →
/// workspace-relative path of the package's library-target root, from
/// `cargo metadata --no-deps`.
///
/// This is enrichment, never a build gate: any failure — the command cannot be spawned, exits
/// non-zero, or emits unparsable output — degrades to an empty map, and downstream consumers fall
/// back to their typed refusals.
fn library_roots(project_root: &Path) -> std::collections::BTreeMap<String, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(project_root)
        .output();
    match output {
        Ok(out) if out.status.success() => parse_library_roots(&out.stdout),
        _ => Default::default(),
    }
}

/// Parse `cargo metadata --no-deps --format-version 1` output into the package → library-root map.
///
/// External boundary: parsing is defensive — wholly unusable input yields an empty map, and a
/// package with a surprising shape is skipped rather than failing the rest. A target counts as the
/// library when any of its kinds is a library kind; bins, tests, examples, and benches are not. The
/// absolute `src_path` is made workspace-relative against the metadata's own `workspace_root` so it
/// matches SCIP document paths; a target rooted outside the workspace is skipped.
fn parse_library_roots(metadata_json: &[u8]) -> std::collections::BTreeMap<String, String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(metadata_json) else {
        return Default::default();
    };
    let Some(workspace_root) = value.get("workspace_root").and_then(|v| v.as_str()) else {
        return Default::default();
    };
    let Some(packages) = value.get("packages").and_then(|v| v.as_array()) else {
        return Default::default();
    };

    let mut roots = std::collections::BTreeMap::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(targets) = package.get("targets").and_then(|v| v.as_array()) else {
            continue;
        };
        for target in targets {
            let is_library = target
                .get("kind")
                .and_then(|v| v.as_array())
                .is_some_and(|kinds| kinds.iter().filter_map(|k| k.as_str()).any(is_library_kind));
            if !is_library {
                continue;
            }
            let Some(src_path) = target.get("src_path").and_then(|v| v.as_str()) else {
                continue;
            };
            let Ok(relative) = Path::new(src_path).strip_prefix(workspace_root) else {
                continue;
            };
            roots.insert(name.to_string(), relative.to_string_lossy().replace('\\', "/"));
            break; // a package has at most one library target
        }
    }
    roots
}

/// Whether a cargo target kind names a library artifact (the target a package-name path resolves
/// to), as opposed to a bin, test, example, or bench.
fn is_library_kind(kind: &str) -> bool {
    matches!(kind, "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cargo-metadata parser extracts each package's library-target root, workspace-relative:
    // a lib target maps; bin/test/bench targets do not; a bin-only package contributes nothing.
    #[test]
    fn parse_library_roots_extracts_lib_targets_workspace_relative() {
        let json = r#"{
            "workspace_root": "/ws/proj",
            "packages": [
                {
                    "name": "silent-cartographer",
                    "targets": [
                        {"name": "silent-cartographer", "kind": ["lib"], "src_path": "/ws/proj/src/lib.rs"},
                        {"name": "c10r", "kind": ["bin"], "src_path": "/ws/proj/src/main.rs"}
                    ]
                },
                {
                    "name": "helper-bin",
                    "targets": [
                        {"name": "helper-bin", "kind": ["bin"], "src_path": "/ws/proj/tools/src/main.rs"}
                    ]
                }
            ]
        }"#;
        let roots = parse_library_roots(json.as_bytes());
        assert_eq!(roots.len(), 1, "only the lib target maps: {roots:?}");
        assert_eq!(
            roots.get("silent-cartographer").map(String::as_str),
            Some("src/lib.rs"),
            "the lib root is workspace-relative: {roots:?}"
        );
    }

    // A proc-macro target is a library artifact; a package rooted outside the workspace is skipped.
    #[test]
    fn parse_library_roots_covers_proc_macros_and_skips_out_of_workspace_targets() {
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
        let roots = parse_library_roots(json.as_bytes());
        assert_eq!(
            roots.get("my-macros").map(String::as_str),
            Some("macros/src/lib.rs"),
            "a proc-macro target is a library: {roots:?}"
        );
        assert!(
            !roots.contains_key("vendored"),
            "a target outside the workspace root is skipped: {roots:?}"
        );
    }

    // The parser is an external boundary: unusable input degrades to an empty map, never an error.
    #[test]
    fn parse_library_roots_degrades_to_empty_on_malformed_input() {
        assert!(parse_library_roots(b"not json at all").is_empty());
        assert!(parse_library_roots(b"{}").is_empty(), "missing fields degrade");
        assert!(
            parse_library_roots(br#"{"workspace_root": "/ws", "packages": "not-an-array"}"#).is_empty(),
            "wrong shapes degrade"
        );
    }

    // Metadata is enrichment, not a build gate: a project directory cargo cannot run in yields an
    // empty map, no error.
    #[test]
    fn library_roots_degrade_to_empty_when_cargo_cannot_run() {
        let roots = library_roots(Path::new("/nonexistent/definitely-not-a-project"));
        assert!(roots.is_empty(), "spawn failure degrades to an empty map: {roots:?}");
    }
}
