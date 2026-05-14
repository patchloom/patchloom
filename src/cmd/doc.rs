use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct DocArgs {
    #[command(subcommand)]
    pub action: DocAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum DocAction {
    /// Read a value at a key path.
    Get { file: String, selector: String },
    /// Check whether a key path exists.
    Has { file: String, selector: String },
    /// List object keys at a path.
    Keys { file: String, selector: String },
    /// Count items in an array or object.
    Len { file: String, selector: String },
    /// Set or create a value at a key path.
    Set {
        file: String,
        selector: String,
        value: String,
    },
    /// Remove a key path.
    Delete { file: String, selector: String },
    /// Merge a partial object from stdin or argument.
    Merge {
        file: String,
        #[arg(long)]
        stdin: bool,
        #[arg(long)]
        value: Option<String>,
    },
    /// Append to an array.
    Append {
        file: String,
        selector: String,
        value: String,
    },
    /// Prepend to an array.
    Prepend {
        file: String,
        selector: String,
        value: String,
    },
    /// Filter array items by predicate.
    Select { file: String, selector: String },
    /// Update all matching nodes.
    Update {
        file: String,
        selector: String,
        value: String,
    },
    /// Move or rename a key path.
    Move {
        file: String,
        from: String,
        to: String,
    },
    /// Ensure a value exists (idempotent set).
    Ensure {
        file: String,
        selector: String,
        value: String,
    },
}

pub fn run(_args: DocArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("doc: not yet implemented")
}
