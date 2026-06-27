use super::execute::execute_and_collect;
use super::output::{
    TxOutput, build_error_output, build_full_tx_output, describe_exit_status,
    describe_lifecycle_cwd,
};
use super::validate::validate_plan_operations;
use crate::cli::global::GlobalFlags;
use crate::exec;
use crate::plan::{self, Plan};
use crate::write::{WritePolicy, atomic_create_new, atomic_write};
use anyhow::Context;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_LIFECYCLE_TIMEOUT_SECS: u64 = 60;

// ---------------------------------------------------------------------------
// Lifecycle helpers (extracted from run() for testability)
// ---------------------------------------------------------------------------

pub(crate) struct LifecycleError {
    pub(crate) message: String,
    pub(crate) kind: &'static str,
}

/// Format a lifecycle step failure message, appending stderr output if available.
fn lifecycle_failure_msg(header: &str, stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        header.to_string()
    } else {
        format!("{header}: {trimmed}")
    }
}

/// Shared runner for format and validation lifecycle steps.
///
/// `label` is used in error messages (e.g. "format step" or "required validation").
/// `error_label` is the shorter variant for the error branch (e.g. "format step" or "validation").
/// `kind` is the `LifecycleError::kind` string.
/// `fail_on_error` controls whether non-required failures are fatal.
fn run_lifecycle_steps(
    steps: impl Iterator<Item = (String, Option<u64>, bool)>,
    base_cwd: &Path,
    cwd: &Path,
    label: &str,
    error_label: &str,
    kind: &'static str,
) -> Result<(), LifecycleError> {
    let lifecycle_cwd = describe_lifecycle_cwd(base_cwd, cwd);
    for (index, (cmd, timeout, required)) in steps.enumerate() {
        let timeout_secs = timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS);
        let result = exec::run_with_timeout(&cmd, timeout_secs, cwd);
        match result {
            Ok(exec::ShellResult {
                status,
                stderr_head,
            }) if !status.success() => {
                let header = format!(
                    "{label} failed (step {}, {}, cwd: {})",
                    index + 1,
                    describe_exit_status(status),
                    lifecycle_cwd
                );
                let msg = lifecycle_failure_msg(&header, &stderr_head);
                eprintln!("tx: {msg}");
                if required {
                    return Err(LifecycleError { message: msg, kind });
                }
            }
            Err(e) => {
                let msg = format!(
                    "{error_label} error (step {}, cwd: {}): {e}",
                    index + 1,
                    lifecycle_cwd
                );
                eprintln!("tx: {msg}");
                if required {
                    return Err(LifecycleError { message: msg, kind });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn run_format_steps(
    steps: &[plan::FormatStep],
    base_cwd: &Path,
    cwd: &Path,
) -> Result<(), LifecycleError> {
    run_lifecycle_steps(
        steps.iter().map(|s| (s.cmd.clone(), s.timeout, true)),
        base_cwd,
        cwd,
        "format step",
        "format step",
        "format_failed",
    )
}

pub(crate) fn run_validate_steps(
    steps: &[plan::ValidationStep],
    base_cwd: &Path,
    cwd: &Path,
) -> Result<(), LifecycleError> {
    run_lifecycle_steps(
        steps
            .iter()
            .map(|s| (s.cmd.clone(), s.timeout, s.required.unwrap_or(false))),
        base_cwd,
        cwd,
        "required validation",
        "validation",
        "validation_failed",
    )
}

pub(crate) fn rollback_strict(
    changes: &[(PathBuf, String, String)],
    pending: &HashMap<PathBuf, (String, String)>,
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
) {
    let noop_policy = WritePolicy::default();
    for (path, original, _) in changes {
        if !existed_before.contains(path) && !deletions.contains(path) {
            if let Err(e) = std::fs::remove_file(path) {
                eprintln!("tx: rollback: failed to remove {}: {e}", path.display());
            }
        } else if let Err(e) = atomic_write(path, original, &noop_policy) {
            eprintln!("tx: rollback: failed to restore {}: {e}", path.display());
        }
    }
    for path in deletions {
        if let Some((orig, _)) = pending.get(path)
            && existed_before.contains(path)
            && let Err(e) = atomic_write(path, orig, &noop_policy)
        {
            eprintln!(
                "tx: rollback: failed to restore deleted {}: {e}",
                path.display()
            );
        }
    }
}

/// Run format and validation lifecycle steps. Returns `None` on success.
pub(crate) fn run_lifecycle(plan: &Plan, base_cwd: &Path, cwd: &Path) -> Option<LifecycleError> {
    plan.format
        .as_deref()
        .map(|steps| run_format_steps(steps, base_cwd, cwd))
        .unwrap_or(Ok(()))
        .err()
        .or_else(|| {
            plan.validate
                .as_deref()
                .and_then(|steps| run_validate_steps(steps, base_cwd, cwd).err())
        })
}

pub(crate) fn resolve_plan_cwd(base_cwd: &Path, plan_cwd: Option<&str>) -> PathBuf {
    match plan_cwd {
        Some(plan_cwd) => {
            let path = Path::new(plan_cwd);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base_cwd.join(path)
            }
        }
        None => base_cwd.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------
// Direct execution (MCP / in-process callers)
// ---------------------------------------------------------------------------

/// Failure while committing staged changes to disk.
#[derive(Debug)]
pub struct CommitError {
    pub message: String,
    pub rollback_ok: bool,
    pub backup_session: Option<String>,
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommitError {}

fn commit_error(message: impl Into<String>) -> CommitError {
    CommitError {
        message: message.into(),
        rollback_ok: true,
        backup_session: None,
    }
}

thread_local! {
    /// Per-thread test hook; never set in production.
    static FORCE_RESTORE_FAIL: std::sync::atomic::AtomicBool =
        const { std::sync::atomic::AtomicBool::new(false) };
}

/// RAII guard that forces restore failure on the current thread only.
#[doc(hidden)]
pub struct RestoreFailGuard;

impl RestoreFailGuard {
    pub fn engage() -> Self {
        FORCE_RESTORE_FAIL.with(|flag| flag.store(true, std::sync::atomic::Ordering::SeqCst));
        Self
    }
}

impl Drop for RestoreFailGuard {
    fn drop(&mut self) {
        FORCE_RESTORE_FAIL.with(|flag| flag.store(false, std::sync::atomic::Ordering::SeqCst));
    }
}

/// Restore files from a backup session after a failed commit. Returns `true`
/// when restore completed successfully.
pub(crate) fn restore_after_failed_commit(cwd: &Path, timestamp: &str) -> bool {
    if FORCE_RESTORE_FAIL.with(|flag| flag.load(std::sync::atomic::Ordering::SeqCst)) {
        return false;
    }
    crate::backup::restore_session(cwd, timestamp).is_ok()
}

/// Apply pending changes to disk: backup originals, write modified files,
/// delete removed files, finalize backup session.
///
/// If any write fails, restores all already-written files from the backup
/// session before returning [`CommitError`].
pub(crate) fn commit_changes(
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> Result<(), CommitError> {
    let mut backup = crate::backup::BackupSession::new(cwd)
        .map_err(|e| commit_error(format!("starting backup session: {e}")))?;
    for (path, _, _) in changes {
        if deletions.contains(path) {
            backup
                .save_before_delete(path)
                .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
        } else {
            backup
                .save_before_write(path)
                .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
        }
    }
    for path in deletions {
        backup
            .save_before_delete(path)
            .map_err(|e| commit_error(format!("backing up {}: {e}", path.display())))?;
    }

    // Finalize before writes so undo can recover from a mid-commit failure.
    let backup_session = backup
        .finalize()
        .map_err(|e| commit_error(format!("finalizing backup session: {e}")))?;

    let noop_policy = WritePolicy::default();
    let write_result = (|| -> anyhow::Result<()> {
        for (path, _, new_content) in changes {
            if deletions.contains(path) {
                std::fs::remove_file(path)
                    .with_context(|| format!("deleting {}", path.display()))?;
            } else {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                    && !parent.exists()
                {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating directory {}", parent.display()))?;
                }
                if !existed_before.contains(path) {
                    atomic_create_new(path, new_content, &noop_policy)?;
                } else {
                    atomic_write(path, new_content, &noop_policy)?;
                }
            }
        }
        for path in deletions {
            if path.exists() {
                std::fs::remove_file(path)
                    .with_context(|| format!("deleting {}", path.display()))?;
            }
        }
        Ok(())
    })();

    if let Err(e) = write_result {
        let rollback_ok = if let Some(ref ts) = backup_session {
            restore_after_failed_commit(cwd, ts)
        } else {
            true
        };
        return Err(CommitError {
            message: e.to_string(),
            rollback_ok,
            backup_session,
        });
    }

    Ok(())
}

fn config_tx_strict(cwd: &Path) -> Option<bool> {
    crate::config::find_and_load(cwd)
        .map(|(config, _)| config.tx.strict)
        .unwrap_or(None)
}

/// Execute a parsed [`Plan`] directly and return the structured `TxOutput` (PlanReport).
/// Does **not** write to stdout or stderr.
///
/// This is the in-process equivalent used by the library API (`api::execute_plan`)
/// and (via serialization) by the CLI tx command and MCP.
/// Library users get a typed `PlanReport` directly (addresses #811).
#[allow(clippy::result_large_err)]
pub(crate) fn validate_and_prepare_plan(
    plan: &Plan,
    cwd: &Path,
    no_strict: bool,
) -> Result<(PathBuf, bool, GlobalFlags), TxOutput> {
    if plan.version != crate::plan::SCHEMA_VERSION {
        let msg = format!(
            "unsupported plan version '{}' (this build supports version {})",
            plan.version,
            crate::plan::SCHEMA_VERSION
        );
        return Err(build_error_output("parse_error", "parse_error", &msg, None));
    }
    if let Err(e) = validate_plan_operations(plan) {
        return Err(build_error_output(
            "parse_error",
            "parse_error",
            &e.to_string(),
            None,
        ));
    }

    let effective_cwd = resolve_plan_cwd(cwd, plan.cwd.as_deref());
    let config_strict = config_tx_strict(&effective_cwd);
    let strict = plan::effective_strict(plan.strict, config_strict, no_strict);

    let mut global = GlobalFlags::with_cwd(&effective_cwd);
    if let Some((config, _)) = crate::config::find_and_load(&effective_cwd) {
        crate::config::apply_config(&mut global, &config);
    }

    Ok((effective_cwd, strict, global))
}

#[allow(clippy::result_large_err)]
pub fn execute_plan_direct(
    plan: Plan,
    cwd: &Path,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<TxOutput> {
    crate::verbose!(
        "tx: direct plan execution ({} ops, cwd={}, guard={})",
        plan.operations.len(),
        cwd.display(),
        guard.is_some()
    );

    let (effective_cwd, strict, global) = match validate_and_prepare_plan(&plan, cwd, false) {
        Ok(v) => v,
        Err(output) => return Ok(output),
    };

    // PathGuard enforcement for library callers of execute_plan (addresses #755).
    // Upfront check on declared paths using shared helper; dynamic (globs
    // patterns, patch embedded paths) are best-effort or handled by loaders.
    if let Some(g) = guard {
        for op in &plan.operations {
            for p in op.declared_paths() {
                g.check_path(p)
                    .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {}", e))?;
            }
        }
    }

    // Execute operations and collect changes in memory.
    let mut result = match execute_and_collect(&plan, &effective_cwd, &global, true, true) {
        Ok(r) => r,
        Err(e) => {
            return Ok(build_error_output(
                "operation_failed",
                "operation_failed",
                &e.to_string(),
                None,
            ));
        }
    };

    if result.replace_no_matches {
        let output = build_error_output(
            "no_matches",
            "no_matches",
            result.replace_hint.as_deref().unwrap_or(""),
            None,
        );
        return Ok(output);
    }

    if result.no_effective_changes {
        let output = build_full_tx_output("success", &mut result, &effective_cwd);
        return Ok(output);
    }

    // Apply: back up originals, write files.
    if let Err(err) = commit_changes(
        &result.changes,
        &result.deletions,
        &result.existed_before,
        &effective_cwd,
    ) {
        let error_kind = if err.rollback_ok {
            "rollback"
        } else {
            "rollback_failed"
        };
        let output = build_error_output(
            error_kind,
            error_kind,
            &err.message,
            err.backup_session.as_deref(),
        );
        return Ok(output);
    }

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd) {
        if strict {
            rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            let msg = format!("strict mode -- all changes reverted ({})", err.message);
            return Ok(build_error_output(err.kind, "rollback", &msg, None));
        }
        return Ok(build_error_output(err.kind, err.kind, &err.message, None));
    }

    let output = build_full_tx_output("success", &mut result, &effective_cwd);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- lifecycle_failure_msg ----

    #[test]
    fn lifecycle_failure_msg_no_stderr() {
        assert_eq!(lifecycle_failure_msg("step failed", ""), "step failed");
    }

    #[test]
    fn lifecycle_failure_msg_whitespace_only_stderr() {
        assert_eq!(
            lifecycle_failure_msg("step failed", "  \n  "),
            "step failed"
        );
    }

    #[test]
    fn lifecycle_failure_msg_with_stderr() {
        let msg = lifecycle_failure_msg("step failed", "  error: bad input\n");
        assert_eq!(msg, "step failed: error: bad input");
    }

    // ---- resolve_plan_cwd ----

    #[test]
    fn resolve_plan_cwd_none() {
        let base = Path::new("/base/dir");
        assert_eq!(resolve_plan_cwd(base, None), PathBuf::from("/base/dir"));
    }

    #[test]
    fn resolve_plan_cwd_relative() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_plan_cwd(base, Some("sub/path")),
            PathBuf::from("/base/dir/sub/path")
        );
    }

    #[test]
    fn resolve_plan_cwd_absolute() {
        let base = Path::new("/base/dir");
        assert_eq!(
            resolve_plan_cwd(base, Some("/other/dir")),
            PathBuf::from("/other/dir")
        );
    }

    // ---- CommitError ----

    #[test]
    fn commit_error_display() {
        let err = CommitError {
            message: "write failed".into(),
            rollback_ok: true,
            backup_session: None,
        };
        assert_eq!(format!("{err}"), "write failed");
    }

    #[test]
    fn commit_error_helper() {
        let err = commit_error("test message");
        assert_eq!(err.message, "test message");
        assert!(err.rollback_ok);
        assert!(err.backup_session.is_none());
    }

    // ---- RestoreFailGuard ----

    #[test]
    fn restore_fail_guard_toggles_flag() {
        // Before engaging, restore should succeed (flag is false).
        assert!(!FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));

        {
            let _guard = RestoreFailGuard::engage();
            assert!(FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));
        }

        // After guard is dropped, flag is reset.
        assert!(!FORCE_RESTORE_FAIL.with(|f| f.load(std::sync::atomic::Ordering::SeqCst)));
    }

    // ---- LifecycleError ----

    #[test]
    fn lifecycle_error_fields() {
        let err = LifecycleError {
            message: "validation step 1 failed".into(),
            kind: "validation_failed",
        };
        assert_eq!(err.message, "validation step 1 failed");
        assert_eq!(err.kind, "validation_failed");
    }

    // ---- run_lifecycle ----

    #[test]
    fn run_lifecycle_no_steps() {
        let plan = Plan {
            version: crate::plan::SCHEMA_VERSION.to_string(),
            operations: Vec::new(),
            format: None,
            validate: None,
            cwd: None,
            strict: None,
            write_policy: None,
        };
        let cwd = Path::new("/tmp");
        assert!(run_lifecycle(&plan, cwd, cwd).is_none());
    }
}
