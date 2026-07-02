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
                cli.json,
            )?;
            println!("{out}");
        }
        Command::Build(args) => {
            let accounting = commands::run_build(&cli.db, &cli.workspace, &args.root, &args.rust_analyzer)?;
            println!(
                "built: aligned={} text_mismatch={} semantic_only={} syntax_only={}",
                accounting.aligned, accounting.text_mismatch, accounting.semantic_only, accounting.syntax_only
            );
        }
        Command::Status => {
            let out = commands::run_status(&cli.db, &root, &default_analyzer(), cli.json)?;
            println!("{out}");
        }
    }
    Ok(())
}

/// The default `rust-analyzer` executable used by query commands for freshness/version detection.
fn default_analyzer() -> String {
    "rust-analyzer".to_string()
}
