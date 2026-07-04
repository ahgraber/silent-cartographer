//! The guarded positional join between the semantic index and the syntax tree.
//!
//! For each semantic occurrence, the join normalizes its range onto byte offsets and dispatches it
//! to a **named alignment rule**: an attribution persists as aligned if and only if one rule's exact
//! expectation is satisfied, and the accepting rule is carried on the attribution as provenance.
//! The default rule is name-token equality — the source bytes at the matched name node equal the
//! symbol's expected name token, the terminal segment of its descriptor, since a SCIP range covers
//! the name token, not the qualified path. Three kind-scoped rules extend it: crate-root (a crate
//! root's token is the package name or the `crate` keyword), operator-desugar (a closed
//! method-to-construct correspondence, matched on the syntax-tree construct rather than raw bytes),
//! and module-span (a module definition spanning its whole document). An occurrence satisfying no
//! rule is surfaced and never persisted as a confident attribution.
//!
//! Every occurrence lands in exactly one per-rule acceptance bucket or one refusal outcome —
//! text-mismatch, semantic-only, or duplicate-ambiguous — and the join separately counts syntax-only
//! constructs the backend did not resolve. Acceptances plus refusals sum to the total occurrences
//! processed.
//!
//! Duplicate-ambiguous is the calibration guard against the semantic backend's true-duplicate defect:
//! when more than one distinct definition shares an identical resolved descriptor, the backend cannot
//! say which twin a reference means. Each definition occurrence still attaches to the definition at
//! its own location (co-location), but every non-definition occurrence of such a descriptor is typed
//! duplicate-ambiguous and attributed to no twin, rather than silently misassigned.

use std::collections::{HashMap, HashSet};

use crate::identity::CanonicalId;
use crate::semantic::model::{ExtractedIndex, ExtractedSymbol, OccurrenceRole, SymbolKind};

use super::range::{ByteSpan, LineIndex, range_to_span};
use super::syntax::{ConstructAt, SyntaxDeclaration, SyntaxTree};

/// The named alignment rule that accepted an attribution. Stored as provenance on every aligned
/// occurrence; each rule also carries its own acceptance bucket in the accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentRule {
    /// The default rule: the source text at the matched name node equals the expected name token.
    Exact,
    /// A crate-root occurrence whose source token is the descriptor's package name or `crate`.
    CrateRoot,
    /// A reference to a desugared-operator method, located at the operator construct it desugars
    /// from, per the closed correspondence.
    OperatorDesugar,
    /// A module definition whose range spans the module's whole document.
    ModuleSpan,
    /// A reference resolving to a type — or to an implementation of one — at a `Self` keyword
    /// token whose nearest enclosing impl's self type shares the expected base name (generic
    /// arguments stripped from both).
    SelfKeyword,
}

impl AlignmentRule {
    /// The stored tag for this rule.
    pub fn tag(&self) -> &'static str {
        match self {
            AlignmentRule::Exact => "exact",
            AlignmentRule::CrateRoot => "crate_root",
            AlignmentRule::OperatorDesugar => "operator_desugar",
            AlignmentRule::ModuleSpan => "module_span",
            AlignmentRule::SelfKeyword => "self_keyword",
        }
    }
}

/// The outcome of joining one semantic occurrence against the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinOutcome {
    /// The occurrence's location holds syntax naming the same symbol (guard passed).
    Aligned,
    /// The location holds a name node whose text is not the expected name token (guard failed).
    TextMismatch,
    /// No syntactic construct exists at the location (e.g. macro expansion, coordinate drift).
    SemanticOnly,
    /// A non-definition occurrence of a descriptor shared by more than one distinct definition: the
    /// backend cannot say which twin it means, so it is attributed to none.
    DuplicateAmbiguous,
}

impl JoinOutcome {
    /// The stored tag for this outcome, used in the discrepancy detail table.
    pub fn tag(&self) -> &'static str {
        match self {
            JoinOutcome::Aligned => "aligned",
            JoinOutcome::TextMismatch => "text_mismatch",
            JoinOutcome::SemanticOnly => "semantic_only",
            JoinOutcome::DuplicateAmbiguous => "duplicate_ambiguous",
        }
    }
}

/// An occurrence that aligned: it is attributed to a syntactic construct and, for a reference, to
/// its nearest enclosing persisted declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignedOccurrence {
    /// The canonical identity of the symbol the occurrence belongs to.
    pub symbol: CanonicalId,
    /// The document the occurrence sits in.
    pub document_path: String,
    /// The byte span of the matched construct: the name node for the exact and crate-root rules,
    /// the operator construct for the operator-desugar rule, the whole document for module-span.
    pub name_span: ByteSpan,
    /// The occurrence role.
    pub role: OccurrenceRole,
    /// The alignment rule that accepted this attribution (its provenance).
    pub rule: AlignmentRule,
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
    /// The byte span at the occurrence's location, when the coordinate normalized onto the source.
    /// Absent when the range could not be reconciled onto the document (e.g. coordinate drift).
    pub span: Option<ByteSpan>,
    /// The expected name token (the descriptor's terminal segment).
    pub expected_name: String,
    /// The source bytes actually found at the location, if any syntax was there.
    pub found_text: Option<String>,
}

/// Per-build counts of join outcomes: one acceptance bucket per alignment rule, plus the refusal
/// outcomes.
///
/// The per-rule acceptance counts and the refusal counts (`text_mismatch + semantic_only +
/// duplicate_ambiguous`) together equal the total semantic occurrences processed; `syntax_only`
/// counts unresolved syntactic constructs separately.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JoinAccounting {
    /// Occurrences accepted by the default name-token-equality rule.
    pub aligned_exact: u64,
    /// Occurrences accepted by the crate-root rule.
    pub aligned_crate_root: u64,
    /// Occurrences accepted by the operator-desugar rule.
    pub aligned_operator_desugar: u64,
    /// Occurrences accepted by the module-span rule.
    pub aligned_module_span: u64,
    /// Occurrences accepted by the self-keyword rule.
    pub aligned_self_keyword: u64,
    /// Occurrences whose location satisfied no alignment rule's expectation.
    pub text_mismatch: u64,
    /// Occurrences with no syntactic construct at their location.
    pub semantic_only: u64,
    /// Non-definition occurrences of a descriptor shared by more than one distinct definition.
    pub duplicate_ambiguous: u64,
    /// Syntactic declarations the backend did not resolve to any symbol.
    pub syntax_only: u64,
}

impl JoinAccounting {
    /// Count one acceptance under `rule`.
    fn accept(&mut self, rule: AlignmentRule) {
        match rule {
            AlignmentRule::Exact => self.aligned_exact += 1,
            AlignmentRule::CrateRoot => self.aligned_crate_root += 1,
            AlignmentRule::OperatorDesugar => self.aligned_operator_desugar += 1,
            AlignmentRule::ModuleSpan => self.aligned_module_span += 1,
            AlignmentRule::SelfKeyword => self.aligned_self_keyword += 1,
        }
    }

    /// The total occurrences accepted across all alignment rules.
    pub fn aligned_total(&self) -> u64 {
        self.aligned_exact
            + self.aligned_crate_root
            + self.aligned_operator_desugar
            + self.aligned_module_span
            + self.aligned_self_keyword
    }

    /// The total semantic occurrences processed (all acceptance buckets plus all refusal outcomes).
    pub fn total_semantic(&self) -> u64 {
        self.aligned_total() + self.text_mismatch + self.semantic_only + self.duplicate_ambiguous
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

    // Symbols whose resolved descriptor is shared by more than one distinct definition. A reference
    // to such a descriptor is genuinely unattributable between the twins, so it is typed
    // duplicate-ambiguous rather than assigned to one; each definition still aligns at its own site.
    let duplicated = duplicated_descriptor_symbols(index, identities);

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
        let is_duplicated = duplicated.contains(&sym_idx);

        for occ in &symbol.occurrences {
            // A non-definition occurrence of a duplicated descriptor is attributed to no twin. Its
            // location is still recorded (normalized when the source is available) so the ambiguity
            // is inspectable.
            if is_duplicated && occ.role != OccurrenceRole::Definition {
                accounting.duplicate_ambiguous += 1;
                let span = prepared.get(&occ.document_path).and_then(|doc| {
                    let encoding = index
                        .encoding_for(&occ.document_path)
                        .expect("document has an encoding");
                    range_to_span(doc.tree.source(), &doc.line_index, occ.range, encoding)
                });
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::DuplicateAmbiguous,
                    span,
                    expected_name: expected_name.clone(),
                    found_text: None,
                });
                continue;
            }
            let Some(doc) = prepared.get(&occ.document_path) else {
                // No source for the document: cannot reconcile, count as semantic-only.
                accounting.semantic_only += 1;
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::SemanticOnly,
                    span: None,
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
                    span: None,
                    expected_name: expected_name.clone(),
                    found_text: None,
                });
                continue;
            };

            match evaluate_rules(symbol, occ.role, span, &expected_name, &doc.tree) {
                Some((rule, matched_span)) => {
                    accounting.accept(rule);
                    let enclosing = if occ.role == OccurrenceRole::Reference {
                        doc.tree.enclosing_declarations(span.start)
                    } else {
                        Vec::new()
                    };
                    aligned_name_spans
                        .entry(occ.document_path.clone())
                        .or_default()
                        .push(matched_span);
                    aligned.push(AlignedOccurrence {
                        symbol: identity.clone(),
                        document_path: occ.document_path.clone(),
                        name_span: matched_span,
                        role: occ.role,
                        rule,
                        enclosing,
                    });
                }
                // No rule's expectation is satisfied: surface the refusal. A name node at the
                // location yields its text as the found evidence; other syntax yields the raw span
                // text; no construct at all is a semantic-only occurrence.
                None => match doc.tree.name_node_containing(span) {
                    Some(name_span) => {
                        accounting.text_mismatch += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::TextMismatch,
                            span: Some(name_span),
                            expected_name: expected_name.clone(),
                            found_text: doc.tree.text_at(name_span).map(str::to_string),
                        });
                    }
                    None if doc.tree.has_construct_at(span) => {
                        accounting.text_mismatch += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::TextMismatch,
                            span: Some(span),
                            expected_name: expected_name.clone(),
                            found_text: doc.tree.text_at(span).map(str::to_string),
                        });
                    }
                    None => {
                        accounting.semantic_only += 1;
                        unaligned.push(UnalignedOccurrence {
                            symbol: identity.clone(),
                            document_path: occ.document_path.clone(),
                            role: occ.role,
                            outcome: JoinOutcome::SemanticOnly,
                            span: Some(span),
                            expected_name: expected_name.clone(),
                            found_text: None,
                        });
                    }
                },
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

/// Dispatch one occurrence to the alignment rules, in order: exact (the default), crate-root,
/// operator-desugar, module-span.
///
/// Returns the accepting rule and the matched construct's span, or `None` when no rule's exact
/// expectation is satisfied.
fn evaluate_rules(
    symbol: &ExtractedSymbol,
    role: OccurrenceRole,
    span: ByteSpan,
    expected_name: &str,
    tree: &SyntaxTree,
) -> Option<(AlignmentRule, ByteSpan)> {
    let name_span = tree.name_node_containing(span);

    // Default rule: name-token equality at the matched name node.
    if let Some(ns) = name_span
        && tree.text_at(ns) == Some(expected_name)
    {
        return Some((AlignmentRule::Exact, ns));
    }

    // Crate-root rule: a descriptor whose terminal segment is `crate` (a keyword, so it can never
    // name a user symbol) is a crate root; its source token is the crate's own package name at
    // external use-sites, or the `crate` path keyword (its own node kind, not an identifier).
    if expected_name == "crate"
        && let Some(descriptor) = &symbol.descriptor
    {
        if let Some(ns) = name_span
            && let Some(found) = tree.text_at(ns)
            && package_matches(found, &descriptor.package)
        {
            return Some((AlignmentRule::CrateRoot, ns));
        }
        if let Some(construct) = tree.construct_at(span)
            && construct.kind == "crate"
        {
            return Some((AlignmentRule::CrateRoot, construct.span));
        }
    }

    // Operator-desugar rule: a reference to a method in the closed correspondence, located at the
    // construct it desugars from. The match is on the syntax-tree construct, never raw bytes: live
    // operator spans are observed sitting adjacent to the sigil (single-byte spans on whitespace
    // beside `==` and `+`), where byte-equality would false-refuse.
    if role == OccurrenceRole::Reference
        && let Some(expectation) = desugar_construct(expected_name)
        && let Some(construct) = tree.construct_at(span)
        && expectation.matches(&construct)
    {
        return Some((AlignmentRule::OperatorDesugar, construct.span));
    }

    // Module-span rule: a module definition whose range spans the module's whole document (the
    // shape rust-analyzer emits for file modules). The module kind gate keeps whole-document spans
    // on non-modules refused.
    if role == OccurrenceRole::Definition
        && symbol.kind == SymbolKind::Module
        && span.start == 0
        && span.end == tree.source().len()
    {
        return Some((AlignmentRule::ModuleSpan, span));
    }

    // Self-keyword rule: a reference resolving to a type — or to an implementation of one — at a
    // `Self` keyword token (type position and `Self::` path segments are both name nodes spelling
    // `Self`) accepts only when the nearest enclosing impl's self type shares the expected base
    // name, generic arguments stripped from both sides. The impl cross-check is what keeps the
    // rule exact: token presence alone would accept coordinate drift landing on any `Self`.
    // Lowercase `self` never matches the token check, and `Self` in a trait body has no enclosing
    // impl, so both stay refused.
    if role == OccurrenceRole::Reference
        && is_self_target(symbol)
        && let Some(ns) = name_span
        && tree.text_at(ns) == Some("Self")
        && let Some(impl_self_type) = tree.enclosing_impl_self_type(span.start)
        && base_type_name(&impl_self_type) == base_type_name(expected_name)
    {
        return Some((AlignmentRule::SelfKeyword, ns));
    }

    None
}

/// Whether a symbol is a legitimate target for the self-keyword rule: a plain type, or the impl
/// symbol rust-analyzer actually resolves `Self` to — kind `Other` with an `impl` path segment in
/// its descriptor (`impl` is a keyword, so the segment name can never collide with a user symbol).
/// The impl-header base-name cross-check carries the rule's exactness either way.
fn is_self_target(symbol: &ExtractedSymbol) -> bool {
    match symbol.kind {
        SymbolKind::Type => true,
        SymbolKind::Other => symbol
            .descriptor
            .as_ref()
            .is_some_and(|d| d.segments.iter().any(|seg| seg.name == "impl")),
        _ => false,
    }
}

/// The base name of a type spelling: generic arguments stripped (`Answer<T>` → `Answer`) and any
/// path qualifier dropped (`module::Answer` → `Answer`), so an impl header and an expected name
/// compare on the same footing.
fn base_type_name(name: &str) -> &str {
    let no_generics = name.split('<').next().unwrap_or(name).trim();
    no_generics.rsplit("::").next().unwrap_or(no_generics).trim()
}

/// Whether a source token names the descriptor's package.
///
/// Cargo package names may use `-` where source identifiers must use `_` (e.g. package
/// `silent-cartographer`, source `silent_cartographer`); crates.io treats the two as equivalent,
/// so the comparison normalizes them.
fn package_matches(found: &str, package: &str) -> bool {
    found.replace('-', "_") == package.replace('-', "_")
}

/// The construct expectation for one method in the closed operator-desugar correspondence.
enum DesugarExpectation {
    /// A try expression (`?`).
    Try,
    /// A binary expression carrying exactly this operator token.
    Binary(&'static str),
    /// A compound assignment carrying exactly this operator token.
    CompoundAssign(&'static str),
    /// A unary expression carrying exactly this leading sigil.
    Unary(&'static str),
    /// An index expression.
    Index,
    /// A call expression on a callable value.
    Call,
    /// A `for` expression (only when the occurrence lands on the loop construct itself).
    ForLoop,
}

impl DesugarExpectation {
    /// Whether the construct at the occurrence's location satisfies this expectation exactly.
    fn matches(&self, construct: &ConstructAt) -> bool {
        match self {
            DesugarExpectation::Try => construct.kind == "try_expression",
            DesugarExpectation::Binary(op) => {
                construct.kind == "binary_expression" && construct.operator.as_deref() == Some(*op)
            }
            DesugarExpectation::CompoundAssign(op) => {
                construct.kind == "compound_assignment_expr" && construct.operator.as_deref() == Some(*op)
            }
            DesugarExpectation::Unary(op) => {
                construct.kind == "unary_expression" && construct.operator.as_deref() == Some(*op)
            }
            DesugarExpectation::Index => construct.kind == "index_expression",
            DesugarExpectation::Call => construct.kind == "call_expression",
            DesugarExpectation::ForLoop => construct.kind == "for_expression",
        }
    }
}

/// The closed method-to-construct correspondence for the operator-desugar rule (design.md).
///
/// Extending this table is a design amendment plus tests, never an implementation convenience.
/// Autoderef at `.` is deliberately excluded: `deref`/`deref_mut` accept only the explicit unary
/// `*`, and a `.` site never parses as a unary expression, so the exclusion is structural.
fn desugar_construct(method: &str) -> Option<DesugarExpectation> {
    use DesugarExpectation::*;
    Some(match method {
        "branch" => Try,
        "eq" => Binary("=="),
        "ne" => Binary("!="),
        "lt" => Binary("<"),
        "le" => Binary("<="),
        "gt" => Binary(">"),
        "ge" => Binary(">="),
        "add" => Binary("+"),
        "sub" => Binary("-"),
        "mul" => Binary("*"),
        "div" => Binary("/"),
        "rem" => Binary("%"),
        "add_assign" => CompoundAssign("+="),
        "sub_assign" => CompoundAssign("-="),
        "mul_assign" => CompoundAssign("*="),
        "div_assign" => CompoundAssign("/="),
        "rem_assign" => CompoundAssign("%="),
        "bitand" => Binary("&"),
        "bitor" => Binary("|"),
        "bitxor" => Binary("^"),
        "shl" => Binary("<<"),
        "shr" => Binary(">>"),
        "bitand_assign" => CompoundAssign("&="),
        "bitor_assign" => CompoundAssign("|="),
        "bitxor_assign" => CompoundAssign("^="),
        "shl_assign" => CompoundAssign("<<="),
        "shr_assign" => CompoundAssign(">>="),
        "neg" => Unary("-"),
        "not" => Unary("!"),
        "index" | "index_mut" => Index,
        "deref" | "deref_mut" => Unary("*"),
        "call" | "call_mut" | "call_once" => Call,
        "into_iter" | "next" => ForLoop,
        _ => return None,
    })
}

/// The set of symbol indices whose resolved descriptor is shared by more than one distinct
/// definition — the semantic backend's true-duplicate defect.
///
/// A descriptor is duplicated when at least two persisted symbols carry that exact descriptor and
/// each has a definition occurrence. Only persisted symbols (those with an assigned identity) are
/// considered; a symbol without a definition occurrence does not count toward the duplicate quorum,
/// so a lone definition plus scattered references is not treated as duplicated.
fn duplicated_descriptor_symbols(index: &ExtractedIndex, identities: &[Option<CanonicalId>]) -> HashSet<usize> {
    // Group the positions of persisted, definition-bearing symbols by their exact descriptor.
    let mut by_descriptor: HashMap<&crate::identity::Descriptor, Vec<usize>> = HashMap::new();
    for (idx, symbol) in index.symbols.iter().enumerate() {
        let Some(Some(_)) = identities.get(idx) else {
            continue;
        };
        let Some(descriptor) = &symbol.descriptor else {
            continue;
        };
        if symbol.definition().is_some() {
            by_descriptor.entry(descriptor).or_default().push(idx);
        }
    }

    let mut duplicated = HashSet::new();
    for positions in by_descriptor.values() {
        if positions.len() > 1 {
            duplicated.extend(positions.iter().copied());
        }
    }
    duplicated
}
