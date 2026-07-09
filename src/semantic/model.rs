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

/// The interpreter-environment facts a backend declares material to an index's meaning: what the
/// index's symbols resolved against. Declared by the Python adapter; absent for backends (like
/// Rust's) whose resolution does not depend on an activated environment. Staleness compares the
/// whole struct, so any drift — interpreter, environment path, or installed package set — marks
/// derived results stale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentFacts {
    /// The interpreter's reported version (the `python --version` output).
    pub interpreter_version: String,
    /// The resolved environment's path.
    pub environment_path: String,
    /// The installed-package fingerprint: a hash over the sorted `*.dist-info` directory names
    /// under the environment's site-packages, so installs, upgrades, and removals all change it.
    pub package_fingerprint: String,
}

/// A source document referenced by the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    /// The workspace-relative path of the document.
    pub path: String,
    /// The document's declared position encoding.
    pub encoding: PositionEncoding,
}

/// The non-definition occurrences of a descriptor shared by more than one distinct in-workspace
/// definition, addressed by the group rather than exclusively assigned to any one definition.
///
/// Produced by [`normalize`] when a backend emits more than one definition occurrence under an
/// identical resolved descriptor: the group keeps every reference occurrence of that descriptor so
/// downstream attribution (locality, then the guarded join) can settle each one, or refuse it
/// honestly, without extraction itself guessing which twin it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroup {
    /// The descriptor shared by the group's definitions.
    pub descriptor: Descriptor,
    /// The group's non-definition occurrences, unassigned to any single definition.
    pub occurrences: Vec<ExtractedOccurrence>,
}

/// The full result a backend produces for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedIndex {
    /// The analyzer provenance of this index.
    pub provenance: AnalyzerProvenance,
    /// The documents referenced.
    pub documents: Vec<SourceDocument>,
    /// The symbols the backend resolved. After [`normalize`], an in-workspace symbol carries at most
    /// one definition occurrence; a descriptor whose backend output carried more than one is split
    /// into one symbol per definition, with the group's non-definition occurrences moved to
    /// `duplicate_groups`.
    pub symbols: Vec<ExtractedSymbol>,
    /// Non-definition occurrences of a duplicated descriptor, addressed by the group rather than
    /// attributed to any single definition. Populated by [`normalize`]; empty for an index with no
    /// same-descriptor duplicate definitions.
    #[serde(default)]
    pub duplicate_groups: Vec<DuplicateGroup>,
    /// The build system's authoritative library-target roots: package name → workspace-relative
    /// document path of that package's library-target root (matching SCIP document paths). Supplied
    /// by the backend as enrichment (e.g. from `cargo metadata --no-deps`); empty when unavailable —
    /// consumers degrade to their typed refusals, never guess.
    #[serde(default)]
    pub library_roots: std::collections::BTreeMap<String, String>,
    /// The interpreter-environment facts the backend declares material to this index's meaning.
    /// Declared by the Python adapter; `None` for backends without an environment dependency (Rust).
    #[serde(default)]
    pub environment: Option<EnvironmentFacts>,
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

/// Split every in-workspace symbol carrying more than one definition occurrence into one symbol per
/// definition, moving the group's non-definition occurrences to a group-addressed collection.
///
/// This is the backend-neutral normalization pass every backend's [`ExtractedIndex`] flows through
/// before identity projection: a backend that accumulates occurrences by descriptor string (e.g. one
/// `ExtractedSymbol` per SCIP symbol string) may merge two genuinely distinct definitions that happen
/// to share an identical resolved descriptor. Splitting here — rather than in each adapter — makes
/// every backend's output hold the semantic-engine contract that same-descriptor definitions are
/// never merged, without adapters needing to know about duplicates at all.
///
/// A symbol with zero or one definition occurrence (including an external symbol, which by
/// definition has none) passes through unchanged. A symbol with more than one definition occurrence
/// is split: each definition occurrence becomes its own symbol (carrying only that one definition
/// occurrence, no references), and every non-definition occurrence of the group moves to a
/// [`DuplicateGroup`] keyed by the shared descriptor.
pub fn normalize(index: ExtractedIndex) -> ExtractedIndex {
    let mut symbols = Vec::with_capacity(index.symbols.len());
    let mut duplicate_groups = index.duplicate_groups;

    for symbol in index.symbols {
        let definition_count = symbol
            .occurrences
            .iter()
            .filter(|o| o.role == OccurrenceRole::Definition)
            .count();
        if definition_count <= 1 {
            symbols.push(symbol);
            continue;
        }

        let ExtractedSymbol {
            descriptor,
            kind,
            class,
            occurrences,
        } = symbol;
        let mut references = Vec::new();
        for occ in occurrences {
            if occ.role == OccurrenceRole::Definition {
                symbols.push(ExtractedSymbol {
                    descriptor: descriptor.clone(),
                    kind,
                    class: class.clone(),
                    occurrences: vec![occ],
                });
            } else {
                references.push(occ);
            }
        }
        if let Some(descriptor) = descriptor
            && !references.is_empty()
        {
            duplicate_groups.push(DuplicateGroup {
                descriptor,
                occurrences: references,
            });
        }
    }

    ExtractedIndex {
        provenance: index.provenance,
        documents: index.documents,
        symbols,
        duplicate_groups,
        library_roots: index.library_roots,
        environment: index.environment,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::DescriptorSegment;

    fn descriptor(package: &str, name: &str, kind: SegmentKind) -> Descriptor {
        Descriptor::new(package, vec![DescriptorSegment::new(name, kind)])
    }

    fn def_occ(document_path: &str, line: u32) -> ExtractedOccurrence {
        ExtractedOccurrence {
            document_path: document_path.to_string(),
            range: SourceRange::new(line, 0, line, 1),
            role: OccurrenceRole::Definition,
        }
    }

    fn ref_occ(document_path: &str, line: u32) -> ExtractedOccurrence {
        ExtractedOccurrence {
            document_path: document_path.to_string(),
            range: SourceRange::new(line, 0, line, 1),
            role: OccurrenceRole::Reference,
        }
    }

    fn index_of(symbols: Vec<ExtractedSymbol>) -> ExtractedIndex {
        ExtractedIndex {
            provenance: AnalyzerProvenance {
                analyzer_name: "test".to_string(),
                analyzer_version: "0".to_string(),
            },
            documents: vec![
                SourceDocument {
                    path: "a.rs".to_string(),
                    encoding: PositionEncoding::Utf8,
                },
                SourceDocument {
                    path: "b.rs".to_string(),
                    encoding: PositionEncoding::Utf8,
                },
            ],
            symbols,
            duplicate_groups: Vec::new(),
            library_roots: Default::default(),
            environment: None,
        }
    }

    // _(Same-descriptor definitions extracted as distinct symbols)_ — two definitions merged under one
    // descriptor by the backend split into two distinct symbols, each anchored at its own definition.
    #[test]
    fn twin_definitions_yield_distinct_symbols() {
        let descriptor = descriptor("c", "Widget", SegmentKind::Type);
        let merged = ExtractedSymbol {
            descriptor: Some(descriptor),
            kind: SymbolKind::Type,
            class: SymbolClass::InWorkspace,
            occurrences: vec![def_occ("a.rs", 0), def_occ("b.rs", 0)],
        };
        let normalized = normalize(index_of(vec![merged]));

        assert_eq!(normalized.symbols.len(), 2, "one symbol per definition occurrence");
        let mut docs: Vec<&str> = normalized
            .symbols
            .iter()
            .map(|s| {
                s.definition()
                    .expect("each twin keeps its own definition")
                    .document_path
                    .as_str()
            })
            .collect();
        docs.sort();
        assert_eq!(
            docs,
            vec!["a.rs", "b.rs"],
            "each twin anchored at its own definition location"
        );
        for twin in &normalized.symbols {
            assert_eq!(
                twin.occurrences.len(),
                1,
                "a split twin carries only its own definition"
            );
        }
    }

    // _(References to a duplicated descriptor survive extraction unassigned)_ — the group's reference
    // occurrences land in the group-addressed collection, on no single twin.
    #[test]
    fn duplicated_references_move_to_the_group_addressed_collection() {
        let descriptor = descriptor("c", "Widget", SegmentKind::Type);
        let merged = ExtractedSymbol {
            descriptor: Some(descriptor.clone()),
            kind: SymbolKind::Type,
            class: SymbolClass::InWorkspace,
            occurrences: vec![
                def_occ("a.rs", 0),
                def_occ("b.rs", 0),
                ref_occ("a.rs", 1),
                ref_occ("b.rs", 1),
            ],
        };
        let normalized = normalize(index_of(vec![merged]));

        for twin in &normalized.symbols {
            assert!(
                twin.occurrences.iter().all(|o| o.role == OccurrenceRole::Definition),
                "no twin carries a reference occurrence: {:?}",
                twin.occurrences
            );
        }
        assert_eq!(
            normalized.duplicate_groups.len(),
            1,
            "one group for the duplicated descriptor"
        );
        let group = &normalized.duplicate_groups[0];
        assert_eq!(group.descriptor, descriptor);
        assert_eq!(group.occurrences.len(), 2, "both references moved to the group");
        assert!(group.occurrences.iter().all(|o| o.role == OccurrenceRole::Reference));
    }

    // _(Unique descriptors are unaffected)_ — a single-definition symbol passes through unchanged.
    #[test]
    fn single_definition_symbol_passes_through_unchanged() {
        let descriptor = descriptor("c", "solo", SegmentKind::Method);
        let symbol = ExtractedSymbol {
            descriptor: Some(descriptor),
            kind: SymbolKind::Method,
            class: SymbolClass::InWorkspace,
            occurrences: vec![def_occ("a.rs", 0), ref_occ("a.rs", 1), ref_occ("b.rs", 2)],
        };
        let before = index_of(vec![symbol]);
        let normalized = normalize(before.clone());

        assert_eq!(
            normalized.symbols, before.symbols,
            "definition and references stay intact"
        );
        assert!(
            normalized.duplicate_groups.is_empty(),
            "no group formed for a unique descriptor"
        );
    }

    // An external symbol (reference-only, no definition occurrence) is never split: zero definitions
    // takes the same pass-through path as exactly one.
    #[test]
    fn external_symbol_with_no_definition_is_never_split() {
        let descriptor = descriptor("thirdparty", "g", SegmentKind::Method);
        let external = ExtractedSymbol {
            descriptor: Some(descriptor),
            kind: SymbolKind::Function,
            class: SymbolClass::External,
            occurrences: vec![ref_occ("a.rs", 0), ref_occ("b.rs", 1)],
        };
        let before = index_of(vec![external]);
        let normalized = normalize(before.clone());

        assert_eq!(
            normalized.symbols, before.symbols,
            "an external symbol is untouched by the split"
        );
        assert!(normalized.duplicate_groups.is_empty());
    }
}
