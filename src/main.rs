//! The `c10r` binary entrypoint: parse the CLI and dispatch to the command handlers.

use anyhow::Result;
use clap::Parser;

use silent_cartographer::cli::{Cli, Command};
use silent_cartographer::commands;

fn main() -> Result<()> {
    // Set a descriptive process title so hosts running many processes can tell c10r apart.
    proctitle::set_title("c10r");

    let cli = Cli::parse();
    let root = std::path::PathBuf::from(".");

    match &cli.command {
        Command::Get(args) => {
            let out = commands::run_get(
                &cli.db,
                &root,
                &default_analyzer(),
                args.reference.as_deref(),
                args.at.as_deref(),
                args.detail.into(),
                cli.json,
            )?;
            println!("{out}");
        }
        Command::Trace(args) => {
            let out = commands::run_trace(
                &cli.db,
                &root,
                &default_analyzer(),
                &args.reference,
                args.relation.into(),
                args.depth,
                cli.json,
            )?;
            println!("{out}");
        }
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
            println!(
                "built: aligned={} (exact={} crate_root={} operator_desugar={} module_span={} \
                 self_keyword={}) text_mismatch={} semantic_only={} duplicate_ambiguous={} syntax_only={}",
                accounting.aligned_total(),
                accounting.aligned_exact,
                accounting.aligned_crate_root,
                accounting.aligned_operator_desugar,
                accounting.aligned_module_span,
                accounting.aligned_self_keyword,
                accounting.text_mismatch,
                accounting.semantic_only,
                accounting.duplicate_ambiguous,
                accounting.syntax_only
            );
        }
        Command::Status(args) => {
            let out = commands::run_status(
                &cli.db,
                &root,
                &default_analyzer(),
                cli.json,
                args.discrepancies,
                args.all,
                args.duplicates,
            )?;
            println!("{out}");
        }
    }
    Ok(())
}

/// The default `rust-analyzer` executable used by query commands for freshness/version detection.
fn default_analyzer() -> String {
    "rust-analyzer".to_string()
}
