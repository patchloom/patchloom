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
/// Pure renames (`file.rename` staged as create-dest + delete-src with
/// identical content) are applied with `fs::rename` so multi-hardlinked
/// inodes stay shared (#1739). Other mutations keep the create/write +
/// delete path.
///
/// If any write fails, restores all already-written files from the backup
/// session before returning [`CommitError`].
/// Finalize backup then write. Returns the backup session timestamp when a
/// session was created (`None` if nothing was backed up).
pub(crate) fn commit_changes(
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
    cwd: &Path,
) -> Result<Option<String>, CommitError> {
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

    let rename_pairs = detect_pure_renames(changes, deletions, existed_before);
    let renamed_from: HashSet<&Path> = rename_pairs.iter().map(|(f, _)| f.as_path()).collect();
    let renamed_to: HashSet<&Path> = rename_pairs.iter().map(|(_, t)| t.as_path()).collect();

    let noop_policy = WritePolicy::default();
    let write_result = (|| -> anyhow::Result<()> {
        // Apply pure renames first via fs::rename (preserves hardlinks #1739).
        for (from, to) in &rename_pairs {
            if let Some(parent) = to.parent()
                && !parent.as_os_str().is_empty()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {}", parent.display()))?;
            }
            rename_or_copy(from, to)?;
        }

        for (path, _, new_content) in changes {
            if renamed_from.contains(path.as_path()) || renamed_to.contains(path.as_path()) {
                continue;
            }
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
            if renamed_from.contains(path.as_path()) {
                continue;
            }
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

    Ok(backup_session)
}

/// Detect pure renames staged as create-dest + delete-src with identical
/// content (how `file.rename` is represented in pending). Pairing is
/// deterministic by path order so identical-content concurrent renames
/// remain stable.
fn detect_pure_renames(
    changes: &[(PathBuf, String, String)],
    deletions: &HashSet<PathBuf>,
    existed_before: &HashSet<PathBuf>,
) -> Vec<(PathBuf, PathBuf)> {
    use std::collections::HashMap;

    // Sources: deleted paths whose staged final content is empty (or which
    // appear only as deletions) with a known original body.
    let mut sources_by_content: HashMap<&str, Vec<&PathBuf>> = HashMap::new();
    for (path, original, new_content) in changes {
        if !deletions.contains(path) {
            continue;
        }
        // Empty final content is the FileRename / delete staging shape.
        if !new_content.is_empty() {
            continue;
        }
        sources_by_content
            .entry(original.as_str())
            .or_default()
            .push(path);
    }
    for paths in sources_by_content.values_mut() {
        paths.sort();
    }

    let mut pairs = Vec::new();
    let mut used_from: HashSet<&Path> = HashSet::new();

    // Destinations: new paths (not existed_before) not themselves deleted.
    let mut dests: Vec<(&PathBuf, &str)> = changes
        .iter()
        .filter(|(path, _, _)| !deletions.contains(path) && !existed_before.contains(path))
        .map(|(path, _, new_content)| (path, new_content.as_str()))
        .collect();
    dests.sort_by(|a, b| a.0.cmp(b.0));

    for (to, content) in dests {
        let Some(sources) = sources_by_content.get_mut(content) else {
            continue;
        };
        while let Some(from) = sources.pop() {
            if used_from.contains(from.as_path()) {
                continue;
            }
            // Avoid renaming a path onto itself.
            if from == to {
                continue;
            }
            used_from.insert(from.as_path());
            pairs.push((from.clone(), to.clone()));
            break;
        }
    }
    pairs
}

/// Rename a file, falling back to copy+delete across devices.
fn rename_or_copy(src: &Path, dst: &Path) -> anyhow::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            std::fs::copy(src, dst).with_context(|| {
                format!("cross-device copy {} -> {}", src.display(), dst.display())
            })?;
            std::fs::remove_file(src).with_context(|| {
                format!("removing source after cross-device copy: {}", src.display())
            })?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn is_cross_device(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn detect_pure_renames_pairs_create_and_delete() {
        let from = PathBuf::from("/tmp/ws/a.txt");
        let to = PathBuf::from("/tmp/ws/c.txt");
        let changes = vec![
            (to.clone(), String::new(), "shared\n".to_string()),
            (from.clone(), "shared\n".to_string(), String::new()),
        ];
        let mut deletions = HashSet::new();
        deletions.insert(from.clone());
        let existed = HashSet::new();
        let pairs = detect_pure_renames(&changes, &deletions, &existed);
        assert_eq!(pairs, vec![(from, to)]);
    }

    #[test]
    fn detect_pure_renames_skips_modified_dest() {
        let from = PathBuf::from("/tmp/ws/a.txt");
        let to = PathBuf::from("/tmp/ws/c.txt");
        let changes = vec![
            (to.clone(), "old\n".to_string(), "shared\n".to_string()),
            (from.clone(), "shared\n".to_string(), String::new()),
        ];
        let mut deletions = HashSet::new();
        deletions.insert(from);
        let mut existed = HashSet::new();
        existed.insert(to);
        let pairs = detect_pure_renames(&changes, &deletions, &existed);
        assert!(
            pairs.is_empty(),
            "force-overwrite dest is not a pure rename"
        );
    }
}
