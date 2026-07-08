//! The Rust semantic backend: `rust-analyzer scip` for cross-file identity/resolution/types,
//! translated into the engine-neutral model.
//!
//! The heavy, environment-dependent step — invoking `rust-analyzer scip` — is isolated in
//! [`RustAdapter::analyze`]. The translation from a SCIP index into the model is a pure function
//! ([`translate_index`]) so it is unit-testable without a live analyzer.

use std::path::Path;
use std::process::Command;

use scip::symbol::parse_symbol;
use scip::types as scip_types;

use super::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind, normalize,
};
use super::{Capabilities, SemanticEngine, SemanticError};
use crate::identity::{Descriptor, DescriptorSegment, SegmentKind};

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

/// Translate a SCIP position encoding into the model's.
fn translate_encoding(enc: scip_types::PositionEncoding) -> PositionEncoding {
    match enc {
        scip_types::PositionEncoding::UTF16CodeUnitOffsetFromLineStart => PositionEncoding::Utf16,
        scip_types::PositionEncoding::UTF32CodeUnitOffsetFromLineStart => PositionEncoding::Utf32,
        // rust-analyzer emits UTF-8 byte offsets; treat unspecified as UTF-8 to match its behavior.
        _ => PositionEncoding::Utf8,
    }
}

/// Translate a SCIP descriptor suffix into a segment kind.
fn translate_suffix(suffix: scip_types::descriptor::Suffix) -> SegmentKind {
    use scip_types::descriptor::Suffix;
    match suffix {
        Suffix::Namespace | Suffix::Package => SegmentKind::Module,
        Suffix::Type => SegmentKind::Type,
        Suffix::Term => SegmentKind::Term,
        Suffix::Method => SegmentKind::Method,
        Suffix::TypeParameter => SegmentKind::TypeParameter,
        Suffix::Macro => SegmentKind::Macro,
        Suffix::Parameter | Suffix::Meta | Suffix::Local | Suffix::UnspecifiedSuffix => SegmentKind::Meta,
    }
}

/// A parsed SCIP symbol string, classified.
pub struct ParsedSymbol {
    /// The resolved descriptor, or `None` for a file-local symbol.
    pub descriptor: Option<Descriptor>,
}

/// Parse a SCIP symbol string into a descriptor, or `None` if it is a file-local symbol.
///
/// Local symbols (`local <id>`) have no global descriptor and are outside the identity projection's
/// domain. A global symbol's descriptor is `<package>` plus its ordered descriptor segments.
pub fn parse_scip_symbol(symbol: &str) -> ParsedSymbol {
    if scip::symbol::is_local_symbol(symbol) {
        return ParsedSymbol { descriptor: None };
    }
    match parse_symbol(symbol) {
        Ok(sym) => {
            let package = sym.package.name.clone();
            let segments: Vec<DescriptorSegment> = sym
                .descriptors
                .iter()
                .map(|d| {
                    let kind = translate_suffix(d.suffix.enum_value_or(scip_types::descriptor::Suffix::Meta));
                    DescriptorSegment::new(d.name.clone(), kind)
                })
                .collect();
            if segments.is_empty() {
                ParsedSymbol { descriptor: None }
            } else {
                ParsedSymbol {
                    descriptor: Some(Descriptor::new(package, segments)),
                }
            }
        }
        // An unparsable symbol string is treated as having no global descriptor.
        Err(_) => ParsedSymbol { descriptor: None },
    }
}

/// Read a SCIP occurrence range (`[startLine, startChar, endChar]` or
/// `[startLine, startChar, endLine, endChar]`) into a [`SourceRange`].
fn read_range(range: &[i32]) -> Option<SourceRange> {
    match range {
        [sl, sc, ec] => Some(SourceRange::new(*sl as u32, *sc as u32, *sl as u32, *ec as u32)),
        [sl, sc, el, ec] => Some(SourceRange::new(*sl as u32, *sc as u32, *el as u32, *ec as u32)),
        _ => None,
    }
}

const DEFINITION_ROLE_BIT: i32 = 1;

/// Map a symbol kind from the SCIP terminal descriptor suffix.
fn symbol_kind_from(descriptor: &Descriptor) -> SymbolKind {
    match descriptor.segments.last().map(|s| s.kind) {
        Some(SegmentKind::Module) => SymbolKind::Module,
        Some(SegmentKind::Type) => SymbolKind::Type,
        Some(SegmentKind::Method) => SymbolKind::Method,
        Some(SegmentKind::Term) => SymbolKind::Constant,
        _ => SymbolKind::Other,
    }
}

/// Translate a full SCIP index into the engine-neutral model.
///
/// Symbols with a definition occurrence in the index are classified in-workspace; symbols that only
/// appear as references (no definition) are classified external — a first-class reference target
/// with no definition span. Local symbols are dropped (they have no global descriptor).
pub fn translate_index(index: &scip_types::Index, provenance: &AnalyzerProvenance) -> ExtractedIndex {
    use std::collections::BTreeMap;

    let mut documents = Vec::new();
    // Accumulate occurrences per SCIP symbol string, preserving whether a definition was seen.
    struct Acc {
        descriptor: Option<Descriptor>,
        occurrences: Vec<ExtractedOccurrence>,
        has_definition: bool,
    }
    // BTreeMap keeps symbol iteration order deterministic regardless of document/occurrence order.
    let mut by_symbol: BTreeMap<String, Acc> = BTreeMap::new();

    for doc in &index.documents {
        let encoding = translate_encoding(
            doc.position_encoding
                .enum_value_or(scip_types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart),
        );
        documents.push(SourceDocument {
            path: doc.relative_path.clone(),
            encoding,
        });
        for occ in &doc.occurrences {
            if occ.symbol.is_empty() {
                continue;
            }
            let Some(range) = read_range(&occ.range) else {
                continue;
            };
            let parsed = parse_scip_symbol(&occ.symbol);
            // Local symbols (no descriptor) are excluded from the persisted base.
            if parsed.descriptor.is_none() {
                continue;
            }
            let is_definition = occ.symbol_roles & DEFINITION_ROLE_BIT != 0;
            let role = if is_definition {
                OccurrenceRole::Definition
            } else {
                OccurrenceRole::Reference
            };
            let entry = by_symbol.entry(occ.symbol.clone()).or_insert_with(|| Acc {
                descriptor: parsed.descriptor.clone(),
                occurrences: Vec::new(),
                has_definition: false,
            });
            entry.has_definition |= is_definition;
            entry.occurrences.push(ExtractedOccurrence {
                document_path: doc.relative_path.clone(),
                range,
                role,
            });
        }
    }

    let symbols = by_symbol
        .into_values()
        .filter_map(|acc| {
            let descriptor = acc.descriptor?;
            let kind = symbol_kind_from(&descriptor);
            let class = if acc.has_definition {
                SymbolClass::InWorkspace
            } else {
                SymbolClass::External
            };
            Some(ExtractedSymbol {
                descriptor: Some(descriptor),
                kind,
                class,
                occurrences: acc.occurrences,
            })
        })
        .collect();

    ExtractedIndex {
        provenance: provenance.clone(),
        documents,
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_scip_symbol_has_no_descriptor() {
        let parsed = parse_scip_symbol("local 42");
        assert!(parsed.descriptor.is_none());
    }

    #[test]
    fn global_scip_symbol_projects_descriptor_with_terminal_name() {
        // A rust-analyzer SCIP symbol for a method `connect` on `Client` in module `net`.
        let sym = "rust-analyzer cargo mycrate 0.1.0 net/Client#connect().";
        let parsed = parse_scip_symbol(sym);
        let descriptor = parsed.descriptor.expect("global symbol has a descriptor");
        assert_eq!(descriptor.terminal_name(), Some("connect"));
    }

    #[test]
    fn translate_index_classifies_definitions_and_external_references() {
        use scip::types::{Document, Index, Occurrence, PositionEncoding};

        // A document with a definition of `f` and a reference to an undefined `g` (external).
        let mut def = Occurrence::new();
        def.symbol = "rust-analyzer cargo mycrate 0.1.0 f().".to_string();
        def.range = vec![0, 3, 4];
        def.symbol_roles = super::DEFINITION_ROLE_BIT;

        let mut ext = Occurrence::new();
        ext.symbol = "rust-analyzer cargo other 0.1.0 g().".to_string();
        ext.range = vec![0, 9, 10];
        ext.symbol_roles = 0;

        let mut doc = Document::new();
        doc.relative_path = "m.rs".to_string();
        doc.position_encoding = PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into();
        doc.occurrences = vec![def, ext];

        let mut index = Index::new();
        index.documents = vec![doc];

        let provenance = AnalyzerProvenance {
            analyzer_name: "rust-analyzer".to_string(),
            analyzer_version: "test".to_string(),
        };
        let translated = translate_index(&index, &provenance);

        let f = translated
            .symbols
            .iter()
            .find(|s| s.terminal_name() == Some("f"))
            .unwrap();
        assert_eq!(f.class, SymbolClass::InWorkspace, "a defined symbol is in-workspace");
        let g = translated
            .symbols
            .iter()
            .find(|s| s.terminal_name() == Some("g"))
            .unwrap();
        assert_eq!(g.class, SymbolClass::External, "a referenced-only symbol is external");
        // The document's declared encoding is carried through as UTF-8.
        assert_eq!(
            translated.encoding_for("m.rs"),
            Some(crate::semantic::model::PositionEncoding::Utf8)
        );
    }

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
