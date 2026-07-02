//! The semantic-engine port: the backend contract for producing a project's symbols and
//! occurrences, plus the engine-neutral model those backends emit.
//!
//! The mandatory contract is the intersection every backend (batch SCIP now, live LSP later) can
//! satisfy: produce symbols with resolved descriptors and role-classified occurrences carrying
//! mappable ranges, plus analyzer provenance. Capabilities only some backends offer are exposed as
//! queryable feature detection ([`Capabilities`]), never assumed.

pub mod capabilities;
pub mod conformance;
pub mod fixture;
pub mod model;
pub mod rust_adapter;

pub use capabilities::Capabilities;
pub use model::{
    AnalyzerProvenance, ExtractedIndex, ExtractedOccurrence, ExtractedSymbol, OccurrenceRole, PositionEncoding,
    SourceDocument, SourceRange, SymbolClass, SymbolKind,
};

use thiserror::Error;

/// An error a semantic backend can return while producing an index.
#[derive(Debug, Error)]
pub enum SemanticError {
    /// The backend could not analyze the project.
    #[error("semantic analysis failed: {0}")]
    Analysis(String),
    /// The backend's tool was not available in the environment.
    #[error("semantic backend unavailable: {0}")]
    Unavailable(String),
}

/// The backend contract floor: the intersection every semantic backend must satisfy.
///
/// A backend is usable through this port if and only if it satisfies the contract in full, verified
/// by the [`conformance`] suite. The Rust adapter is the exemplar implementation.
pub trait SemanticEngine {
    /// The analyzer's identity and version, recorded as provenance of every index it produces.
    fn provenance(&self) -> AnalyzerProvenance;

    /// The optional capabilities this backend declares. Callers query this before relying on any
    /// capability the contract floor does not mandate.
    fn capabilities(&self) -> Capabilities;

    /// Analyze the project rooted at `project_root` and produce its symbols and occurrences.
    ///
    /// Every produced symbol carries a resolved descriptor (or is classified local/external), and
    /// every occurrence is role-classified and carries a range that maps unambiguously to a file
    /// position via the owning document's declared position encoding.
    fn analyze(&self, project_root: &std::path::Path) -> Result<ExtractedIndex, SemanticError>;
}
