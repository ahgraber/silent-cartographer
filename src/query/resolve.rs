//! Reference resolution across the three addressing forms.
//!
//! A reference is supplied in one of three forms along a user-facingness axis that runs inversely to
//! uniqueness: a canonical identity (exact), a qualified name (usually unique), or a shortname
//! (low uniqueness). Resolution accepts any form and resolves up toward identity; an ambiguous lower
//! form yields a typed candidate set rather than an arbitrary choice.

use crate::graph::range::LineColumn;
use crate::graph::store::{GraphStore, SymbolRow};

/// The outcome of resolving a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The reference denoted exactly one symbol.
    Unique(Box<SymbolRow>),
    /// The reference denoted more than one symbol; the caller must narrow.
    Ambiguous(Vec<SymbolRow>),
    /// The reference denoted no symbol.
    None,
}

/// Resolve `reference` against the store.
///
/// The form is inferred from the reference's shape: a fully-qualified identity (matches a
/// `canonical_id` exactly) resolves uniquely; a qualified name (contains `::`) matches by identity
/// suffix; a bare shortname matches by display name. Candidates are ordered deterministically by
/// canonical identity.
pub fn resolve(store: &GraphStore, reference: &str) -> rusqlite::Result<Resolution> {
    // Identity form: an exact canonical-id match.
    if let Some(row) = store.symbol(&crate::identity::CanonicalId::from_raw(reference))? {
        return Ok(Resolution::Unique(Box::new(row)));
    }

    // Qualified-name form: contains a path separator but is not a full identity.
    if reference.contains("::") {
        let mut candidates = store.symbols_by_qualified_suffix(reference)?;
        candidates.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
        return Ok(classify(candidates));
    }

    // Dotted qualified form: a Python-style `pkg.module.Class` name. A dot maps ambiguously onto
    // identity separators (a Python namespace segment's own name contains dots), so candidates are
    // matched by dotted-form suffix: an identity matches when replacing its `::` separators with
    // `.` yields the reference at a `.` boundary.
    if reference.contains('.') {
        let short = reference.rsplit('.').next().unwrap_or(reference);
        let mut candidates: Vec<SymbolRow> = store
            .symbols_by_shortname(short)?
            .into_iter()
            .filter(|row| {
                let dotted = row.canonical_id.as_str().replace("::", ".");
                dotted == reference || dotted.ends_with(&format!(".{reference}"))
            })
            .collect();
        candidates.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
        return Ok(classify(candidates));
    }

    // Shortname form: a bare name.
    let mut candidates = store.symbols_by_shortname(reference)?;
    candidates.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
    Ok(classify(candidates))
}

/// A source position as the caller named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePosition {
    /// A byte offset into the indexed text.
    Byte(usize),
    /// A line and column of the indexed text.
    LineColumn(LineColumn),
    /// A whole line of the indexed text.
    Line(u32),
}

/// Resolve `position` in `document` to the innermost symbol covering it.
///
/// A byte offset or a line and column resolves to the innermost enclosing symbol, or to none. A
/// line resolves to the outermost symbols whose header covers any part of it, or, when no header does,
/// to the innermost symbol enclosing its first column: one is unique, several (none enclosing
/// another) are ambiguous, none is absent.
pub fn resolve_position(store: &GraphStore, document: &str, position: SourcePosition) -> rusqlite::Result<Resolution> {
    Ok(match position {
        SourcePosition::Byte(offset) => single(store.symbol_enclosing_position(document, offset)?),
        SourcePosition::LineColumn(at) => single(store.symbol_enclosing_line_column(document, at)?),
        SourcePosition::Line(line) => classify(store.symbols_for_line(document, line)?),
    })
}

fn single(row: Option<SymbolRow>) -> Resolution {
    row.map_or(Resolution::None, |row| Resolution::Unique(Box::new(row)))
}

fn classify(candidates: Vec<SymbolRow>) -> Resolution {
    match candidates.len() {
        0 => Resolution::None,
        1 => Resolution::Unique(Box::new(candidates.into_iter().next().expect("one candidate"))),
        _ => Resolution::Ambiguous(candidates),
    }
}
