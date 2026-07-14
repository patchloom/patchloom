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

/// Batch `md.insert_after_section` places a sibling after the full body (#1726).
#[test]
fn test_batch_md_insert_after_section() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("readme.md"),
        "## Config\n\nSettings.\n\n## Usage\n\nRun it.\n",
    )
    .unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(
        &ops,
        "md.insert_after_section readme.md \"## Config\" \"## FAQ\"\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("readme.md")).unwrap();
    let settings = content.find("Settings.").unwrap();
    let faq = content.find("## FAQ").unwrap();
    let usage = content.find("## Usage").unwrap();
    assert!(
        settings < faq && faq < usage,
        "batch insert_after_section must keep Config body before FAQ:\n{content}"
    );
}

/// Batch replace optional flags (#1724).
#[test]
fn test_batch_replace_fuzzy_min_score() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello wrold\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello wrold\n").unwrap();

    let ops_high = dir.path().join("ops_high.txt");
    fs::write(
        &ops_high,
        "replace a.txt \"hello world\" \"hello earth\" --fuzzy --min-fuzzy-score 0.99\n",
    )
    .unwrap();
    // High floor rejects this typo (~0.93 score); file must stay unchanged.
    let high = patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops_high)
        .arg("--apply")
        .output()
        .unwrap();
    assert_ne!(
        high.status.code(),
        Some(0),
        "high min-fuzzy-score should not succeed; stderr={}",
        String::from_utf8_lossy(&high.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "hello wrold\n"
    );

    let ops_low = dir.path().join("ops_low.txt");
    fs::write(
        &ops_low,
        "replace b.txt \"hello world\" \"hello earth\" --fuzzy --min-fuzzy-score 0.5\n",
    )
    .unwrap();
    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops_low)
        .arg("--apply")
        .assert()
        .code(0);
    let b = fs::read_to_string(dir.path().join("b.txt")).unwrap();
    assert!(b.contains("earth"), "low floor should apply fuzzy: {b}");
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
        .code(4) // PARSE_ERROR
        .stderr(predicates::str::contains("unknown operation"));
}

/// CLI path for agent-facing batch parse hints (#1483): bare leaf → namespaced op.
#[test]
fn test_batch_bare_create_suggests_file_create() {
    let dir = TempDir::new().unwrap();
    patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("create new.txt \"hi\"\n")
        .assert()
        .code(4) // PARSE_ERROR
        .stderr(predicates::str::contains("did you mean: file.create?"));
}

/// Typo of a known op should offer fuzzy neighbors.
#[test]
fn test_batch_typo_file_create_suggests_neighbors() {
    let dir = TempDir::new().unwrap();
    patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("file.creat new.txt \"hi\"\n")
        .assert()
        .code(4) // PARSE_ERROR
        .stderr(
            predicates::str::contains("did you mean").and(predicates::str::contains("file.create")),
        );
}

/// CLI-only ops are not batch ops; redirect to standalone commands.
#[test]
fn test_batch_cli_only_read_redirects_to_standalone() {
    let dir = TempDir::new().unwrap();
    patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("read file.txt\n")
        .assert()
        .code(4) // PARSE_ERROR
        .stderr(
            predicates::str::contains("not supported in batch")
                .and(predicates::str::contains("patchloom read")),
        );
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
        .code(4) // PARSE_ERROR
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
        .code(1); // not_found (missing target)
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

/// Relative batch input path is resolved under --cwd (parity with `tx` plan files).
#[test]
fn test_batch_relative_input_respects_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"key":"old"}"#).unwrap();
    fs::write(
        dir.path().join("ops.txt"),
        "doc.set data.json key \"new\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg("ops.txt")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains("new"),
        "batch input under --cwd must be found and applied: {content}"
    );
}

/// Under --contain, the batch ops *file path* must stay in the workspace (meta-input).
#[test]
fn test_batch_contain_rejects_ops_file_parent_escape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"key":"old"}"#).unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-batch-ops-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "doc.set data.json key \"new\"\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "--contain",
            "batch",
            &format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicates::str::contains("escapes")
                .or(predicates::str::contains("rejected"))
                .or(predicates::str::contains("workspace guard")),
        );

    // Must not have applied the outside ops file.
    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains("old"),
        "contain must block reading outside ops file: {content}"
    );
    let _ = fs::remove_file(&outside);
}

/// Absolute batch ops path under --cwd is allowed with --contain (AllowIfContained).
#[test]
fn test_batch_contain_allows_absolute_ops_path_inside_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"key":"old"}"#).unwrap();
    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set data.json key \"new\"\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .args(["--contain", "batch"])
        .arg(ops.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains("new"),
        "absolute ops path under --cwd must apply under --contain: {content}"
    );
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
fn test_batch_ast_rewrite_signature() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn process(x: i32) {}\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(
        &ops,
        r#"ast.rewrite_signature lib.rs process "(x: u64)" "-> u64"
"#,
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("u64"),
        "batch rewrite_signature should update types: {content}"
    );
    // #1503: structured batch path must keep space before body brace.
    assert!(
        content.contains("-> u64 {"),
        "batch rewrite_signature must preserve body gap: {content}"
    );
    assert!(
        !content.contains("u64{"),
        "must not glue type to brace: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_batch_ast_rewrite_signature_missing_exits_3() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn keep() {}\n").unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(
        &ops,
        r#"ast.rewrite_signature lib.rs missing_fn "(x: i32)"
"#,
    )
    .unwrap();

    let out = patchloom_in(dir.path())
        .arg("--json")
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["error_kind"], "no_matches");
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
        .code(1); // not_found on second op

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
        .code(4) // PARSE_ERROR
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

#[test]
fn test_batch_json_unknown_op_sets_parse_error_kind() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad.txt");
    fs::write(&ops, "unknown.op foo bar\n").unwrap();
    let output = patchloom_in(dir.path())
        .args(["--json", "batch"])
        .arg(&ops)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "parse_error",
        "batch parse failure should set error_kind: {json}"
    );
}
