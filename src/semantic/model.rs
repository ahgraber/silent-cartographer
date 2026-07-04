//! The engine-neutral model a semantic backend emits.
//!
//! These types are deliberately independent of any specific analyzer's wire format so a future
//! non-SCIP backend produces comparable data. The Rust adapter translates `rust-analyzer`'s SCIP
//! output into this model.

use serde::{Deserialize, Serialize};

use crate::identity::{Descriptor, SegmentKind};

/// How a document's ranges count character offsets within a line.
///
/// SCIP documents declare this per document; the join normalizes ranges through it onto byte
/// offsets before matching against the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionEncoding {
    /// Character offsets count UTF-8 code units (bytes).
    Utf8,
    /// Character offsets count UTF-16 code units.
    Utf16,
    /// Character offsets count Unicode scalar values (code points).
    Utf32,
}

/// A half-open source range `[start, end)` in a document, in the document's declared encoding.
///
/// Lines and characters are zero-based, matching SCIP and LSP conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceRange {
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start character (in the document's encoding).
    pub start_char: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end character (in the document's encoding), exclusive.
    pub end_char: u32,
}

impl SourceRange {
    /// Construct a source range.
    pub fn new(start_line: u32, start_char: u32, end_line: u32, end_char: u32) -> Self {
        Self {
            start_line,
            start_char,
            end_line,
            end_char,
        }
    }
}

/// The role an occurrence plays. The contract floor mandates at minimum definition vs reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OccurrenceRole {
    /// A definition site.
    Definition,
    /// A reference (use) site.
    Reference,
}

/// A single occurrence of a symbol in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedOccurrence {
    /// The document-relative path the occurrence sits in.
    pub document_path: String,
    /// The range of the occurrence, in the document's declared encoding.
    pub range: SourceRange,
    /// The role the occurrence plays.
    pub role: OccurrenceRole,
}

/// The syntactic kind of a symbol, carried through from the backend for display and signature
/// slicing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    /// A module.
    Module,
    /// A struct, enum, or other type.
    Type,
    /// A trait.
    Trait,
    /// A free function.
    Function,
    /// A method.
    Method,
    /// A constant or static value.
    Constant,
    /// A field.
    Field,
    /// Anything else the backend resolved.
    Other,
}

/// The class of a symbol with respect to the analyzed workspace.
///
/// The projection to a canonical identity is a total function only over a defined domain; these are
/// the descriptor shapes the projection must account for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolClass {
    /// A symbol defined in the workspace: it has a resolved descriptor and a definition span.
    InWorkspace,
    /// A resolved descriptor with no in-workspace definition (standard-library or third-party).
    /// Persisted as a first-class reference target with no definition span.
    External,
    /// A file-local symbol (parameter, let-binding) with no global descriptor. Excluded from the
    /// persisted base.
    Local,
}

/// A symbol produced by a backend: its resolved descriptor, kind, class, and occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedSymbol {
    /// The resolved descriptor, or `None` for a file-local symbol with no global descriptor.
    pub descriptor: Option<Descriptor>,
    /// The symbol's syntactic kind.
    pub kind: SymbolKind,
    /// The symbol's class relative to the workspace.
    pub class: SymbolClass,
    /// The occurrences of this symbol across the project.
    pub occurrences: Vec<ExtractedOccurrence>,
}

impl ExtractedSymbol {
    /// The definition occurrence, if the backend produced one.
    pub fn definition(&self) -> Option<&ExtractedOccurrence> {
        self.occurrences.iter().find(|o| o.role == OccurrenceRole::Definition)
    }

    /// The terminal name segment of the descriptor — the join's text-equality guard target.
    pub fn terminal_name(&self) -> Option<&str> {
        self.descriptor.as_ref().and_then(|d| d.terminal_name())
    }
}

/// The provenance of an index: the analyzer's identity and version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerProvenance {
    /// The analyzer's name (e.g. `rust-analyzer`).
    pub analyzer_name: String,
    /// The analyzer's version string.
    pub analyzer_version: String,
}

/// A source document referenced by the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    /// The workspace-relative path of the document.
    pub path: String,
    /// The document's declared position encoding.
    pub encoding: PositionEncoding,
}

/// The full result a backend produces for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedIndex {
    /// The analyzer provenance of this index.
    pub provenance: AnalyzerProvenance,
    /// The documents referenced.
    pub documents: Vec<SourceDocument>,
    /// The symbols the backend resolved.
    pub symbols: Vec<ExtractedSymbol>,
}

impl ExtractedIndex {
    /// The declared encoding for a document path, if the index references it.
    pub fn encoding_for(&self, document_path: &str) -> Option<PositionEncoding> {
        self.documents
            .iter()
            .find(|d| d.path == document_path)
            .map(|d| d.encoding)
    }
}

/// Convenience: the segment kind corresponding to a symbol kind, used when a backend must
/// synthesize a descriptor segment.
pub fn segment_kind_for(kind: SymbolKind) -> SegmentKind {
    match kind {
        SymbolKind::Module => SegmentKind::Module,
        SymbolKind::Type | SymbolKind::Trait => SegmentKind::Type,
        SymbolKind::Function | SymbolKind::Method => SegmentKind::Method,
        SymbolKind::Constant | SymbolKind::Field => SegmentKind::Term,
        SymbolKind::Other => SegmentKind::Meta,
    }
}
