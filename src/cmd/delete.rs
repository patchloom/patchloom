use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::Context;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom delete old_config.json --apply
  patchloom delete temp.log --check")]
pub struct DeleteArgs {
    /// Path of the file to delete.
    pub file: String,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct DeleteOutput {
    ok: bool,
    path: String,
    applied: bool,
}

fn delete_with_backup(path: &std::path::Path, cwd: &std::path::Path) -> anyhow::Result<()> {
    let mut backup = crate::backup::BackupSession::new(cwd)?;
    backup.save_before_delete(path)?;
    std::fs::remove_file(path).with_context(|| format!("failed to delete {}", path.display()))?;
    backup.finalize()?;
    Ok(())
}

/// Delete a file with backup, without writing to stdout.
/// Used by the MCP server for direct in-process deletes.
#[cfg(feature = "mcp")]
pub(crate) fn apply_delete(path: &std::path::Path, cwd: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("file not found: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("target is not a file: {}", path.display());
    }
    delete_with_backup(path, cwd)
}

pub fn run(args: DeleteArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let path = cwd.join(&args.file);

    if !path.exists() {
        anyhow::bail!("file not found: {}", args.file);
    }
    if !path.is_file() {
        anyhow::bail!("target is not a file: {}", args.file);
    }

    if global.check {
        let output = DeleteOutput {
            ok: true,
            path: args.file.clone(),
            applied: false,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!("would delete {}", args.file);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    if global.apply {
        delete_with_backup(&path, &cwd)?;
        crate::write::run_format_command(global, &cwd)?;
        let output = DeleteOutput {
            ok: true,
            path: args.file.clone(),
            applied: true,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!("deleted {}", args.file);
        }
        return Ok(exit::SUCCESS);
    }

    if global.confirm && (global.json || global.jsonl) {
        let applied = global.should_apply();
        if applied {
            delete_with_backup(&path, &cwd)?;
            crate::write::run_format_command(global, &cwd)?;
        }
        let output = DeleteOutput {
            ok: true,
            path: args.file.clone(),
            applied,
        };
        global.emit_json(&output)?;
        return Ok(exit::SUCCESS);
    }

    // Default: dry-run (show what would be deleted).
    let output = DeleteOutput {
        ok: true,
        path: args.file.clone(),
        applied: false,
    };
    if !global.emit_json(&output)? && !global.quiet {
        println!("would delete {}", args.file);
    }

    // --confirm: prompt after showing preview, then delete if confirmed.
    if global.should_apply() {
        delete_with_backup(&path, &cwd)?;
        crate::write::run_format_command(global, &cwd)?;
        if global.show_status() {
            eprintln!("deleted {}", args.file);
        }
    }
    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_args(path: &str) -> DeleteArgs {
        DeleteArgs {
            file: path.to_string(),
            write: crate::cli::global::WriteFlags::default(),
        }
    }

    #[test]
    fn dry_run_does_not_remove_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let code = run(make_args(&file.to_string_lossy()), &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(file.exists(), "dry-run should not delete the file");
    }

    #[test]
    fn apply_removes_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        };
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists(), "--apply should delete the file");
    }

    #[test]
    fn check_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags {
            check: true,
            ..GlobalFlags::default()
        };
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(file.exists(), "--check should not delete the file");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "content",
            "--check must not modify file content"
        );
    }

    #[test]
    fn directory_target_returns_error() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("folder");
        std::fs::create_dir(&target).unwrap();

        let result = run(
            make_args(&target.to_string_lossy()),
            &GlobalFlags::default(),
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("target is not a file")
        );
    }

    #[test]
    fn apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            apply: true,
            ..GlobalFlags::default()
        };
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists());

        // A backup session should exist.
        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(backup_dir.exists(), "backup directory should be created");

        let sessions: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(sessions.len(), 1, "exactly one backup session expected");
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let result = run(
            make_args("/tmp/nonexistent_patchloom_test_file_xyz.txt"),
            &GlobalFlags::default(),
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("file not found"),
            "error should mention 'file not found'"
        );
    }
}
