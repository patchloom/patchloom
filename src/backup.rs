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
use std::path::{Component, Path, PathBuf};

use crate::containment::PathGuard;

/// Directory name under the project root.
pub const BACKUP_DIR: &str = ".patchloom/backups";

/// Sidecar written only by [`BackupSession::finalize`].
///
/// Restore of `__external__*` paths requires this file so a forged
/// `manifest.json` (cloned repo or user `file.create`) cannot write
/// outside the workspace.
pub(crate) const ORIGIN_SIDECAR: &str = ".origin";

/// Maximum age in days before pruning old backups.
const PRUNE_DAYS: u64 = 7;

/// True when `path` is inside a `.patchloom/backups` tree (any ancestor).
///
/// Lexically normalizes `.` and `..` so `foo/../.patchloom/backups/x`
/// is still detected. User writers must not target this store.
pub fn is_under_backup_dir(path: &Path) -> bool {
    let needle: Vec<_> = Path::new(BACKUP_DIR)
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_os_string()),
            _ => None,
        })
        .collect();
    if needle.is_empty() {
        return false;
    }
    let mut norm: Vec<std::ffi::OsString> = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => {}
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = norm.pop();
            }
            Component::Normal(s) => norm.push(s.to_os_string()),
        }
    }
    norm.windows(needle.len())
        .any(|w| w.iter().eq(needle.iter()))
}

/// Refuse a user write whose destination is under [`BACKUP_DIR`].
pub fn refuse_user_write_under_backup_dir(path: &Path) -> anyhow::Result<()> {
    if is_under_backup_dir(path) {
        return Err(crate::exit::InvalidInputError {
            msg: format!("refusing write under {BACKUP_DIR}: {}", path.display()),
        }
        .into());
    }
    Ok(())
}

/// Refuse declared operation paths that resolve under [`BACKUP_DIR`].
pub(crate) fn refuse_declared_paths_under_backup_dir(
    cwd: &Path,
    op: &crate::plan::Operation,
) -> anyhow::Result<()> {
    for p in op.declared_paths() {
        let joined = if Path::new(&p).is_absolute() {
            PathBuf::from(&p)
        } else {
            cwd.join(&p)
        };
        refuse_user_write_under_backup_dir(&joined)?;
    }
    Ok(())
}

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
pub(crate) fn sanitize_rel_path(file_path: &Path, project_root: &Path) -> PathBuf {
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
/// nanosecond in one process (concurrent threads).
static SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Session directory name: `{nanos}_{pid}_{seq}`.
///
/// `seq` is per-process. Parallel CLI subprocesses (integration tests
/// using `cargo_bin` against the same project root) each start `seq` at 0,
/// so nanos+seq alone can collide and overwrite another apply's backup.
fn new_session_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seq = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}_{}_{}", now.as_nanos(), std::process::id(), seq)
}

/// Parse `{nanos}_{pid}_{seq}` or legacy `{nanos}_{seq}` for newest-first order.
///
/// Filename lexicographic sort is wrong: `{n}_99_0` > `{n}_1000_0` as
/// strings, so `undo` would treat the older pid as latest.
fn parse_session_id_parts(name: &str) -> (u128, u32, u64) {
    let mut parts = name.split('_');
    let nanos = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mid = parts
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    match parts.next().and_then(|s| s.parse::<u64>().ok()) {
        Some(seq) => (nanos, mid.min(u32::MAX as u64) as u32, seq),
        None => (nanos, 0, mid),
    }
}

/// Recency key: nanos, then directory mtime (same-nanos cross-process), then seq.
pub(crate) fn session_recency_key(
    session_dir: &Path,
    timestamp: &str,
) -> (u128, std::time::SystemTime, u64) {
    let (nanos, _pid, seq) = parse_session_id_parts(timestamp);
    let mtime = std::fs::metadata(session_dir)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    (nanos, mtime, seq)
}

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
        let timestamp = new_session_id();
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
            // Same special-node rule as save_before_delete: never `fs::copy` a
            // FIFO/socket/device (blocks forever) or symlink target (#2087).
            if crate::ops::file::is_regular_file_for_backup(file_path) {
                std::fs::copy(file_path, &backup_path).with_context(|| {
                    format!(
                        "failed to back up {} to {}",
                        file_path.display(),
                        backup_path.display()
                    )
                })?;
            } else {
                std::fs::write(&backup_path, b"").with_context(|| {
                    format!(
                        "writing empty backup marker for {} (special node)",
                        file_path.display()
                    )
                })?;
            }
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
            // Regular files: full byte backup. Symlinks / FIFO / socket / device:
            // empty marker only (fs::copy on FIFO blocks; symlink copy follows
            // target). Restore recreates an empty regular file — hosts that
            // need the special node type recreated must re-create it themselves
            // (#2087).
            if crate::ops::file::is_regular_file_for_backup(file_path) {
                std::fs::copy(file_path, &backup_path)
                    .with_context(|| format!("backing up {rel_str} before delete"))?;
            } else {
                std::fs::write(&backup_path, b"")
                    .with_context(|| format!("writing empty backup marker for {rel_str}"))?;
            }
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

        let origin_path = self.session_dir.join(ORIGIN_SIDECAR);
        std::fs::write(&origin_path, b"backup-session\n")
            .with_context(|| format!("failed to write session origin {}", origin_path.display()))?;

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
        // Auto-restore on partial write failure; surface typed fail-restore
        // so hosts peel session via `backup_session_from_error` (#2127 residual).
        let Some(ts) = backup_ts else {
            return Err(e);
        };
        let mutation_msg = e.to_string();
        return match restore_session(cwd, &ts) {
            Ok(_) => Err(crate::exit::MutationAfterBackupError::restored(ts, mutation_msg).into()),
            Err(restore_err) => Err(crate::exit::MutationAfterBackupError::restore_failed(
                ts,
                restore_err.to_string(),
                mutation_msg,
            )
            .into()),
        };
    }
    Ok(())
}

/// Walk `path` and its parent directories for roots that contain `.patchloom/backups`.
///
/// Returns project roots (directories that own a `.patchloom/backups` child),
/// nearest first. When `path` is a file, the walk starts at its parent.
/// When `path` is a directory, the walk starts at `path` itself.
///
/// The walk is **uncapped** (climbs to the filesystem root). That matches the
/// common embedder undo helper; contrast [`list_sessions_under`] which caps
/// ancestor depth via [`ListSessionsOptions::max_depth`]. An empty
/// `.patchloom/backups` directory still counts as a root (presence of the
/// directory, not non-empty sessions).
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

    entries.sort_by(|a, b| {
        let ka = session_recency_key(&a.path(), &a.file_name().to_string_lossy());
        let kb = session_recency_key(&b.path(), &b.file_name().to_string_lossy());
        kb.cmp(&ka)
    });

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

    // Global newest-first using the same recency key as list_sessions.
    out.sort_by(|a, b| {
        let ka = a.sessions.first().map(|s| {
            session_recency_key(
                &a.project_root.join(BACKUP_DIR).join(&s.timestamp),
                &s.timestamp,
            )
        });
        let kb = b.sessions.first().map(|s| {
            session_recency_key(
                &b.project_root.join(BACKUP_DIR).join(&s.timestamp),
                &s.timestamp,
            )
        });
        kb.cmp(&ka)
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
    restore_path_from_session_with_guard(project_root, session_timestamp, path, None)
}

/// Like [`restore_path_from_session`], with optional [`PathGuard`].
pub fn restore_path_from_session_with_guard(
    project_root: &Path,
    session_timestamp: &str,
    path: &Path,
    guard: Option<&PathGuard>,
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

    check_restore_policy(project_root, &session_dir, &entry.path, guard)?;
    let target = resolve_restore_path(project_root, &entry.path);

    match entry.action {
        FileAction::Modified => {
            let backup = session_dir.join(&entry.path);
            if !backup.exists() {
                // Entry listed but blob gone: fail closed (not "path not in session").
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "backup session {session_timestamp} is incomplete for {}; modified backup blob missing",
                        entry.path
                    ),
                }
                .into());
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
                return Err(crate::exit::InvalidInputError {
                    msg: format!(
                        "backup session {session_timestamp} is incomplete for {}; deleted backup blob missing",
                        entry.path
                    ),
                }
                .into());
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
///
/// Uncontained (no [`PathGuard`]). Legitimate `__external__*` entries from a
/// real [`BackupSession`] still restore. Use
/// [`restore_session_with_guard`] for `--contain` / MCP / library hosts.
pub fn restore_session(project_root: &Path, timestamp: &str) -> anyhow::Result<usize> {
    restore_session_with_guard(project_root, timestamp, None)
}

/// Restore a backup session, refusing targets outside `guard` when set.
///
/// With a guard: `__external__*` and any resolved path outside the workspace
/// are rejected before any write or delete. Without a guard, `__external__*`
/// still requires [`ORIGIN_SIDECAR`] (written only by [`BackupSession`]).
pub fn restore_session_with_guard(
    project_root: &Path,
    timestamp: &str,
    guard: Option<&PathGuard>,
) -> anyhow::Result<usize> {
    let session_dir = project_root.join(BACKUP_DIR).join(timestamp);
    let manifest_path = session_dir.join("manifest.json");

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("no backup session found for {timestamp} (use `patchloom undo --list` to see available sessions)"))?;
    let manifest: Manifest = serde_json::from_str(&content)
        .with_context(|| format!("parsing backup manifest for session {timestamp}"))?;

    // Phase 1: validate all entries and required blobs before any mutation so
    // a missing blob cannot leave a half-undone tree.
    let mut missing: Vec<String> = Vec::new();
    for entry in &manifest.entries {
        check_restore_policy(project_root, &session_dir, &entry.path, guard)?;
        match entry.action {
            FileAction::Modified | FileAction::Deleted => {
                let backup = session_dir.join(&entry.path);
                if !backup.exists() {
                    let kind = match entry.action {
                        FileAction::Modified => "modified",
                        FileAction::Deleted => "deleted",
                        FileAction::Created => unreachable!(),
                    };
                    missing.push(format!("{} ({kind} backup blob missing)", entry.path));
                }
            }
            FileAction::Created => {}
        }
    }
    if !missing.is_empty() {
        return Err(crate::exit::InvalidInputError {
            msg: format!(
                "backup session {timestamp} is incomplete; not removing session. Missing: {}",
                missing.join("; ")
            ),
        }
        .into());
    }

    // Phase 2: apply restores only after every required blob exists.
    let mut restored = 0;
    for entry in &manifest.entries {
        let target = resolve_restore_path(project_root, &entry.path);
        match entry.action {
            FileAction::Modified => {
                let backup = session_dir.join(&entry.path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("creating parent dir for restore target {}", entry.path)
                    })?;
                }
                std::fs::copy(&backup, &target)
                    .with_context(|| format!("restoring modified file {}", entry.path))?;
                restored += 1;
            }
            FileAction::Created => {
                // File was newly created by the apply; remove it if still present.
                // Already gone is fine (idempotent undo of create).
                if target.exists() {
                    std::fs::remove_file(&target).with_context(|| {
                        format!("removing created file {} during undo", entry.path)
                    })?;
                    restored += 1;
                }
            }
            FileAction::Deleted => {
                let backup = session_dir.join(&entry.path);
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

fn session_is_trusted(session_dir: &Path) -> bool {
    session_dir.join(ORIGIN_SIDECAR).is_file()
}

fn is_external_manifest_path(entry_path: &str) -> bool {
    if entry_path == "__external__" || entry_path.starts_with("__external__/") {
        return true;
    }
    entry_path.starts_with("__external_")
        && entry_path.len() > 14
        && entry_path
            .as_bytes()
            .get(11)
            .is_some_and(|b| b.is_ascii_alphabetic())
        && entry_path[12..].starts_with("__/")
}

/// Path traversal, untrusted `__external__*`, and contained restore policy.
fn check_restore_policy(
    project_root: &Path,
    session_dir: &Path,
    entry_path: &str,
    guard: Option<&PathGuard>,
) -> anyhow::Result<()> {
    validate_restore_path(entry_path)?;
    let external = is_external_manifest_path(entry_path);
    if external && !session_is_trusted(session_dir) {
        return Err(crate::exit::InvalidInputError {
            msg: format!(
                "refusing external restore from untrusted session (missing {ORIGIN_SIDECAR}): {entry_path}"
            ),
        }
        .into());
    }
    if let Some(g) = guard {
        if external {
            return Err(crate::fallback::EditError::guard_rejected(format!(
                "contained restore refuses paths outside the project root: {entry_path}"
            )));
        }
        let target = resolve_restore_path(project_root, entry_path);
        g.check_path(&target.to_string_lossy())
            .map_err(crate::fallback::EditError::guard_rejected)?;
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
        // Session directories are named "{nanos}_{pid}_{seq}" (older
        // sessions were "{nanos}_{seq}"). Parse the nanos prefix.
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
    fn session_id_includes_pid_and_is_unique() {
        let dir = TempDir::new().unwrap();
        let a = BackupSession::new(dir.path()).unwrap();
        let b = BackupSession::new(dir.path()).unwrap();
        assert_ne!(a.timestamp, b.timestamp);
        let pid = std::process::id().to_string();
        let a_parts: Vec<&str> = a.timestamp.split('_').collect();
        let b_parts: Vec<&str> = b.timestamp.split('_').collect();
        assert_eq!(a_parts.len(), 3, "expected nanos_pid_seq: {}", a.timestamp);
        assert_eq!(a_parts[1], pid);
        assert_eq!(b_parts[1], pid);
        assert_ne!(a_parts[2], b_parts[2]);
    }

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

    /// `save_before_write` must not `fs::copy` a FIFO (blocks forever).
    #[cfg(unix)]
    #[test]
    fn save_before_write_fifo_empty_marker_no_hang() {
        use std::process::Command as StdCommand;
        use std::time::{Duration, Instant};

        let dir = TempDir::new().unwrap();
        let fifo = dir.path().join("pipe.fifo");
        let status = StdCommand::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo available on unix CI");
        assert!(status.success());

        let mut session = BackupSession::new(dir.path()).unwrap();
        let start = Instant::now();
        session
            .save_before_write(&fifo)
            .expect("FIFO write backup must not hang");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "save_before_write on FIFO took {:?}",
            start.elapsed()
        );
        let ts = session.finalize().unwrap().unwrap();
        let marker = dir
            .path()
            .join(".patchloom/backups")
            .join(&ts)
            .join("pipe.fifo");
        assert!(marker.exists());
        assert_eq!(std::fs::read(&marker).unwrap(), b"");
    }

    #[test]
    fn restore_incomplete_session_errors_and_keeps_session() {
        // Missing Modified backup blob must fail closed (not silently skip and
        // delete the session), so agents can still recover via manual copy.
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "original").unwrap();

        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();

        // Corrupt: remove the backup blob but leave the manifest entry.
        let backup_blob = dir
            .path()
            .join(".patchloom/backups")
            .join(&ts)
            .join("keep.txt");
        assert!(backup_blob.exists(), "precondition: backup blob exists");
        std::fs::remove_file(&backup_blob).unwrap();

        std::fs::write(&file, "mutated").unwrap();
        let err = restore_session(dir.path(), &ts).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("incomplete") && msg.contains("keep.txt"),
            "expected incomplete-session error, got: {msg}"
        );
        // Session must remain so a second undo attempt or manual recovery works.
        let sessions = list_sessions(dir.path()).unwrap();
        assert!(
            sessions.iter().any(|s| s.timestamp == ts),
            "incomplete restore must not remove the session"
        );
        // Disk left as-is (mutated content not partially restored).
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mutated");
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

    fn write_named_session(root: &std::path::Path, ts: &str) {
        let d = root.join(BACKUP_DIR).join(ts);
        std::fs::create_dir_all(&d).unwrap();
        let manifest = Manifest {
            timestamp: ts.to_string(),
            entries: Vec::new(),
        };
        std::fs::write(
            d.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn list_sessions_same_nanos_orders_by_mtime_not_lexicographic_pid() {
        let dir = TempDir::new().unwrap();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let older = format!("{n}_99_0");
        let newer = format!("{n}_1000_0");
        write_named_session(dir.path(), &older);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_named_session(dir.path(), &newer);

        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.timestamp.as_str())
                .collect::<Vec<_>>(),
            vec![newer.as_str(), older.as_str()],
            "lexicographic Reverse would list _99_ before _1000_"
        );
    }

    #[test]
    fn list_sessions_same_nanos_seq_10_is_newer_than_seq_9() {
        let dir = TempDir::new().unwrap();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        write_named_session(dir.path(), &format!("{n}_1_9"));
        write_named_session(dir.path(), &format!("{n}_1_10"));

        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions[0].timestamp, format!("{n}_1_10"));
        assert_eq!(sessions[1].timestamp, format!("{n}_1_9"));
    }

    #[test]
    fn parse_session_id_legacy_two_part() {
        assert_eq!(parse_session_id_parts("12_3"), (12, 0, 3));
        assert_eq!(parse_session_id_parts("12_4_5"), (12, 4, 5));
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
        let err = result.expect_err("write to missing dir should fail");
        let session = crate::exit::backup_session_from_error(&err)
            .expect("fail-restore must peel session without Display scrape");
        assert!(
            !session.is_empty(),
            "session id must be non-empty after finalize"
        );
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
        let err = result.expect_err("write to missing dir should fail");

        // The manifest must exist because finalize() runs before writes.
        let sessions = list_sessions(dir.path()).unwrap();
        assert_eq!(sessions.len(), 1, "backup session must be finalized");
        let session = crate::exit::backup_session_from_error(&err).expect("peel session");
        assert_eq!(session, sessions[0].timestamp);

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

        // Empty `.patchloom/backups` directory still counts as a root.
        let bare = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(bare.path().join(BACKUP_DIR)).unwrap();
        let f = bare.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();
        let roots = find_backup_roots(&f);
        assert!(
            roots.iter().any(|r| r == bare.path()),
            "empty backups dir must still be a root: {roots:?}"
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

    #[test]
    fn is_under_backup_dir_detects_normalized_paths() {
        assert!(is_under_backup_dir(Path::new(
            ".patchloom/backups/evil/manifest.json"
        )));
        assert!(is_under_backup_dir(Path::new(
            "/proj/.patchloom/backups/id/blob"
        )));
        assert!(is_under_backup_dir(Path::new(
            "foo/../.patchloom/backups/x"
        )));
        assert!(is_under_backup_dir(Path::new(".patchloom/./backups/x")));
        assert!(is_under_backup_dir(Path::new(".patchloom/backups")));
        assert!(!is_under_backup_dir(Path::new(".patchloom/other")));
        assert!(!is_under_backup_dir(Path::new("src/main.rs")));
        assert!(!is_under_backup_dir(Path::new("backups/foo")));
    }

    #[test]
    fn finalize_writes_origin_sidecar() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let mut session = BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&file).unwrap();
        let ts = session.finalize().unwrap().unwrap();
        assert!(
            dir.path()
                .join(BACKUP_DIR)
                .join(&ts)
                .join(ORIGIN_SIDECAR)
                .is_file(),
            "BackupSession must write {ORIGIN_SIDECAR}"
        );
    }

    #[test]
    fn file_create_refuses_backup_dir_write() {
        let dir = TempDir::new().unwrap();
        let target = dir
            .path()
            .join(BACKUP_DIR)
            .join("evil")
            .join("manifest.json");
        let err = crate::api::file_create(
            &target,
            "{\"forged\":true}\n",
            false,
            crate::api::ApplyMode::Apply,
            None,
        )
        .unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "expected invalid_input, got: {err:#}"
        );
        assert!(!target.exists(), "forged backup manifest must not exist");
    }

    #[test]
    fn writers_refuse_backup_dir_targets() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join(BACKUP_DIR).join("evil").join("x.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "old").unwrap();

        let err = crate::api::replace_text(
            &dest,
            "old",
            "new",
            &crate::api::ReplaceOptions::default(),
            crate::api::ApplyMode::Apply,
            None,
        )
        .unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");

        let err =
            crate::api::file_append(&dest, "more", crate::api::ApplyMode::Apply, None).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");

        let err =
            crate::api::file_prepend(&dest, "pre", crate::api::ApplyMode::Apply, None).unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");

        let src = dir.path().join("src.txt");
        std::fs::write(&src, "moved").unwrap();
        let err = crate::api::file_rename(&src, &dest, true, crate::api::ApplyMode::Apply, None)
            .unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), "moved");
    }

    fn write_forged_session(
        project: &Path,
        ts: &str,
        entry_path: &str,
        action: FileAction,
        blob: Option<&[u8]>,
        with_origin: bool,
    ) {
        let session_dir = project.join(BACKUP_DIR).join(ts);
        std::fs::create_dir_all(&session_dir).unwrap();
        if let Some(bytes) = blob {
            let blob_path = session_dir.join(entry_path);
            if let Some(parent) = blob_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(blob_path, bytes).unwrap();
        }
        let manifest = Manifest {
            timestamp: ts.to_string(),
            entries: vec![ManifestEntry {
                path: entry_path.to_string(),
                action,
            }],
        };
        std::fs::write(
            session_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        if with_origin {
            std::fs::write(session_dir.join(ORIGIN_SIDECAR), b"backup-session\n").unwrap();
        }
    }

    #[test]
    fn restore_contain_refuses_forged_external() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("forged-undo-target");
        std::fs::write(&outside_file, "keep me").unwrap();

        let ext_path = sanitize_rel_path(&outside_file, dir.path())
            .to_string_lossy()
            .into_owned();
        assert!(
            ext_path.starts_with("__external"),
            "expected external prefix, got {ext_path}"
        );
        let ts = "forged-contain";
        write_forged_session(
            dir.path(),
            ts,
            &ext_path,
            FileAction::Modified,
            Some(b"pwned"),
            true,
        );

        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            crate::containment::AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let err = restore_session_with_guard(dir.path(), ts, Some(&guard)).unwrap_err();
        assert!(
            crate::api::is_guard_rejected(&err) || crate::exit::is_invalid_input(&err),
            "contained restore must fail, got: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "keep me");
    }

    #[test]
    fn restore_contain_refuses_created_external_delete() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("forged-created-target");
        std::fs::write(&outside_file, "do not delete").unwrap();

        let ext_path = sanitize_rel_path(&outside_file, dir.path())
            .to_string_lossy()
            .into_owned();
        let ts = "forged-created";
        write_forged_session(dir.path(), ts, &ext_path, FileAction::Created, None, true);

        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            crate::containment::AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let err = restore_session_with_guard(dir.path(), ts, Some(&guard)).unwrap_err();
        assert!(
            crate::api::is_guard_rejected(&err) || crate::exit::is_invalid_input(&err),
            "contained restore must fail, got: {err:#}"
        );
        assert!(
            outside_file.exists(),
            "Created + __external__ must not delete under contain"
        );
        assert_eq!(
            std::fs::read_to_string(&outside_file).unwrap(),
            "do not delete"
        );
    }

    #[test]
    fn restore_forged_external_without_origin_refused() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("forged-untrusted");
        std::fs::write(&outside_file, "keep").unwrap();

        let ext_path = sanitize_rel_path(&outside_file, dir.path())
            .to_string_lossy()
            .into_owned();
        let ts = "forged-no-origin";
        write_forged_session(
            dir.path(),
            ts,
            &ext_path,
            FileAction::Modified,
            Some(b"pwned"),
            false,
        );

        let err = restore_session(dir.path(), ts).unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "untrusted external restore must be invalid_input, got: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "keep");
    }
}
