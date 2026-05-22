use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result, unified_diff};
use crate::exit;
use crate::write::{atomic_create_new, atomic_write, policy_from_flags};
use anyhow::Context;
use clap::Args;
use serde::Serialize;
use std::fs;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom rename old_config.json config.json --apply
  patchloom rename src/utils.rs src/helpers.rs --apply --force")]
pub struct RenameArgs {
    /// Source file path.
    pub from: String,
    /// Destination file path.
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

    // Validate source exists and is a file.
    if !src.exists() {
        anyhow::bail!("source file not found: {}", args.from);
    }
    if !src.is_file() {
        anyhow::bail!("source is not a file: {}", args.from);
    }

    if dst.exists() && !dst.is_file() {
        anyhow::bail!("destination is not a file: {}", args.to);
    }

    // Validate destination does not exist (unless --force).
    if !args.force && dst.exists() {
        anyhow::bail!("destination already exists: {}", args.to);
    }

    // If source and destination resolve to the same path, it's a no-op.
    if src == dst {
        let output = RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: args.to.clone(),
            diff: None,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!("source and destination are the same: {}", args.from);
        }
        return Ok(exit::SUCCESS);
    }

    // --check mode: report that rename would happen (no file read needed).
    if global.check {
        let output = RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: args.to.clone(),
            diff: None,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!("would rename {} -> {}", args.from, args.to);
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    let policy = policy_from_flags(global, Some(&dst));

    // --apply mode: perform the rename.
    if global.apply {
        let mut backup = crate::backup::BackupSession::new(&cwd)?;
        backup.save_before_delete(&src)?;
        do_rename(&src, &dst, &args, &policy)?;
        backup.finalize()?;

        if global.json || global.jsonl || global.diff {
            // After --apply, source is gone; read from destination.
            let diff_text = try_diff(&dst, &args.from, &args.to);
            let output = RenameOutput {
                ok: true,
                from: args.from.clone(),
                to: args.to.clone(),
                diff: if global.diff { diff_text.clone() } else { None },
            };
            if !global.emit_json(&output)? {
                if let Some(d) = diff_text {
                    print!("{d}");
                } else if !global.quiet {
                    println!("renamed {} -> {} (binary)", args.from, args.to);
                }
            }
        } else if !global.quiet {
            println!("renamed {} -> {}", args.from, args.to);
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show what would happen (source still exists).
    let diff_text = try_diff(&src, &args.from, &args.to);
    let output = RenameOutput {
        ok: true,
        from: args.from.clone(),
        to: args.to.clone(),
        diff: diff_text.clone(),
    };
    if !global.emit_json(&output)? {
        if let Some(d) = diff_text {
            print!("{d}");
        } else if !global.quiet {
            println!("would rename {} -> {} (binary)", args.from, args.to);
        }
    }

    // --confirm: prompt after showing preview, then rename if confirmed.
    if global.should_apply() {
        let mut backup = crate::backup::BackupSession::new(&cwd)?;
        backup.save_before_delete(&src)?;
        do_rename(&src, &dst, &args, &policy)?;
        backup.finalize()?;
        if global.show_status() {
            eprintln!("renamed {} -> {}", args.from, args.to);
        }
    }

    Ok(exit::SUCCESS)
}

/// Perform a rename with backup, without writing to stdout.
/// Used by the MCP server for direct in-process renames.
#[cfg(feature = "mcp")]
pub(crate) fn apply_rename(
    from: &std::path::Path,
    to: &std::path::Path,
    force: bool,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    if !from.exists() {
        anyhow::bail!("source file not found: {}", from.display());
    }
    if !from.is_file() {
        anyhow::bail!("source is not a file: {}", from.display());
    }
    if to.exists() && !to.is_file() {
        anyhow::bail!("destination is not a file: {}", to.display());
    }
    if !force && to.exists() {
        anyhow::bail!("destination already exists: {}", to.display());
    }
    if from == to {
        return Ok(());
    }

    let mut backup = crate::backup::BackupSession::new(cwd)?;
    backup.save_before_delete(from)?;

    // Create parent directories if needed.
    if let Some(parent) = to.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    // Binary-safe rename via fs::rename (with cross-device fallback).
    rename_or_copy(from, to)?;
    backup.finalize()?;
    Ok(())
}

/// Perform the actual file rename: create parent dirs, move or transform the
/// content, and remove the source.
fn do_rename(
    src: &std::path::Path,
    dst: &std::path::Path,
    args: &RenameArgs,
    policy: &crate::write::WritePolicy,
) -> anyhow::Result<()> {
    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    if policy.is_noop() {
        if args.force || !dst.exists() {
            rename_or_copy(src, dst)?;
        } else {
            anyhow::bail!("destination already exists: {}", args.to);
        }
    } else {
        let content = fs::read_to_string(src).with_context(|| format!("reading {}", args.from))?;
        if args.force {
            atomic_write(dst, &content, policy)?;
        } else {
            atomic_create_new(dst, &content, policy)?;
        }
        fs::remove_file(src)?;
    }
    Ok(())
}

/// Try to read `path` as text and produce a rename diff. Returns `None` for
/// binary files (non-UTF-8 content) or if the file cannot be read.
fn try_diff(path: &std::path::Path, from_label: &str, to_label: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    Some(make_diff_output(from_label, to_label, &content))
}

/// Rename a file, falling back to copy+delete when the source and destination
/// are on different filesystems (where `fs::rename` would fail).
fn rename_or_copy(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            fs::copy(src, dst)?;
            fs::remove_file(src)?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Check if an I/O error is a cross-device link error (EXDEV on Unix,
/// ERROR_NOT_SAME_DEVICE on Windows).
fn is_cross_device(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE = 17
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
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

    fn global_for(dir: &TempDir) -> GlobalFlags {
        GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        }
    }

    #[test]
    fn rename_moves_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = global_for(&dir);
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

        let mut global = global_for(&dir);
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
    fn rename_force_rejects_directory_destination() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("folder");
        fs::write(&src, "hello\n").unwrap();
        fs::create_dir(&dst).unwrap();

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: true,
            write: Default::default(),
        };

        let err = run(args, &global_for(&dir)).unwrap_err();
        assert!(err.to_string().contains("destination is not a file"));
    }

    #[test]
    fn rename_force_overwrites_dst() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "new content\n").unwrap();
        fs::write(&dst, "old content\n").unwrap();

        let mut global = global_for(&dir);
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

        let mut global = global_for(&dir);
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
        let mut global = global_for(&dir);
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
    fn rename_same_path_is_noop() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("same.txt");
        fs::write(&file, "hello\n").unwrap();

        let mut global = global_for(&dir);
        global.apply = true;

        let args = RenameArgs {
            from: file.to_string_lossy().into_owned(),
            to: file.to_string_lossy().into_owned(),
            force: true,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(file.exists(), "file should still exist");
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
    }

    #[test]
    fn rename_binary_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("image.bin");
        let dst = dir.path().join("moved.bin");
        // Write non-UTF-8 content.
        fs::write(&src, b"\x00\x01\x02\xff\xfe").unwrap();

        let mut global = global_for(&dir);
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
        assert_eq!(fs::read(&dst).unwrap(), b"\x00\x01\x02\xff\xfe");
    }

    #[test]
    fn rename_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("sub").join("dir").join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = global_for(&dir);
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
