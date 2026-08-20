//! The `c10r` command-line surface: the query commands (`get`, `trace`, `find`), the operational
//! set (`build`, `status`, `doctor`, `cache`, `hooks`), and the self-describing surface (`manifest`,
//! `completions`).
//!
//! `get` folds definition-lookup and enclosure-by-position onto a detail axis; `trace` folds the
//! relation taxonomy onto a relation argument. Every answer carries the calibrated output contract
//! and can be rendered as JSON with `--json`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::graph::syntax::Language;
use crate::query::{Detail, OrderMode, Relation};

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

    /// When to apply color and styling to the human rendering.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub color: ColorArg,

    #[command(subcommand)]
    pub command: Command,
}

/// When the human rendering applies color and styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorArg {
    /// Style only when standard output is a terminal.
    Auto,
    /// Always apply styling.
    Always,
    /// Never apply styling.
    Never,
}

/// The top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Retrieve a symbol at a chosen detail level, by name or by source position.
    Get(GetArgs),
    /// Return the symbols standing in a named relation to a subject.
    Trace(TraceArgs),
    /// Return the indexed symbols whose name contains a fragment, matched case-insensitively.
    ///
    /// Case folding is ASCII-only: an ASCII letter in the fragment matches regardless of case, while
    /// a non-ASCII character matches only exactly.
    Find(FindArgs),
    /// Search indexed code by meaning: describe what the code does in natural language and get the
    /// nearest candidate symbols by estimated relevance — not the complete set of relevant code.
    ///
    /// The ranking is model-derived estimation over the indexed content (names, documentation, and
    /// source alike are evidence), so an empty or truncated answer is never proof that no relevant
    /// code exists.
    Search(SearchArgs),
    /// Rank the indexed symbols most similar in content to a subject symbol — the nearest
    /// candidates by estimated similarity, not the complete set of similar code.
    ///
    /// The ranking is model-derived estimation; rows that are deterministic clones of the subject
    /// (token-identical, or identical up to consistently renamed identifiers and substituted
    /// literal values) carry a typed clone marker and rank ahead of the estimate.
    Similar(SimilarArgs),
    /// Assess what a change could affect: the dependents of every symbol the change touched, seeded
    /// from a git diff rather than from a symbol the caller names.
    Impact(ImpactArgs),
    /// Build or refresh the index for the workspace.
    Build(BuildArgs),
    /// Report the index's provenance, freshness, and join-alignment counts.
    Status(StatusArgs),
    /// Report whether each required language indexer is present, and its version.
    Doctor,
    /// Remove the stored index at the discovered `--db` path.
    Cache,
    /// Install the repository hooks that keep the index current.
    Hooks(HooksArgs),
    /// Emit the command/flag structure and the current index state as JSON, for machine
    /// orientation; always JSON, regardless of `--json`.
    Manifest,
    /// Emit a shell completion script for a supported shell, generated from the same command
    /// definition the parser executes, so the completed surface cannot drift from the real one.
    ///
    /// Install the emitted script into your shell's completion path, e.g. `c10r completions zsh >
    /// ~/.zfunc/_c10r`; see the README for the install pattern per shell.
    Completions(CompletionsArgs),
}

/// The detail axis for `get` (and, optionally, `trace`).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DetailArg {
    /// The definition file and position.
    Location,
    /// The signature, without the body.
    Signature,
    /// The signature together with the symbol's own documentation.
    Interface,
    /// The full source body.
    Body,
}

impl From<DetailArg> for Detail {
    fn from(d: DetailArg) -> Self {
        match d {
            DetailArg::Location => Detail::Location,
            DetailArg::Signature => Detail::Signature,
            DetailArg::Interface => Detail::Interface,
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
    /// The modules that import the subject.
    Importers,
    /// The types that declare the subject as a supertype — a trait's implementors or a base type's
    /// subtypes.
    Implementers,
    /// The reference sites whose enclosing declaration is classified test code — "what test code
    /// exercises this symbol." Convention-based classification (file names, test attributes, test
    /// directories), not resolved semantic fact; every answer is labeled heuristic-grade.
    Tests,
}

impl From<RelationArg> for Relation {
    fn from(r: RelationArg) -> Self {
        match r {
            RelationArg::Containers => Relation::Containers,
            RelationArg::Contains => Relation::Contains,
            RelationArg::References => Relation::References,
            RelationArg::Dependents => Relation::Dependents,
            RelationArg::Importers => Relation::Importers,
            RelationArg::Implementers => Relation::Implementers,
            RelationArg::Tests => Relation::Tests,
        }
    }
}

/// The order of the detailed dependent rows in a `dependents` trace or an `impact` assessment.
/// Distance is always the primary key; the mode decides what breaks ties within a distance layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OrderArg {
    /// Within each distance layer, order rows by a structural-importance heuristic (codebase-wide,
    /// most important first) — guidance for reading order, not resolved semantic fact.
    Ranked,
    /// Order rows only by the answer's stable structural keys: distance, then dependency kind,
    /// then canonical identity — free of any ranking model.
    Unranked,
}

impl From<OrderArg> for OrderMode {
    fn from(o: OrderArg) -> Self {
        match o {
            OrderArg::Ranked => OrderMode::Ranked,
            OrderArg::Unranked => OrderMode::Unranked,
        }
    }
}

/// The result-set paging bounds shared by the query commands (`get`, `trace`, `find`). Defined
/// per-command rather than globally so clap rejects them on commands they are meaningless for
/// (`build`, `status`, `doctor`, `cache`, `manifest`) as a usage error before any side effect.
#[derive(Debug, Args)]
pub struct PageArgs {
    /// Cap the number of results a result-bearing answer returns. Defaults to 25; `0` means
    /// unbounded (the whole set, with no page block).
    #[arg(long, default_value_t = 25)]
    pub limit: usize,

    /// Resume a prior result set from its opaque continuation token.
    #[arg(long)]
    pub cursor: Option<String>,
}

/// Arguments for `get`.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The symbol reference (identity, qualified name, or shortname). Omit when using `--at`.
    #[arg(conflicts_with = "at")]
    pub reference: Option<String>,

    /// The detail level to retrieve.
    #[arg(long, value_enum, default_value = "location")]
    pub detail: DetailArg,

    /// Retrieve the symbol enclosing a source position, given as `path:byte_offset`. Mutually
    /// exclusive with a positional reference.
    #[arg(long)]
    pub at: Option<String>,

    /// Cap the lines of content returned for a content-bearing detail (signature, interface, body).
    /// Defaults to 100; `0` means unbounded. Applies only to a content-bearing detail.
    #[arg(long, default_value_t = 100)]
    pub max_lines: usize,

    /// The 1-based line within the retrieved content where the returned window starts (default 1).
    /// The window is the lines `[from, from + max-lines)`. Applies only to a content-bearing detail.
    #[arg(long, default_value_t = 1, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
    pub from: usize,

    #[command(flatten)]
    pub paging: PageArgs,
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

    /// Project each result row's tier content at this detail, in addition to its identity and
    /// location. Omitted, rows carry no tier content and the output shape is unchanged; the flag
    /// never changes which rows are returned or their order.
    #[arg(long, value_enum)]
    pub detail: Option<DetailArg>,

    /// For the `dependents` relation, how the detailed rows are ordered: `ranked` (the default)
    /// orders each distance layer by a structural-importance heuristic, most important first;
    /// `unranked` orders by distance, dependency kind, then identity. Never changes which rows are
    /// returned. Supplying it with any other relation is an error.
    #[arg(long, value_enum, default_value = "ranked")]
    pub order: OrderArg,

    /// Cap the lines of content projected onto each row, for a content-bearing detail. Defaults to
    /// 10; `0` means unbounded. Applies only when `--detail` selects a content-bearing tier.
    #[arg(long, default_value_t = 10)]
    pub max_lines: usize,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `find`.
#[derive(Debug, Args)]
pub struct FindArgs {
    /// The name fragment to search for.
    pub fragment: String,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `search`.
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// The natural-language query describing what the code does.
    pub query: String,

    /// Project each result row's tier content at this detail. Never changes which symbols are
    /// returned or their order.
    #[arg(long, value_enum, default_value = "signature")]
    pub detail: DetailArg,

    /// Cap the lines of content projected onto each row, for a content-bearing detail. Defaults to
    /// 10; `0` means unbounded.
    #[arg(long, default_value_t = 10)]
    pub max_lines: usize,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `similar`.
#[derive(Debug, Args)]
pub struct SimilarArgs {
    /// The subject symbol reference (identity, qualified name, or shortname). Omit when using
    /// `--at`.
    #[arg(conflicts_with = "at")]
    pub reference: Option<String>,

    /// Take the symbol enclosing a source position as the subject, given as `path:byte_offset`.
    /// Mutually exclusive with a positional reference.
    #[arg(long)]
    pub at: Option<String>,

    /// Project each result row's tier content at this detail. Never changes which symbols are
    /// returned or their order.
    #[arg(long, value_enum, default_value = "signature")]
    pub detail: DetailArg,

    /// Cap the lines of content projected onto each row, for a content-bearing detail. Defaults to
    /// 10; `0` means unbounded.
    #[arg(long, default_value_t = 10)]
    pub max_lines: usize,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `impact`.
///
/// `impact` carries no `--detail`/`--max-lines`: it projects no tier content onto its rows.
#[derive(Debug, Args)]
pub struct ImpactArgs {
    /// The revision or revision range to seed from (`A..B`, `A...B`, or a single revision, which
    /// means that revision against the working tree, exactly as `git diff <commit>` does). Omitted,
    /// the working-tree change against `HEAD` seeds the assessment.
    #[arg(conflicts_with = "staged")]
    pub revspec: Option<String>,

    /// Seed from the staged change against `HEAD` rather than from the working tree.
    #[arg(long)]
    pub staged: bool,

    /// How many hops of transitive impact to detail (default 1). Depth 0 means aggregate-only: no
    /// detailed rows, every dependent counted in the aggregate.
    #[arg(long, default_value_t = 1)]
    pub depth: u32,

    /// How the detailed dependent rows are ordered: `ranked` (the default) orders each distance
    /// layer by a structural-importance heuristic, most important first; `unranked` orders by
    /// distance, dependency kind, then identity. Never changes which rows are returned.
    #[arg(long, value_enum, default_value = "ranked")]
    pub order: OrderArg,

    /// Narrow the seed to these paths, after `--`. A renamed file is reached by either its
    /// pre-change or its post-change path.
    #[arg(last = true)]
    pub paths: Vec<PathBuf>,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `completions`.
#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// The shell to generate a completion script for. An unsupported value is rejected with the
    /// supported set.
    pub shell: Shell,
}

/// The action `hooks` performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookAction {
    /// Install the post-commit hook that refreshes the index.
    Install,
}

/// Arguments for `hooks`.
#[derive(Debug, Args)]
pub struct HooksArgs {
    /// The action to perform.
    pub action: HookAction,
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

/// The language backend selector for `build`: overrides manifest detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LanguageArg {
    /// The Rust backend (`rust-analyzer scip`).
    Rust,
    /// The Python backend (`scip-python index`).
    Python,
}

impl From<LanguageArg> for Language {
    fn from(l: LanguageArg) -> Self {
        match l {
            LanguageArg::Rust => Language::Rust,
            LanguageArg::Python => Language::Python,
        }
    }
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

    /// The language backend to build with. When omitted, the workspace's project manifest decides
    /// (`Cargo.toml` → rust, `pyproject.toml` → python); with both manifests present this flag is
    /// required.
    #[arg(long, value_enum)]
    pub language: Option<LanguageArg>,

    /// An explicit interpreter-environment path (meaningful for the Python backend). Overrides
    /// `$VIRTUAL_ENV` and the workspace's `.venv`/`venv` directories.
    #[arg(long)]
    pub environment: Option<PathBuf>,

    /// Build even when the stored index already describes the workspace's sources, analyzer, and
    /// environment. Without it, such a build analyzes nothing and leaves the store untouched.
    #[arg(long)]
    pub force: bool,
}
