use super::*;

#[test]
fn test_create_rejects_content_and_stdin_together() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("dual-source.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("inline")
        .arg("--stdin")
        .write_stdin("stdin-data\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--content and --stdin cannot be combined",
        ));

    assert!(
        !file.exists(),
        "file should not be created on invalid input"
    );
}

// ---------------------------------------------------------------------------
// completions
// ---------------------------------------------------------------------------

#[test]
fn test_create_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("backup_test.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(file.to_str().unwrap())
        .arg("--content")
        .arg("hello\n")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    // A backup session directory should exist.
    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(backup_dir.exists(), "backup directory should be created");

    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(sessions.len(), 1, "exactly one backup session expected");
}

#[test]
fn test_create_apply_undo_removes_created_file() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("undoable.txt")
        .arg("--content")
        .arg("hello\n")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(dir.path().join("undoable.txt").exists());

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(
        !dir.path().join("undoable.txt").exists(),
        "undo should remove the newly created file"
    );
}

#[test]
fn test_create_force_apply_undo_restores_overwritten() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("overwrite.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("overwrite.txt")
        .arg("--content")
        .arg("replaced\n")
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "replaced\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "original\n",
        "undo should restore the original content"
    );
}

#[test]
fn test_create_new_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_create_refuses_overwrite() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("overwrite")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("already exists"));

    // Original content must be preserved.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "original content\n");
}

// ---------------------------------------------------------------------------
// tx
// ---------------------------------------------------------------------------

#[test]
fn test_create_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--check")
        .assert()
        .code(2);

    assert!(!file.exists(), "file should not be created in --check mode");
}

#[test]
fn test_create_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_create_check_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    // diff field should be absent in --check mode
    assert!(json.get("diff").is_none());
    assert!(json.get("applied").is_none());

    assert!(
        !file.exists(),
        "file should not be created in --check --json mode"
    );
}

#[cfg(unix)]
#[test]
fn test_create_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("confirmed.txt");

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "create",
            file.to_str().unwrap(),
            "--content",
            "hello",
            "--confirm",
        ],
        "y\n",
    );

    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert_eq!(json["applied"], true);
    assert!(json["diff"].as_str().unwrap().contains("+hello"));
}

#[cfg(unix)]
#[test]
fn test_create_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("declined.txt");

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "create",
            file.to_str().unwrap(),
            "--content",
            "hello",
            "--confirm",
        ],
        "\u{4}",
    );

    assert!(output.status.success());
    assert!(
        !file.exists(),
        "file should not be created when confirmation input ends at EOF"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert_eq!(json["applied"], false);
    assert!(json["diff"].as_str().unwrap().contains("+hello"));
}

// ---------------------------------------------------------------------------
// rename command
// ---------------------------------------------------------------------------

#[test]
fn test_create_check_fails_if_parent_missing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "create --check should fail when parent dir doesn't exist"
    );
}

#[test]
fn test_create_check_force_skips_parent_verification() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "create --check --force should succeed even if parent doesn't exist"
    );
}

// ---------------------------------------------------------------------------
// output format
// ---------------------------------------------------------------------------

#[test]
fn test_create_force_directory_target_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_directory_target_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_directory_target_fails_in_apply_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_overwrites_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("overwritten")
        .arg("--force")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "overwritten");
}

// ---------------------------------------------------------------------------
// error paths
// ---------------------------------------------------------------------------

#[test]
fn test_create_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("trimmed.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello   \nworld\t\n")
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello\nworld\n",
        "trailing whitespace should be trimmed"
    );
}

#[test]
fn test_create_ensure_final_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("newline.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("no trailing newline")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "no trailing newline\n",
        "final newline should be appended via File::create_new path"
    );
}

#[test]
fn test_create_normalize_eol_lf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("eol.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("line1\r\nline2\r\n")
        .arg("--normalize-eol")
        .arg("lf")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "line1\nline2\n",
        "CRLF should be normalized to LF via File::create_new path"
    );
}

// ---------------------------------------------------------------------------
// multi-file replace --apply on directory
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn test_create_in_readonly_dir_fails_gracefully() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir(&sub).unwrap();
    if !readonly_dir_blocks_writes(&sub) {
        return;
    }

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(sub.join("newfile.txt"))
        .arg("--content")
        .arg("test")
        .arg("--apply")
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "create in readonly dir should fail"
    );

    // Restore for cleanup.
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
}

// ---------------------------------------------------------------------------
// EditorConfig integration tests (end-to-end)
// ---------------------------------------------------------------------------

#[test]
fn test_create_format_flag_runs_after_apply() {
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("format_ran.marker");

    patchloom_in(dir.path())
        .args([
            "create",
            "new.txt",
            "--content",
            "hello\n",
            "--apply",
            "--format",
            &shell_touch(&marker),
        ])
        .assert()
        .code(0);

    assert!(
        marker.exists(),
        "--format command should have run after create --apply"
    );
}

// Regression: default (preview) mode must return exit code 2 (CHANGES_DETECTED)
// when the operation would produce changes, not 0 (SUCCESS).
#[test]
fn test_create_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .assert()
        .code(2);

    assert!(
        !file.exists(),
        "file should not be created in default (preview) mode"
    );
}
