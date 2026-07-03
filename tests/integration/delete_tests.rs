use super::*;

#[test]
fn test_delete_removes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "bye\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!file.exists());
}

#[test]
fn test_delete_jsonl_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "keep\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("safe.txt"));
}

#[test]
fn test_delete_json_apply_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!file.exists());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], true);
    assert!(json["path"].as_str().unwrap().contains("doomed.txt"));
}

#[cfg(unix)]
#[test]
fn test_delete_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("confirmed.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &["--json", "delete", file.to_str().unwrap(), "--confirm"],
        "y\n",
    );

    assert!(output.status.success());
    assert!(!file.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], true);
    assert!(json["path"].as_str().unwrap().contains("confirmed.txt"));
}

#[cfg(unix)]
#[test]
fn test_delete_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("declined.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &["--json", "delete", file.to_str().unwrap(), "--confirm"],
        "\u{4}",
    );

    assert_eq!(
        output.status.code(),
        Some(2), // CHANGES_DETECTED
        "declining --confirm --json should return CHANGES_DETECTED (exit 2)"
    );
    assert!(
        file.exists(),
        "file should remain when confirmation input ends at EOF"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "bye\n");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("declined.txt"));
}

#[test]
fn test_delete_json_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "keep\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(file.exists());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("safe.txt"));
}

#[test]
fn test_delete_check_mode_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "still here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(2);

    assert!(file.exists());
}

#[test]
fn test_delete_directory_target_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(target.to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_delete_directory_target_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(target.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_delete_nonexistent_file_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ghost.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_delete_default_dry_run_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "content\n").unwrap();

    // Default mode (no --apply, no --check) is a dry-run.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .assert()
        .code(2)
        .stdout(predicate::str::contains("would delete"));

    assert!(file.exists(), "dry-run should not delete the file");
}

#[test]
fn test_delete_quiet_dry_run_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("quiet_delete.txt");
    fs::write(&file, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());

    assert!(file.exists(), "quiet dry-run should not delete the file");
}

#[test]
fn test_delete_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "original content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!file.exists(), "file should be deleted");

    // A backup session should exist with the original content.
    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(
        backup_dir.exists(),
        "backup dir should exist after delete --apply"
    );

    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(sessions.len(), 1, "one backup session expected");
}

#[test]
fn test_delete_apply_undo_restores_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("precious.txt");
    fs::write(&file, "precious data\n").unwrap();

    // Delete the file.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("precious.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!file.exists());

    // Undo should restore it.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(file.exists(), "undo should restore the deleted file");
    assert_eq!(fs::read_to_string(&file).unwrap(), "precious data\n");
}

/// Binary files must be deletable without UTF-8 errors (#1163).
#[test]
fn test_delete_binary_file_succeeds() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    fs::write(&file, b"\x00\x01\x02\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["delete", "--apply"])
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    assert!(!file.exists(), "binary file should be deleted");
}

// Regression: default (preview) mode must return exit 2 (CHANGES_DETECTED), not 0.
#[test]
fn test_delete_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "keep me\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["delete"])
        .arg(file.to_str().unwrap())
        .assert()
        .code(2);

    assert!(file.exists(), "file should not be deleted in default mode");
}
