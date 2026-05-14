use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct HygieneArgs {
    #[command(subcommand)]
    pub action: HygieneAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum HygieneAction {
    /// Report newline, EOL, and whitespace issues.
    Check { paths: Vec<String> },
    /// Apply normalization fixes.
    Fix { paths: Vec<String> },
}

pub fn run(_args: HygieneArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("hygiene: not yet implemented")
}
