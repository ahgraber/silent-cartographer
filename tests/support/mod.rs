//! Shared test fixtures: a small, hand-built extracted index over a synthetic Rust workspace.
//!
//! The fixture models a workspace with a module `net` containing a type `Client` with methods
//! `connect` and `disconnect`, a free function `open` that calls `connect`, and a reference to an
//! external `std` symbol. It exercises definitions, references, enclosure, method/closure/module
//! attribution, external symbols, and a text-mismatch case.

#![allow(dead_code)]

use silent_cartographer::identity::{Descriptor, DescriptorSegment, SegmentKind};
use silent_cartographer::semantic::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};

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
    }
}
