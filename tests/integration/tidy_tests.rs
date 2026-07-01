use super::*;

#[test]
fn test_tidy_check_detects_missing_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_tidy_fix_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "file should end with a newline after fix"
    );
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

// ── read command ────────────────────────────────────────────────────

#[test]
fn test_tidy_check_exits_2_with_issues() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_tidy_check_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing spaces   \nno final newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["issue_count"].as_u64().unwrap() >= 2);
    assert!(json["issues"].is_array());
}

#[test]
fn test_tidy_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "trailing   \n").unwrap();
    fs::write(&b, "no final newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&a)
        .arg(&b)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert!(
        lines.len() >= 2,
        "should have at least one JSONL line per issue"
    );
    for line in &lines {
        assert!(line["path"].is_string());
        assert!(line["issue"].is_string());
    }
}

#[test]
fn test_tidy_check_exits_0_when_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "clean file\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .code(0);
}

#[test]
fn test_tidy_fix_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing whitespace   ").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--ensure-final-newline")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "trailing whitespace   ",
        "file should be unchanged in --check mode"
    );
}

#[test]
fn test_tidy_fix_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing   \nno final newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "tidy",
            "fix",
            ".",
            "--ensure-final-newline",
            "--trim-trailing-whitespace",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 (changes detected)"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(json["ok"], true);
    assert!(json["files_changed"].as_u64().unwrap() >= 1);
    assert!(!json["files"].as_array().unwrap().is_empty());
    // In diff mode, diff should be present
    assert!(
        json["diff"].is_string(),
        "diff field should be present in default mode"
    );
}

#[test]
fn test_tidy_fix_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing   \n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--trim-trailing-whitespace", "--jsonl"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty(), "expected at least one JSONL line");
    for line in &lines {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each JSONL line should be valid JSON");
        assert!(v["path"].is_string());
    }
}

#[test]
fn test_tidy_fix_json_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("dirty.txt");
    fs::write(&file, "no newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "tidy",
            "fix",
            ".",
            "--ensure-final-newline",
            "--check",
            "--json",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(json["ok"], true);
    assert!(json["files_changed"].as_u64().unwrap() >= 1);
    // In --check mode, diff should not be present
    assert!(
        json["diff"].is_null(),
        "diff should be absent in --check mode"
    );
}

#[test]
fn test_tidy_fix_json_no_changes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("clean.txt");
    fs::write(&file, "already clean\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--ensure-final-newline", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON");
    assert_eq!(json["ok"], true);
    assert_eq!(json["files_changed"], 0);
    assert!(json["files"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// md --apply --check: check takes priority (bug fix)
// ---------------------------------------------------------------------------

#[test]
fn test_tidy_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    // Text file with trailing whitespace.
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "hello   \n").unwrap();
    // Binary file with trailing spaces (should not be touched).
    let bin_file = dir.path().join("data.bin");
    let mut bin_content = b"hello   ".to_vec();
    bin_content.push(0);
    fs::write(&bin_file, &bin_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(dir.path().to_str().unwrap())
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .assert()
        .code(0);

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "hello\n");

    let bin_after = fs::read(&bin_file).unwrap();
    assert_eq!(bin_after, bin_content, "binary file should not be modified");
}

#[test]
fn test_tidy_skips_invalid_utf8_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "hello   \n").unwrap();

    let invalid_file = dir.path().join("invalid.txt");
    let invalid_content = b"hello\xff   ".to_vec();
    fs::write(&invalid_file, &invalid_content).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(dir.path().to_str().unwrap())
        .arg("--trim-trailing-whitespace")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "hello\n");

    let invalid_after = fs::read(&invalid_file).unwrap();
    assert_eq!(
        invalid_after, invalid_content,
        "invalid UTF-8 file should not be modified"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid.txt (invalid UTF-8)"),
        "expected tidy invalid UTF-8 warning, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// status: staged new file (porcelain code "A") shows as created
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn test_tidy_apply_readonly_dir_fails_gracefully() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("locked");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("target.txt");
    fs::write(&file, "trailing spaces   \n").unwrap();
    if !readonly_dir_blocks_writes(&sub) {
        return;
    }

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .output()
        .unwrap();

    assert_ne!(
        output.status.code(),
        Some(0),
        "write in readonly dir should fail"
    );
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "trailing spaces   \n", "file should be unchanged");

    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
}

// ---------------------------------------------------------------------------
// tidy fix --dedent / --indent
// ---------------------------------------------------------------------------

#[test]
fn test_tidy_fix_dedent_auto_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("indented.txt");
    fs::write(&file, "    line1\n        line2\n    line3\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--dedent", "auto", "--apply"])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line1\n    line2\nline3\n");
}

#[test]
fn test_tidy_fix_dedent_numeric_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("indented.txt");
    fs::write(&file, "        line1\n    line2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--dedent", "4", "--apply"])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "    line1\nline2\n");
}

#[test]
fn test_tidy_fix_indent_numeric_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("flat.txt");
    fs::write(&file, "line1\nline2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--indent", "4", "--apply"])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "    line1\n    line2\n");
}

#[test]
fn test_tidy_fix_indent_tab_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("flat.txt");
    fs::write(&file, "line1\nline2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--indent", "tab", "--apply"])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "\tline1\n\tline2\n");
}

#[test]
fn test_tidy_fix_dedent_with_line_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ranged.txt");
    fs::write(&file, "    line1\n    line2\n    line3\n    line4\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "tidy", "fix", ".", "--dedent", "4", "--lines", "2:3", "--apply",
        ])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "    line1\nline2\nline3\n    line4\n");
}

#[test]
fn test_tidy_fix_dedent_with_line_range_dash_separator() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ranged_dash.txt");
    fs::write(&file, "    line1\n    line2\n    line3\n    line4\n").unwrap();

    // Dash separator must work identically to colon after parse_line_range unification.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "tidy", "fix", ".", "--dedent", "4", "--lines", "2-3", "--apply",
        ])
        .current_dir(dir.path())
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "    line1\nline2\nline3\n    line4\n");
}

#[test]
fn test_tidy_fix_dedent_and_indent_rejects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "    line1\n").unwrap();

    // Both --dedent and --indent should fail validation.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "tidy", "fix", ".", "--dedent", "auto", "--indent", "4", "--apply",
        ])
        .current_dir(dir.path())
        .assert()
        .code(1);
}

#[test]
fn test_tidy_fix_dedent_dry_run_no_modify() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("indented.txt");
    let original = "    line1\n    line2\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix", ".", "--dedent", "auto"])
        .current_dir(dir.path())
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, original, "file should not be modified in dry-run");
}
