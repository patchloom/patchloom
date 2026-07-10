use super::*;

#[test]
fn test_search_finds_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"));
}

#[test]
fn test_search_no_matches_exit_3() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("nonexistent_xyz")
        .arg(dir.path())
        .assert()
        .code(3);
}

#[test]
fn test_search_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "some content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("nonexistent_xyz")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no matches"),
        "stderr should contain 'no matches' but was: {stderr}"
    );
}

#[test]
fn test_search_no_match_quiet_suppresses_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "some content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("search")
        .arg("nonexistent_xyz")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("no matches"),
        "stderr should be suppressed with --quiet but was: {stderr}"
    );
}

#[test]
fn test_search_no_match_files_from_mentions_files_from_not_dot() {
    // When the file list is --files-from, "no matches in ." is wrong: search never
    // walked the workspace root. Scope description must name --files-from.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    let list = dir.path().join("list.txt");
    fs::write(&list, "missing.txt\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("list.txt")
        .arg("search")
        .arg("hello")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no matches") && stderr.contains("--files-from"),
        "stderr should mention --files-from, not bare '.', got: {stderr}"
    );
    assert!(
        !stderr.contains("in ."),
        "must not claim workspace root walk when using --files-from: {stderr}"
    );
}

#[test]
fn test_search_empty_files_from_does_not_walk_workspace() {
    // Empty --files-from must not fall back to walking `.` (would match a.txt).
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "unique_token_xyz\n").unwrap();
    fs::write(dir.path().join("empty.txt"), "").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("empty.txt")
        .arg("search")
        .arg("unique_token_xyz")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "empty files-from must yield no matches, not walk workspace"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("unique_token_xyz"),
        "must not search workspace files when list is empty: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--files-from"),
        "no-match message should mention --files-from: {stderr}"
    );
}

#[test]
fn test_search_jsonl_output_has_path_field() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path field must be a string");
    assert!(
        parsed["path"].as_str().unwrap().contains("hello.txt"),
        "path should contain filename"
    );
    assert!(parsed["line"].is_number(), "line field must be a number");
}

#[test]
fn test_search_json_output_reports_line_and_column_without_context() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("positions.txt"),
        "skip\nalpha needle\nneedle suffix\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("needle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let matches = parsed["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["line"], 2);
    assert_eq!(matches[0]["column"], 7);
    assert_eq!(matches[1]["line"], 3);
    assert_eq!(matches[1]["column"], 1);
}

#[test]
fn test_search_jsonl_count_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "aaa\naaa\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--count")
        .arg("aaa")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path must be a string");
    assert_eq!(parsed["count"], 2, "should find 2 matches");
}

#[test]
fn test_search_jsonl_files_with_matches_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("match.txt"), "needle\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--files-with-matches")
        .arg("needle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path must be a string");
    assert!(
        parsed["path"].as_str().unwrap().contains("match.txt"),
        "should contain filename"
    );
}

#[test]
fn test_search_json_no_match_emits_valid_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("zzz_no_match_zzz")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["match_count"], 0);
    assert_eq!(parsed["file_count"], 0);
    assert!(parsed["matches"].as_array().unwrap().is_empty());
    assert_eq!(
        parsed["error_kind"], "no_matches",
        "search --json no-match should set error_kind: {parsed}"
    );
}

#[test]
fn test_search_assert_count_exact_match() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    // 3 matching lines total: 2 in a.txt + 1 in b.txt
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .assert()
        .code(0);
}

#[test]
fn test_search_assert_count_mismatch() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .assert()
        .code(2);
}

#[test]
fn test_search_assert_count_zero() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "no match here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("0")
        .arg("zzz")
        .arg(dir.path())
        .assert()
        .code(0);
}

#[test]
fn test_search_assert_count_zero_but_found() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("0")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(2);
}

#[test]
fn test_search_assert_count_json_success_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["assert_count"]["expected"], 3);
    assert_eq!(json["assert_count"]["actual"], 3);
    assert_eq!(json["assert_count"]["matched"], true);
}

#[test]
fn test_search_assert_count_json_mismatch_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["assert_count"]["expected"], 5);
    assert_eq!(json["assert_count"]["actual"], 2);
    assert_eq!(json["assert_count"]["matched"], false);
}

#[test]
fn test_search_assert_count_jsonl_success_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["assert_count"]["expected"], 3);
    assert_eq!(json["assert_count"]["actual"], 3);
    assert_eq!(json["assert_count"]["matched"], true);
}

#[test]
fn test_search_assert_count_jsonl_mismatch_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["assert_count"]["expected"], 5);
    assert_eq!(json["assert_count"]["actual"], 2);
    assert_eq!(json["assert_count"]["matched"], false);
}

#[test]
fn test_search_multiline_spans_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg(r"fn main\(\).*\}")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fn main()"))
        .stdout(predicate::str::contains("println!"));
}

#[test]
fn test_search_multiline_files_with_matches_and_assert_count_counts_all_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.txt");
    fs::write(&file, "foo\nbar\nfoo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg("--files-with-matches")
        .arg("--assert-count")
        .arg("2")
        .arg("foo")
        .arg(&file)
        .assert()
        .code(0);
}

// ---------------------------------------------------------------------------
// replace
// ---------------------------------------------------------------------------

#[test]
fn test_search_json_quiet_still_emits_structured_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("search_quiet_json.txt");
    fs::write(&file, "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--quiet")
        .arg("search")
        .arg("hello")
        .arg(&file)
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["match_count"], 1);
    assert_eq!(parsed["file_count"], 1);
}

#[test]
fn test_search_count_returns_success_on_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("count_exit.txt");
    fs::write(&file, "hello world\nhello again\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("count_exit.txt:2"));
}

#[test]
fn test_search_files_with_matches_returns_success_on_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("fwm_exit.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("fwm_exit.txt"));
}

#[test]
fn test_search_count_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("count_no_match.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("zzz_no_match_zzz")
        .arg(&file)
        .assert()
        .code(3);
}

#[test]
fn test_search_files_with_matches_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("fwm_no_match.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--files-with-matches")
        .arg("zzz_no_match_zzz")
        .arg(&file)
        .assert()
        .code(3);
}

#[test]
fn test_search_nonexistent_path_warns_on_stderr() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg("totally_nonexistent_dir/")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("totally_nonexistent_dir"));
}

#[test]
fn test_search_invert_match_multiline_rejected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg("-v")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--invert-match and --multiline"));

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "search", "--multiline", "-v", "hello"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "invalid_input",
        "search dual flags should set error_kind: {parsed}"
    );
}

#[test]
fn test_search_json_empty_pattern_sets_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "search", "", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(
        parsed["error_kind"], "invalid_input",
        "empty search pattern should set error_kind: {parsed}"
    );
}

#[test]
fn test_search_count_and_files_with_matches_are_rejected_together() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot be used with")
                .and(predicate::str::contains("--count"))
                .and(predicate::str::contains("--files-with-matches")),
        );
}

#[test]
fn test_search_literal_and_regex_are_rejected_together() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--regex")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot be used with")
                .and(predicate::str::contains("--literal"))
                .and(predicate::str::contains("--regex")),
        );
}

// ---------------------------------------------------------------------------
// replace: invalid regex
// ---------------------------------------------------------------------------

#[test]
fn test_search_context_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nmatch_me\nline4\nline5\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-C")
        .arg("1")
        .arg("match_me")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("line2"))
        .stdout(predicate::str::contains("match_me"))
        .stdout(predicate::str::contains("line4"));
}

#[test]
fn test_search_before_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Use distinctive markers: plain "ddd" can appear in tempfile path names.
    fs::write(&file, "aaa\nbbb\nccc\ntarget\nAFTER_CTX_MARKER\neee\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-B")
        .arg("2")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("bbb"))
        .stdout(predicate::str::contains("ccc"))
        .stdout(predicate::str::contains("target"))
        // no after-context lines
        .stdout(predicate::str::contains("AFTER_CTX_MARKER").not());
}

#[test]
fn test_search_after_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Distinctive markers: short tokens like "bbb" can appear in tempfile paths.
    fs::write(
        &file,
        "BEFORE_CTX_MARKER\nmid\ntarget\nAFTER1_MARKER\nAFTER2_MARKER\ntail\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-A")
        .arg("2")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("AFTER1_MARKER"))
        .stdout(predicate::str::contains("AFTER2_MARKER"))
        // no before-context lines
        .stdout(predicate::str::contains("BEFORE_CTX_MARKER").not());
}

#[test]
fn test_search_asymmetric_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(
        &file,
        "far_before\nBEFORE2_MARKER\nNEAR_BEFORE_MARKER\ntarget\nA1_MARKER\nA2_MARKER\nA3_MARKER\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-B")
        .arg("1")
        .arg("-A")
        .arg("3")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("NEAR_BEFORE_MARKER"))
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("A1_MARKER"))
        .stdout(predicate::str::contains("A2_MARKER"))
        .stdout(predicate::str::contains("A3_MARKER"))
        // BEFORE2 is 2 lines before, should not appear with -B 1
        .stdout(predicate::str::contains("BEFORE2_MARKER").not());
}

/// Adjacent matches with context should NOT have `--` separators (#1111).
#[test]
fn test_search_context_no_separator_for_adjacent_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Lines 1-6; matches on lines 2 and 3 are adjacent.
    fs::write(&file, "aaa\nmatch1\nmatch2\nbbb\nccc\nmatch3\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-C")
        .arg("1")
        .arg("match")
        .arg(&file)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Between match1 (line 2) and match2 (line 3): adjacent, no separator.
    // Between match2 (line 3, after-ctx=line 4) and match3 (line 6, before-ctx=line 5):
    // line 5 is one past line 4, so they are contiguous -- no separator either
    // with -C 1.
    let separator_count = stdout.matches("\n--\n").count();
    assert_eq!(
        separator_count, 0,
        "adjacent/contiguous matches should not have separators: {stdout}"
    );
}

/// Non-adjacent matches with context SHOULD have `--` separators.
#[test]
fn test_search_context_separator_for_distant_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Matches on lines 1 and 7 with 5 lines gap.
    fs::write(
        &file,
        "match_a\nfiller1\nfiller2\nfiller3\nfiller4\nfiller5\nmatch_b\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-C")
        .arg("1")
        .arg("match_")
        .arg(&file)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\n--\n"),
        "distant matches should have a line-delimited separator: {stdout}"
    );
}

#[test]
fn test_search_literal_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // dot in "foo.bar" should NOT match "fooXbar" with --literal
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("foo.bar")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("foo.bar"))
        .stdout(predicate::str::contains("fooXbar").not());
}

// ---------------------------------------------------------------------------
// search short aliases (-F, -l, -c)
// ---------------------------------------------------------------------------

#[test]
fn test_search_short_alias_literal() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-F")
        .arg("foo.bar")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("foo.bar"))
        .stdout(predicate::str::contains("fooXbar").not());
}

#[test]
fn test_search_short_alias_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\nhello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-c")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("test.txt:2"));
}

#[test]
fn test_search_short_alias_files_with_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-l")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));
}

// ---------------------------------------------------------------------------
// create --force
// ---------------------------------------------------------------------------

#[test]
fn test_search_invert_match_normal() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "keep\nremove\nkeep2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-v")
        .arg("remove")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("keep"))
        .stdout(predicate::str::contains("remove").not());
}

// ---------------------------------------------------------------------------
// tx: file.create, file.delete, write_policy, validate
// ---------------------------------------------------------------------------

#[test]
fn test_search_case_insensitive() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "Hello World\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-i")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello World"));
}

#[test]
fn test_search_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "needle in text\n").unwrap();
    // Binary file: contains NUL byte.
    let bin_file = dir.path().join("data.bin");
    fs::write(&bin_file, b"needle\x00in binary").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("needle")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("text.txt"),
        "should find match in text file"
    );
    assert!(
        !stdout.contains("data.bin"),
        "should skip binary file, got: {stdout}"
    );
}

#[test]
fn test_search_skips_invalid_utf8_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "needle in text\n").unwrap();

    let invalid_file = dir.path().join("invalid.txt");
    fs::write(&invalid_file, b"needle\xffin invalid").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("needle")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("text.txt"),
        "should find match in text file"
    );
    assert!(
        !stdout.contains("invalid.txt"),
        "should skip invalid UTF-8 file, got: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid.txt (invalid UTF-8)"),
        "expected search invalid UTF-8 warning, got: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn test_search_follows_symlink_within_cwd() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("target.txt"), "needle in haystack\n").unwrap();
    std::os::unix::fs::symlink(&sub, dir.path().join("link_dir")).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("needle")
        .arg(dir.path().join("link_dir"))
        .assert()
        .success()
        .stdout(predicates::str::contains("needle"));
}

#[test]
fn test_search_skips_patchloom_backup_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("real.txt"), "needle in real file\n").unwrap();
    fs::create_dir_all(dir.path().join(".patchloom/backups/12345")).unwrap();
    fs::write(
        dir.path().join(".patchloom/backups/12345/manifest.json"),
        "needle in backup\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("needle")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("real.txt"))
        .stdout(predicates::str::contains("manifest.json").not());
}

#[test]
fn test_search_jsonl_no_match_emits_valid_jsonl() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("zzz_no_match_zzz")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "--jsonl no-match should emit a JSON line, got empty stdout"
    );
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["match_count"], 0);
    assert_eq!(parsed["file_count"], 0);
    assert!(parsed["matches"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// --contain on search (read-side sandbox) (MPI cycle 18)
// ---------------------------------------------------------------------------

#[test]
fn test_search_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-search-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "SECRET_VALUE=x\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "search",
            "SECRET_VALUE",
            &format!("../{escape_name}"),
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
fn test_search_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "findme here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "search", "findme", "inside.txt"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("findme"));
}

#[test]
fn test_search_contain_rejects_files_from_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-files-from-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "SECRET_VALUE=x\n").unwrap();
    // List path can be relative under --cwd; entries inside are also relative
    // to --cwd. Use a relative list name so the test exercises resolve_user_path.
    fs::write(dir.path().join("list.txt"), format!("../{escape_name}\n")).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .arg("--contain")
        .arg("--files-from")
        .arg("list.txt")
        .args(["search", "SECRET_VALUE"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    let _ = fs::remove_file(&outside);
}
