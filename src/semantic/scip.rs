//! Language-neutral SCIP-protobuf translation: parsing SCIP symbol strings and ranges, and
//! translating a full SCIP index into the engine-neutral model.
//!
//! This module is pure and adapter-agnostic — any backend that produces a SCIP index (Rust via
//! `rust-analyzer`, Python via `scip-python`) translates through the same code, so the translation's
//! bug-fix surface is not duplicated per language.

use std::collections::BTreeMap;

use scip::symbol::parse_symbol;
use scip::types as scip_types;

use super::model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};
use crate::identity::{Descriptor, DescriptorSegment, SegmentKind};

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
pub(super) fn translate_suffix(suffix: scip_types::descriptor::Suffix) -> SegmentKind {
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
pub(super) fn read_range(range: &[i32]) -> Option<SourceRange> {
    match range {
        [sl, sc, ec] => Some(SourceRange::new(*sl as u32, *sc as u32, *sl as u32, *ec as u32)),
        [sl, sc, el, ec] => Some(SourceRange::new(*sl as u32, *sc as u32, *el as u32, *ec as u32)),
        _ => None,
    }
}

/// The bit corresponding to SCIP's `SymbolRole::Definition` in an occurrence's `symbol_roles` bitset.
pub(super) const DEFINITION_ROLE_BIT: i32 = 1;

/// Map a symbol kind from the SCIP terminal descriptor suffix.
///
/// SCIP gives every callable the method suffix, so a callable is a method only when it hangs off a
/// type (`Type#m()`, or rust-analyzer's `impl#[Type]m()`); any other parent — a module, or an
/// enclosing function for a nested one — makes it a function.
pub(super) fn symbol_kind_from(descriptor: &Descriptor) -> SymbolKind {
    match descriptor.segments.last().map(|s| s.kind) {
        Some(SegmentKind::Module) => SymbolKind::Module,
        Some(SegmentKind::Type) => SymbolKind::Type,
        Some(SegmentKind::Method) => {
            let parent = descriptor.segments.iter().rev().nth(1).map(|s| s.kind);
            match parent {
                Some(SegmentKind::Type | SegmentKind::TypeParameter) => SymbolKind::Method,
                _ => SymbolKind::Function,
            }
        }
        Some(SegmentKind::Term) => SymbolKind::Constant,
        _ => SymbolKind::Other,
    }
}

/// Translate a full SCIP index into the engine-neutral model.
///
/// Symbols with a definition occurrence in the index are classified in-workspace; symbols that only
/// appear as references (no definition) are classified external — a first-class reference target
/// with no definition span. Occurrences with no usable symbol identity (local symbols and symbol
/// strings that do not parse) have no global descriptor, so only their ranges are kept, per document.
pub fn translate_index(index: &scip_types::Index, provenance: &AnalyzerProvenance) -> ExtractedIndex {
    let mut documents = Vec::new();
    let mut local_occurrence_ranges: BTreeMap<String, Vec<SourceRange>> = BTreeMap::new();
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
            // An occurrence with no symbol names nothing, such as one emitted only for highlighting,
            // so it neither persists nor counts as resolving a call.
            if occ.symbol.is_empty() {
                continue;
            }
            let Some(range) = read_range(&occ.range) else {
                continue;
            };
            let parsed = parse_scip_symbol(&occ.symbol);
            if parsed.descriptor.is_none() {
                local_occurrence_ranges
                    .entry(doc.relative_path.clone())
                    .or_default()
                    .push(range);
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
        targets: Default::default(),
        environment: None,
        local_occurrence_ranges,
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
    fn callable_kind_follows_its_enclosing_segment() {
        // Free function, inherent method, trait method, trait-impl method (`impl#[Type]`), nested function.
        for (symbol, expected) in [
            ("rust-analyzer cargo c 0.1.0 net/open().", SymbolKind::Function),
            ("rust-analyzer cargo c 0.1.0 net/Client#connect().", SymbolKind::Method),
            ("rust-analyzer cargo c 0.1.0 net/Conn#close().", SymbolKind::Method),
            (
                "rust-analyzer cargo c 0.1.0 net/impl#[`Client`]fmt().",
                SymbolKind::Method,
            ),
            ("scip-python python p 0 `pkg.mod`/build().", SymbolKind::Function),
            ("scip-python python p 0 `pkg.mod`/Widget#size().", SymbolKind::Method),
            (
                "scip-python python p 0 `pkg.mod`/build().inner().",
                SymbolKind::Function,
            ),
        ] {
            let descriptor = parse_scip_symbol(symbol).descriptor.unwrap();
            assert_eq!(symbol_kind_from(&descriptor), expected, "{symbol}");
        }
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

    #[test]
    fn occurrences_without_a_usable_symbol_keep_only_their_range() {
        use scip::types::{Document, Index, Occurrence};

        let occurrence = |symbol: &str, range: Vec<i32>| {
            let mut occ = Occurrence::new();
            occ.symbol = symbol.to_string();
            occ.range = range;
            occ
        };
        let mut doc = Document::new();
        doc.relative_path = "m.py".to_string();
        doc.occurrences = vec![
            occurrence("local 7", vec![0, 0, 1]),
            occurrence("not a scip symbol", vec![1, 0, 1]),
            occurrence("scip-python python p 0 f().", vec![2, 0, 1]),
            occurrence("", vec![3, 0, 1]),
        ];
        let mut index = Index::new();
        index.documents = vec![doc];

        let provenance = AnalyzerProvenance {
            analyzer_name: "scip-python".to_string(),
            analyzer_version: "test".to_string(),
        };
        let translated = translate_index(&index, &provenance);

        assert_eq!(
            translated.local_occurrence_ranges["m.py"],
            [SourceRange::new(0, 0, 0, 1), SourceRange::new(1, 0, 1, 1)]
        );
        assert_eq!(translated.symbols.len(), 1, "only the parsable symbol is persisted");
    }
}
