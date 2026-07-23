//! Backup session management for undo safety net.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). Session create,
//! path sanitization, list/restore/prune, and host restore helper are one
//! unit; tests co-located. Do not split for LOC alone (#1494 restore API).
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub timestamp: String,
    pub entries: Vec<ManifestEntry>,
}

/// Convert a file path into a safe relative path for use inside a backup session.
///
/// If the file is under the project root, returns the relative path. Otherwise,
/// strips the root `/` (or drive prefix on Windows) so the path can be safely
/// joined under the session directory without replacing it.
fn sanitize_rel_path(file_path: &Path, project_root: &Path) -> PathBuf {
    // Strip Windows \\?\ (and //?/) so strip_prefix and drive-letter parsing
    // work when the caller passed a std::fs::canonicalize path (#1931).
    let file_path = dunce::simplified(file_path);
    let project_root = dunce::simplified(project_root);
    if let Ok(rel) = file_path.strip_prefix(project_root) {
        return rel.to_path_buf();
    }
    // File is outside the project root. Place it under __external__/ with
    // enough information to reconstruct the original absolute path on restore.
    let s = file_path.to_string_lossy();
    if let Some(rest) = s.strip_prefix('/') {
        // Unix absolute path: /tmp/foo -> __external__/tmp/foo
        PathBuf::from(format!("__external__/{rest}"))
    } else if s.len() >= 3
        && s.as_bytes()[1] == b':'
        && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/')
    {
        // Windows absolute path: C:\tmp\foo -> __external_C__/tmp/foo
        let drive = s.as_bytes()[0] as char;
        let rest = &s[3..];
        PathBuf::from(format!("__external_{drive}__/{rest}"))
    } else {
        // Relative path that couldn't be stripped (shouldn't normally happen).
        PathBuf::from(format!("__external__/{s}"))
    }
}

/// Monotonic counter to disambiguate backup sessions created in the same
/// nanosecond (e.g., from concurrent threads).
static SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// An active backup session that collects originals before writes.
pub struct BackupSession {
    session_dir: PathBuf,
    project_root: PathBuf,
    timestamp: String,
    entries: Vec<ManifestEntry>,
}

impl BackupSession {
    /// Start a new backup session. Creates the session directory and prunes
    /// stale backups older than 7 days.
    pub fn new(project_root: &Path) -> anyhow::Result<Self> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // Include a monotonic counter to prevent collisions when multiple
        // threads create backup sessions in the same nanosecond.
        let seq = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let timestamp = format!("{}_{}", now.as_nanos(), seq);
        let session_dir = project_root.join(BACKUP_DIR).join(&timestamp);
        std::fs::create_dir_all(&session_dir)
            .with_context(|| format!("failed to create backup dir {}", session_dir.display()))?;

        // Restrict backup directory to owner-only access so that backed-up
        // files with sensitive content are not world-readable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&session_dir, std::fs::Permissions::from_mode(0o700));
        }

        // Best-effort prune of old backups; ignore errors.
        let _ = prune_old_backups(project_root);

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
        let rel = sanitize_rel_path(file_path, &self.project_root);
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
        let rel = sanitize_rel_path(file_path, &self.project_root);
        let rel_str = rel.to_string_lossy().to_string();

        if self.entries.iter().any(|e| e.path == rel_str) {
            return Ok(());
        }

        if file_path.exists() {
            let backup_path = self.session_dir.join(&rel_str);
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating backup dir for {rel_str}"))?;
            }
            std::fs::copy(file_path, &backup_path)
                .with_context(|| format!("backing up {rel_str} before delete"))?;
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

/// Back up files, write new content atomically, and finalize the backup session.
/// All originals are saved before any writes begin, ensuring consistency.
///
/// Each element is `(path, content, policy)` where `path` is the file to write,
/// `content` is the new file content, and `policy` controls write transformations.
pub fn backup_write_files(
    cwd: &Path,
    files: &[(&Path, &str, &crate::write::WritePolicy)],
) -> anyhow::Result<()> {
    let mut session = BackupSession::new(cwd)?;
    for &(path, _, _) in files {
        session.save_before_write(path)?;
    }
    // Finalize (write manifest) BEFORE performing writes so the backup is
    // discoverable even if a write fails mid-batch, allowing `patchloom undo`
    // to restore the partially-modified files.
    let backup_ts = session.finalize()?;

    let write_result: anyhow::Result<()> = (|| {
        for &(path, content, policy) in files {
            crate::write::atomic_write(path, content, policy)?;
        }
        Ok(())
    })();

    if let Err(e) = write_result {
        // Auto-restore from backup on partial write failure so the caller
        // does not end up with a half-written batch.
        if let Some(ref ts) = backup_ts
            && let Err(restore_err) = restore_session(cwd, ts)
        {
            return Err(e.context(format!(
                "write failed AND auto-restore also failed: {restore_err}"
            )));
        }
        return Err(e);
    }
    Ok(())
}

/// Walk `path` and its parent directories for roots that contain `.patchloom/backups`.
///
/// Returns project roots (directories that own a `.patchloom/backups` child),
/// nearest first. When `path` is a file, the walk starts at its parent.
/// When `path` is a directory, the walk starts at `path` itself.
///
/// Embedders use this for undo/restore discovery without reimplementing the
/// parent walk or hard-coding [`BACKUP_DIR`] (#1934). Prefer
/// [`list_sessions_under`] when you need session manifests under a known root;
/// this helper only answers "where are backup directories?" along the ancestor chain.
pub fn find_backup_roots(path: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    loop {
        let backup_dir = current.join(BACKUP_DIR);
        if backup_dir.is_dir() {
            roots.push(current.clone());
        }
        if !current.pop() {
            break;
        }
    }
    roots
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
            let content = std::fs::read_to_string(&manifest_path)
                .with_context(|| format!("reading {}", manifest_path.display()))?;
            match serde_json::from_str::<Manifest>(&content) {
                Ok(manifest) => sessions.push(manifest),
                Err(e) => {
                    eprintln!(
                        "warning: corrupted backup manifest {}: {e}",
                        manifest_path.display()
                    );
                }
            }
        }
    }

    Ok(sessions)
}

/// Options for [`list_sessions_under`] (#1688).
#[derive(Debug, Clone)]
pub struct ListSessionsOptions {
    /// Also search ancestor directories of `project_root` for `.patchloom/backups`.
    pub ancestors: bool,
    /// Also search descendants for nested `.patchloom/backups` (default true).
    pub descendants: bool,
    /// Max directory depth for descendant walks (default 8).
    pub max_depth: Option<usize>,
}

impl Default for ListSessionsOptions {
    fn default() -> Self {
        Self {
            ancestors: false,
            descendants: true,
            max_depth: Some(8),
        }
    }
}

/// One backup root plus its sessions (newest first), for nested monorepo layouts (#1688).
#[derive(Debug, Clone)]
pub struct SessionListing {
    /// Directory that contains `.patchloom/backups` (the project root for that tree).
    pub project_root: PathBuf,
    /// Sessions under that root, newest first.
    pub sessions: Vec<Manifest>,
}

/// List backup sessions under `project_root`, optionally walking nested crates.
///
/// Library Apply stores sessions under each file's parent tree, so monorepo
/// edits may create `crates/foo/.patchloom/backups/` while the workspace root
/// only has its own backups. Agent hosts use this helper instead of
/// reimplementing nested discovery.
pub fn list_sessions_under(
    project_root: &Path,
    opts: &ListSessionsOptions,
) -> anyhow::Result<Vec<SessionListing>> {
    let mut roots: Vec<PathBuf> = Vec::new();
    roots.push(project_root.to_path_buf());

    if opts.ancestors {
        // Cap ancestor walk so hosts cannot accidentally walk to filesystem root
        // on deep absolute paths (same budget as descendant max_depth default).
        let ancestor_cap = opts.max_depth.unwrap_or(8).max(1);
        let mut cur = project_root.parent();
        let mut walked = 0usize;
        while let Some(p) = cur {
            if walked >= ancestor_cap {
                break;
            }
            roots.push(p.to_path_buf());
            walked += 1;
            cur = p.parent();
        }
    }

    if opts.descendants {
        let max_depth = opts.max_depth.unwrap_or(8);
        collect_descendant_backup_roots(project_root, max_depth, 0, &mut roots)?;
    }

    // Dedupe while preserving order.
    let mut seen = std::collections::HashSet::new();
    let mut unique_roots = Vec::new();
    for r in roots {
        if seen.insert(r.clone()) {
            unique_roots.push(r);
        }
    }

    let mut out = Vec::new();
    for root in unique_roots {
        let sessions = list_sessions(&root)?;
        if !sessions.is_empty() {
            out.push(SessionListing {
                project_root: root,
                sessions,
            });
        }
    }

    // Global newest-first by first session timestamp when present.
    out.sort_by(|a, b| {
        let ta = a
            .sessions
            .first()
            .map(|s| s.timestamp.as_str())
            .unwrap_or("");
        let tb = b
            .sessions
            .first()
            .map(|s| s.timestamp.as_str())
            .unwrap_or("");
        tb.cmp(ta)
    });
    Ok(out)
}

fn collect_descendant_backup_roots(
    dir: &Path,
    max_depth: usize,
    depth: usize,
    out: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    if depth >= max_depth {
        return Ok(());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip heavy / VCS / backup dirs themselves.
        if matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | ".patchloom" | "dist" | "build" | ".venv"
        ) {
            continue;
        }
        let backup = path.join(BACKUP_DIR);
        if backup.is_dir() {
            out.push(path.clone());
        }
        collect_descendant_backup_roots(&path, max_depth, depth + 1, out)?;
    }
    Ok(())
}

/// Restore a single path from the most recent backup session that contains it.
///
/// Used by agent hosts for post-Apply validate/revert recipes (#1494): Apply an
/// edit, run a host validator, and on failure call this helper to put the file
/// back without re-implementing backup layout.
///
/// Restores **only** the requested path (exact match), not sibling entries in
/// the same session. Prefer [`restore_path_from_session`] when the host already
/// knows the session timestamp.
///
/// Returns `true` if a backup entry was found and restored, `false` if no
/// session had the path.
pub fn restore_path_from_latest_backup(project_root: &Path, path: &Path) -> anyhow::Result<bool> {
    let sessions = list_sessions(project_root)?;
    // Match the same relative form used when writing the manifest. Exact
    // match only: bare file-name matching would restore the wrong session
    // when two paths share a basename under the same project root.
    let rel = sanitize_rel_path(path, project_root);
    let rel_str = rel.to_string_lossy();
    let abs_str = path.to_string_lossy();

    for manifest in &sessions {
        let hit = manifest
            .entries
            .iter()
            .any(|e| e.path == rel_str || e.path == abs_str);
        if hit {
            return restore_path_from_session(project_root, &manifest.timestamp, path);
        }
    }
    Ok(false)
}

/// Restore one path from a specific backup session (exact path match only).
///
/// Unlike [`restore_session`], sibling files in the same session are left
/// untouched. Missing session is an error; session present but path absent
/// returns `Ok(false)`.
///
/// See #1660.
pub fn restore_path_from_session(
    project_root: &Path,
    session_timestamp: &str,
    path: &Path,
) -> anyhow::Result<bool> {
    let session_dir = project_root.join(BACKUP_DIR).join(session_timestamp);
    let manifest_path = session_dir.join("manifest.json");

    let content = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "no backup session found for {session_timestamp} (use `patchloom undo --list` to see available sessions)"
        )
    })?;
    let manifest: Manifest = serde_json::from_str(&content)
        .with_context(|| format!("parsing backup manifest for session {session_timestamp}"))?;

    let rel = sanitize_rel_path(path, project_root);
    let rel_str = rel.to_string_lossy();
    let abs_str = path.to_string_lossy();

    let Some(entry) = manifest
        .entries
        .iter()
        .find(|e| e.path == rel_str || e.path == abs_str)
    else {
        return Ok(false);
    };

    validate_restore_path(&entry.path)?;
    let target = resolve_restore_path(project_root, &entry.path);

    match entry.action {
        FileAction::Modified => {
            let backup = session_dir.join(&entry.path);
            if !backup.exists() {
                return Ok(false);
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating parent dir for restore target {}", entry.path)
                })?;
            }
            std::fs::copy(&backup, &target)
                .with_context(|| format!("restoring modified file {}", entry.path))?;
            Ok(true)
        }
        FileAction::Created => {
            if target.exists() {
                std::fs::remove_file(&target)
                    .with_context(|| format!("removing created file {} during undo", entry.path))?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        FileAction::Deleted => {
            let backup = session_dir.join(&entry.path);
            if !backup.exists() {
                return Ok(false);
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating parent dir for restore target {}", entry.path)
                })?;
            }
            std::fs::copy(&backup, &target)
                .with_context(|| format!("restoring deleted file {}", entry.path))?;
            Ok(true)
        }
    }
}

/// Restore a specific backup session, returning the number of files restored.
pub fn restore_session(project_root: &Path, timestamp: &str) -> anyhow::Result<usize> {
    let session_dir = project_root.join(BACKUP_DIR).join(timestamp);
    let manifest_path = session_dir.join("manifest.json");

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("no backup session found for {timestamp} (use `patchloom undo --list` to see available sessions)"))?;
    let manifest: Manifest = serde_json::from_str(&content)
        .with_context(|| format!("parsing backup manifest for session {timestamp}"))?;

    let mut restored = 0;
    for entry in &manifest.entries {
        let target = resolve_restore_path(project_root, &entry.path);

        // Validate that path components never escape upward. This catches both
        // internal paths (`../../etc/passwd`) and crafted external prefixes
        // (`__external__/../../../etc/shadow`) that abuse the prefix to skip
        // validation. The `__external__/` prefix itself counts as a Normal
        // component (+1 depth), so legitimate external paths pass.
        validate_restore_path(&entry.path)?;
        match entry.action {
            FileAction::Modified => {
                let backup = session_dir.join(&entry.path);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).with_context(|| {
                            format!("creating parent dir for restore target {}", entry.path)
                        })?;
                    }
                    std::fs::copy(&backup, &target)
                        .with_context(|| format!("restoring modified file {}", entry.path))?;
                    restored += 1;
                }
            }
            FileAction::Created => {
                // File was newly created by the apply; remove it.
                if target.exists() {
                    std::fs::remove_file(&target).with_context(|| {
                        format!("removing created file {} during undo", entry.path)
                    })?;
                    restored += 1;
                }
            }
            FileAction::Deleted => {
                let backup = session_dir.join(&entry.path);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).with_context(|| {
                            format!("creating parent dir for restore target {}", entry.path)
                        })?;
                    }
                    std::fs::copy(&backup, &target)
                        .with_context(|| format!("restoring deleted file {}", entry.path))?;
                    restored += 1;
                }
            }
        }
    }

    Ok(restored)
}

/// Remove a consumed backup session directory so subsequent `undo` calls
/// reach older sessions instead of replaying the same one.
pub fn remove_session(project_root: &Path, timestamp: &str) -> anyhow::Result<()> {
    let session_dir = project_root.join(BACKUP_DIR).join(timestamp);
    if session_dir.is_dir() {
        std::fs::remove_dir_all(&session_dir)
            .with_context(|| format!("removing consumed backup session {timestamp}"))?;
    }
    Ok(())
}

/// Reject internal manifest paths that would escape the project root via
/// `..` traversal. Uses a syntactic depth check so it works regardless of
/// whether the target path exists on disk (#386).
fn validate_restore_path(entry_path: &str) -> anyhow::Result<()> {
    let mut depth: i32 = 0;
    for component in Path::new(entry_path).components() {
        match component {
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                        msg: format!("restore path escapes project root: {entry_path}"),
                    }));
                }
            }
            std::path::Component::Normal(_) => {
                depth += 1;
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("unexpected path component in restore path: {entry_path}"),
                }));
            }
        }
    }
    Ok(())
}

/// Resolve the restore target path from a manifest entry.
///
/// Paths starting with `__external__/` were backed up from outside the project
/// root (Unix). Paths starting with `__external_X__/` carry a Windows drive
/// letter. Both are reconstructed back to their original absolute location.
fn resolve_restore_path(project_root: &Path, entry_path: &str) -> PathBuf {
    if let Some(rest) = entry_path.strip_prefix("__external__/") {
        // Unix external path: __external__/tmp/foo -> /tmp/foo
        PathBuf::from(format!("/{rest}"))
    } else if entry_path.starts_with("__external_")
        && entry_path.len() > 14
        && entry_path
            .as_bytes()
            .get(11)
            .is_some_and(|b| b.is_ascii_alphabetic())
        && entry_path[12..].starts_with("__/")
    {
        // Windows external path: __external_C__/tmp/foo -> C:\tmp\foo
        let drive = entry_path.as_bytes()[11] as char;
        let rest = &entry_path[15..];
        PathBuf::from(format!("{drive}:\\{rest}"))
    } else {
        project_root.join(entry_path)
    }
}

/// Prune backup sessions older than 7 days.
///
/// Uses the creation timestamp embedded in the session directory name
/// (nanoseconds since UNIX epoch) instead of filesystem mtime, which can
/// be updated by file operations like `patchloom undo`.
pub fn prune_old_backups(project_root: &Path) -> anyhow::Result<usize> {
    let backup_dir = project_root.join(BACKUP_DIR);
    if !backup_dir.exists() {
        return Ok(0);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let max_age = std::time::Duration::from_secs(PRUNE_DAYS * 24 * 60 * 60);

    let mut pruned = 0;
    for entry in std::fs::read_dir(&backup_dir)?.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let dir_name = name.to_string_lossy();
        // Session directories are named "{nanos}_{seq}". Parse the
        // nanos prefix to determine session age.
        if let Some(nanos_str) = dir_name.split('_').next()
            && let Ok(nanos) = nanos_str.parse::<u128>()
        {
            // Compare using u128 nanos directly to avoid u128→u64 truncation.
            let now_nanos = now.as_nanos();
            let age_nanos = now_nanos.saturating_sub(nanos);
            let max_age_nanos = max_age.as_nanos();
            if age_nanos > max_age_nanos {
                let _ = std::fs::remove_dir_all(entry.path());
                pruned += 1;
            }
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

        // Create a fake session directory with a timestamp 8 days in the past.
        let eight_days_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            - std::time::Duration::from_secs(8 * 24 * 60 * 60);
        let old_ts = format!("{}_0", eight_days_ago.as_nanos());
        let old_dir = dir.path().join(BACKUP_DIR).join(&old_ts);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("manifest.json"), "[]").unwrap();

        let pruned = prune_old_backups(dir.path()).unwrap();
        assert_eq!(pruned, 1);
        assert!(!old_dir.exists());
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

    #[test]
    fn prune_old_backups_handles_large_nanos_without_truncation() {
        let dir = TempDir::new().unwrap();

        // Create a fake session with a timestamp that exceeds u64::MAX nanos.
        // u64::MAX nanos = ~584 years from epoch (around year 2554).
        // Use a value > u64::MAX to verify no truncation occurs.
        let huge_nanos: u128 = u64::MAX as u128 + 1_000_000_000;
        let ts_str = format!("{huge_nanos}_0");
        let future_dir = dir.path().join(BACKUP_DIR).join(&ts_str);
        std::fs::create_dir_all(&future_dir).unwrap();
        std::fs::write(future_dir.join("manifest.json"), "[]").unwrap();

        // This session is far in the future, so pruning should NOT remove it.
        // Before the fix, `as u64` truncation would make the timestamp wrap
        // around to near-epoch, causing it to appear very old and get pruned.
        let pruned = prune_old_backups(dir.path()).unwrap();
        assert_eq!(
            pruned, 0,
            "future session with u128 timestamp should not be pruned"
        );
        assert!(
            future_dir.exists(),
            "directory should still exist after prune"
        );
    }

    #[test]
    fn sanitize_rel_path_inside_project() {
        let root = Path::new("/project");
        let file = Path::new("/project/src/main.rs");
        let rel = sanitize_rel_path(file, root);
        assert_eq!(rel, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn sanitize_rel_path_outside_project() {
        let root = Path::new("/project");
        let file = Path::new("/tmp/other/file.txt");
        let rel = sanitize_rel_path(file, root);
        assert_eq!(rel, PathBuf::from("__external__/tmp/other/file.txt"));
    }

    #[test]
    fn backup_file_outside_project_root() {
        let project = TempDir::new().unwrap();
        let external = TempDir::new().unwrap();
        let ext_file = external.path().join("outside.txt");
        std::fs::write(&ext_file, "external content").unwrap();

        let mut session = BackupSession::new(project.path()).unwrap();
        session.save_before_write(&ext_file).unwrap();
        session.finalize().unwrap().unwrap();

        // The backup should be stored safely (not overwriting the original).
        assert_eq!(
            std::fs::read_to_string(&ext_file).unwrap(),
            "external content",
            "original file must not be corrupted by backup"
        );

        // The backup directory should contain the file under __external__/.
        let sessions = list_sessions(project.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(
            sessions[0].entries[0].path.starts_with("__external"),
            "external file path should be under __external*/ (got: {})",
            sessions[0].entries[0].path
        );
    }

    #[test]
    fn resolve_restore_path_internal() {
        let root = Path::new("/project");
        let p = resolve_restore_path(root, "src/main.rs");
        assert_eq!(p, PathBuf::from("/project/src/main.rs"));
    }

    #[test]
    fn resolve_restore_path_external_unix() {
        let root = Path::new("/project");
        let p = resolve_restore_path(root, "__external__/tmp/other/file.txt");
        assert_eq!(p, PathBuf::from("/tmp/other/file.txt"));
    }

    #[test]
    fn resolve_restore_path_external_windows() {
        let root = Path::new("/project");
        let p = resolve_restore_path(root, "__external_C__/Users/name/file.txt");
        assert_eq!(p, PathBuf::from("C:\\Users/name/file.txt"));
    }

    #[test]
    fn backup_and_restore_external_file() {
        let project = TempDir::new().unwrap();
        let external = TempDir::new().unwrap();
        let ext_file = external.path().join("data.txt");
        std::fs::write(&ext_file, "original external").unwrap();

        // Back up the external file.
        let mut session = BackupSession::new(project.path()).unwrap();
        session.save_before_write(&ext_file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Simulate modification.
        std::fs::write(&ext_file, "modified external").unwrap();

        // Restore should put the original content back at the external path.
        let restored = restore_session(project.path(), &ts).unwrap();
        assert_eq!(restored, 1);
        assert_eq!(
            std::fs::read_to_string(&ext_file).unwrap(),
            "original external"
        );
    }

    #[test]
    fn delete_backup_file_outside_project_root() {
        let project = TempDir::new().unwrap();
        let external = TempDir::new().unwrap();
        let ext_file = external.path().join("doomed.txt");
        std::fs::write(&ext_file, "doomed external").unwrap();

        let mut session = BackupSession::new(project.path()).unwrap();
        session.save_before_delete(&ext_file).unwrap();
        session.finalize().unwrap().unwrap();

        // Original must not be corrupted by the backup process.
        assert_eq!(
            std::fs::read_to_string(&ext_file).unwrap(),
            "doomed external"
        );
    }

    #[test]
    fn restore_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        // Manually create a crafted manifest with a path traversal entry.
        let ts = "999999999";
        let session_dir = dir.path().join(BACKUP_DIR).join(ts);
        std::fs::create_dir_all(&session_dir).unwrap();
        let manifest = Manifest {
            timestamp: ts.to_string(),
            entries: vec![ManifestEntry {
                path: "../../etc/passwd".to_string(),
                action: FileAction::Modified,
            }],
        };
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        std::fs::write(session_dir.join("manifest.json"), json).unwrap();

        let result = restore_session(dir.path(), ts);
        assert!(
            result.is_err(),
            "restore should reject path traversal, got: {:?}",
            result
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("escapes project root"),
            "error should mention escaping: {err}"
        );
    }

    #[test]
    fn restore_rejects_traversal_in_external_prefix() {
        let dir = TempDir::new().unwrap();
        let ts = "888888888";
        let session_dir = dir.path().join(BACKUP_DIR).join(ts);
        std::fs::create_dir_all(&session_dir).unwrap();
        let manifest = Manifest {
            timestamp: ts.to_string(),
            entries: vec![ManifestEntry {
                path: "__external__/../../../etc/shadow".to_string(),
                action: FileAction::Modified,
            }],
        };
        std::fs::write(
            session_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = restore_session(dir.path(), ts);
        assert!(
            result.is_err(),
            "external path with .. should be rejected, got: {result:?}"
        );
    }

    #[test]
    fn backup_write_files_backs_up_before_writing() {
        let dir = TempDir::new().unwrap();
        let f1 = dir.path().join("a.txt");
        let f2 = dir.path().join("b.txt");
        std::fs::write(&f1, "original-a").unwrap();
        std::fs::write(&f2, "original-b").unwrap();

        let policy = crate::write::WritePolicy::default();
        let files: Vec<(&Path, &str, &crate::write::WritePolicy)> =
            vec![(&f1, "new-a", &policy), (&f2, "new-b", &policy)];
        backup_write_files(dir.path(), &files).unwrap();

        assert_eq!(std::fs::read_to_string(&f1).unwrap(), "new-a");
        assert_eq!(std::fs::read_to_string(&f2).unwrap(), "new-b");

        // Undo should restore originals, proving backup happened before writes.
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        restore_session(dir.path(), &sessions[0].timestamp).unwrap();
        assert_eq!(std::fs::read_to_string(&f1).unwrap(), "original-a");
        assert_eq!(std::fs::read_to_string(&f2).unwrap(), "original-b");
    }

    /// Regression: backup_write_files must auto-restore on partial write
    /// failure so the caller does not end up with a half-written batch.
    #[test]
    fn backup_write_files_auto_restores_on_partial_failure() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, "original").unwrap();

        // Second target is in a nonexistent directory so atomic_write fails.
        let bad = dir.path().join("no_such_dir").join("fail.txt");

        let policy = crate::write::WritePolicy::default();
        let files: Vec<(&Path, &str, &crate::write::WritePolicy)> =
            vec![(&real, "updated", &policy), (&bad, "x", &policy)];
        let result = backup_write_files(dir.path(), &files);
        assert!(result.is_err(), "write to missing dir should fail");

        // After the auto-restore, the first file should be back to its
        // original content (not left in the "updated" state).
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            "original",
            "auto-restore should revert partial writes"
        );
    }

    #[test]
    fn backup_write_files_manifest_survives_write_failure() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, "original").unwrap();

        // Target a file whose parent directory does not exist so atomic_write fails.
        let bad = dir.path().join("no_such_dir").join("fail.txt");

        let policy = crate::write::WritePolicy::default();
        let files: Vec<(&Path, &str, &crate::write::WritePolicy)> =
            vec![(&real, "updated", &policy), (&bad, "x", &policy)];
        let result = backup_write_files(dir.path(), &files);
        assert!(result.is_err(), "write to missing dir should fail");

        // The manifest must exist because finalize() runs before writes.
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1, "backup session must be finalized");

        // Auto-restore should have reverted the first file back to original.
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "original");
    }

    #[test]
    fn remove_session_allows_sequential_undo() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("seq.txt");

        // Session 1: back up "v1"
        std::fs::write(&file, "v1").unwrap();
        let mut s1 = BackupSession::new(dir.path()).unwrap();
        s1.save_before_write(&file).unwrap();
        let ts1 = s1.finalize().unwrap().unwrap();

        // Simulate modification to "v2"
        std::fs::write(&file, "v2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Session 2: back up "v2"
        let mut s2 = BackupSession::new(dir.path()).unwrap();
        s2.save_before_write(&file).unwrap();
        let ts2 = s2.finalize().unwrap().unwrap();

        // Simulate modification to "v3"
        std::fs::write(&file, "v3").unwrap();

        // First undo: restore most recent (ts2) -> file becomes "v2"
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 2);
        let latest = &sessions[0].timestamp;
        assert_eq!(latest, &ts2);
        restore_session(dir.path(), latest).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");

        // Remove consumed session
        remove_session(dir.path(), latest).unwrap();
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1, "consumed session should be removed");

        // Second undo: now the most recent is ts1 -> file becomes "v1"
        let latest = &sessions[0].timestamp;
        assert_eq!(latest, &ts1);
        restore_session(dir.path(), latest).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "v1",
            "sequential undo should reach the original content"
        );
    }

    #[test]
    fn restore_path_from_latest_backup_restores_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("data.txt");
        std::fs::write(&file, "before").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap();

        std::fs::write(&file, "after").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "after");

        let ok = restore_path_from_latest_backup(dir.path(), &file).unwrap();
        assert!(ok);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "before");
    }

    #[test]
    fn restore_path_from_latest_backup_missing_returns_false() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("never_backed_up.txt");
        std::fs::write(&file, "x").unwrap();
        let ok = restore_path_from_latest_backup(dir.path(), &file).unwrap();
        assert!(!ok);
    }

    #[test]
    fn restore_path_from_session_only_one_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "A").unwrap();
        std::fs::write(&b, "B").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&a).unwrap();
        session.save_before_write(&b).unwrap();
        session.finalize().unwrap();
        std::fs::write(&a, "A2").unwrap();
        std::fs::write(&b, "B2").unwrap();

        let sessions = list_sessions(dir.path()).unwrap();
        let ts = &sessions[0].timestamp;
        assert!(restore_path_from_session(dir.path(), ts, &a).unwrap());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "B2");
    }

    #[test]
    fn restore_path_does_not_match_on_basename_alone() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let file_a = a.join("same.txt");
        let file_b = b.join("same.txt");
        std::fs::write(&file_a, "A").unwrap();
        std::fs::write(&file_b, "B").unwrap();

        // Backup only a/same.txt under project root.
        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file_a).unwrap();
        session.finalize().unwrap();
        std::fs::write(&file_a, "A2").unwrap();

        // Requesting b/same.txt must not restore a's session by basename.
        let ok = restore_path_from_latest_backup(dir.path(), &file_b).unwrap();
        assert!(!ok, "basename-only match would restore the wrong path");
        assert_eq!(std::fs::read_to_string(&file_a).unwrap(), "A2");
        assert_eq!(std::fs::read_to_string(&file_b).unwrap(), "B");
    }

    /// #1934: find_backup_roots walks parents and returns nearest roots first.
    #[test]
    fn find_backup_roots_walks_parents_nearest_first() {
        let outer = tempfile::TempDir::new().unwrap();
        let nested = outer.path().join("crates").join("pkg");
        std::fs::create_dir_all(&nested).unwrap();
        let nested_file = nested.join("src").join("lib.rs");
        std::fs::create_dir_all(nested_file.parent().unwrap()).unwrap();
        std::fs::write(&nested_file, "fn main() {}").unwrap();

        // Outer root has backups; nested crate also has its own.
        let outer_marker = outer.path().join("outer.txt");
        std::fs::write(&outer_marker, "o").unwrap();
        let mut s = BackupSession::new(outer.path()).unwrap();
        s.save_before_write(&outer_marker).unwrap();
        s.finalize().unwrap();

        let nested_marker = nested.join("inner.txt");
        std::fs::write(&nested_marker, "i").unwrap();
        let mut s = BackupSession::new(&nested).unwrap();
        s.save_before_write(&nested_marker).unwrap();
        s.finalize().unwrap();

        // From a file under nested: nested root first, then outer.
        let from_file = find_backup_roots(&nested_file);
        assert!(
            from_file.len() >= 2,
            "expected nested + outer roots, got {from_file:?}"
        );
        assert_eq!(from_file[0], nested, "nearest root first: {from_file:?}");
        assert!(
            from_file.iter().any(|r| r == outer.path()),
            "must include outer root: {from_file:?}"
        );

        // From a directory: start at that directory when it has backups.
        let from_dir = find_backup_roots(&nested);
        assert_eq!(from_dir[0], nested);

        // Path with no backup ancestors under a fresh empty tree.
        let empty = tempfile::TempDir::new().unwrap();
        let alone = empty.path().join("alone.txt");
        std::fs::write(&alone, "x").unwrap();
        let none = find_backup_roots(&alone);
        assert!(
            !none.iter().any(|r| r == empty.path()),
            "empty tree must not invent a backup root: {none:?}"
        );
    }

    /// #1688: ancestors walk is capped by max_depth (does not climb to FS root).
    #[test]
    fn list_sessions_under_ancestors_respects_max_depth() {
        let deep = tempfile::TempDir::new().unwrap();
        let nested = deep.path().join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&nested).unwrap();
        // Session only at the temp root (two levels above nested with cap=2).
        let file = deep.path().join("top.txt");
        std::fs::write(&file, "x").unwrap();
        let mut session = BackupSession::new(deep.path()).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap();

        // From nested, max_depth=2 walks a/b then a (not deep root unless shallow enough).
        let found = list_sessions_under(
            &nested,
            &ListSessionsOptions {
                ancestors: true,
                descendants: false,
                max_depth: Some(2),
            },
        )
        .unwrap();
        // nested is deep/a/b/c/d → parents: c, b (cap 2). top session is at deep root.
        assert!(
            found.is_empty(),
            "cap 2 from nested/d must not reach temp root: {found:?}"
        );

        let found_far = list_sessions_under(
            &nested,
            &ListSessionsOptions {
                ancestors: true,
                descendants: false,
                max_depth: Some(8),
            },
        )
        .unwrap();
        // May include sibling host temp sessions above deep; require our root.
        assert!(
            found_far.iter().any(|l| l.project_root == deep.path()),
            "cap 8 should reach temp root: {found_far:?}"
        );
    }

    /// #1688: list_sessions_under walks nested crate roots and skips heavy dirs.
    #[test]
    fn list_sessions_under_nested_and_depth_cap() {
        let workspace = tempfile::TempDir::new().unwrap();
        let nested = workspace.path().join("crates").join("pkg");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("f.txt");
        std::fs::write(&file, "x").unwrap();

        let mut session = BackupSession::new(&nested).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().expect("session");

        // Direct workspace list misses nested.
        assert!(list_sessions(workspace.path()).unwrap().is_empty());

        let listings = list_sessions_under(
            workspace.path(),
            &ListSessionsOptions {
                descendants: true,
                max_depth: Some(8),
                ancestors: false,
            },
        )
        .unwrap();
        assert_eq!(listings.len(), 1);
        assert_eq!(listings[0].sessions[0].timestamp, ts);

        // Depth 1 from workspace only sees `crates/`, not `crates/pkg`.
        let shallow = list_sessions_under(
            workspace.path(),
            &ListSessionsOptions {
                descendants: true,
                max_depth: Some(1),
                ancestors: false,
            },
        )
        .unwrap();
        assert!(
            shallow.is_empty(),
            "max_depth=1 should not reach crates/pkg: {shallow:?}"
        );
    }
}
