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
//! its own location (co-location); a non-definition occurrence of such a descriptor is attributed to a
//! twin only when a locality rule selects it uniquely — the occurrence sits in that twin's own
//! defining document, or in a document reachable only through that twin's module tree — and it still
//! passes the ordinary alignment rules; an occurrence no locality rule settles is typed
//! duplicate-ambiguous and attributed to no twin, rather than silently misassigned.

use std::collections::{HashMap, HashSet};

use crate::identity::{CanonicalId, Descriptor};
use crate::semantic::model::{ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, SymbolKind};

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

/// The locality rule that selected a duplicated descriptor's twin for a group reference occurrence.
/// Stored as additional provenance alongside the [`AlignmentRule`] on an attribution derived from a
/// duplicate group; `None` for an attribution not derived from a duplicate group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalityRule {
    /// The occurrence's document is the definition document of exactly one twin.
    DefiningDocument,
    /// The occurrence's document belongs to exactly one twin's module tree.
    ModuleChain,
    /// The occurrence's source token spells the package's own name, which denotes the package's
    /// library target regardless of the containing document; the twin defined at the library
    /// target's root — per the build system's authoritative target description — is selected.
    TargetMetadata,
}

impl LocalityRule {
    /// The stored tag for this rule.
    pub fn tag(&self) -> &'static str {
        match self {
            LocalityRule::DefiningDocument => "defining_document",
            LocalityRule::ModuleChain => "module_chain",
            LocalityRule::TargetMetadata => "target_metadata",
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
    /// The locality rule that selected this attribution's twin, for an occurrence resolved from a
    /// duplicated descriptor's group; `None` for an ordinary (non-duplicated) attribution.
    pub locality: Option<LocalityRule>,
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
                        locality: None,
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

    // Group-addressed occurrences of duplicated descriptors: locality selects a unique twin, then the
    // occurrence still runs the ordinary alignment rules against that twin's expected name.
    let twins_by_descriptor = twin_symbols_by_descriptor(index, identities);
    let parent_of = parent_document_map(index, &prepared);
    for group in &index.duplicate_groups {
        // The identity the group's own discrepancies are surfaced under: the twins' shared base
        // (their identity with the `#<rank>` disambiguator stripped), so inspection sees which
        // descriptor group an ambiguity belongs to.
        let group_identity = group_base_identity(
            &group.descriptor,
            twins_by_descriptor.get(&group.descriptor).map(Vec::as_slice),
            index,
            identities,
        );
        let Some(twins) = twins_by_descriptor.get(&group.descriptor) else {
            // No persisted twins for this descriptor (e.g. the caller chose not to persist them):
            // nothing to attribute to, so every occurrence is duplicate-ambiguous.
            for occ in &group.occurrences {
                record_group_ambiguous(
                    occ,
                    group,
                    &group_identity,
                    &prepared,
                    index,
                    &mut accounting,
                    &mut unaligned,
                );
            }
            continue;
        };
        let expected_name = group.descriptor.terminal_name().unwrap_or_default();

        for occ in &group.occurrences {
            // Normalize the occurrence's location up front: the package-name check must read the
            // source token, and the alignment rules need the byte span.
            let doc = prepared.get(&occ.document_path);
            let span = doc.and_then(|d| {
                let encoding = index
                    .encoding_for(&occ.document_path)
                    .expect("document has an encoding");
                range_to_span(d.tree.source(), &d.line_index, occ.range, encoding)
            });

            // Package-name resolution via build-target metadata (design.md, 2026-07-07 second
            // amendment): a token spelling the package's own name denotes the package's library
            // target no matter which document it sits in, so containing-document locality is never
            // consulted for it. The library twin comes from the build system's authoritative target
            // description (`library_roots`); when that is unavailable, or names no persisted twin,
            // the occurrence degrades to the typed duplicate-ambiguous refusal — honesty, never a
            // guess. The token comparison reuses the crate-root rule's normalization; the
            // terminal-name guard keeps this path off groups whose own name spells the package
            // name. Scoped to duplicated groups only — a unique crate root's package-name reference
            // still aligns through the ordinary join.
            if let Some(d) = doc
                && let Some(span) = span
                && let Some(name_span) = d.tree.name_node_containing(span)
                && let Some(token) = d.tree.text_at(name_span)
                && token != expected_name
                && package_matches(token, &group.descriptor.package)
            {
                let library_twin = index
                    .library_roots
                    .get(&group.descriptor.package)
                    .and_then(|root| twins.iter().find(|(_, _, def_doc)| *def_doc == root.as_str()));
                match library_twin {
                    Some((twin_symbol, identity, _)) => join_group_occurrence(
                        occ,
                        twin_symbol,
                        identity,
                        LocalityRule::TargetMetadata,
                        span,
                        d,
                        expected_name,
                        &mut accounting,
                        &mut aligned,
                        &mut unaligned,
                        &mut aligned_name_spans,
                    ),
                    None => record_group_ambiguous(
                        occ,
                        group,
                        &group_identity,
                        &prepared,
                        index,
                        &mut accounting,
                        &mut unaligned,
                    ),
                }
                continue;
            }

            let selection = select_twin(&occ.document_path, twins, &parent_of);
            let Some((twin_symbol, identity, locality)) = selection else {
                record_group_ambiguous(
                    occ,
                    group,
                    &group_identity,
                    &prepared,
                    index,
                    &mut accounting,
                    &mut unaligned,
                );
                continue;
            };

            let (Some(d), Some(span)) = (doc, span) else {
                // No source for the document, or coordinates that could not normalize onto it:
                // semantic-only, keyed to the locality-selected twin.
                accounting.semantic_only += 1;
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::SemanticOnly,
                    span: None,
                    expected_name: expected_name.to_string(),
                    found_text: None,
                });
                continue;
            };

            join_group_occurrence(
                occ,
                twin_symbol,
                identity,
                locality,
                span,
                d,
                expected_name,
                &mut accounting,
                &mut aligned,
                &mut unaligned,
                &mut aligned_name_spans,
            );
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

/// Record one group occurrence as duplicate-ambiguous: no locality rule selected a unique twin. The
/// occurrence's location is still recorded (normalized when the source is available) so the ambiguity
/// is inspectable, exactly as the prior unconditional-refusal path did.
/// Run one group-addressed occurrence, its twin already selected by a locality rule, through the
/// ordinary alignment rules: locality selects the target, it never overrides a text refusal — the
/// occurrence is accepted or refused exactly as it would be for a non-duplicated symbol, with the
/// selecting locality rule recorded as provenance on an acceptance.
#[allow(clippy::too_many_arguments)] // the join's shared sinks (accounting, aligned, unaligned, span map) travel together
fn join_group_occurrence(
    occ: &ExtractedOccurrence,
    twin_symbol: &ExtractedSymbol,
    identity: &CanonicalId,
    locality: LocalityRule,
    span: ByteSpan,
    doc: &PreparedDocument,
    expected_name: &str,
    accounting: &mut JoinAccounting,
    aligned: &mut Vec<AlignedOccurrence>,
    unaligned: &mut Vec<UnalignedOccurrence>,
    aligned_name_spans: &mut HashMap<String, Vec<ByteSpan>>,
) {
    match evaluate_rules(twin_symbol, occ.role, span, expected_name, &doc.tree) {
        Some((rule, matched_span)) => {
            accounting.accept(rule);
            let enclosing = doc.tree.enclosing_declarations(span.start);
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
                locality: Some(locality),
                enclosing,
            });
        }
        None => match doc.tree.name_node_containing(span) {
            Some(name_span) => {
                accounting.text_mismatch += 1;
                unaligned.push(UnalignedOccurrence {
                    symbol: identity.clone(),
                    document_path: occ.document_path.clone(),
                    role: occ.role,
                    outcome: JoinOutcome::TextMismatch,
                    span: Some(name_span),
                    expected_name: expected_name.to_string(),
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
                    expected_name: expected_name.to_string(),
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
                    expected_name: expected_name.to_string(),
                    found_text: None,
                });
            }
        },
    }
}

fn record_group_ambiguous(
    occ: &ExtractedOccurrence,
    group: &crate::semantic::model::DuplicateGroup,
    group_identity: &CanonicalId,
    prepared: &HashMap<String, PreparedDocument>,
    index: &ExtractedIndex,
    accounting: &mut JoinAccounting,
    unaligned: &mut Vec<UnalignedOccurrence>,
) {
    accounting.duplicate_ambiguous += 1;
    let span = prepared.get(&occ.document_path).and_then(|doc| {
        let encoding = index
            .encoding_for(&occ.document_path)
            .expect("document has an encoding");
        range_to_span(doc.tree.source(), &doc.line_index, occ.range, encoding)
    });
    // No single twin owns the occurrence, so the discrepancy is surfaced under the group's shared
    // identity base — which descriptor group the ambiguity belongs to stays inspectable.
    unaligned.push(UnalignedOccurrence {
        symbol: group_identity.clone(),
        document_path: occ.document_path.clone(),
        role: occ.role,
        outcome: JoinOutcome::DuplicateAmbiguous,
        span,
        expected_name: group.descriptor.terminal_name().unwrap_or_default().to_string(),
        found_text: None,
    });
}

/// The identity a duplicate group's own discrepancies are surfaced under: the shared base of the
/// twins' identities (the `#<rank>` disambiguator stripped from any twin — the base is common to the
/// whole collision group by construction).
///
/// When the group has no persisted twins, any persisted symbol carrying the same descriptor names
/// the group; failing that, the descriptor's terminal name keeps the discrepancy non-empty and
/// inspectable.
fn group_base_identity(
    descriptor: &Descriptor,
    twins: Option<&[(&ExtractedSymbol, &CanonicalId, &str)]>,
    index: &ExtractedIndex,
    identities: &[Option<CanonicalId>],
) -> CanonicalId {
    if let Some(twins) = twins
        && let Some((_, identity, _)) = twins.first()
    {
        return strip_disambiguator(identity);
    }
    for (idx, symbol) in index.symbols.iter().enumerate() {
        if symbol.descriptor.as_ref() == Some(descriptor)
            && let Some(Some(identity)) = identities.get(idx)
        {
            return strip_disambiguator(identity);
        }
    }
    CanonicalId::from_raw(descriptor.terminal_name().unwrap_or_default().to_string())
}

/// An identity with a trailing `#<digits>` collision disambiguator stripped; an identity carrying no
/// disambiguator is returned unchanged.
fn strip_disambiguator(identity: &CanonicalId) -> CanonicalId {
    match identity.as_str().rsplit_once('#') {
        Some((base, rank)) if !rank.is_empty() && rank.bytes().all(|b| b.is_ascii_digit()) => {
            CanonicalId::from_raw(base.to_string())
        }
        _ => identity.clone(),
    }
}

/// The persisted twin symbols sharing each duplicated descriptor: `(symbol, identity, definition
/// document)`, keyed by the shared descriptor.
///
/// Only symbols with an assigned identity (the caller chose to persist them) and a definition
/// occurrence participate; a descriptor with fewer than two such symbols has no group to resolve
/// against (its occurrences, if any duplicate group still names it, are typed ambiguous).
fn twin_symbols_by_descriptor<'a>(
    index: &'a ExtractedIndex,
    identities: &'a [Option<CanonicalId>],
) -> HashMap<&'a Descriptor, Vec<(&'a ExtractedSymbol, &'a CanonicalId, &'a str)>> {
    let mut by_descriptor: HashMap<&Descriptor, Vec<(&ExtractedSymbol, &CanonicalId, &str)>> = HashMap::new();
    for (idx, symbol) in index.symbols.iter().enumerate() {
        let Some(Some(identity)) = identities.get(idx) else {
            continue;
        };
        let Some(descriptor) = &symbol.descriptor else {
            continue;
        };
        let Some(def) = symbol.definition() else {
            continue;
        };
        by_descriptor
            .entry(descriptor)
            .or_default()
            .push((symbol, identity, def.document_path.as_str()));
    }
    by_descriptor.retain(|_, twins| twins.len() > 1);
    by_descriptor
}

/// The document a module symbol's definition is declared from: for every in-workspace module symbol,
/// its definition document maps to the document containing that module's declaration site (`mod
/// name;` / `mod name { .. }`).
///
/// Built from evidence in the index cross-checked against the syntax oracle: a module symbol is
/// referenced from every `use`/path segment naming it across the workspace, so a reference
/// occurrence counts as declaration evidence only when its span is the name token of a `mod` item in
/// the parsed document. Per definition document, exactly one distinct declaring document yields a
/// parent edge; zero (a crate root, declared by no one) or more than one (conflicting evidence)
/// yields no entry — the chain stops there and the occurrence falls to duplicate-ambiguous, refusal
/// over guessing.
fn parent_document_map(
    index: &ExtractedIndex,
    prepared: &HashMap<String, PreparedDocument>,
) -> HashMap<String, String> {
    // Every declaration-site document observed per definition document.
    let mut declared_from: HashMap<String, HashSet<String>> = HashMap::new();
    for symbol in &index.symbols {
        if symbol.kind != SymbolKind::Module {
            continue;
        }
        let Some(def) = symbol.definition() else {
            continue;
        };
        for occ in &symbol.occurrences {
            if occ.role != OccurrenceRole::Reference {
                continue;
            }
            let Some(doc) = prepared.get(&occ.document_path) else {
                continue;
            };
            let Some(encoding) = index.encoding_for(&occ.document_path) else {
                continue;
            };
            let Some(span) = range_to_span(doc.tree.source(), &doc.line_index, occ.range, encoding) else {
                continue;
            };
            if doc.tree.is_module_declaration_name(span) {
                declared_from
                    .entry(def.document_path.clone())
                    .or_default()
                    .insert(occ.document_path.clone());
            }
        }
    }

    declared_from
        .into_iter()
        .filter_map(|(def_doc, declaring)| {
            if declaring.len() == 1 {
                Some((def_doc, declaring.into_iter().next().expect("one element")))
            } else {
                None
            }
        })
        .collect()
}

/// Select the unique twin a group occurrence's document is associated with, in evidence order:
/// defining-document, then module-chain. Returns `None` when no twin or more than one twin is
/// associated — the occurrence is then typed duplicate-ambiguous.
fn select_twin<'a>(
    document_path: &str,
    twins: &[(&'a ExtractedSymbol, &'a CanonicalId, &'a str)],
    parent_of: &HashMap<String, String>,
) -> Option<(&'a ExtractedSymbol, &'a CanonicalId, LocalityRule)> {
    // Defining-document: the occurrence's document is the definition document of exactly one twin.
    let direct: Vec<_> = twins.iter().filter(|(_, _, doc)| *doc == document_path).collect();
    if direct.len() == 1 {
        let (symbol, identity, _) = direct[0];
        return Some((symbol, identity, LocalityRule::DefiningDocument));
    }
    if direct.len() > 1 {
        return None;
    }

    // Module-chain: walk the occurrence's document up through the parent-of-document map (each hop
    // moving from a module's definition document to the document declaring it) until a document that
    // is a twin's own definition document is reached, or the chain runs out. A cycle terminates the
    // walk (via the visited set) rather than looping forever on malformed input.
    let mut current = document_path.to_string();
    let mut visited: HashSet<String> = HashSet::new();
    loop {
        if !visited.insert(current.clone()) {
            return None;
        }
        let reached: Vec<_> = twins.iter().filter(|(_, _, doc)| *doc == current).collect();
        if reached.len() == 1 {
            let (symbol, identity, _) = reached[0];
            return Some((symbol, identity, LocalityRule::ModuleChain));
        }
        if reached.len() > 1 {
            return None;
        }
        match parent_of.get(&current) {
            Some(parent) => current = parent.clone(),
            None => return None,
        }
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
