//! The Python semantic backend: `scip-python index` for cross-file identity/resolution, translated
//! into the engine-neutral model through the shared [`super::scip`] translation.
//!
//! Resolution meaning depends on the interpreter environment the index ran against, so the adapter
//! resolves an environment explicitly — explicit path, then the activated `$VIRTUAL_ENV`, then a
//! `.venv`/`venv` directory under the workspace root — and refuses with a typed error when none
//! resolves. There is no silent fallback to a system interpreter: indexing against the wrong
//! environment produces confidently wrong third-party resolution. The resolved environment's facts
//! (interpreter version, path, installed-package fingerprint) ride the produced index as declared
//! provenance so staleness can compare them.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::model::{AnalyzerProvenance, EnvironmentFacts, ExtractedIndex, normalize};
use super::scip::translate_index;
use super::{Capabilities, SemanticEngine, SemanticError};

/// The Python adapter, wrapping `scip-python index` and translating its output into the model.
///
/// The invocation always passes `--project-name <name> --project-version 0`: probing scip-python
/// (0.6.6) showed its own name/version inference crashes on a non-git directory whose
/// `pyproject.toml` lacks a `[project]` table, and degenerates the package name to the literal `.`
/// on the same shape inside a git repo — so the adapter never relies on the inference. The constant
/// version is safe because identity projection reads only the package name, never its version.
#[derive(Debug)]
pub struct PythonAdapter {
    /// The `scip-python` executable to invoke.
    executable: String,
    /// The tool version, discovered once at construction.
    version: String,
    /// The resolved interpreter environment the index runs against.
    environment: PathBuf,
    /// The project name stamped into every emitted symbol's package segment (the workspace
    /// identity), passed unconditionally as `--project-name`.
    project_name: String,
}

/// The install guidance appended to every scip-python unavailability error.
const INSTALL_HINT: &str = "install scip-python with `npm install -g @sourcegraph/scip-python`";

impl PythonAdapter {
    /// The analyzer name this adapter reports as provenance.
    pub fn analyzer_name() -> &'static str {
        "scip-python"
    }

    /// Construct an adapter for the given `scip-python` executable over a resolved environment,
    /// discovering the tool's version.
    ///
    /// Fails with a typed [`SemanticError::Unavailable`] naming the tool and how to install it when
    /// the executable cannot be run — a backend whose indexer is missing is not usable, and the
    /// caller learns so explicitly.
    pub fn new(
        executable: impl Into<String>,
        environment: PathBuf,
        project_name: impl Into<String>,
    ) -> Result<Self, SemanticError> {
        let executable = executable.into();
        let version = Self::discover_version(&executable)?;
        Ok(Self {
            executable,
            version,
            environment,
            project_name: project_name.into(),
        })
    }

    /// Discover the scip-python version by running `<executable> --version`.
    ///
    /// Fails with a typed [`SemanticError::Unavailable`] naming the tool and how to install it when
    /// the executable cannot be run.
    pub fn discover_version(executable: &str) -> Result<String, SemanticError> {
        let output = Command::new(executable).arg("--version").output().map_err(|e| {
            SemanticError::Unavailable(format!("cannot run scip-python ({executable}): {e}; {INSTALL_HINT}"))
        })?;
        if !output.status.success() {
            return Err(SemanticError::Unavailable(format!(
                "scip-python ({executable}) --version failed; {INSTALL_HINT}"
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run the adapter's real `scip-python index` invocation over `project_root`, writing the raw
    /// SCIP index to `output`.
    ///
    /// This is the exact command [`SemanticEngine::analyze`] runs — same flags, same activated
    /// environment — exposed so a committed fixture index can be generated through the adapter
    /// rather than through an ad-hoc tool call that could drift from it.
    pub fn write_scip_index(&self, project_root: &Path, output: &Path) -> Result<(), SemanticError> {
        // Run with the resolved environment's interpreter active: its bin/ first on PATH and
        // VIRTUAL_ENV set for the child, exactly what an activated venv provides.
        let child_path = prepend_to_path(&self.environment.join("bin"))?;
        let status = Command::new(&self.executable)
            .arg("index")
            // Unconditional: scip-python's own name/version inference crashes or degenerates on
            // manifest shapes without a `[project]` table (see the type-level doc-comment).
            .arg("--project-name")
            .arg(&self.project_name)
            .arg("--project-version")
            .arg("0")
            .arg("--output")
            .arg(output)
            .current_dir(project_root)
            .env("VIRTUAL_ENV", &self.environment)
            .env("PATH", child_path)
            .status()
            .map_err(|e| {
                SemanticError::Unavailable(format!(
                    "cannot run scip-python ({}) index: {e}; {INSTALL_HINT}",
                    self.executable
                ))
            })?;
        if !status.success() {
            return Err(SemanticError::Analysis(format!(
                "scip-python index exited with status {status}"
            )));
        }
        Ok(())
    }
}

impl SemanticEngine for PythonAdapter {
    fn provenance(&self) -> AnalyzerProvenance {
        AnalyzerProvenance {
            analyzer_name: Self::analyzer_name().to_string(),
            analyzer_version: self.version.clone(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        // Like the Rust adapter: SCIP paired with tree-sitter for enclosure, a full batch index —
        // enclosure support and base-index eligibility; not a live backend.
        Capabilities::none().with_enclosure().with_base_index()
    }

    fn analyze(&self, project_root: &Path) -> Result<ExtractedIndex, SemanticError> {
        // Gather the environment facts first: an environment whose facts cannot be read would
        // produce an index with unverifiable provenance, so it refuses up front.
        let facts = environment_facts(&self.environment)?;

        let output_path = std::env::temp_dir().join(format!("c10r-scip-python-{}.scip", std::process::id()));
        self.write_scip_index(project_root, &output_path)?;
        let bytes = std::fs::read(&output_path)
            .map_err(|e| SemanticError::Analysis(format!("cannot read {}: {e}", output_path.display())))?;
        let index: scip::types::Index = protobuf::Message::parse_from_bytes(&bytes)
            .map_err(|e| SemanticError::Analysis(format!("cannot parse SCIP index: {e}")))?;
        // library_roots stays empty at launch: Python supplies no package→root map yet, so twin
        // groups fall back to the refusal path by construction.
        let mut extracted = translate_index(&index, &self.provenance());
        extracted.environment = Some(facts);
        Ok(normalize(extracted))
    }
}

/// The child-process PATH with `bin` prepended, preserving the rest of the current PATH.
fn prepend_to_path(bin: &Path) -> Result<std::ffi::OsString, SemanticError> {
    let mut parts = vec![bin.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts).map_err(|e| SemanticError::Analysis(format!("cannot build child PATH: {e}")))
}

/// Gather the declared facts of a resolved environment: interpreter version, path, and the
/// installed-package fingerprint over its site-packages.
pub fn environment_facts(environment: &Path) -> Result<EnvironmentFacts, SemanticError> {
    let interpreter = environment.join("bin").join("python");
    let output = Command::new(&interpreter).arg("--version").output().map_err(|e| {
        SemanticError::Environment(format!(
            "cannot run the environment's interpreter {}: {e}",
            interpreter.display()
        ))
    })?;
    if !output.status.success() {
        return Err(SemanticError::Environment(format!(
            "{} --version failed",
            interpreter.display()
        )));
    }
    // Python prints its version on stdout (stderr on some older builds); take whichever spoke.
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let interpreter_version = if stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        stdout
    };

    let site_packages = site_packages_dir(environment)?;
    let package_fingerprint = package_fingerprint(&site_packages)?;

    Ok(EnvironmentFacts {
        interpreter_version,
        environment_path: environment.display().to_string(),
        package_fingerprint,
    })
}

/// The environment's site-packages directory: `<env>/lib/python<X.Y>/site-packages`.
fn site_packages_dir(environment: &Path) -> Result<PathBuf, SemanticError> {
    let lib = environment.join("lib");
    let entries = std::fs::read_dir(&lib)
        .map_err(|e| SemanticError::Environment(format!("cannot read {}: {e}", lib.display())))?;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("python") {
            let candidate = entry.path().join("site-packages");
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }
    Err(SemanticError::Environment(format!(
        "no site-packages directory found under {}",
        lib.display()
    )))
}

/// Resolve the Python environment for a workspace, in fixed precedence: an explicit path argument,
/// then the activated `$VIRTUAL_ENV` (its value passed in by the caller — the process environment is
/// read only at the outer edge, keeping this function pure and testable), then a `.venv/` or `venv/`
/// directory under the workspace root.
///
/// Never falls back to a system interpreter: when none resolves, the typed refusal names all three
/// mechanisms.
pub fn resolve_environment(
    explicit: Option<&Path>,
    virtual_env: Option<&Path>,
    workspace_root: &Path,
) -> Result<PathBuf, SemanticError> {
    // An explicitly named environment is the user's word: it wins outright, and a missing one is a
    // refusal to surface, never a reason to silently try the next mechanism.
    if let Some(path) = explicit {
        if path.is_dir() {
            return Ok(path.to_path_buf());
        }
        return Err(SemanticError::Environment(format!(
            "the explicit environment path {} is not a directory",
            path.display()
        )));
    }
    // An activated environment is taken as-is: activation is the user's declaration of intent.
    if let Some(path) = virtual_env {
        return Ok(path.to_path_buf());
    }
    for candidate in [".venv", "venv"] {
        let dir = workspace_root.join(candidate);
        if dir.is_dir() {
            return Ok(dir);
        }
    }
    Err(SemanticError::Environment(format!(
        "no Python environment resolved for {}: pass an explicit environment path, activate one \
         (set $VIRTUAL_ENV), or create a .venv/ (or venv/) directory in the workspace root",
        workspace_root.display()
    )))
}

/// The installed-package fingerprint of a site-packages directory: a SHA-256 over the sorted
/// `*.dist-info` directory names, hex-encoded.
///
/// Sorting makes the fingerprint a function of the installed set, not enumeration order; installs,
/// upgrades, and removals all change a dist-info name, so each changes the fingerprint.
pub fn package_fingerprint(site_packages: &Path) -> Result<String, SemanticError> {
    let entries = std::fs::read_dir(site_packages)
        .map_err(|e| SemanticError::Environment(format!("cannot read {}: {e}", site_packages.display())))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| SemanticError::Environment(format!("cannot read {}: {e}", site_packages.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".dist-info") && entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            names.push(name);
        }
    }
    names.sort();

    let mut hasher = Sha256::new();
    for name in &names {
        hasher.update(name.as_bytes());
        hasher.update([0u8]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonicalized tempdir path (macOS tempdirs live behind a `/private` symlink; comparing
    /// resolved paths keeps the assertions byte-exact).
    fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        (dir, path)
    }

    #[test]
    fn env_resolution_prefers_explicit_then_virtual_env_then_dot_venv() {
        let (_root_guard, root) = canonical_tempdir();
        std::fs::create_dir(root.join(".venv")).unwrap();
        std::fs::create_dir(root.join("venv")).unwrap();
        let (_explicit_guard, explicit) = canonical_tempdir();
        let (_active_guard, active) = canonical_tempdir();

        // All three mechanisms available: the explicit path wins.
        let resolved = resolve_environment(Some(&explicit), Some(&active), &root).unwrap();
        assert_eq!(resolved, explicit, "explicit path outranks $VIRTUAL_ENV and .venv");

        // No explicit path: the activated $VIRTUAL_ENV wins over the workspace's .venv.
        let resolved = resolve_environment(None, Some(&active), &root).unwrap();
        assert_eq!(resolved, active, "$VIRTUAL_ENV outranks .venv");

        // Neither: the workspace's .venv is found, preferred over venv.
        let resolved = resolve_environment(None, None, &root).unwrap();
        assert_eq!(resolved, root.join(".venv"), ".venv outranks venv");

        // With .venv gone, venv resolves.
        std::fs::remove_dir(root.join(".venv")).unwrap();
        let resolved = resolve_environment(None, None, &root).unwrap();
        assert_eq!(resolved, root.join("venv"), "venv is the final directory candidate");
    }

    // (Scenario: Unresolvable environment refuses with guidance — adapter-level half.)
    #[test]
    fn env_resolution_refuses_when_nothing_resolves() {
        let (_root_guard, root) = canonical_tempdir();
        let err = resolve_environment(None, None, &root).expect_err("nothing resolves");
        assert!(
            matches!(err, SemanticError::Environment(_)),
            "typed environment refusal, got: {err:?}"
        );
        let message = err.to_string();
        for mechanism in ["explicit", "VIRTUAL_ENV", ".venv"] {
            assert!(
                message.contains(mechanism),
                "the refusal names the {mechanism} mechanism: {message}"
            );
        }
    }

    #[test]
    fn package_fingerprint_changes_when_dist_info_set_changes() {
        let (_a_guard, site_a) = canonical_tempdir();
        std::fs::create_dir(site_a.join("foo-1.0.dist-info")).unwrap();
        std::fs::create_dir(site_a.join("bar-2.0.dist-info")).unwrap();
        let before = package_fingerprint(&site_a).unwrap();

        // Adding one distribution changes the fingerprint.
        std::fs::create_dir(site_a.join("baz-3.0.dist-info")).unwrap();
        let after = package_fingerprint(&site_a).unwrap();
        assert_ne!(before, after, "an install changes the fingerprint");

        // The same set created in a different order fingerprints identically.
        let (_b_guard, site_b) = canonical_tempdir();
        std::fs::create_dir(site_b.join("baz-3.0.dist-info")).unwrap();
        std::fs::create_dir(site_b.join("foo-1.0.dist-info")).unwrap();
        std::fs::create_dir(site_b.join("bar-2.0.dist-info")).unwrap();
        let reordered = package_fingerprint(&site_b).unwrap();
        assert_eq!(after, reordered, "the fingerprint is a function of the set, not order");
    }

    // (Scenario: Missing indexer tool refuses with guidance — adapter half.) The tool is looked up
    // at an executable path inside an empty directory, so it is deterministically absent.
    #[test]
    fn python_adapter_reports_unavailable_when_tool_missing() {
        let (_empty_guard, empty) = canonical_tempdir();
        let missing_tool = empty.join("scip-python");
        let err = PythonAdapter::new(missing_tool.to_string_lossy().into_owned(), empty.clone(), "fixture")
            .expect_err("tool is absent");
        assert!(
            matches!(err, SemanticError::Unavailable(_)),
            "typed unavailability, got: {err:?}"
        );
        let message = err.to_string();
        assert!(message.contains("scip-python"), "the error names the tool: {message}");
        assert!(
            message.contains("npm install -g @sourcegraph/scip-python"),
            "the error says how to install it: {message}"
        );
    }
}
