//! Backup session management for undo safety net.
//!
//! Before any `--apply` write, commands save the original content of each
//! affected file to `.patchloom/backups/<timestamp>/`. The `patchloom undo`
//! command restores the most recent (or a chosen) backup.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Directory name under the project root.
pub const BACKUP_DIR: &str = ".patchloom/backups";

/// Maximum age in days before pruning old backups.
const PRUNE_DAYS: u64 = 7;

/// A single file entry in the backup manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Relative path from the project root.
    pub path: String,
    /// What happened to this file.
    pub action: FileAction,
}

/// What the apply operation did to a file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    /// File existed and was modified; original content is backed up.
    Modified,
    /// File was newly created (no original to back up).
    Created,
    /// File was deleted; original content is backed up.
    Deleted,
}

/// The manifest for a backup session.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub timestamp: String,
    pub entries: Vec<ManifestEntry>,
}

/// An active backup session that collects originals before writes.
pub struct BackupSession {
    session_dir: PathBuf,
    project_root: PathBuf,
    timestamp: String,
    entries: Vec<ManifestEntry>,
}

impl BackupSession {
    /// Start a new backup session. Creates the session directory.
    pub fn new(project_root: &Path) -> anyhow::Result<Self> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = format!("{}", now.as_nanos());
        let session_dir = project_root.join(BACKUP_DIR).join(&timestamp);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("failed to create backup dir {}", session_dir.display()))?;

        Ok(Self {
            session_dir,
            project_root: project_root.to_path_buf(),
            timestamp,
            entries: Vec::new(),
        })
    }

    /// Save the original content of a file before it is modified.
    /// If the file does not exist, records it as a "created" action.
    pub fn save_before_write(&mut self, file_path: &Path) -> anyhow::Result<()> {
        let rel = file_path
            .strip_prefix(&self.project_root)
            .unwrap_or(file_path);
        let rel_str = rel.to_string_lossy().to_string();

        // Skip duplicates (same file modified twice in one session).
        if self.entries.iter().any(|e| e.path == rel_str) {
            return Ok(());
        }

        if file_path.exists() {
            // Back up the original content.
            let backup_path = self.session_dir.join(&rel_str);
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(file_path, &backup_path).with_context(|| {
                format!(
                    "failed to back up {} to {}",
                    file_path.display(),
                    backup_path.display()
                )
            })?;
            self.entries.push(ManifestEntry {
                path: rel_str,
                action: FileAction::Modified,
            });
        } else {
            self.entries.push(ManifestEntry {
                path: rel_str,
                action: FileAction::Created,
            });
        }

        Ok(())
    }

    /// Record a file that was deleted by the apply operation.
    pub fn save_before_delete(&mut self, file_path: &Path) -> anyhow::Result<()> {
        let rel = file_path
            .strip_prefix(&self.project_root)
            .unwrap_or(file_path);
        let rel_str = rel.to_string_lossy().to_string();

        if self.entries.iter().any(|e| e.path == rel_str) {
            return Ok(());
        }

        if file_path.exists() {
            let backup_path = self.session_dir.join(&rel_str);
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(file_path, &backup_path)?;
        }

        self.entries.push(ManifestEntry {
            path: rel_str,
            action: FileAction::Deleted,
        });
        Ok(())
    }

    /// Write the manifest and finalize the backup session.
    /// Returns `None` if no files were backed up.
    pub fn finalize(self) -> anyhow::Result<Option<String>> {
        if self.entries.is_empty() {
            // Clean up empty session directory.
            let _ = std::fs::remove_dir(&self.session_dir);
            return Ok(None);
        }

        let manifest = Manifest {
            timestamp: self.timestamp.clone(),
            entries: self.entries,
        };

        let manifest_path = self.session_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&manifest_path, json)
            .with_context(|| format!("failed to write manifest {}", manifest_path.display()))?;

        Ok(Some(self.timestamp))
    }
}

/// List available backup sessions, most recent first.
pub fn list_sessions(project_root: &Path) -> anyhow::Result<Vec<Manifest>> {
    let backup_dir = project_root.join(BACKUP_DIR);
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    // Sort by name descending (timestamps sort lexicographically).
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));

    for entry in entries {
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.exists() {
            let content = std::fs::read_to_string(&manifest_path)?;
            if let Ok(manifest) = serde_json::from_str::<Manifest>(&content) {
                sessions.push(manifest);
            }
        }
    }

    Ok(sessions)
}

/// Restore a specific backup session, returning the number of files restored.
pub fn restore_session(project_root: &Path, timestamp: &str) -> anyhow::Result<usize> {
    let session_dir = project_root.join(BACKUP_DIR).join(timestamp);
    let manifest_path = session_dir.join("manifest.json");

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("no backup session found for {timestamp}"))?;
    let manifest: Manifest = serde_json::from_str(&content)?;

    let mut restored = 0;
    for entry in &manifest.entries {
        let target = project_root.join(&entry.path);
        match entry.action {
            FileAction::Modified => {
                let backup = session_dir.join(&entry.path);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&backup, &target)?;
                    restored += 1;
                }
            }
            FileAction::Created => {
                // File was newly created by the apply; remove it.
                if target.exists() {
                    std::fs::remove_file(&target)?;
                    restored += 1;
                }
            }
            FileAction::Deleted => {
                let backup = session_dir.join(&entry.path);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&backup, &target)?;
                    restored += 1;
                }
            }
        }
    }

    Ok(restored)
}

/// Prune backup sessions older than 7 days.
pub fn prune_old_backups(project_root: &Path) -> anyhow::Result<usize> {
    let backup_dir = project_root.join(BACKUP_DIR);
    if !backup_dir.exists() {
        return Ok(0);
    }

    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(PRUNE_DAYS * 24 * 60 * 60);

    let mut pruned = 0;
    for entry in std::fs::read_dir(&backup_dir)?.filter_map(|e| e.ok()) {
        if let Ok(meta) = entry.metadata()
            && let Ok(modified) = meta.modified()
            && modified < cutoff
        {
            let _ = std::fs::remove_dir_all(entry.path());
            pruned += 1;
        }
    }

    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn backup_and_restore_modified_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "original content").unwrap();

        // Create backup session and save the original.
        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Simulate modification.
        std::fs::write(&file, "modified content").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "modified content");

        // Restore.
        let restored = restore_session(dir.path(), &ts).unwrap();
        assert_eq!(restored, 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original content");
    }

    #[test]
    fn backup_and_restore_created_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");

        // File doesn't exist yet.
        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Simulate creation.
        std::fs::write(&file, "new content").unwrap();
        assert!(file.exists());

        // Restore should delete the file.
        let restored = restore_session(dir.path(), &ts).unwrap();
        assert_eq!(restored, 1);
        assert!(!file.exists());
    }

    #[test]
    fn backup_and_restore_deleted_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("doomed.txt");
        std::fs::write(&file, "doomed content").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_delete(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Simulate deletion.
        std::fs::remove_file(&file).unwrap();
        assert!(!file.exists());

        // Restore should recreate the file.
        let restored = restore_session(dir.path(), &ts).unwrap();
        assert_eq!(restored, 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "doomed content");
    }

    #[test]
    fn list_sessions_returns_newest_first() {
        let dir = TempDir::new().unwrap();

        // Create two sessions with guaranteed different timestamps.
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "v1").unwrap();
        let mut s1 = BackupSession::new(dir.path()).unwrap();
        s1.save_before_write(&file).unwrap();
        let ts1 = s1.finalize().unwrap().unwrap();

        // Small delay to guarantee a different nanosecond timestamp.
        std::thread::sleep(std::time::Duration::from_millis(10));

        std::fs::write(&file, "v2").unwrap();
        let mut s2 = BackupSession::new(dir.path()).unwrap();
        s2.save_before_write(&file).unwrap();
        let ts2 = s2.finalize().unwrap().unwrap();

        assert_ne!(ts1, ts2, "timestamps must differ");
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].timestamp, ts2);
        assert_eq!(sessions[1].timestamp, ts1);
    }

    #[test]
    fn empty_session_cleans_up() {
        let dir = TempDir::new().unwrap();
        let session = BackupSession::new(dir.path()).unwrap();
        let result = session.finalize().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn duplicate_save_ignored() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("dup.txt");
        std::fs::write(&file, "original").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.save_before_write(&file).unwrap(); // Should be ignored.

        let ts = session.finalize().unwrap().unwrap();
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions[0].entries.len(), 1);

        // Still restores correctly.
        std::fs::write(&file, "changed").unwrap();
        restore_session(dir.path(), &ts).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn prune_old_backups_removes_stale_sessions() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "v1").unwrap();

        // Create a backup session.
        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Backdate the session directory's mtime to 8 days ago.
        let session_dir = dir.path().join(BACKUP_DIR).join(&ts);
        let eight_days_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 60 * 60);
        let times = std::fs::FileTimes::new().set_modified(eight_days_ago);
        let f = std::fs::File::open(&session_dir).unwrap();
        f.set_times(times).unwrap();

        let pruned = prune_old_backups(dir.path()).unwrap();
        assert_eq!(pruned, 1);

        // Session should be gone.
        let sessions = list_sessions(dir.path()).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn prune_old_backups_keeps_recent_sessions() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "v1").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap().unwrap();

        // Session is fresh; prune should not remove it.
        let pruned = prune_old_backups(dir.path()).unwrap();
        assert_eq!(pruned, 0);

        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn prune_old_backups_no_backup_dir() {
        let dir = TempDir::new().unwrap();
        let pruned = prune_old_backups(dir.path()).unwrap();
        assert_eq!(pruned, 0);
    }
}
