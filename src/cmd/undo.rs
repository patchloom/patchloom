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
  patchloom undo --session 20240101_120000 --apply

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

    /// Actually restore files. Without this flag, undo only previews
    /// (exit 2) and does not change the working tree.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Serialize)]
struct UndoListEntry {
    timestamp: String,
    /// Backup project root relative to cwd when possible (#1695).
    project_root: String,
    file_count: usize,
    entries: Vec<backup::ManifestEntry>,
}

#[derive(Debug, Serialize)]
struct UndoPreviewEntry {
    path: String,
    action: String,
}

#[derive(Debug, Serialize)]
struct UndoPreviewOutput {
    ok: bool,
    status: &'static str,
    /// Always set on dry-run so agents do not treat exit 2 as a completed restore.
    hint: &'static str,
    session: String,
    project_root: String,
    file_count: usize,
    entries: Vec<UndoPreviewEntry>,
}

const UNDO_DRY_RUN_HINT: &str = "pass --apply to restore files (default is dry-run preview; exit 2 means changes would be made)";

pub fn run(args: UndoArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "undo: list={}, session={:?}, apply={}",
        args.list,
        args.session,
        args.apply
    );
    let cwd = global.resolve_cwd()?;

    if args.list {
        let sessions = collect_sessions(&cwd)?;
        if sessions.is_empty() {
            // Match other CLI exit-3 paths: agents branch on error_kind without
            // scraping stderr (empty array alone was inconsistent with undo apply).
            global.emit_error_json_kind(Some("no_matches"), "no backup sessions found")?;
            return Ok(exit::NO_MATCHES);
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
        Err(e) => return Err(e),
    };

    if !args.apply {
        let entries: Vec<UndoPreviewEntry> = session
            .entries
            .iter()
            .map(|entry| UndoPreviewEntry {
                path: entry.path.clone(),
                action: match entry.action {
                    backup::FileAction::Modified => "restore original",
                    backup::FileAction::Created => "delete (was created by apply)",
                    backup::FileAction::Deleted => "recreate (was deleted by apply)",
                }
                .to_string(),
            })
            .collect();

        let output = UndoPreviewOutput {
            ok: true,
            status: "changes_detected",
            hint: UNDO_DRY_RUN_HINT,
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
    let restored = backup::restore_session(&backup_root, &timestamp)?;
    // Remove the consumed session so subsequent `undo` calls advance to
    // the next-oldest session instead of replaying the same one.
    backup::remove_session(&backup_root, &timestamp)?;
    crate::verbose!("undo: restored {} file(s)", restored);
    if !global.emit_json(&serde_json::json!({
        "ok": true,
        "status": "restored",
        "session": timestamp,
        "project_root": display_root(&cwd, &backup_root),
        "file_count": restored,
    }))? && !global.quiet
    {
        eprintln!("restored {restored} file(s) from session {timestamp}");
    }

    Ok(exit::SUCCESS)
}

/// Sessions under `cwd` and nested monorepo roots, newest first (#1695).
fn collect_sessions(
    cwd: &std::path::Path,
) -> anyhow::Result<Vec<(std::path::PathBuf, backup::Manifest)>> {
    let listings = backup::list_sessions_under(
        cwd,
        &backup::ListSessionsOptions {
            descendants: true,
            ancestors: false,
            max_depth: Some(8),
        },
    )?;
    let mut all = Vec::new();
    for listing in listings {
        for session in listing.sessions {
            all.push((listing.project_root.clone(), session));
        }
    }
    all.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
    Ok(all)
}

fn resolve_session(
    cwd: &std::path::Path,
    wanted: Option<&str>,
) -> anyhow::Result<Option<(std::path::PathBuf, String, backup::Manifest)>> {
    let sessions = collect_sessions(cwd)?;
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
        return Ok(None);
    }
    let (root, manifest) = sessions.into_iter().next().expect("non-empty");
    let ts = manifest.timestamp.clone();
    Ok(Some((root, ts, manifest)))
}

fn display_root(cwd: &std::path::Path, root: &std::path::Path) -> String {
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
            hint: UNDO_DRY_RUN_HINT,
            session: "t".into(),
            project_root: ".".into(),
            file_count: 1,
            entries: vec![],
        };
        let v = serde_json::to_value(&preview).unwrap();
        assert_eq!(v["status"], "changes_detected");
        assert!(
            v["hint"].as_str().unwrap().contains("--apply"),
            "JSON preview must include hint: {v}"
        );
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
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
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
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
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
}
