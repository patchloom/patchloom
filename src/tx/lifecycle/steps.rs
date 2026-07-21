//! Format/validate lifecycle steps, collateral snapshot, and strict rollback.

use crate::exec;
use crate::plan::{self, Plan};
use crate::tx::output::{describe_exit_status, describe_lifecycle_cwd};
use crate::write::{WritePolicy, atomic_write};
#[cfg(any(feature = "cli", feature = "files"))]
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULT_LIFECYCLE_TIMEOUT_SECS: u64 = 60;

// Lifecycle helpers (extracted from run() for testability)
// ---------------------------------------------------------------------------

pub(crate) struct LifecycleError {
    pub message: String,
    pub kind: &'static str,
}

/// Format a lifecycle step failure message, appending stderr output if available.
pub(crate) fn lifecycle_failure_msg(header: &str, stderr: &str) -> String {
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
                let step_label = if required { label } else { error_label };
                let header = format!(
                    "{step_label} failed (step {}, {}, cwd: {})",
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
pub(crate) const COLLATERAL_SNAPSHOT_MAX_SIZE: u64 = 1_048_576; // 1 MiB

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
        // Soft content load: skip binary / unreadable collateral (do not
        // rewrite non-text via atomic_write as UTF-8).
        let current = match crate::files::try_read_text_file(path) {
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
