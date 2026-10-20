//! The code graph: the guarded join, the SQLite-core store, and the ingest that unifies the two
//! oracles into one persisted graph.

pub mod chunk;
pub mod clone;
pub mod corpus;
pub mod embed;
pub mod join;
pub mod prepared;
pub mod range;
pub mod rank;
pub mod schema;
pub mod store;
pub mod syntax;
pub mod units;

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::identity::{CanonicalId, DefinitionSite, ProjectionInput, WorkspaceId, project_all, project_one};
use crate::semantic::model::{ExtractedIndex, ExtractedSymbol, OccurrenceRole, SourceRange, SymbolClass, SymbolKind};
use crate::semantic::python_adapter::PythonIndexer;
use crate::semantic::rust_adapter::RustAdapter;

use join::{
    AlignedOccurrence, JoinAccounting, JoinOutcome, SourceCorpus, UnalignedOccurrence, join, module_by_document,
};
use prepared::PreparedCorpus;
use range::{ByteSpan, LineColumnSpan, PositionIndex, range_to_span};
use store::{
    DiscrepancyRow, EdgeKind, Freshness, GraphStore, IndexMetadata, OccurrenceRow, PersistedClass, RecordedTargets,
    SymbolRow, UnresolvedCallRow, shorten_quoted_text,
};
use syntax::Language;
use units::{UndeterminedUnits, Unit, assign_units, declared_from, document_units, single_parent_map};

/// An error during ingest.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The store rejected an operation.
    #[error("c10r could not write the index, so the build did not complete: {0}")]
    Store(#[from] rusqlite::Error),
    /// The syntax tree could not be reconciled with the index because of a content-hash mismatch.
    #[error(
        "the sources do not match the content hash the build expected, so the build stopped. \
         Run `c10r build` again."
    )]
    ContentHashMismatch,
    /// The analyzer name names no backend this binary knows, so no syntax language follows from it.
    #[error(
        "the analyzer {0:?} names no language backend this binary has, \
         so c10r cannot build or read an index with it. \
         Upgrade c10r if a newer release supports it, or run `c10r build --force` to replace an index another analyzer built."
    )]
    UnknownAnalyzer(String),
}

/// Compute the content hash over the analyzed sources.
///
/// The hash is order-independent: documents are sorted by path so the same sources always hash the
/// same regardless of discovery order. Each path and text is prefixed with its byte length, so a text
/// that contains the bytes of a separator cannot make one file set hash like another.
pub fn content_hash(documents: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = documents.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, text) in sorted {
        for part in [path, text] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
    }
    hex_digest(hasher)
}

/// The SHA-256 of one file's text, as lowercase hex.
pub fn text_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex_digest(hasher)
}

/// The only item of `items`, or `None` when there are none or several.
pub(crate) fn exactly_one<I: IntoIterator>(items: I) -> Option<I::Item> {
    let mut items = items.into_iter();
    match (items.next(), items.next()) {
        (Some(item), None) => Some(item),
        _ => None,
    }
}

/// How a source file differs between an indexed source list and a discovered one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceChange {
    /// Held by the index and still a source file, with different content.
    Changed,
    /// A source file the index does not hold.
    Added,
    /// Held by the index and no longer a source file.
    Removed,
}

impl SourceChange {
    /// The lowercase name the machine answer and the human rendering carry.
    pub fn label(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }
}

/// One file that differs between an indexed source list and a discovered one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SourceDifference {
    pub document_path: String,
    pub change: SourceChange,
}

/// The files that differ between `indexed` and `discovered`, both `(document_path, text_hash)`
/// pairs, sorted by path: a path in both with different hashes is changed, one only in `discovered`
/// is added, one only in `indexed` is removed.
pub fn compare_source_files(indexed: &[(String, String)], discovered: &[(String, String)]) -> Vec<SourceDifference> {
    let indexed: std::collections::BTreeMap<&str, &str> =
        indexed.iter().map(|(p, h)| (p.as_str(), h.as_str())).collect();
    let discovered: std::collections::BTreeMap<&str, &str> =
        discovered.iter().map(|(p, h)| (p.as_str(), h.as_str())).collect();
    let paths: std::collections::BTreeSet<&str> = indexed.keys().chain(discovered.keys()).copied().collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let change = match (indexed.get(path), discovered.get(path)) {
                (Some(then), Some(now)) if then != now => SourceChange::Changed,
                (None, Some(_)) => SourceChange::Added,
                (Some(_), None) => SourceChange::Removed,
                _ => return None,
            };
            Some(SourceDifference {
                document_path: path.to_string(),
                change,
            })
        })
        .collect()
}

fn hex_digest(hasher: Sha256) -> String {
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn kind_tag(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Module => "module",
        SymbolKind::Type => "type",
        SymbolKind::Trait => "trait",
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Constant => "constant",
        SymbolKind::Field => "field",
        SymbolKind::Other => "other",
    }
}

/// Project canonical identities for the persisted symbols of `index` under `workspace`.
///
/// Local symbols (no descriptor) receive `None` and are excluded from the persisted base; every
/// other symbol receives an identity via collision-only disambiguation. `units[i]` places
/// `index.symbols[i]` in its build unit; the descriptors themselves carry no unit.
fn project_identities(workspace: &WorkspaceId, index: &ExtractedIndex, units: &[Unit]) -> Vec<Option<CanonicalId>> {
    // Gather the projection inputs of persisted symbols, remembering their positions. Each input
    // carries its definition location so a collision group ranks by where each twin is defined, not
    // by the order symbols were encountered; the first occurrence is the fallback for a symbol the
    // backend gave no definition occurrence.
    let mut inputs: Vec<ProjectionInput> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        if sym.class == SymbolClass::Local {
            continue;
        }
        if let Some(d) = &sym.descriptor {
            inputs.push(ProjectionInput {
                descriptor: d.clone(),
                unit: units[idx].unit_marker(),
                definition: sym.definition().map(occurrence_site),
                fallback: sym.occurrences.first().map(occurrence_site),
            });
            positions.push(idx);
        }
    }
    let ids = project_all(workspace, &inputs);
    let mut out = vec![None; index.symbols.len()];
    for (slot, id) in positions.into_iter().zip(ids) {
        out[slot] = Some(id);
    }
    out
}

/// The definition-ranking site of an occurrence: its document and (encoding-native) range.
fn occurrence_site(occ: &crate::semantic::model::ExtractedOccurrence) -> DefinitionSite {
    DefinitionSite {
        document_path: occ.document_path.clone(),
        range: occ.range,
    }
}

/// The syntax language a backend's indexes parse as, from the analyzer's recorded name.
///
/// The recorded analyzer identity is the language marker — the design records no separate language
/// column or flag — so this is the single place that reading is done, for a build and for a query
/// alike. An unrecognized name is refused rather than defaulted: parsing a third backend's documents
/// with the Rust grammar would produce a silently wrong graph, where a refusal names the analyzer
/// nobody taught the system about.
pub fn backend_language(analyzer_name: &str) -> Result<Language, IngestError> {
    if PythonIndexer::from_analyzer_name(analyzer_name).is_some() {
        Ok(Language::Python)
    } else if analyzer_name == RustAdapter::analyzer_name() {
        Ok(Language::Rust)
    } else {
        Err(IngestError::UnknownAnalyzer(analyzer_name.to_string()))
    }
}

/// Ingest an extracted index and its sources into the store as one build.
///
/// This is the composition the whole change turns on: project identities, run the guarded join,
/// persist symbols/occurrences/`contains` edges/spans, attribute every aligned reference to its
/// nearest enclosing persisted declaration, populate the (uncontracted) dependency edges, and record
/// provenance, the content-hash gate, and the join-alignment accounting.
///
/// `sources` are `(document_path, source_text)` pairs. `workspace_root` is the canonicalized
/// filesystem root the build indexed, recorded so a later query can tell which workspace the store
/// describes; `None` when it cannot be recorded exactly, which a query discloses as an unknown
/// workspace relationship rather than a match.
pub fn ingest(
    store: &mut GraphStore,
    workspace: &WorkspaceId,
    workspace_root: Option<&str>,
    index: &ExtractedIndex,
    sources: &[(String, String)],
) -> Result<JoinAccounting, IngestError> {
    ingest_with_params(
        store,
        workspace,
        workspace_root,
        index,
        sources,
        &chunk::ChunkParams::default(),
    )
    .map(|(accounting, _)| accounting)
}

/// [`ingest`] under explicit chunk parameters: the parameters shape every chunk the build embeds
/// and are recorded with the store as part of the semantic-index identity. Returns the join
/// accounting and the disclosure of every declaration unit that could not be determined.
pub fn ingest_with_params(
    store: &mut GraphStore,
    workspace: &WorkspaceId,
    workspace_root: Option<&str>,
    index: &ExtractedIndex,
    sources: &[(String, String)],
    chunk_params: &chunk::ChunkParams,
) -> Result<(JoinAccounting, UndeterminedUnits), IngestError> {
    let language = backend_language(&index.provenance.analyzer_name)?;
    let corpus = SourceCorpus::new(sources.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    // One parse per document: every syntax-reading derivation below borrows this corpus rather
    // than parsing anything itself.
    let prepared = PreparedCorpus::prepare(&corpus, language);
    // The `mod`-declaration evidence feeds two consumers: the unit derivation that decides each
    // identity, and the join's module-chain locality.
    let declarations = declared_from(index, &prepared);
    let (units, unit_disclosure) = assign_units(index, &declarations);
    let identities = project_identities(workspace, index, &units);
    let parent_of = single_parent_map(&declarations);
    let document_unit = document_units(index, &declarations);
    // The document→module derivation runs before the join: the join's self-name and super-keyword
    // rules compare each occurrence against its containing document's own module, and the module
    // bookkeeping below reads the same map — one derivation, two consumers.
    let doc_module = module_by_document(index, &identities, &prepared, language);
    let join_result = join(
        index,
        &prepared,
        &identities,
        language,
        &doc_module,
        &parent_of,
        &units,
        &document_unit,
    );

    let content = content_hash(sources);

    // Line and column come from the same text that produced each byte span, so the two forms of a
    // location always denote the same position.
    let position_indexes: HashMap<&str, PositionIndex<'_>> = sources
        .iter()
        .map(|(path, text)| (path.as_str(), PositionIndex::new(text)))
        .collect();
    let line_column = |document: &str, start: usize, end: usize| -> Option<LineColumnSpan> {
        position_indexes.get(document)?.span(start, end)
    };

    // Map each in-workspace symbol's aligned definition to its name-span, so enclosure and
    // attribution can name enclosing declarations by identity.
    let mut def_name_span: HashMap<CanonicalId, (String, ByteSpan)> = HashMap::new();
    for aligned in &join_result.aligned {
        if aligned.role == OccurrenceRole::Definition {
            def_name_span
                .entry(aligned.symbol.clone())
                .or_insert((aligned.document_path.clone(), aligned.name_span));
        }
    }

    // The inverse map enclosure resolves through: a definition-role aligned location → the identity
    // defined there, so an attribution is one lookup rather than a scan over every definition. A
    // location holding definitions of more than one persisted symbol resolves to no symbol — the
    // refusal is stored as `None` so the lookup carries it, and the occurrence attributes as though
    // no declaration enclosed it (disagreement is represented, never silently resolved).
    let mut definitions_at: HashMap<(&str, ByteSpan), Option<&CanonicalId>> = HashMap::new();
    for aligned in &join_result.aligned {
        if aligned.role != OccurrenceRole::Definition {
            continue;
        }
        definitions_at
            .entry((aligned.document_path.as_str(), aligned.name_span))
            .and_modify(|entry| {
                if *entry != Some(&aligned.symbol) {
                    *entry = None;
                }
            })
            .or_insert(Some(&aligned.symbol));
    }

    // A name -> identity index over persisted type symbols, so an `impl` block attributes its
    // members to the type it implements.
    let mut type_by_name: HashMap<String, CanonicalId> = HashMap::new();
    // The identities of every persisted module-kind symbol, so the pass below can recognize a module
    // definition occurrence without re-deriving `sym.kind` from `def_name_span` (which holds only one
    // document per symbol and so cannot answer "is this a module").
    let mut module_ids: std::collections::HashSet<CanonicalId> = std::collections::HashSet::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        if matches!(sym.kind, SymbolKind::Type | SymbolKind::Trait)
            && let Some(name) = sym.terminal_name()
        {
            type_by_name.entry(name.to_string()).or_insert_with(|| id.clone());
        }
        if sym.kind == SymbolKind::Module {
            module_ids.insert(id.clone());
        }
    }

    // The module symbol defined in each document, so a module-scope reference has a real importer.
    // With duplicate crate roots split into distinct symbols upstream, each module symbol (including
    // each crate root) has exactly one defining document, so this is the plain single-document
    // mapping: every document containing an aligned module definition maps to that module's own
    // identity.
    let mut module_by_doc: HashMap<String, CanonicalId> = HashMap::new();
    for aligned in &join_result.aligned {
        if aligned.role == OccurrenceRole::Definition && module_ids.contains(&aligned.symbol) {
            module_by_doc
                .entry(aligned.document_path.clone())
                .or_insert_with(|| aligned.symbol.clone());
        }
    }
    // Python: a module's definition occurrence is scip-python's zero-width marker at the document
    // origin. Which symbol is a document's module is identity bookkeeping derived once, before the
    // join (`module_by_document` — the same map the join's self-name rule reads); merging it here
    // keeps `imports`-edge sourcing working even for a marker the join refused.
    if language == Language::Python {
        for (doc_path, &idx) in &doc_module {
            if let Some(Some(id)) = identities.get(idx) {
                module_by_doc.entry(doc_path.clone()).or_insert_with(|| id.clone());
            }
        }
    }

    // The per-symbol test classification, computed from the same syntax pass and identity maps the
    // build already holds. Read-only over the join's outputs: it alters no alignment, attribution,
    // or edge derivation.
    let test_rules = classify_test_symbols(
        index,
        &identities,
        language,
        &prepared,
        &def_name_span,
        &module_by_doc,
        &join_result.aligned,
    );

    // True same-descriptor twins: descriptors shared by two or more persisted, definition-bearing
    // symbols (the two-definition quorum). This marks real duplicates only — two DISTINCT descriptors
    // whose canonical projections collide are disambiguated by identity projection but are not
    // duplicated descriptors, and must not be disclosed as such.
    let mut definition_count_by_descriptor: HashMap<&crate::identity::Descriptor, usize> = HashMap::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(_)) = identities.get(idx) else {
            continue;
        };
        if sym.definition().is_some()
            && let Some(descriptor) = &sym.descriptor
        {
            *definition_count_by_descriptor.entry(descriptor).or_default() += 1;
        }
    }

    // Every write below runs inside one transaction: the prior build's derived rows are wholly
    // superseded (a same-version rebuild never accumulates or leaves stale rows), and the guard
    // rolls everything back on a failed build, so the store always holds exactly one whole build.
    let tx = store.begin_build()?;
    // The prior build's semantic representations, read before the clear so a passage whose render
    // is unchanged carries its embeddings forward instead of re-embedding — but only under the
    // same chunk parameters: a different size or overlap makes different chunks of the same
    // render, so nothing carries across a parameter change.
    let prior_semantic = store.semantic_representations()?;
    let params_unchanged = store
        .semantic_index_identity()?
        .is_some_and(|identity| identity.chunk_params == *chunk_params);
    store.clear_derived()?;

    // The rows are retained after insertion: the semantic-corpus pass below reads their tier
    // content and containment to assemble the corpus.
    let mut symbol_rows: Vec<SymbolRow> = Vec::new();
    // The unit-free identity of each duplicated symbol's descriptor, so twins belonging to different
    // units still group together, with the kind-aware descriptor that keeps descriptors differing only
    // in a segment kind apart.
    let mut duplicate_keys: Vec<(CanonicalId, String, String)> = Vec::new();
    // The persisted members of each duplicated descriptor. Unlike the unit-free key, the full
    // descriptor keeps segment kinds, so descriptors differing only in a kind stay separate groups.
    let mut duplicate_members: HashMap<&crate::identity::Descriptor, Vec<CanonicalId>> = HashMap::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        let display_name = sym.display_name().unwrap_or_default().to_string();
        // Exhaustive over `SymbolClass`, so a class the backend model gains later cannot default
        // into `in_workspace` and be persisted as a workspace symbol without anyone noticing.
        let class = match sym.class {
            SymbolClass::External => PersistedClass::External,
            SymbolClass::InWorkspace => PersistedClass::InWorkspace,
            // A local symbol is outside the persisted base and carries no identity, so the identity
            // filter has already skipped it. Stating the exclusion again here keeps the decision
            // beside the classification rather than only in `project_identities`.
            SymbolClass::Local => continue,
        };
        // An in-workspace symbol every one of whose occurrences names a document source discovery
        // never supplied (excluded for an undecodable encoding, most commonly) has no corpus content
        // to be found by. Persisting it would round-trip as a queryable row with every content field
        // empty — a `Found` answer with nothing found, the confidently-wrong shape the calibrated
        // output contract exists to rule out. Its occurrences are already recorded as the join's
        // semantic-only refusal; the row itself is simply not written. A symbol carrying at least one
        // occurrence in a document the corpus does hold keeps its existing row, even when that
        // occurrence did not itself align — that is the pre-existing unaligned-content case, untouched
        // here.
        if class == PersistedClass::InWorkspace
            && !sym.occurrences.is_empty()
            && sym
                .occurrences
                .iter()
                .all(|occ| prepared.get(occ.document_path.as_str()).is_none())
        {
            continue;
        }
        let duplicated = sym.definition().is_some()
            && sym
                .descriptor
                .as_ref()
                .is_some_and(|d| definition_count_by_descriptor.get(d).copied().unwrap_or(0) > 1);
        if duplicated && let Some(descriptor) = &sym.descriptor {
            let key = project_one(workspace, descriptor);
            let kinded = serde_json::to_string(descriptor).expect("descriptor serializes");
            duplicate_keys.push((id.clone(), key.as_str().to_string(), kinded));
            duplicate_members.entry(descriptor).or_default().push(id.clone());
        }
        let content = definition_content(sym, id, &def_name_span, &prepared);
        let symbol_line_column = match (&content.document_path, content.span) {
            (Some(document), Some((start, end))) => line_column(document, start, end),
            _ => None,
        };
        let symbol_header_end = match (&content.document_path, content.header_end) {
            (Some(document), Some(offset)) => position_indexes
                .get(document.as_str())
                .and_then(|index| index.position(offset))
                .map(|at| (offset, at)),
            _ => None,
        };
        symbol_rows.push(SymbolRow {
            canonical_id: id.clone(),
            display_name,
            kind: kind_tag(sym.kind).to_string(),
            class,
            document_path: content.document_path,
            span: content.span,
            line_column: symbol_line_column,
            header_end: symbol_header_end,
            span_text: content.span_text,
            signature_text: content.signature_text,
            interface_text: content.interface_text,
            duplicated,
            test_rule: test_rules.get(id).map(|rule| (*rule).to_string()),
        });
    }
    for row in &symbol_rows {
        store.insert_symbol(row)?;
    }
    store.record_duplicate_keys(&duplicate_keys)?;
    // Which symbols contribute passages, decided from tier content alone. Containment for
    // corpus purposes counts contributing children only — a symbol enclosing nothing but name-only
    // symbols (a Python function over its parameter symbols) stays a leaf — so both containment
    // signals below filter through this set.
    let contributing: std::collections::HashSet<&CanonicalId> = symbol_rows
        .iter()
        .filter(|row| corpus::row_contributes(row))
        .map(|row| &row.canonical_id)
        .collect();

    // Persist aligned occurrences, attributing references to the nearest enclosing persisted symbol.
    for aligned in &join_result.aligned {
        let role = match aligned.role {
            OccurrenceRole::Definition => "definition",
            OccurrenceRole::Reference => "reference",
        };
        let enclosing_id = if aligned.role == OccurrenceRole::Reference {
            let source = prepared
                .get(&aligned.document_path)
                .map(|doc| doc.tree.source())
                .unwrap_or_default();
            enclosing_symbol(
                &aligned.enclosing,
                &aligned.document_path,
                source,
                &definitions_at,
                &type_by_name,
            )
        } else {
            None
        };
        store.insert_occurrence(&OccurrenceRow {
            symbol_id: aligned.symbol.clone(),
            document_path: aligned.document_path.clone(),
            span: (aligned.name_span.start, aligned.name_span.end),
            line_column: line_column(&aligned.document_path, aligned.name_span.start, aligned.name_span.end),
            role: role.to_string(),
            rule: aligned.rule.tag().to_string(),
            enclosing_id,
            locality: aligned.locality.map(|l| l.tag().to_string()),
        })?;
    }

    // Derive `contains` edges from enclosure: each definition's nearest enclosing persisted
    // declaration contains it. Parents of *contributing* children double as the corpus's container
    // set: a symbol that contains another corpus-contributing symbol contributes its interface
    // tier, never its full body.
    let mut container_ids: std::collections::HashSet<CanonicalId> = std::collections::HashSet::new();
    for aligned in &join_result.aligned {
        if aligned.role != OccurrenceRole::Definition {
            continue;
        }
        // An empty aligned span (the Python module origin marker) has a position but no extent;
        // reading enclosure from it would fabricate containment — a document whose first byte sits
        // inside a declaration would make that declaration "contain" the module — so it derives no
        // parent. A whole-document span (a Rust file module) has the symmetric problem: its start
        // byte sits inside whichever declaration opens the document, and nothing within a document
        // encloses the module the document itself defines — so it derives no parent either.
        if aligned.name_span.start == aligned.name_span.end {
            continue;
        }
        if aligned.name_span.start == 0
            && prepared
                .get(&aligned.document_path)
                .is_some_and(|doc| aligned.name_span.end == doc.tree.source().len())
        {
            continue;
        }
        if let Some(parent) = parent_of_definition(aligned, &prepared, &definitions_at, &type_by_name) {
            store.insert_edge(EdgeKind::Contains, &parent, &aligned.symbol)?;
            if contributing.contains(&aligned.symbol) {
                container_ids.insert(parent);
            }
        }
    }

    // Derive the reference-grade dependency edges from aligned references. A reference attributed to
    // an enclosing declaration is a `uses` edge from that declaration to the referenced symbol; a
    // reference attributed to the module (no narrower declaration) is an `imports` edge from the
    // module. Insertion is idempotent, so repeated references collapse to one edge per relation.
    for aligned in &join_result.aligned {
        if aligned.role != OccurrenceRole::Reference {
            continue;
        }
        let source = prepared
            .get(&aligned.document_path)
            .map(|doc| doc.tree.source())
            .unwrap_or_default();
        match enclosing_symbol(
            &aligned.enclosing,
            &aligned.document_path,
            source,
            &definitions_at,
            &type_by_name,
        ) {
            // A reference from inside a declaration: the declaration uses the referenced symbol.
            Some(user) => store.insert_edge(EdgeKind::Uses, &user, &aligned.symbol)?,
            // A module-scope reference (no narrower enclosing declaration): the enclosing module
            // imports the referenced symbol, when the module is persisted.
            None => {
                if let Some(module) = module_by_doc.get(&aligned.document_path) {
                    store.insert_edge(EdgeKind::Imports, module, &aligned.symbol)?;
                }
            }
        }
    }

    // Derive `type_hierarchy` edges from `impl Trait for Type` blocks: an edge from the implementing
    // type to the implemented trait. Each header's trait and type name-token spans resolve through
    // the aligned occurrence sitting at exactly that location — the canonical identity SCIP assigned
    // there — so generic and qualified trait names resolve without string surgery, and same-named
    // symbols are never confused. When no aligned occurrence exists at a span, the edge is skipped;
    // an identity is never fabricated from a display name.
    //
    // Relies on the invariant that one (document, name-span) location holds one token and therefore
    // at most one aligned occurrence attributed to it; `or_insert_with` below is a no-op in practice.
    // If the join ever allowed overlapping attributions at the same location, this map would silently
    // first-win rather than surface the conflict.
    let mut occ_by_location: HashMap<(&str, ByteSpan), CanonicalId> = HashMap::new();
    for aligned in &join_result.aligned {
        occ_by_location
            .entry((aligned.document_path.as_str(), aligned.name_span))
            .or_insert_with(|| aligned.symbol.clone());
    }
    for (path, doc) in prepared.iter() {
        let tree = &doc.tree;
        match language {
            Language::Rust => {
                for imp in tree.trait_impls() {
                    let ty = occ_by_location.get(&(path, imp.type_name_span));
                    let tr = occ_by_location.get(&(path, imp.trait_name_span));
                    if let (Some(ty), Some(tr)) = (ty, tr) {
                        store.insert_edge(EdgeKind::TypeHierarchy, ty, tr)?;
                    }
                }
            }
            // Python: `class Sub(Base1, Base2):` headers, one edge per declared base, each endpoint
            // resolved through the aligned occurrence at its own name-token span — a base token with
            // no aligned occurrence contributes no edge (skip, never guess).
            Language::Python => {
                for class in tree.class_bases() {
                    let Some(sub) = occ_by_location.get(&(path, class.class_name_span)) else {
                        continue;
                    };
                    for base_span in class.base_name_spans {
                        if let Some(base) = occ_by_location.get(&(path, base_span)) {
                            store.insert_edge(EdgeKind::TypeHierarchy, sub, base)?;
                        }
                    }
                }
            }
        }
    }

    // Persist the inspectable detail behind every non-aligned outcome (text-mismatch, semantic-only,
    // and duplicate-ambiguous). A span whose coordinates could not normalize is persisted as typed
    // absence, never a fabricated location. The prior build's rows were removed by `clear_derived`
    // at the start of the build transaction.
    let discrepancies: Vec<DiscrepancyRow> = join_result
        .unaligned
        .iter()
        .map(|occ| {
            let found = occ.found_text.as_deref().map(shorten_quoted_text);
            DiscrepancyRow {
                document_path: occ.document_path.clone(),
                span: occ.span.map(|s| (s.start, s.end)),
                line_column: occ.span.and_then(|s| line_column(&occ.document_path, s.start, s.end)),
                outcome: occ.outcome.tag().to_string(),
                expected_name: occ.expected_name.clone(),
                found_text_shortened: found.as_ref().is_some_and(|(_, shortened)| *shortened),
                found_text: found.map(|(text, _)| text),
            }
        })
        .collect();
    let discrepancy_ids = store.insert_discrepancies(&discrepancies)?;
    let persisted_symbols: std::collections::HashSet<&CanonicalId> =
        symbol_rows.iter().map(|row| &row.canonical_id).collect();
    store.insert_discrepancy_symbols(&discrepancy_links(
        &join_result.unaligned,
        &discrepancy_ids,
        &persisted_symbols,
        &duplicate_members,
    ))?;

    store.insert_unresolved_calls(&unresolved_calls(
        index,
        &prepared,
        &definitions_at,
        &type_by_name,
        &module_by_doc,
        &line_column,
    ))?;

    // The semantic corpus and its representations. Containment for corpus purposes is the union of
    // two signals: the `contains`-edge parents (which catch a type whose methods live in `impl`
    // blocks outside its own span) and span containment within a document (which catches a file
    // module, whose members' enclosing-declaration chains are empty so no edge ever names it).
    let mut span_containers: std::collections::HashSet<&CanonicalId> = std::collections::HashSet::new();
    /// One contributing symbol's identity, definition span, and whether it is a module, grouped per
    /// document below. Only contributing symbols enter: containment over non-contributing symbols
    /// (name-only parameter tokens) must not turn their encloser into a container. The clone pass
    /// reads the same grouping for the symbols a function contains.
    type SpannedSymbol<'a> = (&'a CanonicalId, (usize, usize), bool);
    let mut spans_by_doc: HashMap<&str, Vec<SpannedSymbol<'_>>> = HashMap::new();
    for row in &symbol_rows {
        if !contributing.contains(&row.canonical_id) {
            continue;
        }
        if let (Some(doc), Some(span)) = (row.document_path.as_deref(), row.span) {
            spans_by_doc
                .entry(doc)
                .or_default()
                .push((&row.canonical_id, span, row.kind == "module"));
        }
    }
    for spans in spans_by_doc.values() {
        for (id, outer, is_module) in spans {
            // Containment compares identities, not spans: a file module whose span exactly equals
            // its sole declaration's span still encloses it.
            let contains_other = spans
                .iter()
                .any(|(other, inner, _)| *other != *id && corpus::span_encloses(*outer, *inner, *is_module));
            if contains_other {
                span_containers.insert(id);
            }
        }
    }
    let corpus_sources: Vec<corpus::CorpusSource<'_>> = symbol_rows
        .iter()
        .filter(|row| row.class == PersistedClass::InWorkspace)
        .map(|row| corpus::CorpusSource {
            canonical_id: &row.canonical_id,
            display_name: &row.display_name,
            kind: &row.kind,
            document_path: row.document_path.as_deref(),
            span: row.span.map(|(start, end)| ByteSpan { start, end }),
            signature_text: row.signature_text.as_deref(),
            interface_text: row.interface_text.as_deref(),
            span_text: row.span_text.as_deref(),
            contains_persisted: container_ids.contains(&row.canonical_id)
                || span_containers.contains(&row.canonical_id),
        })
        .collect();
    let passages = corpus::assemble(&corpus_sources);

    // Each passage's chunk vectors: carried forward when the render is byte-identical to the prior
    // build's and the chunk parameters are unchanged (the model is deterministic and pinned, so
    // the carried and recomputed vectors are identical by construction); split into chunks and
    // embedded in one batch otherwise. The lexical row stays one per passage over the whole render.
    let mut vectors: Vec<Option<Vec<Vec<u8>>>> = vec![None; passages.len()];
    let mut pending: Vec<usize> = Vec::new();
    for (i, passage) in passages.iter().enumerate() {
        match prior_semantic.get(passage.symbol_id.as_str()) {
            Some(prior) if params_unchanged && prior.render == passage.render => {
                vectors[i] = Some(prior.embeddings.clone())
            }
            _ => pending.push(i),
        }
    }
    if !pending.is_empty() {
        // Split every pending passage — reading the prepared tree, never parsing — then embed all
        // chunks in one batch and distribute the vectors back by count.
        let mut chunk_counts: Vec<usize> = Vec::with_capacity(pending.len());
        let mut texts: Vec<String> = Vec::new();
        for &i in &pending {
            let passage = &passages[i];
            let tree = passage
                .content_location
                .as_ref()
                .and_then(|(path, span)| prepared.get(path).map(|doc| (&doc.tree, *span)));
            let header = chunk::PassageHeader {
                identity: &passage.header_identity,
                documentation: passage.documentation.as_deref(),
            };
            let chunks = chunk::split_passage(&header, &passage.content, tree, chunk_params);
            chunk_counts.push(chunks.len());
            texts.extend(chunks.into_iter().map(|c| c.text));
        }
        let embedded = embed::embed_batch(&texts);
        let mut cursor = 0;
        for (&i, count) in pending.iter().zip(chunk_counts) {
            vectors[i] = Some(
                embedded[cursor..cursor + count]
                    .iter()
                    .map(|vector| embed::vector_bytes(vector))
                    .collect(),
            );
            cursor += count;
        }
    }
    for (passage, chunk_vectors) in passages.iter().zip(&vectors) {
        let chunk_vectors = chunk_vectors.as_ref().expect("every passage embeds or carries forward");
        let words = corpus::split_words(&passage.render).join(" ");
        store.insert_passage(&passage.symbol_id, &passage.render, &words, chunk_vectors)?;
    }

    // Clone-equivalence keys for the corpus's leaves, from each leaf's spelled token sequence, and
    // shared-fragment evidence over every symbol's comparable tokens. Containers carry no keys: a
    // container "clone" over interface text would assert a body equivalence the key never examined.
    // A function or method containing corpus symbols has comparable tokens over its own code, so it
    // carries a token count and fingerprints; a type or module containing them carries nothing.
    let (k, w) = (clone::FRAGMENT_KGRAM_LENGTH, clone::FRAGMENT_WINDOW);
    for source in &corpus_sources {
        let callable = matches!(source.kind, "function" | "method");
        if !source.contributes() || (source.contains_persisted && !callable) {
            continue;
        }
        let (Some(path), Some(span)) = (source.document_path, source.span) else {
            continue;
        };
        let Some(doc) = prepared.get(path) else {
            continue;
        };
        let tokens = if source.contains_persisted {
            let candidates = spans_by_doc
                .get(path)
                .into_iter()
                .flatten()
                .map(|&(id, span, _)| (id, span));
            let contained = clone::contained_symbols(source.canonical_id, (span.start, span.end), candidates);
            clone::comparable_tokens(&doc.tree, span, &contained)
        } else {
            let tokens = doc.tree.spelled_tokens(span);
            let Some(keys) = clone::clone_keys(&tokens) else {
                continue;
            };
            store.set_clone_keys(source.canonical_id, &keys.formatting_key, &keys.substitution_key)?;
            tokens
        };
        store.set_clone_token_count(source.canonical_id, tokens.len())?;
        store.write_clone_fingerprints(source.canonical_id, &clone::winnow(&tokens, k, w))?;
    }

    let fingerprints: Vec<(String, String)> = sources
        .iter()
        .map(|(path, text)| (path.clone(), text_hash(text)))
        .collect();
    store.write_source_files(&fingerprints)?;
    store.write_metadata(&IndexMetadata {
        workspace_id: workspace.clone(),
        workspace_root: workspace_root.map(str::to_string),
        provenance: index.provenance.clone(),
        content_hash: content,
        accounting: join_result.accounting,
        environment: index.environment.clone(),
        chunk_params: *chunk_params,
        targets: RecordedTargets {
            inventory: index.targets.clone(),
            undetermined_units: unit_disclosure.clone(),
        },
    })?;

    // The single commit publishes the whole build atomically: a crash anywhere above rolls back to
    // the prior build wholesale, so fresh metadata can never coexist with another build's rows.
    tx.commit()?;

    Ok((join_result.accounting, unit_disclosure))
}

/// Refuse to join a fresh syntax tree against a stale index: verify the sources hash to the recorded
/// content hash before ingesting an update against an existing index.
pub fn join_guarded(
    store: &mut GraphStore,
    workspace: &WorkspaceId,
    workspace_root: Option<&str>,
    index: &ExtractedIndex,
    sources: &[(String, String)],
    expected_hash: &str,
) -> Result<JoinAccounting, IngestError> {
    if content_hash(sources) != expected_hash {
        return Err(IngestError::ContentHashMismatch);
    }
    ingest(store, workspace, workspace_root, index, sources)
}

/// The definition content persisted for a symbol: its document, byte span, the byte offset where its
/// header ends (none for a document's own module), and the body/signature/interface tier text. All fields default to `None` (no document, no persisted content).
#[derive(Default)]
struct DefinitionContent {
    document_path: Option<String>,
    span: Option<(usize, usize)>,
    header_end: Option<usize>,
    span_text: Option<String>,
    signature_text: Option<String>,
    interface_text: Option<String>,
}

/// The definition content for a symbol, from its aligned definition name-span expanded to the
/// enclosing declaration's full span, with the declaration's tier content alongside it.
///
/// A symbol whose name-span matches a declaration in its document's syntax tree — the ordinary
/// case, which includes an inline `mod name { .. }` declaration — persists that declaration's full
/// span and text as its body, with its signature and interface tiers read off the same tree. A
/// module-kind symbol whose name-span matches no declaration (a Rust file module's whole-file
/// definition occurrence, or a Python module's zero-width origin marker) persists the whole
/// document as its body, its qualified name as its signature, and its signature followed by its
/// module documentation as its interface (the signature alone when the document carries no
/// documentation — the same fallback every symbol's interface obeys). Any other
/// symbol matching no declaration persists its name-span text as body, signature, and interface
/// alike — the honest, total degradation for a shape the declaration walk did not recognize. An
/// external symbol, or an in-workspace symbol whose document carries no source, persists no
/// content.
///
/// The header end is the signature tier's cut at the body delimiter for a recognized declaration,
/// and the span's end for a symbol stored with its name span.
fn definition_content(
    sym: &ExtractedSymbol,
    id: &CanonicalId,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    prepared: &PreparedCorpus,
) -> DefinitionContent {
    if sym.class == SymbolClass::External {
        return DefinitionContent::default();
    }
    let Some((doc, name_span)) = def_name_span.get(id) else {
        return DefinitionContent::default();
    };
    let Some(prepared_doc) = prepared.get(doc) else {
        return DefinitionContent {
            document_path: Some(doc.clone()),
            ..Default::default()
        };
    };
    let source = prepared_doc.tree.source();

    if let Some(decl) = prepared_doc.declaration_at(*name_span) {
        let text = source.get(decl.full_span.start..decl.full_span.end).map(str::to_string);
        let tiers = prepared_doc.tree.declaration_tiers(decl);
        return DefinitionContent {
            document_path: Some(doc.clone()),
            span: Some((decl.full_span.start, decl.full_span.end)),
            header_end: Some(tiers.header_end),
            span_text: text,
            signature_text: Some(tiers.signature),
            interface_text: Some(tiers.interface),
        };
    }

    if sym.kind == SymbolKind::Module {
        let qualified = qualified_name(id);
        let interface = match prepared_doc.tree.module_documentation() {
            Some(docs) => format!("{qualified}\n{}", docs.trim_end()),
            None => qualified.to_string(),
        };
        return DefinitionContent {
            document_path: Some(doc.clone()),
            span: Some((0, source.len())),
            header_end: None,
            span_text: Some(source.to_string()),
            signature_text: Some(qualified.to_string()),
            interface_text: Some(interface),
        };
    }

    let text = source.get(name_span.start..name_span.end).map(str::to_string);
    DefinitionContent {
        document_path: Some(doc.clone()),
        span: Some((name_span.start, name_span.end)),
        header_end: Some(name_span.end),
        span_text: text.clone(),
        signature_text: text.clone(),
        interface_text: text,
    }
}

/// The qualified-name part of a canonical identity: the identity with its leading workspace
/// segment stripped (`<workspace>::<qualified>` → `<qualified>`). A trailing `#<rank>`
/// disambiguator, when the identity carries one, stays — it is part of the identity, not decoration.
fn qualified_name(id: &CanonicalId) -> &str {
    id.as_str()
        .split_once("::")
        .map(|(_, rest)| rest)
        .unwrap_or(id.as_str())
}

#[cfg(test)]
thread_local! {
    /// Candidate comparisons performed while resolving attributions on this thread: one per chain
    /// declaration an attribution lookup considers. Thread-local because ingest is single-threaded,
    /// so a thread-scoped count is exact regardless of other tests running in parallel.
    static ATTRIBUTION_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Reset this thread's attribution-comparison counter.
#[cfg(test)]
pub fn reset_attribution_comparisons() {
    ATTRIBUTION_COMPARISONS.with(|count| count.set(0));
}

/// The number of candidate comparisons attribution lookups performed on this thread since the last
/// reset.
#[cfg(test)]
pub fn attribution_comparisons() -> usize {
    ATTRIBUTION_COMPARISONS.with(|count| count.get())
}

/// Map an enclosing syntax-declaration chain to the identity of the nearest persisted declaration.
///
/// The chain is innermost-first. A declaration attributes to the persisted symbol whose aligned
/// definition sits at the declaration's own name-span location, resolved through the
/// definition-location map — one lookup per chain declaration. A location holding definitions of
/// more than one persisted symbol resolves to no symbol, and the occurrence attributes as though no
/// declaration enclosed it. An `impl_item` is not itself a persisted symbol — it associates its
/// members with the type it implements — so it resolves to the persisted type whose terminal name
/// equals the impl's type identifier, which is how a method attributes to its type. An empty chain
/// (or no match) attributes to the module — represented as `None`, which the store reads as "the
/// module/file itself".
fn enclosing_symbol(
    chain: &[syntax::SyntaxDeclaration],
    document_path: &str,
    source: &str,
    definitions_at: &HashMap<(&str, ByteSpan), Option<&CanonicalId>>,
    type_by_name: &HashMap<String, CanonicalId>,
) -> Option<CanonicalId> {
    for decl in chain {
        #[cfg(test)]
        ATTRIBUTION_COMPARISONS.with(|count| count.set(count.get() + 1));
        match definitions_at.get(&(document_path, decl.name_span)) {
            // A direct persisted declaration: exactly one definition at this location.
            Some(Some(id)) => return Some((*id).clone()),
            // More than one persisted definition at this location: naming either would be a guess,
            // so the whole attribution refuses and the occurrence falls to its module.
            Some(None) => return None,
            None => {}
        }
        // An impl block: resolve to the type it implements, by the type identifier's text.
        if decl.node_kind == "impl_item"
            && let Some(type_name) = source.get(decl.name_span.start..decl.name_span.end)
            && let Some(id) = type_by_name.get(type_name)
        {
            return Some(id.clone());
        }
    }
    None
}

/// The symbols each refused reference occurrence may denote, as `(discrepancy id, symbol)` links.
///
/// `discrepancy_ids` is parallel to `unaligned`. A duplicate-ambiguous occurrence links every
/// persisted member of the duplicate group recorded on it, found by full descriptor equality; any
/// other refused reference, or one whose descriptor has no group, links its own symbol when that is
/// persisted. A refused definition links nothing.
fn discrepancy_links(
    unaligned: &[UnalignedOccurrence],
    discrepancy_ids: &[i64],
    persisted: &std::collections::HashSet<&CanonicalId>,
    duplicate_members: &HashMap<&crate::identity::Descriptor, Vec<CanonicalId>>,
) -> Vec<(i64, CanonicalId)> {
    let mut links = Vec::new();
    for (occ, &discrepancy_id) in unaligned.iter().zip(discrepancy_ids) {
        if occ.role != OccurrenceRole::Reference {
            continue;
        }
        let group = (occ.outcome == JoinOutcome::DuplicateAmbiguous)
            .then(|| occ.duplicate_group.as_ref().and_then(|d| duplicate_members.get(d)))
            .flatten();
        match group {
            Some(members) => links.extend(members.iter().map(|id| (discrepancy_id, id.clone()))),
            None if persisted.contains(&occ.symbol) => links.push((discrepancy_id, occ.symbol.clone())),
            None => {}
        }
    }
    links
}

/// The calls whose called name no semantic occurrence covers.
///
/// A name is covered when its byte span equals the normalized span of any occurrence the index holds
/// for the document — aligned or refused by the join, definition or reference, local or not. An
/// occurrence whose range cannot be normalized covers nothing. A call attributes to its nearest
/// enclosing persisted declaration, else to its document's module.
fn unresolved_calls(
    index: &ExtractedIndex,
    prepared: &PreparedCorpus,
    definitions_at: &HashMap<(&str, ByteSpan), Option<&CanonicalId>>,
    type_by_name: &HashMap<String, CanonicalId>,
    module_by_doc: &HashMap<String, CanonicalId>,
    line_column: &dyn Fn(&str, usize, usize) -> Option<LineColumnSpan>,
) -> Vec<UnresolvedCallRow> {
    let mut ranges: HashMap<&str, Vec<SourceRange>> = HashMap::new();
    let occurrences = index
        .symbols
        .iter()
        .flat_map(|symbol| &symbol.occurrences)
        .chain(index.duplicate_groups.iter().flat_map(|group| &group.occurrences));
    for occ in occurrences {
        ranges.entry(occ.document_path.as_str()).or_default().push(occ.range);
    }
    for (path, local) in &index.local_occurrence_ranges {
        ranges.entry(path.as_str()).or_default().extend(local);
    }

    let mut rows = Vec::new();
    for (path, doc) in prepared.iter() {
        let covered: std::collections::HashSet<ByteSpan> = index
            .encoding_for(path)
            .into_iter()
            .flat_map(|encoding| {
                ranges
                    .get(path)
                    .into_iter()
                    .flatten()
                    .filter_map(move |&range| range_to_span(doc.tree.source(), &doc.line_index, range, encoding))
            })
            .collect();
        let source = doc.tree.source();
        for call in doc.tree.calls() {
            if covered.contains(&call.name_span) {
                continue;
            }
            let chain = doc.tree.enclosing_declarations(call.name_span.start);
            let enclosing_id = enclosing_symbol(&chain, path, source, definitions_at, type_by_name)
                .or_else(|| module_by_doc.get(path).cloned());
            let receiver = call
                .receiver_span
                .and_then(|span| doc.tree.text_at(span))
                .map(shorten_quoted_text);
            rows.push(UnresolvedCallRow {
                document_path: path.to_string(),
                name: call.name,
                span: (call.name_span.start, call.name_span.end),
                line_column: line_column(path, call.name_span.start, call.name_span.end),
                receiver_shortened: receiver.as_ref().is_some_and(|(_, shortened)| *shortened),
                receiver: receiver.map(|(text, _)| text),
                enclosing_id,
            });
        }
    }
    rows
}

/// The parent declaration of a definition: the nearest enclosing persisted declaration other than
/// the definition itself.
fn parent_of_definition(
    aligned: &AlignedOccurrence,
    prepared: &PreparedCorpus,
    definitions_at: &HashMap<(&str, ByteSpan), Option<&CanonicalId>>,
    type_by_name: &HashMap<String, CanonicalId>,
) -> Option<CanonicalId> {
    let doc = prepared.get(&aligned.document_path)?;
    let chain = doc.tree.enclosing_declarations(aligned.name_span.start);
    let parent_chain: Vec<syntax::SyntaxDeclaration> =
        chain.into_iter().filter(|d| d.name_span != aligned.name_span).collect();
    enclosing_symbol(
        &parent_chain,
        &aligned.document_path,
        doc.tree.source(),
        definitions_at,
        type_by_name,
    )
}

/// Classify every persisted in-workspace symbol as test code or not, from statically observable
/// language-convention signals, returning the accepting rule tag per classified identity (a symbol
/// absent from the map is non-test).
///
/// When several rules accept one symbol, the recorded rule is the first in the fixed order
/// attribute > configuration > file > directory — the strongest evidence wins, so provenance is
/// deterministic. The classification reads the join's outputs and the syntax trees only; it never
/// alters alignment, attribution, or edge derivation.
fn classify_test_symbols(
    index: &ExtractedIndex,
    identities: &[Option<CanonicalId>],
    language: Language,
    prepared: &PreparedCorpus,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    module_by_doc: &HashMap<String, CanonicalId>,
    aligned: &[AlignedOccurrence],
) -> HashMap<CanonicalId, &'static str> {
    // Per-document convention signals, read once per document from its prepared syntax tree: the
    // name spans of test-attributed declarations, the spans of inline `#[cfg(test)]` module bodies,
    // the gated out-of-line module declarations, and every out-of-line module declaration (through
    // which gating propagates into other documents).
    let mut attr_names: HashMap<String, std::collections::HashSet<ByteSpan>> = HashMap::new();
    let mut inline_gated: HashMap<String, Vec<ByteSpan>> = HashMap::new();
    let mut gated_mod_decls: Vec<(String, ByteSpan)> = Vec::new();
    let mut out_of_line_mods: HashMap<String, Vec<ByteSpan>> = HashMap::new();
    if language == Language::Rust {
        for (path, doc) in prepared.iter() {
            let tree = &doc.tree;
            attr_names.insert(
                path.to_string(),
                tree.test_attributed_declaration_names().into_iter().collect(),
            );
            for gated in tree.cfg_test_modules() {
                match gated.inline_span {
                    Some(span) => inline_gated.entry(path.to_string()).or_default().push(span),
                    None => gated_mod_decls.push((path.to_string(), gated.name_span)),
                }
            }
            out_of_line_mods.insert(path.to_string(), tree.out_of_line_module_names());
        }
    }

    // The identity aligned at a location, resolving a gated `mod name;` declaration to its module
    // symbol — never by name matching.
    let mut occ_at: HashMap<(&str, ByteSpan), &CanonicalId> = HashMap::new();
    for occ in aligned {
        occ_at
            .entry((occ.document_path.as_str(), occ.name_span))
            .or_insert(&occ.symbol);
    }
    // A module's defining document, for carrying gating into an out-of-line body's document and for
    // classifying a module symbol that carries no aligned definition of its own.
    let mut doc_of_module: HashMap<&CanonicalId, &str> = HashMap::new();
    for (doc, id) in module_by_doc {
        doc_of_module.entry(id).or_insert(doc.as_str());
    }
    let resolve_mod_doc = |doc: &str, name_span: ByteSpan| -> Option<&str> {
        let id = occ_at.get(&(doc, name_span))?;
        doc_of_module.get(id).copied()
    };

    // The documents whose symbols are test-configured: seeded by gated out-of-line declarations (and
    // out-of-line declarations inside an inline-gated body), then closed over the out-of-line module
    // declarations each gated document itself contains — containment taken from the persisted
    // module identities after all documents are parsed.
    let mut gated_docs: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut pending: Vec<&str> = Vec::new();
    for (doc, name_span) in &gated_mod_decls {
        if let Some(target) = resolve_mod_doc(doc, *name_span) {
            pending.push(target);
        }
    }
    for (doc, spans) in &inline_gated {
        for name_span in out_of_line_mods.get(doc).into_iter().flatten() {
            if spans.iter().any(|s| s.contains(name_span))
                && let Some(target) = resolve_mod_doc(doc, *name_span)
            {
                pending.push(target);
            }
        }
    }
    while let Some(doc) = pending.pop() {
        if !gated_docs.insert(doc) {
            continue;
        }
        for name_span in out_of_line_mods.get(doc).into_iter().flatten() {
            if let Some(target) = resolve_mod_doc(doc, *name_span) {
                pending.push(target);
            }
        }
    }

    let mut rules: HashMap<CanonicalId, &'static str> = HashMap::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        if sym.class == SymbolClass::External {
            continue;
        }
        // The symbol's defining document and name span. A symbol with no aligned definition still
        // classifies through its document alone: a module through the document→module derivation (a
        // refused Python origin marker), any other symbol through its extracted definition
        // occurrence — the occurrence's document path is trustworthy even when the join refused its
        // span, so the document-scoped rules apply while the span-dependent ones stay out.
        let (doc, name_span) = match def_name_span.get(id) {
            Some((doc, span)) => (doc.as_str(), Some(*span)),
            None => {
                let module_doc = (sym.kind == SymbolKind::Module)
                    .then(|| doc_of_module.get(id).copied())
                    .flatten();
                match module_doc.or_else(|| sym.definition().map(|occ| occ.document_path.as_str())) {
                    Some(doc) => (doc, None),
                    None => continue,
                }
            }
        };
        let rule = match language {
            Language::Rust => {
                let attributed =
                    name_span.is_some_and(|span| attr_names.get(doc).is_some_and(|names| names.contains(&span)));
                let configured = gated_docs.contains(doc)
                    || name_span
                        .is_some_and(|span| inline_gated.get(doc).into_iter().flatten().any(|s| s.contains(&span)));
                if attributed {
                    Some("test_attribute")
                } else if configured {
                    Some("test_configuration")
                } else if has_tests_directory_component(doc) {
                    Some("test_directory")
                } else {
                    None
                }
            }
            Language::Python => {
                if is_python_test_file_name(doc) {
                    Some("test_file")
                } else if has_tests_directory_component(doc) {
                    Some("test_directory")
                } else {
                    None
                }
            }
        };
        if let Some(rule) = rule {
            rules.insert(id.clone(), rule);
        }
    }
    rules
}

/// Whether a workspace-relative document path lies under a directory named `tests` — the
/// test-directory classification signal (the file name itself is not a directory component).
fn has_tests_directory_component(path: &str) -> bool {
    let mut components: Vec<&str> = path.split('/').collect();
    components.pop();
    components.contains(&"tests")
}

/// Whether a Python document's file name follows the test runners' file-collection conventions:
/// pytest's `test_*.py` / `*_test.py` discovery defaults, the conventional single-file module
/// `tests.py`, or the reserved fixture file `conftest.py`. A file name that merely begins with the
/// word test (`testimony.py`) matches no form and stays non-test.
fn is_python_test_file_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == "tests.py" || name == "conftest.py" {
        return true;
    }
    let Some(stem) = name.strip_suffix(".py") else {
        return false;
    };
    stem.starts_with("test_") || stem.ends_with("_test")
}

/// Evaluate freshness of the store against the current sources, analyzer, and declared environment
/// in effect (`None` when no environment applies or none resolves).
pub fn freshness(
    store: &GraphStore,
    sources: &[(String, String)],
    current: &crate::semantic::model::AnalyzerProvenance,
    current_environment: Option<&crate::semantic::model::EnvironmentFacts>,
) -> rusqlite::Result<Option<Freshness>> {
    let hash = content_hash(sources);
    store.freshness(&hash, current, current_environment)
}

/// Shared fixtures for the work-bound tests: minimal Rust-shaped indexes whose symbols and
/// references align through the ordinary join, so parse and comparison counts measure real ingests.
#[cfg(test)]
pub(crate) mod work_bound_fixture {
    use crate::identity::{Descriptor, DescriptorSegment, SegmentKind};
    use crate::semantic::model::{
        AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
        SourceDocument, SourceRange, SymbolClass, SymbolKind,
    };

    /// An in-workspace function symbol defined at `fn <name>() {}` on line `line` of `doc`, with
    /// any extra occurrences appended.
    pub(crate) fn fn_symbol(name: &str, doc: &str, line: u32) -> ExtractedSymbol {
        ExtractedSymbol {
            descriptor: Some(Descriptor::new(
                "crate",
                vec![DescriptorSegment::new(name, SegmentKind::Term)],
            )),
            kind: SymbolKind::Function,
            class: SymbolClass::InWorkspace,
            occurrences: vec![ExtractedOccurrence {
                document_path: doc.to_string(),
                range: SourceRange::new(line, 3, line, 3 + name.len() as u32),
                role: OccurrenceRole::Definition,
            }],
        }
    }

    /// A Rust index over `(document, symbols)` pairs, every document UTF-8 encoded.
    pub(crate) fn index_over(docs: &[(&str, Vec<ExtractedSymbol>)]) -> ExtractedIndex {
        ExtractedIndex {
            provenance: AnalyzerProvenance {
                analyzer_name: "rust-analyzer".to_string(),
                analyzer_version: "1.0.0".to_string(),
            },
            documents: docs
                .iter()
                .map(|(path, _)| SourceDocument {
                    path: path.to_string(),
                    encoding: PositionEncoding::Utf8,
                })
                .collect(),
            symbols: docs.iter().flat_map(|(_, symbols)| symbols.clone()).collect(),
            duplicate_groups: Vec::new(),
            targets: Default::default(),
            environment: None,
            local_occurrence_ranges: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::work_bound_fixture::{fn_symbol, index_over};
    use super::*;
    use crate::semantic::model::{ExtractedOccurrence, SourceRange};

    fn source(path: &str, text: &str) -> (String, String) {
        (path.to_string(), text.to_string())
    }

    // A file's text may contain any byte sequence, so a separator inside it must not let one file
    // set hash like another.
    #[test]
    fn content_hash_distinguishes_a_text_that_imitates_the_next_file() {
        let one_file = [source("a.rs", "// x\0b.rs\0// y")];
        let two_files = [source("a.rs", "// x"), source("b.rs", "// y")];
        assert_ne!(content_hash(&one_file), content_hash(&two_files));
    }

    #[test]
    fn content_hash_ignores_discovery_order() {
        let forward = [source("a.rs", "one"), source("b.rs", "two")];
        let reversed = [source("b.rs", "two"), source("a.rs", "one")];
        assert_eq!(content_hash(&forward), content_hash(&reversed));
    }

    // Each known analyzer names its own language, and an analyzer no backend claims is refused.
    //
    // A default here would parse a third backend's documents with the Rust grammar and persist a
    // silently wrong graph — every span, every declaration, and every join outcome derived from the
    // wrong tree, with nothing to indicate it.
    #[test]
    fn the_language_dispatch_refuses_an_analyzer_no_backend_claims() {
        assert_eq!(backend_language(RustAdapter::analyzer_name()).unwrap(), Language::Rust);
        for indexer in [PythonIndexer::ScipPython, PythonIndexer::TyScip] {
            assert_eq!(backend_language(indexer.analyzer_name()).unwrap(), Language::Python);
        }

        let refused = backend_language("scip-typescript").expect_err("an unclaimed analyzer is refused");
        assert!(
            matches!(&refused, IngestError::UnknownAnalyzer(name) if name == "scip-typescript"),
            "the refusal names the analyzer: {refused}"
        );
    }

    /// A workspace of one document holding `callee_count` callee functions plus one caller whose
    /// body references the first callee: `fn f0() {} … fn caller() { f0(); }`.
    fn workspace_with_callees(callee_count: u32) -> (ExtractedIndex, Vec<(String, String)>) {
        let mut source = String::new();
        let mut symbols = Vec::new();
        for i in 0..callee_count {
            symbols.push(fn_symbol(&format!("f{i}"), "m.rs", i));
            source.push_str(&format!("fn f{i}() {{}}\n"));
        }
        let caller_line = callee_count;
        let mut caller = fn_symbol("caller", "m.rs", caller_line);
        source.push_str("fn caller() { f0(); }\n");
        // The reference to `f0` inside `caller`'s body: `fn caller() { ` is 14 bytes, `f0` is 2.
        symbols[0].occurrences.push(ExtractedOccurrence {
            document_path: "m.rs".to_string(),
            range: SourceRange::new(caller_line, 14, caller_line, 16),
            role: OccurrenceRole::Reference,
        });
        // `caller` is 6 characters at column 3.
        caller.occurrences[0].range = SourceRange::new(caller_line, 3, caller_line, 9);
        symbols.push(caller);
        (index_over(&[("m.rs", symbols)]), vec![("m.rs".to_string(), source)])
    }

    /// Run one full ingest and return the attribution comparisons it performed.
    fn comparisons_during_ingest(index: &ExtractedIndex, sources: &[(String, String)]) -> usize {
        let mut store = GraphStore::open_in_memory().unwrap();
        reset_attribution_comparisons();
        ingest(&mut store, &WorkspaceId::new("bound-ws"), None, index, sources).unwrap();
        attribution_comparisons()
    }

    // _(Reference occurrences carry enclosing-declaration attribution — work bound)_ — attribution
    // work per occurrence is unchanged when the workspace persists many times as many definitions.
    #[test]
    fn attribution_work_per_occurrence_is_independent_of_definition_count() {
        let (small_index, small_sources) = workspace_with_callees(1);
        let (large_index, large_sources) = workspace_with_callees(16);
        let small = comparisons_during_ingest(&small_index, &small_sources);
        let large = comparisons_during_ingest(&large_index, &large_sources);
        assert!(small > 0, "the reference attributed, so comparisons were performed");
        assert_eq!(
            small, large,
            "one reference costs the same comparisons under 2 and 17 persisted definitions"
        );
    }

    // _(Reference occurrences carry enclosing-declaration attribution — shared-location refusal)_ —
    // an occurrence whose nearest enclosing declaration's location holds two persisted definitions
    // attributes to its module, never to an arbitrary one of the two.
    #[test]
    fn occurrence_under_a_shared_definition_location_attributes_to_its_module() {
        let source = "fn x() { b(); }\nfn b() {}\n";
        // Two persisted symbols both defining at `x`'s name token: identical location, distinct
        // descriptors (a synthetic collision, the shape scip-python emits for `codes = (` openers).
        let x_as_term = fn_symbol("x", "m.rs", 0);
        let mut x_as_type = fn_symbol("x", "m.rs", 0);
        x_as_type.descriptor = Some(crate::identity::Descriptor::new(
            "crate",
            vec![crate::identity::DescriptorSegment::new(
                "x",
                crate::identity::SegmentKind::Type,
            )],
        ));
        let mut b = fn_symbol("b", "m.rs", 1);
        // The reference to `b` inside `fn x`'s body: `fn x() { ` is 9 bytes, `b` is 1.
        b.occurrences.push(ExtractedOccurrence {
            document_path: "m.rs".to_string(),
            range: SourceRange::new(0, 9, 0, 10),
            role: OccurrenceRole::Reference,
        });
        let b_descriptor = b.descriptor.clone().unwrap();
        let index = index_over(&[("m.rs", vec![x_as_term, x_as_type, b])]);

        let mut store = GraphStore::open_in_memory().unwrap();
        let ws = WorkspaceId::new("bound-ws");
        ingest(
            &mut store,
            &ws,
            None,
            &index,
            &[("m.rs".to_string(), source.to_string())],
        )
        .unwrap();

        let b_id = crate::identity::project_one(&ws, &b_descriptor);
        let refs = store.references_of(&b_id).unwrap();
        assert_eq!(refs.len(), 1, "the reference to b persists");
        assert_eq!(
            refs[0].enclosing_id, None,
            "a shared definition location refuses: the occurrence attributes to its module"
        );
    }
}
