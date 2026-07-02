//! The guarded positional join between the semantic index and the syntax tree.
//!
//! For each semantic occurrence, the join normalizes its range onto byte offsets, matches by range
//! containment to the syntactic name node, and asserts the source bytes at the matched range equal
//! the symbol's expected name token — the terminal segment of its descriptor, since a SCIP range
//! covers the name token, not the qualified path. A mismatch is surfaced and not persisted as an
//! aligned attribution.
//!
//! Every occurrence lands in exactly one of three semantic outcomes — aligned, text-mismatch, or
//! semantic-only — and the join separately counts syntax-only constructs the backend did not
//! resolve. The three semantic counts sum to the total occurrences processed.

use std::collections::HashMap;

use crate::identity::CanonicalId;
use crate::semantic::model::{ExtractedIndex, OccurrenceRole};

use super::range::{ByteSpan, LineIndex, range_to_span};
use super::syntax::{SyntaxDeclaration, SyntaxTree};

/// The outcome of joining one semantic occurrence against the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinOutcome {
    /// The occurrence's location holds syntax naming the same symbol (guard passed).
    Aligned,
    /// The location holds a name node whose text is not the expected name token (guard failed).
    TextMismatch,
    /// No syntactic construct exists at the location (e.g. macro expansion, coordinate drift).
    SemanticOnly,
}

/// An occurrence that aligned: it is attributed to a syntactic construct and, for a reference, to
/// its nearest enclosing persisted declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedOccurrence {
    /// The canonical identity of the symbol the occurrence belongs to.
    pub symbol: CanonicalId,
    /// The document the occurrence sits in.
    pub document_path: String,
    /// The byte span of the matched name node.
    pub name_span: ByteSpan,
    /// The occurrence role.
    pub role: OccurrenceRole,
    /// For a reference occurrence, the enclosing declaration chain (innermost first); empty means
    /// the enclosing construct is the module/file itself (the outermost attribution).
    pub enclosing: Vec<SyntaxDeclaration>,
}

/// An occurrence whose location did not reconcile with the syntax at that location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnalignedOccurrence {
    /// The canonical identity of the symbol the occurrence belongs to.
    pub symbol: CanonicalId,
    /// The document the occurrence sits in.
    pub document_path: String,
    /// The occurrence role.
    pub role: OccurrenceRole,
    /// Why it did not align.
    pub outcome: JoinOutcome,
    /// The expected name token (the descriptor's terminal segment).
    pub expected_name: String,
    /// The source bytes actually found at the location, if any syntax was there.
    pub found_text: Option<String>,
}

/// Per-build counts of join outcomes.
///
/// The three semantic-side counts (`aligned + text_mismatch + semantic_only`) equal the total
/// semantic occurrences processed; `syntax_only` counts unresolved syntactic constructs separately.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JoinAccounting {
    /// Occurrences persisted as aligned attributions.
    pub aligned: u64,
    /// Occurrences whose location did not spell the expected name.
    pub text_mismatch: u64,
    /// Occurrences with no syntactic construct at their location.
    pub semantic_only: u64,
    /// Syntactic declarations the backend did not resolve to any symbol.
    pub syntax_only: u64,
}

impl JoinAccounting {
    /// The total semantic occurrences processed (the three semantic-side outcomes).
    pub fn total_semantic(&self) -> u64 {
        self.aligned + self.text_mismatch + self.semantic_only
    }
}

/// The result of joining an index against its source documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinResult {
    /// Occurrences that aligned.
    pub aligned: Vec<AlignedOccurrence>,
    /// Occurrences that did not align (surfaced discrepancies).
    pub unaligned: Vec<UnalignedOccurrence>,
    /// Per-build outcome counts.
    pub accounting: JoinAccounting,
}

/// The inputs a join needs beyond the semantic index: the source text of each referenced document.
pub struct SourceCorpus<'a> {
    texts: HashMap<&'a str, &'a str>,
}

impl<'a> SourceCorpus<'a> {
    /// Build a corpus from `(document_path, source_text)` pairs.
    pub fn new(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self {
            texts: entries.into_iter().collect(),
        }
    }

    fn get(&self, path: &str) -> Option<&'a str> {
        self.texts.get(path).copied()
    }
}

/// A parsed document: its syntax tree, line index, and source, keyed for reuse across occurrences.
struct PreparedDocument {
    tree: SyntaxTree,
    line_index: LineIndex,
}

/// Run the guarded positional join over `index` and its `corpus`, resolving each occurrence's
/// symbol identity through `identities` (the canonical identity per index-symbol position).
///
/// `identities[i]` is the canonical identity of `index.symbols[i]`. Symbols without an identity
/// (e.g. those the caller chose not to persist) are skipped.
pub fn join(index: &ExtractedIndex, corpus: &SourceCorpus, identities: &[Option<CanonicalId>]) -> JoinResult {
    // Parse each referenced document once.
    let mut prepared: HashMap<String, PreparedDocument> = HashMap::new();
    for doc in &index.documents {
        if let Some(source) = corpus.get(&doc.path)
            && let Some(tree) = SyntaxTree::parse(source)
        {
            prepared.insert(
                doc.path.clone(),
                PreparedDocument {
                    tree,
                    line_index: LineIndex::new(source),
                },
            );
        }
    }

    let mut aligned = Vec::new();
    let mut unaligned = Vec::new();
    let mut accounting = JoinAccounting::default();

    // Track name spans the join aligned to, per document, to compute syntax-only declarations.
    let mut aligned_name_spans: HashMap<String, Vec<ByteSpan>> = HashMap::new();

    for (sym_idx, symbol) in index.symbols.iter().enumerate() {
        let Some(Some(identity)) = identities.get(sym_idx) else {
            continue;
        };
        let expected_name = match symbol.terminal_name() {
            Some(n) => n.to_string(),
            None => continue,
        };

        for occ in &symbol.occurrences {
            let Some(doc) = prepared.get(&occ.document_path) else {
                // No source for the document: cannot reconcile, count as semantic-only.
                accounting.semantic_only += 1;
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::SemanticOnly,
                    expected_name: expected_name.clone(),
                    found_text: None,
                });
                continue;
            };
            let encoding = index
                .encoding_for(&occ.document_path)
                .expect("document has an encoding");
            let source = doc.tree.source();

            let Some(span) = range_to_span(source, &doc.line_index, occ.range, encoding) else {
                accounting.semantic_only += 1;
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::SemanticOnly,
                    expected_name: expected_name.clone(),
                    found_text: None,
                });
                continue;
            };

            match doc.tree.name_node_containing(span) {
                Some(name_span) => {
                    let found = doc.tree.text_at(name_span).unwrap_or_default();
                    if found == expected_name {
                        accounting.aligned += 1;
                        let enclosing = if occ.role == OccurrenceRole::Reference {
                            doc.tree.enclosing_declarations(span.start)
                        } else {
                            Vec::new()
                        };
                        aligned_name_spans
                            .entry(occ.document_path.clone())
                            .or_default()
                            .push(name_span);
                        aligned.push(AlignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            name_span,
                            role: occ.role,
                            enclosing,
                        });
                    } else {
                        accounting.text_mismatch += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::TextMismatch,
                            expected_name: expected_name.clone(),
                            found_text: Some(found.to_string()),
                        });
                    }
                }
                None => {
                    // No identifier node contains the span. If no construct at all exists there, it
                    // is a semantic-only occurrence; otherwise the location holds non-identifier
                    // syntax that cannot name the symbol — also a mismatch surfaced, not aligned.
                    if doc.tree.has_construct_at(span) {
                        accounting.text_mismatch += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::TextMismatch,
                            expected_name: expected_name.clone(),
                            found_text: doc.tree.text_at(span).map(str::to_string),
                        });
                    } else {
                        accounting.semantic_only += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::SemanticOnly,
                            expected_name: expected_name.clone(),
                            found_text: None,
                        });
                    }
                }
            }
        }
    }

    // Syntax-only: declarations in a parsed document whose name node no aligned occurrence matched.
    for (path, doc) in &prepared {
        let matched = aligned_name_spans.get(path).cloned().unwrap_or_default();
        for decl in doc.tree.all_declarations() {
            if !matched.contains(&decl.name_span) {
                accounting.syntax_only += 1;
            }
        }
    }

    JoinResult {
        aligned,
        unaligned,
        accounting,
    }
}
