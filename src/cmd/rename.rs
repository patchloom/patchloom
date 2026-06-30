use crate::cli::global::GlobalFlags;
use crate::cmd::output::WritePhase;
use crate::cmd::output::execute_via_engine;
use crate::cmd::write_dispatch::{WriteMessages, execute_write};
use crate::exit;
use crate::plan::Operation;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
}

pub fn run(args: RenameArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!(
        "rename: from={}, to={}, force={}",
        args.from,
        args.to,
        args.force
    );
    let cwd = global.resolve_cwd()?;
    let src = cwd.join(&args.from);
    let dst = cwd.join(&args.to);

    // Pre-validation for CLI-specific checks.
    if !src.exists() {
        anyhow::bail!("source file not found: {}", args.from);
    }
    if !src.is_file() {
        anyhow::bail!("source is not a file: {}", args.from);
    }
    if dst.exists() && !dst.is_file() {
        anyhow::bail!("destination is not a file: {}", args.to);
    }

    // Same-file no-op check. On case-insensitive filesystems (macOS APFS),
    // canonicalize() treats case differences as identical. Allow the rename
    // when only the case differs so users can change filename casing (#1167).
    let is_case_only_change = src != dst
        && src.parent() == dst.parent()
        && src.file_name().map(|n| n.to_ascii_lowercase())
            == dst.file_name().map(|n| n.to_ascii_lowercase());
    if !is_case_only_change
        && (src == dst
            || matches!(
                (src.canonicalize(), dst.canonicalize()),
                (Ok(ref s), Ok(ref d)) if s == d
            ))
    {
        let output = RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: Some(args.to.clone()),

            diff: None,
            applied: None,
        };
        if !global.emit_json(&output)? && !global.quiet {
            println!("source and destination are the same: {}", args.from);
        }
        return Ok(exit::SUCCESS);
    }

    if !args.force && !is_case_only_change && dst.exists() {
        anyhow::bail!("destination already exists: {}", args.to);
    }

    // Binary files can't go through the tx engine (it reads as UTF-8 text).
    // Use a direct fs::rename (or copy+delete) for binary renames.
    // Only read the first 8 KiB instead of the entire file to avoid
    // unnecessary memory allocation on large binaries.
    let is_binary = {
        use std::io::Read;
        let mut file = fs::File::open(&src)?;
        let mut buf = [0u8; 8192];
        let n = file.read(&mut buf)?;
        crate::files::is_binary(&buf[..n])
    };
    if is_binary {
        return run_binary_rename(&args, global, &cwd, &src, &dst);
    }

    // Case-only renames on case-insensitive filesystems (e.g. macOS APFS)
    // cannot go through the tx engine because its write-then-delete approach
    // would delete the destination (same inode as source). Use fs::rename
    // directly, which handles case changes atomically (#1167).
    if is_case_only_change {
        return run_binary_rename(&args, global, &cwd, &src, &dst);
    }

    let op = Operation::FileRename {
        from: args.from.clone(),
        to: args.to.clone(),
        force: args.force,
    };

    let check_msg = format!("would rename {} -> {}", args.from, args.to);
    let apply_msg = format!("renamed {} -> {}", args.from, args.to);

    execute_via_engine(
        op,
        global,
        |phase, diff| RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: Some(args.to.clone()),

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

/// Handle binary file renames directly (bypasses the tx engine which only
/// handles UTF-8 text files).
fn run_binary_rename(
    args: &RenameArgs,
    global: &GlobalFlags,
    cwd: &std::path::Path,
    src: &std::path::Path,
    dst: &std::path::Path,
) -> anyhow::Result<u8> {
    // Write policies require reading the file as UTF-8 text, which binary
    // files don't support. Fail early with a clear error.
    if global.trim_trailing_whitespace
        || global.ensure_final_newline
        || global.normalize_eol.is_some()
    {
        anyhow::bail!(
            "cannot apply write policy to binary file: {}",
            src.display()
        );
    }

    let check_msg = format!("would rename {} -> {} (binary)", args.from, args.to);
    let apply_msg = format!("renamed {} -> {} (binary)", args.from, args.to);
    let post_msg = format!("renamed {} -> {}", args.from, args.to);

    execute_write(
        global,
        cwd,
        |phase, _diff| RenameOutput {
            ok: true,
            from: args.from.clone(),
            to: Some(args.to.clone()),

            diff: None,
            applied: match phase {
                WritePhase::Confirmed(a) => Some(a),
                _ => None,
            },
        },
        None::<&dyn Fn(bool) -> String>,
        || {
            let mut backup = crate::backup::BackupSession::new(cwd)?;
            backup.save_before_delete(src)?;
            backup.save_before_write(dst)?;
            if let Some(parent) = dst.parent()
                && !parent.as_os_str().is_empty()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }
            backup.finalize()?;
            rename_or_copy(src, dst)?;
            Ok(())
        },
        WriteMessages {
            check: &check_msg,
            apply: &apply_msg,
            post_confirm: Some(&post_msg),
        },
    )
}

/// Rename a file, falling back to copy+delete when the source and destination
/// are on different filesystems (where `fs::rename` would fail).
fn rename_or_copy(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            fs::copy(src, dst).with_context(|| {
                format!("cross-device copy {} -> {}", src.display(), dst.display())
            })?;
            fs::remove_file(src).with_context(|| {
                format!("removing source after cross-device copy: {}", src.display())
            })?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Check if an I/O error is a cross-device link error.
fn is_cross_device(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn rename_moves_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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

        let err = run(args, &GlobalFlags::test_with_cwd(dir.path())).unwrap_err();
        assert!(err.to_string().contains("destination is not a file"));
    }

    #[test]
    fn rename_force_overwrites_dst() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        fs::write(&src, "new content\n").unwrap();
        fs::write(&dst, "old content\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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
        let dst = dir.path().join("new.txt");
        fs::write(&src, "hello\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.check = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);
        // Source should still exist in --check mode.
        assert!(src.exists());
        assert_eq!(
            fs::read_to_string(&src).unwrap(),
            "hello\n",
            "--check must not modify source content"
        );
        // Destination must NOT be created in --check mode.
        assert!(!dst.exists(), "--check must not create destination file");
    }

    #[test]
    fn rename_fails_if_src_missing() {
        let dir = TempDir::new().unwrap();
        let mut global = GlobalFlags::test_with_cwd(dir.path());
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

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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
    fn rename_same_file_via_different_path_is_noop() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("same.txt");
        fs::write(&file, "hello\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        // Use "./same.txt" as destination - different string, same file.
        let args = RenameArgs {
            from: file.to_string_lossy().into_owned(),
            to: dir.path().join("./same.txt").to_string_lossy().into_owned(),
            force: true,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(file.exists(), "file must not be deleted");
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
    }

    #[test]
    fn rename_binary_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("image.bin");
        let dst = dir.path().join("moved.bin");
        // Write non-UTF-8 content.
        fs::write(&src, b"\x00\x01\x02\xff\xfe").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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

        let mut global = GlobalFlags::test_with_cwd(dir.path());
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

    #[test]
    fn rename_fails_if_src_is_directory() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("folder");
        let dst = dir.path().join("new.txt");
        fs::create_dir(&src).unwrap();

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let err = run(args, &GlobalFlags::test_with_cwd(dir.path())).unwrap_err();
        assert!(err.to_string().contains("source is not a file"));
    }

    #[test]
    fn rename_creates_backup_before_mutation() {
        // R4 fix: backup.finalize() must be called before rename_or_copy()
        // so the backup is committed even if the rename itself fails.
        // We verify this by checking that a backup session is created
        // after a successful rename.
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("original.txt");
        let dst = dir.path().join("moved.txt");
        fs::write(&src, "backup me\n").unwrap();

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;

        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(!src.exists(), "source should be gone after rename");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "backup me\n");

        // Verify backup session was created (finalize() was called).
        let backup_dir = dir.path().join(".patchloom/backups");
        assert!(
            backup_dir.exists(),
            "backup directory should exist after rename --apply"
        );
        let sessions: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert!(
            !sessions.is_empty(),
            "at least one backup session should be created"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rename_unreadable_file_propagates_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("unreadable.txt");
        let dst = dir.path().join("dest.txt");
        fs::write(&src, "hello\n").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o000)).unwrap();

        // Root (common in Docker) can still read mode-000 files. Skip when
        // permissions do not actually block reading (#1276).
        if fs::read_to_string(&src).is_ok() {
            fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let mut global = GlobalFlags::test_with_cwd(dir.path());
        global.apply = true;
        let args = RenameArgs {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
            force: false,
            write: Default::default(),
        };
        // Should propagate the I/O error, not misclassify as binary
        let result = run(args, &global);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            !err_msg.contains("binary"),
            "should not misclassify permission error as binary: {err_msg}"
        );
        // Cleanup: restore permissions so TempDir can clean up
        fs::set_permissions(&src, fs::Permissions::from_mode(0o644)).unwrap();
    }
}
