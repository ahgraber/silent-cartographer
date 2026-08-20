//! One parse per document: the build-scoped prepared corpus every syntax-reading derivation borrows.
//!
//! [`PreparedCorpus::prepare`] parses every document of a build's source corpus exactly once, before
//! the join runs, and every derivation reads syntax through the resulting map. Nothing parses
//! outside `prepare`, so a document that fails to parse has no entry and no derivation can retry
//! it — the at-most-once parse attempt is structural rather than guarded.

use std::collections::HashMap;

use super::join::SourceCorpus;
use super::range::{ByteSpan, LineIndex};
use super::syntax::{AliasBinding, Language, SyntaxDeclaration, SyntaxTree};

/// A parsed document: its syntax tree, line index, declared alias bindings, and its declaration
/// list indexed by name span — everything the build's derivations read about the document, prepared
/// once.
///
/// The tree owns the document's source text, so consumers read source through
/// [`SyntaxTree::source`] rather than a separate source map.
pub struct PreparedDocument {
    /// The document's syntax tree, owning the source text.
    pub tree: SyntaxTree,
    /// The line index over the same source.
    pub line_index: LineIndex,
    /// The document's declared alias bindings.
    pub alias_bindings: Vec<AliasBinding>,
    /// Every declaration in the document, in tree-walk order.
    declarations: Vec<SyntaxDeclaration>,
    /// Position in `declarations` of the declaration at each name span. First in tree-walk order
    /// wins — the same declaration a linear search over the list would find.
    by_name_span: HashMap<ByteSpan, usize>,
}

impl PreparedDocument {
    /// Every declaration in the document, in tree-walk order — the whole-tree declaration walk,
    /// performed once during preparation rather than once per consumer.
    pub fn declarations(&self) -> &[SyntaxDeclaration] {
        &self.declarations
    }

    /// The declaration whose name span is exactly `name_span`, if the document declares one.
    pub fn declaration_at(&self, name_span: ByteSpan) -> Option<&SyntaxDeclaration> {
        self.by_name_span.get(&name_span).map(|&at| &self.declarations[at])
    }
}

/// The build-scoped map from document path to its prepared document: one entry per parseable
/// document, built once from the source corpus and borrowed by every derivation.
pub struct PreparedCorpus {
    docs: HashMap<String, PreparedDocument>,
}

impl PreparedCorpus {
    /// Parse every document of `corpus` once, as `language`.
    ///
    /// A document that fails to parse has no entry: absence means "attempted and unavailable".
    pub fn prepare(corpus: &SourceCorpus, language: Language) -> Self {
        let mut docs = HashMap::new();
        for (path, source) in corpus.entries() {
            let Some(tree) = SyntaxTree::parse(source, language) else {
                continue;
            };
            let alias_bindings = tree.alias_bindings();
            let declarations = tree.all_declarations();
            let mut by_name_span = HashMap::new();
            for (at, decl) in declarations.iter().enumerate() {
                by_name_span.entry(decl.name_span).or_insert(at);
            }
            docs.insert(
                path.to_string(),
                PreparedDocument {
                    line_index: LineIndex::new(source),
                    alias_bindings,
                    declarations,
                    by_name_span,
                    tree,
                },
            );
        }
        Self { docs }
    }

    /// The prepared document at `path`, if it parsed.
    pub fn get(&self, path: &str) -> Option<&PreparedDocument> {
        self.docs.get(path)
    }

    /// Iterate every prepared document as `(path, document)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PreparedDocument)> {
        self.docs.iter().map(|(path, doc)| (path.as_str(), doc))
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::ingest;
    use crate::graph::store::GraphStore;
    use crate::graph::syntax;
    use crate::graph::work_bound_fixture::{fn_symbol, index_over};
    use crate::identity::WorkspaceId;
    use crate::semantic::model::ExtractedIndex;

    /// The number of syntax parses one full ingest of `index` over `sources` performs.
    fn parses_during_ingest(index: &ExtractedIndex, sources: &[(String, String)]) -> usize {
        let mut store = GraphStore::open_in_memory().unwrap();
        syntax::reset_parse_count();
        ingest(&mut store, &WorkspaceId::new("bound-ws"), None, index, sources).unwrap();
        syntax::parse_count()
    }

    // _(Deriving the graph costs one pass over each document)_ — a symbol-dense document and a
    // sparse document of equal size cost the same parse work: one parse each.
    #[test]
    fn dense_and_sparse_documents_cost_the_same_parse_work() {
        let dense_source = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";
        let sparse_source = "fn a() {}\n// abcdefghijklmnopqrstuvwxyz\n";
        assert_eq!(dense_source.len(), sparse_source.len(), "test setup: equal sizes");

        let dense = parses_during_ingest(
            &index_over(&[(
                "dense.rs",
                vec![
                    fn_symbol("a", "dense.rs", 0),
                    fn_symbol("b", "dense.rs", 1),
                    fn_symbol("c", "dense.rs", 2),
                    fn_symbol("d", "dense.rs", 3),
                ],
            )]),
            &[("dense.rs".to_string(), dense_source.to_string())],
        );
        let sparse = parses_during_ingest(
            &index_over(&[("sparse.rs", vec![fn_symbol("a", "sparse.rs", 0)])]),
            &[("sparse.rs".to_string(), sparse_source.to_string())],
        );
        assert_eq!(dense, sparse, "parse work is per document, not per symbol");
        assert_eq!(dense, 1, "one document costs one parse");
    }

    // _(Deriving the graph costs one pass over each document)_ — adding declarations to a document
    // does not change the parse work performed for it.
    #[test]
    fn adding_declarations_does_not_add_parse_work() {
        let before = parses_during_ingest(
            &index_over(&[("one.rs", vec![fn_symbol("a", "one.rs", 0)])]),
            &[("one.rs".to_string(), "fn a() {}\n".to_string())],
        );
        let after = parses_during_ingest(
            &index_over(&[(
                "one.rs",
                vec![
                    fn_symbol("a", "one.rs", 0),
                    fn_symbol("b", "one.rs", 1),
                    fn_symbol("c", "one.rs", 2),
                ],
            )]),
            &[("one.rs".to_string(), "fn a() {}\nfn b() {}\nfn c() {}\n".to_string())],
        );
        assert_eq!(before, after, "new declarations cost no additional parses");
    }

    // _(Deriving the graph costs one pass over each document)_ — parse work over a multi-document
    // workspace is proportional to its documents.
    #[test]
    fn parse_work_is_proportional_to_documents() {
        let docs: Vec<(String, String)> = (0..3)
            .map(|i| (format!("doc{i}.rs"), "fn a() {}\n".to_string()))
            .collect();
        let index = index_over(
            &docs
                .iter()
                .map(|(path, _)| (path.as_str(), vec![fn_symbol("a", path, 0)]))
                .collect::<Vec<_>>(),
        );
        let parses = parses_during_ingest(&index, &docs);
        assert_eq!(parses, docs.len(), "one parse per document, nothing more");
    }
}
