use crate::backup;
use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(
    about = "Preview or restore files from a backup created by --apply",
    long_about = "\
Preview or restore files from a backup session under `.patchloom/backups/`.

**Dry-run by default (same write singularity as replace/tx):** running \
`patchloom undo` only shows what would be restored and exits 2 \
(`CHANGES_DETECTED`). Pass `--apply` to actually restore. There is no \
`--latest` flag; without `--session`, the most recent backup is used.

Backups are created only when a write command was run with `--apply`."
)]
#[command(after_help = "\
EXAMPLES:
  patchloom undo --list
  patchloom undo              # dry-run preview (exit 2); does NOT restore
  patchloom undo --apply      # restore most recent session
  patchloom undo --session <id-from-undo-list> --apply

NOTE:
  Default is preview only. Agents that want a real restore must pass
  `--apply` (exit 0). Preview is exit 2 with status changes_detected.
  --list walks nested .patchloom/backups roots under the cwd (monorepo
  crates) so sessions from library Apply under crates/foo/ are visible.")]
pub struct UndoArgs {
    /// List available backup sessions (including nested monorepo roots).
    #[arg(long)]
    pub list: bool,

    /// Restore a specific backup session by timestamp (default: most recent).
    #[arg(long)]
    pub session: Option<String>,

    /// Restore only these session paths (repeatable). Unknown path is
    /// `no_matches` and does not restore the rest of the session.
    #[arg(long)]
    pub path: Vec<String>,

    /// Actually restore files. Without this flag, undo only previews
    /// (exit 2) and does not change the working tree.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct UndoListEntry {
    pub(crate) timestamp: String,
    /// Backup project root relative to cwd when possible (#1695).
    pub(crate) project_root: String,
    pub(crate) file_count: usize,
    pub(crate) entries: Vec<backup::ManifestEntry>,
}

/// `--json` list payload: items plus listing warnings (emit_json_items is
/// an array and cannot carry a sibling `warnings` field).
#[derive(Debug, Serialize)]
pub(crate) struct UndoListOutput {
    pub(crate) items: Vec<UndoListEntry>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UndoPreviewEntry {
    pub(crate) path: String,
    pub(crate) action: String,
}

/// Dry-run label for one backup entry. Directory-rename undo is Created
/// plus `renamed_from` and restores with `fs::rename`, not a delete.
pub(crate) fn preview_action(entry: &backup::ManifestEntry) -> String {
    match entry.action {
        backup::FileAction::Modified => "restore original".to_string(),
        backup::FileAction::Created => match entry.renamed_from.as_deref() {
            Some(from) => format!("rename back to {from}"),
            None => "delete (was created by apply)".to_string(),
        },
        backup::FileAction::Deleted => "recreate (was deleted by apply)".to_string(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct UndoPreviewOutput {
    pub(crate) ok: bool,
    pub(crate) status: &'static str,
    /// Same kind as tidy check / status when a dry-run would restore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_kind: Option<&'static str>,
    /// Always set on dry-run so agents do not treat exit 2 as a completed restore.
    pub(crate) hint: &'static str,
    /// False on dry-run: matches write mutators (#1830 / #1788). Agents that
    /// only branch on `ok` + `applied` must not treat preview as a restore.
    pub(crate) applied: bool,
    pub(crate) session: String,
    pub(crate) project_root: String,
    pub(crate) file_count: usize,
    pub(crate) entries: Vec<UndoPreviewEntry>,
}

pub(crate) const UNDO_DRY_RUN_HINT: &str = "pass --apply to restore files (default is dry-run preview; exit 2 means changes would be made)";

pub fn run(args: UndoArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "undo: list={}, session={:?}, path={}, apply={}",
        args.list,
        args.session,
        args.path.len(),
        args.apply
    );
    let cwd = global.resolve_cwd()?;

    if args.list {
        let (sessions, warnings) = collect_sessions(&cwd)?;
        if sessions.is_empty() {
            let (kind, msg, code) = list_no_usable_sessions(&warnings);
            global.emit_error_json_kind(Some(kind), &msg)?;
            return Ok(code);
        }

        let list_items: Vec<UndoListEntry> = sessions
            .iter()
            .map(|(root, manifest)| UndoListEntry {
                timestamp: manifest.timestamp.clone(),
                project_root: display_root(&cwd, root),
                file_count: manifest.entries.len(),
                entries: manifest.entries.clone(),
            })
            .collect();

        // --json wraps items+warnings. --jsonl streams sessions then a
        // `type: warnings` trailer (same as replace/patch summary trailers).
        if global.json {
            global.emit_json(&UndoListOutput {
                items: list_items,
                warnings,
            })?;
            return Ok(exit::SUCCESS);
        }

        if global.jsonl {
            global.emit_json_items(&list_items)?;
            if !warnings.is_empty() {
                global.emit_json(&serde_json::json!({
                    "type": "warnings",
                    "warnings": warnings,
                }))?;
            }
            return Ok(exit::SUCCESS);
        }

        if !global.quiet {
            for warning in &warnings {
                eprintln!("{warning}");
            }
        }
        if !global.emit_json_items(&list_items)? && !global.quiet {
            for (root, s) in &sessions {
                let file_count = s.entries.len();
                let root_disp = display_root(&cwd, root);
                println!("{} ({file_count} file(s)) root={root_disp}", s.timestamp);
                for e in &s.entries {
                    println!("  {} ({})", e.path, action_label(&e.action));
                }
                println!();
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Resolve session across nested backup roots (#1695).
    let (backup_root, timestamp, session) = match resolve_session(&cwd, args.session.as_deref()) {
        Ok(Some(v)) => v,
        Ok(None) => {
            global.emit_error_json_kind(Some("no_matches"), "no backup sessions found")?;
            return Ok(exit::NO_MATCHES);
        }
        // Named session miss: return Ok(NO_MATCHES) so human CLI exit is 3
        // (not bare Err → exit 1) and --json always has error_kind.
        Err(e) if crate::exit::is_no_match(&e) => {
            global.emit_error_json_kind(Some("no_matches"), &e.to_string())?;
            return Ok(exit::NO_MATCHES);
        }
        Err(e) if crate::exit::is_invalid_input(&e) => {
            global.emit_error_json_kind(Some("invalid_input"), &e.to_string())?;
            return Ok(exit::FAILURE);
        }
        Err(e) => return Err(e),
    };

    let mut session = session;
    if !args.path.is_empty() {
        match filter_session_paths(&backup_root, &session, &args.path) {
            Ok(filtered) => session = filtered,
            Err(e) if crate::exit::is_no_match(&e) => {
                global.emit_error_json_kind(Some("no_matches"), &e.to_string())?;
                return Ok(exit::NO_MATCHES);
            }
            Err(e) => return Err(e),
        }
    }

    if !args.apply {
        if let Err(e) = backup::classify_restore_write_dests(&backup_root, &session) {
            global.emit_error_json_kind(Some("invalid_input"), &e.msg)?;
            return Ok(exit::FAILURE);
        }
        let entries: Vec<UndoPreviewEntry> = session
            .entries
            .iter()
            .map(|entry| UndoPreviewEntry {
                path: entry.path.clone(),
                action: preview_action(entry),
            })
            .collect();

        let output = UndoPreviewOutput {
            ok: true,
            status: "changes_detected",
            error_kind: Some("changes_detected"),
            hint: UNDO_DRY_RUN_HINT,
            applied: false,
            session: timestamp.clone(),
            project_root: display_root(&cwd, &backup_root),
            file_count: entries.len(),
            entries,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!(
                "Would restore session {} ({} file(s)) root={}:",
                timestamp,
                session.entries.len(),
                output.project_root
            );
            for entry in &output.entries {
                println!("  {} -> {}", entry.path, entry.action);
            }
            // Always print when not quiet: agents often capture piped stderr
            // and miss TTY-only show_status() hints (fixrealloop confusion).
            eprintln!("hint: {UNDO_DRY_RUN_HINT}");
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // Apply restore from the root that owns the session (#1695).
    crate::verbose!(
        "undo: restoring session {} under {}",
        timestamp,
        backup_root.display()
    );
    let guard = global.workspace_guard(&cwd)?;
    let restored = if args.path.is_empty() {
        let n = backup::restore_session_with_guard(&backup_root, &timestamp, guard.as_ref())?;
        // Remove the consumed session so subsequent `undo` calls advance to
        // the next-oldest session instead of replaying the same one.
        // Even when restored == 0 (e.g. create-only session, files already gone),
        // the session is complete and safe to drop; do not claim applied:true.
        backup::remove_session(&backup_root, &timestamp)?;
        n
    } else {
        let mut n = 0usize;
        for rel in &args.path {
            if backup::restore_path_from_session_with_guard(
                &backup_root,
                &timestamp,
                std::path::Path::new(rel),
                guard.as_ref(),
            )? {
                n += 1;
            }
        }
        n
    };
    crate::verbose!("undo: restored {} file(s)", restored);
    let applied = restored > 0;
    let status = if applied { "restored" } else { "noop" };
    if !global.emit_json(&serde_json::json!({
        "ok": true,
        "status": status,
        "applied": applied,
        "session": timestamp,
        "project_root": display_root(&cwd, &backup_root),
        "file_count": restored,
    }))? && !global.quiet
    {
        if applied {
            eprintln!("restored {restored} file(s) from session {timestamp}");
        } else if args.path.is_empty() {
            eprintln!(
                "session {timestamp}: nothing to restore (already undone or files gone); session removed"
            );
        } else {
            eprintln!(
                "session {timestamp}: nothing to restore (already undone or files gone); session kept"
            );
        }
    }

    Ok(exit::SUCCESS)
}

type ListedSessions = Vec<(std::path::PathBuf, backup::Manifest)>;

/// Sessions under `cwd` and nested monorepo roots, newest first (#1695),
/// plus listing warnings from missing/corrupt/unreadable manifests.
pub(crate) fn collect_sessions(
    cwd: &std::path::Path,
) -> anyhow::Result<(ListedSessions, Vec<String>)> {
    let listings = backup::list_sessions_under(
        cwd,
        &backup::ListSessionsOptions {
            descendants: true,
            ancestors: false,
            max_depth: Some(8),
        },
    )?;
    let mut all = Vec::new();
    let mut warnings = Vec::new();
    for listing in listings {
        warnings.extend(listing.warnings);
        for session in listing.sessions {
            all.push((listing.project_root.clone(), session));
        }
    }
    all.sort_by(|a, b| {
        let da = a.0.join(backup::BACKUP_DIR).join(&a.1.timestamp);
        let db = b.0.join(backup::BACKUP_DIR).join(&b.1.timestamp);
        backup::session_recency_key(&db, &b.1.timestamp)
            .cmp(&backup::session_recency_key(&da, &a.1.timestamp))
    });
    Ok((all, warnings))
}

/// `--list` with zero usable sessions: empty tree is `no_matches`;
/// session dirs that exist but have no readable manifest are `invalid_input`.
pub(crate) fn list_no_usable_sessions(warnings: &[String]) -> (&'static str, String, u8) {
    if warnings.is_empty() {
        (
            "no_matches",
            "no backup sessions found".to_string(),
            exit::NO_MATCHES,
        )
    } else {
        (
            "invalid_input",
            format!(
                "session directories exist but none have a readable manifest.json: {}",
                warnings.join("; ")
            ),
            exit::FAILURE,
        )
    }
}

pub(crate) fn resolve_session(
    cwd: &std::path::Path,
    wanted: Option<&str>,
) -> anyhow::Result<Option<(std::path::PathBuf, String, backup::Manifest)>> {
    let (sessions, warnings) = collect_sessions(cwd)?;
    if let Some(ts) = wanted {
        for (root, manifest) in &sessions {
            if manifest.timestamp == ts {
                return Ok(Some((root.clone(), ts.to_string(), manifest.clone())));
            }
        }
        // Named session missing (whether or not any other sessions exist).
        // Typed no_matches so --json sets error_kind (MPI 2026-07-16: bare
        // anyhow lost error_kind and agents could not branch).
        return Err(crate::exit::NoMatchError {
            msg: format!(
                "no backup session found for {ts} (use `patchloom undo --list` to see available sessions)"
            ),
        }
        .into());
    }
    if sessions.is_empty() {
        if !warnings.is_empty() {
            let (_, msg, _) = list_no_usable_sessions(&warnings);
            return Err(crate::exit::InvalidInputError { msg }.into());
        }
        return Ok(None);
    }
    let (root, manifest) = sessions.into_iter().next().expect("non-empty");
    let ts = manifest.timestamp.clone();
    Ok(Some((root, ts, manifest)))
}

pub(crate) fn display_root(cwd: &std::path::Path, root: &std::path::Path) -> String {
    root.strip_prefix(cwd)
        .map(|p| {
            let s = p.to_string_lossy();
            if s.is_empty() {
                ".".to_string()
            } else {
                s.into_owned()
            }
        })
        .unwrap_or_else(|_| root.to_string_lossy().into_owned())
}

pub(crate) fn filter_session_paths(
    backup_root: &std::path::Path,
    session: &backup::Manifest,
    wanted: &[String],
) -> anyhow::Result<backup::Manifest> {
    let mut kept = Vec::new();
    for raw in wanted {
        let path = std::path::Path::new(raw);
        let rel = if path.is_absolute() {
            path.strip_prefix(backup_root).unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        let rel_str = rel.to_string_lossy();
        let abs_str = backup_root.join(&rel).to_string_lossy().into_owned();
        match session
            .entries
            .iter()
            .find(|e| e.path == rel_str || e.path == abs_str || e.path == *raw)
        {
            Some(entry) => {
                if !kept
                    .iter()
                    .any(|k: &backup::ManifestEntry| k.path == entry.path)
                {
                    kept.push(entry.clone());
                }
            }
            None => {
                return Err(crate::exit::NoMatchError {
                    msg: format!(
                        "no backup entry for path {raw} in session {}",
                        session.timestamp
                    ),
                }
                .into());
            }
        }
    }
    Ok(backup::Manifest {
        timestamp: session.timestamp.clone(),
        entries: kept,
        created_dirs: session.created_dirs.clone(),
    })
}

fn action_label(action: &backup::FileAction) -> &'static str {
    match action {
        backup::FileAction::Modified => "modified",
        backup::FileAction::Created => "created",
        backup::FileAction::Deleted => "deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_backup(dir: &Path, filename: &str, content: &str) -> String {
        let file = dir.join(filename);
        std::fs::write(&file, content).unwrap();
        let mut session = backup::BackupSession::new(dir).unwrap();
        session.save_before_write(&file).unwrap();
        session.finalize().unwrap().unwrap()
    }

    #[test]
    fn list_empty_exits_no_matches() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: true,
            session: None,
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn list_with_sessions_exits_success() {
        let dir = TempDir::new().unwrap();
        create_backup(dir.path(), "a.txt", "content");
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: true,
            session: None,
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    /// #1695: nested crate backups appear when listing from workspace root.
    #[test]
    fn list_finds_nested_monorepo_sessions() {
        let workspace = TempDir::new().unwrap();
        let crate_dir = workspace.path().join("crates").join("foo");
        std::fs::create_dir_all(&crate_dir).unwrap();
        let ts = create_backup(&crate_dir, "lib.txt", "old");

        // Workspace-root only listing would miss nested roots.
        assert!(
            backup::list_sessions(workspace.path()).unwrap().is_empty(),
            "control: flat list_sessions misses nested"
        );

        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(workspace.path().to_string_lossy().to_string());
        let code = run(
            UndoArgs {
                list: true,
                session: None,
                apply: false,
                path: Vec::new(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Restore by session id from workspace cwd.
        std::fs::write(crate_dir.join("lib.txt"), "new").unwrap();
        let code = run(
            UndoArgs {
                list: false,
                session: Some(ts.clone()),
                apply: true,
                path: Vec::new(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(
            std::fs::read_to_string(crate_dir.join("lib.txt")).unwrap(),
            "old"
        );
    }

    #[test]
    fn dry_run_exits_changes_detected() {
        let dir = TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "b.txt", "original");
        std::fs::write(dir.path().join("b.txt"), "modified").unwrap();
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some(ts),
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        // File must remain modified: dry-run never restores without --apply.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "modified"
        );
    }

    #[test]
    fn dry_run_hint_tells_agents_to_pass_apply() {
        assert!(
            UNDO_DRY_RUN_HINT.contains("--apply"),
            "hint must name --apply: {UNDO_DRY_RUN_HINT}"
        );
        assert!(
            UNDO_DRY_RUN_HINT.contains("dry-run") || UNDO_DRY_RUN_HINT.contains("preview"),
            "hint must say dry-run/preview: {UNDO_DRY_RUN_HINT}"
        );
        let preview = UndoPreviewOutput {
            ok: true,
            status: "changes_detected",
            error_kind: Some("changes_detected"),
            hint: UNDO_DRY_RUN_HINT,
            applied: false,
            session: "t".into(),
            project_root: ".".into(),
            file_count: 1,
            entries: vec![],
        };
        let v = serde_json::to_value(&preview).unwrap();
        assert_eq!(v["status"], "changes_detected");
        assert_eq!(v["error_kind"], "changes_detected");
        assert_eq!(
            v["applied"], false,
            "dry-run must set applied:false (#1830): {v}"
        );
        assert!(
            v["hint"].as_str().unwrap().contains("--apply"),
            "JSON preview must include hint: {v}"
        );
    }

    #[test]
    fn apply_contain_refuses_forged_external() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("forged-undo-target");
        std::fs::write(&outside_file, "keep me").unwrap();

        let ext_path = backup::sanitize_rel_path(&outside_file, dir.path())
            .to_string_lossy()
            .into_owned();
        let ts = "forged-undo-contain";
        let session_dir = dir.path().join(backup::BACKUP_DIR).join(ts);
        std::fs::create_dir_all(session_dir.join(Path::new(&ext_path).parent().unwrap())).unwrap();
        std::fs::write(session_dir.join(&ext_path), b"pwned").unwrap();
        let manifest = backup::Manifest {
            timestamp: ts.to_string(),
            entries: vec![backup::ManifestEntry {
                path: ext_path,
                action: backup::FileAction::Modified,
                renamed_from: None,
            }],
            created_dirs: Vec::new(),
        };
        std::fs::write(
            session_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(
            session_dir.join(backup::ORIGIN_SIDECAR),
            backup::ORIGIN_SIDECAR_BYTES,
        )
        .unwrap();

        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        global.contain = true;
        let args = UndoArgs {
            list: false,
            session: Some(ts.to_string()),
            apply: true,
            path: Vec::new(),
        };
        let err = run(args, &global).unwrap_err();
        assert!(
            crate::api::is_guard_rejected(&err) || crate::exit::is_invalid_input(&err),
            "undo --contain must refuse forged external, got: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "keep me");
    }

    #[test]
    fn apply_restores_and_exits_success() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("c.txt");
        let ts = create_backup(dir.path(), "c.txt", "original");
        std::fs::write(&file, "modified").unwrap();
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some(ts),
            apply: true,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    }

    #[test]
    fn apply_path_restores_only_that_file() {
        let dir = TempDir::new().unwrap();
        let keep = dir.path().join("keep.txt");
        let change = dir.path().join("change.txt");
        std::fs::write(&keep, "keep-orig").unwrap();
        std::fs::write(&change, "change-orig").unwrap();
        let mut session = backup::BackupSession::new(dir.path()).unwrap();
        session.save_before_write(&keep).unwrap();
        session.save_before_write(&change).unwrap();
        let ts = session.finalize().unwrap().unwrap();
        std::fs::write(&keep, "keep-new").unwrap();
        std::fs::write(&change, "change-new").unwrap();

        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some(ts.clone()),
            apply: true,
            path: vec!["change.txt".into()],
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(std::fs::read_to_string(&change).unwrap(), "change-orig");
        assert_eq!(
            std::fs::read_to_string(&keep).unwrap(),
            "keep-new",
            "unlisted path must stay modified"
        );
    }

    #[test]
    fn apply_unknown_path_is_no_matches() {
        let dir = TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "c.txt", "original");
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some(ts),
            apply: true,
            path: vec!["missing.txt".into()],
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn apply_without_session_uses_most_recent() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("d.txt");
        create_backup(dir.path(), "d.txt", "v1");
        std::fs::write(&file, "v1-modified").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ts2 = create_backup(dir.path(), "d.txt", "v2");
        std::fs::write(&file, "v3").unwrap();
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: None,
            apply: true,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
    }

    #[test]
    fn nonexistent_session_exits_no_matches() {
        let dir = TempDir::new().unwrap();
        create_backup(dir.path(), "e.txt", "content");
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some("99999999".to_string()),
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn no_sessions_without_apply_exits_no_matches() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_default();
        global.quiet = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: None,
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn no_sessions_json_emits_error_object() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_default();
        global.json = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: None,
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
        // JSON error envelope (ok/error/error_kind) is emitted via
        // emit_error_json_kind. Exit code is the unit-test contract; payload
        // shape is covered by emit_error_json_kind tests in cli/global.rs and
        // by integration coverage for other exit-3 commands.
    }

    #[test]
    fn list_empty_json_exits_no_matches() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_default();
        global.json = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: true,
            session: None,
            apply: false,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn list_json_missing_manifest_is_invalid_input_not_no_matches() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir
            .path()
            .join(backup::BACKUP_DIR)
            .join("incomplete-no-manifest");
        std::fs::create_dir_all(&session_dir).unwrap();

        let global = GlobalFlags {
            json: true,
            cwd: Some(dir.path().to_string_lossy().to_string()),
            ..GlobalFlags::test_default()
        };
        let code = run(
            UndoArgs {
                list: true,
                session: None,
                apply: false,
                path: Vec::new(),
            },
            &global,
        )
        .unwrap();
        assert_ne!(
            code,
            exit::NO_MATCHES,
            "dirs without a readable manifest are not a clean miss"
        );
        assert_eq!(code, exit::FAILURE);

        let (_, warnings) = collect_sessions(dir.path()).unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("manifest.json")),
            "listing must surface the missing manifest path: {warnings:?}"
        );
        let (kind, msg, kind_code) = list_no_usable_sessions(&warnings);
        assert_ne!(kind, "no_matches");
        assert_eq!(kind, "invalid_input");
        assert_eq!(kind_code, exit::FAILURE);
        assert!(
            msg.contains("manifest.json"),
            "JSON error message must name manifest.json so agents see the path: {msg}"
        );
        let payload = serde_json::json!({
            "ok": false,
            "error": msg,
            "error_kind": kind,
        });
        assert_ne!(payload["error_kind"], "no_matches");
        assert_eq!(payload["error_kind"], "invalid_input");
    }

    #[test]
    fn undo_without_list_corrupt_only_is_invalid_input_not_no_matches() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir
            .path()
            .join(backup::BACKUP_DIR)
            .join("incomplete-no-manifest");
        std::fs::create_dir_all(&session_dir).unwrap();

        let global = GlobalFlags {
            json: true,
            cwd: Some(dir.path().to_string_lossy().to_string()),
            ..GlobalFlags::test_default()
        };
        let code = run(
            UndoArgs {
                list: false,
                session: None,
                apply: false,
                path: Vec::new(),
            },
            &global,
        )
        .unwrap();
        assert_ne!(code, exit::NO_MATCHES);
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn list_json_with_session_succeeds() {
        let dir = TempDir::new().unwrap();
        let ts = create_backup(dir.path(), "listed.txt", "content");
        let global = GlobalFlags {
            json: true,
            cwd: Some(dir.path().to_string_lossy().to_string()),
            ..GlobalFlags::test_default()
        };
        let code = run(
            UndoArgs {
                list: true,
                session: None,
                apply: false,
                path: Vec::new(),
            },
            &global,
        )
        .unwrap();
        assert_eq!(code, exit::SUCCESS);

        let (sessions, warnings) = collect_sessions(dir.path()).unwrap();
        assert!(
            warnings.is_empty(),
            "usable session must not warn: {warnings:?}"
        );
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].1.timestamp, ts);
        let payload = serde_json::to_value(&UndoListOutput {
            items: sessions
                .iter()
                .map(|(root, manifest)| UndoListEntry {
                    timestamp: manifest.timestamp.clone(),
                    project_root: display_root(dir.path(), root),
                    file_count: manifest.entries.len(),
                    entries: manifest.entries.clone(),
                })
                .collect(),
            warnings,
        })
        .unwrap();
        assert_eq!(payload["items"][0]["timestamp"], ts);
        assert!(payload["items"][0].get("project_root").is_some());
        assert!(payload["items"][0].get("file_count").is_some());
        assert!(payload["items"][0].get("entries").is_some());
        assert_eq!(payload["warnings"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn apply_json_emits_restored_object() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("f.txt");
        let ts = create_backup(dir.path(), "f.txt", "original");
        std::fs::write(&file, "modified").unwrap();
        let mut global = GlobalFlags::test_default();
        global.json = true;
        global.cwd = Some(dir.path().to_string_lossy().to_string());
        let args = UndoArgs {
            list: false,
            session: Some(ts),
            apply: true,
            path: Vec::new(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
        // JSON restored object is emitted via global.emit_json. We verify
        // the exit code and that the file was actually restored.
    }

    #[test]
    fn action_label_values() {
        assert_eq!(action_label(&backup::FileAction::Modified), "modified");
        assert_eq!(action_label(&backup::FileAction::Created), "created");
        assert_eq!(action_label(&backup::FileAction::Deleted), "deleted");
    }

    fn preview_entry(
        action: backup::FileAction,
        renamed_from: Option<&str>,
    ) -> backup::ManifestEntry {
        backup::ManifestEntry {
            path: "dest".to_string(),
            action,
            renamed_from: renamed_from.map(str::to_string),
        }
    }

    #[test]
    fn preview_action_created_with_renamed_from_is_rename_back() {
        let entry = preview_entry(backup::FileAction::Created, Some("src"));
        assert_eq!(preview_action(&entry), "rename back to src");
    }

    #[test]
    fn preview_action_created_only_is_delete() {
        let entry = preview_entry(backup::FileAction::Created, None);
        assert_eq!(preview_action(&entry), "delete (was created by apply)");
    }

    #[test]
    fn preview_action_modified_is_restore_original() {
        let entry = preview_entry(backup::FileAction::Modified, None);
        assert_eq!(preview_action(&entry), "restore original");
    }

    #[test]
    fn preview_action_deleted_is_recreate() {
        let entry = preview_entry(backup::FileAction::Deleted, None);
        assert_eq!(preview_action(&entry), "recreate (was deleted by apply)");
    }
}
