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
    assert_eq!(json["applied"], true, "{json}");
    // Direct rename creates a backup; agents need the session id for undo.
    assert!(
        json["backup_session"].as_str().is_some_and(|s| !s.is_empty()),
        "rename --apply must include backup_session: {json}"
    );
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
    assert_eq!(
        json["applied"], false,
        "check must set applied:false: {json}"
    );
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

    assert_eq!(
        output.status.code(),
        Some(2), // CHANGES_DETECTED
        "declining --confirm --json should return CHANGES_DETECTED (exit 2)"
    );
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

    // Default mode (--diff) should not crash on binary files and should
    // return exit 2 (CHANGES_DETECTED) since a rename would happen.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("moved.bin"))
        .assert()
        .code(2);

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
        .code(2)
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
// rename backup session
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

/// Case-only renames should work on case-insensitive filesystems (#1167).
#[test]
fn test_rename_case_only_change() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(&file, "hello\n").unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["rename", "--apply"])
        .arg(file.to_str().unwrap())
        .arg(dir.path().join("README.md").to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    // Direct case-only path must not pretend the file is binary (fixrealloop R5).
    assert!(
        !combined.contains("(binary)"),
        "case-only rename of text must not say (binary): {combined}"
    );
    if combined.contains("renamed") {
        assert!(
            combined.contains("(case-only)") || !combined.contains('('),
            "expected (case-only) label when status is printed: {combined}"
        );
    }

    // Verify the rename happened by reading the directory.
    let entries: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();

    // On case-insensitive FS, there should be exactly one .md file.
    assert_eq!(entries.len(), 1);
    // On case-sensitive FS, the new name should be README.md.
    // On case-insensitive FS, the dir entry shows whatever case was set last.
    assert!(
        entries[0] == "README.md" || entries[0] == "readme.md",
        "expected README.md or readme.md, got: {}",
        entries[0]
    );
}

/// Cross-directory rename with case-similar filenames must NOT bypass
/// the destination-exists check (#1169).
#[test]
fn test_rename_cross_dir_case_similar_requires_force() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::create_dir(dir.path().join("lib")).unwrap();
    fs::write(dir.path().join("src/Foo.txt"), "A\n").unwrap();
    fs::write(dir.path().join("lib/foo.txt"), "B\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["rename", "--apply"])
        .arg(dir.path().join("src/Foo.txt").to_str().unwrap())
        .arg(dir.path().join("lib/foo.txt").to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("destination already exists"));

    // Original files untouched.
    assert_eq!(
        fs::read_to_string(dir.path().join("src/Foo.txt")).unwrap(),
        "A\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("lib/foo.txt")).unwrap(),
        "B\n"
    );
}

// ---------------------------------------------------------------------------
// --contain: all rename paths (text engine + binary callback) (#1406 / #1409)
// ---------------------------------------------------------------------------

#[test]
fn test_rename_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "keep\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "rename",
            "inside.txt",
            "../escape-renamed.txt",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert!(dir.path().join("inside.txt").exists());
    assert!(!dir.path().join("../escape-renamed.txt").exists());
}

#[test]
fn test_rename_binary_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("image.bin"), b"\x00\x01\x02\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "rename",
            "image.bin",
            "../escaped.bin",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert!(dir.path().join("image.bin").exists());
    assert!(!dir.path().join("../escaped.bin").exists());
}

#[test]
fn test_rename_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "rename", "old.txt", "new.txt", "--apply"])
        .assert()
        .code(0);

    assert!(!dir.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_rename_without_contain_allows_parent_escape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "move me\n").unwrap();
    let name = format!(
        "patchloom-rename-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .arg("rename")
        .arg("inside.txt")
        .arg(format!("../{name}"))
        .arg("--apply")
        .assert()
        .code(0);

    let outside = dir.path().parent().unwrap().join(&name);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "move me\n");
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_rename_empty_path_rejected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "x\n").unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["rename", "", "b.txt"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("path must not be empty"));
}

#[test]
fn test_rename_json_already_exists_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "rename", "a.txt", "b.txt", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "already_exists",
        "rename --json existing dest should set error_kind: {parsed}"
    );
}

#[test]
fn test_rename_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "content\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json", "rename", "a.txt", "b.txt", "--apply", "--format", "false", "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "format_failed",
        "rename --format failure should set error_kind: {parsed}"
    );
    // Rename still applied before format failure.
    assert!(!dir.path().join("a.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "content\n"
    );
}
