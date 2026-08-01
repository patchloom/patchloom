use std::path::{Path, PathBuf};

use crate::ops::replace::preferred_line_ending;

/// Pure helpers for file append/prepend content computation.
/// Used by api::file_* , plan execution (tx), and cmd/append for consistency.
/// Centralizes the "ensure line ending between existing and new" logic.
///
/// When a separator is needed, uses the file's dominant EOL (CRLF / CR / LF)
/// so Windows CRLF files without a final newline do not gain a bare LF.
pub fn append_content(existing: &str, append: &str) -> String {
    if append.is_empty() {
        return existing.to_string();
    }
    let mut combined = existing.to_string();
    if !combined.is_empty() && !ends_with_line_ending(&combined) {
        combined.push_str(preferred_line_ending(existing));
    }
    combined.push_str(append);
    combined
}

pub fn prepend_content(existing: &str, prepend: &str) -> String {
    let mut combined = prepend.to_string();
    if !combined.is_empty() && !ends_with_line_ending(&combined) && !existing.is_empty() {
        combined.push_str(preferred_line_ending(existing));
    }
    combined.push_str(existing);
    combined
}

#[inline]
fn ends_with_line_ending(s: &str) -> bool {
    s.ends_with("\r\n") || s.ends_with('\n') || s.ends_with('\r')
}

/// Directory-entry classification from one `symlink_metadata` call (#2087 / #2091).
///
/// Prefer this when a call site needs existence **and** kind (rename/delete
/// prechecks) so it does not re-stat the same path three times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathEntryKind {
    /// No directory entry (or unreadable metadata).
    Missing,
    /// Regular file (`FileType::is_file`, not a symlink).
    RegularFile,
    /// Real directory (not a symlink to a directory).
    RealDirectory,
    /// Symlink, FIFO, socket, device, or other non-regular entry.
    Special,
}

impl PathEntryKind {
    #[inline]
    pub fn exists(self) -> bool {
        !matches!(self, Self::Missing)
    }

    #[inline]
    pub fn is_regular_file(self) -> bool {
        matches!(self, Self::RegularFile)
    }

    #[inline]
    pub fn is_real_directory(self) -> bool {
        matches!(self, Self::RealDirectory)
    }
}

/// Classify a path with a single `symlink_metadata` (does not follow links).
pub fn classify_path_entry(path: &Path) -> PathEntryKind {
    match std::fs::symlink_metadata(path) {
        Err(_) => PathEntryKind::Missing,
        Ok(meta) => {
            let ft = meta.file_type();
            // Symlinks report is_dir/is_file false for the link itself on Unix;
            // real directories are never symlinks.
            if ft.is_dir() && !ft.is_symlink() {
                PathEntryKind::RealDirectory
            } else if ft.is_file() {
                PathEntryKind::RegularFile
            } else {
                PathEntryKind::Special
            }
        }
    }
}

/// True when the path exists as a directory entry (including dangling symlinks).
///
/// Prefer this over `Path::exists()` for delete/unlink: `exists()` follows
/// links and reports false for broken symlinks that `remove_file` can still
/// unlink (#2087). When you also need kind, use [`classify_path_entry`] once.
pub fn path_entry_exists(path: &Path) -> bool {
    classify_path_entry(path).exists()
}

/// True when `path` is a real directory (not a symlink to a directory).
///
/// Symlinks are unlinkable even when their target is a directory (#2087).
pub fn is_real_directory(path: &Path) -> bool {
    classify_path_entry(path).is_real_directory()
}

/// Refuse delete/rename targets that are real directories.
///
/// Allows regular files, symlinks (unlink the link only), FIFOs, sockets,
/// and device nodes. Hosts use this so `file_delete` does not dual-path
/// through `std::fs::remove_file` for special nodes (#2087).
pub fn ensure_unlinkable_not_directory(
    path: &Path,
    display: &str,
) -> Result<(), crate::exit::InvalidInputError> {
    if classify_path_entry(path).is_real_directory() {
        return Err(crate::exit::InvalidInputError {
            msg: format!("target is not a file: {display}"),
        });
    }
    Ok(())
}

/// True when the path is a regular file suitable for byte backup via `fs::copy`.
///
/// Symlinks, FIFOs, sockets, and devices return false so callers write an empty
/// backup marker instead of blocking on FIFO open or copying through a link (#2087).
pub fn is_regular_file_for_backup(path: &Path) -> bool {
    classify_path_entry(path).is_regular_file()
}

/// Rename a path, falling back to copy+delete across devices.
///
/// Shared by CLI direct-rename and tx commit (#2091 dedup). Force overwrite
/// must not delete a pre-existing dest on partial failure (backup holds bytes).
/// Uses [`path_entry_exists`] so dangling dest entries count as present.
pub fn rename_or_copy(src: &Path, dst: &Path) -> anyhow::Result<()> {
    use anyhow::Context;

    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device_rename_error(&e) => {
            let dest_existed = path_entry_exists(dst);
            std::fs::copy(src, dst).with_context(|| {
                format!("cross-device copy {} -> {}", src.display(), dst.display())
            })?;
            if let Err(remove_err) = std::fs::remove_file(src) {
                if !dest_existed {
                    let _ = std::fs::remove_file(dst);
                }
                return Err(remove_err).with_context(|| {
                    format!(
                        "removing source after cross-device copy: {} -> {}",
                        src.display(),
                        dst.display()
                    )
                });
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

fn is_cross_device_rename_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        // ERROR_NOT_SAME_DEVICE
        e.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

/// Walk ancestors of `path` and ensure every existing component is a directory.
///
/// Missing parents are fine (`create_dir_all` creates them on apply). An
/// ancestor that exists as a file (or other non-dir) makes
/// `create_dir_all` / tempfile placement fail at commit with a bare IO
/// error and can leave a spurious backup entry for a path that was never
/// written. Call this before staging `file.create` / rename destinations.
pub fn ensure_parent_components_are_directories(
    path: &Path,
) -> Result<(), crate::exit::InvalidInputError> {
    let mut current = path.parent();
    while let Some(p) = current {
        if p.as_os_str().is_empty() {
            break;
        }
        if p.exists() {
            if !p.is_dir() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("parent path is not a directory: {}", p.display()),
                });
            }
            // An existing directory implies higher ancestors are also dirs.
            break;
        }
        current = p.parent();
    }
    Ok(())
}

/// Reject on-disk binary targets for text-oriented file ops (append/prepend).
///
/// NUL bytes are valid UTF-8, so `read_to_string` succeeds and used to rewrite
/// binaries as text. Matches replace/search skip policy (`is_binary` on the
/// first 8 KiB). `display` is the path shown in the error (CLI arg or op path).
/// Returns [`crate::exit::BinaryError`] (`error_kind: binary`) so hosts can
/// distinguish content SoftSkip from argument `invalid_input` (#1963).
pub fn ensure_not_binary_file(path: &Path, display: &str) -> Result<(), crate::exit::BinaryError> {
    use std::io::Read;

    if !path.exists() {
        return Ok(());
    }
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        // Existence was checked by the caller; open errors surface on full read.
        Err(_) => return Ok(()),
    };
    let mut buf = [0u8; 8192];
    let n = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return Ok(()),
    };
    if crate::files::is_binary(&buf[..n]) {
        return Err(crate::exit::BinaryError {
            msg: format!("target is a binary file: {display}"),
        });
    }
    Ok(())
}

/// When the user names exactly one existing file that is not valid text
/// (binary NUL, invalid UTF-8) **or cannot be read** (permission/IO), return
/// a typed error (`BinaryError` / `InvalidEncodingError` / `InvalidInputError`).
///
/// Soft walks use [`crate::files::try_read_text_file`]; explicit multi-path
/// lists use [`explicit_multi_path_non_text_refused`]. Sole explicit non-text
/// / unreadable must not report pattern `no_matches` or vacuous tidy "clean"
/// (#1894; fixrealloop sole unreadable). Content SoftSkip peels to
/// `binary` / `invalid_encoding` (#1963), not coarse `invalid_input`.
///
/// Prefer [`sole_explicit_non_text_for_scan`] when `--files-from` may supply
/// the sole path (positional `paths` is empty).
pub fn sole_explicit_non_text(paths: &[String], cwd: &Path) -> Option<anyhow::Error> {
    if paths.len() != 1 {
        return None;
    }
    let display = paths[0].as_str();
    if display.is_empty() {
        return None;
    }
    let path = {
        let p = Path::new(display);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    };
    // Classify without following: dangling symlink / FIFO is present but not
    // text. Path::is_file() follows and reports false → used to fall through
    // as not_found after soft scan (#2087 dual-path vs append/create).
    match classify_path_entry(&path) {
        PathEntryKind::Missing | PathEntryKind::RealDirectory => {
            // Missing: later not_found. Directory: walk / multi-file scan.
            return None;
        }
        PathEntryKind::Special => {
            return Some(
                crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {display}"),
                }
                .into(),
            );
        }
        PathEntryKind::RegularFile => {}
    }
    // Strict sole-path: binary / invalid UTF-8 → typed kinds; unreadable IO
    // on an existing file must also hard-fail (not soft no_matches / clean).
    match crate::files::load_text_strict(&path, display) {
        Ok(_) => None,
        Err(e) => {
            if crate::exit::is_load_text_strict_fail(&e) {
                Some(e)
            } else {
                // load_text_strict already prefixes "failed to read {display}";
                // do not double-wrap (fixrealloop: "failed to read x: failed to read x").
                Some(
                    crate::exit::InvalidInputError {
                        msg: crate::exit::agent_error_message(&e),
                    }
                    .into(),
                )
            }
        }
    }
}

/// Sole non-text for a scan that may use `--files-from`.
///
/// When `files_from` is set, resolve the list and apply the sole-path rule to
/// that list (positional `paths` is often empty). File-backed lists are
/// re-read; stdin lists should be resolved once by the caller and passed via
/// [`sole_explicit_non_text`] on the resolved slice.
pub fn sole_explicit_non_text_for_scan(
    paths: &[String],
    files_from: Option<&[String]>,
    cwd: &Path,
) -> Option<anyhow::Error> {
    match files_from {
        Some(list) => sole_explicit_non_text(list, cwd),
        None => sole_explicit_non_text(paths, cwd),
    }
}

/// When a multi-path / directory walk produced zero useful results, sample
/// unreadable paths so callers do **not** report pattern `no_matches` or
/// vacuous tidy "clean" (SoftTextSkip: unreadable may have masked the scan).
///
/// Content SoftSkip (binary / invalid UTF-8) alone does not trigger this.
pub fn empty_scan_masked_by_unreadable(
    file_paths: &[PathBuf],
    cwd: &Path,
) -> Option<crate::exit::InvalidInputError> {
    const SAMPLE_LIMIT: usize = 8;
    let mut sample = Vec::new();
    let mut count = 0usize;
    for path in file_paths {
        if !path.is_file() {
            continue;
        }
        if matches!(
            crate::files::try_read_text_file(path),
            Err(crate::files::SoftTextSkip::Unreadable)
        ) {
            count += 1;
            if sample.len() < SAMPLE_LIMIT {
                #[cfg(any(feature = "cli", feature = "files"))]
                let display = crate::files::relative_display(path, cwd)
                    .to_string_lossy()
                    .into_owned();
                #[cfg(not(any(feature = "cli", feature = "files")))]
                let display = path
                    .strip_prefix(cwd)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                sample.push(display);
            }
        }
    }
    if count == 0 {
        return None;
    }
    let sample_s = sample.join(", ");
    Some(crate::exit::InvalidInputError {
        msg: format!(
            "could not read {count} path(s) while scanning (e.g. {sample_s}); \
             not reporting as no matches / clean"
        ),
    })
}

/// Path soft-refused for non-pattern reasons (parity with replace/search `refused[]`).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct PathRefused {
    pub path: String,
    /// `binary`, `invalid_utf8`, or `unreadable`.
    pub reason: &'static str,
}

/// Explicit multi-file lists only: non-text / unreadable co-paths that walks
/// soft-skip.
///
/// Sole non-text is hard `invalid_input` via [`sole_explicit_non_text`].
/// Directory walks (any path is a directory) omit mass `refused[]` (noise).
/// Reasons: `binary`, `invalid_utf8`, `unreadable`.
pub fn explicit_multi_path_non_text_refused(
    paths: &[String],
    cwd: &Path,
) -> Option<Vec<PathRefused>> {
    if paths.len() < 2 {
        return None;
    }
    if paths.iter().any(|p| {
        let path = {
            let raw = Path::new(p);
            if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                cwd.join(raw)
            }
        };
        path.is_dir()
    }) {
        return None;
    }
    let mut refused = Vec::new();
    for p in paths {
        let path = {
            let raw = Path::new(p);
            if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                cwd.join(raw)
            }
        };
        if !path.is_file() {
            continue;
        }
        match crate::files::try_read_text_file(&path) {
            Ok(_) => {}
            Err(skip) => refused.push(PathRefused {
                path: p.clone(),
                reason: skip.as_reason(),
            }),
        }
    }
    if refused.is_empty() {
        None
    } else {
        refused.sort_by(|a, b| a.path.cmp(&b.path));
        Some(refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn append_adds_newline_separator() {
        assert_eq!(append_content("existing", "new"), "existing\nnew");
    }

    #[test]
    fn append_no_double_newline() {
        assert_eq!(append_content("existing\n", "new"), "existing\nnew");
    }

    #[test]
    fn append_empty_existing() {
        assert_eq!(append_content("", "new"), "new");
    }

    #[test]
    fn prepend_adds_newline_separator() {
        assert_eq!(prepend_content("existing", "new"), "new\nexisting");
    }

    #[test]
    fn prepend_no_double_newline() {
        assert_eq!(prepend_content("existing", "new\n"), "new\nexisting");
    }

    #[test]
    fn prepend_empty_existing() {
        assert_eq!(prepend_content("", "new"), "new");
    }

    #[test]
    fn prepend_empty_prepend() {
        assert_eq!(prepend_content("existing", ""), "existing");
    }

    #[test]
    fn append_empty_append() {
        assert_eq!(append_content("existing", ""), "existing");
    }

    #[test]
    fn append_empty_both() {
        assert_eq!(append_content("", ""), "");
    }

    #[test]
    fn prepend_empty_both() {
        assert_eq!(prepend_content("", ""), "");
    }

    #[test]
    fn prepend_symmetry_with_append() {
        // Both ensure a newline separator when content lacks trailing newline
        let a = append_content("base", "added");
        assert!(a.contains('\n'));
        let p = prepend_content("base", "added");
        assert!(p.contains('\n'));
    }

    #[test]
    fn real_directory_detected_not_file() {
        let dir = TempDir::new().unwrap();
        assert!(is_real_directory(dir.path()));
        assert_eq!(
            classify_path_entry(dir.path()),
            PathEntryKind::RealDirectory
        );
        let f = dir.path().join("f.txt");
        fs::write(&f, "x").unwrap();
        assert!(!is_real_directory(&f));
        assert!(is_regular_file_for_backup(&f));
        assert_eq!(classify_path_entry(&f), PathEntryKind::RegularFile);
        assert_eq!(
            classify_path_entry(&dir.path().join("missing")),
            PathEntryKind::Missing
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_unlinkable_not_real_directory() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("t");
        let link = dir.path().join("l");
        fs::write(&target, "x").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(!is_real_directory(&link));
        assert!(!is_regular_file_for_backup(&link));
        assert_eq!(classify_path_entry(&link), PathEntryKind::Special);
        ensure_unlinkable_not_directory(&link, "l").unwrap();
    }

    #[test]
    fn rename_or_copy_moves_regular_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("a.txt");
        let dst = dir.path().join("b.txt");
        fs::write(&src, "payload\n").unwrap();
        rename_or_copy(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "payload\n");
    }

    #[test]
    fn append_crlf_file_without_final_newline_uses_crlf_separator() {
        // Windows file missing final newline must not gain a bare LF.
        let out = append_content("line1\r\nline2", "line3");
        assert_eq!(out, "line1\r\nline2\r\nline3");
    }

    #[test]
    fn append_crlf_file_with_final_newline_no_extra_separator() {
        let out = append_content("line1\r\nline2\r\n", "line3");
        assert_eq!(out, "line1\r\nline2\r\nline3");
    }

    #[test]
    fn prepend_crlf_payload_uses_file_eol_when_payload_lacks_eol() {
        let out = prepend_content("body\r\n", "head");
        assert_eq!(out, "head\r\nbody\r\n");
    }

    #[test]
    fn ensure_parents_ok_when_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a").join("b").join("c.txt");
        ensure_parent_components_are_directories(&path).unwrap();
    }

    #[test]
    fn ensure_parents_ok_when_dirs_exist() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("c.txt");
        ensure_parent_components_are_directories(&path).unwrap();
    }

    #[test]
    fn ensure_parents_rejects_file_as_parent() {
        let dir = TempDir::new().unwrap();
        let blocking = dir.path().join("notdir");
        fs::write(&blocking, "file\n").unwrap();
        let path = blocking.join("child.txt");
        let err = ensure_parent_components_are_directories(&path).unwrap_err();
        assert!(
            err.msg.contains("not a directory"),
            "unexpected message: {}",
            err.msg
        );
        assert!(err.msg.contains("notdir"), "message should name the path");
    }

    #[test]
    fn ensure_parents_rejects_file_as_intermediate() {
        let dir = TempDir::new().unwrap();
        let blocking = dir.path().join("a");
        fs::write(&blocking, "file\n").unwrap();
        let path = blocking.join("b").join("c.txt");
        let err = ensure_parent_components_are_directories(&path).unwrap_err();
        assert!(err.msg.contains("not a directory"), "got: {}", err.msg);
    }

    #[test]
    fn ensure_not_binary_ok_for_text() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.txt");
        fs::write(&path, "hello\n").unwrap();
        ensure_not_binary_file(&path, "t.txt").unwrap();
    }

    #[test]
    fn ensure_not_binary_rejects_nul() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("b.bin");
        fs::write(&path, b"hello\x00world").unwrap();
        let err = ensure_not_binary_file(&path, "b.bin").unwrap_err();
        assert!(err.msg.contains("binary file"), "got: {}", err.msg);
        assert!(err.msg.contains("b.bin"));
        let ae: anyhow::Error = err.into();
        assert_eq!(
            crate::fallback::edit_error_kind(&ae),
            Some(crate::fallback::EditErrorKind::Binary)
        );
    }

    #[test]
    fn ensure_not_binary_missing_path_ok() {
        let dir = TempDir::new().unwrap();
        ensure_not_binary_file(&dir.path().join("nope"), "nope").unwrap();
    }

    #[test]
    fn sole_explicit_non_text_detects_sole_binary() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("b.bin");
        fs::write(&path, b"x\x00y").unwrap();
        let err = sole_explicit_non_text(&["b.bin".into()], dir.path()).unwrap();
        assert!(err.to_string().contains("binary"));
        assert_eq!(
            crate::fallback::error_kind_str(&err),
            Some("binary"),
            "sole binary must peel to error_kind binary (#1963)"
        );
    }

    #[test]
    fn sole_explicit_non_text_none_for_text_or_multi() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("t.txt"), "hi\n").unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        assert!(sole_explicit_non_text(&["t.txt".into()], dir.path()).is_none());
        assert!(sole_explicit_non_text(&["t.txt".into(), "b.bin".into()], dir.path()).is_none());
        assert!(sole_explicit_non_text(&[".".into()], dir.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn sole_explicit_non_text_rejects_dangling_symlink() {
        let dir = TempDir::new().unwrap();
        let link = dir.path().join("dangling.txt");
        std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
        let err = sole_explicit_non_text(&["dangling.txt".into()], dir.path()).unwrap();
        assert!(
            crate::exit::is_invalid_input(&err),
            "dangling sole path must be invalid_input not not_found: {err}"
        );
        assert!(err.to_string().contains("not a file"), "got: {err}");
        assert_eq!(crate::fallback::error_kind_str(&err), Some("invalid_input"));
    }

    #[test]
    fn sole_explicit_non_text_rejects_invalid_utf8() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.txt");
        fs::write(&path, b"hello \xff world\n").unwrap();
        let err = sole_explicit_non_text(&["bad.txt".into()], dir.path()).unwrap();
        let msg = err.to_string();
        assert!(msg.contains("UTF-8") || msg.contains("utf"), "got: {msg}");
        assert!(msg.contains("bad.txt"), "got: {msg}");
        assert_eq!(
            crate::fallback::error_kind_str(&err),
            Some("invalid_encoding"),
            "sole invalid UTF-8 must peel to invalid_encoding (#1963)"
        );
    }

    #[test]
    fn sole_explicit_non_text_for_scan_uses_files_from_list() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        let list = vec!["b.bin".into()];
        // Positionals empty (typical --files-from only).
        assert!(sole_explicit_non_text(&[], dir.path()).is_none());
        let err = sole_explicit_non_text_for_scan(&[], Some(&list), dir.path()).unwrap();
        assert!(err.to_string().contains("binary"), "got: {err}");
        assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
    }

    #[test]
    fn empty_scan_masked_by_unreadable_samples() {
        let dir = TempDir::new().unwrap();
        let locked = dir.path().join("locked.txt");
        fs::write(&locked, "x\n").unwrap();
        fs::write(dir.path().join("ok.txt"), "y\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            if fs::read_to_string(&locked).is_ok() {
                fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
                return;
            }
            let paths = vec![locked.clone(), dir.path().join("ok.txt")];
            // ok.txt is readable → still report unreadable co-path when asked
            // only about the locked file set:
            let only_locked = vec![locked.clone()];
            let err = empty_scan_masked_by_unreadable(&only_locked, dir.path()).unwrap();
            assert!(err.msg.contains("could not read"), "got: {}", err.msg);
            assert!(err.msg.contains("locked.txt"), "got: {}", err.msg);
            // Mixed: still reports when unreadable present among scanned paths
            let err2 = empty_scan_masked_by_unreadable(&paths, dir.path()).unwrap();
            assert!(err2.msg.contains("could not read"), "got: {}", err2.msg);
            // Readable-only: no mask error
            assert!(
                empty_scan_masked_by_unreadable(&[dir.path().join("ok.txt")], dir.path()).is_none()
            );
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn sole_explicit_non_text_rejects_unreadable() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("locked.txt");
        fs::write(&path, "hello\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            // Root (common in Docker) can still read mode-000 files.
            if fs::read_to_string(&path).is_ok() {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                return;
            }
            let err = sole_explicit_non_text(&["locked.txt".into()], dir.path()).unwrap();
            let msg = err.to_string();
            assert!(msg.contains("failed to read"), "got: {msg}");
            // OS detail must appear in Display/msg (not only via {:#} on a chain).
            assert!(
                msg.contains("Permission denied")
                    || msg.contains("PermissionDenied")
                    || msg.contains("os error"),
                "OS detail missing: {msg}"
            );
            // Single prefix only (not "failed to read x: failed to read x").
            let failed_count = msg.matches("failed to read").count();
            assert_eq!(
                failed_count, 1,
                "unreadable error must not double-wrap load_text_strict context: {msg}"
            );
            assert!(
                msg.contains("locked.txt"),
                "path should appear once in message: {msg}"
            );
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }
    }

    #[test]
    fn multi_path_non_text_refused_lists_binary_and_utf8() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("t.txt"), "hi\n").unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        fs::write(dir.path().join("bad.txt"), b"hi \xff\n").unwrap();
        let refused = explicit_multi_path_non_text_refused(
            &["t.txt".into(), "b.bin".into(), "bad.txt".into()],
            dir.path(),
        )
        .expect("non-text co-paths");
        assert_eq!(refused.len(), 2);
        let reasons: Vec<_> = refused.iter().map(|r| r.reason).collect();
        assert!(reasons.contains(&"binary"), "{refused:?}");
        assert!(reasons.contains(&"invalid_utf8"), "{refused:?}");
    }

    #[test]
    fn multi_path_non_text_refused_none_for_dir_walk_or_sole() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        assert!(explicit_multi_path_non_text_refused(&["b.bin".into()], dir.path()).is_none());
        assert!(explicit_multi_path_non_text_refused(&[".".into()], dir.path()).is_none());
    }
}
