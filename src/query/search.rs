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

use crate::graph::store::SymbolRow;
use crate::graph::{corpus, embed};
use crate::identity::CanonicalId;

use super::output::{Answer, Location, SemanticIndexView, SymbolView};
use super::resolve::Resolution;
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

    /// `similar` by source position: the subject is the symbol enclosing `(document, byte_offset)`,
    /// resolved exactly as `get --at` resolves it.
    pub fn similar_by_position(
        &self,
        document: &str,
        byte_offset: usize,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SimilarItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match self.store.symbol_enclosing_position(document, byte_offset)? {
            Some(subject) => self.similar_of(&subject, detail, max_lines),
            // No enclosing symbol resolves: no ranking was derived, so no marker or provenance.
            None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// The `similar` ranking for a resolved subject: the two-signal RRF hybrid — the subject's vector
    /// neighbors fused with BM25 of the subject's own render words — with the subject excluded,
    /// clone-marked rows lifted above unmarked ones (exact before variant), fused estimated order
    /// within each certainty class.
    fn similar_of(
        &self,
        subject: &SymbolRow,
        detail: Detail,
        max_lines: Option<usize>,
    ) -> Result<Answer<SimilarItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let semantic_index = self.semantic_index_view()?;
        // A subject outside the corpus (an external, or a symbol with no persisted content) has no
        // representation to compare — a definite empty, not a failure.
        let subject_vectors = self.store.chunk_vectors_of(&subject.canonical_id)?;
        if subject_vectors.is_empty() {
            return Ok(Answer::empty(provenance, freshness).estimated(semantic_index));
        }
        let corpus_size = self.store.corpus_size()? as usize;
        // Similarity is the best pair over the subject's chunks and each candidate's: each subject
        // chunk's neighbor list arrives deduplicated to candidates by best chunk, and merging by
        // minimum distance across subject chunks completes the maximum over pairs. Neither side
        // gains from the number of chunks representing it.
        let chunk_count = self.store.chunk_count()? as usize;
        let mut best: std::collections::HashMap<CanonicalId, f64> = std::collections::HashMap::new();
        for subject_vector in &subject_vectors {
            for (id, distance) in self.store.vector_neighbors(subject_vector, chunk_count)? {
                let entry = best.entry(id).or_insert(distance);
                if distance < *entry {
                    *entry = distance;
                }
            }
        }
        let mut nearest: Vec<(CanonicalId, f64)> = best.into_iter().collect();
        nearest.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
        let vector_ranks: Vec<CanonicalId> = nearest.into_iter().map(|(id, _)| id).collect();
        let lexical_ranks: Vec<CanonicalId> = match self
            .store
            .semantic_render_of(&subject.canonical_id)?
            .as_deref()
            .and_then(subject_word_expr)
        {
            Some(expr) => self
                .store
                .lexical_neighbors(&expr, corpus_size)?
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
            None => Vec::new(),
        };
        let fused = rrf_fuse(&[vector_ranks, lexical_ranks]);
        let subject_keys = self.store.clone_keys_of(&subject.canonical_id)?;

        let mut items = Vec::with_capacity(fused.len().saturating_sub(1));
        for id in fused {
            if id == subject.canonical_id {
                continue;
            }
            let Some(row) = self.store.symbol(&id)? else {
                return Err(QueryError::MissingSymbol(id));
            };
            let clone_certainty = match &subject_keys {
                None => None,
                Some((subject_formatting, subject_substitution)) => {
                    match self.store.clone_keys_of(&row.canonical_id)? {
                        Some((formatting, _)) if formatting == *subject_formatting => Some(CloneCertainty::ExactClone),
                        Some((_, substitution)) if substitution == *subject_substitution => {
                            Some(CloneCertainty::VariantClone)
                        }
                        _ => None,
                    }
                }
            };
            let (content, content_truncated) = projected_row_content(&row, detail, max_lines);
            items.push(SimilarItem {
                symbol: symbol_view(&row),
                clone_certainty,
                location: location_of(&row),
                content,
                content_truncated,
            });
        }
        if items.is_empty() {
            return Ok(Answer::empty(provenance, freshness).estimated(semantic_index));
        }
        // Certainty classes order ahead of the estimate: exact clones, then variant clones, then
        // unmarked rows; the stable sort preserves the estimated-similarity order within each class.
        items.sort_by_key(|item| match item.clone_certainty {
            Some(CloneCertainty::ExactClone) => 0,
            Some(CloneCertainty::VariantClone) => 1,
            None => 2,
        });
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
