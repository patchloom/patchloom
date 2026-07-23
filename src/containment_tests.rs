use super::*;
use std::fs;
use std::path::{Path, PathBuf};

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
    // PathGuard root and resolved path must agree under starts_with (#1931).
    assert!(
        resolved.starts_with(guard.canon_root()),
        "resolved {resolved:?} must start with canon_root {:?}",
        guard.canon_root()
    );
}

#[test]
fn root_and_canon_root_accessors() {
    let dir = tempfile::TempDir::new().unwrap();
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    assert_eq!(guard.root(), dir.path());
    assert!(guard.canon_root().is_absolute());
}

#[test]
fn safe_canonicalize_existing_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let canon = safe_canonicalize(dir.path()).unwrap();
    assert!(canon.is_absolute());
    assert!(canon.exists());
}

#[test]
fn safe_canonicalize_nonexistent_returns_error() {
    let result = safe_canonicalize(Path::new("/nonexistent/patchloom_dunce_xyz"));
    assert!(result.is_err());
}

#[test]
fn path_guard_canon_root_has_no_windows_unc_prefix() {
    // #1931: std::fs::canonicalize on Windows yields \\?\ paths that break
    // starts_with when compared to non-UNC absolutes. dunce strips them.
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "ok").unwrap();
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    let root_s = guard.canon_root().to_string_lossy();
    assert!(
        !root_s.starts_with(r"\\?\"),
        "canon_root must not use UNC prefix, got: {root_s}"
    );
    let resolved = guard.check_path("f.txt").unwrap();
    let res_s = resolved.to_string_lossy();
    assert!(
        !res_s.starts_with(r"\\?\"),
        "check_path result must not use UNC prefix, got: {res_s}"
    );
    // Absolute path under AllowIfContained must still contain.
    let abs = dir.path().join("f.txt");
    let guard_abs = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let abs_resolved = guard_abs
        .check_path(abs.to_str().expect("utf-8 temp path"))
        .expect("absolute under workspace must be allowed");
    assert!(
        abs_resolved.starts_with(guard_abs.canon_root()),
        "abs resolved {abs_resolved:?} vs root {:?}",
        guard_abs.canon_root()
    );
}

#[cfg(windows)]
#[test]
fn windows_safe_canonicalize_preserves_drive_letter() {
    let dir = tempfile::TempDir::new().unwrap();
    let canon = safe_canonicalize(dir.path()).unwrap();
    let s = canon.to_string_lossy();
    assert!(
        s.len() >= 3
            && s.as_bytes()[0].is_ascii_alphabetic()
            && s.as_bytes()[1] == b':'
            && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/'),
        "expected drive letter path, got: {s}"
    );
}

#[cfg(windows)]
#[test]
fn windows_mixed_unc_and_plain_still_contain() {
    // Simulate the embedder failure mode: root built from dunce, check
    // path from std canonicalize (or the reverse). Both paths through
    // PathGuard must use the same helper so starts_with works.
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "x").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    // Absolute path as Windows would spell it without going through our API.
    let abs = dir.path().join("inside.txt");
    let std_canon = std::fs::canonicalize(&abs).unwrap();
    // Even if the raw string has UNC, PathGuard re-canonicalizes with dunce.
    let s = std_canon.to_string_lossy();
    let ok = guard.check_path(s.as_ref());
    assert!(
        ok.is_ok(),
        "PathGuard must accept std-canonicalized absolute path {s}: {ok:?}"
    );
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

    let guard = PathGuard::new(workspace.clone(), AbsolutePathPolicy::AllowIfContained).unwrap();

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
