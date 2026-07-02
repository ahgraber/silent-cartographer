//! The code graph: the guarded join, the SQLite-core store, and the ingest that unifies the two
//! oracles into one persisted graph.

pub mod join;
pub mod range;
pub mod schema;
pub mod store;
pub mod syntax;

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::identity::{CanonicalId, Descriptor, WorkspaceId, project_all};
use crate::semantic::model::{ExtractedIndex, ExtractedSymbol, OccurrenceRole, SymbolClass, SymbolKind};

use join::{AlignedOccurrence, JoinAccounting, SourceCorpus, join};
use range::ByteSpan;
use store::{EdgeKind, Freshness, GraphStore, IndexMetadata, OccurrenceRow, PersistedClass, SymbolRow};

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
    // Gather the descriptors of persisted symbols, remembering their positions.
    let mut descriptors: Vec<Descriptor> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        if sym.class == SymbolClass::Local {
            continue;
        }
        if let Some(d) = &sym.descriptor {
            descriptors.push(d.clone());
            positions.push(idx);
        }
    }
    let ids = project_all(workspace, &descriptors);
    let mut out = vec![None; index.symbols.len()];
    for (slot, id) in positions.into_iter().zip(ids) {
        out[slot] = Some(id);
    }
    out
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
    let corpus = SourceCorpus::new(sources.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    let join_result = join(index, &corpus, &identities);

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
    // The module symbol defined in each document, so a module-scope reference has a real importer.
    let mut module_by_doc: HashMap<String, CanonicalId> = HashMap::new();
    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        if matches!(sym.kind, SymbolKind::Type | SymbolKind::Trait)
            && let Some(name) = sym.terminal_name()
        {
            type_by_name.entry(name.to_string()).or_insert_with(|| id.clone());
        }
        if sym.kind == SymbolKind::Module
            && let Some((doc, _)) = def_name_span.get(id)
        {
            module_by_doc.entry(doc.clone()).or_insert_with(|| id.clone());
        }
    }

    for (idx, sym) in index.symbols.iter().enumerate() {
        let Some(Some(id)) = identities.get(idx) else {
            continue;
        };
        let display_name = sym.terminal_name().unwrap_or_default().to_string();
        let class = match sym.class {
            SymbolClass::External => PersistedClass::External,
            _ => PersistedClass::InWorkspace,
        };
        let (document_path, span, span_text) = definition_span(sym, id, &def_name_span, &source_map);
        store.insert_symbol(&SymbolRow {
            canonical_id: id.clone(),
            display_name,
            kind: kind_tag(sym.kind).to_string(),
            class,
            document_path,
            span,
            span_text,
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
            enclosing_id,
        })?;
    }

    // Derive `contains` edges from enclosure: each definition's nearest enclosing persisted
    // declaration contains it.
    for aligned in &join_result.aligned {
        if aligned.role != OccurrenceRole::Definition {
            continue;
        }
        if let Some(parent) = parent_of_definition(aligned, &source_map, &def_name_span, &type_by_name) {
            store.insert_edge(EdgeKind::Contains, &parent, &aligned.symbol)?;
        }
    }

    // Populate the uncontracted dependency edges (unverified until proposal 2): a reference from an
    // enclosing declaration to the referenced symbol is a `calls` candidate.
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
            // A reference from inside a declaration is a call candidate.
            Some(caller) => store.insert_edge(EdgeKind::Calls, &caller, &aligned.symbol)?,
            // A module-scope reference (no narrower enclosing declaration) is an import candidate,
            // from the enclosing module to the referenced symbol, when the module is persisted.
            None => {
                if let Some(module) = module_by_doc.get(&aligned.document_path) {
                    store.insert_edge(EdgeKind::Imports, module, &aligned.symbol)?;
                }
            }
        }
    }

    // Populate `type_hierarchy` edges (unverified until proposal 2) from `impl Trait for Type`
    // blocks: an edge from the implementing type to the implemented trait.
    for source in source_map.values() {
        if let Some(tree) = syntax::SyntaxTree::parse(source) {
            for (type_name, trait_name) in tree.trait_impls() {
                if let (Some(ty), Some(tr)) = (type_by_name.get(&type_name), type_by_name.get(&trait_name)) {
                    store.insert_edge(EdgeKind::TypeHierarchy, ty, tr)?;
                }
            }
        }
    }

    store.write_metadata(&IndexMetadata {
        workspace_id: workspace.clone(),
        provenance: index.provenance.clone(),
        content_hash: content,
        accounting: join_result.accounting,
    })?;

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
    if let Some(tree) = syntax::SyntaxTree::parse(source) {
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
) -> Option<CanonicalId> {
    let source = sources.get(aligned.document_path.as_str())?;
    let tree = syntax::SyntaxTree::parse(source)?;
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

/// Evaluate freshness of the store against the current sources and analyzer.
pub fn freshness(
    store: &GraphStore,
    sources: &[(String, String)],
    current: &crate::semantic::model::AnalyzerProvenance,
) -> rusqlite::Result<Option<Freshness>> {
    let hash = content_hash(sources);
    store.freshness(&hash, current)
}
