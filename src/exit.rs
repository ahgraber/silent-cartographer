//! The process exit-code taxonomy: a closed set of outcome codes and the classified failures the
//! command surface raises to reach them.
//!
//! A caller branches on the process exit code without scraping the diagnostic text; every failure is
//! mapped onto one of these codes in a single place at the top of `main`.

use crate::graph::store::StoreOpenError;

/// The closed set of process outcomes, one distinct code per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// The command produced its answer, including a typed-empty answer.
    Success,
    /// A generic operational failure with no more specific category.
    General,
    /// The invocation was malformed: an unknown flag, an out-of-set enum value, or an argument
    /// meaningless for the chosen command.
    Usage,
    /// No index was discovered for the workspace.
    NoIndex,
    /// The index store was written under a different schema version than this binary expects.
    IncompatibleStore,
    /// A required language indexer or setup step was unavailable.
    IndexerSetup,
}

impl ExitCode {
    /// The numeric process exit code for this outcome.
    pub fn code(self) -> i32 {
        match self {
            ExitCode::Success => 0,
            ExitCode::General => 1,
            ExitCode::Usage => 2,
            ExitCode::NoIndex => 3,
            ExitCode::IncompatibleStore => 4,
            ExitCode::IndexerSetup => 5,
        }
    }
}

/// A command failure tagged with the exit-code category it maps to, for the categories a command
/// handler raises directly (as opposed to those inferred from an underlying store error).
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// A malformed invocation the parser did not catch — an argument valid in isolation but
    /// meaningless for the chosen command. Maps to [`ExitCode::Usage`].
    #[error("{0}")]
    Usage(String),
    /// No index was found to answer the query. Maps to [`ExitCode::NoIndex`].
    #[error("{0}")]
    NoIndex(String),
    /// A required language indexer was unavailable. Maps to [`ExitCode::IndexerSetup`].
    #[error("{0}")]
    IndexerSetup(String),
}

/// Classify a command failure into its exit-code category.
///
/// A [`Failure`] tag is honored directly. A store-open schema mismatch splits by its stamp: an
/// unstamped store (version 0) reads as no index, a store stamped with a different version as an
/// incompatible store. Anything else is a generic operational failure.
pub fn classify(error: &anyhow::Error) -> ExitCode {
    if let Some(failure) = error.chain().find_map(|e| e.downcast_ref::<Failure>()) {
        return match failure {
            Failure::Usage(_) => ExitCode::Usage,
            Failure::NoIndex(_) => ExitCode::NoIndex,
            Failure::IndexerSetup(_) => ExitCode::IndexerSetup,
        };
    }
    if let Some(StoreOpenError::SchemaVersionMismatch { found, .. }) =
        error.chain().find_map(|e| e.downcast_ref::<StoreOpenError>())
    {
        return if *found == 0 {
            ExitCode::NoIndex
        } else {
            ExitCode::IncompatibleStore
        };
    }
    ExitCode::General
}
