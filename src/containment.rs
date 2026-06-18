//! Workspace path containment.
//!
//! Ensures that file operations stay within a designated workspace directory,
//! preventing path traversal attacks via `../` or symlinks that point outside
//! the workspace root.
//!
//! Two-layer defense:
//! 1. **Syntactic check** (no I/O): rejects `../` traversal that goes beyond root depth
//! 2. **Symlink-aware check**: canonicalizes and verifies the resolved path is contained
//!
//! # Example
//!
//! ```rust,no_run
//! use patchloom::containment::{PathGuard, AbsolutePathPolicy};
//! use std::path::PathBuf;
//!
//! let guard = PathGuard::new(
//!     PathBuf::from("/home/user/project"),
//!     AbsolutePathPolicy::Reject,
//! ).unwrap();
//!
//! // OK: relative path within workspace
//! let resolved = guard.check_path("src/main.rs").unwrap();
//!
//! // Error: escapes workspace
//! assert!(guard.check_path("../../etc/passwd").is_err());
//! ```

use std::path::{Component, Path, PathBuf};

/// Policy for handling absolute paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsolutePathPolicy {
    /// Reject all absolute paths (current patchloom MCP behavior).
    Reject,
    /// Allow absolute paths if they resolve within the workspace root.
    AllowIfContained,
}

/// Errors from workspace path validation.
#[derive(Debug)]
pub enum ContainmentError {
    /// The path is absolute and the policy rejects absolute paths.
    AbsolutePath(String),

    /// The path escapes the workspace directory (via `../` or symlinks).
    Escaped {
        /// The offending path.
        path: String,
        /// The workspace root.
        root: String,
    },

    /// Failed to canonicalize a path (I/O error).
    Canonicalize {
        /// The path that failed to canonicalize.
        path: String,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for ContainmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainmentError::AbsolutePath(p) => write!(f, "absolute paths are not allowed: {p}"),
            ContainmentError::Escaped { path, root } => {
                write!(
                    f,
                    "path escapes workspace directory: {path} (workspace: {root})"
                )
            }
            ContainmentError::Canonicalize { path, source } => {
                write!(f, "failed to canonicalize path: {path}: {source}")
            }
        }
    }
}

impl std::error::Error for ContainmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ContainmentError::Canonicalize { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Workspace path guard with cached canonical root.
///
/// Validates that paths stay within the workspace directory after
/// symlink resolution. Two-layer defense:
/// 1. Syntactic check (no I/O): rejects `../` traversal beyond root
/// 2. Symlink-aware check: canonicalizes and verifies containment
#[derive(Debug, Clone)]
pub struct PathGuard {
    root: PathBuf,
    canon_root: PathBuf,
    absolute_policy: AbsolutePathPolicy,
}

impl PathGuard {
    /// Create a new path guard rooted at `root`.
    ///
    /// Canonicalizes `root` once at construction time.
    /// Returns an error if `root` cannot be canonicalized.
    pub fn new(
        root: PathBuf,
        absolute_policy: AbsolutePathPolicy,
    ) -> Result<Self, ContainmentError> {
        let canon_root = root
            .canonicalize()
            .map_err(|e| ContainmentError::Canonicalize {
                path: root.display().to_string(),
                source: e,
            })?;
        Ok(Self {
            root,
            canon_root,
            absolute_policy,
        })
    }

    /// Validate that `path` stays within the workspace root.
    ///
    /// Returns the canonicalized path on success. Rejects paths that
    /// escape the workspace via `../` traversal or symlinks.
    pub fn check_path(&self, path: &str) -> Result<PathBuf, ContainmentError> {
        let p = Path::new(path);

        if p.is_absolute() {
            match self.absolute_policy {
                AbsolutePathPolicy::Reject => {
                    return Err(ContainmentError::AbsolutePath(path.to_string()));
                }
                AbsolutePathPolicy::AllowIfContained => {
                    // Skip syntactic check, go straight to symlink-aware check.
                    return self.check_resolved_absolute(path, p);
                }
            }
        }

        // Syntactic depth-tracking check (no I/O).
        validate_relative_depth(path, p)?;

        // Symlink-aware containment check.
        self.check_resolved_relative(path)
    }

    /// The original (non-canonicalized) workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The canonicalized workspace root.
    pub fn canon_root(&self) -> &Path {
        &self.canon_root
    }

    /// Check that an absolute path resolves within the workspace.
    fn check_resolved_absolute(&self, path: &str, p: &Path) -> Result<PathBuf, ContainmentError> {
        let canon = canonicalize_or_ancestor(p).map_err(|e| ContainmentError::Canonicalize {
            path: path.to_string(),
            source: e,
        })?;
        if !canon.starts_with(&self.canon_root) {
            return Err(ContainmentError::Escaped {
                path: path.to_string(),
                root: self.root.display().to_string(),
            });
        }
        Ok(canon)
    }

    /// Check that a relative path, joined with root, resolves within the workspace.
    fn check_resolved_relative(&self, path: &str) -> Result<PathBuf, ContainmentError> {
        let joined = self.root.join(path);
        let canon =
            canonicalize_or_ancestor(&joined).map_err(|e| ContainmentError::Canonicalize {
                path: path.to_string(),
                source: e,
            })?;
        if !canon.starts_with(&self.canon_root) {
            return Err(ContainmentError::Escaped {
                path: path.to_string(),
                root: self.root.display().to_string(),
            });
        }
        Ok(canon)
    }
}

/// Syntactic depth check: walk path components, reject if `../` takes
/// depth below zero (escaping the root).
fn validate_relative_depth(path: &str, p: &Path) -> Result<(), ContainmentError> {
    let mut depth: i32 = 0;
    for component in p.components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(ContainmentError::Escaped {
                        path: path.to_string(),
                        root: String::new(),
                    });
                }
            }
            Component::Normal(_) => {
                depth += 1;
            }
            Component::CurDir => {}
            _ => {
                return Err(ContainmentError::Escaped {
                    path: path.to_string(),
                    root: String::new(),
                });
            }
        }
    }
    Ok(())
}

/// Canonicalize a path, or if it doesn't exist, canonicalize the nearest
/// existing ancestor and append the remaining components.
fn canonicalize_or_ancestor(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    // Walk up to the nearest existing ancestor.
    let mut ancestor = path;
    let mut tail_components = Vec::new();
    loop {
        match ancestor.parent() {
            Some(p) if p.exists() => {
                // Collect remaining components (the filename at each level).
                if let Some(file_name) = ancestor.file_name() {
                    tail_components.push(file_name.to_os_string());
                }
                let canon_ancestor = p.canonicalize()?;
                // Rebuild the path by appending tail components in reverse.
                let mut result = canon_ancestor;
                for c in tail_components.into_iter().rev() {
                    result.push(c);
                }
                return Ok(result);
            }
            Some(p) => {
                if let Some(file_name) = ancestor.file_name() {
                    tail_components.push(file_name.to_os_string());
                }
                ancestor = p;
            }
            None => return path.canonicalize(),
        }
    }
}

// Static assertions: all public API types must be Send + Sync.
const _: () = {
    fn _assert<T: Send + Sync>() {}
    let _ = _assert::<PathGuard>;
    let _ = _assert::<AbsolutePathPolicy>;
    let _ = _assert::<ContainmentError>;
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relative_path_within_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("src").join("..").join("file.txt"), "ok").ok();
        fs::write(dir.path().join("file.txt"), "ok").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert!(guard.check_path("file.txt").is_ok());
    }

    #[test]
    fn relative_path_with_safe_parent_traversal() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), "ok").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        // src/../Cargo.toml stays within workspace
        assert!(guard.check_path("src/../Cargo.toml").is_ok());
    }

    #[test]
    fn relative_path_escaping_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let err = guard.check_path("../../etc/passwd").unwrap_err();
        assert!(matches!(err, ContainmentError::Escaped { .. }));
    }

    #[test]
    fn absolute_path_with_allow_if_contained() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("inside.txt"), "ok").unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let abs = dir.path().join("inside.txt");
        assert!(guard.check_path(abs.to_str().unwrap()).is_ok());
    }

    /// Return an absolute path string that is guaranteed outside any temp dir.
    fn outside_absolute_path() -> &'static str {
        #[cfg(unix)]
        {
            "/etc/passwd"
        }
        #[cfg(windows)]
        {
            "C:\\Windows\\System32\\notepad.exe"
        }
    }

    #[test]
    fn absolute_path_outside_workspace_with_allow_if_contained() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowIfContained,
        )
        .unwrap();
        let err = guard.check_path(outside_absolute_path()).unwrap_err();
        assert!(matches!(err, ContainmentError::Escaped { .. }));
    }

    #[test]
    fn absolute_path_with_reject_policy() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let err = guard.check_path(outside_absolute_path()).unwrap_err();
        assert!(matches!(err, ContainmentError::AbsolutePath(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("escape");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let err = guard.check_path("escape").unwrap_err();
        assert!(matches!(err, ContainmentError::Escaped { .. }));
    }

    #[test]
    fn nonexistent_file_in_existing_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let result = guard.check_path("new_file.txt");
        assert!(
            result.is_ok(),
            "non-existent file with safe ancestor should be allowed"
        );
    }

    #[test]
    fn empty_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        // Empty string resolves to cwd itself, which is contained.
        assert!(guard.check_path("").is_ok());
    }

    #[test]
    fn single_parent_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert!(guard.check_path("..").is_err());
    }

    #[test]
    fn deep_traversal_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert!(guard.check_path("a/b/../../../escape").is_err());
    }

    #[test]
    fn dot_relative_path_allowed() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("foo")).unwrap();
        fs::write(dir.path().join("foo/bar.json"), "{}").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert!(guard.check_path("./foo/bar.json").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn new_file_through_symlink_dir_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("link_dir");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let err = guard.check_path("link_dir/new_file.txt").unwrap_err();
        assert!(
            matches!(err, ContainmentError::Escaped { .. }),
            "new file through symlink escaping workspace should be rejected"
        );
    }

    #[test]
    fn check_path_returns_canonicalized_path() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "ok").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let resolved = guard.check_path("test.txt").unwrap();
        // The returned path should be absolute (canonicalized).
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("test.txt"));
    }

    #[test]
    fn root_and_canon_root_accessors() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert_eq!(guard.root(), dir.path());
        assert!(guard.canon_root().is_absolute());
    }

    #[test]
    fn new_with_nonexistent_root_returns_canonicalize_error() {
        let err = PathGuard::new(
            PathBuf::from("/nonexistent_patchloom_test_dir_xyz"),
            AbsolutePathPolicy::Reject,
        )
        .unwrap_err();
        assert!(matches!(err, ContainmentError::Canonicalize { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains("failed to canonicalize"),
            "expected canonicalize message, got: {msg}"
        );
    }

    #[test]
    fn error_display_absolute_path() {
        let err = ContainmentError::AbsolutePath("/etc/passwd".to_string());
        let msg = err.to_string();
        assert!(
            msg.contains("absolute paths are not allowed") && msg.contains("/etc/passwd"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn error_display_escaped() {
        let err = ContainmentError::Escaped {
            path: "../../secret".to_string(),
            root: "/home/user".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("escapes workspace") && msg.contains("../../secret"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn error_source_canonicalize_returns_inner_error() {
        use std::error::Error;
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = ContainmentError::Canonicalize {
            path: "test".to_string(),
            source: io_err,
        };
        assert!(err.source().is_some());
        let msg = err.to_string();
        assert!(msg.contains("failed to canonicalize"), "got: {msg}");
    }

    #[test]
    fn error_source_non_canonicalize_returns_none() {
        use std::error::Error;
        let err = ContainmentError::AbsolutePath("/x".to_string());
        assert!(err.source().is_none());
        let err2 = ContainmentError::Escaped {
            path: "..".to_string(),
            root: "/r".to_string(),
        };
        assert!(err2.source().is_none());
    }
}
