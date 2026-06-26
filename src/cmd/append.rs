use crate::cli::global::GlobalFlags;
use crate::cmd::output::execute_via_engine;
use crate::cmd::write_dispatch::WritePhase;
use crate::plan::Operation;
use anyhow::bail;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom append tests/integration.rs --content 'fn new_test() {}' --apply
  echo 'new content' | patchloom append tests/integration.rs --stdin --apply")]
pub struct AppendArgs {
    /// Path of the file to append to.
    pub file: String,
    /// Content to append (alternative to --stdin).
    #[arg(long)]
    pub content: Option<String>,
    /// Read content from stdin.
    #[arg(long)]
    pub stdin: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct AppendOutput {
    ok: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

pub fn run(args: AppendArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    if args.content.is_some() && args.stdin {
        bail!("--content and --stdin cannot be combined");
    }

    let append_content = if let Some(ref c) = args.content {
        c.clone()
    } else if args.stdin {
        std::io::read_to_string(std::io::stdin())?
    } else {
        bail!("either --content or --stdin must be provided");
    };

    // Pre-validation: the engine's FileAppend op checks existence too,
    // but we validate here for consistent CLI error messages.
    let cwd = global.resolve_cwd()?;
    let path = cwd.join(&args.file);
    if !path.exists() {
        bail!("file does not exist: {}", args.file);
    }
    if !path.is_file() {
        bail!("target is not a file: {}", args.file);
    }

    let op = Operation::FileAppend {
        path: args.file.clone(),
        content: append_content,
    };

    let check_msg = format!("would append to {}", args.file);
    let apply_msg = format!("appended to {}", args.file);

    execute_via_engine(
        op,
        global,
        |phase, diff| AppendOutput {
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
    fn append_adds_content_to_existing_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "line one\n").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("line two\n".to_string()),
            stdin: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_apply();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "line one\nline two\n");
    }

    #[test]
    fn append_inserts_newline_if_file_lacks_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "no trailing newline").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("appended\n".to_string()),
            stdin: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_apply();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "no trailing newline\nappended\n");
    }

    #[test]
    fn append_fails_if_file_does_not_exist() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("missing.txt");

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("content\n".to_string()),
            stdin: false,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("file does not exist"));
    }

    #[test]
    fn append_check_returns_exit_2() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing\n").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("new\n".to_string()),
            stdin: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        // File should be unchanged.
        assert_eq!(fs::read_to_string(&file).unwrap(), "existing\n");
    }

    #[test]
    fn append_rejects_content_and_stdin_together() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing\n").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("inline\n".to_string()),
            stdin: true,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("--content and --stdin"));
    }

    #[test]
    fn append_rejects_directory_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("folder");
        fs::create_dir(&target).unwrap();

        let args = AppendArgs {
            file: target.to_string_lossy().into_owned(),
            content: Some("content\n".to_string()),
            stdin: false,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("target is not a file"));
    }
}
