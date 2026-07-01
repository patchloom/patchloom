use crate::cli::global::GlobalFlags;
use crate::cmd::append::{ContentPosition, run_content_inject};
use clap::Args;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom prepend src/main.rs --content '// Copyright 2026\\n' --apply
  echo '# Header' | patchloom prepend README.md --stdin --apply")]
pub struct PrependArgs {
    /// Path of the file to prepend to.
    pub file: String,
    /// Content to prepend (alternative to --stdin).
    #[arg(long)]
    pub content: Option<String>,
    /// Read content from stdin.
    #[arg(long)]
    pub stdin: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub fn run(args: PrependArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    run_content_inject(
        &args.file,
        args.content.as_deref(),
        args.stdin,
        ContentPosition::Prepend,
        global,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn prepend_adds_content_to_beginning() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing content\n").unwrap();

        let args = PrependArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("header\n".to_string()),
            stdin: false,
            write: Default::default(),
        };
        let global = GlobalFlags::test_apply();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "header\nexisting content\n");
    }

    #[test]
    fn prepend_check_returns_exit_2() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing\n").unwrap();

        let args = PrependArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("new\n".to_string()),
            stdin: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_default();
        global.check = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        assert_eq!(fs::read_to_string(&file).unwrap(), "existing\n");
    }

    #[test]
    fn prepend_fails_if_file_does_not_exist() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("missing.txt");

        let args = PrependArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("content\n".to_string()),
            stdin: false,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("file does not exist"));
    }

    #[test]
    fn prepend_rejects_content_and_stdin_together() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing\n").unwrap();

        let args = PrependArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("inline\n".to_string()),
            stdin: true,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("--content and --stdin"));
    }

    #[test]
    fn prepend_rejects_directory_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("folder");
        fs::create_dir(&target).unwrap();

        let args = PrependArgs {
            file: target.to_string_lossy().into_owned(),
            content: Some("content\n".to_string()),
            stdin: false,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::default()).unwrap_err();
        assert!(err.to_string().contains("target is not a file"));
    }
}
