//! The Python semantic backend: `ty-scip index` or `scip-python index` for cross-file
//! identity/resolution, translated into the engine-neutral model through the shared [`super::scip`]
//! translation.
//!
//! Resolution meaning depends on the interpreter environment the index ran against, so the adapter
//! resolves an environment explicitly — explicit path, then the activated `$VIRTUAL_ENV`, then a
//! `.venv`/`venv` directory under the workspace root — and refuses with a typed error when none
//! resolves. There is no silent fallback to a system interpreter: indexing against the wrong
//! environment produces confidently wrong third-party resolution. The resolved environment's facts
//! (interpreter version, path, installed-package fingerprint) ride the produced index as declared
//! provenance so staleness can compare them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::model::{AnalyzerProvenance, EnvironmentFacts, ExtractedIndex, SymbolKind, normalize};
use super::probe::{PROBE_DEADLINE, ProbeOutcome, run_bounded};
use super::scip::translate_index;
use super::{Capabilities, SemanticEngine, SemanticError};
use crate::identity::SegmentKind;

/// The Python indexers the adapter can drive. Both accept the same `index` invocation and emit SCIP;
/// they differ in executable, recorded analyzer name, install guidance, and how a failed run is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonIndexer {
    /// `ty-scip`, built on the ty type checker.
    TyScip,
    /// Sourcegraph's `scip-python`.
    ScipPython,
}

impl PythonIndexer {
    /// The executable name looked up on `PATH`.
    pub fn executable(self) -> &'static str {
        match self {
            Self::TyScip => "ty-scip",
            Self::ScipPython => "scip-python",
        }
    }

    /// The analyzer name recorded as provenance of every index this indexer produces.
    pub fn analyzer_name(self) -> &'static str {
        match self {
            Self::TyScip => "ty-scip",
            Self::ScipPython => "scip-python",
        }
    }

    /// The install guidance named when this indexer cannot be run.
    pub fn install_hint(self) -> &'static str {
        match self {
            Self::TyScip => "install ty-scip with `uv tool install ty-scip`",
            Self::ScipPython => "install scip-python with `npm install -g @sourcegraph/scip-python`",
        }
    }

    /// The command that installs this indexer, named on its own when the indexer is present but broken.
    pub fn install_command(self) -> &'static str {
        match self {
            Self::TyScip => "uv tool install ty-scip",
            Self::ScipPython => "npm install -g @sourcegraph/scip-python",
        }
    }

    /// The indexer that records `name` as its analyzer name, if any.
    pub fn from_analyzer_name(name: &str) -> Option<Self> {
        [Self::TyScip, Self::ScipPython]
            .into_iter()
            .find(|indexer| indexer.analyzer_name() == name)
    }
}

/// Choose the indexer a build or freshness check runs and probe its version.
///
/// The candidate is the explicit choice, else the indexer recorded by the existing store, else
/// scip-python. Only the candidate is probed: when it is absent or unresponsive the probe's error
/// passes through, and the other indexer is never tried, so a host's installed tools cannot change
/// which indexer a store is built by.
pub fn select_python_indexer(
    explicit: Option<PythonIndexer>,
    recorded: Option<PythonIndexer>,
    probe: impl Fn(PythonIndexer) -> Result<String, SemanticError>,
) -> Result<(PythonIndexer, String), SemanticError> {
    let candidate = explicit.or(recorded).unwrap_or(PythonIndexer::ScipPython);
    probe(candidate).map(|version| (candidate, version))
}

/// The Python adapter, wrapping `<indexer> index` and translating its output into the model.
///
/// The invocation always passes `--project-name <name> --project-version 0`: probing scip-python
/// (0.6.6) showed its own name/version inference crashes on a non-git directory whose
/// `pyproject.toml` lacks a `[project]` table, and degenerates the package name to the literal `.`
/// on the same shape inside a git repo — so the adapter never relies on the inference. The constant
/// version is safe because identity projection reads only the package name, never its version.
#[derive(Debug)]
pub struct PythonAdapter {
    /// The indexer this adapter drives.
    indexer: PythonIndexer,
    /// The indexer's executable to invoke.
    executable: String,
    /// The tool version, discovered once at construction.
    version: String,
    /// The resolved interpreter environment the index runs against.
    environment: PathBuf,
    /// The project name stamped into every emitted symbol's package segment (the workspace
    /// identity), passed unconditionally as `--project-name`.
    project_name: String,
}

/// The consequence and remedy for an environment directory that cannot be read.
const BROKEN_ENVIRONMENT: &str = "The environment looks broken; pass `--environment <virtualenv>` or recreate it.";

impl PythonAdapter {
    /// Construct an adapter for `indexer`, run through `executable` (production callers pass
    /// [`PythonIndexer::executable`]), over a resolved environment, discovering the tool's version.
    ///
    /// Fails with a typed [`SemanticError::Unavailable`] naming the tool and how to install it when
    /// the executable cannot be run — a backend whose indexer is missing is not usable, and the
    /// caller learns so explicitly.
    pub fn new(
        indexer: PythonIndexer,
        executable: impl Into<String>,
        environment: PathBuf,
        project_name: impl Into<String>,
    ) -> Result<Self, SemanticError> {
        let executable = executable.into();
        let version = Self::discover_version(indexer, &executable)?;
        Ok(Self {
            indexer,
            executable,
            version,
            environment,
            project_name: project_name.into(),
        })
    }

    /// Discover the indexer's version by running `<executable> --version`.
    ///
    /// The probe is bounded: a missing executable is [`SemanticError::Unavailable`] naming the tool
    /// and how to install it, and one that is present but does not answer within the probe deadline is
    /// [`SemanticError::Timeout`] — present but unresponsive, distinct from missing.
    pub fn discover_version(indexer: PythonIndexer, executable: &str) -> Result<String, SemanticError> {
        let (name, hint) = (indexer.analyzer_name(), indexer.install_hint());
        let mut command = Command::new(executable);
        command.arg("--version");
        let capture = match run_bounded(command, PROBE_DEADLINE) {
            Ok(ProbeOutcome::Completed(capture)) => capture,
            Ok(ProbeOutcome::TimedOut) => {
                return Err(SemanticError::Timeout(format!(
                    "{name} ({executable}) did not answer `--version` within {} s, so no index was built. \
                     Run `{executable} --version` to see why.",
                    PROBE_DEADLINE.as_secs()
                )));
            }
            Err(e) => {
                return Err(SemanticError::Unavailable(format!(
                    "cannot run {name} ({executable}), so no index was built; {hint}. The system reported: {e}"
                )));
            }
        };
        if !capture.status.success() {
            return Err(SemanticError::Unavailable(format!(
                "`{executable} --version` failed ({}), so no index was built. If the installation is damaged, reinstall it with `{}`.",
                capture.failure_detail(),
                indexer.install_command()
            )));
        }
        Ok(String::from_utf8_lossy(&capture.stdout).trim().to_string())
    }

    /// Run the adapter's real `<indexer> index` invocation over `project_root`, writing the raw
    /// SCIP index to `output`.
    ///
    /// This is the exact command [`SemanticEngine::analyze`] runs — same flags, same activated
    /// environment — exposed so a committed fixture index can be generated through the adapter
    /// rather than through an ad-hoc tool call that could drift from it.
    pub fn write_scip_index(&self, project_root: &Path, output: &Path) -> Result<(), SemanticError> {
        // Run with the resolved environment's interpreter active: its bin/ first on PATH and
        // VIRTUAL_ENV set for the child, exactly what an activated venv provides.
        let child_path = prepend_to_path(&self.environment.join("bin"))?;
        let name = self.indexer.analyzer_name();
        let unavailable = |e: std::io::Error| {
            SemanticError::Unavailable(format!(
                "cannot run {name} ({}) index, so no index was built; {}. The system reported: {e}",
                self.executable,
                self.indexer.install_hint()
            ))
        };
        let mut command = Command::new(&self.executable);
        command
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
            .env("PATH", child_path);
        if self.indexer == PythonIndexer::ScipPython {
            let status = command.status().map_err(unavailable)?;
            if !status.success() {
                return Err(SemanticError::Analysis(format!(
                    "`{} index` failed ({status}). Its own error output is printed above.",
                    self.executable
                )));
            }
            return Ok(());
        }
        // ty-scip: a refusal is read from standard error, which is replayed afterwards so the user
        // still sees the indexer's own diagnostics.
        let run = command.stdout(Stdio::inherit()).output().map_err(unavailable)?;
        let stderr = String::from_utf8_lossy(&run.stderr);
        eprint!("{stderr}");
        if !run.status.success() {
            return Err(rejected_file(&stderr, project_root).unwrap_or_else(|| {
                SemanticError::Analysis(format!(
                    "`{} index` failed ({}). Its own error output is printed above.",
                    self.executable, run.status
                ))
            }));
        }
        Ok(())
    }
}

/// The line prefix ty-scip prints when it refuses a file that has no module name in the project.
const REJECTED_FILE_PREFIX: &str = "project file has no importable module name: ";

/// Read a ty-scip failure's standard error for a file the indexer refused, resolving a relative path
/// against `project_root`. `None` when the text names no such file.
fn rejected_file(stderr: &str, project_root: &Path) -> Option<SemanticError> {
    let named = stderr
        .lines()
        .find_map(|line| line.split_once(REJECTED_FILE_PREFIX).map(|(_, path)| path.trim()))?;
    let path = project_root.join(named);
    let remedy = rejected_file_remedy(&path, named);
    Some(SemanticError::RejectedFile { path, remedy })
}

/// The configuration change that stops ty-scip refusing `path`: a file whose leading comment lines
/// hold a PEP 723 block is a standalone script, excluded wholesale by `exclude-scripts`; any other
/// file (a path component that is not an identifier, say) is excluded by name.
fn rejected_file_remedy(path: &Path, named: &str) -> String {
    let is_script = std::fs::read_to_string(path).is_ok_and(|text| {
        text.lines()
            .map(str::trim)
            .take_while(|line| line.is_empty() || line.starts_with('#'))
            .any(|line| line == "# /// script")
    });
    if is_script {
        "set `exclude-scripts = true` under `[src]` in `ty.toml`, or under `[tool.ty.src]` in `pyproject.toml`"
            .to_string()
    } else {
        format!(
            "add `{}` to `exclude` under `[src]` in `ty.toml`, or under `[tool.ty.src]` in `pyproject.toml`",
            named
        )
    }
}

/// Classify every symbol whose descriptor carries scip-python's module shape as `SymbolKind::Module`.
///
/// scip-python does not report a module kind in `SymbolInformation`, so a module's descriptor arrives
/// with a meta-suffixed `__init__` terminal segment following the module's own namespace segment —
/// e.g. `pkg.shapes` (module, `SegmentKind::Module`) followed by `__init__` (`SegmentKind::Meta`). This
/// enrichment recognizes that shape once, at the adapter boundary, so every downstream consumer (the
/// join's module-name rule, module-to-document bookkeeping, `get`/`status` display) keys off the one
/// `module` kind vocabulary instead of re-deriving the shape from the descriptor.
///
/// A second, unrelated module shape needs no enrichment here: a reference occurrence at a bare
/// `__name__`/`__file__` token resolves to a symbol string ending in a single `Namespace`-suffixed
/// descriptor segment with no `__init__` — e.g. `` `tests.test_basic`/ `` (one segment, name
/// `tests.test_basic`, `Suffix::Namespace`), verified against a live scip-python (0.6.6) index. That
/// shape already classifies as `SymbolKind::Module` through [`super::scip::symbol_kind_from`]'s
/// existing `Namespace → SegmentKind::Module` mapping, which runs before this function during
/// translation — so it never reaches this enrichment as anything but a module. The self-name
/// alignment rule compares such an occurrence's resolved symbol against the containing document's own
/// module by descriptor identity; no adapter change was needed for it.
pub fn classify_module_kinds(index: &mut ExtractedIndex) {
    for symbol in &mut index.symbols {
        let Some(descriptor) = &symbol.descriptor else {
            continue;
        };
        let is_module_shape = descriptor.segments.len() >= 2
            && descriptor
                .segments
                .last()
                .is_some_and(|seg| seg.name == "__init__" && seg.kind == SegmentKind::Meta);
        if is_module_shape {
            symbol.kind = SymbolKind::Module;
        }
    }
}

impl SemanticEngine for PythonAdapter {
    fn provenance(&self) -> AnalyzerProvenance {
        AnalyzerProvenance {
            analyzer_name: self.indexer.analyzer_name().to_string(),
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

        let output_path = std::env::temp_dir().join(format!(
            "c10r-{}-{}.scip",
            self.indexer.analyzer_name(),
            std::process::id()
        ));
        self.write_scip_index(project_root, &output_path)?;
        let bytes = std::fs::read(&output_path).map_err(|e| {
            SemanticError::Analysis(format!(
                "`{} index` finished, but its output {} could not be read ({e}).",
                self.executable,
                output_path.display()
            ))
        })?;
        let index: scip::types::Index = protobuf::Message::parse_from_bytes(&bytes).map_err(|e| {
            SemanticError::Analysis(format!(
                "`{} index` finished, but its output {} is not a valid SCIP index ({e}).",
                self.executable,
                output_path.display()
            ))
        })?;
        // library_roots stays empty at launch: Python supplies no package→root map yet, so twin
        // groups fall back to the refusal path by construction.
        let mut extracted = translate_index(&index, &self.provenance());
        extracted.environment = Some(facts);
        classify_module_kinds(&mut extracted);
        Ok(normalize(extracted))
    }
}

/// The child-process PATH with `bin` prepended, preserving the rest of the current PATH.
fn prepend_to_path(bin: &Path) -> Result<std::ffi::OsString, SemanticError> {
    let mut parts = vec![bin.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }
    // The existing entries come from splitting PATH, so only `bin` can hold the separator.
    std::env::join_paths(parts).map_err(|_| {
        SemanticError::Analysis(format!(
            "could not put {} on the PATH for scip-python, because the path contains the PATH separator, so no index was built. Move the environment to a path without that character, or pass `--environment <virtualenv>`.",
            bin.display()
        ))
    })
}

/// Gather the declared facts of a resolved environment: interpreter version, path, and the
/// installed-package fingerprint over its site-packages.
pub fn environment_facts(environment: &Path) -> Result<EnvironmentFacts, SemanticError> {
    let interpreter = environment.join("bin").join("python");
    let mut command = Command::new(&interpreter);
    command.arg("--version");
    // Bounded probe: a present-but-unresponsive interpreter is a distinct timeout the freshness
    // consumer discloses, never an indefinite hang on a query.
    let capture = match run_bounded(command, PROBE_DEADLINE) {
        Ok(ProbeOutcome::Completed(capture)) => capture,
        Ok(ProbeOutcome::TimedOut) => {
            return Err(SemanticError::Timeout(format!(
                "the environment's interpreter {} did not answer `--version` within {} s. \
                 Run it by hand to see why, or pass a different `--environment <virtualenv>`.",
                interpreter.display(),
                PROBE_DEADLINE.as_secs()
            )));
        }
        Err(e) => {
            return Err(SemanticError::Environment(format!(
                "could not run the environment's interpreter {} ({e}). \
                 Pass `--environment <virtualenv>`, or recreate the environment.",
                interpreter.display()
            )));
        }
    };
    if !capture.status.success() {
        return Err(SemanticError::Environment(format!(
            "the environment's interpreter {} --version failed ({}). \
             Pass `--environment <virtualenv>`, or recreate the environment.",
            interpreter.display(),
            capture.failure_detail()
        )));
    }
    // Python prints its version on stdout (stderr on some older builds); take whichever spoke.
    let stdout = String::from_utf8_lossy(&capture.stdout).trim().to_string();
    let interpreter_version = if stdout.is_empty() {
        String::from_utf8_lossy(&capture.stderr).trim().to_string()
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
    let entries = std::fs::read_dir(&lib).map_err(|e| {
        SemanticError::Environment(format!("could not read {} ({e}). {BROKEN_ENVIRONMENT}", lib.display()))
    })?;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("python") {
            let candidate = entry.path().join("site-packages");
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }
    Err(SemanticError::Environment(format!(
        "{} holds no python*/site-packages directory, so {} does not look like a virtualenv. \
         Pass `--environment <virtualenv>`.",
        lib.display(),
        environment.display()
    )))
}

/// Resolve the Python environment for a workspace, in fixed precedence: an explicit path argument,
/// then the activated `$VIRTUAL_ENV` (its value passed in by the caller — the process environment is
/// read only at the outer edge, keeping this function pure and testable), then a `.venv/` or `venv/`
/// directory under the workspace root.
///
/// Returns the canonical absolute path, so the same environment resolves to the same text regardless
/// of how the workspace root or an explicit path was spelled; a path that cannot be canonicalized is
/// returned as resolved.
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
            return Ok(canonicalize_or(path));
        }
        return Err(SemanticError::Environment(format!(
            "the explicit environment path {} is not a directory, so no index was built. Pass `--environment <virtualenv>` with an existing directory.",
            path.display()
        )));
    }
    // An activated environment is taken as-is: activation is the user's declaration of intent.
    if let Some(path) = virtual_env {
        return Ok(canonicalize_or(path));
    }
    for candidate in [".venv", "venv"] {
        let dir = workspace_root.join(candidate);
        if dir.is_dir() {
            return Ok(canonicalize_or(&dir));
        }
    }
    Err(SemanticError::Environment(format!(
        "no Python environment resolved for {}, so no index was built. Pass `--environment <virtualenv>`, activate one \
         (set $VIRTUAL_ENV), or create a .venv/ (or venv/) directory in the workspace root.",
        workspace_root.display()
    )))
}

/// `path` canonicalized, or `path` itself when canonicalization fails.
fn canonicalize_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The installed-package fingerprint of a site-packages directory: a SHA-256 over the sorted
/// `*.dist-info` directory names, hex-encoded.
///
/// Sorting makes the fingerprint a function of the installed set, not enumeration order; installs,
/// upgrades, and removals all change a dist-info name, so each changes the fingerprint.
pub fn package_fingerprint(site_packages: &Path) -> Result<String, SemanticError> {
    let entries = std::fs::read_dir(site_packages).map_err(|e| {
        SemanticError::Environment(format!(
            "could not read {} ({e}). {BROKEN_ENVIRONMENT}",
            site_packages.display()
        ))
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            SemanticError::Environment(format!(
                "could not read {} ({e}). {BROKEN_ENVIRONMENT}",
                site_packages.display()
            ))
        })?;
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
    use crate::identity::{Descriptor, DescriptorSegment};
    use crate::semantic::model::{ExtractedSymbol, SymbolClass};

    /// A canonicalized tempdir path (macOS tempdirs live behind a `/private` symlink; comparing
    /// resolved paths keeps the assertions byte-exact).
    fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        (dir, path)
    }

    // A reference occurrence at a bare `__name__`/`__file__` token resolves to a single-segment
    // `Namespace`-suffixed descriptor (e.g. `` `tests.test_basic`/ ``, no `__init__` terminal) —
    // distinct from the `__init__`-terminal shape this module's enrichment recognizes. That shape
    // already classifies as `SymbolKind::Module` through the translation's own descriptor→kind
    // mapping before `classify_module_kinds` runs, so the enrichment is a no-op on it — pinning that
    // here guards against a future translation change silently reclassifying it.
    #[test]
    fn namespace_only_descriptor_is_already_module_kind_without_enrichment() {
        let mut index = ExtractedIndex {
            provenance: AnalyzerProvenance {
                analyzer_name: "scip-python".to_string(),
                analyzer_version: "0.6.6".to_string(),
            },
            documents: Vec::new(),
            symbols: vec![ExtractedSymbol {
                descriptor: Some(Descriptor::new(
                    "flask",
                    vec![DescriptorSegment::new("tests.test_basic", SegmentKind::Module)],
                )),
                kind: SymbolKind::Module,
                class: SymbolClass::InWorkspace,
                occurrences: Vec::new(),
            }],
            duplicate_groups: Vec::new(),
            library_roots: Default::default(),
            environment: None,
        };
        classify_module_kinds(&mut index);
        assert_eq!(index.symbols[0].kind, SymbolKind::Module);
    }

    /// An executable shell stub named `name` in `dir` running `body`.
    fn write_stub(dir: &Path, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    // A scip-python that is installed but fails its version probe reports the exit status and error
    // output, and does not tell the user to install what is already there.
    #[test]
    fn failed_version_probe_message_carries_exit_status_and_does_not_say_to_install() {
        let dir = tempfile::tempdir().unwrap();
        let exe = write_stub(dir.path(), "scip-python", "echo 'node crashed' >&2\nexit 4");
        let err =
            PythonAdapter::discover_version(PythonIndexer::ScipPython, &exe).expect_err("a failing probe refuses");
        let text = err.to_string();
        assert!(text.contains("exit status: 4"), "{text}");
        assert!(text.contains("node crashed"), "{text}");
        assert!(!text.contains("install scip-python"), "{text}");
    }

    /// A fake environment under `root`: a `bin/python` stub running `python_body` and a
    /// `lib/python3.12/site-packages` directory.
    fn fake_environment(root: &Path, python_body: &str) -> PathBuf {
        std::fs::create_dir_all(root.join("bin")).unwrap();
        write_stub(&root.join("bin"), "python", python_body);
        std::fs::create_dir_all(root.join("lib/python3.12/site-packages")).unwrap();
        root.to_path_buf()
    }

    /// The Python adapter over `stub_body` as `scip-python`, with a working fake environment.
    fn adapter_over(dir: &Path, stub_body: &str) -> PythonAdapter {
        let exe = write_stub(dir, "scip-python", stub_body);
        let environment = fake_environment(&dir.join("env"), "echo 'Python 3.12.0'");
        PythonAdapter::new(PythonIndexer::ScipPython, exe, environment, "fixture").unwrap()
    }

    // A scip-python that exits nonzero from `index` names the tool, the exit status, and where its
    // own error output went.
    #[test]
    fn a_failing_index_run_names_the_tool_and_exit_status() {
        let (_guard, dir) = canonical_tempdir();
        let adapter = adapter_over(
            &dir,
            "if [ \"$1\" = \"--version\" ]; then echo 0.6.0; exit 0; fi\nexit 3",
        );
        let err = adapter
            .write_scip_index(&dir, &dir.join("out.scip"))
            .expect_err("a nonzero exit refuses");
        assert!(matches!(err, SemanticError::Analysis(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("scip-python") && text.contains("index"), "{text}");
        assert!(text.contains("exit status: 3"), "{text}");
        assert!(text.contains("error output is printed above"), "{text}");
    }

    // A scip-python that answered its version probe but cannot be started for `index` names the
    // operating system's cause and the install step.
    #[test]
    fn an_indexer_that_cannot_start_names_the_cause_and_install_step() {
        let (_guard, dir) = canonical_tempdir();
        let adapter = adapter_over(&dir, "echo 0.6.0");
        std::fs::remove_file(dir.join("scip-python")).unwrap();
        let err = adapter
            .write_scip_index(&dir, &dir.join("out.scip"))
            .expect_err("the indexer is gone");
        assert!(matches!(err, SemanticError::Unavailable(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("No such file or directory"), "{text}");
        assert!(text.contains("npm install -g @sourcegraph/scip-python"), "{text}");
    }

    // An indexer that exits 0 but leaves no output, or output that is not a SCIP index, names the
    // tool and the output path. One test, because `analyze` writes to one process-wide path.
    #[test]
    fn an_indexer_with_missing_or_invalid_output_names_the_output() {
        let (_guard, dir) = canonical_tempdir();
        let missing = adapter_over(
            &dir,
            "if [ \"$1\" = \"--version\" ]; then echo 0.6.0; exit 0; fi\nrm -f \"$7\"\nexit 0",
        );
        let err = missing.analyze(&dir).expect_err("no output was written");
        let text = err.to_string();
        assert!(matches!(err, SemanticError::Analysis(_)), "{err:?}");
        assert!(
            text.contains("c10r-scip-python") && text.contains("could not be read"),
            "{text}"
        );

        let garbage = adapter_over(
            &dir,
            "if [ \"$1\" = \"--version\" ]; then echo 0.6.0; exit 0; fi\nprintf '\\377\\377\\377\\377' > \"$7\"\nexit 0",
        );
        let err = garbage.analyze(&dir).expect_err("the output is not SCIP");
        let text = err.to_string();
        assert!(matches!(err, SemanticError::Analysis(_)), "{err:?}");
        assert!(
            text.contains("c10r-scip-python") && text.contains("not a valid SCIP index"),
            "{text}"
        );
    }

    // An interpreter that cannot be run, or fails `--version`, names the interpreter, the cause, and
    // the flag that points elsewhere.
    #[test]
    fn an_interpreter_that_cannot_run_or_fails_names_the_cause_and_remedy() {
        let (_guard, dir) = canonical_tempdir();
        let environment = fake_environment(&dir, "echo 'broken' >&2\nexit 2");
        let err = environment_facts(&environment).expect_err("a failing interpreter refuses");
        assert!(matches!(err, SemanticError::Environment(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("bin/python") && text.contains("exit status: 2"), "{text}");
        assert!(text.contains("broken"), "{text}");
        assert!(text.contains("--environment"), "{text}");

        std::fs::remove_file(environment.join("bin/python")).unwrap();
        let err = environment_facts(&environment).expect_err("a missing interpreter refuses");
        assert!(matches!(err, SemanticError::Environment(_)), "{err:?}");
        let text = err.to_string();
        assert!(
            text.contains("bin/python") && text.contains("No such file or directory"),
            "{text}"
        );
        assert!(text.contains("--environment"), "{text}");
    }

    // An unreadable directory under the environment's lib names the directory, the cause, and the
    // remedy, whether it is `lib` itself or the site-packages directory.
    #[test]
    fn an_unreadable_environment_directory_names_it_and_the_remedy() {
        use std::os::unix::fs::PermissionsExt;
        let (_guard, dir) = canonical_tempdir();
        let environment = fake_environment(&dir, "echo 'Python 3.12.0'");
        let lib = environment.join("lib");
        let site = lib.join("python3.12/site-packages");
        let lock = |path: &Path, mode: u32| std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));

        lock(&site, 0o000).unwrap();
        let site_err = environment_facts(&environment);
        lock(&site, 0o755).unwrap();
        lock(&lib, 0o000).unwrap();
        let lib_err = environment_facts(&environment);
        lock(&lib, 0o755).unwrap();

        for (err, unreadable) in [(site_err, &site), (lib_err, &lib)] {
            let err = err.expect_err("an unreadable directory refuses");
            assert!(matches!(err, SemanticError::Environment(_)), "{err:?}");
            let text = err.to_string();
            assert!(text.contains(&unreadable.display().to_string()), "{text}");
            assert!(text.contains("Permission denied"), "{text}");
            assert!(text.contains("--environment"), "{text}");
        }
    }

    // A missing scip-python names the install command and the cause the operating system gave.
    #[test]
    fn missing_tool_message_names_install_command() {
        let err = PythonAdapter::discover_version(PythonIndexer::ScipPython, "/nonexistent/scip-python")
            .expect_err("a missing tool refuses");
        let text = err.to_string();
        assert!(text.contains("npm install -g @sourcegraph/scip-python"), "{text}");
        assert!(text.contains("No such file or directory"), "{text}");
    }

    // An environment whose lib/ holds no site-packages names the directory, the environment, and
    // the flag that points elsewhere.
    #[test]
    fn environment_without_site_packages_names_the_flag() {
        let (_guard, root) = canonical_tempdir();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        let err = site_packages_dir(&root).expect_err("no site-packages refuses");
        let text = err.to_string();
        assert!(text.contains("site-packages"), "{text}");
        assert!(text.contains(&root.display().to_string()), "{text}");
        assert!(text.contains("--environment"), "{text}");
    }

    // An environment whose path holds the PATH separator cannot be put on scip-python's PATH; the
    // refusal names the directory, the cause, and the remedy.
    #[cfg(unix)]
    #[test]
    fn an_environment_path_holding_the_separator_names_the_cause_and_remedy() {
        let bin = Path::new("/envs/my:env/bin");
        let text = prepend_to_path(bin)
            .expect_err("a separator in the path refuses")
            .to_string();
        assert!(text.contains("/envs/my:env/bin"), "{text}");
        assert!(text.contains("PATH separator"), "{text}");
        assert!(text.contains("no index was built"), "{text}");
        assert!(text.contains("--environment"), "{text}");
    }

    // Precedence among the three environment-resolution mechanisms: an explicit path outranks an
    // activated $VIRTUAL_ENV, which outranks the workspace's .venv, which outranks a bare venv
    // directory.
    #[test]
    fn env_resolution_prefers_explicit_then_virtual_env_then_dot_venv() {
        let (_root_guard, root) = canonical_tempdir();
        std::fs::create_dir(root.join(".venv")).unwrap();
        std::fs::create_dir(root.join("venv")).unwrap();
        let (_explicit_guard, explicit) = canonical_tempdir();
        let (_active_guard, active) = canonical_tempdir();
        // Every input is reached through a symbolic link, so each result is canonical only through
        // resolution. (`PathBuf` equality ignores `.` components, so a `/./` spelling would not show it.)
        let (_links_guard, links) = canonical_tempdir();
        let linked = |path: &Path| {
            let link = links.join(path.file_name().unwrap());
            if !link.exists() {
                std::os::unix::fs::symlink(path, &link).unwrap();
            }
            link
        };

        // All three mechanisms available: the explicit path wins.
        let resolved = resolve_environment(Some(&linked(&explicit)), Some(&linked(&active)), &linked(&root)).unwrap();
        assert_eq!(resolved, explicit, "explicit path outranks $VIRTUAL_ENV and .venv");

        // No explicit path: the activated $VIRTUAL_ENV wins over the workspace's .venv.
        let resolved = resolve_environment(None, Some(&linked(&active)), &linked(&root)).unwrap();
        assert_eq!(resolved, active, "$VIRTUAL_ENV outranks .venv");

        // Neither: the workspace's .venv is found, preferred over venv.
        let resolved = resolve_environment(None, None, &linked(&root)).unwrap();
        assert_eq!(resolved, root.join(".venv"), ".venv outranks venv");

        // With .venv gone, venv resolves.
        std::fs::remove_dir(root.join(".venv")).unwrap();
        let resolved = resolve_environment(None, None, &linked(&root)).unwrap();
        assert_eq!(resolved, root.join("venv"), "venv is the final directory candidate");
    }

    // A refusal when no mechanism resolves an environment names every mechanism it tried, so the
    // caller knows what to fix.
    #[test]
    fn env_resolution_refuses_when_nothing_resolves() {
        let (_root_guard, root) = canonical_tempdir();
        let err = resolve_environment(None, None, &root).expect_err("nothing resolves");
        assert!(
            matches!(err, SemanticError::Environment(_)),
            "typed environment refusal, got: {err:?}"
        );
        let message = err.to_string();
        for mechanism in ["--environment", "VIRTUAL_ENV", ".venv"] {
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

    #[test]
    fn each_indexer_yields_its_executable_analyzer_name_and_install_hint() {
        let cases = [
            (PythonIndexer::TyScip, "ty-scip", "uv tool install ty-scip"),
            (
                PythonIndexer::ScipPython,
                "scip-python",
                "npm install -g @sourcegraph/scip-python",
            ),
        ];
        for (indexer, name, install) in cases {
            assert_eq!(indexer.executable(), name);
            assert_eq!(indexer.analyzer_name(), name);
            assert!(indexer.install_hint().contains(install), "{indexer:?}");
            assert_eq!(PythonIndexer::from_analyzer_name(name), Some(indexer));
        }
        assert_eq!(PythonIndexer::from_analyzer_name("rust-analyzer"), None);
    }

    // The adapter's provenance carries the indexer's analyzer name and the version the tool reports.
    #[test]
    fn adapter_provenance_carries_the_indexers_analyzer_name() {
        use std::os::unix::fs::PermissionsExt;

        let (_guard, dir) = canonical_tempdir();
        let tool = dir.join("fake-indexer");
        std::fs::write(&tool, "#!/bin/sh\necho 9.9.9\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        for indexer in [PythonIndexer::TyScip, PythonIndexer::ScipPython] {
            let adapter = PythonAdapter::new(indexer, tool.to_string_lossy().into_owned(), dir.clone(), "fixture")
                .expect("fake tool answers --version");
            let provenance = adapter.provenance();
            assert_eq!(provenance.analyzer_name, indexer.analyzer_name());
            assert_eq!(provenance.analyzer_version, "9.9.9");
        }
    }

    // A missing ty-scip names ty-scip and its own install command, not scip-python's.
    #[test]
    fn missing_ty_scip_names_its_own_install_hint() {
        let (_guard, dir) = canonical_tempdir();
        let err = PythonAdapter::discover_version(PythonIndexer::TyScip, &dir.join("ty-scip").to_string_lossy())
            .expect_err("tool is absent");
        assert!(matches!(err, SemanticError::Unavailable(_)), "{err:?}");
        let message = err.to_string();
        assert!(message.contains("uv tool install ty-scip"), "{message}");
        assert!(!message.contains("scip-python"), "{message}");
    }

    /// A probe that answers for the indexers in `present`, is unresponsive for those in `hanging`,
    /// and is unavailable otherwise; it records which indexers were probed.
    fn probe_over<'a>(
        present: &'a [PythonIndexer],
        hanging: &'a [PythonIndexer],
        probed: &'a std::cell::RefCell<Vec<PythonIndexer>>,
    ) -> impl Fn(PythonIndexer) -> Result<String, SemanticError> + 'a {
        move |indexer| {
            probed.borrow_mut().push(indexer);
            if present.contains(&indexer) {
                Ok(format!("{} 1.0", indexer.analyzer_name()))
            } else if hanging.contains(&indexer) {
                Err(SemanticError::Timeout(format!(
                    "{} did not respond",
                    indexer.analyzer_name()
                )))
            } else {
                Err(SemanticError::Unavailable(format!(
                    "cannot run {}; {}",
                    indexer.analyzer_name(),
                    indexer.install_hint()
                )))
            }
        }
    }

    // Each candidate source (explicit, recorded, default) is probed alone: present, it is returned
    // with its probed version; absent or unresponsive, its own error passes through — even while the
    // other indexer is present, which is never probed or returned.
    #[test]
    fn selection_probes_only_the_candidate() {
        use PythonIndexer::{ScipPython, TyScip};
        let other = |i: PythonIndexer| if i == TyScip { ScipPython } else { TyScip };
        let sources: [(Option<PythonIndexer>, Option<PythonIndexer>, PythonIndexer); 4] = [
            (Some(TyScip), None, TyScip),
            (Some(ScipPython), Some(TyScip), ScipPython),
            (None, Some(TyScip), TyScip),
            (None, None, ScipPython),
        ];
        for (explicit, recorded, candidate) in sources {
            let probed = std::cell::RefCell::new(Vec::new());
            let both = [candidate, other(candidate)];

            let selected = select_python_indexer(explicit, recorded, probe_over(&both, &[], &probed)).unwrap();
            assert_eq!(selected, (candidate, format!("{} 1.0", candidate.analyzer_name())));

            let absent = select_python_indexer(explicit, recorded, probe_over(&[other(candidate)], &[], &probed))
                .expect_err("candidate absent");
            assert!(matches!(absent, SemanticError::Unavailable(_)), "{absent:?}");
            assert!(absent.to_string().contains(candidate.install_hint()), "{absent}");

            let hung = select_python_indexer(
                explicit,
                recorded,
                probe_over(&[other(candidate)], &[candidate], &probed),
            )
            .expect_err("candidate unresponsive");
            assert!(matches!(hung, SemanticError::Timeout(_)), "{hung:?}");
            assert!(hung.to_string().contains(candidate.analyzer_name()), "{hung}");

            assert!(
                probed.borrow().iter().all(|p| *p == candidate),
                "only the candidate is probed"
            );
        }
    }

    // A ty-scip refusal names the file; a PEP 723 script gets the exclude-scripts remedy, any other
    // file gets an exclude entry for its path, and text naming no file is not a refusal.
    #[test]
    fn rejected_file_remedy_follows_the_named_files_header() {
        let (_guard, root) = canonical_tempdir();
        std::fs::write(
            root.join("tool.py"),
            "#!/usr/bin/env -S uv run\n# /// script\n# dependencies = []\n# ///\nprint()\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("test-examples")).unwrap();
        std::fs::write(root.join("test-examples").join("case.py"), "x = 1\n").unwrap();
        // A script marker below the first code line is not a leading comment.
        std::fs::write(root.join("late.py"), "x = 1\n# /// script\n").unwrap();

        let stderr = "ty-scip: indexing\nerror: project file has no importable module name: tool.py \n";
        let Some(SemanticError::RejectedFile { path, remedy }) = rejected_file(stderr, &root) else {
            panic!("a refusal naming a file is typed");
        };
        assert_eq!(path, root.join("tool.py"));
        assert!(remedy.contains("exclude-scripts = true"), "{remedy}");

        for named in ["test-examples/case.py", "late.py"] {
            let stderr = format!("project file has no importable module name: {named}\n");
            let Some(SemanticError::RejectedFile { remedy, .. }) = rejected_file(&stderr, &root) else {
                panic!("typed");
            };
            assert!(remedy.contains(&format!("add `{named}` to `exclude`")), "{remedy}");
        }

        assert!(rejected_file("ty-scip: Failed to discover local Python environment\n", &root).is_none());
    }

    // A nonzero ty-scip run becomes a typed refusal only when its standard error names a refused
    // file; any other nonzero exit stays an analysis failure.
    #[test]
    fn failed_ty_scip_run_is_classified_from_its_standard_error() {
        use std::os::unix::fs::PermissionsExt;

        let (_guard, dir) = canonical_tempdir();
        let tool = dir.join("ty-scip");
        let script = |body: &str| {
            std::fs::write(
                &tool,
                format!("#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 0.1.0; exit 0; }}\n{body}\n"),
            )
            .unwrap();
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let run = || {
            let adapter = PythonAdapter::new(
                PythonIndexer::TyScip,
                tool.to_string_lossy().into_owned(),
                dir.clone(),
                "fixture",
            )
            .unwrap();
            adapter.write_scip_index(&dir, &dir.join("out.scip"))
        };

        script("echo 'project file has no importable module name: a/b.py' >&2; exit 1");
        assert!(matches!(run(), Err(SemanticError::RejectedFile { .. })));
        script("echo 'ty-scip: Failed to discover local Python environment' >&2; exit 1");
        assert!(matches!(run(), Err(SemanticError::Analysis(_))));
        script("exit 0");
        assert!(run().is_ok());
    }

    // A missing indexer tool refuses with guidance naming the tool and how to install it. The tool
    // is looked up at an executable path inside an empty directory, so it is deterministically absent.
    #[test]
    fn python_adapter_reports_unavailable_when_tool_missing() {
        let (_empty_guard, empty) = canonical_tempdir();
        let missing_tool = empty.join("scip-python");
        let err = PythonAdapter::new(
            PythonIndexer::ScipPython,
            missing_tool.to_string_lossy().into_owned(),
            empty.clone(),
            "fixture",
        )
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
