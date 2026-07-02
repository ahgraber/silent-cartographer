//! The query surface over the code graph: reference resolution, symbol retrieval at a chosen
//! detail, relationship tracing, and the calibrated output contract every answer carries.

pub mod output;
pub mod resolve;

use crate::graph::store::{Freshness, GraphStore, OccurrenceRow, PersistedClass, SymbolRow};
use crate::identity::CanonicalId;
use crate::semantic::model::AnalyzerProvenance;

use output::{Answer, Location, Provenance, SymbolView};
use resolve::{Resolution, resolve};

/// The detail level at which `get` retrieves a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The symbol's definition file and position.
    Location,
    /// The symbol's signature, without its body.
    Signature,
    /// The symbol's full source body.
    Body,
}

/// A relation `trace` walks from a subject symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The declaration that directly encloses the subject (upward).
    Containers,
    /// The symbols the subject directly contains (downward).
    Contains,
    /// The sites that reference the subject (type-occurrences for a type subject).
    References,
}

/// A query error distinct from a typed-absence answer (which is a successful "none").
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The store could not be read.
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),
}

/// The query engine over a store, carrying the analyzer provenance in effect and a content hash for
/// freshness evaluation.
pub struct QueryEngine<'a> {
    store: &'a GraphStore,
    current_provenance: AnalyzerProvenance,
    current_hash: String,
}

impl<'a> QueryEngine<'a> {
    /// Construct a query engine over `store`, told the analyzer and source hash currently in effect
    /// so every answer can be marked fresh or stale.
    pub fn new(store: &'a GraphStore, current_provenance: AnalyzerProvenance, current_hash: String) -> Self {
        Self {
            store,
            current_provenance,
            current_hash,
        }
    }

    /// The provenance recorded for the index, and the freshness of the index in effect.
    fn provenance_and_freshness(&self) -> Result<(Provenance, Freshness), QueryError> {
        let meta = self.store.read_metadata()?;
        let freshness = self
            .store
            .freshness(&self.current_hash, &self.current_provenance)?
            .unwrap_or(Freshness::StaleContent);
        let provenance = match meta {
            Some(m) => Provenance {
                analyzer_name: m.provenance.analyzer_name,
                analyzer_version: m.provenance.analyzer_version,
            },
            None => Provenance {
                analyzer_name: self.current_provenance.analyzer_name.clone(),
                analyzer_version: self.current_provenance.analyzer_version.clone(),
            },
        };
        Ok((provenance, freshness))
    }

    /// Resolve a reference string to the symbol(s) it denotes.
    pub fn resolve(&self, reference: &str) -> Result<Resolution, QueryError> {
        Ok(resolve(self.store, reference)?)
    }

    /// `get`: retrieve the symbol denoted by `reference` at `detail`.
    ///
    /// An ambiguous reference yields a typed candidate set rather than an arbitrary choice.
    pub fn get(&self, reference: &str, detail: Detail) -> Result<Answer<SymbolDetail>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match self.resolve(reference)? {
            Resolution::Unique(row) => {
                let payload = self.detail_of(&row, detail);
                Ok(Answer::found(vec![payload], provenance, freshness))
            }
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                Ok(Answer::ambiguous(views, provenance, freshness))
            }
            Resolution::None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// `get` by source position: retrieve the symbol enclosing `(document, byte_offset)`.
    pub fn get_by_position(
        &self,
        document: &str,
        byte_offset: usize,
        detail: Detail,
    ) -> Result<Answer<SymbolDetail>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match self.store.symbol_enclosing_position(document, byte_offset)? {
            Some(row) => {
                let payload = self.detail_of(&row, detail);
                Ok(Answer::found(vec![payload], provenance, freshness))
            }
            None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// `trace`: return the symbols standing in `relation` to the subject `reference`.
    pub fn trace(&self, reference: &str, relation: Relation) -> Result<Answer<TraceItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let subject = match self.resolve(reference)? {
            Resolution::Unique(row) => row,
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                return Ok(Answer::ambiguous(views, provenance, freshness));
            }
            Resolution::None => return Ok(Answer::absent(provenance, freshness)),
        };

        let items: Vec<TraceItem> = match relation {
            Relation::Contains => self
                .store
                .contains(&subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| TraceItem::symbol(symbol_view(&row)))
                .collect(),
            Relation::Containers => self
                .store
                .containers(&subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| TraceItem::symbol(symbol_view(&row)))
                .collect(),
            Relation::References => self
                .store
                .references_of(&subject.canonical_id)?
                .into_iter()
                .map(|occ| TraceItem::reference(&subject, occ))
                .collect(),
        };

        if items.is_empty() {
            Ok(Answer::empty(provenance, freshness))
        } else {
            Ok(Answer::found(items, provenance, freshness))
        }
    }

    /// Render a symbol at a detail level.
    fn detail_of(&self, row: &SymbolRow, detail: Detail) -> SymbolDetail {
        let view = symbol_view(row);
        let payload = match detail {
            Detail::Location => DetailPayload::Location {
                location: location_of(row),
            },
            Detail::Body => DetailPayload::Body {
                body: row.span_text.clone(),
            },
            Detail::Signature => DetailPayload::Signature {
                signature: signature_of(row),
            },
        };
        SymbolDetail { symbol: view, payload }
    }
}

/// A symbol retrieved at a detail level.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SymbolDetail {
    /// The symbol's identity and name.
    pub symbol: SymbolView,
    /// The detail payload.
    pub payload: DetailPayload,
}

/// The payload of a `get` at a chosen detail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "detail", rename_all = "snake_case")]
pub enum DetailPayload {
    /// The definition location.
    Location {
        /// The definition location, or `None` for an external symbol with no source here.
        location: Option<Location>,
    },
    /// The signature (declaration without the body).
    Signature {
        /// The signature text, or `None` when the symbol has no persisted span.
        signature: Option<String>,
    },
    /// The full source body.
    Body {
        /// The exact span text, or `None` when the symbol has no persisted span.
        body: Option<String>,
    },
}

/// An item returned by `trace`: either a related symbol or a reference occurrence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum TraceItem {
    /// A symbol standing in the relation (containers / contains).
    Symbol(SymbolView),
    /// A reference site of the subject (references).
    Reference {
        /// The referenced subject.
        subject: SymbolView,
        /// The location of the reference.
        location: Location,
        /// The enclosing declaration the reference is attributed to, if any.
        enclosing: Option<CanonicalId>,
    },
}

impl TraceItem {
    fn symbol(view: SymbolView) -> Self {
        TraceItem::Symbol(view)
    }

    fn reference(subject: &SymbolRow, occ: OccurrenceRow) -> Self {
        TraceItem::Reference {
            subject: symbol_view(subject),
            location: Location {
                document_path: occ.document_path,
                span_start: occ.span.0,
                span_end: occ.span.1,
            },
            enclosing: occ.enclosing_id,
        }
    }
}

/// Build the identity+name view of a symbol.
fn symbol_view(row: &SymbolRow) -> SymbolView {
    SymbolView {
        canonical_id: row.canonical_id.clone(),
        name: row.display_name.clone(),
        kind: row.kind.clone(),
        external: row.class == PersistedClass::External,
    }
}

/// The definition location of a symbol, if it has a definition span.
fn location_of(row: &SymbolRow) -> Option<Location> {
    match (&row.document_path, row.span) {
        (Some(doc), Some((start, end))) => Some(Location {
            document_path: doc.clone(),
            span_start: start,
            span_end: end,
        }),
        _ => None,
    }
}

/// The signature of a symbol: the declaration text up to its body.
///
/// The signature is the span text truncated at the first `{` (the body opener) or `;`, whichever
/// comes first — the declaration without the body. A symbol whose declaration is not distinct from
/// its body (e.g. a unit struct) falls back to its full span rather than a contract change (the
/// open question in `proposal.md`, settled here as a presentation fall-back).
fn signature_of(row: &SymbolRow) -> Option<String> {
    let text = row.span_text.as_ref()?;
    let cut = text.find('{').or_else(|| text.find(';')).unwrap_or(text.len());
    Some(text[..cut].trim_end().to_string())
}
