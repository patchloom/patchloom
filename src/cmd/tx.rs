use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
pub struct TxArgs {
    /// Path to a plan JSON file, or `-` for stdin.
    #[arg(long)]
    pub plan: String,
}

pub fn run(_args: TxArgs, _global: &GlobalFlags) -> anyhow::Result<u8> {
    anyhow::bail!("tx: not yet implemented")
}
