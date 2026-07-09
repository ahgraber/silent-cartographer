//! An in-memory semantic backend for testing the join, persistence, and query pipeline without a
//! live analyzer.
//!
//! The fixture is a hand-built [`ExtractedIndex`] wrapped in the [`SemanticEngine`] port, plus a
//! deliberately-broken variant used to prove the conformance suite rejects a non-conformant backend.

use std::path::Path;

use super::model::{AnalyzerProvenance, ExtractedIndex, normalize};
use super::{Capabilities, SemanticEngine, SemanticError};

/// A backend that replays a pre-built index. Used to exercise everything downstream of extraction.
pub struct FixtureEngine {
    index: ExtractedIndex,
    capabilities: Capabilities,
    /// When set, `analyze` fails to model an unavailable backend.
    failure: Option<String>,
}

impl FixtureEngine {
    /// Wrap a pre-built index as a conformant backend that declares enclosure support and base-index
    /// eligibility (matching the Rust adapter's declarations).
    pub fn new(index: ExtractedIndex) -> Self {
        Self {
            index: normalize(index),
            capabilities: Capabilities::none().with_enclosure().with_base_index(),
            failure: None,
        }
    }

    /// Override the declared capabilities (e.g. to model a backend that declares nothing).
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Model an unavailable backend whose `analyze` fails.
    pub fn failing(provenance: AnalyzerProvenance, reason: impl Into<String>) -> Self {
        Self {
            index: ExtractedIndex {
                provenance,
                documents: Vec::new(),
                symbols: Vec::new(),
                duplicate_groups: Vec::new(),
                library_roots: Default::default(),
                environment: None,
            },
            capabilities: Capabilities::none(),
            failure: Some(reason.into()),
        }
    }
}

impl SemanticEngine for FixtureEngine {
    fn provenance(&self) -> AnalyzerProvenance {
        self.index.provenance.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn analyze(&self, _project_root: &Path) -> Result<ExtractedIndex, SemanticError> {
        if let Some(reason) = &self.failure {
            return Err(SemanticError::Unavailable(reason.clone()));
        }
        Ok(self.index.clone())
    }
}
