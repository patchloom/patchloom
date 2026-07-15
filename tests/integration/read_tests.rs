use super::*;

#[test]
fn test_read_prints_file_contents() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .assert()
        .success()
        .stdout("line1\nline2\nline3\n");
}

#[test]
fn test_read_lines_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("five.txt");
    fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("2:4")
        .assert()
        .success()
        .stdout("b\nc\nd");
}

#[test]
fn test_read_nonexistent_file_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(nonexistent_path("file-xyz"))
        .assert()
        .code(1);
}

#[test]
fn test_read_invalid_lines_returns_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("0:1")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("line numbers are 1-based"));
}

#[test]
fn test_read_json_invalid_lines_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("0:1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("invalid --lines value '0:1'"));
    assert!(error.contains("line numbers are 1-based"));
    assert_eq!(
        json["error_kind"], "invalid_input",
        "read invalid --lines should set error_kind: {json}"
    );
}

/// --lines past EOF must not return ok:true with empty content (agents treat that
/// as a successful empty read).
#[test]
fn test_read_lines_beyond_eof_json_no_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("short.txt");
    fs::write(&file, "a\nb\nc\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("5-10")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(3),
        "expected NO_MATCHES, stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "no_matches");
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("line range outside file"),
        "got error: {error}"
    );
}

#[test]
fn test_read_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_lines"], 2);
    assert_eq!(json["start_line"], 1);
    assert_eq!(json["end_line"], 2);
    assert!(json["content"].as_str().unwrap().contains("hello"));
}

#[test]
fn test_read_json_lines_start_past_eof_is_no_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("short.txt");
    fs::write(&file, "a\nb\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("5:9")
        .output()
        .unwrap();

    // Past-EOF must not look like ok:true empty content (agent honesty).
    assert_eq!(output.status.code(), Some(3));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "no_matches");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("line range outside file"),
        "got: {json}"
    );
}

#[test]
fn test_read_respects_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inner.txt"), "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("read")
        .arg("inner.txt")
        .assert()
        .success()
        .stdout("content\n");
}

#[test]
fn test_read_multiple_files() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("a.txt");
    let f2 = dir.path().join("b.txt");
    fs::write(&f1, "alpha\n").unwrap();
    fs::write(&f2, "beta\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicates::str::contains("==> "))
        .stdout(predicates::str::contains("alpha"))
        .stdout(predicates::str::contains("beta"));
}

#[test]
fn test_read_multiple_files_json() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("x.txt");
    let f2 = dir.path().join("y.txt");
    fs::write(&f1, "one\n").unwrap();
    fs::write(&f2, "two\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 2);
    assert_eq!(json[0]["content"], "one\n", "first file content");
    assert_eq!(json[1]["content"], "two\n", "second file content");
}

#[test]
fn test_read_multiple_files_json_partial_failure_reports_skipped() {
    let dir = TempDir::new().unwrap();
    let existing = dir.path().join("exists.txt");
    fs::write(&existing, "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(existing.to_str().unwrap())
        .arg(nonexistent_path("no-such-file-json-array-shape"))
        .output()
        .unwrap();

    // Partial failure: FAILURE exit + envelope so agents see skipped paths
    // (not a bare success array that hides misses).
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(json["error_kind"], "not_found", "{json}");
    assert!(json["files"].is_array(), "{json}");
    assert_eq!(json["files"].as_array().unwrap().len(), 1);
    assert_eq!(json["files"][0]["path"], existing.to_str().unwrap());
    assert_eq!(json["files"][0]["content"], "hello\n");
    let skipped = json["skipped"]
        .as_array()
        .expect("partial multi-read must list skipped");
    assert_eq!(skipped.len(), 1, "{json}");
    assert!(
        skipped[0]
            .as_str()
            .is_some_and(|s| s.contains("no-such-file-json-array-shape")),
        "skipped should name missing path: {json}"
    );
}

#[test]
fn test_read_multiple_files_jsonl() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("p.txt");
    let f2 = dir.path().join("q.txt");
    fs::write(&f1, "first\n").unwrap();
    fs::write(&f2, "second\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let lines: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .trim()
        .lines()
        .collect();
    assert_eq!(lines.len(), 2);
    let j1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let j2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert!(j1["content"].as_str().unwrap().contains("first"));
    assert!(j2["content"].as_str().unwrap().contains("second"));
}

#[test]
fn test_read_partial_failure_returns_failure() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("exists.txt");
    fs::write(&f1, "hello\n").unwrap();

    // Partial failure now returns FAILURE exit code (#1166).
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(nonexistent_path("no-such-file-xyz"))
        .assert()
        .code(1)
        .stdout(predicates::str::contains("hello"));
}

#[test]
fn test_read_all_fail_returns_failure() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(nonexistent_path("no-1-xyz"))
        .arg(nonexistent_path("no-2-xyz"))
        .assert()
        .code(1);
}

#[test]
fn test_read_json_all_fail_returns_error_object() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(nonexistent_path("no-1-json-read-fail"))
        .arg(nonexistent_path("no-2-json-read-fail"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("no-1-json-read-fail"));
    assert!(error.contains("no-2-json-read-fail"));
    assert_eq!(
        json["error_kind"], "not_found",
        "read all-fail should set error_kind: {json}"
    );
}

#[test]
fn test_read_jsonl_all_fail_returns_error_object() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("read")
        .arg(nonexistent_path("no-1-jsonl-read-fail"))
        .arg(nonexistent_path("no-2-jsonl-read-fail"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL error should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|_| panic!("expected JSON line, got: {}", lines[0]));
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("no-1-jsonl-read-fail"));
    assert!(error.contains("no-2-jsonl-read-fail"));
    assert_eq!(
        json["error_kind"], "not_found",
        "read jsonl all-fail should set error_kind: {json}"
    );
}

#[test]
fn test_read_multiple_files_with_lines() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("long.txt");
    let f2 = dir.path().join("short.txt");
    fs::write(&f1, "alpha\nbravo\ncharlie\ndelta\necho\n").unwrap();
    fs::write(&f2, "xray\nyankee\n").unwrap();

    // --lines 2:4 on a 5-line file gives lines 2-4; on a 2-line file gives line 2 only
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .arg("--lines")
        .arg("2:4")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("bravo"),
        "line 2 of first file should be present"
    );
    assert!(
        stdout.contains("charlie"),
        "line 3 of first file should be present"
    );
    assert!(
        stdout.contains("delta"),
        "line 4 of first file should be present"
    );
    assert!(
        stdout.contains("yankee"),
        "line 2 of second file should be present"
    );
    // Lines outside the range should not appear
    assert!(
        !stdout.contains("alpha"),
        "line 1 of first file should be excluded"
    );
    assert!(
        !stdout.contains("echo"),
        "line 5 of first file should be excluded"
    );
    assert!(
        !stdout.contains("xray"),
        "line 1 of second file should be excluded"
    );
}

// ── status command ─────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn test_read_via_symlink_within_cwd() {
    let dir = TempDir::new().unwrap();
    let real_file = dir.path().join("data.txt");
    fs::write(&real_file, "symlinked content\n").unwrap();
    std::os::unix::fs::symlink(&real_file, dir.path().join("alias.txt")).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(dir.path().join("alias.txt"))
        .assert()
        .success()
        .stdout(predicates::str::contains("symlinked content"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_symlink_outside_cwd_rejected() {
    if !has_mcp_support() {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "do not read\n").unwrap();

    // Create a symlink inside the workspace that points outside.
    std::os::unix::fs::symlink(&secret, workspace.path().join("escape.txt")).unwrap();

    let client = spawn_mcp_client(workspace.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("read_file")
        .with_arguments(serde_json::from_value(serde_json::json!({"path": "escape.txt"})).unwrap());
    let result = client.peer().call_tool(params).await;
    // Path validation returns McpError at the protocol level (Err), not a tool
    // result with is_error=true.
    match result {
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("escapes") || msg.contains("escape"),
                "error should mention path escape: {msg}"
            );
        }
        Ok(r) => {
            assert!(
                r.is_error.unwrap_or(false),
                "reading a symlink that escapes the workspace should fail"
            );
        }
    }
    client.cancel().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_symlink_dir_outside_cwd_rejected() {
    if !has_mcp_support() {
        return;
    }
    let workspace = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret.txt"), "hidden\n").unwrap();

    // Symlink a directory inside the workspace pointing outside.
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape_dir")).unwrap();

    let client = spawn_mcp_client(workspace.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("read_file").with_arguments(
        serde_json::from_value(serde_json::json!({"path": "escape_dir/secret.txt"})).unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    match result {
        Err(e) => {
            let msg = format!("{e:?}");
            assert!(
                msg.contains("escapes") || msg.contains("escape"),
                "error should mention path escape: {msg}"
            );
        }
        Ok(r) => {
            assert!(
                r.is_error.unwrap_or(false),
                "reading through a symlinked dir that escapes should fail"
            );
        }
    }
    client.cancel().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn test_mcp_symlink_within_cwd_allowed() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("real");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("data.txt"), "safe content\n").unwrap();
    std::os::unix::fs::symlink(sub.join("data.txt"), dir.path().join("link.txt")).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, val) = call_tool_value(
        &client,
        "read_file",
        serde_json::json!({"path": "link.txt"}),
    )
    .await;
    assert!(
        !is_error,
        "symlink within workspace should be allowed: {val}"
    );
    let content = val["reads"][0]["content"]
        .as_str()
        .expect("reads[0].content should be a string");
    assert_eq!(
        content, "safe content\n",
        "symlink read should return file content"
    );
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Permission error integration tests (unix only)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// --contain on read (read-side sandbox) (MPI cycle 18)
// ---------------------------------------------------------------------------

#[test]
fn test_read_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-read-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "SECRET\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", &format!("../{escape_name}")])
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
fn test_read_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "read", "inside.txt"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn test_read_empty_path_rejected() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["read", ""])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("path must not be empty"));
}
