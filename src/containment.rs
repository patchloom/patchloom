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

/// Canonicalize a path without the Windows `\\?\` UNC prefix when safe.
///
/// On Windows, [`std::fs::canonicalize`] returns paths like
/// `\\?\C:\Users\...`. Those break [`Path::starts_with`] when compared to a
/// non-UNC root (or vice versa) and look wrong in agent-facing display.
/// [`dunce::canonicalize`] strips the prefix when the path is shorter than
/// `MAX_PATH` and has no special characters. On non-Windows this is a
/// transparent passthrough to `std::fs::canonicalize`.
///
/// Prefer this for PathGuard roots, containment checks, and any path that may
/// be compared with `starts_with` or shown in error messages.
pub fn safe_canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

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
                        if let Ok(c) = safe_canonicalize(r) {
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
        let canon_root = safe_canonicalize(&root).map_err(|e| ContainmentError::Canonicalize {
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
///
/// Uses [`safe_canonicalize`] (dunce) so Windows UNC prefixes do not break
/// containment `starts_with` checks against the PathGuard root.
fn canonicalize_or_ancestor(path: &Path) -> std::io::Result<PathBuf> {
    // Normalize `..` and `.` lexically first so the ancestor walk never
    // encounters components that `file_name()` would silently skip.
    let normalized = normalize_lexical(path);
    let path = normalized.as_path();

    if path.exists() {
        return safe_canonicalize(path);
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
                let canon_ancestor = safe_canonicalize(p)?;
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
            None => return safe_canonicalize(path),
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
#[path = "containment_tests.rs"]
mod tests;
