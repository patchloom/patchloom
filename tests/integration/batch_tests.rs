use super::*;

#[test]
fn test_batch_file_rename() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("file.rename old.txt new.txt\n")
        .assert()
        .code(0);

    assert!(!dir.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_batch_md_lint_agents() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# Rules\nfoo\n# Rules\nbar\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .args(["batch", "-"])
        .write_stdin(format!("md.lint_agents {}", portable_path_str(&file)))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let lints = json["lints"].as_array().unwrap();
    assert_eq!(lints.len(), 1);
    assert!(lints[0]["issue_count"].as_u64().unwrap() >= 1);
}

#[test]
fn test_batch_diff_mode_does_not_write() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .assert()
        .code(2);

    // File must be unchanged in default (diff) mode.
    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("1.0.0"),
        "file should be unchanged: {content}"
    );
}

#[test]
fn test_batch_apply_modifies_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("2.0.0"),
        "file should be updated: {content}"
    );
}

#[test]
fn test_batch_empty_input_succeeds() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicates::str::contains("no operations found"));
}

#[test]
fn test_batch_comment_only_input_succeeds() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("comments.txt");
    fs::write(&ops, "# This is a comment\n\n# Another comment\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicates::str::contains("no operations found"));
}

#[test]
fn test_batch_json_empty_input_returns_structured_success() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 0);
    assert_eq!(json["files_created"], 0);
    assert_eq!(json["files_deleted"], 0);
    assert_eq!(json["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn test_batch_jsonl_empty_input_returns_structured_success() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "# only a comment\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 0);
}

#[test]
fn test_batch_malformed_line_fails() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad.txt");
    fs::write(&ops, "unknown.op foo bar\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("unknown operation"));
}

#[test]
fn test_batch_extra_args_fail() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad.txt");
    fs::write(&ops, "file.delete old.txt extra\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("requires exactly 1 argument"));
}

#[test]
fn test_batch_nonexistent_target_file_rollback() {
    let dir = TempDir::new().unwrap();
    // Do NOT create missing.json.
    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set missing.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(9); // OPERATION_FAILED
}

#[test]
fn test_batch_from_stdin() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"old"}"#).unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("doc.set data.json name \"new\"\n")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["name"], "new",
        "stdin batch should update name field: {content}"
    );
}

#[test]
fn test_batch_check_mode_reports_changes() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--check")
        .assert()
        .code(2); // CHANGES_DETECTED

    // File must be unchanged in --check mode.
    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("1.0.0"),
        "file should be unchanged in check mode: {content}"
    );
}

#[test]
fn test_batch_quiet_suppresses_empty_message() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "quiet should suppress stderr, got: {stderr}"
    );
}

#[test]
fn test_batch_json_output_on_apply() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"key":"old"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set data.json key \"new\"\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 1);
}

/// #1439: batch --json surfaces delete-where mutation summary (shared tx path).
#[test]
fn test_batch_json_delete_where_mutations() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("data.json"),
        r#"{"items":[{"name":"keep"},{"name":"drop"}]}"#,
    )
    .unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.delete_where data.json items name=drop\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "payload: {json}");
    assert_eq!(json["changed"], true, "payload: {json}");
    assert_eq!(json["removed"], 1, "payload: {json}");
    assert_eq!(json["mutations"][0]["op"], "doc.delete_where");
    assert_eq!(json["mutations"][0]["path"], "data.json");
    assert_eq!(json["mutations"][0]["removed"], 1);

    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("data.json")).unwrap()).unwrap();
    assert_eq!(content["items"].as_array().unwrap().len(), 1);
    assert_eq!(content["items"][0]["name"], "keep");
}

#[test]
fn test_batch_empty_quoted_string_sets_empty_value() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"old"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set data.json name \"\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains(r#""name": """#) || content.contains(r#""name":"""#),
        "empty quoted string should set value to empty string, got: {content}"
    );
}

#[test]
fn test_batch_reference_operation_count_matches_parser() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let markers = reference_markers(&reference);
    let section = reference_section(&reference, &markers, "command:batch");
    let listed = section
        .split("covers ")
        .nth(1)
        .and_then(|rest| rest.split(" operations (").next())
        .unwrap_or_else(|| panic!("batch reference section should contain an operation count"))
        .parse::<usize>()
        .unwrap();

    let actual =
        collect_batch_operation_names(&repo_root().join("src").join("cmd").join("batch.rs")).len();

    assert_eq!(
        listed, actual,
        "batch reference doc should list the same number of operations the parser supports"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_batch_ast_rename() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("mod.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() { alpha(); }\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "ast.rename mod.rs alpha gamma\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("gamma"),
        "batch ast.rename should rename: {content}"
    );
    assert!(
        !content.contains("alpha"),
        "old name should be gone: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_batch_ast_replace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("main.rs");
    fs::write(&file, "fn run() {\n    let val = 100;\n}\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "ast.replace main.rs run 100 200\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("200"),
        "batch ast.replace should replace: {content}"
    );
}

#[test]
fn test_batch_file_append() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "existing\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "file.append data.txt appended-line\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("appended-line"),
        "batch file.append should append: {content}"
    );
}

#[test]
fn test_batch_file_prepend() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "existing\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "file.prepend data.txt prepended-line\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.starts_with("prepended-line"),
        "batch file.prepend should prepend: {content}"
    );
}

#[test]
fn test_batch_multi_op_rollback_on_second_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"original"}"#).unwrap();
    // Do NOT create missing.json.

    let ops = dir.path().join("ops.txt");
    fs::write(
        &ops,
        "doc.set data.json name \"changed\"\ndoc.set missing.json version \"1.0\"\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(9); // OPERATION_FAILED

    // data.json must be rolled back to original content.
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("original"),
        "data.json should be rolled back after second op fails: {content}"
    );
}

#[test]
fn test_batch_unterminated_quote_fails() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad_quote.txt");
    fs::write(&ops, "doc.set file.json key \"unclosed value\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("unterminated"));
}

// ---------------------------------------------------------------------------
// --contain on batch (delegates to tx execute_and_collect) (MPI cycle 14)
// ---------------------------------------------------------------------------

#[test]
fn test_batch_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-batch-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    let _ = fs::remove_file(&outside);

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "batch", "--apply"])
        .write_stdin(format!("file.create ../{escape_name} \"nope\"\n"))
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert!(!outside.exists(), "batch --contain must not write outside");
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_batch_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "batch", "--apply"])
        .write_stdin("file.create inside-batch.txt \"ok\"\n")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("inside-batch.txt")).unwrap(),
        "ok"
    );
}
