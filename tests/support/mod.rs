//! Shared test fixtures: a small, hand-built extracted index over a synthetic Rust workspace.
//!
//! The fixture models a workspace with a module `net` containing a type `Client` with methods
//! `connect` and `disconnect`, a free function `open` that calls `connect`, and a reference to an
//! external `std` symbol. It exercises definitions, references, enclosure, method/closure/module
//! attribution, external symbols, and a text-mismatch case.

#![allow(dead_code)]

use silent_cartographer::graph::join::JoinAccounting;
use silent_cartographer::graph::store::{GraphStore, IndexMetadata};
use silent_cartographer::identity::{
    CanonicalId, Descriptor, DescriptorSegment, SegmentKind, WorkspaceId, project_one,
};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};

/// Stamp a directly-built fixture store — one populated through `insert_symbol` rather than the build
/// path — with minimal index metadata, so it reads as a completed build. The read-only query path
/// refuses a schema-stamped store that carries no build metadata (a never-completed build), so a
/// fixture that queries through the CLI must record metadata even when the join accounting is empty.
pub fn stamp_metadata(store: &GraphStore, workspace: &str, root: &std::path::Path) {
    let workspace_root = std::fs::canonicalize(root)
        .unwrap_or_else(|e| panic!("canonicalizing the fixture workspace root {}: {e}", root.display()));
    store
        .write_metadata(&IndexMetadata {
            workspace_id: WorkspaceId::new(workspace),
            workspace_root: workspace_root.to_str().map(str::to_string),
            provenance: provenance(),
            content_hash: String::new(),
            accounting: JoinAccounting::default(),
            environment: None,
            chunk_params: Default::default(),
        })
        .unwrap();
}

/// Stamp a directly-built fixture store the same way [`stamp_metadata`] does, except the recorded
/// content hash is `root`'s real, current Rust-source hash rather than an always-stale placeholder —
/// so a query against the fixture reads as fresh (no refresh notice) as long as the caller also makes
/// the analyzer provenance the query sees match [`provenance`].
pub fn stamp_fresh_metadata(store: &GraphStore, workspace: &str, root: &std::path::Path) {
    let workspace_root = std::fs::canonicalize(root)
        .unwrap_or_else(|e| panic!("canonicalizing the fixture workspace root {}: {e}", root.display()));
    let sources = silent_cartographer::commands::collect_rust_sources(root)
        .unwrap_or_else(|e| panic!("collecting rust sources under {}: {e}", root.display()));
    let content_hash = silent_cartographer::graph::content_hash(&sources);
    store
        .write_metadata(&IndexMetadata {
            workspace_id: WorkspaceId::new(workspace),
            workspace_root: workspace_root.to_str().map(str::to_string),
            provenance: provenance(),
            content_hash,
            accounting: JoinAccounting::default(),
            environment: None,
            chunk_params: Default::default(),
        })
        .unwrap();
}

/// The workspace-relative path of the single fixture source file.
pub const DOC: &str = "src/net.rs";

/// The source text the fixture's ranges index into. Byte offsets and line/col are chosen to match
/// the occurrence ranges below exactly.
pub const SOURCE: &str = "\
mod net {
    pub struct Client;
    impl Client {
        pub fn connect(&self) {}
        pub fn disconnect(&self) {}
    }
    pub fn open() -> Client {
        let c = Client;
        let f = || connect(&c);
        c
    }
}
";

fn seg(name: &str, kind: SegmentKind) -> DescriptorSegment {
    DescriptorSegment::new(name, kind)
}

fn descriptor(segments: Vec<DescriptorSegment>) -> Option<Descriptor> {
    Some(Descriptor::new("mycrate", segments))
}

fn occ(line: u32, start: u32, end: u32, role: OccurrenceRole) -> ExtractedOccurrence {
    ExtractedOccurrence {
        document_path: DOC.to_string(),
        range: SourceRange::new(line, start, line, end),
        role,
    }
}

/// The analyzer provenance the fixture reports.
pub fn provenance() -> AnalyzerProvenance {
    AnalyzerProvenance {
        analyzer_name: "rust-analyzer".to_string(),
        analyzer_version: "1.85.0".to_string(),
    }
}

/// Build the fixture index. Ranges are UTF-8 and refer to [`SOURCE`].
pub fn fixture_index() -> ExtractedIndex {
    // Line indices (0-based) into SOURCE:
    // 0: mod net {
    // 1:     pub struct Client;
    // 2:     impl Client {
    // 3:         pub fn connect(&self) {}
    // 4:         pub fn disconnect(&self) {}
    // 5:     }
    // 6:     pub fn open() -> Client {
    // 7:         let c = Client;
    // 8:         let f = || connect(&c);
    // 9:         c
    // 10:    }
    // 11: }
    let module = ExtractedSymbol {
        descriptor: descriptor(vec![seg("net", SegmentKind::Module)]),
        kind: SymbolKind::Module,
        class: SymbolClass::InWorkspace,
        // "net" at line 0, cols 4..7
        occurrences: vec![occ(0, 4, 7, OccurrenceRole::Definition)],
    };
    let client = ExtractedSymbol {
        descriptor: descriptor(vec![seg("net", SegmentKind::Module), seg("Client", SegmentKind::Type)]),
        kind: SymbolKind::Type,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            // definition "Client" at line 1, cols 15..21
            occ(1, 15, 21, OccurrenceRole::Definition),
            // reference in `impl Client` at line 2, cols 9..15
            occ(2, 9, 15, OccurrenceRole::Reference),
            // reference in return type at line 6, cols 21..27
            occ(6, 21, 27, OccurrenceRole::Reference),
            // reference `Client;` at line 7, cols 16..22
            occ(7, 16, 22, OccurrenceRole::Reference),
        ],
    };
    let connect = ExtractedSymbol {
        descriptor: descriptor(vec![
            seg("net", SegmentKind::Module),
            seg("Client", SegmentKind::Type),
            seg("connect", SegmentKind::Method),
        ]),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        occurrences: vec![
            // definition "connect" at line 3, cols 15..22
            occ(3, 15, 22, OccurrenceRole::Definition),
            // reference inside the closure at line 8, cols 19..26
            occ(8, 19, 26, OccurrenceRole::Reference),
        ],
    };
    let disconnect = ExtractedSymbol {
        descriptor: descriptor(vec![
            seg("net", SegmentKind::Module),
            seg("Client", SegmentKind::Type),
            seg("disconnect", SegmentKind::Method),
        ]),
        kind: SymbolKind::Method,
        class: SymbolClass::InWorkspace,
        // definition "disconnect" at line 4, cols 15..25
        occurrences: vec![occ(4, 15, 25, OccurrenceRole::Definition)],
    };
    let open = ExtractedSymbol {
        descriptor: descriptor(vec![seg("net", SegmentKind::Module), seg("open", SegmentKind::Method)]),
        kind: SymbolKind::Function,
        class: SymbolClass::InWorkspace,
        // definition "open" at line 6, cols 11..15
        occurrences: vec![occ(6, 11, 15, OccurrenceRole::Definition)],
    };

    ExtractedIndex {
        provenance: provenance(),
        documents: vec![SourceDocument {
            path: DOC.to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols: vec![module, client, connect, disconnect, open],
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

/// The single fixture source, paired with its workspace-relative path.
pub fn sources() -> Vec<(String, String)> {
    vec![(DOC.to_string(), SOURCE.to_string())]
}

/// Build the exemplar Rust fixture into a store at `dir/index.db` under `workspace`, returning its
/// path.
pub fn build_fixture_db(dir: &std::path::Path, workspace: &str) -> std::path::PathBuf {
    let db = dir.join("index.db");
    silent_cartographer::commands::build_from_index(&db, workspace, dir, &fixture_index(), &sources()).unwrap();
    db
}

/// The workspace identity hand-built fixture stores are namespaced under.
pub fn ws() -> WorkspaceId {
    WorkspaceId::new("test-ws")
}

/// The canonical identity of a synthetic symbol under an explicit package.
pub fn id_of_pkg(package: &str, segments: &[(&str, SegmentKind)]) -> CanonicalId {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    project_one(&ws(), &Descriptor::new(package, segs))
}

/// A one-document index over `path`, carrying the fixture [`provenance`].
pub fn one_doc_index(path: &str, symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
    ExtractedIndex {
        provenance: provenance(),
        documents: vec![SourceDocument {
            path: path.to_string(),
            encoding: PositionEncoding::Utf8,
        }],
        symbols,
        duplicate_groups: Vec::new(),
        library_roots: Default::default(),
        environment: None,
    }
}

/// A symbol with one occurrence, for hand-built rule and tier fixtures.
pub fn one_occ_symbol(
    package: &str,
    segments: &[(&str, SegmentKind)],
    kind: SymbolKind,
    class: SymbolClass,
    doc: &str,
    range: SourceRange,
    role: OccurrenceRole,
) -> ExtractedSymbol {
    let segs: Vec<DescriptorSegment> = segments.iter().map(|(n, k)| DescriptorSegment::new(*n, *k)).collect();
    ExtractedSymbol {
        descriptor: Some(Descriptor::new(package, segs)),
        kind,
        class,
        occurrences: vec![ExtractedOccurrence {
            document_path: doc.to_string(),
            range,
            role,
        }],
    }
}

/// The zero-based `(line, col)` of the byte at `pos` in single-byte-per-char test sources.
pub fn line_col(source: &str, pos: usize) -> (u32, u32) {
    let line = source[..pos].matches('\n').count() as u32;
    let line_start = source[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    (line, (pos - line_start) as u32)
}

/// The committed Python conformance fixture directory (`tests/fixtures/python-conformance`).
pub fn python_fixture_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python-conformance")
}

/// The committed Python fixture SCIP index, taken through the shared translation and normalization
/// exactly as `PythonAdapter::analyze` takes the tool's live output.
///
/// # Panics
///
/// Panics if `index.scip` is missing from the fixture directory or fails to parse as a SCIP
/// protobuf.
pub fn python_fixture_index() -> ExtractedIndex {
    use silent_cartographer::semantic::model::normalize;
    use silent_cartographer::semantic::python_adapter::classify_module_kinds;
    use silent_cartographer::semantic::scip::translate_index;

    let path = python_fixture_root().join("index.scip");
    let bytes = std::fs::read(&path).expect("checked-in python-conformance index.scip present");
    let index: scip::types::Index = protobuf::Message::parse_from_bytes(&bytes).expect("valid SCIP protobuf");
    let provenance = AnalyzerProvenance {
        analyzer_name: index.metadata.tool_info.name.clone(),
        analyzer_version: index.metadata.tool_info.version.clone(),
    };
    let mut extracted = translate_index(&index, &provenance);
    classify_module_kinds(&mut extracted);
    normalize(extracted)
}

/// The Python fixture's sources, `(scip_document_path, text)`, read from the committed project.
///
/// # Panics
///
/// Panics if any of the fixture's source files is missing.
pub fn python_fixture_sources() -> Vec<(String, String)> {
    let root = python_fixture_root();
    ["pkg/__init__.py", "pkg/consumer.py", "pkg/shapes.py"]
        .iter()
        .map(|rel| {
            let text = std::fs::read_to_string(root.join(rel)).expect("fixture source present");
            (rel.to_string(), text)
        })
        .collect()
}

/// Write an executable stub tool that answers `--version` with `version`, and — for any other
/// invocation carrying `--output <path>`, the shape both `rust-analyzer` and `scip-python` are run
/// with — writes an empty file at `<path>` and exits successfully. An empty SCIP index is a valid one
/// (zero documents), so this lets a real `c10r build` run to completion without a real analyzer
/// available in this sandbox.
pub fn write_indexing_stub(dir: &std::path::Path, name: &str, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then\n  echo \"{version}\"\n  exit 0\nfi\n\
             while [ $# -gt 0 ]; do\n\
             \x20 if [ \"$1\" = \"--output\" ]; then\n    : > \"$2\"\n    exit 0\n  fi\n\
             \x20 shift\n\
             done\n\
             exit 1\n"
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    path
}

/// `PATH` with `dirs` prepended, in order, ahead of the inherited value, so a stub placed in one of
/// them is found before any same-named tool already on `PATH`.
pub fn prepend_path(dirs: &[&std::path::Path]) -> std::ffi::OsString {
    let mut parts: Vec<std::path::PathBuf> = dirs.iter().map(|d| d.to_path_buf()).collect();
    if let Some(existing) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts).unwrap()
}

/// The source languages the build has a backend for. The exhaustive match in [`backend_provenance`]
/// stops compiling when a backend is added without a case there.
pub const LANGUAGES: [silent_cartographer::graph::syntax::Language; 2] = [
    silent_cartographer::graph::syntax::Language::Rust,
    silent_cartographer::graph::syntax::Language::Python,
];

/// The provenance a backend for `language` reports.
pub fn backend_provenance(language: silent_cartographer::graph::syntax::Language) -> AnalyzerProvenance {
    use silent_cartographer::graph::syntax::Language;
    use silent_cartographer::semantic::python_adapter::PythonAdapter;
    use silent_cartographer::semantic::rust_adapter::RustAdapter;
    let analyzer_name = match language {
        Language::Rust => RustAdapter::analyzer_name(),
        Language::Python => PythonAdapter::analyzer_name(),
    };
    AnalyzerProvenance {
        analyzer_name: analyzer_name.to_string(),
        analyzer_version: "1.0.0".to_string(),
    }
}

/// One declaration of a hand-built index: its descriptor path under the package `poscrate`, its kind,
/// and the one place in the source its name sits.
pub struct Decl<'a> {
    pub segments: &'a [(&'a str, SegmentKind)],
    pub kind: SymbolKind,
    /// A substring occurring exactly once in the source that contains the declaration's name.
    pub within: &'a str,
}

/// The identity of a [`Decl`] path under the package `poscrate`.
pub fn decl_id(segments: &[(&str, SegmentKind)]) -> CanonicalId {
    id_of_pkg("poscrate", segments)
}

/// A store built through `language`'s backend rules from `source` at `doc`, recording `decls` as
/// definitions at their names plus the document's own module.
pub fn declared_store(
    language: silent_cartographer::graph::syntax::Language,
    doc: &str,
    source: &str,
    decls: &[Decl],
) -> GraphStore {
    let index = declared_index(language, doc, source, decls);
    let mut store = GraphStore::open_in_memory().unwrap();
    silent_cartographer::graph::ingest(
        &mut store,
        &ws(),
        Some("/test-ws"),
        &index,
        &[(doc.to_string(), source.to_string())],
    )
    .unwrap();
    store
}

/// The index [`declared_store`] ingests: `decls` as definitions at their names plus the document's
/// own module, under `language`'s provenance.
pub fn declared_index(
    language: silent_cartographer::graph::syntax::Language,
    doc: &str,
    source: &str,
    decls: &[Decl],
) -> ExtractedIndex {
    let mut symbols: Vec<ExtractedSymbol> = decls
        .iter()
        .map(|decl| {
            assert_eq!(
                source.matches(decl.within).count(),
                1,
                "{:?} must occur once",
                decl.within
            );
            let name = decl.segments.last().unwrap().0;
            let at = source.find(decl.within).unwrap() + decl.within.find(name).unwrap();
            let (line, col) = line_col(source, at);
            one_occ_symbol(
                "poscrate",
                decl.segments,
                decl.kind,
                SymbolClass::InWorkspace,
                doc,
                SourceRange::new(line, col, line, col + name.len() as u32),
                OccurrenceRole::Definition,
            )
        })
        .collect();
    // A Rust file module is defined over the whole file; a Python module by a zero-width marker.
    let module_range = match language {
        silent_cartographer::graph::syntax::Language::Rust => {
            let (end_line, end_col) = line_col(source, source.len());
            SourceRange::new(0, 0, end_line, end_col)
        }
        silent_cartographer::graph::syntax::Language::Python => SourceRange::new(0, 0, 0, 0),
    };
    symbols.push(one_occ_symbol(
        "poscrate",
        &[("module", SegmentKind::Module)],
        SymbolKind::Module,
        SymbolClass::InWorkspace,
        doc,
        module_range,
        OccurrenceRole::Definition,
    ));
    let mut index = one_doc_index(doc, symbols);
    index.provenance = backend_provenance(language);
    index
}

/// The 1-based line of the first occurrence of `needle` in `source`.
pub fn line_number(source: &str, needle: &str) -> u32 {
    line_col(source, source.find(needle).unwrap()).0 + 1
}

/// A source file holding the declaration shapes the header rule distinguishes, in one language, with
/// the declarations an index records for it.
pub struct HeaderFixture {
    pub doc: &'static str,
    pub source: &'static str,
    pub decls: Vec<Decl<'static>>,
}

impl HeaderFixture {
    /// The store a build over this fixture produces.
    pub fn store(&self, language: silent_cartographer::graph::syntax::Language) -> GraphStore {
        declared_store(language, self.doc, self.source, &self.decls)
    }

    /// A store file under `dir` built from this fixture, with its source written beside it so a
    /// command handler can read both. Returns the store's path.
    pub fn build_db(
        &self,
        language: silent_cartographer::graph::syntax::Language,
        dir: &std::path::Path,
    ) -> std::path::PathBuf {
        let doc_path = dir.join(self.doc);
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        std::fs::write(&doc_path, self.source).unwrap();
        let db = dir.join("index.db");
        let index = declared_index(language, self.doc, self.source, &self.decls);
        silent_cartographer::commands::build_from_index(
            &db,
            "poscrate-ws",
            dir,
            &index,
            &[(self.doc.to_string(), self.source.to_string())],
        )
        .unwrap();
        db
    }

    /// The identity of the declaration whose descriptor names are `path`.
    pub fn id(&self, path: &[&str]) -> CanonicalId {
        let decl = self
            .decls
            .iter()
            .find(|d| d.segments.iter().map(|s| s.0).eq(path.iter().copied()))
            .unwrap_or_else(|| panic!("no declaration {path:?}"));
        decl_id(decl.segments)
    }

    /// The identity of the document's own module.
    pub fn module_id(&self) -> CanonicalId {
        decl_id(&[("module", SegmentKind::Module)])
    }

    /// The 1-based line of the first occurrence of `needle` in the source.
    pub fn line(&self, needle: &str) -> u32 {
        line_number(self.source, needle)
    }
}

const RUST_HEADER_SOURCE: &str = "\
#[derive(Debug)]
pub struct P { pub x: u32, pub y: u32 }

pub struct Q {
    pub size: u32,
}

impl P {
    pub fn get(&self, a: u32) -> u32 {
        a + 1
    }

    pub fn wide(
        &self,
        a: u32,
    ) -> u32 {
        a + 2
    }
}

pub const K: u32 = 1; pub static S: u32 = 2;

pub fn alpha() {} pub fn beta() {}

pub mod md {
    pub fn inner() {}
}
";

const PYTHON_HEADER_SOURCE: &str = "\
LIMIT = 1
ALPHA = 1; BETA = 2


class P: x = 1; y = 2


class Q:
    size: int = 0

    def plain(self, a, b=1):
        return a

    @property
    @cached
    def area(self):
        return 1

    async def wide(
        self,
        value: int,
    ) -> None:
        pass
";

/// The header-shape fixture for `language`; the exhaustive match stops compiling when a backend is
/// added without one.
pub fn header_fixture(language: silent_cartographer::graph::syntax::Language) -> HeaderFixture {
    use SegmentKind::{Method, Module, Term, Type};
    use silent_cartographer::graph::syntax::Language;
    match language {
        Language::Rust => HeaderFixture {
            doc: "src/shapes.rs",
            source: RUST_HEADER_SOURCE,
            decls: vec![
                Decl {
                    segments: &[("P", Type)],
                    kind: SymbolKind::Type,
                    within: "struct P",
                },
                Decl {
                    segments: &[("P", Type), ("x", Term)],
                    kind: SymbolKind::Field,
                    within: "x: u32,",
                },
                Decl {
                    segments: &[("P", Type), ("y", Term)],
                    kind: SymbolKind::Field,
                    within: "y: u32 }",
                },
                Decl {
                    segments: &[("Q", Type)],
                    kind: SymbolKind::Type,
                    within: "struct Q",
                },
                Decl {
                    segments: &[("Q", Type), ("size", Term)],
                    kind: SymbolKind::Field,
                    within: "size: u32",
                },
                Decl {
                    segments: &[("P", Type), ("get", Method)],
                    kind: SymbolKind::Method,
                    within: "get(&self",
                },
                Decl {
                    segments: &[("P", Type), ("wide", Method)],
                    kind: SymbolKind::Method,
                    within: "wide(",
                },
                Decl {
                    segments: &[("K", Term)],
                    kind: SymbolKind::Constant,
                    within: "K: u32",
                },
                Decl {
                    segments: &[("S", Term)],
                    kind: SymbolKind::Constant,
                    within: "S: u32",
                },
                Decl {
                    segments: &[("alpha", Method)],
                    kind: SymbolKind::Function,
                    within: "alpha()",
                },
                Decl {
                    segments: &[("beta", Method)],
                    kind: SymbolKind::Function,
                    within: "beta()",
                },
                Decl {
                    segments: &[("md", Module)],
                    kind: SymbolKind::Module,
                    within: "md {",
                },
                Decl {
                    segments: &[("md", Module), ("inner", Method)],
                    kind: SymbolKind::Function,
                    within: "inner()",
                },
            ],
        },
        Language::Python => HeaderFixture {
            doc: "shapes.py",
            source: PYTHON_HEADER_SOURCE,
            decls: vec![
                Decl {
                    segments: &[("LIMIT", Term)],
                    kind: SymbolKind::Constant,
                    within: "LIMIT",
                },
                Decl {
                    segments: &[("ALPHA", Term)],
                    kind: SymbolKind::Constant,
                    within: "ALPHA",
                },
                Decl {
                    segments: &[("BETA", Term)],
                    kind: SymbolKind::Constant,
                    within: "BETA",
                },
                Decl {
                    segments: &[("P", Type)],
                    kind: SymbolKind::Type,
                    within: "P:",
                },
                Decl {
                    segments: &[("P", Type), ("x", Term)],
                    kind: SymbolKind::Field,
                    within: "x = 1",
                },
                Decl {
                    segments: &[("P", Type), ("y", Term)],
                    kind: SymbolKind::Field,
                    within: "y = 2",
                },
                Decl {
                    segments: &[("Q", Type)],
                    kind: SymbolKind::Type,
                    within: "Q:",
                },
                Decl {
                    segments: &[("Q", Type), ("size", Term)],
                    kind: SymbolKind::Field,
                    within: "size:",
                },
                Decl {
                    segments: &[("Q", Type), ("plain", Method)],
                    kind: SymbolKind::Method,
                    within: "plain(",
                },
                Decl {
                    segments: &[("Q", Type), ("plain", Method), ("self", Term)],
                    kind: SymbolKind::Other,
                    within: "self, a",
                },
                Decl {
                    segments: &[("Q", Type), ("plain", Method), ("a", Term)],
                    kind: SymbolKind::Other,
                    within: "a, b=1",
                },
                Decl {
                    segments: &[("Q", Type), ("plain", Method), ("b", Term)],
                    kind: SymbolKind::Other,
                    within: "b=1",
                },
                Decl {
                    segments: &[("Q", Type), ("area", Method)],
                    kind: SymbolKind::Method,
                    within: "area(",
                },
                Decl {
                    segments: &[("Q", Type), ("wide", Method)],
                    kind: SymbolKind::Method,
                    within: "wide(",
                },
                Decl {
                    segments: &[("Q", Type), ("wide", Method), ("self", Term)],
                    kind: SymbolKind::Other,
                    within: "self,\n        value",
                },
                Decl {
                    segments: &[("Q", Type), ("wide", Method), ("value", Term)],
                    kind: SymbolKind::Other,
                    within: "value: int",
                },
            ],
        },
    }
}
