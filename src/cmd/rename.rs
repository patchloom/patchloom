use crate::cli::global::GlobalFlags;
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_create_new, atomic_write, policy_from_flags};
use clap::Args;
use serde::Serialize;
use std::fs;

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// Source file path.
    #[arg(long)]
    pub from: String,
    /// Destination file path.
    #[arg(long)]
    pub to: String,
    /// Overwrite if destination already exists.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct RenameOutput {
    ok: bool,
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
}

pub fn run(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let src = cwd.join(&args.from);
    let dst = cwd.join(&args.to);

    // Validate source exists.
    if !src.exists() {
        anyhow::bail!("source file not found: {}", args.from);
    }

    // Validate destination does not exist (unless --force).
    if !args.force && dst.exists() {
        anyhow::bail!("destination already exists: {}", args.to);
    }

    let content = fs::read_to_string(&src)?;

    // --check mode: report that rename would happen.
    if global.check {
        if global.json {
            let output = RenameOutput {
                ok: true,
                from: args.from.clone(),
                to: args.to.clone(),
                diff: None,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !global.quiet {
            println!("would rename {} -> {}", args.from, args.to);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: perform the rename.
    if global.apply {
        let policy = policy_from_flags(global, Some(&dst));

        // Ensure parent directories exist for the destination.
        if let Some(parent) = dst.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        if args.force {
            atomic_write(&dst, &content, &policy)?;
        } else {
            atomic_create_new(&dst, &content, &policy)?;
        }
        fs::remove_file(&src)?;

        if global.json {
            let diff_text = if global.diff {
                Some(make_diff_output(&args.from, &args.to, &content))
            } else {
                None
            };
            let output = RenameOutput {
                ok: true,
                from: args.from.clone(),
                to: args.to.clone(),
                diff: diff_text,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if global.diff {
            print!("{}", make_diff_output(&args.from, &args.to, &content));
        } else if !global.quiet {
            println!("renamed {} -> {}", args.from, args.to);
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show what would happen.
    if global.json {
        let output = RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: args.to.clone(),
            diff: Some(make_diff_output(&args.from, &args.to, &content)),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print!("{}", make_diff_output(&args.from, &args.to, &content));
    }

    Ok(exit::SUCCESS)
}

fn make_diff_output(from: &str, to: &str, content: &str) -> String {
    let del_diff = unified_diff(from, content, "");
    let add_diff = unified_diff(to, "", content);
    let diffs: Vec<_> = [del_diff, add_diff]
        .into_iter()
        .filter(|d| d.has_changes)
        .collect();
    let total = diffs.len();
    let result = DiffResult {
        diffs,
        total_files_changed: total,
    };
    format_diff_result(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

    #[test]
    fn rename_moves_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = default_global();
        global.apply = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello\n");
    }

    #[test]
    fn rename_fails_if_dst_exists() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "hello\n").unwrap();
        fs::write(&dst, "existing\n").unwrap();

        let mut global = default_global();
        global.apply = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let err = run(args, &global).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn rename_force_overwrites_dst() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "new content\n").unwrap();
        fs::write(&dst, "old content\n").unwrap();

        let mut global = default_global();
        global.apply = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: true,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new content\n");
    }

    #[test]
    fn rename_check_reports_changes() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = default_global();
        global.check = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dir.path().join("new.txt").to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        // Source should still exist in --check mode.
        assert!(src.exists());
    }

    #[test]
    fn rename_fails_if_src_missing() {
        let dir = TempDir::new().unwrap();
        let mut global = default_global();
        global.apply = true;

        let args = RenameArgs {
            from: dir.path().join("nope.txt").to_string_lossy().into_owned(),
            to: dir.path().join("dst.txt").to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let err = run(args, &global).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn rename_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("sub").join("dir").join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = default_global();
        global.apply = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello\n");
    }
}
