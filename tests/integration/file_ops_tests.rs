use super::*;

#[test]
fn test_files_from_restricts_search() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("included.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("excluded.txt"), "findme\n").unwrap();

    let list = dir.path().join("filelist.txt");
    fs::write(&list, "included.txt\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg(&list)
        .arg("search")
        .arg("findme")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("included.txt"))
        .stdout(predicate::str::contains("excluded.txt").not());
}

#[test]
fn test_files_from_stdin_restricts_search() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("included.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("excluded.txt"), "findme\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("-")
        .arg("search")
        .arg("findme")
        .arg(".")
        .write_stdin("included.txt\n")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("included.txt"));
    assert!(
        !stdout.contains("excluded.txt"),
        "excluded file should not appear"
    );
}

#[test]
fn test_files_from_nonexistent_path_fails() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--files-from")
        .arg("/nonexistent/file_list_xyz.txt")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--files-from"));
}

// ---------------------------------------------------------------------------
// editorconfig integration
// ---------------------------------------------------------------------------

#[test]
fn test_append_cli_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "line one\n").unwrap();

    patchloom_in(dir.path())
        .args(["append", "log.txt", "--content", "line two\n", "--apply"])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line one\nline two\n");
}

#[test]
fn test_append_cli_check_returns_exit_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "existing\n").unwrap();

    patchloom_in(dir.path())
        .args(["append", "log.txt", "--content", "new\n", "--check"])
        .assert()
        .code(2);

    // File must be unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "existing\n");
}

#[test]
fn test_append_cli_missing_file_fails() {
    let dir = TempDir::new().unwrap();

    patchloom_in(dir.path())
        .args(["append", "nope.txt", "--content", "data\n", "--apply"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_append_cli_diff_shows_unified_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "first\n").unwrap();

    patchloom_in(dir.path())
        .args(["append", "log.txt", "--content", "second\n"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("+second"));
}

#[test]
fn test_append_format_flag_runs_after_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "line\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    patchloom_in(dir.path())
        .args([
            "append",
            "f.txt",
            "--content",
            "extra\n",
            "--apply",
            "--format",
            &shell_touch(&marker),
        ])
        .assert()
        .code(0);

    assert!(
        marker.exists(),
        "--format command should have run after append --apply"
    );
}

// ---------------------------------------------------------------------------
// prepend CLI (direct subcommand, not via tx/doc)
// ---------------------------------------------------------------------------

#[test]
fn test_prepend_cli_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "line two\n").unwrap();

    patchloom_in(dir.path())
        .args(["prepend", "log.txt", "--content", "line one\n", "--apply"])
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line one\nline two\n");
}

#[test]
fn test_prepend_cli_check_returns_exit_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "existing\n").unwrap();

    patchloom_in(dir.path())
        .args(["prepend", "log.txt", "--content", "new\n", "--check"])
        .assert()
        .code(2);

    // File must be unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "existing\n");
}

#[test]
fn test_prepend_cli_missing_file_fails() {
    let dir = TempDir::new().unwrap();

    patchloom_in(dir.path())
        .args(["prepend", "nope.txt", "--content", "data\n", "--apply"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_prepend_cli_diff_shows_unified_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "second\n").unwrap();

    patchloom_in(dir.path())
        .args(["prepend", "log.txt", "--content", "first\n"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("+first"));
}

// Regression: default (preview) mode must return exit 2 (CHANGES_DETECTED), not 0.
#[test]
fn test_append_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "line1\n").unwrap();

    patchloom_in(dir.path())
        .args(["append", "log.txt", "--content", "line2\n"])
        .assert()
        .code(2);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\n",
        "file should not be modified in default mode"
    );
}

#[test]
fn test_prepend_default_mode_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "line1\n").unwrap();

    patchloom_in(dir.path())
        .args(["prepend", "log.txt", "--content", "header\n"])
        .assert()
        .code(2);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\n",
        "file should not be modified in default mode"
    );
}
