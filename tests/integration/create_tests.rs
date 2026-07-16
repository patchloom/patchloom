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
    assert_eq!(
        json["applied"], false,
        "check must set applied:false: {json}"
    );

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

    assert_eq!(
        output.status.code(),
        Some(2), // CHANGES_DETECTED
        "declining --confirm --json should return CHANGES_DETECTED (exit 2)"
    );
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
fn test_create_check_allows_missing_parent() {
    // --apply creates parent dirs; --check must not reject missing parents
    // (parity restored after brief parent-must-exist check was removed).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("would create"));

    assert!(!file.exists(), "check mode must not create the file");
}

#[test]
fn test_create_apply_creates_missing_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_create_rejects_parent_that_is_a_file() {
    let dir = TempDir::new().unwrap();
    let blocking = dir.path().join("notdir");
    fs::write(&blocking, "i am a file\n").unwrap();
    let child = blocking.join("child.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("create")
        .arg(&child)
        .arg("--content")
        .arg("x\n")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "invalid_input");
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("not a directory"),
        "error should mention not a directory: {json}"
    );

    assert!(!child.exists());
    assert!(blocking.is_file());
    assert!(
        !dir.path().join(".patchloom/backups").exists(),
        "must not leave a backup session for an unwritten create"
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

#[test]
fn test_create_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = patchloom_in(dir.path())
        .args([
            "--json",
            "create",
            "new.txt",
            "--content",
            "hello\n",
            "--apply",
            "--format",
            "false",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "format_failed",
        "create --format failure should set error_kind: {parsed}"
    );
    assert!(
        file.exists() && fs::read_to_string(&file).unwrap().contains("hello"),
        "create must still write before format failure"
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

// ---------------------------------------------------------------------------
// --contain on create (engine-backed path) (#1406)
// ---------------------------------------------------------------------------

#[test]
fn test_create_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "create",
            "../escape-outside.txt",
            "--content",
            "nope\n",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert!(!dir.path().join("../escape-outside.txt").exists());
}

#[test]
fn test_create_without_contain_allows_parent_escape() {
    let dir = TempDir::new().unwrap();
    let name = format!(
        "patchloom-create-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .arg("create")
        .arg(format!("../{name}"))
        .args(["--content", "escaped\n", "--apply"])
        .assert()
        .code(0);

    let outside = dir.path().parent().unwrap().join(&name);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "escaped\n");
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_create_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "create",
            "inside.txt",
            "--content",
            "ok\n",
            "--apply",
        ])
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("inside.txt")).unwrap(),
        "ok\n"
    );
}

#[test]
fn test_create_json_already_exists_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("exists.txt");
    fs::write(&file, "original\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "create"])
        .arg(&file)
        .args(["--content", "new\n", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "already_exists",
        "create --json existing file should set error_kind: {parsed}"
    );
}

#[test]
fn test_create_apply_json_reports_applied_true() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["applied"], true,
        "create --apply --json must set applied:true for agent parity with delete: {json}"
    );
}
