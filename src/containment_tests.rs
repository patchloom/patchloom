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
    // Empty / whitespace would resolve as the workspace root (not a file target).
    let err = guard.check_path("").expect_err("empty path rejected");
    assert!(
        matches!(err, ContainmentError::EmptyPath),
        "expected EmptyPath, got {err:?}"
    );
    assert!(err.to_string().contains("path must not be empty"), "{err}");
    let err = guard
        .check_path("  \t")
        .expect_err("whitespace path rejected");
    assert!(matches!(err, ContainmentError::EmptyPath), "{err:?}");
    let err = guard
        .check_path_entry("")
        .expect_err("empty entry path rejected");
    assert!(matches!(err, ContainmentError::EmptyPath), "{err:?}");
    let zwsp = "\u{200b}";
    let err = guard.check_path(zwsp).expect_err("ZWSP-only path rejected");
    assert!(matches!(err, ContainmentError::EmptyPath), "{err:?}");
    assert!(crate::containment::is_blank_path("\u{200b}\u{feff}"));
    assert!(!crate::containment::is_blank_path("a"));
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
    assert!(
        result.is_err(),
        "missing path must error, got Ok: {result:?}"
    );
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
/// Entry mode: symlink to outside target is allowed when the link entry is inside (#2115).
#[cfg(unix)]
#[test]
fn check_path_entry_allows_symlink_to_outside_target() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    let secret = outside.path().join("secret");
    fs::write(&secret, "x").unwrap();
    let link = dir.path().join("link");
    symlink(&secret, &link).unwrap();

    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    assert!(
        guard.check_path(link.to_str().unwrap()).is_err(),
        "follow mode must deny outside target"
    );
    let entry = guard
        .check_path_entry(link.to_str().unwrap())
        .expect("entry mode must allow workspace link");
    assert!(
        entry.starts_with(guard.canon_root()),
        "entry path under root: {entry:?}"
    );
    assert!(guard.would_allow_entry(link.to_str().unwrap()));
}

/// Entry mode still rejects paths that escape via relative traversal.
#[test]
fn check_path_entry_rejects_parent_escape() {
    let dir = tempfile::TempDir::new().unwrap();
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    assert!(guard.check_path_entry("../../etc/passwd").is_err());
}

/// Entry mode follows intermediate directory components: a parent that is a
/// symlink out of the workspace is still rejected (#2115).
#[cfg(unix)]
#[test]
fn check_path_entry_rejects_intermediate_parent_symlink_escape() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    fs::write(outside.path().join("secret"), "x").unwrap();
    // workspace/outlink → outside dir; path outlink/secret would reach outside.
    let outlink = dir.path().join("outlink");
    symlink(outside.path(), &outlink).unwrap();

    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let via = outlink.join("secret");
    let via_str = via.to_str().expect("utf-8 temp path");
    assert!(
        guard.check_path(via_str).is_err(),
        "follow mode must deny intermediate outlink"
    );
    assert!(
        guard.check_path_entry(via_str).is_err(),
        "entry mode must still deny intermediate parent symlink escape"
    );
    // The outlink entry itself remains an in-workspace name (delete/rename ok).
    let outlink_str = outlink.to_str().expect("utf-8 temp path");
    assert!(
        guard.check_path_entry(outlink_str).is_ok(),
        "entry mode must allow the symlink directory entry itself"
    );
}

/// `link/../pwned.txt` must not collapse `link` away before following it.
/// A link to `.` makes the kernel resolve `..` as the workspace parent (#2466).
#[cfg(unix)]
#[test]
fn symlink_parent_dotdot_escape_is_rejected() {
    let parent = tempfile::TempDir::new().unwrap();
    let ws = parent.path().join("ws");
    fs::create_dir(&ws).unwrap();
    std::os::unix::fs::symlink(".", ws.join("link")).unwrap();
    let guard = PathGuard::new(ws, AbsolutePathPolicy::AllowIfContained).unwrap();
    let err = guard
        .check_path("link/../pwned.txt")
        .expect_err("symlink/.. must not collapse before follow");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "expected Escaped, got {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn symlink_parent_dotdot_escape_is_rejected_entry() {
    let parent = tempfile::TempDir::new().unwrap();
    let ws = parent.path().join("ws");
    fs::create_dir(&ws).unwrap();
    std::os::unix::fs::symlink(".", ws.join("link")).unwrap();
    let guard = PathGuard::new(ws, AbsolutePathPolicy::AllowIfContained).unwrap();
    let err = guard
        .check_path_entry("link/../pwned.txt")
        .expect_err("entry mode must also refuse symlink/.. escape");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "expected Escaped, got {err:?}"
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

/// `\\localhost\C$\...` is the same file as `C:\...` (fixrealloop R117/R127).
#[cfg(windows)]
#[test]
fn windows_localhost_admin_share_is_contained() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "x").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let drive = dir.path().to_string_lossy();
    assert!(
        drive.len() >= 3 && drive.as_bytes()[1] == b':',
        "temp dir must be a drive path: {drive}"
    );
    let letter = drive.as_bytes()[0] as char;
    let rest = drive[2..].trim_start_matches(['\\', '/']);
    let unc = format!(r"\\localhost\{letter}$\{rest}\inside.txt");
    let resolved = guard
        .check_path(&unc)
        .unwrap_or_else(|e| panic!("UNC in-ws must be contained: {unc} {e}"));
    let resolved_s = resolved.to_string_lossy();
    assert!(
        resolved_s.as_bytes().get(1) == Some(&b':'),
        "resolved dest must be a drive path for backup/persist, got {resolved_s}"
    );
}

#[cfg(windows)]
#[test]
fn windows_remote_admin_share_is_not_mapped() {
    assert!(
        super::windows_local_drive_share_path(std::path::Path::new(r"\\otherhost\C$\Windows"))
            .is_none(),
        "remote C$ must not map to a local drive"
    );
}

/// `//?/C:/...` exists for Path::exists but CopyFile is OS 123 (R137).
#[cfg(windows)]
#[test]
fn prefer_openable_path_forward_extended_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("t.txt");
    fs::write(&file, "x\n").unwrap();
    let fwd = std::path::PathBuf::from(format!(
        "//?/{}",
        file.display().to_string().replace('\\', "/")
    ));
    assert!(fwd.exists(), "Python/std exists accepts //?/: {fwd:?}");
    let open = super::prefer_openable_path(&fwd);
    let dest = dir.path().join("copy.txt");
    std::fs::copy(&open, &dest)
        .unwrap_or_else(|e| panic!("copy via prefer_openable_path must work: open={open:?} {e}"));
    assert_eq!(fs::read_to_string(&dest).unwrap(), "x\n");
    let s = open.to_string_lossy();
    assert!(
        !s.starts_with("//?/"),
        "openable spelling must not keep //?/: {s}"
    );
    assert!(
        s.len() >= 2 && s.as_bytes()[1] == b':',
        "openable must be drive-letter: {s}"
    );
    let root = dunce::simplified(dir.path());
    let open_simple = dunce::simplified(&open);
    assert!(
        open_simple.strip_prefix(root).is_ok(),
        "openable must stay under temp dir: open={open:?} root={:?}",
        dir.path()
    );
    assert_eq!(open.file_name().and_then(|n| n.to_str()), Some("t.txt"));
}

/// Lexical rewrite must not require the dest to exist (no canonicalize).
#[cfg(windows)]
#[test]
fn prefer_openable_path_forward_extended_prefix_is_lexical() {
    let p = std::path::PathBuf::from("//?/C:/does-not-exist-xyz/t.txt");
    let open = super::prefer_openable_path(&p);
    assert_eq!(
        open,
        std::path::PathBuf::from(r"C:\does-not-exist-xyz\t.txt")
    );
}

/// `\\.\C:\...` is the same drive dest as `C:\...` (#2322).
#[cfg(windows)]
#[test]
fn prefer_openable_path_dot_device_drive_is_lexical() {
    let p = std::path::PathBuf::from(r"\\.\C:\does-not-exist-xyz\t.txt");
    let open = super::prefer_openable_path(&p);
    assert_eq!(
        open,
        std::path::PathBuf::from(r"C:\does-not-exist-xyz\t.txt")
    );
    let nul = super::prefer_openable_path(std::path::Path::new(r"\\.\NUL"));
    assert_eq!(
        nul,
        std::path::PathBuf::from(r"\\.\NUL"),
        "NUL device must not map to a drive"
    );
    let pipe = super::prefer_openable_path(std::path::Path::new(r"\\.\pipe\pl-test"));
    assert_eq!(
        pipe,
        std::path::PathBuf::from(r"\\.\pipe\pl-test"),
        "named pipe must not map to a drive"
    );
}

/// `--contain` must treat `//?/C:/ws/file` as inside `C:\ws` (#2320).
#[cfg(windows)]
#[test]
fn contain_forward_extended_prefix_in_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("con.txt");
    fs::write(&file, "in\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let fwd = format!("//?/{}", file.display().to_string().replace('\\', "/"));
    let resolved = guard
        .check_path(&fwd)
        .unwrap_or_else(|e| panic!("//?/ in-ws must be contained: {fwd} {e}"));
    assert_eq!(
        resolved.file_name().and_then(|n| n.to_str()),
        Some("con.txt"),
        "GHA TEMP can be 8.3 (RUNNER~1) while the dest is the long name"
    );
}

/// Outside `//?/C:/Windows/...` must still be Escaped under --contain.
#[cfg(windows)]
#[test]
fn contain_forward_extended_prefix_outside_workspace() {
    let dir = tempfile::TempDir::new().unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let err = guard.check_path("//?/C:/Windows/win.ini").unwrap_err();
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "outside //?/ must stay Escaped, got {err:?}"
    );
}

/// `\\[::1]\C$\...` is the same file as `C:\...` (fixrealloop R131).
#[cfg(windows)]
#[test]
fn windows_ipv6_loopback_admin_share_is_contained() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "x").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let drive = dir.path().to_string_lossy();
    assert!(
        drive.len() >= 3 && drive.as_bytes()[1] == b':',
        "temp dir must be a drive path: {drive}"
    );
    let letter = drive.as_bytes()[0] as char;
    let rest = drive[2..].trim_start_matches(['\\', '/']);
    for host in ["[::1]", "::1"] {
        let unc = format!(r"\\{host}\{letter}$\{rest}\inside.txt");
        let resolved = guard
            .check_path(&unc)
            .unwrap_or_else(|e| panic!("IPv6 UNC in-ws must be contained: {unc} {e}"));
        let resolved_s = resolved.to_string_lossy();
        assert!(
            resolved_s.as_bytes().get(1) == Some(&b':'),
            "resolved dest must be a drive path for backup/persist, got {resolved_s}"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_remote_ipv6_admin_share_is_not_mapped() {
    assert!(
        super::windows_local_drive_share_path(std::path::Path::new(r"\\[2001:db8::1]\C$\Windows"))
            .is_none(),
        "remote IPv6 C$ must not map to a local drive"
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
    let err3 = ContainmentError::EmptyPath;
    assert!(err3.source().is_none());
    assert!(
        err3.to_string().contains("path must not be empty"),
        "{}",
        err3
    );
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

/// Missing extra allow-roots stay on the allowlist via their existing ancestor.
#[test]
fn missing_extra_allow_root_accepts_paths_under_it() {
    let parent = tempfile::TempDir::new().unwrap();
    let workspace = parent.path().join("ws");
    fs::create_dir(&workspace).unwrap();
    let scratch = parent.path().join("scratch");
    assert!(
        !scratch.exists(),
        "scratch dir must not exist yet: {}",
        scratch.display()
    );

    let guard = PathGuard::builder(workspace)
        .allow_root(&scratch)
        .build()
        .expect("a missing extra root must not fail PathGuard::build");

    let file = scratch.join("nested").join("file.txt");
    let resolved = guard
        .check_path(file.to_str().expect("utf-8 temp path"))
        .expect("absolute file under a missing extra root must be allowed");
    let expected = safe_canonicalize(parent.path())
        .expect("temp parent exists")
        .join("scratch")
        .join("nested")
        .join("file.txt");
    assert_eq!(
        resolved, expected,
        "resolved path must use the canonical ancestor, not the original spelling"
    );

    let outside = parent.path().join("outside").join("nope.txt");
    let err = guard
        .check_path(outside.to_str().expect("utf-8 temp path"))
        .expect_err("file outside the scratch root must be rejected");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "expected Escaped, got {err:?}"
    );
    assert!(
        !err.to_string().contains("could not be resolved"),
        "a missing leaf whose ancestor exists stays a plain escape: {err}"
    );
}

/// An extra allow-root with no resolvable ancestor is named when a check
/// fails. Paths under the workspace, and under a root that did resolve,
/// stay allowed (#2735).
#[test]
fn unresolved_extra_allow_root_is_named_on_escape() {
    let parent = tempfile::TempDir::new().unwrap();
    let workspace = parent.path().join("ws");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("inside.txt"), "ok").unwrap();
    let scratch = parent.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    let pid = std::process::id();
    let bad = PathBuf::from(format!("patchloom-missing-allow-root-{pid}-nope/scratch"));
    let bad_also = PathBuf::from(format!("patchloom-missing-allow-root-{pid}-also/scratch"));
    assert!(
        canonicalize_or_ancestor(&bad).is_err(),
        "fixture must be a root canonicalize cannot resolve"
    );
    assert!(canonicalize_or_ancestor(&bad_also).is_err());

    let guard = PathGuard::builder(workspace.clone())
        .allow_root(&scratch)
        .allow_root(&bad)
        .allow_root(&bad_also)
        .build()
        .expect("unresolvable extra roots are reported on check, not on build");

    guard
        .check_path(workspace.join("inside.txt").to_str().unwrap())
        .expect("in-workspace path stays allowed when another extra root failed");
    guard
        .check_path(scratch.join("note.txt").to_str().unwrap())
        .expect("a resolved extra root stays allowed beside an unresolved one");

    let outside = parent.path().join("outside.txt");
    let err = guard
        .check_path(outside.to_str().unwrap())
        .expect_err("outside path is rejected");
    let bad_disp = bad.display().to_string();
    let bad_also_disp = bad_also.display().to_string();
    match &err {
        ContainmentError::UnresolvedAllowRoot {
            unresolved, root, ..
        } => {
            assert_eq!(unresolved.len(), 2, "both failed roots are named");
            assert_eq!(unresolved[0].path, bad_disp);
            assert_eq!(unresolved[1].path, bad_also_disp);
            assert_eq!(root, &workspace.display().to_string());
        }
        other => panic!("expected UnresolvedAllowRoot, got {other:?}"),
    }
    let msg = err.to_string();
    assert!(
        msg.contains("escapes workspace")
            && msg.contains("extra allow-roots could not be resolved")
            && msg.contains(&bad_disp)
            && msg.contains(&bad_also_disp),
        "escape must name each unresolved allow-root, got: {msg}"
    );
    assert!(
        std::error::Error::source(&err).is_some(),
        "unresolved allow-root error must expose the I/O source"
    );

    let entry_err = guard
        .check_path_entry(outside.to_str().unwrap())
        .expect_err("entry mode rejects the same outside path");
    let entry_msg = entry_err.to_string();
    assert!(
        matches!(entry_err, ContainmentError::UnresolvedAllowRoot { .. })
            && entry_msg.contains(&bad_disp)
            && entry_msg.contains(&bad_also_disp),
        "entry escape must name the unresolved allow-roots, got: {entry_msg}"
    );
}

#[test]
fn error_display_unresolved_allow_root_names_the_root() {
    let err = ContainmentError::UnresolvedAllowRoot {
        path: "/outside/file".to_string(),
        root: "/ws".to_string(),
        unresolved: vec![FailedAllowRoot {
            path: "nope/scratch".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing ancestor"),
        }],
    };
    let msg = err.to_string();
    assert!(
        msg.contains("escapes workspace")
            && msg.contains("/outside/file")
            && msg.contains("/ws")
            && msg.contains("extra allow-root could not be resolved")
            && msg.contains("nope/scratch")
            && msg.contains("missing ancestor"),
        "unexpected message: {msg}"
    );
    assert!(
        !msg.contains("extra allow-roots could not be resolved"),
        "one root uses the singular label: {msg}"
    );
}

/// A symlink loop whose parent exists is kept via that ancestor. It is not
/// the unresolved-root failure path (#2735).
#[cfg(unix)]
#[test]
fn symlink_loop_extra_root_with_existing_parent_stays_allowed() {
    use std::os::unix::fs::symlink;
    let parent = tempfile::TempDir::new().unwrap();
    let workspace = parent.path().join("ws");
    fs::create_dir(&workspace).unwrap();
    let loop_path = parent.path().join("loop");
    symlink("loop", &loop_path).unwrap();
    assert!(
        canonicalize_or_ancestor(&loop_path).is_ok(),
        "a loop whose parent exists is kept via that ancestor"
    );
    let guard = PathGuard::builder(workspace)
        .allow_root(&loop_path)
        .build()
        .unwrap();
    let under = loop_path.join("file.txt");
    guard
        .check_path(under.to_str().unwrap())
        .expect("lexical child of a loop root stays allowed");
    let outside = parent.path().join("outside.txt");
    let err = guard
        .check_path(outside.to_str().unwrap())
        .expect_err("outside path is still rejected");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "a resolved loop root must not look unresolved: {err}"
    );
}

/// Permission denied while resolving an extra root is named, not omitted.
#[cfg(unix)]
#[test]
fn permission_denied_extra_allow_root_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::TempDir::new().unwrap();
    let workspace = parent.path().join("ws");
    fs::create_dir(&workspace).unwrap();
    fs::write(workspace.join("inside.txt"), "ok").unwrap();
    let blocked = parent.path().join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
    struct Restore<'a>(&'a Path);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let _restore = Restore(&blocked);
    // Root (common in Docker) can still read mode-000 directories.
    if fs::read_dir(&blocked).is_ok() {
        return;
    }
    let extra = blocked.join("scratch");
    if canonicalize_or_ancestor(&extra).is_ok() {
        return;
    }
    let guard = PathGuard::builder(workspace.clone())
        .allow_root(&extra)
        .build()
        .unwrap();
    guard
        .check_path(workspace.join("inside.txt").to_str().unwrap())
        .expect("in-workspace path stays allowed when the extra root is unreadable");
    let outside = parent.path().join("outside.txt");
    let err = guard
        .check_path(outside.to_str().unwrap())
        .expect_err("outside path is rejected");
    let msg = err.to_string();
    assert!(
        matches!(err, ContainmentError::UnresolvedAllowRoot { .. })
            && msg.contains(&extra.display().to_string())
            && msg.contains("could not be resolved"),
        "permission failure must name the extra allow-root, got: {msg}"
    );
}

/// `../<dirname>/<file>` climbs out and resolves back inside the workspace.
#[test]
fn relative_dotdot_reentering_workspace_is_allowed() {
    let parent = tempfile::TempDir::new().unwrap();
    let ws = parent.path().join("proj");
    fs::create_dir_all(ws.join("src")).unwrap();
    let file = ws.join("src").join("lib.rs");
    fs::write(&file, "ok").unwrap();
    let expected = safe_canonicalize(&file).expect("workspace file exists");
    let guard = PathGuard::new(ws, AbsolutePathPolicy::Reject).unwrap();

    let resolved = guard
        .check_path("../proj/src/lib.rs")
        .expect("climb that resolves back inside the workspace must be allowed");
    assert_eq!(resolved, expected);
    let entry = guard
        .check_path_entry("../proj/src/lib.rs")
        .expect("entry mode must allow the same in-workspace climb");
    assert_eq!(entry, expected);

    let err = guard
        .check_path("../../secret.txt")
        .expect_err("a real relative escape must stay rejected");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "expected Escaped, got {err:?}"
    );
    let err = guard
        .check_path_entry("../../secret.txt")
        .expect_err("entry mode must reject a real relative escape");
    assert!(
        matches!(err, ContainmentError::Escaped { .. }),
        "expected Escaped, got {err:?}"
    );
}

/// A directory symlink to a nested dir, then enough `..` to go lexically
/// negative, still resolves inside. Collapsing `..` before following `link`
/// would land outside the workspace (#2466).
#[cfg(unix)]
#[test]
fn relative_dotdot_through_dir_symlink_reenters_workspace() {
    use std::os::unix::fs::symlink;
    let parent = tempfile::TempDir::new().unwrap();
    let ws = parent.path().join("proj");
    let nested = ws.join("nested").join("inner");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("file.txt");
    fs::write(&file, "ok").unwrap();
    symlink("nested/inner", ws.join("link")).unwrap();
    let expected = safe_canonicalize(&file).expect("nested file exists");
    let guard = PathGuard::new(ws, AbsolutePathPolicy::Reject).unwrap();

    let resolved = guard
        .check_path("link/../../nested/inner/file.txt")
        .expect("symlink climb that re-enters the workspace must be allowed");
    assert_eq!(resolved, expected);
    assert!(
        resolved.starts_with(guard.canon_root()),
        "resolved {resolved:?} must stay under {:?}",
        guard.canon_root()
    );
    let entry = guard
        .check_path_entry("link/../../nested/inner/file.txt")
        .expect("entry mode must allow the same symlink climb");
    assert_eq!(entry, expected);
}
