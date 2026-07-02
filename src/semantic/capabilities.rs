//! Queryable feature detection for optional backend capabilities.
//!
//! The contract floor mandates only what every backend can do. Capabilities only some backends
//! offer — enclosure information, base-index eligibility, live incremental updates — are declared
//! here and queried before use, never assumed present.

use serde::{Deserialize, Serialize};

/// The optional capabilities a backend may declare.
///
/// An undeclared capability is treated as absent: the system does not infer it present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Whether the backend supplies enclosure (structure/containment) information.
    supplies_enclosure: bool,
    /// Whether the backend is eligible to serve as an authoritative base index.
    base_index_eligible: bool,
    /// Whether the backend supports live incremental updates.
    live_updates: bool,
}

impl Capabilities {
    /// A capability set with nothing declared. Callers add declarations explicitly.
    pub const fn none() -> Self {
        Self {
            supplies_enclosure: false,
            base_index_eligible: false,
            live_updates: false,
        }
    }

    /// Declare that the backend supplies enclosure information.
    pub const fn with_enclosure(mut self) -> Self {
        self.supplies_enclosure = true;
        self
    }

    /// Declare that the backend is eligible as an authoritative base index.
    pub const fn with_base_index(mut self) -> Self {
        self.base_index_eligible = true;
        self
    }

    /// Declare that the backend supports live incremental updates.
    pub const fn with_live_updates(mut self) -> Self {
        self.live_updates = true;
        self
    }

    /// Whether enclosure information is available and may be relied upon.
    pub const fn supplies_enclosure(&self) -> bool {
        self.supplies_enclosure
    }

    /// Whether the backend may be used as an authoritative base index.
    pub const fn base_index_eligible(&self) -> bool {
        self.base_index_eligible
    }

    /// Whether the backend supports live incremental updates.
    pub const fn live_updates(&self) -> bool {
        self.live_updates
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::none()
    }
}
