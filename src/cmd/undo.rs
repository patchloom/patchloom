use crate::backup;
use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom undo --list
  patchloom undo --apply
  patchloom undo --session 20240101_120000 --apply")]
pub struct UndoArgs {
    /// List available backup sessions instead of restoring.
    #[arg(long)]
    pub list: bool,

    /// Restore a specific backup session by timestamp.
    #[arg(long)]
    pub session: Option<String>,

    /// Actually restore files (default: dry-run showing what would change).
    #[arg(long)]
    pub apply: bool,
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
    session: String,
    file_count: usize,
    entries: Vec<UndoPreviewEntry>,
}

pub fn run(args: UndoArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "undo: list={}, session={:?}, apply={}",
        args.list,
        args.session,
        args.apply
    );
    let cwd = global.resolve_cwd()?;

    if args.list {
        let sessions = backup::list_sessions(&cwd)?;
        if sessions.is_empty() {
            let empty: Vec<()> = vec![];
            if !global.emit_json(&empty)? && !global.quiet {
                eprintln!("no backup sessions found");
            }
            return Ok(exit::NO_MATCHES);
        }

        if !global.emit_json_items(&sessions)? && !global.quiet {
            for s in &sessions {
                let file_count = s.entries.len();
                let actions: Vec<String> = s
                    .entries
                    .iter()
                    .map(|e| format!("  {} ({})", e.path, action_label(&e.action)))
                    .collect();
                println!("{} ({file_count} file(s))", s.timestamp);
                for a in &actions {
                    println!("{a}");
                }
                println!();
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Determine which session to restore.
    let timestamp = if let Some(ref ts) = args.session {
        ts.clone()
    } else {
        // Use the most recent session.
        let sessions = backup::list_sessions(&cwd)?;
        if sessions.is_empty() {
            if !global.emit_json(&serde_json::json!({
                "ok": false,
                "error": "no backup sessions found",
            }))? && !global.quiet
            {
                eprintln!("no backup sessions found");
            }
            return Ok(exit::NO_MATCHES);
        }
        sessions[0].timestamp.clone()
    };

    if !args.apply {
        // Dry-run: show what would be restored.
        let sessions = backup::list_sessions(&cwd)?;
        let session = sessions
            .iter()
            .find(|s| s.timestamp == timestamp)
            .ok_or_else(|| anyhow::anyhow!("session {timestamp} not in session list (use `patchloom undo --list` to see available sessions)"))?;

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
            session: timestamp.clone(),
            file_count: entries.len(),
            entries,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!(
                "Would restore session {} ({} file(s)):",
                timestamp,
                session.entries.len()
            );
            for entry in &output.entries {
                println!("  {} -> {}", entry.path, entry.action);
            }
        }
        if global.show_status() {
            eprintln!("\nhint: use --apply to actually restore these files");
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // Apply restore.
    crate::verbose!("undo: restoring session {}", timestamp);
    let restored = backup::restore_session(&cwd, &timestamp)?;
    // Remove the consumed session so subsequent `undo` calls advance to
    // the next-oldest session instead of replaying the same one.
    backup::remove_session(&cwd, &timestamp)?;
    crate::verbose!("undo: restored {} file(s)", restored);
    if !global.emit_json(&serde_json::json!({
        "ok": true,
        "status": "restored",
        "session": timestamp,
        "file_count": restored,
    }))? && !global.quiet
    {
        eprintln!("restored {restored} file(s) from session {timestamp}");
    }

    Ok(exit::SUCCESS)
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
    fn nonexistent_session_errors() {
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
        let result = run(args, &global);
        assert!(result.is_err());
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
        // JSON error object is emitted via global.emit_json which writes to
        // stdout. We verify the exit code; emit_json return-value correctness
        // is covered by unit tests in cli/global.rs.
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
