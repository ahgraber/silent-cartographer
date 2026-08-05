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
    /// A file at the store path is not recognized as this system's own store, so it was refused.
    UnrecognizedStore,
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
            ExitCode::UnrecognizedStore => 6,
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
    /// A target was refused because the system could not confirm it is a store the system created,
    /// raised by a handler that reaches its own recognition verdict rather than opening the store.
    /// Maps to [`ExitCode::UnrecognizedStore`].
    #[error("{0}")]
    UnrecognizedStore(String),
}

/// Classify a command failure into its exit-code category.
///
/// A [`Failure`] tag is honored directly. A store-open failure maps by kind: a target the system
/// cannot confirm is its own store is its own category — the recovery is to correct the path, the
/// opposite of the rebuild an incompatible store invites — and it covers every way that confirmation
/// can fail, whether the marker is wrong, the path cannot be examined, or a file already occupies the
/// path a build would create. An absent store reads as no index, and a recognized store at the wrong
/// schema version as an incompatible store. Anything else is a generic operational failure.
///
/// The category is the target's, not the command's: a handler that recognizes the store itself
/// rather than opening it — `cache`, which decides whether to unlink — reaches the same category
/// through [`Failure::UnrecognizedStore`], so an unconfirmed target answers alike whichever command
/// met it. An error raised after the target cleared recognition (a failed unlink, say) is an
/// operational failure, because the guard's question was answered.
pub fn classify(error: &anyhow::Error) -> ExitCode {
    if let Some(failure) = error.chain().find_map(|e| e.downcast_ref::<Failure>()) {
        return match failure {
            Failure::Usage(_) => ExitCode::Usage,
            Failure::NoIndex(_) => ExitCode::NoIndex,
            Failure::IndexerSetup(_) => ExitCode::IndexerSetup,
            Failure::UnrecognizedStore(_) => ExitCode::UnrecognizedStore,
        };
    }
    if let Some(store_error) = error.chain().find_map(|e| e.downcast_ref::<StoreOpenError>()) {
        return match store_error {
            StoreOpenError::UnrecognizedStore { .. }
            | StoreOpenError::UnreadableStore { .. }
            | StoreOpenError::StaleBuildFile { .. } => ExitCode::UnrecognizedStore,
            StoreOpenError::MissingStore { .. } => ExitCode::NoIndex,
            StoreOpenError::SchemaVersionMismatch { .. } => ExitCode::IncompatibleStore,
            StoreOpenError::Storage(_) | StoreOpenError::Io(_) => ExitCode::General,
        };
    }
    ExitCode::General
}

#[cfg(test)]
mod tests {
    use super::*;

    // The three store-related outcomes reach three distinct codes. Their remedies are mutually
    // exclusive — build here, rebuild here, do not build here at all — so a caller that branches on
    // the code must never see two of them collapse into one.
    #[test]
    fn the_store_outcomes_classify_to_three_distinct_codes() {
        let unrecognized = classify(&anyhow::Error::new(StoreOpenError::UnrecognizedStore {
            path: "/w/index.db".to_string(),
        }));
        let missing = classify(&anyhow::Error::new(StoreOpenError::MissingStore {
            path: "/w/index.db".to_string(),
        }));
        let incompatible = classify(&anyhow::Error::new(StoreOpenError::SchemaVersionMismatch {
            path: "/w/index.db".to_string(),
            found: 11,
            expected: 13,
        }));

        assert_eq!(unrecognized, ExitCode::UnrecognizedStore);
        assert_eq!(unrecognized.code(), 6);
        assert_eq!(missing, ExitCode::NoIndex);
        assert_eq!(missing.code(), 3);
        assert_eq!(incompatible, ExitCode::IncompatibleStore);
        assert_eq!(incompatible.code(), 4);
    }

    // Every way confirming ownership can fail reaches the same code, because the caller's next action
    // is the same in all of them: do not build here, correct the path. The category is "cannot confirm
    // this is ours", not "the marker was wrong", so a path that could not be examined and a path a
    // build found occupied belong to it as much as a foreign marker does.
    #[test]
    fn every_unconfirmed_ownership_outcome_reaches_the_same_code() {
        let outcomes = [
            StoreOpenError::UnrecognizedStore {
                path: "/w/index.db".to_string(),
            },
            StoreOpenError::UnreadableStore {
                path: "/w/.c10r".to_string(),
                detail: "Is a directory (os error 21)".to_string(),
            },
            StoreOpenError::StaleBuildFile {
                path: "/w/index.db.c10r-tmp-1".to_string(),
            },
        ];
        for outcome in outcomes {
            let described = outcome.to_string();
            assert_eq!(
                classify(&anyhow::Error::new(outcome)),
                ExitCode::UnrecognizedStore,
                "{described}"
            );
        }
    }

    // A version-mismatched store classifies as incompatible whatever version it carries: the stamp is
    // no longer a proxy for ownership, so a zero stamp is not a special case here.
    #[test]
    fn a_version_mismatch_is_incompatible_at_any_stamp() {
        for found in [0, 1, 12] {
            let code = classify(&anyhow::Error::new(StoreOpenError::SchemaVersionMismatch {
                path: "/w/index.db".to_string(),
                found,
                expected: 13,
            }));
            assert_eq!(code, ExitCode::IncompatibleStore, "stamp {found}");
        }
    }

    // A store-open failure wrapped in caller context still classifies: the handlers attach context
    // ("opening index database") before the error reaches the process edge.
    #[test]
    fn a_wrapped_store_refusal_still_classifies() {
        let error = anyhow::Error::new(StoreOpenError::UnrecognizedStore {
            path: "/w/index.db".to_string(),
        })
        .context("opening index database");
        assert_eq!(classify(&error), ExitCode::UnrecognizedStore);
    }
}
