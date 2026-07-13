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
//! ## Policy choices for different use cases
//!
//! | Policy | Use when | Example |
//! |--------|----------|---------|
//! | `Reject` | MCP server or untrusted agent output (default for safety) | CLI tools exposed over network |
//! | `AllowIfContained` | Trusted library use, absolute paths inside workspace only | Standard agent in project dir |
//! | `AllowAdditionalRoots(...)` or builder | Agents needing /tmp (incl. macOS symlinks), build artifacts, scratch dirs while keeping guard for sensitive paths | Host experiment mode / temp-file agents |
//!
//! **Threat model note**: MCP uses untrusted LLM-generated paths, so strict `Reject`.
//! Direct library embedding (LLM agent hosts that control the agent process) can use
//! relaxed policies because the host owns path policy.
//! Even with extra roots, escapes *out of* allowed roots are still blocked.
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
//!
//! ## Builder for agents
//!
//! ```rust,no_run
//! use patchloom::containment::PathGuard;
//!
//! let guard = PathGuard::builder(std::env::current_dir().unwrap())
//!     .allow_temp_directory()           // /tmp + std temp (handles macOS symlinks)
//!     .allow_root("/tmp/my-experiments")
//!     .build()
//!     .unwrap();
//! ```

use std::path::{Component, Path, PathBuf};

/// Policy for handling absolute paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbsolutePathPolicy {
    /// Reject all absolute paths (current strict default, used by MCP).
    Reject,

    /// Allow absolute paths only if they resolve inside the primary workspace root.
    AllowIfContained,

    /// Allow absolute paths if they resolve inside the workspace root **or**
    /// inside any of the additional allowed roots.
    ///
    /// Recommended for most library/agent use cases (e.g. /tmp, scratch dirs).
    AllowAdditionalRoots(Vec<PathBuf>),
}

/// Return a list of common system temp directory paths (entry points).
/// Includes `std::env::temp_dir()` plus conventional locations like `/tmp`
/// (and its symlinked target on macOS), `/var/tmp`, and `$TMPDIR` on Unix.
/// On Windows also considers TEMP/TMP.
/// These are stored raw; callers canonicalize at check time so that
/// paths passed as `/tmp/...` resolve correctly under the allowed root.
fn system_temp_directory_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![std::env::temp_dir()];

    #[cfg(unix)]
    {
        push_unique(&mut roots, PathBuf::from("/tmp"));
        push_unique(&mut roots, PathBuf::from("/var/tmp"));
        if let Ok(v) = std::env::var("TMPDIR") {
            push_unique(&mut roots, PathBuf::from(v));
        }
    }

    #[cfg(windows)]
    {
        for var in ["TEMP", "TMP"] {
            if let Ok(v) = std::env::var(var) {
                push_unique(&mut roots, PathBuf::from(v));
            }
        }
    }

    roots
}

/// Helper to keep additional roots deduplicated (used by temp root builder
/// and policy merge paths).
fn push_unique(roots: &mut Vec<PathBuf>, p: PathBuf) {
    if !roots.contains(&p) {
        roots.push(p);
    }
}

impl AbsolutePathPolicy {
    /// Workspace root + common system temp directories (via `system_temp_directory_roots`).
    /// This covers `std::env::temp_dir()` plus `/tmp` (and symlinked forms like `/private/tmp`
    /// on macOS), `$TMPDIR`, etc. Intended for agents that need temp files or build outputs
    /// and commonly spell temp paths as `/tmp/...`.
    pub fn allow_workspace_and_temp_dir() -> Self {
        AbsolutePathPolicy::AllowAdditionalRoots(system_temp_directory_roots())
    }

    /// Allow the workspace plus any number of extra trusted roots.
    pub fn allow_additional_roots(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        AbsolutePathPolicy::AllowAdditionalRoots(roots.into_iter().collect())
    }
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
        Self::new_with_policy(root, absolute_policy)
    }

    /// Validate that `path` stays within the workspace root (or additional roots if configured).
    ///
    /// Returns the canonicalized path on success. Rejects paths that
    /// escape the allowed roots via `../` traversal or symlinks.
    pub fn check_path(&self, path: &str) -> Result<PathBuf, ContainmentError> {
        let p = Path::new(path);

        if p.is_absolute() {
            match &self.absolute_policy {
                AbsolutePathPolicy::Reject => {
                    return Err(ContainmentError::AbsolutePath(path.to_string()));
                }
                AbsolutePathPolicy::AllowIfContained => {
                    // Skip syntactic check, go straight to symlink-aware check.
                    let roots = vec![self.canon_root.clone()];
                    return self.check_resolved_absolute(path, p, &roots);
                }
                AbsolutePathPolicy::AllowAdditionalRoots(extra) => {
                    let mut allowed = vec![self.canon_root.clone()];
                    for r in extra {
                        if let Ok(c) = r.canonicalize() {
                            allowed.push(c);
                        }
                    }
                    return self.check_resolved_absolute(path, p, &allowed);
                }
            }
        }

        // Syntactic depth-tracking check (no I/O). Relative always against primary root.
        validate_relative_depth(path, p, &self.root)?;

        // Symlink-aware containment check (primary root).
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

    /// Returns true if the path would be allowed under the current policy
    /// (dry-run, no error details).
    pub fn would_allow(&self, path: &str) -> bool {
        self.check_path(path).is_ok()
    }

    /// Check that an absolute path resolves within one of the allowed roots.
    fn check_resolved_absolute(
        &self,
        path: &str,
        p: &Path,
        allowed_roots: &[PathBuf],
    ) -> Result<PathBuf, ContainmentError> {
        let canon = canonicalize_or_ancestor(p).map_err(|e| ContainmentError::Canonicalize {
            path: path.to_string(),
            source: e,
        })?;
        let contained = allowed_roots.iter().any(|r| canon.starts_with(r));
        if !contained {
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

/// Builder for `PathGuard` to ergonomically configure flexible policies
/// (useful for library users like agents that need temp dirs or extra roots).
pub struct PathGuardBuilder {
    root: PathBuf,
    policy: AbsolutePathPolicy,
}

impl PathGuard {
    /// Create a builder for ergonomic configuration of allowed roots.
    pub fn builder(root: PathBuf) -> PathGuardBuilder {
        PathGuardBuilder {
            root,
            policy: AbsolutePathPolicy::Reject,
        }
    }

    /// Create with explicit policy (back-compat + power users).
    pub fn new_with_policy(
        root: PathBuf,
        policy: AbsolutePathPolicy,
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
            absolute_policy: policy,
        })
    }
}

impl PathGuardBuilder {
    /// Allow the system temporary directory (and common aliases such as `/tmp` on Unix).
    /// Uses `system_temp_directory_roots()` so that literal `/tmp/...` paths (common
    /// from agents, scripts, and LLM output) are allowed even on macOS where
    /// `std::env::temp_dir()` returns a `/var/folders/...` path and `/tmp` is a symlink
    /// to `/private/tmp`.
    /// Merges into existing policy if needed.
    pub fn allow_temp_directory(mut self) -> Self {
        let temps = system_temp_directory_roots();
        self.policy = match self.policy {
            AbsolutePathPolicy::Reject | AbsolutePathPolicy::AllowIfContained => {
                AbsolutePathPolicy::AllowAdditionalRoots(temps)
            }
            AbsolutePathPolicy::AllowAdditionalRoots(mut roots) => {
                for t in temps {
                    push_unique(&mut roots, t);
                }
                AbsolutePathPolicy::AllowAdditionalRoots(roots)
            }
        };
        self
    }

    /// Allow one or more additional trusted roots (e.g. scratch dirs).
    /// Merges into existing policy.
    pub fn allow_root(mut self, additional: impl Into<PathBuf>) -> Self {
        let extra = additional.into();
        self.policy = match self.policy {
            AbsolutePathPolicy::Reject | AbsolutePathPolicy::AllowIfContained => {
                AbsolutePathPolicy::AllowAdditionalRoots(vec![extra])
            }
            AbsolutePathPolicy::AllowAdditionalRoots(mut roots) => {
                push_unique(&mut roots, extra);
                AbsolutePathPolicy::AllowAdditionalRoots(roots)
            }
        };
        self
    }

    /// Build the `PathGuard`.
    pub fn build(self) -> Result<PathGuard, ContainmentError> {
        PathGuard::new_with_policy(self.root, self.policy)
    }
}

/// Syntactic depth check: walk path components, reject if `../` takes
/// depth below zero (escaping the root).
///
/// `root` is included in [`ContainmentError::Escaped`] so CLI `--contain`
/// and MCP errors show which workspace was enforced (MPI cycle 16; previously
/// the message always printed `workspace: ` with an empty root).
fn validate_relative_depth(path: &str, p: &Path, root: &Path) -> Result<(), ContainmentError> {
    let mut depth: i32 = 0;
    for component in p.components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(ContainmentError::Escaped {
                        path: path.to_string(),
                        root: root.display().to_string(),
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
                    root: root.display().to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Lexically resolve `.` and `..` components without touching the filesystem.
///
/// This must run before the ancestor walk in [`canonicalize_or_ancestor`]
/// because [`Path::file_name`] returns `None` for `..` components, which
/// would silently drop them during reconstruction.
fn normalize_lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut parts: Vec<Component<'_>> = Vec::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if matches!(parts.last(), Some(Component::Normal(_))) {
                    parts.pop();
                } else {
                    // At root or beyond; keep the `..` so canonicalize()
                    // can produce a proper I/O error if needed.
                    parts.push(c);
                }
            }
            Component::CurDir => { /* skip */ }
            _ => parts.push(c),
        }
    }
    parts.iter().collect()
}

/// Canonicalize a path, or if it doesn't exist, canonicalize the nearest
/// existing ancestor and append the remaining components.
fn canonicalize_or_ancestor(path: &Path) -> std::io::Result<PathBuf> {
    // Normalize `..` and `.` lexically first so the ancestor walk never
    // encounters components that `file_name()` would silently skip.
    let normalized = normalize_lexical(path);
    let path = normalized.as_path();

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
        fs::write(dir.path().join("file.txt"), "ok").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        guard.check_path("file.txt").unwrap();
    }

    #[test]
    fn relative_path_with_safe_parent_traversal() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), "ok").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        // src/../Cargo.toml stays within workspace
        guard.check_path("src/../Cargo.toml").unwrap();
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
        guard.check_path(abs.to_str().unwrap()).unwrap();
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
        guard.check_path("").unwrap();
    }

    #[test]
    fn single_parent_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        guard.check_path("..").expect_err("expected error");
    }

    #[test]
    fn deep_traversal_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        guard
            .check_path("a/b/../../../escape")
            .expect_err("expected error");
    }

    #[test]
    fn dot_relative_path_allowed() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("foo")).unwrap();
        fs::write(dir.path().join("foo/bar.json"), "{}").unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        guard.check_path("./foo/bar.json").unwrap();
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
            msg.contains("escapes workspace")
                && msg.contains("../../secret")
                && msg.contains("/home/user"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn parent_escape_error_includes_workspace_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        let err = guard.check_path("../escape.txt").unwrap_err();
        let msg = err.to_string();
        let root_disp = dir.path().display().to_string();
        assert!(
            msg.contains("escapes") && msg.contains(&root_disp),
            "escape error must name workspace root {root_disp:?}, got: {msg}"
        );
        assert!(
            !msg.contains("(workspace: )"),
            "must not print empty workspace root: {msg}"
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
        // strengthened (was bare is_some); expect gives context on failure
        let _src = err
            .source()
            .expect("Canonicalize error must carry inner io source");
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

    #[test]
    fn allow_additional_roots_allows_temp_and_extra() {
        let dir = tempfile::TempDir::new().unwrap();
        let temp = std::env::temp_dir();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::AllowAdditionalRoots(vec![temp.clone()]),
        )
        .unwrap();
        // absolute in extra root should work
        let tmp_file = temp.join("patchloom_test_extra.txt");
        // create it
        std::fs::write(&tmp_file, "ok").unwrap();
        let res = guard.check_path(tmp_file.to_str().unwrap());
        res.unwrap();
        // clean
        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn allow_additional_roots_still_rejects_outside() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::allow_workspace_and_temp_dir(),
        )
        .unwrap();
        let err = guard.check_path("/etc/passwd").unwrap_err();
        assert!(matches!(err, ContainmentError::Escaped { .. }));
    }

    #[test]
    fn builder_allows_temp() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();
        // should allow temp
        let temp = std::env::temp_dir().join("patchloom_builder_test.txt");
        std::fs::write(&temp, "ok").unwrap();
        guard.check_path(temp.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&temp);
    }

    /// Regression test for #781: literal `/tmp/...` (common spelling from agents/LLMs)
    /// must be allowed by `allow_temp_directory()` even on macOS where
    /// std::env::temp_dir() is under /var/folders and /tmp symlinks to /private/tmp.
    #[cfg(unix)]
    #[test]
    fn builder_allows_literal_tmp_path_unix() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        // Use a unique name under the conventional /tmp to simulate real usage
        // (host experiment-mode paths, agent-generated paths, etc.).
        let tmp_path = format!("/tmp/patchloom_781_test_{}.txt", std::process::id());
        let _ = std::fs::remove_file(&tmp_path);
        std::fs::write(&tmp_path, "data for 781").unwrap();
        let res = guard.check_path(&tmp_path);
        assert!(
            res.is_ok(),
            "literal /tmp path must be allowed: {:?}",
            res.err()
        );
        let _ = std::fs::remove_file(&tmp_path);
    }

    /// Non-existing file under /tmp must still be allowed (via ancestor canonicalization).
    #[cfg(unix)]
    #[test]
    fn builder_allows_nonexistent_under_literal_tmp() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let tmp_path = format!("/tmp/patchloom_781_nonexist_{}.txt", std::process::id());
        // do not create it
        let res = guard.check_path(&tmp_path);
        assert!(
            res.is_ok(),
            "non-existing /tmp path must be allowed via ancestor: {:?}",
            res.err()
        );
    }

    #[test]
    fn allow_workspace_and_temp_dir_allows_std_temp() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::allow_workspace_and_temp_dir(),
        )
        .unwrap();
        let temp = std::env::temp_dir().join("patchloom_awtd_std.txt");
        std::fs::write(&temp, "ok").unwrap();
        guard.check_path(temp.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_file(&temp);
    }

    /// #781: allow_workspace_and_temp_dir must also cover literal /tmp.
    #[cfg(unix)]
    #[test]
    fn allow_workspace_and_temp_dir_allows_literal_tmp() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(
            dir.path().to_path_buf(),
            AbsolutePathPolicy::allow_workspace_and_temp_dir(),
        )
        .unwrap();

        let tmp_path = format!("/tmp/patchloom_781_awtd_{}.txt", std::process::id());
        let _ = std::fs::remove_file(&tmp_path);
        std::fs::write(&tmp_path, "data").unwrap();
        guard.check_path(&tmp_path).unwrap();
        let _ = std::fs::remove_file(&tmp_path);
    }

    #[test]
    fn would_allow_works() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
        assert!(guard.would_allow("foo.txt"));
        assert!(!guard.would_allow("../escape"));
    }

    /// Basic coverage for the helper (addresses review note for direct test of system_temp...).
    #[test]
    fn system_temp_directory_roots_contains_expected_entries() {
        let roots = system_temp_directory_roots();
        // Always at least the std one
        assert!(!roots.is_empty());
        // On unix we explicitly add the conventional ones
        #[cfg(unix)]
        {
            assert!(roots.iter().any(
                |r| r == std::path::Path::new("/tmp") || r == std::path::Path::new("/var/tmp")
            ));
            // std env temp is present
            let stdt = std::env::temp_dir();
            assert!(roots.iter().any(|r| r == &stdt));
        }
    }

    /// Regression for coverage: paths using ../ to escape an *allowed additional root*
    /// (e.g. literal /tmp on macOS) must still be rejected after canonicalization.
    /// This was not explicitly asserted in prior temp-allow tests.
    #[cfg(unix)]
    #[test]
    fn allow_temp_rejects_escape_from_tmp_via_parent() {
        let dir = tempfile::TempDir::new().unwrap();
        let guard = PathGuard::builder(dir.path().to_path_buf())
            .allow_temp_directory()
            .build()
            .unwrap();

        let escape_path = "/tmp/../etc/passwd";
        let err = guard.check_path(escape_path).unwrap_err();
        assert!(
            matches!(err, ContainmentError::Escaped { .. }),
            "expected Escaped for escape from /tmp root via .., got {:?}",
            err
        );
    }

    /// Regression: `..` components in non-existent absolute paths must not be
    /// silently dropped during ancestor canonicalization. `Path::file_name()`
    /// returns `None` for `..`, so the old code skipped those components,
    /// making a path like `workspace/nonexistent/../../outside/secret.txt`
    /// appear to be `workspace/nonexistent/outside/secret.txt` (inside).
    #[test]
    fn dotdot_in_nonexistent_absolute_path_rejected() {
        let base = tempfile::TempDir::new().unwrap();
        let workspace = base.path().join("workspace");
        let outside = base.path().join("outside");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "sensitive").unwrap();

        let guard =
            PathGuard::new(workspace.clone(), AbsolutePathPolicy::AllowIfContained).unwrap();

        // workspace/nonexistent/../../outside/secret.txt
        // resolves lexically to: base/outside/secret.txt (outside workspace)
        let escape = workspace
            .join("nonexistent")
            .join("..")
            .join("..")
            .join("outside")
            .join("secret.txt");

        let result = guard.check_path(escape.to_str().unwrap());
        assert!(
            matches!(result, Err(ContainmentError::Escaped { .. })),
            "path with .. through non-existent dir should be rejected, got: {:?}",
            result,
        );
    }

    /// Same bypass with AllowAdditionalRoots policy.
    #[test]
    fn dotdot_in_nonexistent_path_rejected_with_additional_roots() {
        let base = tempfile::TempDir::new().unwrap();
        let workspace = base.path().join("workspace");
        let extra_root = base.path().join("extra");
        let outside = base.path().join("outside");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&extra_root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("data.txt"), "sensitive").unwrap();

        let guard = PathGuard::new(
            workspace.clone(),
            AbsolutePathPolicy::AllowAdditionalRoots(vec![extra_root]),
        )
        .unwrap();

        // extra uses nonexistent to escape via ..
        let escape = workspace
            .join("nonexistent")
            .join("..")
            .join("..")
            .join("outside")
            .join("data.txt");

        let result = guard.check_path(escape.to_str().unwrap());
        assert!(
            matches!(result, Err(ContainmentError::Escaped { .. })),
            "dotdot escape via additional-roots should be rejected, got: {:?}",
            result,
        );
    }
}
