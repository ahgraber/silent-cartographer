//! The code graph: the guarded join, the SQLite-core store, and the ingest that unifies the two
//! oracles into one persisted graph.

pub mod clone;
pub mod corpus;
pub mod embed;
pub mod join;
pub mod range;
pub mod rank;
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

    // The per-symbol test classification, computed from the same syntax pass and identity maps the
    // build already holds. Read-only over the join's outputs: it alters no alignment, attribution,
    // or edge derivation.
    let test_rules = classify_test_symbols(
        index,
        &identities,
        language,
        &source_map,
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
    // The prior build's semantic representations, read before the clear so an entry whose render is
    // unchanged carries its embedding forward instead of re-embedding.
    let prior_semantic = store.semantic_representations()?;
    store.clear_derived()?;

    // The rows are retained after insertion: the semantic-corpus pass below reads their tier
    // content and containment to assemble the corpus.
    let mut symbol_rows: Vec<SymbolRow> = Vec::new();
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
        let content = definition_content(sym, id, &def_name_span, &source_map, language);
        symbol_rows.push(SymbolRow {
            canonical_id: id.clone(),
            display_name,
            kind: kind_tag(sym.kind).to_string(),
            class,
            document_path: content.document_path,
            span: content.span,
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
    // Which symbols contribute corpus entries, decided from tier content alone. Containment for
    // corpus purposes counts contributing children only — a symbol enclosing nothing but name-only
    // symbols (a Python function over its parameter symbols) stays a leaf — so both containment
    // signals below filter through this set.
    let contributing: std::collections::HashSet<&CanonicalId> = symbol_rows
        .iter()
        .filter(|row| row.class == PersistedClass::InWorkspace)
        .filter(|row| {
            corpus::content_contributes(
                &row.display_name,
                row.signature_text.as_deref(),
                row.interface_text.as_deref(),
                row.span_text.as_deref(),
            )
        })
        .map(|row| &row.canonical_id)
        .collect();

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
            && source_map
                .get(aligned.document_path.as_str())
                .is_some_and(|source| aligned.name_span.end == source.len())
        {
            continue;
        }
        if let Some(parent) = parent_of_definition(aligned, &source_map, &def_name_span, &type_by_name, language) {
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

    // The semantic corpus and its representations. Containment for corpus purposes is the union of
    // two signals: the `contains`-edge parents (which catch a type whose methods live in `impl`
    // blocks outside its own span) and span containment within a document (which catches a file
    // module, whose members' enclosing-declaration chains are empty so no edge ever names it).
    let mut span_containers: std::collections::HashSet<&CanonicalId> = std::collections::HashSet::new();
    {
        /// One contributing symbol's identity, definition span, and whether it is a module,
        /// grouped per document below. Only contributing symbols enter: containment over
        /// non-contributing symbols (name-only parameter tokens) must not turn their encloser into
        /// a container.
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
                // Containment compares identities, not spans: a file module whose span exactly
                // equals its sole declaration's span still encloses it. Equal spans confer
                // containment only on the module side of the pair (a document encloses its
                // declarations), so the declaration stays a leaf.
                let contains_other = spans.iter().any(|(other, inner, _)| {
                    *other != *id && outer.0 <= inner.0 && inner.1 <= outer.1 && (*inner != *outer || *is_module)
                });
                if contains_other {
                    span_containers.insert(id);
                }
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
            signature_text: row.signature_text.as_deref(),
            interface_text: row.interface_text.as_deref(),
            span_text: row.span_text.as_deref(),
            contains_persisted: container_ids.contains(&row.canonical_id)
                || span_containers.contains(&row.canonical_id),
        })
        .collect();
    let entries = corpus::assemble(&corpus_sources);

    // Each entry's vector: carried forward when the render is byte-identical to the prior build's
    // (the model is deterministic and pinned, so the carried and recomputed vectors are identical
    // by construction), embedded in one batch otherwise.
    let mut vectors: Vec<Option<Vec<u8>>> = vec![None; entries.len()];
    let mut pending: Vec<usize> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match prior_semantic.get(entry.symbol_id.as_str()) {
            Some(prior) if prior.render == entry.render => vectors[i] = Some(prior.embedding.clone()),
            _ => pending.push(i),
        }
    }
    if !pending.is_empty() {
        let texts: Vec<String> = pending.iter().map(|&i| entries[i].render.clone()).collect();
        for (&i, vector) in pending.iter().zip(embed::embed_batch(&texts)) {
            vectors[i] = Some(embed::vector_bytes(&vector));
        }
    }
    for (entry, vector) in entries.iter().zip(&vectors) {
        let vector = vector.as_ref().expect("every corpus entry embeds or carries forward");
        let words = corpus::split_words(&entry.render).join(" ");
        store.insert_corpus_entry(&entry.symbol_id, &entry.render, &words, vector)?;
    }

    // Clone-equivalence keys for the corpus's leaves, from each leaf's spelled token sequence.
    // Containers carry none: a container "clone" over interface text would assert a body
    // equivalence the key never examined.
    let mut trees: HashMap<&str, Option<syntax::SyntaxTree>> = HashMap::new();
    for source in &corpus_sources {
        if source.contains_persisted || !source.contributes() {
            continue;
        }
        let row = symbol_rows
            .iter()
            .find(|row| &row.canonical_id == source.canonical_id)
            .expect("corpus sources are drawn from the retained rows");
        let (Some(doc), Some((start, end))) = (row.document_path.as_deref(), row.span) else {
            continue;
        };
        let tree = trees.entry(doc).or_insert_with(|| {
            source_map
                .get(doc)
                .copied()
                .and_then(|s| syntax::SyntaxTree::parse(s, language))
        });
        let Some(tree) = tree.as_ref() else {
            continue;
        };
        let tokens = tree.spelled_tokens(ByteSpan { start, end });
        if let Some(keys) = clone::clone_keys(&tokens) {
            store.set_clone_keys(source.canonical_id, &keys.formatting_key, &keys.substitution_key)?;
        }
    }

    store.write_metadata(&IndexMetadata {
        workspace_id: workspace.clone(),
        workspace_root: workspace_root.map(str::to_string),
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

/// The definition content persisted for a symbol: its document, byte span, and the body/signature/
/// interface tier text. All fields default to `None` (no document, no persisted content).
#[derive(Default)]
struct DefinitionContent {
    document_path: Option<String>,
    span: Option<(usize, usize)>,
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
fn definition_content(
    sym: &ExtractedSymbol,
    id: &CanonicalId,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    sources: &HashMap<&str, &str>,
    language: Language,
) -> DefinitionContent {
    if sym.class == SymbolClass::External {
        return DefinitionContent::default();
    }
    let Some((doc, name_span)) = def_name_span.get(id) else {
        return DefinitionContent::default();
    };
    let Some(source) = sources.get(doc.as_str()) else {
        return DefinitionContent {
            document_path: Some(doc.clone()),
            ..Default::default()
        };
    };
    let tree = syntax::SyntaxTree::parse(source, language);
    let matched = tree.as_ref().and_then(|tree| {
        tree.all_declarations()
            .into_iter()
            .find(|decl| decl.name_span == *name_span)
    });

    if let Some(decl) = matched {
        let tree = tree.as_ref().expect("a matched declaration implies a parsed tree");
        let text = source.get(decl.full_span.start..decl.full_span.end).map(str::to_string);
        let tiers = tree.declaration_tiers(&decl);
        return DefinitionContent {
            document_path: Some(doc.clone()),
            span: Some((decl.full_span.start, decl.full_span.end)),
            span_text: text,
            signature_text: Some(tiers.signature),
            interface_text: Some(tiers.interface),
        };
    }

    if sym.kind == SymbolKind::Module {
        let qualified = qualified_name(id);
        let interface = match tree.as_ref().and_then(|tree| tree.module_documentation()) {
            Some(docs) => format!("{qualified}\n{}", docs.trim_end()),
            None => qualified.to_string(),
        };
        return DefinitionContent {
            document_path: Some(doc.clone()),
            span: Some((0, source.len())),
            span_text: Some(source.to_string()),
            signature_text: Some(qualified.to_string()),
            interface_text: Some(interface),
        };
    }

    let text = source.get(name_span.start..name_span.end).map(str::to_string);
    DefinitionContent {
        document_path: Some(doc.clone()),
        span: Some((name_span.start, name_span.end)),
        span_text: text.clone(),
        signature_text: text.clone(),
        interface_text: text,
    }
}

/// The qualified-name portion of a canonical identity: the identity with its leading workspace
/// segment stripped (`<workspace>::<qualified>` → `<qualified>`). A trailing `#<rank>`
/// disambiguator, when the identity carries one, stays — it is part of the identity, not decoration.
fn qualified_name(id: &CanonicalId) -> &str {
    id.as_str()
        .split_once("::")
        .map(|(_, rest)| rest)
        .unwrap_or(id.as_str())
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
    source_map: &HashMap<&str, &str>,
    def_name_span: &HashMap<CanonicalId, (String, ByteSpan)>,
    module_by_doc: &HashMap<String, CanonicalId>,
    aligned: &[AlignedOccurrence],
) -> HashMap<CanonicalId, &'static str> {
    // Per-document convention signals, read once per document from its syntax tree: the name spans
    // of test-attributed declarations, the spans of inline `#[cfg(test)]` module bodies, the gated
    // out-of-line module declarations, and every out-of-line module declaration (through which
    // gating propagates into other documents).
    let mut attr_names: HashMap<String, std::collections::HashSet<ByteSpan>> = HashMap::new();
    let mut inline_gated: HashMap<String, Vec<ByteSpan>> = HashMap::new();
    let mut gated_mod_decls: Vec<(String, ByteSpan)> = Vec::new();
    let mut out_of_line_mods: HashMap<String, Vec<ByteSpan>> = HashMap::new();
    if language == Language::Rust {
        for (path, source) in source_map {
            let Some(tree) = syntax::SyntaxTree::parse(source, language) else {
                continue;
            };
            attr_names.insert(
                (*path).to_string(),
                tree.test_attributed_declaration_names().into_iter().collect(),
            );
            for gated in tree.cfg_test_modules() {
                match gated.inline_span {
                    Some(span) => inline_gated.entry((*path).to_string()).or_default().push(span),
                    None => gated_mod_decls.push(((*path).to_string(), gated.name_span)),
                }
            }
            out_of_line_mods.insert((*path).to_string(), tree.out_of_line_module_names());
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
