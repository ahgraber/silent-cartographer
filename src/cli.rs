//! The `c10r` command-line surface: the query commands (`get`, `trace`, `find`), the operational
//! set (`build`, `status`, `doctor`, `cache` (`clear`/`dir`/`size`), `hooks`), and the
//! self-describing surface (`manifest`, `completions`).
//!
//! `get` folds definition-lookup and enclosure-by-position onto a detail axis; `trace` folds the
//! relation taxonomy onto a relation argument. Every answer carries the calibrated output contract
//! and can be rendered as JSON with `--json`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::graph::syntax::Language;
use crate::hooks::HookEvent;
use crate::query::search::RelationLabel;
use crate::query::{Detail, OrderMode, Relation};
use crate::semantic::python_adapter::PythonIndexer;

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

    /// The unit of a source position: how the human rendering names a location and how `--at` reads
    /// one. The machine answer carries both units whichever is selected.
    #[arg(long, value_enum, default_value = "line", global = true)]
    pub position: PositionUnit,

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

/// The unit a source position is named and read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PositionUnit {
    /// `path:line:column`, numbered from 1.
    Line,
    /// `path:start-end` byte offsets in the human rendering; `path:byte_offset` for `--at`.
    Byte,
}

impl PositionUnit {
    /// The flag value naming this unit.
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Byte => "byte",
        }
    }
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
    /// List deterministic clones: with no subject, every group of symbols whose token sequences are
    /// identical up to consistently renamed identifiers and substituted literal values; with a
    /// subject, the other symbols that are such copies of it and the token runs it shares with part
    /// of another symbol.
    ///
    /// Rows are token equivalence — identical token structure — never proof of identical behavior.
    Clones(ClonesArgs),
    /// Assess what a change could affect: the dependents of every symbol the change touched, seeded
    /// from a git diff rather than from a symbol the caller names.
    ///
    /// An answer that lists seeds and no dependents means no resolved dependent was recorded, not
    /// that none exists: calls by frameworks, implicit calls, and lookups by string are not seen.
    Impact(ImpactArgs),
    /// Build or refresh the index for the workspace.
    Build(BuildArgs),
    /// Report the index's provenance, freshness, and join-alignment counts.
    Status(StatusArgs),
    /// Report whether each required language indexer is present, and its version.
    Doctor,
    /// The index store: the action decides whether it is removed, located, or measured.
    Cache(CacheArgs),
    /// Install or remove the repository hooks that keep the index current, or run one refresh
    /// event's rules (the installed hooks call this).
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

/// The relation labels `similar` rows can carry, and so the labels `--exclude` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RelationLabelArg {
    /// Rows whose symbol uses the subject.
    #[value(name = "uses_subject")]
    UsesSubject,
    /// Rows whose symbol the subject uses.
    #[value(name = "used_by_subject")]
    UsedBySubject,
    /// Rows whose symbol is test code that uses the subject.
    #[value(name = "tests_subject")]
    TestsSubject,
    /// Rows whose declaring type is a supertype, subtype, or sibling of the subject's declaring type.
    #[value(name = "type_family")]
    TypeFamily,
}

impl From<RelationLabelArg> for RelationLabel {
    fn from(label: RelationLabelArg) -> Self {
        match label {
            RelationLabelArg::UsesSubject => RelationLabel::UsesSubject,
            RelationLabelArg::UsedBySubject => RelationLabel::UsedBySubject,
            RelationLabelArg::TestsSubject => RelationLabel::TestsSubject,
            RelationLabelArg::TypeFamily => RelationLabel::TypeFamily,
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
    /// For a type, the types that declare it as a supertype (a trait's implementors or a base type's
    /// subtypes); for a method, the methods that directly override it.
    Implementers,
    /// For a type, the supertypes it directly declares; for a method, the methods it directly
    /// overrides.
    Implements,
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
            RelationArg::Implements => Relation::Implements,
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
/// (`build`, `doctor`, `cache`, `manifest`) as a usage error before any side effect.
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

    /// Retrieve the symbol enclosing a source position, given as `path:line:column` or `path:line`
    /// (`path:byte_offset` under `--position byte`). Mutually exclusive with a positional reference.
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

    /// The relation to trace. An empty answer of `references`, `dependents`, `importers`, or `tests` means no
    /// resolved instance was recorded, not that none exists: calls by frameworks, implicit calls, and
    /// lookups by string are not seen. Structural relations (`containers`, `contains`, `implementers`, `implements`)
    /// answer a definite none.
    #[arg(long, value_enum)]
    pub relation: RelationArg,

    /// For the `dependents` relation, how many hops of transitive impact to detail (default 1).
    /// Depth 0 means aggregate-only: no detailed rows, every dependent counted in the aggregate.
    /// Supplying it with any other relation is an error.
    #[arg(long)]
    pub depth: Option<u32>,

    /// Project each result row's content at this detail, in addition to its identity and
    /// location. Omitted, rows carry no content and the output shape is unchanged; the flag
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
    /// 10; `0` means unbounded. Applies only when `--detail` selects a content-bearing level.
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

    /// Project each result row's content at this detail. Never changes which symbols are
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

    /// Take the symbol enclosing a source position as the subject, given as `path:line:column` or
    /// `path:line` (`path:byte_offset` under `--position byte`). Mutually exclusive with a positional
    /// reference.
    #[arg(long)]
    pub at: Option<String>,

    /// Answer for every reference in a list, one per line, read from this file or from standard
    /// input when `-`. Each answer is the one a single-subject invocation returns. Mutually
    /// exclusive with a positional reference, `--at`, and `--cursor`.
    #[arg(long, value_name = "PATH|-", conflicts_with_all = ["reference", "at", "cursor"])]
    pub subjects_from: Option<String>,

    /// List every pair of corpus symbols that are each the first row of the other's answer, under
    /// `--exclude`, ordered by the pair's lower member. The limit counts pairs; the answer carries no
    /// total, and `--limit 0` walks the whole corpus. Mutually exclusive with a positional
    /// reference, `--at`, and `--subjects-from`.
    #[arg(long, conflicts_with_all = ["reference", "at", "subjects_from"])]
    pub mutual: bool,

    /// Project each result row's content at this detail. Never changes which symbols are
    /// returned or their order.
    #[arg(long, value_enum, default_value = "signature")]
    pub detail: DetailArg,

    /// Cap the lines of content projected onto each row, for a content-bearing detail. Defaults to
    /// 10; `0` means unbounded.
    #[arg(long, default_value_t = 10)]
    pub max_lines: usize,

    /// Omit rows carrying this relation label; repeat to omit several. The result limit counts only
    /// the remaining rows.
    #[arg(long, value_enum)]
    pub exclude: Vec<RelationLabelArg>,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `clones`.
///
/// `clones` carries no `--detail`/`--max-lines`: it projects no content onto its rows.
#[derive(Debug, Args)]
pub struct ClonesArgs {
    /// The subject symbol reference (identity, qualified name, or shortname). Omit for the clone
    /// groups of the whole corpus, or when using `--at`.
    #[arg(conflicts_with = "at")]
    pub reference: Option<String>,

    /// Take the symbol enclosing a source position as the subject, given as `path:line:column` or
    /// `path:line` (`path:byte_offset` under `--position byte`). Mutually exclusive with a positional
    /// reference.
    #[arg(long)]
    pub at: Option<String>,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// Arguments for `impact`.
///
/// `impact` carries no `--detail`/`--max-lines`: it projects no content onto its rows.
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
    /// Install the refresh hooks that keep the index current.
    Install,
    /// Remove a workspace from the installed refresh hooks.
    Uninstall,
    /// Run one refresh event's rules and build when they call for it.
    ///
    /// The installed hooks call this; it is not meant to be typed by hand, though its output is
    /// always safe to read.
    Run,
}

/// Arguments for `hooks`.
#[derive(Debug, Args)]
pub struct HooksArgs {
    /// The action to perform.
    pub action: HookAction,

    /// The refresh event this invocation fired for. Required by `run`; rejected for every other
    /// action.
    pub event: Option<HookEvent>,

    /// Delete an existing hook file instead of keeping it. Accepted only with `install`.
    #[arg(long)]
    pub overwrite: bool,

    /// The workspace root the refresh applies to. Required by `run`; rejected for every other
    /// action.
    #[arg(long)]
    pub root: Option<PathBuf>,

    /// The git arguments the event fired with, after `--`. Accepted only with `run`.
    #[arg(last = true, allow_hyphen_values = true)]
    pub git_args: Vec<String>,
}

/// The action `cache` performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CacheAction {
    /// Remove the stored index at the discovered `--db` path.
    Clear,
    /// Report the directory holding the index store.
    Dir,
    /// Report the on-disk size of the index store.
    Size,
}

/// Arguments for `cache`.
#[derive(Debug, Args)]
pub struct CacheArgs {
    /// The action to perform.
    ///
    /// The value name spells the accepted set, so the rejection a caller gets for omitting the
    /// action names the alternatives without a second invocation to discover them.
    #[arg(value_name = "clear|dir|size")]
    pub action: CacheAction,
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

    /// Name each source file that differs from the index. The only value is `files`, which a bare
    /// `--detail` also means.
    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "files")]
    pub detail: Option<StatusDetail>,

    #[command(flatten)]
    pub paging: PageArgs,
}

/// The detail kinds `status --detail` can add.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusDetail {
    /// Each source file that differs from the index, with how it differs.
    Files,
}

impl StatusDetail {
    /// The value name the file list's continuation token binds.
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "files",
        }
    }
}

/// The language backend selector for `build`: overrides manifest detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LanguageArg {
    /// The Rust backend (`rust-analyzer scip`).
    Rust,
    /// The Python backend (`ty-scip index` or `scip-python index`; see `--python-indexer`).
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

/// The Python indexer selector for `build`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PythonIndexerArg {
    /// `ty-scip`, built on the ty type checker.
    TyScip,
    /// Sourcegraph's `scip-python`.
    ScipPython,
}

impl From<PythonIndexerArg> for PythonIndexer {
    fn from(i: PythonIndexerArg) -> Self {
        match i {
            PythonIndexerArg::TyScip => PythonIndexer::TyScip,
            PythonIndexerArg::ScipPython => PythonIndexer::ScipPython,
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

    /// The Python indexer to build with: `ty-scip` or `scip-python`. Without it, a build uses the
    /// indexer recorded in the existing store, and scip-python when no store exists. Refused for a
    /// Rust workspace.
    #[arg(long, value_enum)]
    pub python_indexer: Option<PythonIndexerArg>,

    /// An explicit interpreter-environment path (meaningful for the Python backend). Overrides
    /// `$VIRTUAL_ENV` and the workspace's `.venv`/`venv` directories.
    #[arg(long)]
    pub environment: Option<PathBuf>,

    /// Build even when the stored index already describes the workspace, replacing the stored index with one
    /// built from empty. Asks for confirmation unless `--yes` is given.
    #[arg(long)]
    pub force: bool,

    /// Confirm a forced build without asking. Accepted only together with `--force`.
    #[arg(long, requires = "force")]
    pub yes: bool,

    /// The chunk size in model tokens: the bound on the whole text embedded per chunk, the
    /// passage's header included. A passage exceeding it is represented by several chunks.
    #[arg(long, default_value_t = crate::graph::chunk::DEFAULT_CHUNK_SIZE)]
    pub chunk_size: usize,

    /// The overlap between adjacent chunks of one passage, in model tokens. Must be smaller than
    /// the chunk size.
    #[arg(long, default_value_t = crate::graph::chunk::DEFAULT_CHUNK_OVERLAP)]
    pub chunk_overlap: usize,
}
