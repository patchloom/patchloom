//! Commit staged changes with backup and restore-on-failure.

use crate::write::{WritePolicy, atomic_create_new, atomic_write};
use anyhow::Context;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Commit staged changes (backup + restore-on-failure)
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

pub(crate) fn commit_error(message: impl Into<String>) -> CommitError {
    CommitError {
        message: message.into(),
        rollback_ok: true,
        backup_session: None,
    }
}

thread_local! {
    /// Per-thread test hook; never set in production.
    pub(crate) static FORCE_RESTORE_FAIL: std::sync::atomic::AtomicBool =
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
