//! The calibrated output contract: the shape every query answer carries.
//!
//! Every answer carries its provenance and freshness, identifies each symbol by both its canonical
//! identity and a human-readable name, represents an empty result as typed absence distinct from a
//! failure, orders results deterministically, and serializes to JSON.

use serde::Serialize;

use crate::graph::range::LineColumnSpan;
use crate::graph::store::Freshness;
use crate::identity::CanonicalId;
use crate::query::OrderMode;
use crate::query::search::RelationLabel;

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

/// The grade and guaranteed fragment length every `clones` answer that reaches the clone evidence
/// carries: its rows are deterministic token equivalence, never behavioral equivalence.
///
/// Flattened into the answer, so `grade` and `guaranteed_fragment_length` are sibling keys. Absent
/// from every other command's answers, and from a `clones` answer that ends at reference
/// resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenEquivalence {
    /// Always `token_equivalence`.
    pub grade: &'static str,
    /// The length, in tokens, of the shortest shared run the store's recorded fingerprint
    /// parameters guarantee to find.
    pub guaranteed_fragment_length: usize,
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

/// The line and column of a span's start and exclusive end, serialized as four flat fields beside
/// the span's byte offsets; all `null` where the position is absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LineColumns {
    /// The start line, from 1.
    pub start_line: Option<u32>,
    /// The start column, from 1, counting Unicode scalar values.
    pub start_column: Option<u32>,
    /// The line of the position after the span's last character, from 1.
    pub end_line: Option<u32>,
    /// The column of the position after the span's last character, from 1.
    pub end_column: Option<u32>,
}

impl From<Option<LineColumnSpan>> for LineColumns {
    fn from(span: Option<LineColumnSpan>) -> Self {
        Self {
            start_line: span.map(|s| s.start.line),
            start_column: span.map(|s| s.start.column),
            end_line: span.map(|s| s.end.line),
            end_column: span.map(|s| s.end.column),
        }
    }
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
    /// The same span as line and column.
    #[serde(flatten)]
    pub line_column: LineColumns,
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

/// The most candidate identities an unconfirmed use lists.
pub const UNCONFIRMED_CANDIDATES_MAX: usize = 10;

/// One call named like a `references` subject that no semantic occurrence covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedCallView {
    /// The receiver's source text; absent for a call with no receiver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Whether `receiver` was cut; absent exactly when `receiver` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_shortened: Option<bool>,
    /// The called name's position.
    pub location: Location,
    /// The declaration, else the module, the call sits in; absent when the document has no persisted
    /// module symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing: Option<CanonicalId>,
}

/// The calls named like the subject that the indexer left unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedCalls {
    /// The called name matched: the last component of the subject's name.
    pub name: String,
    /// How many matching calls exist, before any cap.
    pub total: usize,
    /// Whether the cap dropped calls.
    pub truncated: bool,
    /// The calls, by document path then position.
    pub calls: Vec<UnresolvedCallView>,
}

/// One reference occurrence the join refused that may denote the subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnconfirmedUse {
    /// The document the occurrence sits in; present even when `location` is absent.
    pub document_path: String,
    /// The occurrence's position; absent when the discrepancy recorded no span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// The discrepancy's outcome kind.
    pub reason: String,
    /// Up to [`UNCONFIRMED_CANDIDATES_MAX`] symbols a duplicate-ambiguous use may denote, in
    /// canonical-identity order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<Vec<CanonicalId>>,
    /// How many symbols the use may denote; present exactly when `candidates` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_total: Option<usize>,
    /// Whether `candidates` lists fewer than `candidates_total`; present exactly when `candidates` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates_shortened: Option<bool>,
    /// The source text found at the position, as the discrepancy stored it; absent for a
    /// duplicate-ambiguous use and when no text was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_text: Option<String>,
    /// Whether `found_text` was cut; present exactly when `found_text` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_text_shortened: Option<bool>,
}

/// The refused reference occurrences that may denote the subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnconfirmedUses {
    /// How many such uses exist, before any cap.
    pub total: usize,
    /// Whether the cap dropped uses.
    pub truncated: bool,
    /// The uses, by document path then position.
    pub uses: Vec<UnconfirmedUse>,
}

/// One direction of a method's override relations: the methods reached by following override edges in
/// that direction, transitively, and their resolved references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverridePart {
    /// How many methods the walk reached, the subject excluded.
    pub methods: usize,
    /// The total of those methods' resolved reference occurrences.
    pub references: usize,
    /// The `trace` command that lists the directly related methods; present exactly when `methods`
    /// is not zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// The override relations of a `references` subject that is a method, beside its results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideRelations {
    /// The methods that override the subject, directly or through other overrides.
    pub overridden_by: OverridePart,
    /// The methods the subject overrides, directly or through other overrides.
    pub overrides: OverridePart,
    /// The subject the commands name; never serialized.
    #[serde(skip)]
    pub subject: CanonicalId,
}

/// The relation an empty answer came from, for the three whose empty answer is scoped to resolved
/// instances. Read only by the human rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyScope {
    /// `references`: no resolved reference was recorded.
    References,
    /// `dependents`: no resolved dependent was recorded.
    Dependents,
    /// `importers`: no resolved importer was recorded.
    Importers,
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
    /// The subject carries no comparable content, so no comparison was possible — distinct from
    /// `Empty`, where the comparison ran and found no instance. Produced only by `clones`.
    NotComparable,
    /// The relation exists but the index recorded no instance of it for the subject. For a structural
    /// relation this is a definite empty set; for `references`, `dependents`, `importers`, and
    /// `impact` it means only that no resolved instance was recorded.
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
    /// How many results matched in total, across every page; absent on an answer whose walk stops
    /// before it knows the count (`similar --mutual`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
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
    /// The runnable command that refreshes the index this answer came from, present exactly when
    /// `stale` is true so a fresh answer's shape is unchanged. Names the index's location, workspace
    /// identity, and workspace root, and running it makes the same query answer fresh again when the
    /// sources, analyzer, and environment stay as they were when the command was built.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<String>,
    /// The heuristic grade the answer's content carries, with the provenance that grade obliges.
    /// Absent for relations derived only from resolved reference evidence, so their shape is
    /// unchanged. Rides independently of provenance and freshness, never replacing either.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// The token-equivalence grade and guaranteed fragment length of a `clones` answer; absent on
    /// every other answer.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub token_equivalence: Option<TokenEquivalence>,
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
    /// The relation labels a `similar` query excluded, sorted: present exactly when an exclusion was
    /// requested, so an unexcluded answer's shape is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_relations: Option<Vec<RelationLabel>>,
    /// The symbols a `clones` answer did not compare with the subject because the comparison
    /// exceeded the work limit, in canonical-identity order: present exactly when there are any,
    /// so every other answer's shape is unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_compared_completely: Option<Vec<SymbolView>>,
    /// The calls named like a `references` subject that no semantic occurrence covers: present on every
    /// `references` answer whose subject resolves to one symbol, absent on every other answer. It sits
    /// beside the outcome, never among its results, so results and paging counts are unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_calls: Option<UnresolvedCalls>,
    /// The refused reference occurrences that may denote the subject: present exactly when
    /// `unresolved_calls` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unconfirmed_uses: Option<UnconfirmedUses>,
    /// The override relations of a `references` subject of kind `method`: absent for every other
    /// subject and answer. It sits beside the outcome, never among its results, so results and paging
    /// counts are unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_relations: Option<OverrideRelations>,
    /// Which scoped empty message the human rendering prints for an empty outcome; never serialized.
    #[serde(skip)]
    pub empty_scope: Option<EmptyScope>,
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

    /// An answer whose relation holds no recorded instance.
    pub fn empty(provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::Empty, provenance, freshness)
    }

    /// An answer stating the subject has no comparable content.
    pub fn not_comparable(provenance: Provenance, freshness: Freshness) -> Self {
        Self::wrap(Outcome::NotComparable, provenance, freshness)
    }

    fn wrap(outcome: Outcome<T>, provenance: Provenance, freshness: Freshness) -> Self {
        Self {
            provenance,
            freshness: freshness.into(),
            stale: freshness.is_stale(),
            refresh: None,
            classification: None,
            token_equivalence: None,
            workspace_relation: None,
            ordering: None,
            excluded_relations: None,
            not_compared_completely: None,
            unresolved_calls: None,
            unconfirmed_uses: None,
            override_relations: None,
            empty_scope: None,
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

    /// Mark the answer as deterministic token equivalence and state the guaranteed fragment length
    /// in effect — the pair every `clones` answer that reached the clone evidence carries.
    pub fn with_token_equivalence(mut self, guaranteed_fragment_length: usize) -> Self {
        self.token_equivalence = Some(TokenEquivalence {
            grade: "token_equivalence",
            guaranteed_fragment_length,
        });
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

    /// Disclose the relation labels the query excluded, given sorted and deduplicated; no field at
    /// all when none was requested.
    pub fn with_excluded_relations(mut self, excluded: &[RelationLabel]) -> Self {
        self.excluded_relations = (!excluded.is_empty()).then(|| excluded.to_vec());
        self
    }

    /// Name the symbols a `clones` query did not compare completely; no field at all when none.
    pub fn with_not_compared_completely(mut self, symbols: Vec<SymbolView>) -> Self {
        self.not_compared_completely = (!symbols.is_empty()).then_some(symbols);
        self
    }

    /// Attach the unresolved-call and unconfirmed-use groups of a `references` answer.
    pub fn with_reference_groups(mut self, calls: UnresolvedCalls, uses: UnconfirmedUses) -> Self {
        self.unresolved_calls = Some(calls);
        self.unconfirmed_uses = Some(uses);
        self
    }

    /// Attach the override relations of a `references` subject that is a method.
    pub fn with_override_relations(mut self, relations: Option<OverrideRelations>) -> Self {
        self.override_relations = relations;
        self
    }

    /// Fill the `trace` command of each override part that reached methods: `command` receives the
    /// subject and the relation (`implementers` for the first part, `implements` for the second).
    pub fn with_override_commands(mut self, command: impl Fn(&CanonicalId, &str) -> String) -> Self {
        if let Some(relations) = &mut self.override_relations {
            let subject = relations.subject.clone();
            for (part, relation) in [
                (&mut relations.overridden_by, "implementers"),
                (&mut relations.overrides, "implements"),
            ] {
                part.command = (part.methods > 0).then(|| command(&subject, relation));
            }
        }
        self
    }

    /// Scope the human rendering of an empty outcome to the relation it came from.
    pub fn with_empty_scope(mut self, scope: EmptyScope) -> Self {
        self.empty_scope = Some(scope);
        self
    }

    /// Attach the refresh command to a stale answer. A fresh answer is returned unchanged, without
    /// calling `refresh`, so its shape carries no refresh command.
    pub fn with_refresh(mut self, refresh: impl FnOnce() -> String) -> Self {
        if self.stale {
            self.refresh = Some(refresh());
        }
        self
    }
}

impl<T: Serialize> Answer<T> {
    /// Render the answer as structured JSON (the `--json` output).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}
