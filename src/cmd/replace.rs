use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct ReplaceArgs {
    /// Text to find.
    #[arg(long)]
    pub from: String,
    /// Text to replace with.
    #[arg(long)]
    pub to: String,
    /// Paths to operate on.
    pub paths: Vec<String>,
    /// Treat --from as a literal string.
    #[arg(long)]
    pub literal: bool,
    /// Treat --from as a regex.
    #[arg(long)]
    pub regex: bool,
}

pub fn run(_args: ReplaceArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("replace: not yet implemented")
}
