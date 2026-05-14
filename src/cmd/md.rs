use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct MdArgs {
    #[command(subcommand)]
    pub action: MdAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum MdAction {
    /// Replace a heading section.
    ReplaceSection {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        /// Read replacement content from stdin.
        #[arg(long)]
        stdin: bool,
        /// Replacement content as argument.
        #[arg(long)]
        content: Option<String>,
    },
    /// Insert content after a heading.
    InsertAfterHeading {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        content: Option<String>,
    },
    /// Add a bullet under a heading if not already present.
    UpsertBullet {
        #[arg(long)]
        file: String,
        #[arg(long)]
        heading: String,
        #[arg(long)]
        bullet: String,
    },
    /// Remove duplicate headings.
    DedupeHeadings {
        #[arg(long)]
        file: String,
    },
    /// Lint common AGENTS.md problems.
    LintAgents {
        #[arg(long)]
        file: String,
    },
}

pub fn run(_args: MdArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("md: not yet implemented")
}
