//! Meaning-based retrieval over the semantic corpus: `search` (natural-language query in, ranked
//! candidate symbols out) and `similar` (subject symbol in, ranked neighbors out with deterministic
//! clone-certainty markers above the estimated ranking).
//!
//! `search` runs two signals over the same persisted render — the embedding vector signal and the FTS5
//! BM25 lexical signal — and fuses their ranks with reciprocal rank fusion (RRF). RRF fuses ranks, not
//! scores, so the signals' incomparable score scales never need calibration; an exact-identifier query
//! is rescued by the lexical signal and a paraphrase query by the vector signal, with no mode flag.
//!
//! Answers expose order only — no cosine, BM25, or RRF numbers. None of those numbers is
//! calibrated, and exposing them invites downstream thresholds that silently break on every model
//! bump. The order is the claim; the answer-level estimation marker states its grade.

use std::collections::{HashMap, HashSet};

use crate::graph::store::{EdgeKind, GraphStore, SymbolRow};
use crate::graph::{corpus, embed};
use crate::identity::CanonicalId;

use super::output::{Answer, Location, Outcome, SemanticIndexView, SymbolView};
use super::resolve::{Resolution, SourcePosition, resolve_position};
use super::{Detail, QueryEngine, QueryError, location_of, projected_content, symbol_view};

/// The RRF constant `k` in `score(d) = Σ 1/(k + rank_d)`: the standard damping that keeps a single
/// signal's top rank from dominating the fusion.
const RRF_K: f64 = 60.0;

/// The deterministic clone-certainty marker a `similar` row can carry, derived from the persisted
/// equivalence keys — never from estimated similarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneCertainty {
    /// The row shares the subject's formatting-insensitive key: identical token sequences, the
    /// sources differing only in whitespace and comments.
    ExactClone,
    /// The row shares the subject's substitution-insensitive key but not its formatting-insensitive
    /// key: identical token structure with only identifiers and literal values substituted — never
    /// a claim of behavioral equivalence.
    VariantClone,
}

/// A graph relation between a `similar` row's symbol and the subject. Each label states a persisted
/// fact only, never that the row duplicates, overrides, or implements the subject. The declaration
/// order is the order labels appear on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationLabel {
    /// The row's symbol has a `uses` edge to the subject.
    UsesSubject,
    /// The subject has a `uses` edge to the row's symbol.
    UsedBySubject,
    /// The row's symbol is classified test code and has a `uses` edge to the subject.
    TestsSubject,
    /// The row's declaring type and the subject's differ, and one declares the other as a supertype
    /// or both declare a common supertype defined in the workspace.
    TypeFamily,
}

impl RelationLabel {
    /// The label as it appears in answers and on the command line.
    pub fn tag(self) -> &'static str {
        match self {
            RelationLabel::UsesSubject => "uses_subject",
            RelationLabel::UsedBySubject => "used_by_subject",
            RelationLabel::TestsSubject => "tests_subject",
            RelationLabel::TypeFamily => "type_family",
        }
    }
}

/// One relation label on a `similar` row.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RelationView {
    /// The label.
    pub label: RelationLabel,
    /// The convention rule that classified the row's symbol as test code; present only on
    /// `tests_subject`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

/// One `search` result row: a corpus symbol at its estimated-relevance position, carrying its
/// content at the requested detail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SearchItem {
    /// The matched symbol's identity and name.
    pub symbol: SymbolView,
    /// The symbol's definition location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// The symbol's content at the requested detail; absent at location detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Whether that content was truncated by the `--max-lines` bound.
    #[serde(skip_serializing_if = "super::is_false")]
    pub content_truncated: bool,
}

/// One `similar` result row: a corpus symbol at its estimated-similarity position, optionally
/// carrying the deterministic clone-certainty marker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SimilarItem {
    /// The neighboring symbol's identity and name.
    pub symbol: SymbolView,
    /// The deterministic clone-certainty marker, when the row shares an equivalence key with the
    /// subject; absent otherwise, and on every row when the subject carries no keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clone_certainty: Option<CloneCertainty>,
    /// The relation labels that hold between this symbol and the subject, in label order; empty when
    /// none holds. Never alters which rows are returned or their order.
    pub relations: Vec<RelationView>,
    /// The symbol's definition location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// The symbol's content at the requested detail; absent at location detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Whether that content was truncated by the `--max-lines` bound.
    #[serde(skip_serializing_if = "super::is_false")]
    pub content_truncated: bool,
}

impl QueryEngine<'_> {
    /// `search`: corpus symbols ordered by estimated relevance to a natural-language query, most
    /// relevant first — the fused vector and lexical ranking over the persisted renders.
    ///
    /// The detail projection never changes which symbols are returned or their order; each
    /// corpus-contributing symbol appears at most once. An empty corpus is a typed-empty answer.
    pub fn search(
        &self,
        query: &str,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SearchItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let semantic_index = self.semantic_index_view()?;
        let corpus_size = self.store.corpus_size()? as usize;
        if corpus_size == 0 {
            return Ok(Answer::empty(provenance, freshness).estimated(semantic_index));
        }

        // Both signals rank the full corpus; RRF needs whole rank lists, and the corpus is repo-scale.
        // The dense pool is requested in chunks — the store deduplicates to symbols by best chunk.
        let chunk_count = self.store.chunk_count()? as usize;
        let query_vector = embed::vector_bytes(&embed::embed(query));
        let vector_ranks: Vec<CanonicalId> = self
            .store
            .vector_neighbors(&query_vector, chunk_count)?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let words = corpus::split_words(query);
        let lexical_ranks: Vec<CanonicalId> = if words.is_empty() {
            Vec::new()
        } else {
            self.store
                .lexical_neighbors(&fts_match_expr(&words), corpus_size)?
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };

        let fused = rrf_fuse(&[vector_ranks, lexical_ranks]);
        let mut items = Vec::with_capacity(fused.len());
        for id in fused {
            let Some(row) = self.store.symbol(&id)? else {
                return Err(QueryError::MissingSymbol(id));
            };
            items.push(search_item(&row, detail, max_lines));
        }
        Ok(Answer::found(items, provenance, freshness).estimated(semantic_index))
    }

    /// `similar`: the corpus symbols most similar in content to a subject symbol, most similar
    /// first — the same two-signal RRF hybrid `search` uses (the subject's vector neighbors fused
    /// with BM25 over the subject's render words), beneath the deterministic clone-certainty class.
    ///
    /// The subject never appears in its own answer. An ambiguous subject reference yields the typed
    /// candidate set; a corpus offering no other symbol is a typed-empty answer.
    pub fn similar(
        &self,
        reference: &str,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SimilarItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        // An ambiguous or unresolved subject terminates before any ranking is derived, so the
        // answer carries neither the estimation marker nor the semantic-index provenance — the
        // marker asserts a derivation, never merely the command invoked.
        let subject = match self.resolve(reference)? {
            Resolution::Unique(row) => row,
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                return Ok(Answer::ambiguous(views, provenance, freshness));
            }
            Resolution::None => return Ok(Answer::absent(provenance, freshness)),
        };
        self.similar_of(&subject, detail, max_lines)
    }

    /// `similar` by source position: the subject is the symbol `position` resolves to in `document`,
    /// resolved exactly as `get --at` resolves it.
    pub fn similar_by_position(
        &self,
        document: &str,
        position: SourcePosition,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SimilarItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match resolve_position(self.store, document, position)? {
            Resolution::Unique(subject) => self.similar_of(&subject, detail, max_lines),
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                Ok(Answer::ambiguous(views, provenance, freshness))
            }
            // No symbol resolves: no ranking was derived, so no marker or provenance.
            Resolution::None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// The `similar` answer for a resolved subject: the ranked candidates of [`similar_candidates`],
    /// materialized to rows by [`similar_items`].
    fn similar_of(
        &self,
        subject: &SymbolRow,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SimilarItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let semantic_index = self.semantic_index_view()?;
        let candidates = similar_candidates(self.store, subject)?;
        if candidates.is_empty() {
            return Ok(Answer::empty(provenance, freshness).estimated(semantic_index));
        }
        let items = similar_items(self.store, subject, &candidates, detail, max_lines)?;
        Ok(Answer::found(items, provenance, freshness).estimated(semantic_index))
    }

    /// The semantic-index provenance view of the persisted build, if one exists.
    /// The semantic-index identity every estimation-graded answer carries.
    ///
    /// A store recording none is refused here rather than answered without provenance: the marker
    /// asserts a ranking derived under a particular regime, and an answer that cannot name the
    /// regime is not one this surface will produce.
    fn semantic_index_view(&self) -> Result<SemanticIndexView, QueryError> {
        let identity = self
            .store
            .semantic_index_identity()?
            .ok_or(QueryError::MissingSemanticIndex)?;
        Ok(SemanticIndexView {
            model_identity: identity.model_identity,
            corpus_definition_version: identity.corpus_definition_version,
            chunk_size: identity.chunk_params.chunk_size,
            chunk_overlap: identity.chunk_params.overlap,
        })
    }
}

/// One ranked `similar` candidate: a corpus symbol and the clone class it shares with the subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimilarCandidate {
    /// The candidate's canonical identity.
    pub id: CanonicalId,
    /// The deterministic clone-certainty marker; `None` when the candidate shares no key with the subject.
    pub clone_certainty: Option<CloneCertainty>,
}

/// The `similar` ranking for a resolved subject, before paging: the two-signal RRF hybrid — the
/// subject's vector neighbors fused with BM25 of the subject's own render words — with the subject
/// excluded, clone-marked candidates lifted above unmarked ones (exact before variant), fused
/// estimated order within each certainty class.
///
/// Empty when the subject has no persisted content to compare (an external, or a symbol outside the
/// corpus) or the corpus offers no other symbol. Clone classes come from one lookup of the symbols
/// sharing the subject's substitution key, never from a read of each candidate.
pub fn similar_candidates(store: &GraphStore, subject: &SymbolRow) -> Result<Vec<SimilarCandidate>, QueryError> {
    let subject_vectors = store.chunk_vectors_of(&subject.canonical_id)?;
    if subject_vectors.is_empty() {
        return Ok(Vec::new());
    }
    let corpus_size = store.corpus_size()? as usize;
    // Similarity is the best pair over the subject's chunks and each candidate's: each subject
    // chunk's neighbor list arrives deduplicated to candidates by best chunk, and merging by
    // minimum distance across subject chunks completes the maximum over pairs. Neither side
    // gains from the number of chunks representing it.
    let chunk_count = store.chunk_count()? as usize;
    let mut best: std::collections::HashMap<CanonicalId, f64> = std::collections::HashMap::new();
    for subject_vector in &subject_vectors {
        for (id, distance) in store.vector_neighbors(subject_vector, chunk_count)? {
            let entry = best.entry(id).or_insert(distance);
            if distance < *entry {
                *entry = distance;
            }
        }
    }
    let mut nearest: Vec<(CanonicalId, f64)> = best.into_iter().collect();
    nearest.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let vector_ranks: Vec<CanonicalId> = nearest.into_iter().map(|(id, _)| id).collect();
    let lexical_ranks: Vec<CanonicalId> = match store
        .semantic_render_of(&subject.canonical_id)?
        .as_deref()
        .and_then(subject_word_expr)
    {
        Some(expr) => store
            .lexical_neighbors(&expr, corpus_size)?
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
        None => Vec::new(),
    };
    let fused = rrf_fuse(&[vector_ranks, lexical_ranks]);

    let (subject_formatting, clone_matches) = match store.clone_keys_of(&subject.canonical_id)? {
        Some((formatting, substitution)) => (Some(formatting), store.clone_key_matches(&substitution)?),
        None => (None, std::collections::HashMap::new()),
    };
    let mut candidates: Vec<SimilarCandidate> = fused
        .into_iter()
        .filter(|id| *id != subject.canonical_id)
        .map(|id| {
            let clone_certainty = clone_matches.get(&id).map(|formatting| {
                if Some(formatting) == subject_formatting.as_ref() {
                    CloneCertainty::ExactClone
                } else {
                    CloneCertainty::VariantClone
                }
            });
            SimilarCandidate { id, clone_certainty }
        })
        .collect();
    // Certainty classes order ahead of the estimate: exact clones, then variant clones, then
    // unmarked candidates; the stable sort preserves the estimated-similarity order within each class.
    candidates.sort_by_key(|candidate| match candidate.clone_certainty {
        Some(CloneCertainty::ExactClone) => 0,
        Some(CloneCertainty::VariantClone) => 1,
        None => 2,
    });
    Ok(candidates)
}

/// The `similar` rows for `candidates`, in their order: each candidate's symbol row at the requested
/// detail, capped at `max_lines`, with its relation labels against `subject`. A candidate absent from
/// the store is a [`QueryError::MissingSymbol`].
pub fn similar_items(
    store: &GraphStore,
    subject: &SymbolRow,
    candidates: &[SimilarCandidate],
    detail: Detail,
    max_lines: Option<usize>,
) -> Result<Vec<SimilarItem>, QueryError> {
    let mut relations = SubjectRelations::new(store, subject)?;
    candidates
        .iter()
        .map(|candidate| {
            let Some(row) = store.symbol(&candidate.id)? else {
                return Err(QueryError::MissingSymbol(candidate.id.clone()));
            };
            let (content, content_truncated) = projected_row_content(&row, detail, max_lines);
            Ok(SimilarItem {
                symbol: symbol_view(&row),
                clone_certainty: candidate.clone_certainty,
                relations: relations.labels(store, &row)?,
                location: location_of(&row),
                content,
                content_truncated,
            })
        })
        .collect()
}

/// The subject-side facts the relation labels read, gathered once per subject so labeling each
/// candidate costs indexed lookups only.
pub struct SubjectRelations {
    /// The symbols with a `uses` edge to the subject.
    users: HashSet<CanonicalId>,
    /// The symbols the subject has a `uses` edge to.
    used: HashSet<CanonicalId>,
    /// The subject's declaring type and its type relations; `None` when no type declares the subject.
    family: Option<SubjectFamily>,
    /// Whether a candidate's declaring type is related to the subject's, by declaring type.
    family_verdicts: HashMap<CanonicalId, bool>,
}

struct SubjectFamily {
    declaring: CanonicalId,
    /// The types with a direct `type_hierarchy` edge to or from the subject's declaring type.
    direct: HashSet<CanonicalId>,
    /// The in-workspace supertypes of the subject's declaring type.
    supertypes: HashSet<CanonicalId>,
}

impl SubjectRelations {
    /// Read the subject-side facts from `store`.
    pub fn new(store: &GraphStore, subject: &SymbolRow) -> Result<Self, QueryError> {
        let id = &subject.canonical_id;
        let family = match store.declaring_type_of(id)? {
            Some(declaring) => {
                let mut direct: HashSet<CanonicalId> = store
                    .edge_targets(EdgeKind::TypeHierarchy, &declaring)?
                    .into_iter()
                    .collect();
                direct.extend(store.edge_sources(EdgeKind::TypeHierarchy, &declaring)?);
                let supertypes = store.workspace_supertypes_of(&declaring)?.into_iter().collect();
                Some(SubjectFamily {
                    declaring,
                    direct,
                    supertypes,
                })
            }
            None => None,
        };
        Ok(Self {
            users: store.edge_sources(EdgeKind::Uses, id)?.into_iter().collect(),
            used: store.edge_targets(EdgeKind::Uses, id)?.into_iter().collect(),
            family,
            family_verdicts: HashMap::new(),
        })
    }

    /// The labels that hold between `row`'s symbol and the subject, in [`RelationLabel`] order.
    pub fn labels(&mut self, store: &GraphStore, row: &SymbolRow) -> Result<Vec<RelationView>, QueryError> {
        let mut labels = Vec::new();
        let uses_subject = self.users.contains(&row.canonical_id);
        if uses_subject {
            labels.push(RelationView {
                label: RelationLabel::UsesSubject,
                rule: None,
            });
        }
        if self.used.contains(&row.canonical_id) {
            labels.push(RelationView {
                label: RelationLabel::UsedBySubject,
                rule: None,
            });
        }
        if uses_subject && let Some(rule) = &row.test_rule {
            labels.push(RelationView {
                label: RelationLabel::TestsSubject,
                rule: Some(rule.clone()),
            });
        }
        if self.in_type_family(store, &row.canonical_id)? {
            labels.push(RelationView {
                label: RelationLabel::TypeFamily,
                rule: None,
            });
        }
        Ok(labels)
    }

    fn in_type_family(&mut self, store: &GraphStore, id: &CanonicalId) -> Result<bool, QueryError> {
        let Some(family) = &self.family else {
            return Ok(false);
        };
        let Some(declaring) = store.declaring_type_of(id)? else {
            return Ok(false);
        };
        if declaring == family.declaring {
            return Ok(false);
        }
        if let Some(verdict) = self.family_verdicts.get(&declaring) {
            return Ok(*verdict);
        }
        let related = family.direct.contains(&declaring)
            || store
                .workspace_supertypes_of(&declaring)?
                .iter()
                .any(|supertype| family.supertypes.contains(supertype));
        self.family_verdicts.insert(declaring, related);
        Ok(related)
    }
}

/// The `similar` answer without the rows that carry any of `excluded`, the remaining rows in their
/// original order. An answer left with no rows is typed-empty. Applied before paging, so the result
/// limit and continuation token count only the remaining rows.
pub fn exclude_relations(mut answer: Answer<SimilarItem>, excluded: &[RelationLabel]) -> Answer<SimilarItem> {
    if let Outcome::Found { results } = &mut answer.outcome {
        results.retain(|item| {
            item.relations
                .iter()
                .all(|relation| !excluded.contains(&relation.label))
        });
        if results.is_empty() {
            answer.outcome = Outcome::Empty;
        }
    }
    answer
}

/// Build one `search` row: identity, location, and content at the requested detail.
fn search_item(row: &SymbolRow, detail: Detail, max_lines: Option<usize>) -> SearchItem {
    let (content, content_truncated) = projected_row_content(row, detail, max_lines);
    SearchItem {
        symbol: symbol_view(row),
        location: location_of(row),
        content,
        content_truncated,
    }
}

/// The content a `search`/`similar` row carries at `detail`: none at location detail (the
/// location is already on every row), the row's persisted text at that detail otherwise, capped at
/// `max_lines` through the shared cap point.
fn projected_row_content(row: &SymbolRow, detail: Detail, max_lines: Option<usize>) -> (Option<String>, bool) {
    match detail {
        Detail::Location => (None, false),
        other => projected_content(row, Some(other), max_lines),
    }
}

/// The lexical-signal match expression for a `similar` subject: the distinct words of its render,
/// quoted and OR-joined, so candidates rank by BM25 vocabulary overlap with the subject. `None`
/// when the render spells no words.
pub fn subject_word_expr(render: &str) -> Option<String> {
    let mut words = corpus::split_words(render);
    words.sort();
    words.dedup();
    if words.is_empty() {
        None
    } else {
        Some(fts_match_expr(&words))
    }
}

/// The FTS5 match expression for a natural-language query's words: each word quoted (so no word is
/// read as FTS syntax) and joined with OR — any-word matching, ranked by BM25.
pub fn fts_match_expr(words: &[String]) -> String {
    words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Fuse per-signal rank lists with reciprocal rank fusion: `score(d) = Σ 1/(RRF_K + rank_d)` over the
/// rank lists that returned `d` (ranks are 1-based), ordered by descending score with exact ties broken
/// by canonical identity. A document appears at most once however many signals returned it.
pub fn rrf_fuse(signals: &[Vec<CanonicalId>]) -> Vec<CanonicalId> {
    let mut scores: std::collections::HashMap<&CanonicalId, f64> = std::collections::HashMap::new();
    for signal in signals {
        for (index, id) in signal.iter().enumerate() {
            *scores.entry(id).or_insert(0.0) += 1.0 / (RRF_K + (index + 1) as f64);
        }
    }
    let mut fused: Vec<(&CanonicalId, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    fused.into_iter().map(|(id, _)| id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str) -> CanonicalId {
        CanonicalId::from_raw(raw)
    }

    // A document ranked well by both signals fuses ahead of one ranked well by a single signal, each
    // document appears once, and exact score ties break by canonical identity — deterministically.
    #[test]
    fn rrf_fusion_orders_by_summed_reciprocal_rank_with_identity_tie_break() {
        let vector_ranks = vec![id("ws::a"), id("ws::b"), id("ws::c")];
        let lexical_ranks = vec![id("ws::b"), id("ws::d")];
        let fused = rrf_fuse(&[vector_ranks, lexical_ranks]);
        // b: 1/62 + 1/61 beats a: 1/61; c (1/63) and d (1/62) trail; d beats c on rank.
        assert_eq!(fused[0], id("ws::b"));
        assert_eq!(fused[1], id("ws::a"));
        assert_eq!(fused[2], id("ws::d"));
        assert_eq!(fused[3], id("ws::c"));
        assert_eq!(fused.len(), 4, "each document appears at most once");

        // Symmetric signals put two documents at exactly tied scores: identity breaks the tie.
        let tied = rrf_fuse(&[vec![id("ws::z"), id("ws::y")], vec![id("ws::y"), id("ws::z")]]);
        assert_eq!(tied, vec![id("ws::y"), id("ws::z")]);
    }

    // Query words enter the FTS expression quoted, so none is read as FTS syntax, joined with OR
    // for any-word matching.
    #[test]
    fn fts_expression_quotes_words_and_joins_with_or() {
        let words = vec!["parse".to_string(), "config".to_string()];
        assert_eq!(fts_match_expr(&words), "\"parse\" OR \"config\"");
    }
}
