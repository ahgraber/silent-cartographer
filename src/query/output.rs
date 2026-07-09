//! The calibrated output contract: the shape every query answer carries.
//!
//! Every answer carries its provenance and freshness, identifies each symbol by both its canonical
//! identity and a human-readable name, represents an empty result as typed absence distinct from a
//! failure, orders results deterministically, and serializes to JSON.

use serde::Serialize;

use crate::graph::store::Freshness;
use crate::identity::CanonicalId;

/// The analyzer provenance carried on every answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// The analyzer name.
    pub analyzer_name: String,
    /// The analyzer version.
    pub analyzer_version: String,
}

/// The freshness label carried on every answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessLabel {
    /// Sources, analyzer, and declared environment unchanged since indexing.
    Fresh,
    /// The source content changed since indexing.
    StaleContent,
    /// The analyzer version differs from the recorded provenance.
    StaleVersion,
    /// The recorded interpreter environment differs from the one in effect.
    StaleEnvironment,
}

impl From<Freshness> for FreshnessLabel {
    fn from(f: Freshness) -> Self {
        match f {
            Freshness::Fresh => FreshnessLabel::Fresh,
            Freshness::StaleContent => FreshnessLabel::StaleContent,
            Freshness::StaleVersion => FreshnessLabel::StaleVersion,
            Freshness::StaleEnvironment => FreshnessLabel::StaleEnvironment,
        }
    }
}

/// The identity+name view of a symbol carried in every answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolView {
    /// The stable canonical identity (source of truth).
    pub canonical_id: CanonicalId,
    /// A human-readable name (the descriptor's terminal segment).
    pub name: String,
    /// The symbol kind tag.
    pub kind: String,
    /// Whether the symbol is external (no source in this workspace).
    pub external: bool,
}

/// A source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// The document the location is in.
    pub document_path: String,
    /// The start byte offset.
    pub span_start: usize,
    /// The end byte offset.
    pub span_end: usize,
}

/// The outcome of a query: a found result set, a typed candidate set on ambiguity, or typed absence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome<T> {
    /// Results, ordered deterministically.
    Found {
        /// The result payload.
        results: Vec<T>,
    },
    /// The reference denoted more than one symbol; the caller must narrow.
    Ambiguous {
        /// The candidate symbols.
        candidates: Vec<SymbolView>,
    },
    /// A definite "none" — the query succeeded and found nothing, distinct from a failure.
    Absent,
    /// The relation exists but the subject stands in no instance of it — a definite empty set.
    Empty,
}

/// A complete query answer: provenance, freshness, staleness, and the outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Answer<T> {
    /// The analyzer provenance.
    pub provenance: Provenance,
    /// The freshness label.
    pub freshness: FreshnessLabel,
    /// Whether the answer is stale (either reason).
    pub stale: bool,
    /// The outcome.
    pub outcome: Outcome<T>,
}

impl<T> Answer<T> {
    /// An answer carrying found results.
    pub fn found(results: Vec<T>, provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Found { results }, provenance, freshness)
    }

    /// An answer carrying a typed candidate set (ambiguous reference).
    pub fn ambiguous(candidates: Vec<SymbolView>, provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Ambiguous { candidates }, provenance, freshness)
    }

    /// An answer carrying typed absence (no symbol denoted).
    pub fn absent(provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Absent, provenance, freshness)
    }

    /// An answer carrying a definite empty relation.
    pub fn empty(provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Empty, provenance, freshness)
    }

    fn wrap(outcome: Outcome<T>, provenance: Provenance, freshness: Freshness) -> Self {
        Self {
            provenance,
            freshness: freshness.into(),
            stale: freshness.is_stale(),
            outcome,
        }
    }
}

impl<T: Serialize> Answer<T> {
    /// Render the answer as structured JSON (the `--json` output).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
