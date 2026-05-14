use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct PatchArgs {
    #[command(subcommand)]
    pub action: PatchAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum PatchAction {
    /// Check whether a patch applies cleanly.
    Check {
        /// Path to a diff file.
        #[arg(long)]
        file: Option<String>,
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
    /// Apply a unified diff.
    Apply {
        /// Path to a diff file.
        #[arg(long)]
        file: Option<String>,
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
}

pub fn run(_args: PatchArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("patch: not yet implemented")
}
