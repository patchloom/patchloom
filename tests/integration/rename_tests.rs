use super::*;

#[test]
fn test_rename_moves_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be removed");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");
}

#[test]
fn test_rename_check_does_not_move() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(src.exists(), "source should still exist in --check mode");
    assert!(
        !dst.exists(),
        "destination should not be created in --check mode"
    );
}

#[test]
fn test_rename_refuses_overwrite_without_force() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "source\n").unwrap();
    fs::write(&dst, "existing\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));

    // Both files should remain untouched.
    assert_eq!(fs::read_to_string(&src).unwrap(), "source\n");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "existing\n");
}

#[test]
fn test_rename_same_path_without_force_is_noop() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("same.txt");
    fs::write(&path, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&path)
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "source and destination are the same",
        ));

    assert_eq!(fs::read_to_string(&path).unwrap(), "content\n");
}

#[test]
fn test_rename_different_path_string_is_noop() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("same.txt");
    fs::write(&file, "content\n").unwrap();

    // Use ./same.txt as destination - different string, same file.
    let alt_path = dir.path().join("./same.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&file)
        .arg(&alt_path)
        .arg("--apply")
        .arg("--force")
        .assert()
        .code(0);

    assert!(file.exists(), "file must not be deleted");
    assert_eq!(fs::read_to_string(&file).unwrap(), "content\n");
}

#[test]
fn test_rename_force_overwrites() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "new content\n").unwrap();
    fs::write(&dst, "old content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "new content\n");
}

#[test]
fn test_rename_apply_undo_restores_original_paths() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be removed after rename");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&src).unwrap(), "content\n");
    assert!(
        !dst.exists(),
        "undo should remove the created destination file"
    );
}

#[test]
fn test_rename_force_apply_undo_restores_overwritten_destination() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "source content\n").unwrap();
    fs::write(&dst, "destination content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be removed after rename");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "source content\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&src).unwrap(), "source content\n");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "destination content\n");
}

#[test]
fn test_rename_missing_source_fails() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(dir.path().join("nope.txt"))
        .arg(dir.path().join("dst.txt"))
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("source file not found"));
}

#[test]
fn test_rename_check_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("moved-folder");
    fs::create_dir(&src).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("source is not a file"));

    assert!(src.is_dir(), "source directory should remain in place");
    assert!(!dst.exists(), "destination should not be created");
}

#[test]
fn test_rename_force_directory_destination_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_force_directory_destination_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_force_directory_destination_fails_in_apply_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("moved-folder");
    fs::create_dir(&src).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("source is not a file"));

    assert!(src.is_dir(), "source directory should remain in place");
    assert!(!dst.exists(), "destination should not be created");
}

#[test]
fn test_rename_binary_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("image.bin");
    let dst = dir.path().join("moved.bin");
    // Non-UTF-8 content.
    fs::write(&src, b"\x00\x01\x02\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"\x00\x01\x02\xff\xfe");
}

#[test]
fn test_rename_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("new.txt"))
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dir.path().join("new.txt").to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_rename_json_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");
}

#[test]
fn test_rename_check_json_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("new.txt"))
        .arg("--check")
        .output()
        .unwrap();

    // --check returns exit code 2 (CHANGES_DETECTED).
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json.get("applied").is_none());
    // Source should still exist in --check mode.
    assert!(src.exists());
}

#[cfg(unix)]
#[test]
fn test_rename_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "rename",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--confirm",
        ],
        "y\n",
    );

    assert!(output.status.success());
    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert_eq!(json["applied"], true);
    assert!(json["diff"].as_str().unwrap().contains("+content"));
}

#[cfg(unix)]
#[test]
fn test_rename_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "rename",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--confirm",
        ],
        "\u{4}",
    );

    assert!(output.status.success());
    assert!(
        src.exists(),
        "source should remain when confirmation input ends at EOF"
    );
    assert!(
        !dst.exists(),
        "destination should not be created when confirmation input ends at EOF"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert_eq!(json["applied"], false);
    assert!(json["diff"].as_str().unwrap().contains("+content"));
}

#[test]
fn test_rename_same_path_without_force_json_output() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("same.txt");
    fs::write(&path, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&path)
        .arg(&path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], path.to_str().unwrap());
    assert_eq!(json["to"], path.to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_rename_binary_file_diff_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("binary.bin");
    fs::write(&src, b"\x00\x01\x02\xff").unwrap();

    // Default mode (--diff) should not crash on binary files.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("moved.bin"))
        .assert()
        .code(0);

    // Source should still exist (no --apply).
    assert!(src.exists());
}

#[test]
fn test_rename_with_write_policy() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("no-newline.txt");
    let dst = dir.path().join("fixed.txt");
    fs::write(&src, "no newline at end").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "no newline at end\n");
}

#[test]
fn test_rename_binary_with_write_policy_includes_path_in_error() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("binary.bin");
    // Non-UTF-8 content triggers read_to_string failure in write-policy branch.
    fs::write(&src, b"\x00\x01\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("moved.bin"))
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("binary.bin"));
}

// ---------------------------------------------------------------------------
// rename: default diff preview and tx --check mode
// ---------------------------------------------------------------------------

#[test]
fn test_rename_default_diff_preview() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "hello\n").unwrap();
    let dst = dir.path().join("new.txt");

    // No --apply or --check: default mode shows diff preview.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .assert()
        .success()
        .stdout(predicate::str::contains("-hello"))
        .stdout(predicate::str::contains("+hello"));

    // Source should still exist (no mutation in preview mode).
    assert!(src.exists());
    assert!(!dst.exists());
}

#[test]
fn test_rename_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("flat.txt");
    let dst = dir.path().join("sub").join("dir").join("moved.txt");
    fs::write(&src, "data\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "data\n");
}

// ---------------------------------------------------------------------------
// create --check parent directory verification
// ---------------------------------------------------------------------------

#[test]
fn test_rename_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be gone");
    assert!(dir.path().join("new.txt").exists(), "dest should exist");

    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(
        backup_dir.exists(),
        "backup dir should exist after rename --apply"
    );
}
