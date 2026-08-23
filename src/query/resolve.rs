//! Reference resolution across the three addressing forms.
//!
//! A reference is supplied in one of three forms along a user-facingness axis that runs inversely to
//! uniqueness: a canonical identity (exact), a qualified name (usually unique), or a shortname
//! (low uniqueness). Resolution accepts any form and resolves up toward identity; an ambiguous lower
//! form yields a typed candidate set rather than an arbitrary choice.

use crate::graph::store::{GraphStore, SymbolRow};

/// The outcome of resolving a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The reference denoted exactly one symbol.
    Unique(SymbolRow),
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
        return Ok(Resolution::Unique(row));
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

fn classify(candidates: Vec<SymbolRow>) -> Resolution {
    match candidates.len() {
        0 => Resolution::None,
        1 => Resolution::Unique(candidates.into_iter().next().expect("one candidate")),
        _ => Resolution::Ambiguous(candidates),
    }
}
