//! The code graph: the guarded join, the SQLite-core store, and the ingest that unifies the two
//! oracles into one persisted graph.

pub mod join;
pub mod range;
pub mod schema;
pub mod store;
pub mod syntax;

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::identity::{CanonicalId, DefinitionSite, ProjectionInput, WorkspaceId, project_all};
use crate::semantic::model::{ExtractedIndex, ExtractedSymbol, OccurrenceRole, SymbolClass, SymbolKind};
use crate::semantic::python_adapter::PythonAdapter;

use join::{AlignedOccurrence, JoinAccounting, SourceCorpus, join, module_by_document};
use range::ByteSpan;
use store::{
    DiscrepancyRow, EdgeKind, Freshness, GraphStore, IndexMetadata, OccurrenceRow, PersistedClass, SymbolRow,
};
use syntax::Language;

/// An error during ingest.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The store rejected an operation.
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),
    /// The syntax tree could not be reconciled with the index because of a content-hash mismatch.
    #[error("content-hash mismatch: sources do not match the index being joined")]
    ContentHashMismatch,
}

/// Compute the content hash over the analyzed sources.
///
/// The hash is order-independent: documents are sorted by path so the same sources always hash the
/// same regardless of discovery order.
pub fn content_hash(documents: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = documents.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, text) in sorted {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(text.as_bytes());
        hasher.update([0u8]);
    }
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
/// other symbol receives an identity via collision-only disambiguation.
fn project_identities(workspace: &WorkspaceId, index: &ExtractedIndex) -> Vec<Option<CanonicalId>> {
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

/// The syntax language an index's documents parse as.
///
/// The recorded analyzer identity is the language marker (the design records no separate language
/// column or flag): a scip-python index is Python source, anything else is Rust.
fn index_language(index: &ExtractedIndex) -> Language {
    if index.provenance.analyzer_name == PythonAdapter::analyzer_name() {
        Language::Python
    } else {
        Language::Rust
    }
}

/// Ingest an extracted index and its sources into the store as one build.
///
/// This is the composition the whole change turns on: project identities, run the guarded join,
/// persist symbols/occurrences/`contains` edges/spans, attribute every aligned reference to its
/// nearest enclosing persisted declaration, populate the (uncontracted) dependency edges, and record
/// provenance, the content-hash gate, and the join-alignment accounting.
///
/// `sources` are `(document_path, source_text)` pairs.
pub fn ingest(
    store: &mut GraphStore,
    workspace: &WorkspaceId,
    index: &ExtractedIndex,
    sources: &[(String, String)],
) -> Result<JoinAccounting, IngestError> {
    let identities = project_identities(workspace, index);
    let language = index_language(index);
    let corpus = SourceCorpus::new(sources.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    // The document→module derivation runs before the join: the join's self-name and super-keyword
    // rules compare each occurrence against its containing document's own module, and the module
    // bookkeeping below reads the same map — one derivation, two consumers.
    let doc_module = module_by_document(index, &identities, &corpus, language);
    let join_result = join(index, &corpus, &identities, language, &doc_module);

    let source_map: HashMap<&str, &str> = sources.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let content = content_hash(sources);

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
    store.clear_derived()?;

    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        let display_name = sym.terminal_name().unwrap_or_default().to_string();
        let class = match sym.class {
            SymbolClass::External => PersistedClass::External,
            _ => PersistedClass::InWorkspace,
        };
        let duplicated = sym.definition().is_some()
            && sym
                .descriptor
                .as_ref()
                .is_some_and(|d| definition_count_by_descriptor.get(d).copied().unwrap_or(0) > 1);
        let (document_path, span, span_text) = definition_span(sym, id, &def_name_span, &source_map, language);
        store.insert_symbol(&SymbolRow {
            canonical_id: id.clone(),
            display_name,
            kind: kind_tag(sym.kind).to_string(),
            class,
            document_path,
            span,
            span_text,
            duplicated,
        })?;
    }

    // Persist aligned occurrences, attributing references to the nearest enclosing persisted symbol.
    for aligned in &join_result.aligned {
        let role = match aligned.role {
            OccurrenceRole::Definition => "definition",
            OccurrenceRole::Reference => "reference",
        };
        let enclosing_id = if aligned.role == OccurrenceRole::Reference {
            let source = source_map
                .get(aligned.document_path.as_str())
                .copied()
                .unwrap_or_default();
            enclosing_symbol(
                &aligned.enclosing,
                &aligned.document_path,
                source,
                &def_name_span,
                &type_by_name,
            )
        } else {
            None
        };
        store.insert_occurrence(&OccurrenceRow {
            symbol_id: aligned.symbol.clone(),
            document_path: aligned.document_path.clone(),
            span: (aligned.name_span.start, aligned.name_span.end),
            role: role.to_string(),
            rule: aligned.rule.tag().to_string(),
            enclosing_id,
            locality: aligned.locality.map(|l| l.tag().to_string()),
        })?;
    }

    // Derive `contains` edges from enclosure: each definition's nearest enclosing persisted
    // declaration contains it.
    for aligned in &join_result.aligned {
        if aligned.role != OccurrenceRole::Definition {
            continue;
        }
        // An empty aligned span (the Python module origin marker) has a position but no extent;
        // reading enclosure from it would fabricate containment — a document whose first byte sits
        // inside a declaration would make that declaration "contain" the module — so it derives no
        // parent.
        if aligned.name_span.start == aligned.name_span.end {
            continue;
        }
        if let Some(parent) = parent_of_definition(aligned, &source_map, &def_name_span, &type_by_name, language) {
            store.insert_edge(EdgeKind::Contains, &parent, &aligned.symbol)?;
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
        let source = source_map
            .get(aligned.document_path.as_str())
            .copied()
            .unwrap_or_default();
        match enclosing_symbol(
            &aligned.enclosing,
            &aligned.document_path,
            source,
            &def_name_span,
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
    for (path, source) in &source_map {
        let Some(tree) = syntax::SyntaxTree::parse(source, language) else {
            continue;
        };
        match language {
            Language::Rust => {
                for imp in tree.trait_impls() {
                    let ty = occ_by_location.get(&(*path, imp.type_name_span));
                    let tr = occ_by_location.get(&(*path, imp.trait_name_span));
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
                    let Some(sub) = occ_by_location.get(&(*path, class.class_name_span)) else {
                        continue;
                    };
                    for base_span in class.base_name_spans {
                        if let Some(base) = occ_by_location.get(&(*path, base_span)) {
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
        .map(|occ| DiscrepancyRow {
            document_path: occ.document_path.clone(),
            span: occ.span.map(|s| (s.start, s.end)),
            outcome: occ.outcome.tag().to_string(),
            expected_name: occ.expected_name.clone(),
            found_text: occ.found_text.clone(),
        })
        .collect();
    store.insert_discrepancies(&discrepancies)?;

    store.write_metadata(&IndexMetadata {
        workspace_id: workspace.clone(),
        provenance: index.provenance.clone(),
        content_hash: content,
        accounting: join_result.accounting,
        environment: index.environment.clone(),
    })?;

    // The single commit publishes the whole build atomically: a crash anywhere above rolls back to
    // the prior build wholesale, so fresh metadata can never coexist with another build's rows.
    tx.commit()?;

    Ok(join_result.accounting)
}

/// Refuse to join a fresh syntax tree against a stale index: verify the sources hash to the recorded
/// content hash before ingesting an update against an existing index.
pub fn join_guarded(
    store: &mut GraphStore,
    workspace: &WorkspaceId,
    index: &ExtractedIndex,
    sources: &[(String, String)],
    expected_hash: &str,
) -> Result<JoinAccounting, IngestError> {
    if content_hash(sources) != expected_hash {
        return Err(IngestError::ContentHashMismatch);
    }
    ingest(store, workspace, index, sources)
}

/// The definition span (document, byte span, exact text) for a symbol, from its aligned definition
/// name-span expanded to the enclosing declaration's full span.
fn definition_span(
    sym: &ExtractedSymbol,
    id: &CanonicalId,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    sources: &HashMap<&str, &str>,
    language: Language,
) -> (Option<String>, Option<(usize, usize)>, Option<String>) {
    if sym.class == SymbolClass::External {
        return (None, None, None);
    }
    let Some((doc, name_span)) = def_name_span.get(id) else {
        return (None, None, None);
    };
    let Some(source) = sources.get(doc.as_str()) else {
        return (Some(doc.clone()), None, None);
    };
    if let Some(tree) = syntax::SyntaxTree::parse(source, language) {
        for decl in tree.all_declarations() {
            if decl.name_span == *name_span {
                let text = source.get(decl.full_span.start..decl.full_span.end).map(str::to_string);
                return (
                    Some(doc.clone()),
                    Some((decl.full_span.start, decl.full_span.end)),
                    text,
                );
            }
        }
    }
    let text = source.get(name_span.start..name_span.end).map(str::to_string);
    (Some(doc.clone()), Some((name_span.start, name_span.end)), text)
}

/// Map an enclosing syntax-declaration chain to the identity of the nearest persisted declaration.
///
/// The chain is innermost-first. A declaration attributes to a persisted symbol when its name-span
/// matches that symbol's definition name-span. An `impl_item` is not itself a persisted symbol — it
/// associates its members with the type it implements — so it resolves to the persisted type whose
/// terminal name equals the impl's type identifier, which is how a method attributes to its type.
/// An empty chain (or no match) attributes to the module — represented as `None`, which the store
/// reads as "the module/file itself".
fn enclosing_symbol(
    chain: &[syntax::SyntaxDeclaration],
    document_path: &str,
    source: &str,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    type_by_name: &HashMap<String, CanonicalId>,
) -> Option<CanonicalId> {
    for decl in chain {
        // A direct persisted declaration: name-span matches a definition.
        for (id, (doc, span)) in def_name_span {
            if doc == document_path && *span == decl.name_span {
                return Some(id.clone());
            }
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

/// The parent declaration of a definition: the nearest enclosing persisted declaration other than
/// the definition itself.
fn parent_of_definition(
    aligned: &AlignedOccurrence,
    sources: &HashMap<&str, &str>,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    type_by_name: &HashMap<String, CanonicalId>,
    language: Language,
) -> Option<CanonicalId> {
    let source = sources.get(aligned.document_path.as_str())?;
    let tree = syntax::SyntaxTree::parse(source, language)?;
    let chain = tree.enclosing_declarations(aligned.name_span.start);
    let parent_chain: Vec<syntax::SyntaxDeclaration> =
        chain.into_iter().filter(|d| d.name_span != aligned.name_span).collect();
    enclosing_symbol(
        &parent_chain,
        &aligned.document_path,
        source,
        def_name_span,
        type_by_name,
    )
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
