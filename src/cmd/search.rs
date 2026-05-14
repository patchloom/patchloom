use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Pattern to search for.
    pub pattern: String,
    /// Paths to search in.
    pub paths: Vec<String>,
    /// Treat pattern as a literal string.
    #[arg(long)]
    pub literal: bool,
    /// Treat pattern as a regex (default).
    #[arg(long)]
    pub regex: bool,
    /// Lines of context around matches.
    #[arg(long, short = 'C')]
    pub context: Option<usize>,
    /// Only print file paths with matches.
    #[arg(long)]
    pub files_with_matches: bool,
    /// Only print match counts per file.
    #[arg(long)]
    pub count: bool,
}

pub fn run(_args: SearchArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("search: not yet implemented")
}
