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

// ---------------------------------------------------------------------------
// Expansions: md / patch / tx / rename / long-path
// ---------------------------------------------------------------------------

/// Absolute path under `--contain` for md upsert-bullet (engine write path).
#[test]
fn contain_md_upsert_bullet_absolute_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Intro\n\n").unwrap();
    let abs = file
        .canonicalize()
        .unwrap_or_else(|_| file.clone())
        .to_string_lossy()
        .replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "md",
            "upsert-bullet",
            &abs,
            "--heading",
            "Intro",
            "--bullet",
            "- item",
            "--apply",
        ])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("- item"),
        "md absolute under --contain must write: {content}"
    );
}

/// Absolute patch *file* path under `--contain` (meta-input resolve_user_path).
#[test]
fn contain_patch_check_absolute_patch_file() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("t.txt");
    fs::write(&target, "hello\n").unwrap();
    let patch = dir.path().join("change.diff");
    // Unified diff with relative target under --cwd.
    let body = "\
--- a/t.txt
+++ b/t.txt
@@ -1 +1 @@
-hello
+world
";
    fs::write(&patch, body).unwrap();
    let abs_patch = patch
        .canonicalize()
        .unwrap_or_else(|_| patch.clone())
        .to_string_lossy()
        .replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "patch", "check", &abs_patch])
        .assert()
        .code(2); // changes detected in check mode
}

/// Absolute plan path for `tx` under `--contain`.
#[test]
fn contain_tx_absolute_plan_path() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("note.txt");
    fs::write(&target, "alpha\n").unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{
  "version": 1,
  "operations": [
    {"op": "replace", "path": "note.txt", "old": "alpha", "new": "beta"}
  ]
}
"#,
    )
    .unwrap();
    let abs_plan = plan
        .canonicalize()
        .unwrap_or_else(|_| plan.clone())
        .to_string_lossy()
        .replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "tx", &abs_plan, "--apply"])
        .assert()
        .code(0);

    let content = fs::read_to_string(&target).unwrap();
    assert!(
        content.contains("beta"),
        "tx absolute plan under --contain must apply: {content}"
    );
}

/// Absolute plan path outside workspace rejected under `--contain`.
#[test]
fn contain_tx_rejects_absolute_plan_outside_workspace() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let plan = outside.path().join("evil.json");
    fs::write(&plan, r#"{"version":1,"operations":[]}"#).unwrap();
    let abs = plan
        .canonicalize()
        .unwrap_or_else(|_| plan.clone())
        .to_string_lossy()
        .into_owned();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "tx", &abs, "--apply"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );
}

/// Absolute rename under `--contain` (both from and to).
#[test]
fn contain_rename_absolute_paths_apply() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old-name.txt");
    fs::write(&src, "data\n").unwrap();
    let dst = dir.path().join("new-name.txt");
    let abs_src = src
        .canonicalize()
        .unwrap_or_else(|_| src.clone())
        .to_string_lossy()
        .replace('\\', "/");
    let abs_dst = dst.to_string_lossy().replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "rename", &abs_src, &abs_dst, "--apply"])
        .assert()
        .code(0);

    assert!(!src.exists() || !src.is_file() || fs::read_to_string(&src).is_err());
    let content = fs::read_to_string(&dst).unwrap();
    assert!(
        content.contains("data"),
        "rename absolute under --contain: {content}"
    );
}

/// Case-only rename under `--contain` with absolute paths (Windows/macOS).
#[test]
fn contain_rename_case_only_absolute_paths() {
    let dir = TempDir::new().unwrap();
    let lower = dir.path().join("readme.md");
    fs::write(&lower, "hello\n").unwrap();
    let abs_from = lower
        .canonicalize()
        .unwrap_or_else(|_| lower.clone())
        .to_string_lossy()
        .replace('\\', "/");
    // Same directory, different case of basename.
    let mut abs_to = abs_from.clone();
    if let Some(i) = abs_to.rfind("readme.md") {
        abs_to.replace_range(i..i + "readme.md".len(), "README.md");
    } else if let Some(i) = abs_to.rfind("README.md") {
        abs_to.replace_range(i..i + "README.md".len(), "readme.md");
    }

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "rename", &abs_from, &abs_to, "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let entries: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.eq_ignore_ascii_case("readme.md"))
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected one readme.md entry: {entries:?}"
    );
}

/// Deep relative escape under `--contain` still fails closed.
#[test]
fn contain_rejects_deep_relative_escape() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    fs::write(dir.path().join("a/b/c/x.txt"), "in\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", "a/b/c/../../../../../../etc/passwd"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );
}

/// Batch ops file as absolute path under `--contain`.
#[test]
fn contain_batch_absolute_ops_file() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("b.txt");
    fs::write(&target, "old\n").unwrap();
    let ops = dir.path().join("ops.txt");
    // batch replace: PATH OLD NEW
    fs::write(&ops, "replace b.txt old new\n").unwrap();
    let abs_ops = ops
        .canonicalize()
        .unwrap_or_else(|_| ops.clone())
        .to_string_lossy()
        .replace('\\', "/");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "batch", &abs_ops, "--apply"])
        .assert()
        .code(0);

    let content = fs::read_to_string(&target).unwrap();
    assert!(
        content.contains("new"),
        "batch absolute ops under --contain: {content}"
    );
}

#[cfg(windows)]
mod windows_expansions {
    use super::*;
    use std::path::PathBuf;

    /// Case-only rename with std-canonicalize (UNC) absolute paths under contain.
    #[test]
    fn contain_rename_case_only_std_canonicalized_paths() {
        let dir = TempDir::new().unwrap();
        let lower = dir.path().join("notes.txt");
        fs::write(&lower, "body\n").unwrap();
        let from = std::fs::canonicalize(&lower).unwrap();
        let from_s = from.to_string_lossy().into_owned();
        let to_s = from_s.replace("notes.txt", "NOTES.txt");

        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--cwd"])
            .arg(dir.path())
            .args(["--contain", "rename", &from_s, &to_s, "--apply"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.eq_ignore_ascii_case("notes.txt"))
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    /// Long path under workspace: either succeeds with simplified path, or
    /// fails closed with a clear error (never panics / never silent wrong write).
    #[test]
    fn long_path_under_workspace_is_honest() {
        let dir = TempDir::new().unwrap();
        // Build a nested directory name that exceeds classic MAX_PATH (260)
        // when fully joined, using a single long component where possible.
        let mut long = dir.path().to_path_buf();
        // ~40 * 8 = 320 chars of nesting under temp (often > 260 total).
        for i in 0..40 {
            long.push(format!("seg{i:02}xx"));
        }
        // Creating deep trees may require \\?\ on Windows.
        let create = std::process::Command::new("cmd")
            .args(["/C", "mkdir", &format!("\\\\?\\{}", long.display())])
            .output();
        if create.map(|o| !o.status.success()).unwrap_or(true) {
            // Fallback: try std create_dir_all (works if long-path registry is on).
            if fs::create_dir_all(&long).is_err() {
                // Environment cannot create long paths; skip without failing CI.
                return;
            }
        }
        let file = long.join("leaf.txt");
        let write_path = PathBuf::from(format!("\\\\?\\{}", file.display()));
        if fs::write(&write_path, "long-ok\n").is_err() {
            let _ = fs::write(&file, "long-ok\n");
        }
        if !file.exists() && !write_path.exists() {
            return;
        }

        let abs = std::fs::canonicalize(&file)
            .or_else(|_| std::fs::canonicalize(&write_path))
            .unwrap_or(file.clone());
        let s = abs.to_string_lossy().into_owned();

        let out = Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--json", "--cwd"])
            .arg(dir.path())
            .args(["--contain", "read", &s])
            .output()
            .unwrap();

        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if out.status.success() {
            assert!(
                combined.contains("long-ok"),
                "long path read success must return content: {combined}"
            );
        } else {
            // Fail closed: structured failure, no panic, path mentioned.
            assert!(
                combined.contains("error")
                    || combined.contains("failed")
                    || combined.contains("path")
                    || combined.contains("escapes"),
                "long path failure must be actionable: {combined}"
            );
        }
    }

    /// Outside long path under --contain rejects (fail closed).
    #[test]
    fn contain_rejects_outside_long_absolute_path() {
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let mut long = outside.path().to_path_buf();
        for i in 0..20 {
            long.push(format!("out{i:02}"));
        }
        let _ = fs::create_dir_all(&long);
        let file = long.join("x.txt");
        if fs::write(&file, "x\n").is_err() {
            return;
        }
        let abs = std::fs::canonicalize(&file).unwrap_or(file);
        let s = abs.to_string_lossy().into_owned();

        Command::cargo_bin("patchloom")
            .unwrap()
            .args(["--cwd"])
            .arg(dir.path())
            .args(["--contain", "read", &s])
            .assert()
            .failure()
            .stderr(
                predicate::str::contains("escapes")
                    .or(predicate::str::contains("rejected"))
                    .or(predicate::str::contains("workspace guard")),
            );
    }
}
