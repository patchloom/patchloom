//! Transaction lifecycle: format/validate steps, commit/rollback, plan execution.
//!
//! Split for module hygiene (former monofile size-waiver retired). Commit and
//! rollback stay co-located under [`commit`]; plan execution under [`plan_exec`].

// Re-exports are consumed via `crate::tx::lifecycle::*` and `crate::tx` facades.
#![allow(unused_imports)]

mod commit;
mod plan_exec;
mod steps;

pub use commit::{CommitError, RestoreFailGuard, WriteFailGuard};
pub use plan_exec::execute_plan_direct;

pub(crate) use commit::{commit_changes, restore_after_failed_commit};
pub(crate) use plan_exec::validate_and_prepare_plan;
pub(crate) use steps::{
    resolve_plan_cwd, restore_collateral_files, rollback_strict, run_format_steps, run_lifecycle,
    run_validate_steps, snapshot_non_tx_files,
};
