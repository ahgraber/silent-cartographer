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
