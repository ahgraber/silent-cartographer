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

use silent_cartographer::cli::{CacheAction, Cli, Command, CompletionsArgs, HookAction, HooksArgs};
use silent_cartographer::commands;
use silent_cartographer::exit::{ExitCode, Failure, classify};
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

    // `hooks`' cross-action argument validation happens here, ahead of every side effect: an event,
    // `--root`, and the trailing git arguments belong only to `run`, and `--overwrite` only to
    // `install`, whichever action the shared `run` path below would otherwise attempt first.
    //
    // `hooks run` is then dispatched ahead of the shared path too, like `doctor` and `completions`:
    // it must print nothing when the refresh rules skip the event, and must always exit with the
    // success code, which the shared `Ok`-prints / `Err`-diagnoses pattern below does not express.
    if let Command::Hooks(args) = &cli.command {
        if let Err(failure) = validate_hooks_args(args) {
            let error = anyhow::Error::from(failure);
            eprintln!("{}", render::sanitize(&format!("{error:#}")));
            std::process::exit(classify(&error).code());
        }
        if args.action == HookAction::Run {
            run_hooks_run(&cli, args);
        }
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
    let report = commands::run_doctor();
    let output = if cli.json {
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
    } else {
        commands::render_doctor_report(&report)
    };
    println!("{output}");

    let not_ready: Vec<&str> = report
        .indexers
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

/// Reject a `hooks` argument given to an action it does not belong to, before `hooks` has any side
/// effect: an event, `--root`, and the trailing git arguments belong only to `run`; `--overwrite`
/// belongs only to `install`. `run` requires both an event and `--root`.
fn validate_hooks_args(args: &HooksArgs) -> Result<(), Failure> {
    match args.action {
        HookAction::Run => {
            if args.event.is_none() {
                return Err(Failure::Usage("`hooks run` requires an event".to_string()));
            }
            if args.root.is_none() {
                return Err(Failure::Usage("`hooks run` requires --root".to_string()));
            }
        }
        HookAction::Install | HookAction::Uninstall => {
            if args.event.is_some() {
                return Err(Failure::Usage("an event applies only to `hooks run`".to_string()));
            }
            if args.root.is_some() {
                return Err(Failure::Usage("--root applies only to `hooks run`".to_string()));
            }
            if !args.git_args.is_empty() {
                return Err(Failure::Usage(
                    "trailing arguments after `--` apply only to `hooks run`".to_string(),
                ));
            }
        }
    }
    if args.overwrite && args.action != HookAction::Install {
        return Err(Failure::Usage(
            "--overwrite applies only to `hooks install`".to_string(),
        ));
    }
    Ok(())
}

/// Run `hooks run` and exit: see the dispatch comment in `main` for why this runs ahead of the shared
/// `run` path. `validate_hooks_args` has already confirmed `args.event` and `args.root` are present.
fn run_hooks_run(cli: &Cli, args: &HooksArgs) -> ! {
    let event = args.event.expect("validate_hooks_args confirmed an event is present");
    let root = args
        .root
        .as_deref()
        .expect("validate_hooks_args confirmed --root is present");
    if let Some(line) = commands::run_hooks_run(event, root, &cli.db, cli.workspace.as_deref(), &args.git_args) {
        println!("{line}");
    }
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
            cli.position,
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
            cli.position,
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
            cli.position,
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
            cli.position,
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
            cli.position,
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
            cli.position,
        ),
        Command::Build(args) => {
            let outcome = commands::run_build(
                &cli.db,
                cli.workspace.as_deref(),
                &args.root,
                &args.rust_analyzer,
                "scip-python",
                args.environment.as_deref(),
                args.language.map(Into::into),
                args.force,
                &silent_cartographer::graph::chunk::ChunkParams {
                    chunk_size: args.chunk_size,
                    overlap: args.chunk_overlap,
                },
            )?;
            // Under `--json` the standard-output answer is the structured machine projection of the
            // same accounting the human line renders, carrying whether the store was rewritten.
            if cli.json {
                Ok(serde_json::to_string_pretty(&commands::build_outcome_json(&outcome))?)
            } else {
                Ok(commands::build_outcome_line(&outcome))
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
            cli.position,
        ),
        Command::Doctor => unreachable!("doctor is dispatched before run() in main"),
        Command::Completions(_) => unreachable!("completions is dispatched before run() in main"),
        Command::Cache(args) => match args.action {
            CacheAction::Clear => {
                let outcome = commands::run_cache(&cli.db)?;
                Ok(commands::render_cache_report(&outcome, cli.json))
            }
            CacheAction::Dir => {
                let outcome = commands::run_cache_dir(&cli.db);
                Ok(commands::render_cache_dir_report(&outcome, cli.json))
            }
            CacheAction::Size => {
                let outcome = commands::run_cache_size(&cli.db)?;
                Ok(commands::render_cache_size_report(&outcome, cli.json))
            }
        },
        Command::Hooks(args) => {
            let outcome = match args.action {
                HookAction::Install => {
                    commands::run_hooks_install(&root, &cli.db, cli.workspace.as_deref(), args.overwrite)?
                }
                HookAction::Uninstall => commands::run_hooks_uninstall(&root, &cli.db, cli.workspace.as_deref())?,
                HookAction::Run => unreachable!("hooks run is dispatched before run() in main"),
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
