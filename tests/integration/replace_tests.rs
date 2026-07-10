use super::*;

#[test]
fn test_replace_json_no_match_emits_valid_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("zzz_no_match_zzz")
        .arg("--new")
        .arg("replacement")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["match_count"], 0);
    assert_eq!(parsed["file_count"], 0);
    assert!(parsed["files"].as_array().unwrap().is_empty());
}

#[test]
fn test_replace_context_json_no_match_emits_ok_false() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("nonexistent")
        .arg("--new")
        .arg("replacement")
        .arg("--before-context")
        .arg("hello")
        .arg(dir.path().join("f.txt"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["match_count"], 0);
}

#[test]
fn test_replace_apply_modifies_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old_text content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old_text")
        .arg("--new")
        .arg("new_text")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new_text"), "file should contain new_text");
    assert!(
        !content.contains("old_text"),
        "file should not contain old_text"
    );
}

#[test]
fn test_replace_dry_run_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old_text content\n").unwrap();

    // Without --apply, patchloom should show a diff but NOT modify the file.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old_text")
        .arg("--new")
        .arg("new_text")
        .arg(&file)
        .assert()
        .code(2); // CHANGES_DETECTED: preview shows changes, not applied

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("old_text"),
        "file should be unchanged in dry-run mode"
    );
    assert!(
        !content.contains("new_text"),
        "file should not be modified in dry-run mode"
    );
}

#[test]
fn test_replace_if_exists_no_match_exit_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent")
        .arg("--new")
        .arg("new")
        .arg("--if-exists")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);
}

// ---------------------------------------------------------------------------
// --quiet flag
// ---------------------------------------------------------------------------

#[test]
fn test_replace_empty_from_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("")
        .arg("--new")
        .arg("X")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("search pattern must not be empty"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_replace_invalid_regex_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg("[invalid(regex")
        .arg("--new")
        .arg("x")
        .arg(&file)
        .assert()
        .failure()
        .stderr(predicate::str::contains("regex parse error"));
}

// ---------------------------------------------------------------------------
// replace --multiline
// ---------------------------------------------------------------------------

#[test]
fn test_replace_multiline_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg("--multiline")
        .arg(r"fn main\(\) \{.*\}")
        .arg("--new")
        .arg("fn main() { /* replaced */ }")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("/* replaced */"),
        "multiline replace should span newlines"
    );
}

// ---------------------------------------------------------------------------
// replace --whole-line, --range, --collapse-blanks
// ---------------------------------------------------------------------------

#[test]
fn test_replace_whole_line_deletes_matching_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(
        &file,
        "fn main() {\n    let _x = foo();\n    let y = bar();\n    let _z = baz();\n}\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("let _")
        .arg("--whole-line")
        .arg("--new")
        .arg("")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn main() {\n    let y = bar();\n}\n"
    );
}

#[test]
fn test_replace_whole_line_with_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\nbbb\neee\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("bbb")
        .arg("--whole-line")
        .arg("--range")
        .arg("1:3")
        .arg("--new")
        .arg("")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nccc\nbbb\neee\n");
}

#[test]
fn test_replace_collapse_blanks_after_whole_line_delete() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    // Two blank lines surround "remove"; deleting it leaves consecutive blanks.
    fs::write(&file, "keep\n\nremove\n\nalso keep\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("remove")
        .arg("--whole-line")
        .arg("--new")
        .arg("")
        .arg("--collapse-blanks")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "keep\n\nalso keep\n");
}

#[test]
fn test_replace_range_requires_whole_line() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--range")
        .arg("1:5")
        .arg("--new")
        .arg("hi")
        .arg(file.to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("range requires whole_line"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_replace_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("hi")
        .arg(&file)
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello world\n",
        "file should be unchanged in --check mode"
    );
}

#[test]
fn test_replace_json_check_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file.txt"), "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("bye")
        .arg("--check")
        .arg(dir.path())
        .assert()
        .code(2); // CHANGES_DETECTED

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["files"].is_array(), "files should list affected paths");
}

#[test]
fn test_replace_jsonl_check_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello again\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("bye")
        .arg("--check")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(
        lines.len(),
        2,
        "should have one JSONL line per matched file"
    );
    for line in &lines {
        assert!(line["path"].is_string());
        assert!(line["match_count"].as_u64().unwrap() >= 1);
    }
}

// ---------------------------------------------------------------------------
// CLI flags
// ---------------------------------------------------------------------------

#[test]
fn test_replace_no_match_without_if_exists_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent_pattern_xyz")
        .arg("--new")
        .arg("new")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(3);
}

#[test]
fn test_replace_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent_xyz")
        .arg("--new")
        .arg("new")
        .arg(file.to_str().unwrap())
        .arg("--apply")
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
fn test_replace_no_match_quiet_suppresses_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("replace")
        .arg("nonexistent_xyz")
        .arg("--new")
        .arg("new")
        .arg(file.to_str().unwrap())
        .arg("--apply")
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
fn test_replace_no_match_files_from_mentions_files_from_not_dot() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("list.txt"), "missing.txt\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("list.txt")
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("bye")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no matches") && stderr.contains("--files-from"),
        "stderr should mention --files-from: {stderr}"
    );
    assert!(
        !stderr.contains("in ."),
        "must not claim workspace root when using --files-from: {stderr}"
    );
}

#[test]
fn test_replace_empty_files_from_does_not_mutate_workspace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "unique_replace_token\n").unwrap();
    fs::write(dir.path().join("empty.txt"), "").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("empty.txt")
        .arg("replace")
        .arg("unique_replace_token")
        .arg("--new")
        .arg("CHANGED")
        .arg("--apply")
        .assert()
        .code(3);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "unique_replace_token\n",
        "empty files-from must not fall back to walking workspace"
    );
}

#[test]
fn test_replace_ambiguous_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa bbb aaa\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("aaa")
        .arg("--new")
        .arg("ccc")
        .arg("--unique")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5)); // AMBIGUOUS
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ambiguous match"),
        "stderr should contain 'ambiguous match' but was: {stderr}"
    );
}

#[test]
fn test_replace_ambiguous_match_quiet_suppresses_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa bbb aaa\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("replace")
        .arg("aaa")
        .arg("--new")
        .arg("ccc")
        .arg("--unique")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5)); // AMBIGUOUS
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("ambiguous match"),
        "stderr should be suppressed with --quiet but was: {stderr}"
    );
}

#[test]
fn test_replace_context_no_match_text_mode_emits_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line one\nline two\nline three\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent_xyz")
        .arg("--new")
        .arg("replaced")
        .arg("--before-context")
        .arg("line one")
        .arg(file.to_str().unwrap())
        .arg("--apply")
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
fn test_replace_normalize_eol_lf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "old\r\ncontent\r\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old")
        .arg("--new")
        .arg("new")
        .arg("--normalize-eol")
        .arg("lf")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read(&file).unwrap();
    assert!(
        !content.windows(2).any(|w| w == b"\r\n"),
        "CRLF should be normalized to LF"
    );
    assert!(
        content.windows(3).any(|w| w == b"new"),
        "replacement should be applied"
    );
}

#[test]
fn test_replace_directory_modifies_all_matching_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello again\n").unwrap();
    fs::write(dir.path().join("c.txt"), "no match here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("bye")
        .arg(dir.path())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "bye world\n",
        "a.txt should have hello replaced with bye"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "bye again\n",
        "b.txt should have hello replaced with bye"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("c.txt")).unwrap(),
        "no match here\n",
        "file without match should be untouched"
    );
}

// ---------------------------------------------------------------------------
// --files-from error on bad path
// ---------------------------------------------------------------------------

#[test]
fn test_replace_nth_replaces_only_nth_occurrence() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar foo baz foo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("foo")
        .arg("--new")
        .arg("REPLACED")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "foo bar REPLACED baz foo\n");
}

#[test]
fn test_replace_nth_zero_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("hi")
        .arg("--nth")
        .arg("0")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("1-based"));

    // File must be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello world\n");
}

#[test]
fn test_replace_nth_no_match_when_out_of_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("foo")
        .arg("--new")
        .arg("REPLACED")
        .arg("--nth")
        .arg("5")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(3); // NO_MATCHES

    // File unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "foo bar\n");
}

#[test]
fn test_replace_insert_before() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "    /// Doc comment.\n    pub field: bool,\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("    /// Doc comment.")
        .arg("--insert-before")
        .arg("    // marker\n")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "    // marker\n    /// Doc comment.\n    pub field: bool,\n"
    );
}

#[test]
fn test_replace_insert_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "line1\nanchor\nline3\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("anchor")
        .arg("--insert-after")
        .arg(" // tagged")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nanchor // tagged\nline3\n"
    );
}

#[test]
fn test_replace_insert_before_with_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("b+")
        .arg("--regex")
        .arg("--insert-before")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nXbbb\nccc\n");
}

#[test]
fn test_replace_insert_after_with_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("b+")
        .arg("--regex")
        .arg("--insert-after")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nbbbX\nccc\n");
}

#[test]
fn test_replace_insert_before_nth() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "x a x a x\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("a")
        .arg("--insert-before")
        .arg("[")
        .arg("--nth")
        .arg("2")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "x a x [a x\n");
}

#[test]
fn test_replace_insert_after_nth() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "x a x a x\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("a")
        .arg("--insert-after")
        .arg("]")
        .arg("--nth")
        .arg("2")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "x a x a] x\n");
}

#[test]
fn test_replace_insert_before_and_to_conflict() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("world")
        .arg("--insert-before")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "'to' cannot be combined with 'insert_before' or 'insert_after'",
        ));
}

#[test]
fn test_replace_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Hello HELLO hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("HI")
        .arg("-i")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "HI HI HI\n");
}

#[test]
fn test_replace_nth_regex_replaces_only_nth() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "v1.0 and v2.0 and v3.0\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg(r"v\d+\.\d+")
        .arg("--new")
        .arg("vX.Y")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "v1.0 and vX.Y and v3.0\n");
}

#[test]
fn test_replace_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "old value\n").unwrap();
    // Binary file with the same target text.
    let bin_file = dir.path().join("data.bin");
    let mut bin_content = b"old value".to_vec();
    bin_content.push(0);
    bin_content.extend_from_slice(b" more data");
    fs::write(&bin_file, &bin_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old value")
        .arg("--new")
        .arg("new value")
        .arg(dir.path().to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "new value\n");

    // Binary file should be untouched.
    let bin_after = fs::read(&bin_file).unwrap();
    assert_eq!(bin_after, bin_content, "binary file should not be modified");
}

#[test]
fn test_replace_skips_invalid_utf8_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "old value\n").unwrap();

    let invalid_file = dir.path().join("invalid.txt");
    let invalid_content = b"old value\xff trailing".to_vec();
    fs::write(&invalid_file, &invalid_content).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old value")
        .arg("--new")
        .arg("new value")
        .arg(dir.path().to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "new value\n");

    let invalid_after = fs::read(&invalid_file).unwrap();
    assert_eq!(
        invalid_after, invalid_content,
        "invalid UTF-8 file should not be modified"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid.txt (invalid UTF-8)"),
        "expected replace invalid UTF-8 warning, got: {stderr}"
    );
}

#[test]
fn test_replace_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("goodbye")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    // A backup session directory should exist under .patchloom/backups/.
    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(backup_dir.exists(), "backup directory should be created");

    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(sessions.len(), 1, "exactly one backup session expected");

    // The session should contain a manifest.json.
    let manifest = sessions[0].path().join("manifest.json");
    assert!(manifest.exists(), "manifest.json should exist in session");
}

#[test]
fn test_replace_apply_prunes_old_backup_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\n").unwrap();

    // Create a fake old backup session with a timestamp 8 days in the past.
    // Pruning now uses the directory name (nanos_seq) to determine age.
    let eight_days_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        - std::time::Duration::from_secs(8 * 24 * 60 * 60);
    let old_ts = format!("{}_0", eight_days_ago.as_nanos());
    let old_session = dir.path().join(format!(".patchloom/backups/{old_ts}"));
    fs::create_dir_all(&old_session).unwrap();
    fs::write(
        old_session.join("manifest.json"),
        r#"{"timestamp":"old","entries":[]}"#,
    )
    .unwrap();

    // Now run a replace --apply, which creates a new session and prunes old ones.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("aaa")
        .arg("--new")
        .arg("bbb")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    let backup_dir = dir.path().join(".patchloom/backups");
    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    // The old session should have been pruned; only the new one remains.
    assert_eq!(
        sessions.len(),
        1,
        "old session should be pruned, only the new one remains"
    );
    assert!(
        !old_session.exists(),
        "the old_session directory should be removed"
    );
}

#[test]
fn test_replace_apply_keeps_recent_backup_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "first\n").unwrap();

    // First apply creates a backup.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("first")
        .arg("--new")
        .arg("second")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    // Small delay to ensure different timestamps.
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Second apply creates another backup. Both are recent.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("second")
        .arg("--new")
        .arg("third")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .code(0);

    let backup_dir = dir.path().join(".patchloom/backups");
    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    // Both sessions are recent; neither should be pruned.
    assert_eq!(
        sessions.len(),
        2,
        "both recent backup sessions should be kept"
    );
}

#[cfg(unix)]
#[test]
fn test_replace_follows_symlink_within_cwd() {
    let dir = TempDir::new().unwrap();
    let real_file = dir.path().join("real.txt");
    fs::write(&real_file, "hello world\n").unwrap();
    let link = dir.path().join("link.txt");
    std::os::unix::fs::symlink(&real_file, &link).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("goodbye")
        .arg("--apply")
        .arg(&link)
        .assert()
        .code(0);

    // atomic_write replaces the symlink with a regular file (rename semantics).
    // Reading the link path should show the replacement.
    let content = fs::read_to_string(&link).unwrap();
    assert_eq!(content, "goodbye world\n");
}

#[cfg(unix)]
#[test]
fn test_replace_apply_readonly_dir_fails_gracefully() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("target.txt");
    fs::write(&file, "hello world\n").unwrap();
    if !readonly_dir_blocks_writes(&sub) {
        return;
    }

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("goodbye")
        .arg("--apply")
        .arg(&file)
        .output()
        .unwrap();

    // Should fail (exit code 1), not panic.
    assert_ne!(
        output.status.code(),
        Some(0),
        "write in readonly dir should fail"
    );
    // File should be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello world\n");

    // Restore for cleanup.
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn test_replace_format_flag_runs_after_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    patchloom_in(dir.path())
        .args([
            "replace",
            "aaa",
            "--new",
            "bbb",
            "--apply",
            "--format",
            &shell_touch(&marker),
        ])
        .assert()
        .code(0);

    assert!(
        marker.exists(),
        "--format command should have created the marker file"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "bbb\n");
}

#[test]
fn test_replace_format_flag_skipped_in_diff_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    patchloom_in(dir.path())
        .args([
            "replace",
            "aaa",
            "--new",
            "bbb",
            "--format",
            &shell_touch(&marker),
        ])
        .assert()
        .code(2); // CHANGES_DETECTED: preview mode, not applied

    assert!(
        !marker.exists(),
        "--format should NOT run in diff-only mode"
    );
}

#[test]
fn test_replace_before_context_disambiguates() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    // Two identical "port = 8080" lines; before_context picks the right one.
    fs::write(&file, "[server]\nport = 8080\n\n[database]\nport = 8080\n").unwrap();

    patchloom_in(dir.path())
        .arg("replace")
        .arg("port = 8080")
        .arg("--new")
        .arg("port = 9090")
        .arg("--before-context")
        .arg("[database]")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("[server]\nport = 8080"),
        "server port should be unchanged: {result}"
    );
    assert!(
        result.contains("[database]\nport = 9090"),
        "database port should be updated: {result}"
    );
}

#[test]
fn test_replace_after_context_disambiguates() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(
        &file,
        "name = alpha\nrole = admin\n\nname = beta\nrole = user\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("replace")
        .arg("name = alpha")
        .arg("--new")
        .arg("name = gamma")
        .arg("--after-context")
        .arg("role = admin")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("name = gamma\nrole = admin"),
        "first name should be updated: {result}"
    );
    assert!(
        result.contains("name = beta"),
        "second name should be unchanged: {result}"
    );
}

#[test]
fn test_replace_context_requires_explicit_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    // No file paths at all (only --cwd is set via patchloom_in).
    patchloom_in(dir.path())
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("world")
        .arg("--before-context")
        .arg("ctx")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("explicit file paths"));
}

#[test]
fn test_replace_context_in_tx_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("conf.ini");
    fs::write(&file, "[a]\nval = 1\n\n[b]\nval = 1\n").unwrap();

    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        format!(
            r#"{{"version":1,"operations":[{{"op":"replace","path":"{}","old":"val = 1","new":"val = 2","before_context":"[b]"}}]}}"#,
            portable_path_str(&file)
        ),
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert!(
        result.contains("[a]\nval = 1"),
        "section [a] should be unchanged: {result}"
    );
    assert!(
        result.contains("[b]\nval = 2"),
        "section [b] should be updated: {result}"
    );
}

// ---------------------------------------------------------------------------
// --contain on replace (precomputed multi-file write path) (MPI cycle 15)
// ---------------------------------------------------------------------------

#[test]
fn test_replace_contain_rejects_parent_escape() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-replace-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    fs::write(&outside, "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "replace",
            "foo",
            "--new",
            "ZAZ",
            &format!("../{escape_name}"),
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "foo bar\n",
        "replace --contain must not mutate escaped path"
    );
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_replace_contain_allows_in_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inside.txt"), "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "--contain",
            "replace",
            "foo",
            "--new",
            "ZAZ",
            "inside.txt",
            "--apply",
        ])
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("inside.txt")).unwrap(),
        "ZAZ bar\n"
    );
}

// -- command_position CLI surface (#1494 / MPI loop21) ----------------------

#[test]
fn test_replace_cli_command_position_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "sudo pip install x\nuv pip install\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "replace",
            "pip",
            "--new",
            "uv",
            "--command-position",
            "install.sh",
            "--apply",
        ])
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "sudo uv install x\nuv pip install\n"
    );
}

#[test]
fn test_replace_cli_command_position_rejects_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "sudo pip install\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "replace",
            "pip",
            "--new",
            "uv",
            "--command-position",
            "--regex",
            "install.sh",
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "command_position cannot be combined",
        ));
}

#[test]
fn test_replace_cli_require_change_no_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args([
            "replace",
            "missing",
            "--new",
            "x",
            "--require-change",
            "install.sh",
            "--apply",
        ])
        .assert()
        .code(3); // NO_MATCHES

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}
