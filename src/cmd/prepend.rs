use crate::cli::global::GlobalFlags;
use crate::cmd::append::{ContentPosition, run_content_inject};
use clap::Args;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom prepend src/main.rs --content '// Copyright 2026' --apply
  echo '# Header' | patchloom prepend README.md --stdin --apply

NOTES:
  If --content does not already end with a newline, prepend inserts the file's
  line ending after it so existing text starts on the next line.")]
pub struct PrependArgs {
    /// Path of the file to prepend to.
    pub file: String,
    /// Content to prepend (alternative to --stdin).
    /// If this text does not already end with a newline, the file's line ending
    /// is inserted after it.
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn prepend_help_example_does_not_advertise_backslash_n_escape() {
        // `--content` is literal; bash single quotes do not decode `\n`,
        // and patchloom does not interpret C-escapes (#2192).
        use clap::CommandFactory;
        let cmd = crate::cli::Cli::command();
        let prepend = cmd.find_subcommand("prepend").expect("prepend");
        let mut buf = Vec::new();
        prepend.clone().write_long_help(&mut buf).unwrap();
        let help = String::from_utf8(buf).unwrap();
        assert!(
            !help.contains("--content '// Copyright 2026\\n'"),
            "help example must not show a \\n escape inside single quotes:\n{help}"
        );
        assert!(
            help.contains("--content '// Copyright 2026'"),
            "help should still show the copyright prepend example:\n{help}"
        );
        assert!(
            help.contains("line ending") && help.contains("newline"),
            "help must mention the separator newline (#2199):\n{help}"
        );
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }
}
