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

#[test]
fn test_append_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "line\n").unwrap();

    let output = patchloom_in(dir.path())
        .args([
            "--json",
            "append",
            "f.txt",
            "--content",
            "extra\n",
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
        "append --format failure should set error_kind: {parsed}"
    );
    assert!(
        fs::read_to_string(&file).unwrap().contains("extra"),
        "append must still write before format failure"
    );
}

#[test]
fn test_prepend_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "line\n").unwrap();

    let output = patchloom_in(dir.path())
        .args([
            "--json",
            "prepend",
            "f.txt",
            "--content",
            "head\n",
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
        "prepend --format failure should set error_kind: {parsed}"
    );
    assert!(
        fs::read_to_string(&file).unwrap().starts_with("head"),
        "prepend must still write before format failure"
    );
}

#[test]
fn test_delete_format_failure_json_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "line\n").unwrap();

    let output = patchloom_in(dir.path())
        .args(["--json", "delete", "f.txt", "--apply", "--format", "false"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "format_failed",
        "delete --format failure should set error_kind: {parsed}"
    );
    assert!(
        !file.exists(),
        "delete must still remove the file before format failure"
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

// ---------------------------------------------------------------------------
// --contain on append (engine-backed content inject) (MPI cycle 13)
// ---------------------------------------------------------------------------

#[test]
fn test_append_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-append-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "base\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "--contain",
            "append",
            &format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            "--content",
            "injected\n",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert_eq!(fs::read_to_string(&outside).unwrap(), "base\n");
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_append_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("log.txt"), "one\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "--contain",
            "append",
            "log.txt",
            "--content",
            "two\n",
            "--apply",
        ])
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("log.txt")).unwrap(),
        "one\ntwo\n"
    );
}

#[test]
fn test_files_from_resolves_list_under_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "findme here\n").unwrap();
    fs::write(dir.path().join("list.txt"), "inside.txt\n").unwrap();

    // Process cwd is not dir; list path is relative and must open under --cwd.
    Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path().parent().unwrap())
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--files-from", "list.txt", "search", "findme"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("findme"));
}

/// Under --contain, the --files-from *list file* itself must not escape the workspace
/// (meta-input read). Previously only entries inside the list were checked, so an
/// absolute list path like /etc/passwd was opened and leaked via skip messages.
#[test]
fn test_files_from_contain_rejects_absolute_list_path_outside_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "ok\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "--files-from",
            "/etc/hosts",
            "search",
            "localhost",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("absolute"))
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );
}

/// #1451: absolute --files-from list path under --cwd is allowed with --contain.
#[test]
fn test_files_from_contain_allows_absolute_list_inside_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "findme here\n").unwrap();
    let list = dir.path().join("list.txt");
    fs::write(&list, "inside.txt\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .arg("--contain")
        .arg("--files-from")
        .arg(list.to_str().unwrap())
        .args(["search", "findme"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("findme"));
}

#[test]
fn test_files_from_contain_rejects_parent_list_path() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-meta-list-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "inside.txt\n").unwrap();
    fs::write(dir.path().join("inside.txt"), "findme\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "--files-from",
            &format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            "search",
            "findme",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}

#[test]
fn test_append_contain_rejects_missing_parent_escape_not_not_found() {
    // Escaped path that does not exist must still fail with containment, not
    // "file does not exist" (existence used to run before PathGuard).
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-append-missing-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "append",
            &format!("../{escape_name}"),
            "--content",
            "x\n",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        )
        .stderr(predicate::str::contains("does not exist").not());
}

#[test]
fn test_prepend_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-prepend-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "base\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "prepend",
            &format!("../{escape_name}"),
            "--content",
            "hdr\n",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert_eq!(fs::read_to_string(&outside).unwrap(), "base\n");
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_append_json_not_found_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "append",
            "missing.txt",
            "--content",
            "x\n",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "not_found",
        "append --json missing file should set error_kind: {parsed}"
    );
}

/// Empty append under --apply writes nothing: applied must be false.
#[test]
fn test_append_empty_apply_json_applied_false() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "append",
            "a.txt",
            "--content",
            "",
            "--apply",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], true, "{parsed}");
    assert_eq!(
        parsed["applied"], false,
        "empty append must not claim applied: {parsed}"
    );
    assert!(
        parsed.get("backup_session").is_none(),
        "empty append must not create a backup: {parsed}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_append_rejects_binary_file_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.bin");
    fs::write(&file, b"hello\x00world").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "append",
            "data.bin",
            "--content",
            "evil\n",
            "--apply",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "invalid_input");
    assert!(
        json["error"].as_str().unwrap_or("").contains("binary file"),
        "error should name binary: {json}"
    );
    assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");
}

#[test]
fn test_prepend_rejects_binary_file_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.bin");
    fs::write(&file, b"hello\x00world").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "prepend",
            "data.bin",
            "--content",
            "evil\n",
            "--apply",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input");
    assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");
}

#[test]
fn test_tx_append_rejects_binary_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.bin"), b"hello\x00world").unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "file.append", "path": "data.bin", "content": "evil\n"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "tx"])
        .arg(&plan_file)
        .args(["--apply", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input");
    assert!(
        json["error"].as_str().unwrap_or("").contains("binary"),
        "tx append binary: {json}"
    );
    assert_eq!(
        fs::read(dir.path().join("data.bin")).unwrap(),
        b"hello\x00world"
    );
}

#[test]
fn test_replace_sole_binary_target_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("only.bin"), b"hello\x00world").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json", "replace", "hello", "only.bin", "--new", "hi", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input");
    assert!(
        json["error"].as_str().unwrap_or("").contains("binary"),
        "replace sole binary: {json}"
    );
    assert_eq!(
        fs::read(dir.path().join("only.bin")).unwrap(),
        b"hello\x00world"
    );
}

/// Sole explicit invalid UTF-8 must be invalid_input, not pattern no_matches
/// (soft-skip misreported as a miss; fixrealloop 2026-07-21).
#[test]
fn test_replace_sole_invalid_utf8_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.txt"), b"hello \xff world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json", "replace", "hello", "bad.txt", "--new", "hi", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input", "{json}");
    let err = json["error"].as_str().unwrap_or("");
    assert!(
        err.contains("UTF-8") || err.contains("utf"),
        "replace sole invalid UTF-8: {json}"
    );
    assert_eq!(
        fs::read(dir.path().join("bad.txt")).unwrap(),
        b"hello \xff world\n"
    );
}

/// Sole tidy check on invalid UTF-8 must not report vacuous clean (exit 0).
#[test]
fn test_tidy_check_sole_invalid_utf8_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.txt"), b"line  \xff\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "tidy", "check", "bad.txt", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input", "{json}");
}

#[test]
fn test_tidy_check_sole_binary_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("only.bin"), b"a\x00b  \n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "tidy", "check", "only.bin", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input");
}

#[test]
fn test_tidy_fix_sole_binary_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("only.bin"), b"a\x00b  \n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "tidy",
            "fix",
            "only.bin",
            "--trim-trailing-whitespace",
            "--apply",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input");
    assert_eq!(
        fs::read(dir.path().join("only.bin")).unwrap(),
        b"a\x00b  \n"
    );
}

#[test]
fn test_tidy_multi_path_binary_refused() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "text  \n").unwrap();
    fs::write(dir.path().join("bin.dat"), b"hello\x00world").unwrap();
    fs::write(dir.path().join("b.txt"), "more  \n").unwrap();

    for sub in ["check", "fix"] {
        let mut args = vec!["--json", "tidy", sub, "a.txt", "bin.dat", "b.txt"];
        if sub == "fix" {
            args.push("--trim-trailing-whitespace");
        }
        args.push("--cwd");
        let output = Command::cargo_bin("patchloom")
            .unwrap()
            .args(&args)
            .arg(dir.path())
            .output()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "tidy {sub} multi binary: parse {}: {}",
                e,
                String::from_utf8_lossy(&output.stdout)
            )
        });
        let refused = json["refused"]
            .as_array()
            .unwrap_or_else(|| panic!("tidy {sub} multi-path must report refused[]: {json}"));
        assert_eq!(refused.len(), 1, "tidy {sub}: {json}");
        assert_eq!(refused[0]["reason"], "binary", "tidy {sub}: {json}");
        let path = refused[0]["path"].as_str().unwrap_or("");
        assert!(path.contains("bin.dat"), "tidy {sub} refused path: {json}");
    }
}

#[test]
fn test_md_sole_binary_is_invalid_input_not_no_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("only.bin"), b"hello\x00# H\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "--json",
            "md",
            "replace-section",
            "only.bin",
            "--heading",
            "H",
            "--content",
            "x",
            "--cwd",
        ])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "invalid_input", "{json}");
    assert!(
        json["error"].as_str().unwrap_or("").contains("binary"),
        "{json}"
    );
}
