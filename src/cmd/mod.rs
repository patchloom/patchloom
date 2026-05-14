pub mod doc;
pub mod hygiene;
pub mod md;
pub mod patch;
pub mod replace;
pub mod search;
pub mod tx;

use crate::cli::Cli;
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fast literal or regex search across a repo.
    Search(search::SearchArgs),
    /// Mechanical string replacement with diff preview.
    Replace(replace::ReplaceArgs),
    /// Preview or apply unified diffs safely.
    Patch(patch::PatchArgs),
    /// Markdown section-aware operations.
    Md(md::MdArgs),
    /// Parser-backed JSON, YAML, and TOML operations.
    Doc(doc::DocArgs),
    /// Final newline, line ending, and whitespace normalization.
    Hygiene(hygiene::HygieneArgs),
    /// Execute a multi-operation plan atomically.
    Tx(tx::TxArgs),
}

pub fn dispatch(cli: Cli) -> anyhow::Result<u8> {
    match cli.command {
        Command::Search(args) => search::run(args, &cli.global),
        Command::Replace(args) => replace::run(args, &cli.global),
        Command::Patch(args) => patch::run(args, &cli.global),
        Command::Md(args) => md::run(args, &cli.global),
        Command::Doc(args) => doc::run(args, &cli.global),
        Command::Hygiene(args) => hygiene::run(args, &cli.global),
        Command::Tx(args) => tx::run(args, &cli.global),
    }
}
