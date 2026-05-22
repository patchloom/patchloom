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
        let mut backup = crate::backup::BackupSession::new(&cwd)?;
        backup.save_before_delete(&path)?;
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to delete {}", path.display()))?;
        backup.finalize()?;
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
        let mut backup = crate::backup::BackupSession::new(&cwd)?;
        backup.save_before_delete(&path)?;
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to delete {}", path.display()))?;
        backup.finalize()?;
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
    fn nonexistent_file_returns_error() {
        let result = run(
            make_args("/tmp/nonexistent_patchloom_test_file_xyz.txt"),
            &GlobalFlags::default(),
        );
        assert!(result.is_err());
    }
}
