use crate::cli::global::GlobalFlags;
use crate::cmd::output::execute_via_engine;
use crate::plan::Operation;
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

pub fn run(args: AppendArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    run_content_inject(
        &args.file,
        args.content.as_deref(),
        args.stdin,
        ContentPosition::Append,
        global,
    )
}

// ---------------------------------------------------------------------------
// Shared append/prepend implementation
// ---------------------------------------------------------------------------

pub(crate) enum ContentPosition {
    Append,
    Prepend,
}

#[derive(Debug, Serialize)]
struct ContentInjectOutput {
    ok: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

pub(crate) fn run_content_inject(
    file: &str,
    content: Option<&str>,
    stdin: bool,
    position: ContentPosition,
    global: &GlobalFlags,
) -> anyhow::Result<u8> {
    let verb = match position {
        ContentPosition::Append => "append",
        ContentPosition::Prepend => "prepend",
    };

    crate::verbose!("{verb}: file={file}");
    if content.is_some() && stdin {
        let msg = "--content and --stdin cannot be combined";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let inject_content = if let Some(c) = content {
        c.to_string()
    } else if stdin {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let msg = "either --content or --stdin must be provided";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    };

    let cwd = global.resolve_cwd()?;
    // Containment before existence so --contain cannot be distinguished from
    // "missing file" for escaped paths (agent sandbox).
    global.check_paths_contained(&cwd, [file])?;
    let path = cwd.join(file);
    if !path.exists() {
        let msg = format!("file does not exist: {file}");
        global.emit_error_json_kind(Some("not_found"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }
    if !path.is_file() {
        let msg = format!("target is not a file: {file}");
        global.emit_error_json_kind(Some("invalid_input"), &msg)?;
        return Ok(crate::exit::FAILURE);
    }
    // Fail before the engine so --json gets error_kind and binaries are not
    // rewritten as text (NUL is valid UTF-8).
    if let Err(e) = crate::ops::file::ensure_not_binary_file(&path, file) {
        global.emit_error_json_kind(Some("invalid_input"), &e.msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let op = match position {
        ContentPosition::Append => Operation::FileAppend {
            path: file.to_string(),
            content: inject_content,
        },
        ContentPosition::Prepend => Operation::FilePrepend {
            path: file.to_string(),
            content: inject_content,
        },
    };

    let check_msg = format!("would {verb} to {file}");
    let apply_msg = format!("{verb}ed to {file}");
    let file_owned = file.to_string();

    execute_via_engine(
        op,
        global,
        |phase, diff| ContentInjectOutput {
            ok: true,
            path: file_owned.clone(),
            diff,
            applied: phase.applied_flag(),
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn append_requires_content_or_stdin() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "existing\n").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: None,
            stdin: false,
            write: Default::default(),
        };

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
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

        let code = run(args, &GlobalFlags::default()).unwrap();
        assert_eq!(code, exit::FAILURE);
    }

    #[test]
    fn append_rejects_binary_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.bin");
        fs::write(&file, b"hello\x00world").unwrap();

        let args = AppendArgs {
            file: file.to_string_lossy().into_owned(),
            content: Some("evil\n".into()),
            stdin: false,
            write: Default::default(),
        };
        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::FAILURE);
        assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");
    }

    #[test]
    fn prepend_rejects_binary_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.bin");
        fs::write(&file, b"hello\x00world").unwrap();

        let code = run_content_inject(
            &file.to_string_lossy(),
            Some("evil\n"),
            false,
            ContentPosition::Prepend,
            &GlobalFlags {
                apply: true,
                ..GlobalFlags::test_with_cwd(dir.path())
            },
        )
        .unwrap();
        assert_eq!(code, exit::FAILURE);
        assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");
    }
}
