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

/// How the workspace a store was built for relates to the workspace being queried, disclosed on an
/// answer whenever the relationship is anything other than a match.
///
/// A match carries no disclosure at all, so a matched answer's shape is unchanged. The two disclosed
/// states are kept apart because they license different reactions: a mismatch says the graph
/// describes somewhere else, while an unevaluable comparison says only that the question could not be
/// answered — which is never the same as an answered "they match".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkspaceRelation {
    /// The store records a different workspace root than the one the query was invoked against.
    Mismatched {
        /// The workspace root the store was built for.
        recorded_root: String,
    },
    /// The comparison could not be evaluated, so the relationship is unknown — never implied to
    /// match.
    Unknown,
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

/// The 1-based line window a windowed `get` content answer covers, disclosed whenever the returned
/// content is not the whole tier text so a caller knows which slice it holds and how much text
/// exists. Absent when the answer carries the whole content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ContentLines {
    /// The 1-based line the window starts at.
    pub start: usize,
    /// The 1-based line the window ends at (inclusive); one less than `start` for an empty window
    /// requested past the end of the content.
    pub end: usize,
    /// The total number of lines in the full tier text.
    pub total: usize,
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
        /// The candidate symbols, capped at the effective result limit.
        candidates: Vec<SymbolView>,
        /// The total number of candidates when the list was capped by the limit, so a caller knows
        /// how many were withheld; absent when every candidate is present.
        #[serde(skip_serializing_if = "Option::is_none")]
        candidates_total: Option<usize>,
    },
    /// A definite "none" — the query succeeded and found nothing, distinct from a failure.
    Absent,
    /// The relation exists but the subject stands in no instance of it — a definite empty set.
    Empty,
}

/// The result-set paging disclosure carried by a bounded answer.
///
/// Present only when the answer is a partial view — the result set was truncated to the limit, or a
/// later page was resumed; absent when the whole set fits on the first page (including an unbounded
/// answer), so a complete answer's shape is unchanged. When the returned page does not exhaust the
/// matched results, `truncated` is true and `cursor` carries the opaque token that resumes the next
/// page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageInfo {
    /// Whether matched results remain beyond this page.
    pub truncated: bool,
    /// The zero-based index of the page returned.
    pub page_index: usize,
    /// How many results this page carries.
    pub returned: usize,
    /// How many results matched in total, across every page.
    pub total: usize,
    /// The opaque continuation token that resumes the next page, present only when `truncated`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// A complete query answer: provenance, freshness, staleness, the outcome, and any paging disclosure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Answer<T> {
    /// The analyzer provenance.
    pub provenance: Provenance,
    /// The freshness label.
    pub freshness: FreshnessLabel,
    /// Whether the answer is stale (either reason).
    pub stale: bool,
    /// The heuristic-grade marker: `Some("convention")` when the answer derives from
    /// convention-based classification rather than resolved semantic fact (the `tests` relation).
    /// Absent for relations derived only from resolved reference evidence, so their shape is
    /// unchanged. Rides independently of provenance and freshness, never replacing either.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<&'static str>,
    /// The workspace-relationship disclosure: present when the store describes a different workspace
    /// than the one queried, or when that comparison could not be evaluated; absent on a match, so a
    /// matched answer's shape is unchanged. Rides independently of freshness — a store can be current
    /// and still describe somewhere else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_relation: Option<WorkspaceRelation>,
    /// The outcome.
    pub outcome: Outcome<T>,
    /// The result-set paging disclosure, present only when a result limit was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<PageInfo>,
}

impl<T> Answer<T> {
    /// An answer carrying found results.
    pub fn found(results: Vec<T>, provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Found { results }, provenance, freshness)
    }

    /// An answer carrying a typed candidate set (ambiguous reference). The candidate list is capped
    /// later, in the shared pagination step, so the engine produces the full set here.
    pub fn ambiguous(candidates: Vec<SymbolView>, provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(
            Outcome::Ambiguous {
                candidates,
                candidates_total: None,
            },
            provenance,
            freshness,
        )
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
            classification: None,
            workspace_relation: None,
            outcome,
            page: None,
        }
    }

    /// Mark the answer as derived from convention-based classification — the structural
    /// heuristic-grade marker every `tests` answer carries, found and empty alike.
    pub fn convention_classified(mut self) -> Self {
        self.classification = Some("convention");
        self
    }

    /// Attach the workspace-relationship disclosure, if any: `None` for a matched workspace, which
    /// carries no marker.
    pub fn with_workspace_relation(mut self, relation: Option<WorkspaceRelation>) -> Self {
        self.workspace_relation = relation;
        self
    }
}

impl<T: Serialize> Answer<T> {
    /// Render the answer as structured JSON (the `--json` output).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
