//! Reference resolution across the three addressing tiers.
//!
//! A reference is supplied at one of three tiers along a user-facingness axis that runs inversely to
//! uniqueness: a canonical identity (exact), a qualified name (usually unique), or a shortname
//! (low uniqueness). Resolution accepts any tier and resolves up toward identity; an ambiguous lower
//! tier yields a typed candidate set rather than an arbitrary choice.

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
/// The tier is inferred from the reference's shape: a fully-qualified identity (matches a
/// `canonical_id` exactly) resolves uniquely; a qualified name (contains `::`) matches by identity
/// suffix; a bare shortname matches by display name. Candidates are ordered deterministically by
/// canonical identity.
pub fn resolve(store: &GraphStore, reference: &str) -> rusqlite::Result<Resolution> {
    // Identity tier: an exact canonical-id match.
    if let Some(row) = store.symbol(&crate::identity::CanonicalId::from_raw(reference))? {
        return Ok(Resolution::Unique(row));
    }

    // Qualified-name tier: contains a path separator but is not a full identity.
    if reference.contains("::") {
        let mut candidates = store.symbols_by_qualified_suffix(reference)?;
        candidates.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
        return Ok(classify(candidates));
    }

    // Shortname tier: a bare name.
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
