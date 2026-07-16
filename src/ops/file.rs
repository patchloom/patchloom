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
}
