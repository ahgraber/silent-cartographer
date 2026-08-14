//! The `c10r` binary entrypoint: parse the CLI, dispatch to the command handlers, and map the
//! outcome onto the exit-code taxonomy.
//!
//! Answers go to standard output; diagnostics go to standard error. Every failure is classified into
//! the closed exit-code taxonomy in one place here.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};

use silent_cartographer::cli::{Cli, Command, CompletionsArgs, HookAction};
use silent_cartographer::commands;
use silent_cartographer::exit::{ExitCode, classify};
use silent_cartographer::manifest;
use silent_cartographer::render;

fn main() {
    // Set a descriptive process title so hosts running many processes can tell c10r apart.
    proctitle::set_title("c10r");

    // clap reports its own usage errors (unknown flag, out-of-set enum value) on standard error and
    // exits with the usage code before dispatch, so argument validation always precedes any side
    // effect. The raw `ArgMatches` is kept so the dispatch can tell an explicitly-typed content bound
    // from its default (`ValueSource`), which the modal `--max-lines`/`--from` teaching errors need.
    let matches = Cli::command().get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("clap validated the invocation");

    // `doctor` is dispatched ahead of the shared `run` path: its report belongs on standard output
    // even when it exits with the indexer/setup-failure code, which the shared `Ok`-prints-and-exits
    // / `Err`-diagnoses-and-exits pattern below does not express.
    if let Command::Doctor = cli.command {
        run_doctor(&cli);
    }

    // `completions` is dispatched ahead of the shared `run` path too: it writes a script directly to
    // standard output and exits, rather than returning a machine answer through the `Ok`-prints /
    // `Err`-diagnoses pattern below.
    if let Command::Completions(args) = &cli.command {
        run_completions(&matches, args);
    }

    match run(&cli, &matches) {
        Ok(answer) => {
            println!("{answer}");
            std::process::exit(ExitCode::Success.code());
        }
        Err(error) => {
            // The diagnostic can embed user-supplied text — a `--db`/`--at` path, a reference string —
            // so it is sanitized before reaching the terminal, the same escape-injection guard the
            // answer rendering applies to structural fields.
            eprintln!("{}", render::sanitize(&format!("{error:#}")));
            std::process::exit(classify(&error).code());
        }
    }
}

/// Run `doctor` and exit: print the readiness report to standard output unconditionally, then exit
/// with the success code when every required indexer is present and responsive, or the
/// indexer/setup-failure code with a diagnostic on standard error naming what is not ready — a tool
/// that is absent or present-but-unresponsive.
fn run_doctor(cli: &Cli) -> ! {
    let statuses = commands::run_doctor();
    let report = if cli.json {
        serde_json::to_string_pretty(&statuses).unwrap_or_else(|_| "[]".to_string())
    } else {
        commands::render_doctor_report(&statuses)
    };
    println!("{report}");

    let not_ready: Vec<&str> = statuses
        .iter()
        .filter(|s| !s.is_ready())
        .map(|s| s.name.as_str())
        .collect();
    if not_ready.is_empty() {
        std::process::exit(ExitCode::Success.code());
    }
    eprintln!(
        "{}",
        render::sanitize(&format!("indexer setup failure: {}", not_ready.join(", ")))
    );
    std::process::exit(ExitCode::IndexerSetup.code());
}

/// Run `completions` and exit: emit a shell completion script for `args.shell` on standard output,
/// generated from the same `Cli::command()` tree that `--help` and `manifest` walk, so the completed
/// surface cannot drift from the real one. A script is not a machine answer, so an explicit `--json`
/// is rejected as a usage error before anything is written, mirroring the modal content-bound
/// teaching-error convention (`ValueSource::CommandLine` distinguishes an explicit flag from its
/// default).
fn run_completions(matches: &ArgMatches, args: &CompletionsArgs) -> ! {
    if matches.value_source("json") == Some(ValueSource::CommandLine) {
        eprintln!("`--json` is not meaningful for `completions`: it emits a shell script, not a machine answer");
        std::process::exit(ExitCode::Usage.code());
    }
    let mut cmd = Cli::command();
    clap_complete::generate(args.shell, &mut cmd, "c10r", &mut std::io::stdout());
    std::process::exit(ExitCode::Success.code());
}

/// The default `rust-analyzer` executable query commands probe for freshness/version detection.
const DEFAULT_ANALYZER: &str = "rust-analyzer";

/// Whether a subcommand flag was typed on the command line, as opposed to filled by its default —
/// the distinction the modal content-bound teaching errors turn on. A flag absent from the invoked
/// subcommand reads as not explicit.
fn flag_explicit(matches: &ArgMatches, id: &str) -> bool {
    matches
        .subcommand()
        .and_then(|(_, sub)| sub.value_source(id))
        .is_some_and(|source| source == ValueSource::CommandLine)
}

/// Dispatch a parsed invocation to its command handler, returning the answer destined for standard
/// output.
fn run(cli: &Cli, matches: &ArgMatches) -> Result<String> {
    let root = PathBuf::from(".");
    // The color gate is decided once, at the process edge, from the terminal state of standard output.
    let styled = render::should_style(cli.color, cli.json, std::io::stdout().is_terminal());
    match &cli.command {
        Command::Get(args) => commands::run_get(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            args.reference.as_deref(),
            args.at.as_deref(),
            args.detail.into(),
            args.max_lines,
            args.from,
            flag_explicit(matches, "max_lines"),
            flag_explicit(matches, "from"),
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Trace(args) => commands::run_trace(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            &args.reference,
            args.relation.into(),
            args.depth,
            args.detail.map(Into::into),
            args.max_lines,
            flag_explicit(matches, "max_lines"),
            args.order.into(),
            flag_explicit(matches, "order"),
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Find(args) => commands::run_find(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            &args.fragment,
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Search(args) => commands::run_search(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            &args.query,
            args.detail.into(),
            args.max_lines,
            flag_explicit(matches, "max_lines"),
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Similar(args) => commands::run_similar(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            args.reference.as_deref(),
            args.at.as_deref(),
            args.detail.into(),
            args.max_lines,
            flag_explicit(matches, "max_lines"),
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Impact(args) => commands::run_impact(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            cli.workspace.as_deref(),
            args.revspec.as_deref(),
            args.staged,
            args.depth,
            args.order.into(),
            &args.paths,
            args.paging.limit,
            args.paging.cursor.as_deref(),
            cli.json,
            styled,
        ),
        Command::Build(args) => {
            let accounting = commands::run_build(
                &cli.db,
                cli.workspace.as_deref(),
                &args.root,
                &args.rust_analyzer,
                "scip-python",
                args.environment.as_deref(),
                args.language.map(Into::into),
            )?;
            // Under `--json` the standard-output answer is the structured machine projection of the
            // same accounting the human line renders.
            if cli.json {
                Ok(serde_json::to_string_pretty(&commands::build_accounting_json(
                    &accounting,
                ))?)
            } else {
                Ok(commands::build_accounting_line(&accounting))
            }
        }
        Command::Status(args) => commands::run_status(
            &cli.db,
            &root,
            DEFAULT_ANALYZER,
            cli.json,
            args.discrepancies,
            args.all,
            args.duplicates,
        ),
        Command::Doctor => unreachable!("doctor is dispatched before run() in main"),
        Command::Completions(_) => unreachable!("completions is dispatched before run() in main"),
        Command::Cache => {
            let outcome = commands::run_cache(&cli.db)?;
            Ok(commands::render_cache_report(&outcome, cli.json))
        }
        Command::Hooks(args) => {
            let outcome = match args.action {
                HookAction::Install => commands::run_hooks_install(&root, &cli.db, cli.workspace.as_deref())?,
            };
            Ok(commands::render_hook_report(&outcome, cli.json))
        }
        // The structural index is always machine-readable JSON: `--json` is accepted (it is
        // global) but changes nothing, since manifest's answer is the machine answer.
        Command::Manifest => {
            let index = commands::index_state(&cli.db, &root, DEFAULT_ANALYZER);
            let manifest = manifest::to_manifest_json(&Cli::command(), index);
            Ok(serde_json::to_string_pretty(&manifest)?)
        }
    }
}
