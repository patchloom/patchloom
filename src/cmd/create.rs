use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::cmd::output::execute_via_engine;
use crate::plan::Operation;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom create src/config.json --content '{\"version\": 1}' --apply
  echo 'hello' | patchloom create greeting.txt --stdin --apply")]
pub struct CreateArgs {
    /// Path of the file to create.
    pub file: String,
    /// Content to write (alternative to --stdin).
    #[arg(long)]
    pub content: Option<String>,
    // ref:create-mode:stdin
    /// Read content from stdin.
    #[arg(long)]
    pub stdin: bool,
    // ref:create-mode:force
    /// Overwrite if file already exists.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct CreateOutput {
    ok: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

pub fn run(args: CreateArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("create: file={}, force={}", args.file, args.force);
    if args.content.is_some() && args.stdin {
        let msg = "--content and --stdin cannot be combined";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let content = if let Some(ref c) = args.content {
        c.clone()
    } else if args.stdin {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let msg = "either --content or --stdin must be provided";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    };

    let cwd = global.resolve_cwd()?;
    global.check_paths_contained(&cwd, [&args.file])?;
    let path = cwd.join(&args.file);
    if path.exists() && !path.is_file() {
        let msg = format!("target is not a file: {}", args.file);
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }
    // Catch exists before the engine so --json gets error_kind (apply and
    // preview). --force continues into FileCreate overwrite.
    if !args.force && path.exists() {
        let msg = format!("file already exists: {}", args.file);
        global.emit_error_json_kind(Some("already_exists"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let op = Operation::FileCreate {
        path: args.file.clone(),
        content,
        force: Some(args.force),
    };

    let check_msg = format!("would create {}", args.file);
    let apply_msg = format!("created {}", args.file);

    execute_via_engine(
        op,
        global,
        |phase, diff| CreateOutput {
            ok: true,
            path: args.file.clone(),
            diff,
            applied: match phase {
                WritePhase::Confirmed(a) => Some(a),
                _ => None,
            },
        },
        &check_msg,
        &apply_msg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn create_writes_file_with_correct_content() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("hello world\n".to_string()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn create_with_contain_rejects_parent_escape() {
        let dir = TempDir::new().unwrap();
        let args = CreateArgs {
            file: "../escape-outside.txt".into(),
            content: Some("nope\n".into()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::with_cwd(dir.path());
        global.apply = true;
        global.contain = true;

        let err = run(args, &global).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes") || msg.contains("rejected"),
            "expected containment error, got: {msg}"
        );
        assert!(!dir.path().join("../escape-outside.txt").exists());
    }

    #[test]
    fn create_without_contain_allows_parent_escape() {
        let dir = TempDir::new().unwrap();
        let name = format!(
            "patchloom-create-escape-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        let args = CreateArgs {
            file: format!("../{name}"),
            content: Some("escaped\n".into()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::with_cwd(dir.path());
        global.apply = true;
        // contain defaults to false

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        let outside = dir.path().parent().unwrap().join(&name);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "escaped\n");
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn create_with_contain_allows_in_workspace_relative_path() {
        let dir = TempDir::new().unwrap();
        let args = CreateArgs {
            file: "inside.txt".into(),
            content: Some("ok\n".into()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::with_cwd(dir.path());
        global.apply = true;
        global.contain = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(
            fs::read_to_string(dir.path().join("inside.txt")).unwrap(),
            "ok\n"
        );
    }

    #[test]
    fn create_with_contain_allows_absolute_path_inside_workspace() {
        // CLI --contain uses AllowIfContained (#1451): absolute paths under
        // --cwd are accepted so agents may pass absolutized paths.
        let dir = TempDir::new().unwrap();
        let abs = dir.path().join("abs.txt");
        let args = CreateArgs {
            file: abs.to_string_lossy().into_owned(),
            content: Some("ok\n".into()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::with_cwd(dir.path());
        global.apply = true;
        global.contain = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert_eq!(fs::read_to_string(&abs).unwrap(), "ok\n");
    }

    #[test]
    fn create_with_contain_rejects_absolute_path_outside_workspace() {
        let dir = TempDir::new().unwrap();
        let outside = dir.path().parent().unwrap().join(format!(
            "patchloom-create-outside-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let args = CreateArgs {
            file: outside.to_string_lossy().into_owned(),
            content: Some("nope\n".into()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::with_cwd(dir.path());
        global.apply = true;
        global.contain = true;

        let err = run(args, &global).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escapes")
                || msg.contains("rejected")
                || msg.contains("workspace guard")
                || msg.contains("absolute"),
            "expected outside-workspace containment error, got: {msg}"
        );
        assert!(!outside.exists());
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn create_refuses_to_overwrite_existing_file_without_force() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "original\n").unwrap();

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("new content\n".to_string()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);

        // Original content should be unchanged.
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "original\n");
    }

    #[test]
    fn create_with_force_overwrites_existing_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "original\n").unwrap();

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("overwritten\n".to_string()),
            stdin: false,
            force: true,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "overwritten\n");
    }

    #[test]
    fn create_rejects_directory_target_even_with_force() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("folder");
        fs::create_dir(&target).unwrap();

        let args = CreateArgs {
            file: target.to_string_lossy().into_owned(),
            content: Some("hello\n".to_string()),
            stdin: false,
            force: true,
            write: Default::default(),
        };

        let code = run(args, &GlobalFlags::test_default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn create_with_check_returns_exit_2() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("check.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("content\n".to_string()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File should NOT have been created.
        assert!(!file.exists());
    }

    #[test]
    fn create_default_mode_shows_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("diff_preview.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("hello world\n".to_string()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        // Default mode: no --apply, no --check.
        let global = GlobalFlags::test_default();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File should NOT be created in default diff-preview mode.
        assert!(!file.exists());
    }

    #[test]
    fn create_with_no_content_source_returns_error() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("no_content.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: None,
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_default();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn create_apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("backup test\n".to_string()),
            stdin: false,
            force: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // A backup session should exist.
        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(backup_dir.exists(), "backup directory should be created");

        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(sessions.len(), 1, "exactly one backup session expected");
    }

    #[test]
    fn create_force_apply_creates_backup_session() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("existing.txt");
        fs::write(&file, "original\n").unwrap();

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("overwritten\n".to_string()),
            stdin: false,
            force: true,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // Backup session should exist with the original content.
        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(backup_dir.exists(), "backup directory should be created");

        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(sessions.len(), 1, "exactly one backup session expected");
    }

    #[test]
    fn create_rejects_content_and_stdin_together() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("dual_source.txt");

        let args = CreateArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("inline\n".to_string()),
            stdin: true,
            force: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_default();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}
