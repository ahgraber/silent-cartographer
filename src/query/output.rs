//! The calibrated output contract: the shape every query answer carries.
//!
//! Every answer carries its provenance and freshness, identifies each symbol by both its canonical
//! identity and a human-readable name, represents an empty result as typed absence distinct from a
//! failure, orders results deterministically, and serializes to JSON.

use serde::Serialize;

use crate::graph::store::Freshness;
use crate::identity::CanonicalId;
use crate::query::OrderMode;

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

/// The semantic-index identity a `search`/`similar` answer carries as provenance: the embedding
/// model, the corpus definition, and the chunk parameters the store's semantic representations
/// were built under — the whole recorded identity, so an answer attributes its ranking to the
/// regime that produced it even when the operator varies the parameters between builds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticIndexView {
    /// The embedding model identity (upstream repository at its vendored revision).
    pub model_identity: String,
    /// The corpus definition version the render derives under.
    pub corpus_definition_version: u32,
    /// The chunk size the build ran under, in model tokens.
    pub chunk_size: usize,
    /// The overlap between adjacent chunks the build ran under, in model tokens.
    pub chunk_overlap: usize,
}

/// The heuristic grade an answer's content carries, together with the provenance that grade obliges.
///
/// Absent on an answer derived only from resolved semantic fact, so its shape is unchanged. The
/// grade and its provenance are one value rather than two fields because they are one decision: an
/// estimation-graded answer must carry the semantic-index identity that produced its ranking
/// (`.specs/specs/code-navigation/spec.md:871`), and a convention-graded answer has no such identity
/// to carry. Held apart, either could appear without the other.
///
/// Flattened into the answer, so `classification` and `semantic_index` remain sibling keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "classification", rename_all = "snake_case")]
pub enum Classification {
    /// The answer derives from convention-based classification rather than resolved semantic fact —
    /// the `tests` relation.
    Convention,
    /// The answer's ranking is model-derived estimation — `search` and `similar` — carrying the
    /// semantic-index identity in effect.
    Estimation {
        /// The semantic-index provenance the ranking derives under.
        ///
        /// Not optional: the spec obliges an estimation-graded answer to carry the recorded
        /// identity, so an answer that could not name one is not an answer this type can express.
        /// A store with no recorded identity is refused before an answer is built, rather than
        /// yielding a marker with nothing behind it.
        semantic_index: SemanticIndexView,
    },
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
/// content is not the whole text at that detail so a caller knows which slice it holds and how much
/// exists. Absent when the answer carries the whole content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ContentLines {
    /// The 1-based line the window starts at.
    pub start: usize,
    /// The 1-based line the window ends at (inclusive); one less than `start` for an empty window
    /// requested past the end of the content.
    pub end: usize,
    /// The total number of lines in the full text at that detail.
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
    /// The heuristic grade the answer's content carries, with the provenance that grade obliges.
    /// Absent for relations derived only from resolved reference evidence, so their shape is
    /// unchanged. Rides independently of provenance and freshness, never replacing either.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// The workspace-relationship disclosure: present when the store describes a different workspace
    /// than the one queried, or when that comparison could not be evaluated; absent on a match, so a
    /// matched answer's shape is unchanged. Rides independently of freshness — a store can be current
    /// and still describe somewhere else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_relation: Option<WorkspaceRelation>,
    /// The ordering disclosure every `dependents`/`impact` answer carries, a typed-empty answer
    /// included: which ordering is in effect (`ranked` or `unranked`). `ranked` orders rows within
    /// a distance layer by a structural-importance heuristic — guidance, never resolved semantic
    /// fact; `unranked` derives only from the answer's stable structural keys. Absent on answers
    /// with no orderable rows, so their shape is unchanged; rides beside provenance and freshness,
    /// never replacing either — and distinct from the heuristic-grade `classification` marker,
    /// which speaks to answer content, not ordering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordering: Option<OrderMode>,
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
            ordering: None,
            outcome,
            page: None,
        }
    }

    /// Mark the answer as derived from convention-based classification — the structural
    /// heuristic-grade marker every `tests` answer carries, found and empty alike.
    pub fn convention_classified(mut self) -> Self {
        self.classification = Some(Classification::Convention);
        self
    }

    /// Mark the answer's ranking as model-derived estimation and attach the semantic-index
    /// provenance — the pair every `search`/`similar` answer carries, found and empty alike. The
    /// marker accompanies provenance, freshness, and staleness, never replacing any of them.
    ///
    /// The identity is required: the marker asserts a ranking derived under a particular regime, so
    /// an answer carrying the marker always names it.
    pub fn estimated(mut self, semantic_index: SemanticIndexView) -> Self {
        self.classification = Some(Classification::Estimation { semantic_index });
        self
    }

    /// Attach the workspace-relationship disclosure, if any: `None` for a matched workspace, which
    /// carries no marker.
    pub fn with_workspace_relation(mut self, relation: Option<WorkspaceRelation>) -> Self {
        self.workspace_relation = relation;
        self
    }

    /// Attach the ordering disclosure — the single structural field every `dependents`/`impact`
    /// answer carries, whatever its outcome, naming the ordering in effect.
    pub fn with_ordering(mut self, ordering: OrderMode) -> Self {
        self.ordering = Some(ordering);
        self
    }
}

impl<T: Serialize> Answer<T> {
    /// Render the answer as structured JSON (the `--json` output).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
