//! The `c10r` command-line surface: two query meta-operations (`get`, `trace`) and the operational
//! pair (`build`, `status`).
//!
//! `get` folds definition-lookup and enclosure-by-position onto a detail axis; `trace` folds the
//! relation taxonomy onto a relation argument. Every answer carries the calibrated output contract
//! and can be rendered as JSON with `--json`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::query::{Detail, Relation};

/// The `c10r` CLI: precise, type-aware navigation over a persisted code graph.
#[derive(Debug, Parser)]
#[command(name = "c10r", version, about)]
pub struct Cli {
    /// Path to the SQLite index database.
    #[arg(long, default_value = ".c10r/index.db", global = true)]
    pub db: PathBuf,

    /// The workspace identity to namespace symbols under. When omitted, it is derived from the
    /// workspace root's directory name.
    #[arg(long, global = true)]
    pub workspace: Option<String>,

    /// Render the answer as structured JSON.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// The top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Retrieve a symbol at a chosen detail level, by name or by source position.
    Get(GetArgs),
    /// Return the symbols standing in a named relation to a subject.
    Trace(TraceArgs),
    /// Build or refresh the index for the workspace.
    Build(BuildArgs),
    /// Report the index's provenance, freshness, and join-alignment counts.
    Status(StatusArgs),
}

/// The detail axis for `get`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DetailArg {
    /// The definition file and position.
    Location,
    /// The signature, without the body.
    Signature,
    /// The full source body.
    Body,
}

impl From<DetailArg> for Detail {
    fn from(d: DetailArg) -> Self {
        match d {
            DetailArg::Location => Detail::Location,
            DetailArg::Signature => Detail::Signature,
            DetailArg::Body => Detail::Body,
        }
    }
}

/// The relation axis for `trace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RelationArg {
    /// The declaration that directly encloses the subject.
    Containers,
    /// The symbols the subject directly contains.
    Contains,
    /// The sites that reference the subject.
    References,
    /// Everything that depends on the subject, directly and transitively to `--depth`: the impact
    /// assessment — "what could break if this symbol changes." Reference-grade, so any mention counts
    /// (a type usage or constant read, not only a call); the set is inclusive by design.
    Dependents,
}

impl From<RelationArg> for Relation {
    fn from(r: RelationArg) -> Self {
        match r {
            RelationArg::Containers => Relation::Containers,
            RelationArg::Contains => Relation::Contains,
            RelationArg::References => Relation::References,
            RelationArg::Dependents => Relation::Dependents,
        }
    }
}

/// Arguments for `get`.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The symbol reference (identity, qualified name, or shortname). Omit when using `--at`.
    pub reference: Option<String>,

    /// The detail level to retrieve.
    #[arg(long, value_enum, default_value = "location")]
    pub detail: DetailArg,

    /// Retrieve the symbol enclosing a source position, given as `path:byte_offset`.
    #[arg(long)]
    pub at: Option<String>,
}

/// Arguments for `trace`.
#[derive(Debug, Args)]
pub struct TraceArgs {
    /// The subject symbol reference.
    pub reference: String,

    /// The relation to trace.
    #[arg(long, value_enum)]
    pub relation: RelationArg,

    /// For the `dependents` relation, how many hops of transitive impact to detail (default 1).
    /// Depth 0 means aggregate-only: no detailed rows, every dependent counted in the aggregate.
    /// Supplying it with any other relation is an error.
    #[arg(long)]
    pub depth: Option<u32>,
}

/// Arguments for `status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Include the join-discrepancy detail: a bounded, grouped summary of the non-aligned
    /// occurrences behind the counts (group by outcome kind and expected name token).
    #[arg(long)]
    pub discrepancies: bool,

    /// With the discrepancy detail, return every persisted row rather than the bounded summary.
    #[arg(long)]
    pub all: bool,

    /// Include the duplicated-descriptor group detail: each group's shared descriptor and the
    /// definitions that share it. The group count is always reported in the summary; this flag adds
    /// the per-group detail.
    #[arg(long)]
    pub duplicates: bool,
}

/// Arguments for `build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// The workspace root to index.
    #[arg(default_value = ".")]
    pub root: PathBuf,

    /// Path to the `rust-analyzer` executable.
    #[arg(long, default_value = "rust-analyzer")]
    pub rust_analyzer: String,
}
