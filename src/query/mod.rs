//! The query surface over the code graph: reference resolution, symbol retrieval at a chosen
//! detail, relationship tracing, and the calibrated output contract every answer carries.

pub mod diff;
pub mod impact;
pub mod output;
pub mod page;
pub mod resolve;
pub mod search;

use crate::graph::rank;
use crate::graph::store::{
    DEPENDENTS_HORIZON, DependencyKind, DependentRow, EdgeKind, Freshness, GraphStore, OccurrenceRow, PersistedClass,
    SymbolRow,
};
use crate::identity::CanonicalId;
use crate::semantic::model::{AnalyzerProvenance, EnvironmentFacts};

use output::{Answer, ContentLines, Location, Provenance, SymbolView};
use resolve::{Resolution, resolve};

/// The detail level at which `get` retrieves a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The symbol's definition file and position.
    Location,
    /// The symbol's signature, without its body.
    Signature,
    /// The signature together with the symbol's own documentation.
    Interface,
    /// The symbol's full source body.
    Body,
}

/// A relation `trace` walks from a subject symbol: the relations whose answer is a flat list of
/// items.
///
/// This is [`Relation`] less `Dependents`. The dependents relation carries a depth bound and a
/// horizon aggregate that a flat `Vec<TraceItem>` has nowhere to put, so it is answered by
/// [`QueryEngine::dependents`]. Keeping it out of this type is what makes `trace(dependents)`
/// impossible to write, rather than possible to write and refused at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceRelation {
    /// The declaration that directly encloses the subject (upward).
    Containers,
    /// The symbols the subject directly contains (downward).
    Contains,
    /// The sites that reference the subject (type-occurrences for a type subject).
    References,
    /// The modules that import the subject.
    Importers,
    /// The types that declare the subject as a supertype — a trait's implementors or a base type's
    /// subtypes.
    Implementers,
    /// The reference sites of the subject whose enclosing declaration is classified test code.
    /// Convention-based classification, not resolved semantic fact; every answer carries the
    /// heuristic-grade marker.
    Tests,
}

/// A relation the command surface accepts, the full set the spec enumerates.
///
/// `dependents` is one of them — `.specs/specs/code-navigation/spec.md:81` lists it among `trace`'s
/// relations, and the CLI keeps `--relation dependents` — but it is served by a different engine
/// method. [`Relation::as_trace`] is the one place that split is decided.
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
    /// The modules that import the subject.
    Importers,
    /// The types that declare the subject as a supertype — a trait's implementors or a base type's
    /// subtypes.
    Implementers,
    /// The reference sites of the subject whose enclosing declaration is classified test code — the
    /// answer to "what test code exercises this symbol". Convention-based classification, not
    /// resolved semantic fact; every answer carries the heuristic-grade marker.
    Tests,
}

impl Relation {
    /// The trace relation this is, or `None` for `dependents` — the one relation `trace` cannot
    /// express, which the command surface routes to [`QueryEngine::dependents`] instead.
    pub fn as_trace(self) -> Option<TraceRelation> {
        match self {
            Relation::Containers => Some(TraceRelation::Containers),
            Relation::Contains => Some(TraceRelation::Contains),
            Relation::References => Some(TraceRelation::References),
            Relation::Importers => Some(TraceRelation::Importers),
            Relation::Implementers => Some(TraceRelation::Implementers),
            Relation::Tests => Some(TraceRelation::Tests),
            Relation::Dependents => None,
        }
    }
}

/// How the detailed rows of a `dependents`/`impact` answer are ordered. Distance is always the
/// primary key; the mode decides what breaks ties within a distance layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderMode {
    /// Within each distance layer, order by codebase-wide structural importance (a heuristic),
    /// most important first: `(distance, rank desc, kind order, identity)`.
    Ranked,
    /// Order derived only from the answer's stable structural keys: `(distance, kind order,
    /// identity)` — free of any ranking model.
    Unranked,
}

impl OrderMode {
    /// The closed-vocabulary label this mode carries in query identities and answer disclosures.
    pub fn label(&self) -> &'static str {
        match self {
            OrderMode::Ranked => "ranked",
            OrderMode::Unranked => "unranked",
        }
    }
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
    /// A `search` or `similar` was asked of a store recording no semantic-index identity. Every
    /// estimation-graded answer must name the regime its ranking derives under, so there is no
    /// honest answer to give — an invariant violation, since a completed build always records one.
    #[error("store corruption: no semantic-index identity recorded; rebuild the index")]
    MissingSemanticIndex,
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

    /// `get`: retrieve the symbol denoted by `reference` at `detail`, returning a window of its
    /// content — the lines `[from, from + max_lines)`, 1-based — with a `None` `max_lines` meaning "to
    /// the end of the content."
    ///
    /// An ambiguous reference yields a typed candidate set rather than an arbitrary choice.
    pub fn get(
        &self,
        reference: &str,
        detail: Detail,
        max_lines: Option<usize>,
        from: usize,
    ) -> Result<Answer<SymbolDetail>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match self.resolve(reference)? {
            Resolution::Unique(row) => {
                let payload = self.detail_of(&row, detail, max_lines, from);
                Ok(Answer::found(vec![payload], provenance, freshness))
            }
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                Ok(Answer::ambiguous(views, provenance, freshness))
            }
            Resolution::None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// `get` by source position: retrieve the symbol enclosing `(document, byte_offset)`, returning a
    /// window of its content as [`QueryEngine::get`] does.
    pub fn get_by_position(
        &self,
        document: &str,
        byte_offset: usize,
        detail: Detail,
        max_lines: Option<usize>,
        from: usize,
    ) -> Result<Answer<SymbolDetail>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        match self.store.symbol_enclosing_position(document, byte_offset)? {
            Some(row) => {
                let payload = self.detail_of(&row, detail, max_lines, from);
                Ok(Answer::found(vec![payload], provenance, freshness))
            }
            None => Ok(Answer::absent(provenance, freshness)),
        }
    }

    /// `trace`: return the symbols standing in `relation` to the subject `reference`, each optionally
    /// carrying its content at `detail`.
    ///
    /// `detail` never changes which results are returned or their order — `None` and
    /// `Some(Detail::Location)` both mean "no content field" (the location is already on every row);
    /// any other detail projects content onto each row, as described on [`TraceItem`].
    pub fn trace(
        &self,
        reference: &str,
        relation: TraceRelation,
        detail: Option<Detail>,
        max_lines: Option<usize>,
    ) -> Result<Answer<TraceItem>, QueryError> {
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
            TraceRelation::Contains => self
                .store
                .contains(&subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| {
                    let (content, truncated) = projected_content(&row, detail, max_lines);
                    TraceItem::symbol(symbol_view(&row), location_of(&row), content, truncated)
                })
                .collect(),
            TraceRelation::Containers => self
                .store
                .containers(&subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| {
                    let (content, truncated) = projected_content(&row, detail, max_lines);
                    TraceItem::symbol(symbol_view(&row), location_of(&row), content, truncated)
                })
                .collect(),
            TraceRelation::References => {
                let occs = self.store.references_of(&subject.canonical_id)?;
                let mut items = Vec::with_capacity(occs.len());
                for occ in occs {
                    let (content, truncated) =
                        self.reference_content(&occ.document_path, occ.enclosing_id.as_ref(), detail, max_lines)?;
                    items.push(TraceItem::reference(&subject, occ, None, content, truncated));
                }
                items
            }
            // The `tests` relation is the reverse-reference traversal filtered at the
            // enclosing-declaration grain, preserving the `references` ordering: a site counts
            // exactly when the declaration it is attributed to is classified test code, and each
            // kept site carries that classification's rule as provenance.
            TraceRelation::Tests => {
                let occs = self.store.references_of(&subject.canonical_id)?;
                let mut items = Vec::with_capacity(occs.len());
                for occ in occs {
                    let Some(rule) = self.attributed_test_rule(&occ)? else {
                        continue;
                    };
                    let (content, truncated) =
                        self.reference_content(&occ.document_path, occ.enclosing_id.as_ref(), detail, max_lines)?;
                    items.push(TraceItem::reference(&subject, occ, Some(rule), content, truncated));
                }
                items
            }
            TraceRelation::Importers => self
                .store
                .edge_sources(EdgeKind::Imports, &subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| {
                    let (content, truncated) = projected_content(&row, detail, max_lines);
                    TraceItem::symbol(symbol_view(&row), location_of(&row), content, truncated)
                })
                .collect(),
            TraceRelation::Implementers => self
                .store
                .edge_sources(EdgeKind::TypeHierarchy, &subject.canonical_id)?
                .into_iter()
                .filter_map(|id| self.store.symbol(&id).ok().flatten())
                .map(|row| {
                    let (content, truncated) = projected_content(&row, detail, max_lines);
                    TraceItem::symbol(symbol_view(&row), location_of(&row), content, truncated)
                })
                .collect(),
        };

        let answer = if items.is_empty() {
            Answer::empty(provenance, freshness)
        } else {
            Answer::found(items, provenance, freshness)
        };
        // Every `tests` answer — found and empty alike — carries the structural heuristic-grade
        // marker: an empty answer asserts only that no convention-classified site was found, never
        // that nothing tests the subject.
        if relation == TraceRelation::Tests {
            Ok(answer.convention_classified())
        } else {
            Ok(answer)
        }
    }

    /// `dependents`: the impact assessment for the subject `reference` — the symbols that depend on
    /// it, directly or transitively, to `depth`.
    ///
    /// Detailed results run to the requested depth; each carries the dependent symbol, the connecting
    /// edge kind, its hop distance, its location, and (when `detail` requests it) the dependent's own
    /// content. Dependents deeper than the bound are reported in aggregate — counts by edge kind
    /// and distance, never carrying content — up to the internal horizon, and the answer always
    /// discloses whether reach ends within the bound, extends beyond it, or is itself cut off at the
    /// horizon. A subject with no dependents is typed absence (`Empty`), not a failure.
    pub fn dependents(
        &self,
        reference: &str,
        depth: u32,
        detail: Option<Detail>,
        max_lines: Option<usize>,
        order: OrderMode,
    ) -> Result<Answer<DependentsReport>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let subject = match self.resolve(reference)? {
            Resolution::Unique(row) => row,
            Resolution::Ambiguous(rows) => {
                let views = rows.iter().map(symbol_view).collect();
                return Ok(Answer::ambiguous(views, provenance, freshness));
            }
            Resolution::None => return Ok(Answer::absent(provenance, freshness)),
        };

        let mut rows = self.store.dependents(&subject.canonical_id, DEPENDENTS_HORIZON)?;
        if rows.is_empty() {
            return Ok(Answer::empty(provenance, freshness));
        }
        self.order_dependent_rows(&mut rows, order)?;

        let report = self.dependents_report(&rows, depth, detail, max_lines)?;
        Ok(Answer::found(vec![report], provenance, freshness))
    }

    /// `find`: return every symbol whose name contains `fragment`, matched case-insensitively,
    /// independent of exact reference resolution. A fragment matching no symbol is typed absence.
    pub fn find(&self, fragment: &str) -> Result<Answer<FindItem>, QueryError> {
        let (provenance, freshness) = self.provenance_and_freshness()?;
        let rows = self.store.symbols_by_fragment(fragment)?;
        if rows.is_empty() {
            return Ok(Answer::absent(provenance, freshness));
        }
        let items = rows
            .iter()
            .map(|row| FindItem {
                symbol: symbol_view(row),
                location: location_of(row),
            })
            .collect();
        Ok(Answer::found(items, provenance, freshness))
    }

    /// Render a symbol at a detail level, returning the content windowed to the lines
    /// `[from, from + max_lines)` (1-based). Every content-bearing detail passes through the single
    /// [`window_content`] cap point; a `location` detail carries no content, so the window is
    /// irrelevant to it.
    fn detail_of(&self, row: &SymbolRow, detail: Detail, max_lines: Option<usize>, from: usize) -> SymbolDetail {
        let view = symbol_view(row);
        let (payload, content_lines, content_truncated) = match detail {
            Detail::Location => (
                DetailPayload::Location {
                    location: location_of(row),
                },
                None,
                false,
            ),
            Detail::Signature => {
                let (signature, lines, truncated) = window_content(row.signature_text.clone(), from, max_lines);
                (DetailPayload::Signature { signature }, lines, truncated)
            }
            Detail::Interface => {
                let (interface, lines, truncated) = window_content(row.interface_text.clone(), from, max_lines);
                (DetailPayload::Interface { interface }, lines, truncated)
            }
            Detail::Body => {
                let (body, lines, truncated) = window_content(row.span_text.clone(), from, max_lines);
                (DetailPayload::Body { body }, lines, truncated)
            }
        };
        SymbolDetail {
            symbol: view,
            payload,
            content_lines,
            content_truncated,
        }
    }

    /// The `test_rule` of the declaration a reference site is attributed to: its enclosing
    /// declaration (`enclosing_id`), or the document's module when the site attributes to the
    /// module itself — the same fallback the detail projection uses. `None` means the attributed
    /// declaration is not classified test code.
    fn attributed_test_rule(&self, occ: &OccurrenceRow) -> Result<Option<String>, QueryError> {
        let row = match occ.enclosing_id.as_ref() {
            Some(id) => self.store.symbol(id)?,
            None => self.store.module_of_document(&occ.document_path)?,
        };
        Ok(row.and_then(|r| r.test_rule))
    }

    /// The content for a `references` row at `detail`, capped at `max_lines`: the content of the
    /// declaration the reference site is attributed to (`enclosing_id`), or, when the site attributes
    /// to the module/file itself (`enclosing_id` is `None`), the content of that document's module
    /// symbol. The `bool` reports whether the content was truncated by the cap.
    fn reference_content(
        &self,
        document_path: &str,
        enclosing_id: Option<&CanonicalId>,
        detail: Option<Detail>,
        max_lines: Option<usize>,
    ) -> Result<(Option<String>, bool), QueryError> {
        let Some(detail) = detail else { return Ok((None, false)) };
        if detail == Detail::Location {
            return Ok((None, false));
        }
        let row = match enclosing_id {
            Some(id) => self.store.symbol(id)?,
            None => self.store.module_of_document(document_path)?,
        };
        match row {
            Some(row) => Ok(projected_content(&row, Some(detail), max_lines)),
            None => Ok((None, false)),
        }
    }

    /// Apply the order selector to a dependents walk's rows.
    ///
    /// `Unranked` keeps the store's own `(distance, kind order, identity)` order untouched.
    /// `Ranked` recomputes global rank over the projected graph and re-sorts to `(distance, rank
    /// descending, kind order, identity)`: distance stays primary, importance breaks ties within a
    /// layer, and the trailing structural keys keep the sort total under exact score ties. Rank
    /// values compare via IEEE total order, and the rank itself is deterministic (fixed iteration
    /// and summation order), so identical inputs always order identically. Ordering never changes
    /// which rows are present. A row absent from the projection ranks as zero — the projection
    /// spans every in-workspace symbol, and only in-workspace symbols appear as dependents, so the
    /// fallback is a safety net rather than an expected path.
    fn order_dependent_rows(&self, rows: &mut [DependentRow], order: OrderMode) -> Result<(), QueryError> {
        // No rows, nothing to order: an empty walk (an impact over a seedless diff) never pays for
        // a whole-graph rank it cannot use.
        if order == OrderMode::Unranked || rows.is_empty() {
            return Ok(());
        }
        let projection = self.store.rank_projection()?;
        let scores = rank::global_rank(&projection);
        let rank_of: std::collections::HashMap<&str, f64> = projection
            .nodes
            .iter()
            .zip(scores.iter().copied())
            .map(|(id, score)| (id.as_str(), score))
            .collect();
        rows.sort_by(|a, b| {
            let rank_a = rank_of.get(a.id.as_str()).copied().unwrap_or(0.0);
            let rank_b = rank_of.get(b.id.as_str()).copied().unwrap_or(0.0);
            a.depth
                .cmp(&b.depth)
                .then_with(|| rank_b.total_cmp(&rank_a))
                .then_with(|| a.kind.order().cmp(&b.kind.order()))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(())
    }

    /// Project an ordered dependents walk into the report a caller answers with.
    ///
    /// Split at the depth bound: detailed rows up to the bound (in the order `rows` already carries),
    /// aggregate counts by (distance, kind) beyond it. `cut_at_horizon` is computed independently of
    /// that split: a dependent at the horizon depth means deeper reach may exist unexplored,
    /// regardless of whether that row is detailed or aggregated, so it must not depend on
    /// `depth < DEPENDENTS_HORIZON` to be observed.
    ///
    /// `detail` requests the dependent's own content on the detailed rows; passing `None`
    /// projects location-only rows, which is what a multi-seed walk answers with.
    fn dependents_report(
        &self,
        rows: &[DependentRow],
        depth: u32,
        detail: Option<Detail>,
        max_lines: Option<usize>,
    ) -> Result<DependentsReport, QueryError> {
        let mut detailed = Vec::new();
        let mut aggregate: std::collections::BTreeMap<(u32, DependencyKind), u64> = std::collections::BTreeMap::new();
        let mut beyond_exists = false;
        let mut cut_at_horizon = false;
        for r in rows {
            if r.depth >= DEPENDENTS_HORIZON {
                cut_at_horizon = true;
            }
            if r.depth <= depth {
                let Some(row) = self.store.symbol(&r.id)? else {
                    // NOT NULL foreign keys tie every edge to a symbol row, so a miss here means the
                    // store's invariant was violated, not a legitimate absence.
                    return Err(QueryError::MissingSymbol(r.id.clone()));
                };
                let (content, content_truncated) = projected_content(&row, detail, max_lines);
                detailed.push(DependentItem {
                    content,
                    content_truncated,
                    symbol: symbol_view(&row),
                    kind: r.kind,
                    distance: r.depth,
                    location: location_of(&row),
                });
            } else {
                beyond_exists = true;
                *aggregate.entry((r.depth, r.kind)).or_insert(0) += 1;
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

        Ok(DependentsReport {
            depth_bound: depth,
            horizon: DEPENDENTS_HORIZON,
            disclosure,
            detail: detailed,
            beyond_bound,
        })
    }
}

/// A symbol retrieved at a detail level.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SymbolDetail {
    /// The symbol's identity and name.
    pub symbol: SymbolView,
    /// The detail payload.
    pub payload: DetailPayload,
    /// The line window the content covers, disclosed only when the content is windowed or truncated
    /// (i.e. it is not the whole text at that detail); absent when the whole content is returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_lines: Option<ContentLines>,
    /// Whether the payload's content is a proper subset of the whole text at that detail — the window did not
    /// cover it end to end (`--from` past line 1, or `--max-lines` cut the tail).
    #[serde(skip_serializing_if = "is_false")]
    pub content_truncated: bool,
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
    /// The signature together with the symbol's own documentation.
    Interface {
        /// The interface text, or `None` when the symbol has no persisted span.
        interface: Option<String>,
    },
    /// The full source body.
    Body {
        /// The exact span text, or `None` when the symbol has no persisted span.
        body: Option<String>,
    },
}

/// An item returned by `trace`: either a related symbol or a reference occurrence, each optionally
/// carrying content projected at the requested detail (`None` when no detail was requested, or
/// when the row carries no content at that detail).
///
/// A `Symbol` row projects its own content; a `Reference` row projects the content of the declaration
/// its site is attributed to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum TraceItem {
    /// A symbol standing in the relation (containers / contains / importers / implementers).
    Symbol {
        /// The related symbol's identity and name.
        #[serde(flatten)]
        symbol: SymbolView,
        /// The symbol's own definition location, absent for an external symbol with no source in
        /// this workspace.
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<Location>,
        /// The symbol's own content at the requested detail.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Whether that content was truncated by the `--max-lines` bound.
        #[serde(skip_serializing_if = "is_false")]
        content_truncated: bool,
    },
    /// A reference site of the subject (references).
    Reference {
        /// The referenced subject.
        subject: SymbolView,
        /// The location of the reference.
        location: Location,
        /// The enclosing declaration the reference is attributed to, if any.
        enclosing: Option<CanonicalId>,
        /// The convention rule that classified the attributed declaration as test code — per-site
        /// provenance on a `tests` answer. Absent on a `references` answer, so its shape is
        /// unchanged.
        #[serde(skip_serializing_if = "Option::is_none")]
        test_rule: Option<String>,
        /// The content of the declaration the site is attributed to, at the requested detail.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Whether that content was truncated by the `--max-lines` bound.
        #[serde(skip_serializing_if = "is_false")]
        content_truncated: bool,
    },
}

impl TraceItem {
    fn symbol(view: SymbolView, location: Option<Location>, content: Option<String>, content_truncated: bool) -> Self {
        TraceItem::Symbol {
            symbol: view,
            location,
            content,
            content_truncated,
        }
    }

    fn reference(
        subject: &SymbolRow,
        occ: OccurrenceRow,
        test_rule: Option<String>,
        content: Option<String>,
        content_truncated: bool,
    ) -> Self {
        TraceItem::Reference {
            subject: symbol_view(subject),
            location: Location {
                document_path: occ.document_path,
                span_start: occ.span.0,
                span_end: occ.span.1,
            },
            enclosing: occ.enclosing_id,
            test_rule,
            content,
            content_truncated,
        }
    }
}

/// One symbol returned by `find`: its identity and name, plus its definition location, when it has
/// one (`None` for an external symbol with no source in this workspace).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FindItem {
    /// The matched symbol's identity and name.
    #[serde(flatten)]
    pub symbol: SymbolView,
    /// The symbol's definition location, or `None` for an external symbol.
    pub location: Option<Location>,
}

/// One detailed dependent in an impact answer: the depending symbol, the kind of dependency edge
/// that connected it, its hop distance from the subject, its definition location, and (when
/// requested) its own content at the requested detail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DependentItem {
    /// The dependent symbol's identity and name.
    pub symbol: SymbolView,
    /// The connecting dependency edge kind (`uses`, `imports`, or `type_hierarchy`).
    pub kind: DependencyKind,
    /// The hop distance from the subject (the shortest, when several paths exist).
    pub distance: u32,
    /// The dependent's definition location, or `None` for an external symbol with no source here.
    pub location: Option<Location>,
    /// The dependent's own content at the requested detail, absent when no detail was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Whether that content was truncated by the `--max-lines` bound.
    #[serde(skip_serializing_if = "is_false")]
    pub content_truncated: bool,
}

/// An aggregate count of dependents beyond the requested depth: how many were reached at a given
/// distance through a given edge kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AggregateCount {
    /// The connecting dependency edge kind.
    pub kind: DependencyKind,
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

/// The content a `trace`/`dependents` row projects at `detail`, capped to its first `max_lines`
/// lines: `None` when no detail was requested or the requested detail is `Location` (the location is
/// already on every row), otherwise the row's persisted text at that detail — itself `None` for a
/// detail the row carries no content for (e.g. an external symbol). The `bool` reports whether the cap
/// truncated the content. Trace rows are never windowed (they start at line 1); every content-bearing
/// path routes through the single [`cap_lines`] cap point.
fn projected_content(row: &SymbolRow, detail: Option<Detail>, max_lines: Option<usize>) -> (Option<String>, bool) {
    let raw = match detail {
        None | Some(Detail::Location) => None,
        Some(Detail::Signature) => row.signature_text.clone(),
        Some(Detail::Interface) => row.interface_text.clone(),
        Some(Detail::Body) => row.span_text.clone(),
    };
    cap_lines(raw, max_lines)
}

/// Cap `text` to its first `max_lines` lines, reporting whether the tail was dropped. Lines are split
/// on `\n`, which never falls inside a multibyte UTF-8 code point (a `\n` byte cannot appear as a
/// continuation byte), so the cut is always on a line boundary and never splits a character. A `None`
/// bound, or content already within the bound, passes through untruncated. This is the row-content
/// cap point for `trace`/`dependents`; `get` windows content through [`window_content`].
fn cap_lines(text: Option<String>, max_lines: Option<usize>) -> (Option<String>, bool) {
    match (text, max_lines) {
        (Some(text), Some(max)) => {
            let total = text.split('\n').count();
            if max == 0 || total <= max {
                (Some(text), false)
            } else {
                let kept = text.split('\n').take(max).collect::<Vec<_>>().join("\n");
                (Some(kept), true)
            }
        }
        (text, _) => (text, false),
    }
}

/// The single content-window point for `get`: return the lines `[from, from + max_lines)` of `text`
/// (1-based `from`), with a `None` `max_lines` meaning "to the end." Alongside the windowed text it
/// returns the [`ContentLines`] disclosure (present only when the window is a proper subset of the
/// whole text) and whether the content was truncated (the window did not cover the whole text).
///
/// Lines split on `\n` and rejoin with `\n`, so taking the whole text reconstructs it byte-for-byte.
/// A `from` past the end of the content returns empty content with a disclosure naming the total line
/// count — a data-dependent outcome, not an error.
fn window_content(
    text: Option<String>,
    from: usize,
    max_lines: Option<usize>,
) -> (Option<String>, Option<ContentLines>, bool) {
    let Some(text) = text else {
        return (None, None, false);
    };
    let lines: Vec<&str> = text.split('\n').collect();
    let total = lines.len();
    let start = from;

    // Requested past the end: no lines to return, but the total is disclosed so a caller can recover.
    if start > total {
        let disclosure = ContentLines {
            start,
            end: start - 1,
            total,
        };
        return (Some(String::new()), Some(disclosure), true);
    }

    // `max_lines` of `None` (unbounded) or `0` both mean "to the end of the content."
    let count = match max_lines {
        None | Some(0) => total - (start - 1),
        Some(n) => n,
    };
    // Saturating: a caller-supplied `--max-lines` near `usize::MAX` must not overflow `start - 1 +
    // count`; the window simply reaches the end of the content.
    let end = (start - 1).saturating_add(count).min(total);
    let windowed = lines[(start - 1)..end].join("\n");
    let covers_whole = start == 1 && end == total;
    let disclosure = (!covers_whole).then_some(ContentLines { start, end, total });
    (Some(windowed), disclosure, !covers_whole)
}

/// Whether a truncation flag is unset — the `skip_serializing_if` predicate that keeps an untruncated
/// result's shape unchanged.
fn is_false(flag: &bool) -> bool {
    !*flag
}

#[cfg(test)]
mod tests {
    use super::{cap_lines, window_content};
    use crate::query::output::ContentLines;

    // _(Content bounded with truncation disclosed — line cap)_ — a row-content cap keeps the first
    // `max_lines` lines and discloses the drop; the whole text passes through untouched otherwise.
    #[test]
    fn cap_lines_keeps_the_first_n_lines() {
        let text = || Some("one\ntwo\nthree\nfour".to_string());
        assert_eq!(cap_lines(text(), Some(2)), (Some("one\ntwo".to_string()), true));
        // A bound at or beyond the line count passes the text through untruncated.
        assert_eq!(cap_lines(text(), Some(4)), (text(), false));
        assert_eq!(cap_lines(text(), Some(9)), (text(), false));
        // No bound, or no content, passes through with no disclosure.
        assert_eq!(cap_lines(text(), None), (text(), false));
        assert_eq!(cap_lines(None, Some(2)), (None, false));
    }

    // _(Content bounded with truncation disclosed — UTF-8 safety)_ — splitting on `\n` never lands
    // inside a multibyte code point, so lines carrying multibyte characters survive the cap intact.
    #[test]
    fn cap_lines_preserves_multibyte_lines() {
        // Each line holds multibyte characters (é, 😀, 汉); a byte-based cut could split one, a
        // line-based cut cannot.
        let text = || Some("café\n😀 emoji\n汉字".to_string());
        assert_eq!(
            cap_lines(text(), Some(2)),
            (Some("café\n😀 emoji".to_string()), true),
            "the kept lines are byte-for-byte intact"
        );
        assert_eq!(cap_lines(text(), Some(3)), (text(), false));
    }

    // _(Windowed content access on `get`)_ — the window is the lines `[from, from + max_lines)`, the
    // disclosure carries its position and the total, and the whole text carries neither.
    #[test]
    fn window_content_returns_the_requested_slice_and_discloses_it() {
        let text = || Some("l1\nl2\nl3\nl4\nl5".to_string());

        // A middle window: three lines from line 2, truncated on both ends, position disclosed.
        assert_eq!(
            window_content(text(), 2, Some(3)),
            (
                Some("l2\nl3\nl4".to_string()),
                Some(ContentLines {
                    start: 2,
                    end: 4,
                    total: 5,
                }),
                true,
            )
        );

        // The whole text (from line 1, unbounded): no disclosure, not truncated, byte-for-byte.
        assert_eq!(window_content(text(), 1, None), (text(), None, false));
        // From line 1 with a bound covering everything is also the whole text.
        assert_eq!(window_content(text(), 1, Some(5)), (text(), None, false));

        // A window reaching the end but starting past line 1 is truncated (it omits the head).
        assert_eq!(
            window_content(text(), 4, None),
            (
                Some("l4\nl5".to_string()),
                Some(ContentLines {
                    start: 4,
                    end: 5,
                    total: 5,
                }),
                true,
            )
        );

        // Past the end: empty content, disclosure names the total line count, truncated.
        assert_eq!(
            window_content(text(), 9, Some(3)),
            (
                Some(String::new()),
                Some(ContentLines {
                    start: 9,
                    end: 8,
                    total: 5,
                }),
                true,
            )
        );

        // No content passes through with no disclosure.
        assert_eq!(window_content(None, 1, Some(3)), (None, None, false));
    }
}
