use crate::backup;
use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;

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

pub fn run(args: UndoArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;

    if args.list {
        let sessions = backup::list_sessions(&cwd)?;
        if sessions.is_empty() {
            if global.show_status() {
                eprintln!("no backup sessions found");
            }
            return Ok(exit::NO_MATCHES);
        }

        if global.json {
            let json = serde_json::to_string_pretty(&sessions)?;
            println!("{json}");
        } else {
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
            if global.show_status() {
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
            .ok_or_else(|| anyhow::anyhow!("no backup session found for {timestamp}"))?;

        println!(
            "Would restore session {} ({} file(s)):",
            timestamp,
            session.entries.len()
        );
        for entry in &session.entries {
            let action = match entry.action {
                backup::FileAction::Modified => "restore original",
                backup::FileAction::Created => "delete (was created by apply)",
                backup::FileAction::Deleted => "recreate (was deleted by apply)",
            };
            println!("  {} -> {action}", entry.path);
        }
        if global.show_status() {
            eprintln!("\nhint: use --apply to actually restore these files");
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // Apply restore.
    let restored = backup::restore_session(&cwd, &timestamp)?;
    if global.show_status() {
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
