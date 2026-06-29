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
#[cfg(any(feature = "cli", feature = "files"))]
use ignore::WalkBuilder;
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
        crate::verbose!(
            "tx: running {} step {}: {:?} (timeout={}s, required={})",
            label,
            index + 1,
            cmd,
            timeout.unwrap_or(DEFAULT_LIFECYCLE_TIMEOUT_SECS),
            required
        );
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

/// Maximum file size (in bytes) to include in the collateral snapshot.
/// Files above this threshold are extremely unlikely to be reformatted by
/// standard formatters and snapshotting them wastes memory.
const COLLATERAL_SNAPSHOT_MAX_SIZE: u64 = 1_048_576; // 1 MiB

/// Snapshot the content of non-binary text files under `cwd` that are NOT
/// already tracked by the transaction. This captures the pre-format state of
/// files that a format step (e.g. `cargo fmt`) might modify as a side effect.
///
/// Only called when `strict` mode is active and the plan has format or
/// validate steps, so the cost is opt-in.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn snapshot_non_tx_files(
    cwd: &Path,
    tx_paths: &HashSet<PathBuf>,
) -> HashMap<PathBuf, String> {
    let mut snapshot = HashMap::new();
    crate::verbose!(
        "tx: collateral snapshot: walking {} (tx_paths: {})",
        cwd.display(),
        tx_paths.len()
    );
    let walker = WalkBuilder::new(cwd).hidden(false).build();
    for entry in walker.flatten() {
        let path = entry.path().to_path_buf();
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if tx_paths.contains(&path) {
            crate::verbose!(
                "tx: collateral snapshot: skipping tx file {}",
                path.display()
            );
            continue;
        }
        // Skip files above the size threshold.
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() > COLLATERAL_SNAPSHOT_MAX_SIZE
        {
            crate::verbose!(
                "tx: collateral snapshot: skipping large file {} ({} bytes)",
                path.display(),
                meta.len()
            );
            continue;
        }
        // Read bytes and skip binary files.
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if crate::files::is_binary(&bytes) {
            continue;
        }
        if let Ok(content) = String::from_utf8(bytes) {
            crate::verbose!(
                "tx: collateral snapshot: captured {} ({} bytes)",
                path.display(),
                content.len()
            );
            snapshot.insert(path, content);
        }
    }
    crate::verbose!("tx: collateral snapshot: {} files captured", snapshot.len());
    snapshot
}

/// Restore any files that were modified by format/validate steps but were
/// not part of the transaction. Compares current content against the
/// snapshot and writes back the original content for any files that changed.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn restore_collateral_files(snapshot: &HashMap<PathBuf, String>) {
    let noop_policy = WritePolicy::default();
    let mut restored = 0usize;
    for (path, original) in snapshot {
        let current = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if current != *original {
            crate::verbose!(
                "tx: collateral restore: reverting {} (changed by formatter)",
                path.display()
            );
            if let Err(e) = atomic_write(path, original, &noop_policy) {
                eprintln!(
                    "tx: rollback: failed to restore collateral file {}: {e}",
                    path.display()
                );
            } else {
                restored += 1;
            }
        }
    }
    if restored > 0 {
        crate::verbose!("tx: collateral restore: reverted {} file(s)", restored);
    }
}

pub(crate) fn rollback_strict(
    changes: &[(PathBuf, String, String)],
    pending: &HashMap<PathBuf, (String, String)>,
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
) {
    let noop_policy = WritePolicy::default();
    for (path, original, _) in changes {
        if !existed_before.contains(path) {
            // File was created during this tx. Whether it was also deleted
            // does not matter: it should not exist after rollback.
            if let Err(e) = std::fs::remove_file(path) {
                eprintln!(
                    "tx: rollback: failed to remove created file {}: {e}",
                    path.display()
                );
            }
        } else if !deletions.contains(path) {
            // File existed before and was modified (not deleted): restore.
            if let Err(e) = atomic_write(path, original, &noop_policy) {
                eprintln!("tx: rollback: failed to restore {}: {e}", path.display());
            }
        }
        // If existed_before AND in deletions: handled by the deletions loop below.
    }
    for path in deletions {
        if let Some((orig, _)) = pending.get(path)
            && existed_before.contains(path)
        {
            // Ensure parent directory exists before restoring; the directory
            // may have been removed if the deletion was the last file in it.
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
                && !parent.exists()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!(
                    "tx: rollback: failed to create dir {}: {e}",
                    parent.display()
                );
                continue;
            }
            if let Err(e) = atomic_write(path, orig, &noop_policy) {
                eprintln!(
                    "tx: rollback: failed to restore deleted {}: {e}",
                    path.display()
                );
            }
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
        // Skip files already backed up in the changes loop above (#1111).
        if changes.iter().any(|(p, _, _)| p == path) {
            continue;
        }
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

pub fn execute_plan_direct(
    plan: Plan,
    cwd: &Path,
    guard: Option<&crate::containment::PathGuard>,
) -> anyhow::Result<TxOutput> {
    // Expand for_each (glob-driven batch) before anything else.
    #[cfg(feature = "cli")]
    let mut plan = plan;
    #[cfg(feature = "cli")]
    if plan.for_each.is_some() {
        crate::plan::expand_for_each(&mut plan, cwd)?;
    }

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
    if let Some(g) = guard {
        // Defense-in-depth: reject plans whose cwd would escape the guard's
        // workspace root. The MCP handler strips plan.cwd, but library
        // callers might not.
        if plan.cwd.is_some() {
            let canon_cwd = effective_cwd
                .canonicalize()
                .unwrap_or_else(|_| effective_cwd.clone());
            if !canon_cwd.starts_with(g.canon_root()) {
                anyhow::bail!(
                    "plan cwd '{}' escapes workspace root '{}'",
                    effective_cwd.display(),
                    g.root().display()
                );
            }
        }
        // Upfront check on declared paths using shared helper; dynamic (globs
        // patterns, patch embedded paths) are best-effort or handled by loaders.
        for op in &plan.operations {
            for p in op.declared_paths() {
                g.check_path(p)
                    .map_err(|e| anyhow::anyhow!("path rejected by workspace guard: {}", e))?;
            }
        }
    }

    // Pre-execution verification snapshot.
    #[cfg(feature = "ast")]
    let verify_before = if let Some(ref checks) = plan.verify {
        if !checks.is_empty() {
            let affected = super::verify::affected_file_paths(&plan, &effective_cwd);
            checks
                .iter()
                .map(|check| {
                    let snap = super::verify::snapshot_symbols(&affected, check);
                    (check.clone(), snap)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

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

    // Post-execution verification against pending content.
    #[cfg(feature = "ast")]
    if !verify_before.is_empty() {
        let affected = super::verify::affected_file_paths(&plan, &effective_cwd);
        let mut messages = Vec::new();
        let mut any_failed = false;
        for (check, before_snap) in &verify_before {
            let after_snap =
                super::verify::snapshot_symbols_from_pending(&affected, &result.pending, check);
            let vr =
                super::verify::compare_snapshots(before_snap, &after_snap, check, &effective_cwd);
            messages.push(vr.message.clone());
            if !vr.passed {
                any_failed = true;
            }
        }
        if any_failed {
            let summary = messages.join("\n");
            let msg = format!("verification failed, changes not applied:\n{summary}");
            return Ok(build_error_output(
                "verification_failed",
                "verification_failed",
                &msg,
                None,
            ));
        }
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

    // Snapshot non-tx files before format/validate steps so we can restore
    // collateral changes on strict rollback (#1111.7).
    #[cfg(any(feature = "cli", feature = "files"))]
    let collateral_snapshot = if strict && plan.has_lifecycle_steps() {
        let tx_paths: HashSet<PathBuf> = result.changes.iter().map(|(p, _, _)| p.clone()).collect();
        snapshot_non_tx_files(&effective_cwd, &tx_paths)
    } else {
        HashMap::new()
    };

    // Run format steps, then validation steps.
    if let Some(err) = run_lifecycle(&plan, cwd, &effective_cwd) {
        if strict {
            rollback_strict(
                &result.changes,
                &result.pending,
                &result.deletions,
                &result.existed_before,
            );
            #[cfg(any(feature = "cli", feature = "files"))]
            restore_collateral_files(&collateral_snapshot);
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

    #[test]
    fn execute_plan_direct_rejects_escaped_cwd_with_guard() {
        // Regression: a plan with cwd pointing outside the workspace
        // must be rejected when a PathGuard is present.
        use crate::containment::{AbsolutePathPolicy, PathGuard};

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "content").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();

        let plan = crate::plan::Plan {
            version: 1,
            cwd: Some("/tmp".into()),
            operations: vec![crate::plan::Operation::Read {
                path: "test.txt".into(),
                lines: None,
            }],
            write_policy: None,
            strict: None,
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };

        let result = execute_plan_direct(plan, dir.path(), Some(&guard));
        assert!(
            result.is_err(),
            "should reject plan.cwd that escapes workspace"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("escapes workspace root"),
            "error should mention escape: {msg}"
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

    /// Regression: rollback_strict must create parent directories before
    /// restoring deleted files via atomic_write. If the parent dir was
    /// removed (e.g., the delete was the last file in it), the restore fails.
    #[test]
    fn rollback_strict_creates_parent_dir_for_deleted_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("file.txt");
        std::fs::write(&file, "original").unwrap();

        // Simulate: file was deleted and its parent dir removed.
        let file_pb = file.clone();
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&sub).unwrap();
        assert!(!sub.exists());

        // Build the state as if the tx engine tracked this deletion.
        let mut pending = HashMap::new();
        pending.insert(file_pb.clone(), ("original".to_string(), String::new()));
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let mut existed_before = HashSet::new();
        existed_before.insert(file_pb.clone());

        // rollback_strict should recreate the parent dir and restore the file.
        rollback_strict(&[], &pending, &deletions, &existed_before);
        assert!(
            file.exists(),
            "rollback should restore file even when parent dir was removed"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    /// Regression (#1063): a file created and then deleted in the same tx
    /// must not exist after rollback (it did not exist before the tx).
    #[test]
    fn rollback_strict_create_then_delete_leaves_no_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("ephemeral.txt");

        // Simulate: the tx engine created this file and then deleted it.
        // The changes list contains an entry with empty original (did not
        // exist before) and the deletions set also contains it.
        let file_pb = file.clone();
        let changes = vec![(file_pb.clone(), String::new(), "hello".to_string())];
        let pending = HashMap::new();
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let existed_before = HashSet::new(); // did NOT exist before tx

        // Create the file on disk to simulate mid-tx state.
        std::fs::write(&file, "hello").unwrap();

        rollback_strict(&changes, &pending, &deletions, &existed_before);

        assert!(
            !file.exists(),
            "create-then-delete file must not exist after rollback"
        );
    }

    /// Ensure rollback_strict still restores modified-then-deleted files
    /// (files that existed before the tx, were modified, then deleted).
    #[test]
    fn rollback_strict_modify_then_delete_restores_original() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "original").unwrap();

        let file_pb = file.clone();
        // File was modified (shows up in changes) and also deleted.
        let changes = vec![(
            file_pb.clone(),
            "original".to_string(),
            "modified".to_string(),
        )];
        let mut pending = HashMap::new();
        pending.insert(
            file_pb.clone(),
            ("original".to_string(), "modified".to_string()),
        );
        let mut deletions = HashSet::new();
        deletions.insert(file_pb.clone());
        let mut existed_before = HashSet::new();
        existed_before.insert(file_pb.clone());

        // Simulate mid-tx state: file was deleted.
        std::fs::remove_file(&file).unwrap();

        rollback_strict(&changes, &pending, &deletions, &existed_before);

        // The deletions loop should restore the original.
        assert!(file.exists(), "deleted file should be restored");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

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
            version: crate::plan::SCHEMA_VERSION,
            operations: Vec::new(),
            format: None,
            validate: None,
            verify: None,
            cwd: None,
            strict: None,
            write_policy: None,
            for_each: None,
        };
        let cwd = Path::new("/tmp");
        assert!(run_lifecycle(&plan, cwd, cwd).is_none());
    }

    // ---- snapshot / restore collateral (#1111.7) ----

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn snapshot_non_tx_files_captures_text_skips_binary_and_tx() {
        let dir = tempfile::TempDir::new().unwrap();

        // A text file not in the tx (should be captured).
        let collateral = dir.path().join("other.rs");
        std::fs::write(&collateral, "fn main() {}").unwrap();

        // A binary file (should be skipped).
        let binary = dir.path().join("image.png");
        std::fs::write(&binary, b"\x89PNG\x00\x00").unwrap();

        // A file that IS in the tx (should be skipped).
        let tx_file = dir.path().join("changed.rs");
        std::fs::write(&tx_file, "// changed").unwrap();

        let mut tx_paths = HashSet::new();
        tx_paths.insert(tx_file.clone());

        let snapshot = snapshot_non_tx_files(dir.path(), &tx_paths);

        assert!(
            snapshot.contains_key(&collateral),
            "collateral text file should be in snapshot"
        );
        assert_eq!(snapshot[&collateral], "fn main() {}");
        assert!(
            !snapshot.contains_key(&binary),
            "binary file should not be in snapshot"
        );
        assert!(
            !snapshot.contains_key(&tx_file),
            "tx file should not be in snapshot"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn restore_collateral_files_reverts_changed_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("bystander.rs");
        std::fs::write(&file, "original content").unwrap();

        // Build a snapshot with the original content.
        let mut snapshot = HashMap::new();
        snapshot.insert(file.clone(), "original content".to_string());

        // Simulate a formatter modifying the file.
        std::fs::write(&file, "reformatted content").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "reformatted content"
        );

        // Restore should revert the file.
        restore_collateral_files(&snapshot);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "original content",
            "collateral file should be restored to pre-format content"
        );
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn restore_collateral_skips_unchanged_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("untouched.rs");
        std::fs::write(&file, "same content").unwrap();

        let mut snapshot = HashMap::new();
        snapshot.insert(file.clone(), "same content".to_string());

        // File was not modified by the formatter. restore should be a no-op.
        restore_collateral_files(&snapshot);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "same content");
    }

    #[cfg(any(feature = "cli", feature = "files"))]
    #[test]
    fn snapshot_skips_large_files() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a file just above the 1 MiB threshold.
        let big_file = dir.path().join("huge.txt");
        let big_content = "x".repeat(COLLATERAL_SNAPSHOT_MAX_SIZE as usize + 1);
        std::fs::write(&big_file, &big_content).unwrap();

        // Create a small file that should be captured.
        let small_file = dir.path().join("small.txt");
        std::fs::write(&small_file, "tiny").unwrap();

        let snapshot = snapshot_non_tx_files(dir.path(), &HashSet::new());

        assert!(
            !snapshot.contains_key(&big_file),
            "file above size cap should not be in snapshot"
        );
        assert!(
            snapshot.contains_key(&small_file),
            "small file should be in snapshot"
        );
    }
}
