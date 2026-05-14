pub mod global;

use crate::cmd::Command;
use clap::Parser;
use global::GlobalFlags;

/// Patchloom: agent-grade repo operations.
#[derive(Debug, Parser)]
#[command(name = "patchloom", version, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,
}
