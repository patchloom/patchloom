#![deny(unsafe_code)]

pub mod cli;
pub mod cmd;
pub mod diff;
pub mod exit;
pub(crate) mod files;
pub(crate) mod ops;
pub mod plan;
pub mod selector;
pub mod write;

pub(crate) use files::*;

use clap::Parser;
use cli::Cli;

/// Run the patchloom CLI. Returns the exit code as a u8.
pub fn run() -> anyhow::Result<u8> {
    let cli = Cli::parse();
    cmd::dispatch(cli)
}
