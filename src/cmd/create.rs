use crate::cli::global::GlobalFlags;
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_write, policy_from_flags};
use anyhow::bail;
use clap::Args;
use serde::Serialize;
use std::fs;
use std::io::Read;

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Path of the file to create.
    #[arg(long)]
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
}

fn make_diff_output(path: &str, content: &str) -> String {
    let diff = unified_diff(path, "", content);
    let total_files_changed = usize::from(diff.has_changes);
    let diff_result = DiffResult {
        diffs: vec![diff],
        total_files_changed,
    };
    format_diff_result(&diff_result)
}

pub fn run(args: CreateArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;

    // Resolve content from --content or --stdin.
    let content = if let Some(ref c) = args.content {
        c.clone()
    } else if args.stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        bail!("either --content or --stdin must be provided");
    };

    let path = cwd.join(&args.file);

    // Check if file already exists.
    if path.exists() && !args.force {
        bail!("file already exists: {}", args.file);
    }

    // --check mode: report that file would be created, exit 2.
    if global.check {
        if global.json {
            let output = CreateOutput {
                ok: true,
                path: args.file.clone(),
                diff: None,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !global.quiet {
            println!("would create {}", args.file);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: write file using atomic_write.
    if global.apply {
        let policy = policy_from_flags(global, Some(&path));

        // Ensure parent directories exist.
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        atomic_write(&path, &content, &policy)?;

        if global.json {
            let diff_text = if global.diff {
                Some(make_diff_output(&args.file, &content))
            } else {
                None
            };
            let output = CreateOutput {
                ok: true,
                path: args.file.clone(),
                diff: diff_text,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if global.diff {
            print!("{}", make_diff_output(&args.file, &content));
        } else if !global.quiet {
            println!("created {}", args.file);
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show unified diff of changes.
    if global.json {
        let output = CreateOutput {
            ok: true,
            path: args.file.clone(),
            diff: Some(make_diff_output(&args.file, &content)),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print!("{}", make_diff_output(&args.file, &content));
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

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
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world\n");
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
        let mut global = default_global();
        global.apply = true;

        let err = run(args, &global).unwrap_err();
        assert!(err.to_string().contains("file already exists:"));

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
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "overwritten\n");
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
        let mut global = default_global();
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
        let global = default_global();

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

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
        let global = default_global();

        let err = run(args, &global).unwrap_err();
        assert!(err.to_string().contains("--content or --stdin"));
    }
}
