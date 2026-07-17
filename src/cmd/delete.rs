use crate::cli::global::GlobalFlags;
use crate::cmd::output::execute_via_engine_no_preview_diffs;
use crate::plan::Operation;
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
    /// Backup session id after a successful apply (#1802).
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

pub fn run(args: DeleteArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("delete: file={}", args.file);
    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [&args.file])?;
    let path = cwd.join(&args.file);

    if !path.exists() {
        let msg = format!("file not found: {}", args.file);
        global.emit_error_json_kind(Some("not_found"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }
    if !path.is_file() {
        let msg = format!("target is not a file: {}", args.file);
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let op = Operation::FileDelete {
        path: args.file.clone(),
    };

    let check_msg = format!("would delete {}", args.file);
    let apply_msg = format!("deleted {}", args.file);

    execute_via_engine_no_preview_diffs(
        op,
        global,
        |phase, _diff, backup| DeleteOutput {
            ok: true,
            path: args.file.clone(),
            applied: phase.applied_flag().unwrap_or(false),
            backup_session: backup,
        },
        &check_msg,
        &apply_msg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
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
        assert_eq!(code, exit::CHANGES_DETECTED);
        assert!(file.exists(), "dry-run should not delete the file");
    }

    #[test]
    fn apply_removes_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let global = GlobalFlags::test_apply();
        let code = run(make_args(&file.to_string_lossy()), &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!file.exists(), "--apply should delete the file");
    }

    #[test]
    fn check_returns_changes_detected() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let mut global = GlobalFlags::test_default();
        global.check = true;
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

        let code = run(
            make_args(&target.to_string_lossy()),
            &GlobalFlags::default(),
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("target.txt");
        std::fs::write(&file, "content").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
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
        let code = run(
            make_args("/tmp/nonexistent_patchloom_test_file_xyz.txt"),
            &GlobalFlags::default(),
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}
