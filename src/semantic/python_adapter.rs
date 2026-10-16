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

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::model::{
    AnalyzerProvenance, EnvironmentFacts, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, PositionEncoding,
    SourceRange, SymbolClass, SymbolKind, normalize,
};
use super::probe::{PROBE_DEADLINE, ProbeOutcome, run_bounded};
use super::scip::translate_index;
use super::{Capabilities, SemanticEngine, SemanticError};
use crate::graph::range::{ByteSpan, LineIndex, range_to_span};
use crate::graph::syntax::{Language, RelativeImport, SyntaxTree};
use crate::identity::{Descriptor, DescriptorSegment, SegmentKind};

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
        // ty-scip: a refusal is read from standard error, so each line is passed through to the user
        // as it arrives and kept for classifying a failed run.
        let mut child = command.stderr(Stdio::piped()).spawn().map_err(unavailable)?;
        let child_stderr = child.stderr.take().expect("standard error is piped");
        let forwarder = std::thread::spawn(move || forward_lines(child_stderr, std::io::stderr()));
        let status = child.wait().map_err(unavailable)?;
        let stderr = forwarder.join().unwrap_or_default();
        if !status.success() {
            return Err(rejected_file(&stderr, project_root).unwrap_or_else(|| {
                SemanticError::Analysis(format!(
                    "`{} index` failed ({status}). Its own error output is printed above.",
                    self.executable
                ))
            }));
        }
        Ok(())
    }
}

/// Copy `source` to `sink` one line at a time as each line arrives, and return everything read.
fn forward_lines(source: impl std::io::Read, mut sink: impl std::io::Write) -> String {
    use std::io::BufRead;

    let mut reader = std::io::BufReader::new(source);
    let mut captured = Vec::new();
    let mut line = Vec::new();
    while matches!(reader.read_until(b'\n', &mut line), Ok(n) if n > 0) {
        // A closed terminal must not stop the capture that classifies the run.
        let _ = sink.write_all(&line).and_then(|()| sink.flush());
        captured.append(&mut line);
    }
    String::from_utf8_lossy(&captured).into_owned()
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
    let remedy = rejected_file_remedy(&path, &relative_to_root(&path, project_root));
    Some(SemanticError::RejectedFile { path, remedy })
}

/// `path` relative to `project_root`, comparing symlink-resolved forms (ty-scip reports resolved
/// paths); `path` unchanged when it lies outside the root.
fn relative_to_root(path: &Path, project_root: &Path) -> String {
    let resolve = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (path, root) = (resolve(path), resolve(project_root));
    path.strip_prefix(&root).unwrap_or(&path).display().to_string()
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

/// Rewrite scip-python's descriptors into the canonical Python form.
///
/// The canonical form has one module segment per dotted component, no `__init__` terminal, and one
/// segment per module global:
///
/// - a dotted module segment splits per component;
/// - a trailing `__init__` meta segment after the module run is dropped;
/// - two consecutive term segments of equal name directly after the module run collapse to one.
///
/// Symbols whose descriptors become equal merge into the first-seen symbol, which keeps its kind,
/// gains the others' occurrences, and is in-workspace if any of them was. Every other segment (class,
/// method, attribute) is unchanged. Only scip-python needs this: ty-scip already writes the canonical form.
pub fn canonicalize_scip_python_descriptors(index: &mut ExtractedIndex) {
    let mut merged: Vec<ExtractedSymbol> = Vec::with_capacity(index.symbols.len());
    let mut by_descriptor: HashMap<Descriptor, usize> = HashMap::new();
    for mut symbol in std::mem::take(&mut index.symbols) {
        if let Some(descriptor) = &mut symbol.descriptor {
            descriptor.segments = canonical_segments(std::mem::take(&mut descriptor.segments));
            if let Some(&first) = by_descriptor.get(descriptor) {
                let kept = &mut merged[first];
                kept.occurrences.append(&mut symbol.occurrences);
                if symbol.class == SymbolClass::InWorkspace {
                    kept.class = SymbolClass::InWorkspace;
                }
                continue;
            }
            by_descriptor.insert(descriptor.clone(), merged.len());
        }
        merged.push(symbol);
    }
    index.symbols = merged;
}

/// The canonical segments for one scip-python descriptor's segments.
fn canonical_segments(mut segments: Vec<DescriptorSegment>) -> Vec<DescriptorSegment> {
    let module_run = segments
        .iter()
        .take_while(|seg| seg.kind == SegmentKind::Module)
        .count();
    // scip-python writes a module as one namespace segment holding the dotted name (`pkg.shapes`),
    // writes each module twice (once with an `__init__` meta terminal, once bare), and writes an
    // annotated module global with its name twice.
    let mut rest = segments.split_off(module_run);
    let mut out: Vec<DescriptorSegment> = segments
        .iter()
        .flat_map(|seg| seg.name.split('.'))
        .map(|part| DescriptorSegment::new(part, SegmentKind::Module))
        .collect();
    if module_run > 0 {
        if let [only] = rest.as_slice()
            && only.name == "__init__"
            && only.kind == SegmentKind::Meta
        {
            rest.clear();
        } else if let [first, second, ..] = rest.as_slice()
            && first.kind == SegmentKind::Term
            && second.kind == SegmentKind::Term
            && first.name == second.name
        {
            rest.remove(1);
        }
    }
    out.extend(rest);
    out
}

/// Classify every symbol whose descriptor is made only of module segments as `SymbolKind::Module`.
///
/// scip-python does not report a module kind, so this runs after canonicalization.
pub fn classify_module_kinds(index: &mut ExtractedIndex) {
    for symbol in &mut index.symbols {
        if symbol
            .descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.module_names().is_some())
        {
            symbol.kind = SymbolKind::Module;
        }
    }
}

/// The module names of a symbol whose descriptor is made only of module segments, with its package.
fn module_path(symbol: &ExtractedSymbol) -> Option<(&str, impl Iterator<Item = &str>)> {
    let descriptor = symbol.descriptor.as_ref()?;
    Some((descriptor.package.as_str(), descriptor.module_names()?))
}

/// What one relative import statement says about each module name scip-python can report for it.
struct StatementTargets {
    /// The byte span of the statement in its document.
    span: ByteSpan,
    /// The in-workspace module each name maps to, `None` when no workspace module sits there.
    targets: HashMap<Vec<String>, Option<usize>>,
}

/// One document's relative import statements with the source they were read from.
struct DocumentImports {
    source: String,
    lines: LineIndex,
    statements: Vec<StatementTargets>,
}

impl DocumentImports {
    /// The distinct targets the document's statements give `names`; empty when none names it.
    fn targets_for(&self, names: &[String]) -> BTreeSet<Option<usize>> {
        self.statements
            .iter()
            .filter_map(|statement| statement.targets.get(names))
            .copied()
            .collect()
    }

    /// The target of the statement holding `range`, when a statement does and it names `names`.
    fn statement_target(
        &self,
        range: SourceRange,
        encoding: PositionEncoding,
        names: &[String],
    ) -> Option<Option<usize>> {
        let span = range_to_span(&self.source, &self.lines, range, encoding)?;
        self.statements
            .iter()
            .find(|statement| statement.span.contains(&span))?
            .targets
            .get(names)
            .copied()
    }
}

/// What a document's relative imports say about each module name scip-python can report for them.
///
/// scip-python names a relatively imported module by the text after the dots (`from .shapes import W`
/// gives `shapes`; `from . import shapes` gives `shapes`; `from .sub import deep` gives `sub` and
/// `sub::deep`). Each such name maps to the in-workspace module at the document's package, climbed
/// by the extra dots, plus the name; `None` when no workspace module sits there. A statement whose
/// dots climb past the top package maps every name to `None`. A name that is only an imported
/// attribute (not a module) also maps to `None`, so the document never borrows another package's
/// module for it.
fn relative_import_targets(
    imports: &[RelativeImport],
    own: Option<&[String]>,
    is_initializer: bool,
    workspace_modules: &HashMap<Vec<String>, usize>,
) -> Vec<StatementTargets> {
    let package = own.map(|own| if is_initializer { own } else { &own[..own.len() - 1] });
    imports
        .iter()
        .map(|import| {
            let base = package.and_then(|package| {
                package
                    .len()
                    .checked_sub(import.dots.saturating_sub(1))
                    .filter(|&depth| depth > 0)
                    .map(|depth| &package[..depth])
            });
            let module_at = |tail: &[String]| {
                let path: Vec<String> = base?.iter().chain(tail).cloned().collect();
                workspace_modules.get(&path).copied()
            };
            let mut targets = HashMap::new();
            if !import.module.is_empty() {
                targets.insert(import.module.clone(), module_at(&import.module));
            }
            for name in &import.names {
                let named: Vec<String> = import.module.iter().chain([name]).cloned().collect();
                let target = module_at(&named);
                targets.insert(named, target);
            }
            StatementTargets {
                span: import.span,
                targets,
            }
        })
        .collect()
}

/// Move scip-python's references to relatively imported modules onto the in-workspace modules they name.
///
/// scip-python records `from .shapes import Widget` as a reference to an external module named by the
/// text after the dots, in the project's package, not to `pkg.shapes`. The absolute path comes from
/// the document's own module and the dots in its source, read from `project_root`:
///
/// - a reference inside a relative import statement that names the external module moves to the
///   in-workspace module at that statement's path;
/// - any other reference in a document with relative imports that name the external module moves to
///   the in-workspace module at their path, when all those imports agree on one;
/// - in any other readable document, they move only when every relative import in the workspace that
///   names the external module resolves to the same in-workspace module;
/// - otherwise they stay on the external module.
///
/// A document that cannot be read keeps its references. An external module left with no occurrences
/// is removed. Run after [`canonicalize_scip_python_descriptors`], before [`classify_module_kinds`];
/// only scip-python needs it, because ty-scip already resolves relative imports.
pub fn resolve_scip_python_relative_imports(index: &mut ExtractedIndex, project_root: &Path) {
    let mut package: Option<String> = None;
    let mut workspace_modules: HashMap<Vec<String>, usize> = HashMap::new();
    let mut own_modules: HashMap<String, Vec<String>> = HashMap::new();
    for (position, symbol) in index.symbols.iter().enumerate() {
        let Some((symbol_package, names)) = module_path(symbol) else {
            continue;
        };
        if symbol.class != SymbolClass::InWorkspace {
            continue;
        }
        package.get_or_insert_with(|| symbol_package.to_string());
        let names: Vec<String> = names.map(str::to_string).collect();
        if let Some(definition) = symbol.definition() {
            own_modules.insert(definition.document_path.clone(), names.clone());
        }
        workspace_modules.insert(names, position);
    }
    let Some(package) = package else {
        return;
    };
    let candidates: Vec<(usize, Vec<String>)> = index
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.class == SymbolClass::External)
        .filter_map(|(position, symbol)| {
            let (symbol_package, names) = module_path(symbol)?;
            (symbol_package == package.as_str()).then(|| (position, names.map(str::to_string).collect()))
        })
        .collect();
    let documents: BTreeSet<String> = candidates
        .iter()
        .flat_map(|(position, _)| &index.symbols[*position].occurrences)
        .map(|occurrence| occurrence.document_path.clone())
        .collect();

    let mut imports_by_document: HashMap<String, DocumentImports> = HashMap::new();
    for document in documents {
        let Ok(source) = std::fs::read_to_string(project_root.join(&document)) else {
            continue;
        };
        let Some(tree) = SyntaxTree::parse(&source, Language::Python) else {
            continue;
        };
        let own = own_modules.get(&document).map(Vec::as_slice);
        let is_initializer = Path::new(&document)
            .file_name()
            .is_some_and(|name| name == "__init__.py");
        let statements = relative_import_targets(&tree.relative_imports(), own, is_initializer, &workspace_modules);
        let lines = LineIndex::new(&source);
        imports_by_document.insert(
            document,
            DocumentImports {
                source,
                lines,
                statements,
            },
        );
    }

    // The distinct targets every statement in the workspace gives each module name.
    let mut workspace_targets: HashMap<&[String], BTreeSet<Option<usize>>> = HashMap::new();
    for statement in imports_by_document.values().flat_map(|imports| &imports.statements) {
        for (names, target) in &statement.targets {
            workspace_targets.entry(names.as_slice()).or_default().insert(*target);
        }
    }

    // Keyed by candidate position and occurrence position within it.
    let mut moves: HashMap<(usize, usize), usize> = HashMap::new();
    for (position, names) in &candidates {
        for (slot, occurrence) in index.symbols[*position].occurrences.iter().enumerate() {
            let Some(imports) = imports_by_document.get(&occurrence.document_path) else {
                continue;
            };
            let encoding = index
                .encoding_for(&occurrence.document_path)
                .unwrap_or(PositionEncoding::Utf8);
            let settled = match imports.statement_target(occurrence.range, encoding, names) {
                Some(target) => BTreeSet::from([target]),
                None => {
                    let in_document = imports.targets_for(names);
                    if in_document.is_empty() {
                        workspace_targets.get(names.as_slice()).cloned().unwrap_or_default()
                    } else {
                        in_document
                    }
                }
            };
            if let Some(Some(target)) = settled.first()
                && settled.len() == 1
            {
                moves.insert((*position, slot), *target);
            }
        }
    }

    let mut arrivals: Vec<(usize, ExtractedOccurrence)> = Vec::new();
    for (position, _) in &candidates {
        let symbol = &mut index.symbols[*position];
        for (slot, occurrence) in std::mem::take(&mut symbol.occurrences).into_iter().enumerate() {
            match moves.get(&(*position, slot)) {
                Some(target) => arrivals.push((*target, occurrence)),
                None => symbol.occurrences.push(occurrence),
            }
        }
    }
    for (target, occurrence) in arrivals {
        index.symbols[target].occurrences.push(occurrence);
    }
    let emptied: BTreeSet<usize> = candidates
        .iter()
        .map(|(position, _)| *position)
        .filter(|position| index.symbols[*position].occurrences.is_empty())
        .collect();
    let mut position = 0;
    index.symbols.retain(|_| {
        position += 1;
        !emptied.contains(&(position - 1))
    });
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
        // Python has no units below the project, so the target inventory stays not-applicable.
        let mut extracted = translate_index(&index, &self.provenance());
        extracted.environment = Some(facts);
        Ok(finish_translation(extracted, self.indexer, project_root))
    }
}

/// Bring a translated Python index to the canonical form: rewrite scip-python's output (ty-scip
/// already writes the canonical form), classify modules, and normalize.
pub fn finish_translation(
    mut extracted: ExtractedIndex,
    indexer: PythonIndexer,
    project_root: &Path,
) -> ExtractedIndex {
    if indexer == PythonIndexer::ScipPython {
        canonicalize_scip_python_descriptors(&mut extracted);
        resolve_scip_python_relative_imports(&mut extracted, project_root);
    }
    classify_module_kinds(&mut extracted);
    normalize(extracted)
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
    use crate::semantic::model::ExtractedOccurrence;

    /// A canonicalized tempdir path (macOS tempdirs live behind a `/private` symlink; comparing
    /// resolved paths keeps the assertions byte-exact).
    fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        (dir, path)
    }

    fn symbol_with(segments: &[(&str, SegmentKind)], kind: SymbolKind) -> ExtractedSymbol {
        ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "pkg",
                segments
                    .iter()
                    .map(|(name, kind)| DescriptorSegment::new(*name, *kind))
                    .collect(),
            )),
            kind,
            class: SymbolClass::InWorkspace,
            occurrences: Vec::new(),
        }
    }

    fn index_of(symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
        ExtractedIndex {
            provenance: AnalyzerProvenance {
                analyzer_name: "scip-python".to_string(),
                analyzer_version: "0.6.6".to_string(),
            },
            documents: Vec::new(),
            symbols,
            duplicate_groups: Vec::new(),
            targets: Default::default(),
            environment: None,
        }
    }

    /// The segment names of the only symbol of `index`, joined by `/`.
    fn only_path(index: &ExtractedIndex) -> String {
        assert_eq!(index.symbols.len(), 1);
        index.symbols[0]
            .descriptor
            .as_ref()
            .unwrap()
            .segments
            .iter()
            .map(|seg| seg.name.as_str())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn definition_at(line: u32) -> ExtractedOccurrence {
        ExtractedOccurrence {
            document_path: "pkg/shapes.py".to_string(),
            range: crate::semantic::model::SourceRange::new(line, 0, line, 1),
            role: crate::semantic::model::OccurrenceRole::Definition,
        }
    }

    fn reference_at(line: u32) -> ExtractedOccurrence {
        ExtractedOccurrence {
            role: crate::semantic::model::OccurrenceRole::Reference,
            ..definition_at(line)
        }
    }

    // _(Python module path splits per component)_
    #[test]
    fn dotted_module_splits_per_component() {
        let mut index = index_of(vec![symbol_with(
            &[("pkg.sub.shapes", SegmentKind::Module), ("Widget", SegmentKind::Type)],
            SymbolKind::Type,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/sub/shapes/Widget");
        let segments = &index.symbols[0].descriptor.as_ref().unwrap().segments;
        assert!(segments[..3].iter().all(|seg| seg.kind == SegmentKind::Module));
        assert_eq!(segments[3].kind, SegmentKind::Type);
    }

    // _(Python module and package have no `__init__` segment)_
    #[test]
    fn init_marker_dropped_and_module_symbols_merge() {
        let bare = ExtractedSymbol {
            class: SymbolClass::External,
            occurrences: vec![reference_at(5)],
            ..symbol_with(&[("pkg.shapes", SegmentKind::Module)], SymbolKind::Module)
        };
        let marker = ExtractedSymbol {
            occurrences: vec![definition_at(0)],
            ..symbol_with(
                &[("pkg.shapes", SegmentKind::Module), ("__init__", SegmentKind::Meta)],
                SymbolKind::Other,
            )
        };
        let mut index = index_of(vec![bare, marker]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/shapes");
        assert_eq!(
            index.symbols[0].kind,
            SymbolKind::Module,
            "first-seen symbol's kind wins"
        );
        assert_eq!(
            index.symbols[0].class,
            SymbolClass::InWorkspace,
            "a definition makes the merge in-workspace"
        );
        assert_eq!(index.symbols[0].occurrences, vec![reference_at(5), definition_at(0)]);
    }

    #[test]
    fn top_level_module_is_one_segment() {
        let mut index = index_of(vec![symbol_with(
            &[("shapes", SegmentKind::Module), ("__init__", SegmentKind::Meta)],
            SymbolKind::Other,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "shapes");
    }

    // _(Python method nests in its class)_
    #[test]
    fn class_and_method_segments_unchanged() {
        let mut index = index_of(vec![symbol_with(
            &[
                ("pkg.shapes", SegmentKind::Module),
                ("Widget", SegmentKind::Type),
                ("area", SegmentKind::Method),
            ],
            SymbolKind::Method,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/shapes/Widget/area");
    }

    // _(Python module global is one segment)_
    #[test]
    fn doubled_module_global_collapses_to_one_segment() {
        let mut index = index_of(vec![symbol_with(
            &[
                ("flask.globals", SegmentKind::Module),
                ("current_app", SegmentKind::Term),
                ("current_app", SegmentKind::Term),
            ],
            SymbolKind::Constant,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "flask/globals/current_app");
    }

    #[test]
    fn near_doubled_names_are_unchanged() {
        let attributes = [("size", SegmentKind::Term), ("_size", SegmentKind::Term)];
        let mut index = index_of(vec![symbol_with(
            &[
                ("pkg", SegmentKind::Module),
                ("Widget", SegmentKind::Type),
                attributes[0],
            ],
            SymbolKind::Field,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/Widget/size");

        let mut index = index_of(vec![symbol_with(
            &[
                ("pkg", SegmentKind::Module),
                ("Widget", SegmentKind::Type),
                attributes[1],
            ],
            SymbolKind::Field,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/Widget/_size");

        let mut index = index_of(vec![symbol_with(
            &[
                ("pkg", SegmentKind::Module),
                ("Widget", SegmentKind::Type),
                ("Widget", SegmentKind::Method),
            ],
            SymbolKind::Method,
        )]);
        canonicalize_scip_python_descriptors(&mut index);
        assert_eq!(only_path(&index), "pkg/Widget/Widget");
    }

    #[test]
    fn module_only_descriptor_classifies_as_module() {
        let mut index = index_of(vec![
            symbol_with(
                &[("pkg", SegmentKind::Module), ("shapes", SegmentKind::Module)],
                SymbolKind::Other,
            ),
            symbol_with(
                &[("pkg", SegmentKind::Module), ("Widget", SegmentKind::Type)],
                SymbolKind::Type,
            ),
        ]);
        classify_module_kinds(&mut index);
        assert_eq!(index.symbols[0].kind, SymbolKind::Module);
        assert_eq!(index.symbols[1].kind, SymbolKind::Type);
    }

    fn occurrence_at(document: &str, line: u32, role: crate::semantic::model::OccurrenceRole) -> ExtractedOccurrence {
        ExtractedOccurrence {
            document_path: document.to_string(),
            range: crate::semantic::model::SourceRange::new(line, 0, line, 1),
            role,
        }
    }

    /// An in-workspace module defined in `document`.
    fn workspace_module(path: &[&str], document: &str) -> ExtractedSymbol {
        let segments: Vec<_> = path.iter().map(|name| (*name, SegmentKind::Module)).collect();
        ExtractedSymbol {
            occurrences: vec![occurrence_at(
                document,
                0,
                crate::semantic::model::OccurrenceRole::Definition,
            )],
            ..symbol_with(&segments, SymbolKind::Module)
        }
    }

    /// An external module named `path`, referenced at each `(document, line)`.
    fn external_module(path: &[&str], references: &[(&str, u32)]) -> ExtractedSymbol {
        let segments: Vec<_> = path.iter().map(|name| (*name, SegmentKind::Module)).collect();
        ExtractedSymbol {
            class: SymbolClass::External,
            occurrences: references
                .iter()
                .map(|(document, line)| {
                    occurrence_at(document, *line, crate::semantic::model::OccurrenceRole::Reference)
                })
                .collect(),
            ..symbol_with(&segments, SymbolKind::Module)
        }
    }

    /// Run the relative-import step over `symbols` with the project sources `files` written to disk.
    fn resolve_with_sources(files: &[(&str, &str)], symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
        let (_guard, root) = canonical_tempdir();
        for (path, text) in files {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let mut index = index_of(symbols);
        resolve_scip_python_relative_imports(&mut index, &root);
        index
    }

    /// The `(document, line)` references of the module at `path` with class `class`, or `None` when
    /// the index holds no such symbol.
    fn module_references(index: &ExtractedIndex, path: &[&str], class: SymbolClass) -> Option<Vec<(String, u32)>> {
        index
            .symbols
            .iter()
            .find(|symbol| {
                symbol.class == class && module_path(symbol).is_some_and(|(_, names)| names.eq(path.iter().copied()))
            })
            .map(|symbol| {
                symbol
                    .occurrences
                    .iter()
                    .filter(|o| o.role == crate::semantic::model::OccurrenceRole::Reference)
                    .map(|o| (o.document_path.clone(), o.range.start_line))
                    .collect()
            })
    }

    fn at(document: &str, line: u32) -> (String, u32) {
        (document.to_string(), line)
    }

    // _(Python import from the same package)_
    #[test]
    fn same_package_import_moves_to_the_sibling_module() {
        let index = resolve_with_sources(
            &[("pkg/consumer.py", "from .shapes import Widget\n")],
            vec![
                workspace_module(&["pkg"], "pkg/__init__.py"),
                workspace_module(&["pkg", "consumer"], "pkg/consumer.py"),
                workspace_module(&["pkg", "shapes"], "pkg/shapes.py"),
                external_module(&["shapes"], &[("pkg/consumer.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "shapes"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/consumer.py", 0)])
        );
        assert_eq!(module_references(&index, &["shapes"], SymbolClass::External), None);
    }

    // _(Python import from a parent package)_
    #[test]
    fn parent_package_import_climbs_one_package_per_extra_dot() {
        let index = resolve_with_sources(
            &[("pkg/sub/deep.py", "from ..shapes import Widget\n")],
            vec![
                workspace_module(&["pkg", "sub", "deep"], "pkg/sub/deep.py"),
                workspace_module(&["pkg", "shapes"], "pkg/shapes.py"),
                external_module(&["shapes"], &[("pkg/sub/deep.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "shapes"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/sub/deep.py", 0)])
        );
    }

    // A package initializer is its own package, so one dot stays inside it.
    #[test]
    fn initializer_import_resolves_inside_its_own_package() {
        let index = resolve_with_sources(
            &[("pkg/sub/__init__.py", "from .app import run\n")],
            vec![
                workspace_module(&["pkg", "sub"], "pkg/sub/__init__.py"),
                workspace_module(&["pkg", "sub", "app"], "pkg/sub/app.py"),
                external_module(&["app"], &[("pkg/sub/__init__.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "sub", "app"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/sub/__init__.py", 0)])
        );
    }

    // _(Python name bound by a relative import)_
    #[test]
    fn module_bound_by_from_dot_import_moves_with_its_later_use() {
        let index = resolve_with_sources(
            &[("pkg/relative.py", "from . import shapes\n\nw = shapes.Widget()\n")],
            vec![
                workspace_module(&["pkg", "relative"], "pkg/relative.py"),
                workspace_module(&["pkg", "shapes"], "pkg/shapes.py"),
                external_module(&["shapes"], &[("pkg/relative.py", 0), ("pkg/relative.py", 2)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "shapes"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/relative.py", 0), at("pkg/relative.py", 2)])
        );
        assert_eq!(module_references(&index, &["shapes"], SymbolClass::External), None);
    }

    // An imported name that is itself a module resolves to that module; the package it is imported
    // from resolves too.
    #[test]
    fn module_imported_by_name_from_a_subpackage_resolves_with_the_subpackage() {
        let index = resolve_with_sources(
            &[("pkg/more.py", "from .sub import deep\n")],
            vec![
                workspace_module(&["pkg", "more"], "pkg/more.py"),
                workspace_module(&["pkg", "sub"], "pkg/sub/__init__.py"),
                workspace_module(&["pkg", "sub", "deep"], "pkg/sub/deep.py"),
                external_module(&["sub"], &[("pkg/more.py", 0)]),
                external_module(&["sub", "deep"], &[("pkg/more.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "sub"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/more.py", 0)])
        );
        assert_eq!(
            module_references(&index, &["pkg", "sub", "deep"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/more.py", 0)])
        );
    }

    // _(Same relative name in two packages)_
    #[test]
    fn same_relative_name_resolves_per_package() {
        let index = resolve_with_sources(
            &[
                ("pkg/relative.py", "from .app import run\n"),
                ("pkg/sub/deep.py", "from .app import run\n"),
            ],
            vec![
                workspace_module(&["pkg", "relative"], "pkg/relative.py"),
                workspace_module(&["pkg", "sub", "deep"], "pkg/sub/deep.py"),
                workspace_module(&["pkg", "app"], "pkg/app.py"),
                workspace_module(&["pkg", "sub", "app"], "pkg/sub/app.py"),
                external_module(&["app"], &[("pkg/relative.py", 0), ("pkg/sub/deep.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "app"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/relative.py", 0)])
        );
        assert_eq!(
            module_references(&index, &["pkg", "sub", "app"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/sub/deep.py", 0)])
        );
        assert_eq!(module_references(&index, &["app"], SymbolClass::External), None);
    }

    // A document with no relative import (a re-exporting module's user) follows the workspace only
    // when every relative import of that name resolves to one module.
    #[test]
    fn cross_document_use_moves_only_when_every_import_agrees() {
        let modules = || {
            vec![
                workspace_module(&["pkg", "relative"], "pkg/relative.py"),
                workspace_module(&["pkg", "other"], "pkg/other.py"),
                workspace_module(&["pkg", "shapes"], "pkg/shapes.py"),
                workspace_module(&["pkg", "app"], "pkg/app.py"),
                workspace_module(&["pkg", "sub", "deep"], "pkg/sub/deep.py"),
                workspace_module(&["pkg", "sub", "app"], "pkg/sub/app.py"),
            ]
        };
        let files = [
            ("pkg/relative.py", "from .shapes import W\nfrom .app import run\n"),
            ("pkg/sub/deep.py", "from .app import run\n"),
            ("pkg/other.py", "x = 1\n"),
        ];

        let mut symbols = modules();
        symbols.push(external_module(
            &["shapes"],
            &[("pkg/relative.py", 0), ("pkg/other.py", 0)],
        ));
        let index = resolve_with_sources(&files, symbols);
        assert_eq!(
            module_references(&index, &["pkg", "shapes"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/relative.py", 0), at("pkg/other.py", 0)]),
            "one import, one module: the document without an import follows"
        );

        let mut symbols = modules();
        symbols.push(external_module(
            &["app"],
            &[("pkg/relative.py", 1), ("pkg/sub/deep.py", 0), ("pkg/other.py", 0)],
        ));
        let index = resolve_with_sources(&files, symbols);
        assert_eq!(
            module_references(&index, &["app"], SymbolClass::External),
            Some(vec![at("pkg/other.py", 0)]),
            "two imports, two modules: the document without an import stays external"
        );
    }

    // _(a relative import that names no workspace module)_
    #[test]
    fn import_naming_no_workspace_module_leaves_the_reference_unchanged() {
        let index = resolve_with_sources(
            &[("pkg/consumer.py", "from .missing import thing\n")],
            vec![
                workspace_module(&["pkg", "consumer"], "pkg/consumer.py"),
                external_module(&["missing"], &[("pkg/consumer.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["missing"], SymbolClass::External),
            Some(vec![at("pkg/consumer.py", 0)])
        );
    }

    // Each relative import statement settles its own reference, even when the same document imports
    // the same name from two packages.
    #[test]
    fn statements_with_different_dot_counts_each_resolve() {
        let index = resolve_with_sources(
            &[(
                "pkg/sub/consumer.py",
                "from .app import run as local_run\nfrom ..app import run as parent_run\n",
            )],
            vec![
                workspace_module(&["pkg", "sub", "consumer"], "pkg/sub/consumer.py"),
                workspace_module(&["pkg", "app"], "pkg/app.py"),
                workspace_module(&["pkg", "sub", "app"], "pkg/sub/app.py"),
                external_module(&["app"], &[("pkg/sub/consumer.py", 0), ("pkg/sub/consumer.py", 1)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["pkg", "sub", "app"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/sub/consumer.py", 0)])
        );
        assert_eq!(
            module_references(&index, &["pkg", "app"], SymbolClass::InWorkspace),
            Some(vec![at("pkg/sub/consumer.py", 1)])
        );
        assert_eq!(module_references(&index, &["app"], SymbolClass::External), None);
    }

    // Dots that climb past the top package name no module, even when a top-level module has the name.
    #[test]
    fn dots_past_the_top_package_resolve_to_nothing() {
        for (document, own) in [("pkg/consumer.py", "consumer"), ("pkg/__init__.py", "")] {
            let own_path: Vec<&str> = ["pkg", own].into_iter().filter(|name| !name.is_empty()).collect();
            let index = resolve_with_sources(
                &[(document, "from ..app import run\n")],
                vec![
                    workspace_module(&own_path, document),
                    workspace_module(&["app"], "app.py"),
                    external_module(&["app"], &[(document, 0)]),
                ],
            );
            assert_eq!(
                module_references(&index, &["app"], SymbolClass::External),
                Some(vec![at(document, 0)]),
                "{document}"
            );
        }
    }

    // A `from . import name` that finds no workspace module in its own package does not take the
    // module another package's import of the same name resolves to.
    #[test]
    fn unresolved_imported_name_ignores_other_packages() {
        let index = resolve_with_sources(
            &[
                ("a/consumer.py", "from . import app\n"),
                ("b/consumer.py", "from . import app\n"),
            ],
            vec![
                workspace_module(&["a", "consumer"], "a/consumer.py"),
                workspace_module(&["b", "consumer"], "b/consumer.py"),
                workspace_module(&["b", "app"], "b/app.py"),
                external_module(&["app"], &[("a/consumer.py", 0), ("b/consumer.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["b", "app"], SymbolClass::InWorkspace),
            Some(vec![at("b/consumer.py", 0)])
        );
        assert_eq!(
            module_references(&index, &["app"], SymbolClass::External),
            Some(vec![at("a/consumer.py", 0)])
        );
    }

    // A source that cannot be read keeps its references where the indexer put them.
    #[test]
    fn unreadable_source_leaves_its_references_unchanged() {
        let index = resolve_with_sources(
            &[],
            vec![
                workspace_module(&["pkg", "consumer"], "pkg/consumer.py"),
                workspace_module(&["pkg", "shapes"], "pkg/shapes.py"),
                external_module(&["shapes"], &[("pkg/consumer.py", 0)]),
            ],
        );
        assert_eq!(
            module_references(&index, &["shapes"], SymbolClass::External),
            Some(vec![at("pkg/consumer.py", 0)])
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

    // ty-scip names a refused file by its absolute, symlink-resolved path; the remedy names it relative
    // to the project root, so it can be pasted into the project's configuration as printed.
    #[test]
    fn rejected_file_remedy_names_the_path_relative_to_the_project_root() {
        let (_guard, real_root) = canonical_tempdir();
        std::fs::create_dir(real_root.join("test-examples")).unwrap();
        std::fs::write(real_root.join("test-examples").join("case.py"), "x = 1\n").unwrap();
        let (_link_guard, link_dir) = canonical_tempdir();
        let linked_root = link_dir.join("project");
        std::os::unix::fs::symlink(&real_root, &linked_root).unwrap();

        let absolute = real_root.join("test-examples").join("case.py");
        let stderr = format!("project file has no importable module name: {}\n", absolute.display());
        let Some(SemanticError::RejectedFile { remedy, .. }) = rejected_file(&stderr, &linked_root) else {
            panic!("typed");
        };
        assert!(remedy.contains("add `test-examples/case.py` to `exclude`"), "{remedy}");
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

    // Each line of the indexer's error output reaches the user while the indexer still runs, and the
    // whole output is returned once the stream closes.
    #[test]
    fn indexer_error_output_is_forwarded_as_it_arrives() {
        use std::io::Write;
        use std::sync::mpsc;
        use std::time::Duration;

        struct ChannelSink(mpsc::Sender<Vec<u8>>);
        impl Write for ChannelSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.send(buf.to_vec()).unwrap();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let (reader, mut writer) = std::io::pipe().unwrap();
        let (tx, rx) = mpsc::channel();
        let forwarder = std::thread::spawn(move || forward_lines(reader, ChannelSink(tx)));

        writer.write_all(b"ty-scip: indexing\n").unwrap();
        let first = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("forwarded before the stream closes");
        assert_eq!(first, b"ty-scip: indexing\n");

        writer
            .write_all(b"project file has no importable module name: a.py")
            .unwrap();
        drop(writer);
        assert_eq!(
            forwarder.join().unwrap(),
            "ty-scip: indexing\nproject file has no importable module name: a.py"
        );
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
