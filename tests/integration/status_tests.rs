use super::*;

#[test]
fn test_status_clean_repo() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .code(0); // exit 0 = no changes
}

#[test]
fn test_status_outside_git_repo_shows_actionable_hint() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("git status failed:"))
        .stderr(predicate::str::contains(
            "hint: run `git init` first, or run patchloom status from inside an existing git repository",
        ));
}

#[test]
fn test_status_modified_file() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Modify the committed file.
    fs::write(dir.path().join("a.txt"), "changed\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .code(2); // exit 2 = changes detected
}

#[test]
fn test_status_jsonl_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    fs::write(dir.path().join("new.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_changes"], 1);
    assert!(
        json["created"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "new.txt")
    );
}

#[test]
fn test_status_json_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Create untracked file.
    fs::write(dir.path().join("new.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_changes"], 1);
    assert!(
        json["created"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap() == "new.txt")
    );
}

#[test]
fn test_status_deleted_file() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "doomed.txt", "bye\n");
    git_ok(dir.path(), &["rm", "doomed.txt"]);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code().unwrap(), 2); // CHANGES_DETECTED
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap() == "doomed.txt")
    );
}

#[test]
fn test_status_glob_matches_filename_with_spaces() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    fs::write(dir.path().join("file name.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--glob")
        .arg("*.txt")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(
        created.iter().any(|v| v.as_str() == Some("file name.txt")),
        "glob-filtered status should report the unquoted filename, got: {json}"
    );
}

#[test]
fn test_status_glob_matches_nested_relative_pattern() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/keep.txt"), "new\n").unwrap();
    fs::write(dir.path().join("other.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--glob")
        .arg("sub/*.txt")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(created.iter().any(|v| v.as_str() == Some("sub/keep.txt")));
    assert!(!created.iter().any(|v| v.as_str() == Some("other.txt")));
}

// ── create command ─────────────────────────────────────────────────

#[test]
fn test_status_staged_new_file_shows_as_created() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "init.txt", "init\n");

    // Stage a new file without committing (porcelain code "A ").
    fs::write(dir.path().join("new.txt"), "new content\n").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2)); // CHANGES_DETECTED
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(
        created.iter().any(|v| v.as_str() == Some("new.txt")),
        "staged new file should appear in created list, got: {json}"
    );
}

// ---------------------------------------------------------------------------
// status --quiet suppresses output
// ---------------------------------------------------------------------------

#[test]
fn test_status_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Modify the committed file to produce changes.
    fs::write(dir.path().join("a.txt"), "changed\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2)); // CHANGES_DETECTED
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

// ---------------------------------------------------------------------------
// completions: PowerShell produces output
// ---------------------------------------------------------------------------
