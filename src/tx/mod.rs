// Most items in `tx` are consumed by CLI commands or the `files`-feature
// library API (`crate::api`).  When neither feature is active the types
// are still compiled (Plan lives here unconditionally) but have no
// callers, so we suppress the warnings at the module level.
//
// NOTE: this blanket means `make test-library-hygiene` (which runs
// clippy under `--no-default-features --features "ast,files"`) will
// NOT flag genuinely dead items in this module.  Add per-item
// `#[cfg(…)]` gates or targeted `#[allow(dead_code)]` with a
// justification comment for items that are only used under a specific
// feature set.
#![cfg_attr(not(feature = "cli"), allow(dead_code, unused_imports))]

pub mod context;
pub mod engine;
mod execute;
mod lifecycle;
mod output;
mod replace_op;
mod search_op;
mod validate;
#[cfg(feature = "ast")]
pub(crate) mod verify;

// Operation handler modules (extracted from the former monofile execute module).
#[cfg(feature = "ast")]
mod ast_op;
mod md_op;
mod patch_op;
mod tidy_op;

// Re-export all pub and pub(crate) items so `crate::tx::*` paths continue to work.

// output.rs
pub(crate) use output::{
    ReplaceMatchMeta, TxExecResult, build_applied_with_error_output, build_error_output,
    build_full_tx_output, format_error_with_backup_hint, match_mode_label,
};
pub use output::{
    TxChange, TxDocMutation, TxLintResult, TxOutput, TxReadResult, TxRefused, TxSearchMatch,
    TxSearchResult, exit_code_from_tx_output,
};

// validate.rs (test-only re-exports for src/cmd/tx.rs tests)
#[cfg(test)]
pub(crate) use validate::validate_operation;

// execute/ (dispatcher + file_ops + policy)
pub(crate) use execute::execute_and_collect;
// Test-only re-exports for src/cmd/tx.rs tests
#[cfg(test)]
pub(crate) use execute::TxStateFixture;
#[cfg(test)]
pub(crate) use execute::{path_err, read_and_probe, read_file_content, update_file_content};

// lifecycle.rs
pub use lifecycle::{CommitError, RestoreFailGuard, execute_plan_direct};
pub(crate) use lifecycle::{
    commit_changes, rollback_strict, run_lifecycle, validate_and_prepare_plan,
};
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) use lifecycle::{restore_collateral_files, snapshot_non_tx_files};

#[cfg(feature = "cli")]
use clap::Args;

#[cfg(feature = "cli")]
#[derive(Debug, Args)]
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
