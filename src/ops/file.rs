use std::path::Path;

/// Pure helpers for file append/prepend content computation.
/// Used by api::file_* , plan execution (tx), and cmd/append for consistency.
/// Centralizes the "ensure nl between existing and new" logic.
pub fn append_content(existing: &str, append: &str) -> String {
    if append.is_empty() {
        return existing.to_string();
    }
    let mut combined = existing.to_string();
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(append);
    combined
}

pub fn prepend_content(existing: &str, prepend: &str) -> String {
    let mut combined = prepend.to_string();
    if !combined.is_empty() && !combined.ends_with('\n') && !existing.is_empty() {
        combined.push('\n');
    }
    combined.push_str(existing);
    combined
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
pub fn ensure_not_binary_file(
    path: &Path,
    display: &str,
) -> Result<(), crate::exit::InvalidInputError> {
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
        return Err(crate::exit::InvalidInputError {
            msg: format!("target is a binary file: {display}"),
        });
    }
    Ok(())
}

/// When the user names exactly one existing file and it is binary, return that
/// display path. Directory walks and multi-path lists still soft-skip binaries.
pub fn single_explicit_binary_target(
    paths: &[String],
    cwd: &Path,
) -> Option<crate::exit::InvalidInputError> {
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
    if !path.is_file() {
        return None;
    }
    ensure_not_binary_file(&path, display).err()
}

/// Path soft-refused for non-pattern reasons (parity with replace/search `refused[]`).
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct PathRefused {
    pub path: String,
    pub reason: &'static str,
}

/// Explicit multi-file lists only: binary co-paths that text ops soft-skip.
///
/// Sole binary is hard `invalid_input` via [`single_explicit_binary_target`].
/// Directory walks (any path is a directory) omit mass `refused[]` (noise).
/// Used by tidy multi-path check/fix; search/replace keep local variants with
/// extra reason fields where needed.
pub fn explicit_multi_path_binary_refused(
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
        // Path preflight via ensure_not_binary_file (typed InvalidInput on NUL).
        if path.is_file() && ensure_not_binary_file(&path, p).is_err() {
            // Keep the user-facing path string (CLI arg), not only the abs path.
            refused.push(PathRefused {
                path: p.clone(),
                reason: "binary",
            });
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
    }

    #[test]
    fn ensure_not_binary_missing_path_ok() {
        let dir = TempDir::new().unwrap();
        ensure_not_binary_file(&dir.path().join("nope"), "nope").unwrap();
    }

    #[test]
    fn single_explicit_binary_detects_sole_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("b.bin");
        fs::write(&path, b"x\x00y").unwrap();
        let err = single_explicit_binary_target(&["b.bin".into()], dir.path()).unwrap();
        assert!(err.msg.contains("binary"));
    }

    #[test]
    fn single_explicit_binary_none_for_text_or_multi() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("t.txt"), "hi\n").unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        assert!(single_explicit_binary_target(&["t.txt".into()], dir.path()).is_none());
        assert!(
            single_explicit_binary_target(&["t.txt".into(), "b.bin".into()], dir.path()).is_none()
        );
        assert!(single_explicit_binary_target(&[".".into()], dir.path()).is_none());
    }

    #[test]
    fn multi_path_binary_refused_lists_binary_co_path() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("t.txt"), "hi\n").unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        let refused =
            explicit_multi_path_binary_refused(&["t.txt".into(), "b.bin".into()], dir.path())
                .expect("binary co-path");
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].reason, "binary");
        assert!(refused[0].path.contains("b.bin"), "got {}", refused[0].path);
    }

    #[test]
    fn multi_path_binary_refused_none_for_dir_walk_or_sole() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.bin"), b"x\x00y").unwrap();
        assert!(explicit_multi_path_binary_refused(&["b.bin".into()], dir.path()).is_none());
        assert!(explicit_multi_path_binary_refused(&[".".into()], dir.path()).is_none());
    }
}
