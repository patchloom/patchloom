pub mod global;

#[cfg(feature = "cli")]
use crate::cmd::Command;
#[cfg(feature = "cli")]
use clap::Parser;
#[cfg(feature = "cli")]
use global::GlobalFlags;

/// Patchloom: agent-grade repo operations.
#[cfg(feature = "cli")]
#[derive(Debug, Parser)]
#[command(name = "patchloom", version, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,
}
