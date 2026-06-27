#![cfg_attr(not(feature = "cli"), allow(dead_code, unused_imports))]

pub mod engine;
mod execute;
mod lifecycle;
mod output;
mod replace_op;
mod search_op;
mod validate;
pub(crate) mod verify;

// Re-export all pub and pub(crate) items so `crate::tx::*` paths continue to work.

// output.rs
pub use output::{
    TxChange, TxLintResult, TxOutput, TxReadResult, TxSearchMatch, TxSearchResult,
    exit_code_from_tx_output,
};
pub(crate) use output::{
    TxExecResult, build_error_output, build_full_tx_output, format_error_with_backup_hint,
};

// validate.rs (test-only re-exports for src/cmd/tx.rs tests)
#[cfg(test)]
pub(crate) use validate::validate_operation;

// execute.rs
pub(crate) use execute::execute_and_collect;
// Test-only re-exports for src/cmd/tx.rs tests
#[cfg(test)]
pub(crate) use execute::{path_err, read_and_probe, read_file_content, update_file_content};

// lifecycle.rs
pub use lifecycle::{CommitError, RestoreFailGuard, execute_plan_direct};
pub(crate) use lifecycle::{
    commit_changes, rollback_strict, run_lifecycle, validate_and_prepare_plan,
};

#[cfg(feature = "cli")]
use clap::Args;

#[cfg(feature = "cli")]
#[derive(Debug, Args)]
#[non_exhaustive]
#[command(after_help = "\
EXAMPLES:
  patchloom tx plan.json
  patchloom tx plan.json --apply
  patchloom tx plan.yaml --check --json")]
pub struct TxArgs {
    // ref:tx-mode:plan-stdin
    /// Path to a plan file (JSON/YAML/TOML), or `-` for stdin.
    pub plan: String,
    // ref:tx-mode:plan-yaml
    /// Plan format when reading from stdin (json, yaml, toml). Auto-detected from file extension otherwise.
    #[arg(long)]
    pub plan_format: Option<String>,
    /// Disable strict rollback on format/validate failure.
    #[arg(long)]
    pub no_strict: bool,

    /// Verify symbol counts before and after execution. Repeatable.
    /// Examples: `--verify="kind=function,attr=test"`, `--verify=unique_names`.
    #[arg(long)]
    pub verify: Vec<String>,

    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}
