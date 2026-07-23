//! Platform path honesty: absolute paths under `--contain`, Windows UNC, and
//! CLI error paths that embed OS paths.
//!
//! These are the minimum integration locks for Windows-native and WSL-style
//! absolute path handling. Unit tests in `containment_tests` cover PathGuard
//! library behavior; this module exercises the full CLI binary.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// #1931 / #1451: absolute path under workspace is allowed with `--contain`.
#[test]
fn contain_allows_absolute_path_inside_workspace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("inside.txt");
    fs::write(&file, "hello-abs\n").unwrap();
    let abs = file
        .canonicalize()
        .unwrap_or_else(|_| file.clone())
        .to_string_lossy()
        .into_owned();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", &abs])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("hello-abs"));
}

/// Absolute path outside the workspace must fail closed under `--contain`.
#[test]
fn contain_rejects_absolute_path_outside_workspace() {
    let dir = TempDir::new().unwrap();
    let outside_dir = TempDir::new().unwrap();
    let outside = outside_dir.path().join("secret.txt");
    fs::write(&outside, "SECRET\n").unwrap();
    let abs = outside
        .canonicalize()
        .unwrap_or_else(|_| outside.clone())
        .to_string_lossy()
        .into_owned();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", &abs])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard"))
                .or(predicate::str::contains("absolute")),
        );
}

/// `--json` missing-path error includes OS detail once (no UNC double-noise).
#[test]
fn json_missing_path_error_is_single_line_usable() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope.txt");
    let abs = missing.to_string_lossy().into_owned();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["read", &abs])
        .output()
        .expect("run patchloom");

    assert_ne!(out.status.code(), Some(0), "missing path must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let body = if stdout.contains("error") {
        stdout.as_ref()
    } else {
        stderr.as_ref()
    };
    assert!(
        body.contains("nope") || body.contains("not found") || body.contains("No such file"),
        "expected path/OS detail in JSON error, got: {body}"
    );
    // #1931 display honesty: agent envelopes must not show raw UNC.
    assert!(
        !body.contains(r"\\?\"),
        "JSON error must not embed Windows UNC prefix: {body}"
    );
    // #1929: OS detail once (when present).
    // #1929: "No such file" must not appear twice in one envelope.
    assert!(
        body.matches("No such file").count() <= 1,
        "OS detail must not double in agent JSON: {body}"
    );
}

/// Doc get absolute path under `--contain` (sole-path load + JSON).
#[test]
fn contain_doc_get_absolute_inside_workspace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("cfg.json");
    fs::write(&file, r#"{"port": 9}"#).unwrap();
    let abs = file
        .canonicalize()
        .unwrap_or_else(|_| file.clone())
        .to_string_lossy()
        .into_owned();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["--contain", "doc", "get", &abs, "port"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains('9'));
}

/// Replace with absolute path under `--contain` (write path + PathGuard).
#[test]
fn contain_replace_absolute_path_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("note.txt");
    fs::write(&file, "old-token\n").unwrap();
    let abs = file
        .canonicalize()
        .unwrap_or_else(|_| file.clone())
        .to_string_lossy()
        .into_owned();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "replace",
            "old-token",
            "--new",
            "new-token",
            &abs,
            "--apply",
        ])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new-token"),
        "absolute-path replace under --contain must write: {content}"
    );
}

/// Forward-slash absolute paths (common from agents / WSL-style spellings).
#[test]
fn contain_allows_forward_slash_absolute_inside_workspace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("slash.txt");
    fs::write(&file, "slash-ok\n").unwrap();
    let abs = file.canonicalize().unwrap_or_else(|_| file.clone());
    let slashy = abs.to_string_lossy().replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", &slashy])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("slash-ok"));
}

/// Library PathGuard via CLI: create under absolute dest path inside workspace.
#[test]
fn contain_create_absolute_path_apply() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("created.txt");
    // Use non-existing absolute path (ancestor canonicalize).
    let abs = dest.to_string_lossy().replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "create",
            &abs,
            "--content",
            "created-ok\n",
            "--apply",
        ])
        .assert()
        .code(0);

    let content = fs::read_to_string(&dest).unwrap();
    assert!(
        content.contains("created-ok"),
        "absolute create under --contain must write: {content}"
    );
}

#[cfg(windows)]
mod windows_native {
    use super::*;

    /// #1931 regression at CLI: feed a path that went through std canonicalize
    /// (may be `\\?\` form). PathGuard must re-canonicalize with dunce and allow.
    #[test]
    fn contain_accepts_std_canonicalized_absolute_path() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("unc.txt");
        fs::write(&file, "unc-ok\n").unwrap();
        let std_canon = std::fs::canonicalize(&file).expect("std canonicalize");
        let s = std_canon.to_string_lossy().into_owned();
        // On modern Windows this often starts with \\?\ — that is the bug class.
        let _maybe_unc = s.starts_with(r"\\?\");

        Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--cwd"])
            .arg(dir.path())
            .args(["--contain", "read", &s])
            .assert()
            .code(0)
            .stdout(predicate::str::contains("unc-ok"));
    }

    /// Agent JSON for containment escape must not leak UNC prefixes.
    #[test]
    fn contain_json_escape_error_has_no_unc_prefix() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let file = outside.path().join("x.txt");
        fs::write(&file, "x\n").unwrap();
        let abs = std::fs::canonicalize(&file)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned();

        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--json", "--cwd"])
            .arg(dir.path())
            .args(["--contain", "read", &abs])
            .output()
            .expect("run");

        assert!(!out.status.success());
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !combined.contains(r"\\?\"),
            "containment error JSON must not use UNC: {combined}"
        );
    }

    /// Drive-letter absolute path under workspace (C:\... form without UNC).
    #[test]
    fn contain_drive_letter_absolute_inside_workspace() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("drive.txt");
        fs::write(&file, "drive-ok\n").unwrap();
        // Prefer dunce form if available via patchloom API; otherwise strip UNC.
        let abs = std::fs::canonicalize(&file).unwrap();
        let s = abs.to_string_lossy();
        let drive = s.strip_prefix(r"\\?\").unwrap_or(&s);

        Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--cwd"])
            .arg(dir.path())
            .args(["--contain", "read", drive])
            .assert()
            .code(0)
            .stdout(predicate::str::contains("drive-ok"));
    }
}
