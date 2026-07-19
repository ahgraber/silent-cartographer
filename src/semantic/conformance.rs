//! The backend conformance suite: the mandatory extraction contract, encoded as checks a backend's
//! output must satisfy to be treated as usable.
//!
//! A backend is usable through the port if and only if it satisfies the contract in full. The suite
//! runs a backend over a project and reports conformance; a violation makes the backend
//! non-conformant, and the system must not treat a non-conformant backend as usable.

use std::path::Path;

use super::model::{ExtractedIndex, OccurrenceRole, SymbolClass};
use super::{SemanticEngine, SemanticError};

/// A single contract clause a backend either satisfies or violates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The contract clause that was violated.
    pub clause: &'static str,
    /// A human-readable detail of how it was violated.
    pub detail: String,
}

/// The outcome of running the conformance suite against a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    violations: Vec<Violation>,
}

impl ConformanceReport {
    /// Whether the backend satisfied every clause of the contract.
    pub fn is_conformant(&self) -> bool {
        self.violations.is_empty()
    }

    /// The violations found, empty when conformant.
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }
}

/// Check an already-produced index against the mandatory extraction contract.
///
/// The contract floor requires: every non-local symbol carries a resolved descriptor; every
/// occurrence is role-classified and carries a well-formed, mappable range; the index carries
/// analyzer provenance; and at least one definition-role and one reference-role occurrence are
/// producible (evidence the backend classifies both roles).
pub fn check_index(index: &ExtractedIndex) -> ConformanceReport {
    let mut violations = Vec::new();

    if index.provenance.analyzer_name.trim().is_empty() || index.provenance.analyzer_version.trim().is_empty() {
        violations.push(Violation {
            clause: "provenance-present",
            detail: "analyzer name and version must both be non-empty".to_string(),
        });
    }

    let mut saw_definition = false;
    let mut saw_reference = false;

    for symbol in &index.symbols {
        // A non-local symbol must carry a resolved descriptor.
        if symbol.class != SymbolClass::Local && symbol.descriptor.is_none() {
            violations.push(Violation {
                clause: "resolved-descriptor",
                detail: format!("non-local {:?} symbol has no resolved descriptor", symbol.kind),
            });
        }
        // A resolved descriptor must have a terminal name segment (the guard target).
        if let Some(descriptor) = &symbol.descriptor
            && descriptor.terminal_name().is_none()
        {
            violations.push(Violation {
                clause: "descriptor-terminal-name",
                detail: "resolved descriptor has no terminal name segment".to_string(),
            });
        }

        for occ in &symbol.occurrences {
            match occ.role {
                OccurrenceRole::Definition => saw_definition = true,
                OccurrenceRole::Reference => saw_reference = true,
            }
            // A range must map unambiguously to a file position: it must be well-formed (end not
            // before start) and its document must declare an encoding.
            if index.encoding_for(&occ.document_path).is_none() {
                violations.push(Violation {
                    clause: "range-mappable",
                    detail: format!(
                        "occurrence document {} declares no position encoding",
                        occ.document_path
                    ),
                });
            }
            let r = occ.range;
            let well_formed = (r.end_line, r.end_char) >= (r.start_line, r.start_char) || r.end_line > r.start_line;
            if !well_formed {
                violations.push(Violation {
                    clause: "range-well-formed",
                    detail: format!("occurrence range {r:?} is not well-formed"),
                });
            }
        }
    }

    if !saw_definition {
        violations.push(Violation {
            clause: "role-definition",
            detail: "no definition-role occurrence produced".to_string(),
        });
    }
    if !saw_reference {
        violations.push(Violation {
            clause: "role-reference",
            detail: "no reference-role occurrence produced".to_string(),
        });
    }

    ConformanceReport { violations }
}

/// Run the same contract clauses against several backends, one project root per backend, reporting
/// each under its label.
///
/// Backends gate independently: a violation in one backend's report never affects another's — the
/// suite is parameterized by backend, not aggregated across them.
pub fn run_all<'a>(backends: &[(&'a str, &'a dyn SemanticEngine, &'a Path)]) -> Vec<(&'a str, ConformanceReport)> {
    backends
        .iter()
        .map(|(label, engine, root)| (*label, run(*engine, root)))
        .collect()
}

/// Run a backend over a project and check its output against the contract.
///
/// A backend whose `analyze` fails is reported non-conformant (it cannot be relied upon), rather
/// than propagating the error, so callers get a uniform usability verdict.
pub fn run(engine: &dyn SemanticEngine, project_root: &Path) -> ConformanceReport {
    match engine.analyze(project_root) {
        Ok(index) => check_index(&index),
        Err(
            SemanticError::Analysis(msg)
            | SemanticError::Unavailable(msg)
            | SemanticError::Environment(msg)
            | SemanticError::Timeout(msg),
        ) => ConformanceReport {
            violations: vec![Violation {
                clause: "analyze-succeeds",
                detail: msg,
            }],
        },
    }
}
