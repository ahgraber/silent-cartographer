//! `clones`: deterministic token-equivalence copies, never behavioral equivalence.
//!
//! With no subject the command lists every group of symbols sharing a substitution-insensitive key;
//! with a subject it lists the other symbols sharing the subject's key. Every row derives from the
//! persisted equivalence keys alone, so the answers carry no estimation marker and no score.

use serde::Serialize;

use crate::graph::clone::guaranteed_fragment_length;
use crate::graph::store::SymbolRow;
use crate::identity::CanonicalId;

use super::output::{Answer, Location, SymbolView};
use super::resolve::{Resolution, SourcePosition, resolve_position};
use super::search::CloneCertainty;
use super::{QueryEngine, QueryError, location_of, symbol_view};

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
///
/// A `fragment` row will add an optional `fragment` field carrying the run's location in the
/// subject, its location in the other symbol, and its token length; `whole_symbol` rows omit it, so
/// they serialize unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneItem {
    /// What the row reports.
    pub kind: CloneKind,
    /// `exact` when the other symbol shares the subject's formatting-insensitive key, else `variant`.
    pub certainty: CloneCertainty,
    /// The other symbol's identity and name.
    pub symbol: SymbolView,
    /// The other symbol's definition location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
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
        let length = self.guaranteed_fragment_length()?;
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
        let length = self.guaranteed_fragment_length()?;
        let Some((subject_formatting, substitution)) = self.store.clone_keys_of(&subject.canonical_id)? else {
            return Ok(Answer::not_comparable(provenance, freshness).with_token_equivalence(length));
        };
        let items = self.whole_symbol_items(&subject, &subject_formatting, &substitution)?;
        if items.is_empty() {
            return Ok(Answer::empty(provenance, freshness).with_token_equivalence(length));
        }
        Ok(Answer::found(items, provenance, freshness).with_token_equivalence(length))
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
                })
            })
            .collect()
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

    /// The guaranteed fragment length under the winnowing parameters the store records, never the
    /// binary's constants: a store built under other parameters answers under its own.
    fn guaranteed_fragment_length(&self) -> Result<usize, QueryError> {
        let (k, w) = self
            .store
            .fragment_parameters()?
            .ok_or(QueryError::MissingFragmentParameters)?;
        Ok(guaranteed_fragment_length(k, w))
    }
}
