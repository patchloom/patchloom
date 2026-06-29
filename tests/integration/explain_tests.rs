use super::*;

#[test]
fn test_explain_prints_human_summary() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{
            "version": 1,
            "strict": true,
            "operations": [
                {"op": "file.create", "path": "test.txt", "content": "hi"},
                {"op": "file.delete", "path": "old.txt"}
            ],
            "validate": [{"cmd": "echo ok", "required": true}]
        }"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains("2 operation(s) (strict mode)"))
        .stdout(predicates::str::contains("Create file test.txt"))
        .stdout(predicates::str::contains("Delete file old.txt"))
        .stdout(predicates::str::contains("Validate: echo ok (required)"));
}

#[test]
fn test_explain_json_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "replace", "old": "a", "new": "b"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--json"])
        .arg(&plan)
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["operation_count"], 1);
    assert_eq!(json["strict"], true);
    assert!(json["has_write_policy"].is_boolean());
    assert_eq!(json["format_steps"], 0);
    assert_eq!(json["validate_steps"], 0);
    let ops = json["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["index"], 1);
    assert!(ops[0]["description"].as_str().unwrap().contains("Replace"));
}

#[test]
fn test_explain_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "file.delete", "path": "x.txt"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--jsonl"])
        .arg(&plan)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["operation_count"], 1);
    assert_eq!(json["operations"][0]["description"], "Delete file x.txt");
}

#[test]
fn test_explain_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "file.delete", "path": "x.txt"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "explain"])
        .arg(&plan)
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
fn test_explain_invalid_plan_fails() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("bad.json");
    fs::write(&plan, "not valid json at all").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .failure()
        .stderr(predicate::str::contains("expected ident").or(predicate::str::contains("parse")));
}

#[test]
fn test_explain_stdin() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--stdin"])
        .write_stdin(r#"{"version": 1, "operations": [{"op": "file.delete", "path": "x.txt"}]}"#)
        .assert()
        .success()
        .stdout(predicates::str::contains("Delete file x.txt"));
}

#[test]
fn test_explain_stdin_takes_precedence_over_path() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "file.delete", "path": "from-file.txt"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--stdin"])
        .arg(&plan)
        .write_stdin(
            r#"{"version": 1, "operations": [{"op": "file.delete", "path": "from-stdin.txt"}]}"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains("Delete file from-stdin.txt"))
        .stdout(predicates::str::contains("from-file.txt").not());
}

// ── undo ─────────────────────────────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_explain_ast_rename_description() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "ast.rename", "path": "lib.rs", "old_name": "foo", "new_name": "bar"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "AST rename \"foo\" to \"bar\" in lib.rs",
        ));
}

#[test]
#[cfg(feature = "ast")]
fn test_explain_ast_replace_description() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "ast.replace", "path": "app.rs", "symbol": "main", "old": "old", "new": "new"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "AST replace \"old\" with \"new\" in main in app.rs",
        ));
}

// --- AST subcommand integration coverage (for #694) ---

#[test]
fn test_explain_replace_word_boundary() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "replace", "path": "f.txt", "old": "cat", "new": "dog", "word_boundary": true}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains("word-boundary"));
}

#[test]
fn test_explain_file_append_description() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "file.append", "path": "log.txt", "content": "extra"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains("Append"));
}

#[test]
fn test_explain_file_prepend_description() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": 1, "operations": [{"op": "file.prepend", "path": "main.rs", "content": "// license"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains("Prepend"));
}

// ── --format flag on write commands ────────────────────────────
