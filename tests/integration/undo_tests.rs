use super::*;

#[test]
fn test_undo_restores_replaced_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    // Apply a replace.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "goodbye", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "goodbye world\n");

    // Undo should restore the original.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello world\n");
}

#[test]
fn test_undo_list_shows_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    // Apply a replace to create a backup.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    // List should show the session.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test.txt"));
}

#[test]
fn test_undo_dry_run_by_default() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    // Apply.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    // Undo without --apply should show what would change but not restore.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(2) // CHANGES_DETECTED
        .stdout(predicates::str::contains("restore original"));

    // File should still be modified.
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified\n");
}

#[test]
fn test_undo_dry_run_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "undo", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified\n");
}

#[test]
fn test_undo_list_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--json", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("\"timestamp\""))
        .stdout(predicates::str::contains("\"entries\""));
}

#[test]
fn test_undo_list_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--jsonl", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "JSONL output should be one session per line"
    );
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(json["timestamp"].is_string());
    assert!(json["entries"].is_array());
    assert_eq!(json["entries"][0]["path"], "test.txt");
}

#[test]
fn test_undo_dry_run_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--json", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert!(json["session"].is_string());
    assert_eq!(json["file_count"], 1);
    assert_eq!(json["entries"][0]["path"], "test.txt");
    assert_eq!(json["entries"][0]["action"], "restore original");
}

#[test]
fn test_undo_dry_run_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--jsonl", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["entries"][0]["path"], "test.txt");
    assert_eq!(json["entries"][0]["action"], "restore original");
}

#[test]
fn test_undo_list_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .code(0);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "undo", "--list", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_undo_tx_restores_multi_file() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("a.txt");
    let f2 = dir.path().join("b.txt");
    fs::write(&f1, "alpha\n").unwrap();
    fs::write(&f2, "beta\n").unwrap();

    let plan_content = format!(
        r#"{{"version":"1","operations":[
            {{"op":"replace","path":"{}","from":"alpha","to":"omega"}},
            {{"op":"replace","path":"{}","from":"beta","to":"gamma"}}
        ]}}"#,
        portable_path_str(&f1),
        portable_path_str(&f2)
    );
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, &plan_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tx", "--apply"])
        .arg(portable_path_str(&plan_file))
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&f1).unwrap(), "omega\n");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "gamma\n");

    // Undo should restore both files.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&f1).unwrap(), "alpha\n");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "beta\n");
}

#[test]
fn test_undo_no_sessions_exits_3() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_undo_list_json_empty_emits_array() {
    let dir = TempDir::new().unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "undo", "--list", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed.is_array(), "empty undo --list --json should emit []");
    assert_eq!(parsed.as_array().unwrap().len(), 0);
}
