//! `clones`: deterministic token-equivalence copies, never behavioral equivalence.
//!
//! With no subject the command lists every group of symbols sharing a substitution-insensitive key;
//! with a subject it lists the other symbols sharing the subject's key, then the token runs the
//! subject shares with other symbols. Every row derives from the persisted equivalence keys and
//! fingerprints and the indexed text, so the answers carry no estimation marker and no score.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use serde::Serialize;

use crate::graph::backend_language;
use crate::graph::clone::{SharedRun, comparable_tokens, contained_symbols, guaranteed_fragment_length, shared_runs};
use crate::graph::corpus::row_contributes;
use crate::graph::range::{ByteSpan, PositionIndex};
use crate::graph::store::{GraphStore, SymbolRow};
use crate::graph::syntax::{Language, SpelledToken, SyntaxTree};
use crate::identity::CanonicalId;

use super::output::{Answer, Location, SymbolView};
use super::resolve::{Resolution, SourcePosition, resolve_position};
use super::search::CloneCertainty;
use super::{QueryEngine, QueryError, location_of, symbol_view};

/// The most candidate seeds the comparison of the subject with one other symbol may have: the sum,
/// over the fingerprint hashes the two share, of the product of the two occurrence counts. A pair
/// over the limit is not compared and is named in the answer instead.
pub const FRAGMENT_SEED_LIMIT: usize = 1_000_000;

/// One member of a clone group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneMember {
    /// The member's identity and name.
    pub symbol: SymbolView,
    /// The member's definition location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// The ordinal, from 1, of the member's exact set within its group: equal for members sharing a
    /// formatting-insensitive key, numbered in the order the sets' first members appear.
    pub exact_set: usize,
}

/// A group of two or more symbols sharing a substitution-insensitive key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneGroup {
    /// `exact` when every member shares one formatting-insensitive key, else `variant`.
    pub certainty: CloneCertainty,
    /// The length of the token sequence every member shares.
    pub token_count: usize,
    /// The members, ordered by canonical identity.
    pub members: Vec<CloneMember>,
}

/// What a `clones` subject row reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneKind {
    /// The other symbol as a whole shares the subject's substitution-insensitive key.
    WholeSymbol,
    /// A token run of the subject is shared with a run of the other symbol.
    Fragment,
}

/// One `clones` row for a subject symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneItem {
    /// What the row reports.
    pub kind: CloneKind,
    /// For a whole-symbol row, `exact` when the other symbol shares the subject's
    /// formatting-insensitive key; for a fragment row, `exact` when the two runs are spelled alike.
    /// `variant` otherwise.
    pub certainty: CloneCertainty,
    /// The other symbol's identity and name.
    pub symbol: SymbolView,
    /// The other symbol's definition location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// The shared runs, present exactly on a `fragment` row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment: Option<Fragment>,
}

/// A pair of shared token runs: one in the subject, one in the other symbol, located in the indexed
/// text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fragment {
    /// The length of each run in tokens.
    pub token_length: usize,
    /// The run's location in the subject.
    pub subject_run: Location,
    /// The run's location in the other symbol.
    pub other_run: Location,
}

/// The byte span from the first token's start to the last token's end.
fn run_span(tokens: &[SpelledToken]) -> ByteSpan {
    let last = tokens.last().expect("a shared run holds tokens");
    ByteSpan {
        start: tokens[0].start,
        end: last.start + last.text.len(),
    }
}

/// The indexed text of each document one query reads, parsed once per document, never read from
/// the workspace.
struct IndexedTexts<'s> {
    store: &'s GraphStore,
    language: Language,
    trees: HashMap<String, SyntaxTree>,
}

impl<'s> IndexedTexts<'s> {
    /// Indexed texts parsed under the language of the store's recorded `analyzer`.
    fn new(store: &'s GraphStore, analyzer: &str) -> Result<Self, QueryError> {
        let language = backend_language(analyzer).map_err(|_| QueryError::UnknownAnalyzer(analyzer.to_string()))?;
        Ok(Self {
            store,
            language,
            trees: HashMap::new(),
        })
    }

    /// A symbol's comparable tokens, exactly as the build computed them for its fingerprints: the
    /// same call over the same span of the same text, cutting the corpus-contributing symbols the
    /// store holds within the span under the build's containment rule.
    fn tokens(&mut self, row: &SymbolRow) -> Result<Vec<SpelledToken>, QueryError> {
        let (Some(document), Some((start, end))) = (row.document_path.as_deref(), row.span) else {
            return Err(QueryError::MissingIndexedText(row.canonical_id.to_string()));
        };
        if !self.trees.contains_key(document) {
            let tree = self
                .store
                .module_of_document(document)?
                .and_then(|module| module.span_text)
                .and_then(|text| SyntaxTree::parse(&text, self.language))
                .ok_or_else(|| QueryError::MissingIndexedText(document.to_string()))?;
            self.trees.insert(document.to_string(), tree);
        }
        let within: Vec<SymbolRow> = self
            .store
            .symbols_overlapping_span(document, start, end)?
            .into_iter()
            .filter(row_contributes)
            .collect();
        let candidates = within
            .iter()
            .filter_map(|other| Some((&other.canonical_id, other.span?)));
        let contained = contained_symbols(&row.canonical_id, (start, end), candidates);
        Ok(comparable_tokens(
            &self.trees[document],
            ByteSpan { start, end },
            &contained,
        ))
    }
}

/// The fragment lengths a store's recorded winnowing parameters imply.
struct FragmentLengths {
    data_stretch_length: usize,
    guaranteed_length: usize,
}

/// A group of symbols sharing one substitution-insensitive key, before its members are read.
struct RawGroup {
    token_count: usize,
    /// `(canonical identity, formatting-insensitive key)`, ordered by identity.
    members: Vec<(CanonicalId, String)>,
}

/// Collect `rows` — ordered by substitution key, then identity — into groups ordered by token count
/// descending, member count descending, then the lowest member's identity.
fn order_groups(rows: Vec<(CanonicalId, String, String, usize)>) -> Vec<RawGroup> {
    let mut groups: Vec<(String, RawGroup)> = Vec::new();
    for (id, formatting, substitution, token_count) in rows {
        match groups.last_mut() {
            Some((key, group)) if *key == substitution => group.members.push((id, formatting)),
            _ => groups.push((
                substitution,
                RawGroup {
                    token_count,
                    members: vec![(id, formatting)],
                },
            )),
        }
    }
    let mut groups: Vec<RawGroup> = groups.into_iter().map(|(_, group)| group).collect();
    groups.sort_by(|a, b| {
        b.token_count
            .cmp(&a.token_count)
            .then_with(|| b.members.len().cmp(&a.members.len()))
            .then_with(|| a.members[0].0.cmp(&b.members[0].0))
    });
    groups
}

impl QueryEngine<'_> {
    /// `clones` with no subject: every group of two or more symbols sharing a substitution-insensitive
    /// key, longest token sequences first. A corpus with no shared key is a typed-empty answer.
    pub fn clone_groups(&self) -> Result<Answer<CloneGroup>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let length = self.fragment_lengths()?.guaranteed_length;
        let groups = order_groups(self.store.clone_key_sharers()?);
        if groups.is_empty() {
            return Ok(Answer::empty(provenance, freshness).with_token_equivalence(length));
        }
        let items = groups
            .into_iter()
            .map(|group| self.materialize_group(group))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Answer::found(items, provenance, freshness).with_token_equivalence(length))
    }

    /// `clones` for a subject named by reference. An ambiguous reference yields the typed candidate
    /// set and an unresolved one the typed absence, neither carrying the token-equivalence grade.
    pub fn clones(&self, reference: &str) -> Result<Answer<CloneItem>, QueryError> {
        self.clones_of(self.resolve(reference)?)
    }

    /// `clones` for the subject a source position resolves to, resolved as `get --at` resolves it.
    pub fn clones_by_position(
        &self,
        document: &str,
        position: SourcePosition,
    ) -> Result<Answer<CloneItem>, QueryError> {
        self.clones_of(resolve_position(self.store, document, position)?)
    }

    fn clones_of(&self, resolution: Resolution) -> Result<Answer<CloneItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        // Ambiguity and absence end before the clone evidence is read, so they carry no grade.
        let subject = match resolution {
            Resolution::Unique(row) => row,
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                return Ok(Answer::ambiguous(views, provenance, freshness));
            }
            Resolution::None => return Ok(Answer::absent(provenance, freshness)),
        };
        let FragmentLengths {
            data_stretch_length,
            guaranteed_length: length,
        } = self.fragment_lengths()?;
        if self.store.clone_token_count_of(&subject.canonical_id)?.is_none() {
            return Ok(Answer::not_comparable(provenance, freshness).with_token_equivalence(length));
        }
        // Only a leaf carries keys; a function containing corpus symbols has fragments alone.
        let mut items = match self.store.clone_keys_of(&subject.canonical_id)? {
            Some((subject_formatting, substitution)) => {
                self.whole_symbol_items(&subject, &subject_formatting, &substitution)?
            }
            None => Vec::new(),
        };
        let whole_symbol: HashSet<CanonicalId> = items.iter().map(|item| item.symbol.canonical_id.clone()).collect();
        let (fragments, not_compared) = self.fragment_items(
            &subject,
            &whole_symbol,
            length,
            data_stretch_length,
            &provenance.analyzer_name,
        )?;
        items.extend(fragments);
        let answer = if items.is_empty() {
            Answer::empty(provenance, freshness)
        } else {
            Answer::found(items, provenance, freshness)
        };
        Ok(answer
            .with_token_equivalence(length)
            .with_not_compared_completely(not_compared))
    }

    /// The other symbols sharing `substitution`, exact before variant, then by identity.
    fn whole_symbol_items(
        &self,
        subject: &SymbolRow,
        subject_formatting: &str,
        substitution: &str,
    ) -> Result<Vec<CloneItem>, QueryError> {
        let mut matches: Vec<(CanonicalId, CloneCertainty)> = self
            .store
            .clone_key_matches(substitution)?
            .into_iter()
            .filter(|(id, _)| *id != subject.canonical_id)
            .map(|(id, formatting)| {
                let certainty = if formatting == subject_formatting {
                    CloneCertainty::ExactClone
                } else {
                    CloneCertainty::VariantClone
                };
                (id, certainty)
            })
            .collect();
        matches.sort_by(|a, b| {
            let rank = |certainty: CloneCertainty| certainty == CloneCertainty::VariantClone;
            rank(a.1).cmp(&rank(b.1)).then_with(|| a.0.cmp(&b.0))
        });
        matches
            .into_iter()
            .map(|(id, certainty)| {
                let row = self.store.symbol(&id)?.ok_or(QueryError::MissingSymbol(id))?;
                Ok(CloneItem {
                    kind: CloneKind::WholeSymbol,
                    certainty,
                    symbol: symbol_view(&row),
                    location: location_of(&row),
                    fragment: None,
                })
            })
            .collect()
    }

    /// The token runs of at least `min_length` tokens the subject shares with each other symbol
    /// outside `whole_symbol`. Runs are verified against tokens re-spelled from the indexed text
    /// under the store's recorded `analyzer`. `data_stretch_length` is the length of a data stretch.
    /// Runs are ordered by length descending, then the other symbol's identity, then the subject
    /// run's start, then the other run's start.
    ///
    /// A symbol whose candidate seed count with the subject exceeds [`FRAGMENT_SEED_LIMIT`] is not
    /// compared. Its view is returned apart, in canonical-identity order.
    fn fragment_items(
        &self,
        subject: &SymbolRow,
        whole_symbol: &HashSet<CanonicalId>,
        min_length: usize,
        data_stretch_length: usize,
        analyzer: &str,
    ) -> Result<(Vec<CloneItem>, Vec<SymbolView>), QueryError> {
        let mut subject_ordinals: HashMap<i64, Vec<usize>> = HashMap::new();
        for (hash, ordinal) in self.store.clone_fingerprints_of(&subject.canonical_id)? {
            subject_ordinals.entry(hash).or_default().push(ordinal);
        }
        // A pair's candidate seed count sums, over the hashes they share, the product of the two
        // occurrence counts. It accumulates from the other symbol's positions, so a pair over the
        // limit never has its position cross product built.
        let mut occurrences: HashMap<CanonicalId, Vec<(i64, usize)>> = HashMap::new();
        for hash in subject_ordinals.keys() {
            for (other, other_ordinal) in self.store.symbols_with_fingerprint(*hash)? {
                if other == subject.canonical_id || whole_symbol.contains(&other) {
                    continue;
                }
                occurrences.entry(other).or_default().push((*hash, other_ordinal));
            }
        }
        let mut limited: Vec<CanonicalId> = Vec::new();
        let mut seeds: HashMap<CanonicalId, Vec<(usize, usize)>> = HashMap::new();
        for (other, positions) in occurrences {
            let seed_count: usize = positions.iter().map(|(hash, _)| subject_ordinals[hash].len()).sum();
            if seed_count > FRAGMENT_SEED_LIMIT {
                limited.push(other);
                continue;
            }
            let pairs = seeds.entry(other).or_default();
            for (hash, other_ordinal) in positions {
                pairs.extend(subject_ordinals[&hash].iter().map(|&ordinal| (ordinal, other_ordinal)));
            }
        }
        limited.sort();
        let not_compared = limited
            .into_iter()
            .map(|id| {
                let row = self.store.symbol(&id)?.ok_or(QueryError::MissingSymbol(id))?;
                Ok(symbol_view(&row))
            })
            .collect::<Result<Vec<_>, QueryError>>()?;
        if seeds.is_empty() {
            return Ok((Vec::new(), not_compared));
        }

        let mut texts = IndexedTexts::new(self.store, analyzer)?;
        let subject_tokens = texts.tokens(subject)?;
        let mut found: Vec<(Rc<SymbolRow>, SharedRun, ByteSpan, ByteSpan)> = Vec::new();
        for (other, pairs) in seeds {
            let row = Rc::new(
                self.store
                    .symbol(&other)?
                    .ok_or_else(|| QueryError::MissingSymbol(other.clone()))?,
            );
            let other_tokens = texts.tokens(&row)?;
            for run in shared_runs(&subject_tokens, &other_tokens, &pairs, min_length, data_stretch_length) {
                let subject_span = run_span(&subject_tokens[run.a_start..run.a_start + run.length]);
                let other_span = run_span(&other_tokens[run.b_start..run.b_start + run.length]);
                found.push((Rc::clone(&row), run, subject_span, other_span));
            }
        }
        found.sort_by(|a, b| {
            b.1.length
                .cmp(&a.1.length)
                .then_with(|| a.0.canonical_id.cmp(&b.0.canonical_id))
                .then_with(|| a.2.start.cmp(&b.2.start))
                .then_with(|| a.3.start.cmp(&b.3.start))
        });

        let mut positions: HashMap<String, PositionIndex<'_>> = HashMap::new();
        let mut locate = |document: &str, span: ByteSpan| {
            let index = positions
                .entry(document.to_string())
                .or_insert_with(|| PositionIndex::new(texts.trees[document].source()));
            Location {
                document_path: document.to_string(),
                span_start: span.start,
                span_end: span.end,
                line_column: index.span(span.start, span.end).into(),
            }
        };
        let subject_document = subject
            .document_path
            .as_deref()
            .expect("a re-spelled subject has a document");
        let items = found
            .into_iter()
            .map(|(row, run, subject_span, other_span)| {
                let other_document = row
                    .document_path
                    .as_deref()
                    .expect("a re-spelled symbol has a document");
                CloneItem {
                    kind: CloneKind::Fragment,
                    certainty: if run.exact {
                        CloneCertainty::ExactClone
                    } else {
                        CloneCertainty::VariantClone
                    },
                    fragment: Some(Fragment {
                        token_length: run.length,
                        subject_run: locate(subject_document, subject_span),
                        other_run: locate(other_document, other_span),
                    }),
                    symbol: symbol_view(&row),
                    location: location_of(&row),
                }
            })
            .collect();
        Ok((items, not_compared))
    }

    /// A symbol's comparable tokens as a `clones` query re-spells them from the indexed text, under
    /// the store's recorded analyzer. Their positions are the ordinals the stored fingerprints index.
    pub fn comparable_tokens_of(&self, row: &SymbolRow) -> Result<Vec<SpelledToken>, QueryError> {
        let (provenance, _) = self.provenance_and_freshness()?;
        IndexedTexts::new(self.store, &provenance.analyzer_name)?.tokens(row)
    }

    fn materialize_group(&self, group: RawGroup) -> Result<CloneGroup, QueryError> {
        let mut exact_sets: Vec<&str> = Vec::new();
        let mut members = Vec::with_capacity(group.members.len());
        for (id, formatting) in &group.members {
            let exact_set = match exact_sets.iter().position(|key| key == formatting) {
                Some(index) => index + 1,
                None => {
                    exact_sets.push(formatting);
                    exact_sets.len()
                }
            };
            let row = self
                .store
                .symbol(id)?
                .ok_or_else(|| QueryError::MissingSymbol(id.clone()))?;
            members.push(CloneMember {
                symbol: symbol_view(&row),
                location: location_of(&row),
                exact_set,
            });
        }
        let certainty = if exact_sets.len() == 1 {
            CloneCertainty::ExactClone
        } else {
            CloneCertainty::VariantClone
        };
        Ok(CloneGroup {
            certainty,
            token_count: group.token_count,
            members,
        })
    }

    /// The data-stretch length and the guaranteed fragment length under the winnowing parameters
    /// the store records, never the binary's constants: a store built under other parameters
    /// answers under its own. The data-stretch length is the recorded k-gram length.
    fn fragment_lengths(&self) -> Result<FragmentLengths, QueryError> {
        let (k, w) = self
            .store
            .fragment_parameters()?
            .ok_or(QueryError::MissingFragmentParameters)?;
        Ok(FragmentLengths {
            data_stretch_length: k,
            guaranteed_length: guaranteed_fragment_length(k, w),
        })
    }
}
