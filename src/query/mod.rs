//! The query surface over the code graph: reference resolution, symbol retrieval at a chosen
//! detail, relationship tracing, and the calibrated output contract every answer carries.

pub mod output;
pub mod resolve;

use crate::graph::store::{DEPENDENTS_HORIZON, Freshness, GraphStore, OccurrenceRow, PersistedClass, SymbolRow};
use crate::identity::CanonicalId;
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts};

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
    /// The symbols that depend on the subject, directly or transitively (the impact assessment).
    Dependents,
}

/// A query error distinct from a typed-absence answer (which is a successful "none").
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The store could not be read.
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),
    /// A dependency edge named a symbol that has no row in the store — an invariant violation, since
    /// edges carry NOT NULL foreign keys to symbols.
    #[error("store corruption: dependency edge references missing symbol {0}")]
    MissingSymbol(CanonicalId),
    /// `trace` was called with `Relation::Dependents`, which carries a depth bound and horizon
    /// aggregate that do not fit the plain relation payload.
    #[error(
        "the `dependents` relation carries a depth bound and horizon aggregate that `trace` cannot \
         express; call `QueryEngine::dependents` instead"
    )]
    DependentsNotTraceable,
}

/// The query engine over a store, carrying the analyzer provenance, content hash, and declared
/// environment in effect for freshness evaluation.
pub struct QueryEngine<'a> {
    store: &'a GraphStore,
    current_provenance: AnalyzerProvenance,
    current_hash: String,
    current_environment: Option<EnvironmentFacts>,
}

impl<'a> QueryEngine<'a> {
    /// Construct a query engine over `store`, told the analyzer, source hash, and declared
    /// environment currently in effect so every answer can be marked fresh or stale.
    pub fn new(
        store: &'a GraphStore,
        current_provenance: AnalyzerProvenance,
        current_hash: String,
        current_environment: Option<EnvironmentFacts>,
    ) -> Self {
        Self {
            store,
            current_provenance,
            current_hash,
            current_environment,
        }
    }

    /// The provenance recorded for the index, and the freshness of the index in effect.
    fn provenance_and_freshness(&self) -> Result<(Provenance, Freshness), QueryError> {
        let meta = self.store.read_metadata()?;
        let freshness = self
            .store
            .freshness(
                &self.current_hash,
                &self.current_provenance,
                self.current_environment.as_ref(),
            )?
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
        // The dependents relation carries a depth bound and a horizon aggregate that do not fit the
        // plain relation payload; it is answered by `dependents()`, not `trace`. A confident empty
        // answer here would misrepresent a subject that may have many dependents, so this is an error
        // rather than a typed absence.
        if relation == Relation::Dependents {
            return Err(QueryError::DependentsNotTraceable);
        }

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
            Relation::Dependents => unreachable!("returned above"),
        };

        if items.is_empty() {
            Ok(Answer::empty(provenance, freshness))
        } else {
            Ok(Answer::found(items, provenance, freshness))
        }
    }

    /// `dependents`: the impact assessment for the subject `reference` — the symbols that depend on
    /// it, directly or transitively, to `depth`.
    ///
    /// Detailed results run to the requested depth; each carries the dependent symbol, the connecting
    /// edge kind, its hop distance, and its location. Dependents deeper than the bound are reported in
    /// aggregate — counts by edge kind and distance — up to the internal horizon, and the answer
    /// always discloses whether reach ends within the bound, extends beyond it, or is itself cut off
    /// at the horizon. A subject with no dependents is typed absence (`Empty`), not a failure.
    pub fn dependents(&self, reference: &str, depth: u32) -> Result<Answer<DependentsReport>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let subject = match self.resolve(reference)? {
            Resolution::Unique(row) => row,
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                return Ok(Answer::ambiguous(views, provenance, freshness));
            }
            Resolution::None => return Ok(Answer::absent(provenance, freshness)),
        };

        let rows = self.store.dependents(&subject.canonical_id, DEPENDENTS_HORIZON)?;
        if rows.is_empty() {
            return Ok(Answer::empty(provenance, freshness));
        }

        // Split at the depth bound: detail rows up to the bound (already ordered by the store),
        // aggregate counts by (distance, kind) beyond it. `cut_at_horizon` is computed independently
        // of that split: a dependent at the horizon depth means deeper reach may exist unexplored,
        // regardless of whether that row is detailed or aggregated, so it must not depend on
        // `depth < DEPENDENTS_HORIZON` to be observed.
        let mut detail = Vec::new();
        let mut aggregate: std::collections::BTreeMap<(u32, String), u64> = std::collections::BTreeMap::new();
        let mut beyond_exists = false;
        let mut cut_at_horizon = false;
        for r in &rows {
            if r.depth >= DEPENDENTS_HORIZON {
                cut_at_horizon = true;
            }
            if r.depth <= depth {
                let Some(row) = self.store.symbol(&r.id)? else {
                    // NOT NULL foreign keys tie every edge to a symbol row, so a miss here means the
                    // store's invariant was violated, not a legitimate absence.
                    return Err(QueryError::MissingSymbol(r.id.clone()));
                };
                detail.push(DependentItem {
                    symbol: symbol_view(&row),
                    kind: r.kind.clone(),
                    distance: r.depth,
                    location: location_of(&row),
                });
            } else {
                beyond_exists = true;
                *aggregate.entry((r.depth, r.kind.clone())).or_insert(0) += 1;
            }
        }

        // Precedence: a horizon cut is disclosed first — it means the walk itself stopped early, so
        // "ends within bound" or "beyond bound" would both overstate confidence in the reach shown.
        let disclosure = if cut_at_horizon {
            HorizonDisclosure::CutAtHorizon
        } else if beyond_exists {
            HorizonDisclosure::BeyondBound
        } else {
            HorizonDisclosure::EndsWithinBound
        };

        let beyond_bound = aggregate
            .into_iter()
            .map(|((distance, kind), count)| AggregateCount { kind, distance, count })
            .collect();

        let report = DependentsReport {
            depth_bound: depth,
            horizon: DEPENDENTS_HORIZON,
            disclosure,
            detail,
            beyond_bound,
        };
        Ok(Answer::found(vec![report], provenance, freshness))
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

/// One detailed dependent in an impact answer: the depending symbol, the kind of dependency edge
/// that connected it, its hop distance from the subject, and its definition location.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DependentItem {
    /// The dependent symbol's identity and name.
    pub symbol: SymbolView,
    /// The connecting dependency edge kind (`uses`, `imports`, or `type_hierarchy`).
    pub kind: String,
    /// The hop distance from the subject (the shortest, when several paths exist).
    pub distance: u32,
    /// The dependent's definition location, or `None` for an external symbol with no source here.
    pub location: Option<Location>,
}

/// An aggregate count of dependents beyond the requested depth: how many were reached at a given
/// distance through a given edge kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AggregateCount {
    /// The connecting dependency edge kind.
    pub kind: String,
    /// The hop distance the count is for.
    pub distance: u32,
    /// How many dependents were reached at that distance through that kind.
    pub count: u64,
}

/// How far the dependency network extends relative to the requested depth — the honest horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonDisclosure {
    /// No reach extends beyond the detailed results: the impact is fully shown.
    EndsWithinBound,
    /// Reach extends past the requested depth; the deeper dependents are fully aggregated within the
    /// internal horizon.
    BeyondBound,
    /// The aggregate itself is bounded: the walk reached the internal horizon, so dependents deeper
    /// than the horizon exist unseen — the reported reach is a floor, not the total.
    CutAtHorizon,
}

/// A depth-bounded impact answer: detailed dependents up to the requested depth, aggregate counts of
/// the deeper reach, and the horizon disclosure that keeps "the query stopped here" distinct from
/// "the impact ends here."
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DependentsReport {
    /// The requested depth bound the detailed results run to.
    pub depth_bound: u32,
    /// The internal horizon the aggregate itself is bounded by.
    pub horizon: u32,
    /// How the reach relates to the bound and the horizon.
    pub disclosure: HorizonDisclosure,
    /// The detailed dependents at distance ≤ the bound, ordered by (distance, kind, identity).
    pub detail: Vec<DependentItem>,
    /// Aggregate counts of dependents beyond the bound, by (distance, kind).
    pub beyond_bound: Vec<AggregateCount>,
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
/// The signature is the span text truncated at the first `{` (the body opener), or the first `;` if
/// there is no `{` — the declaration without the body. A symbol whose declaration is not distinct from
/// its body (e.g. a unit struct) falls back to its full span rather than a contract change (the
/// open question in `proposal.md`, settled here as a presentation fall-back).
fn signature_of(row: &SymbolRow) -> Option<String> {
    let text = row.span_text.as_ref()?;
    let cut = text.find('{').or_else(|| text.find(';')).unwrap_or(text.len());
    Some(text[..cut].trim_end().to_string())
}
