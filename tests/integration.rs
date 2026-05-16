use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn shell_false() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "false"
    }
}

fn shell_exit_1() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "exit 1"
    }
}

fn shell_sleep_300() -> &'static str {
    #[cfg(windows)]
    {
        "powershell -Command \"Start-Sleep -Seconds 300\""
    }
    #[cfg(not(windows))]
    {
        "sleep 300"
    }
}

fn shell_touch(path: &Path) -> String {
    let path = path.display();
    #[cfg(windows)]
    {
        format!("powershell -Command \"New-Item -ItemType File -Path '{path}' -Force | Out-Null\"")
    }
    #[cfg(not(windows))]
    {
        format!("touch '{path}'")
    }
}

fn shell_fail_with_secret(secret: &str) -> String {
    #[cfg(windows)]
    {
        format!("cmd /C \"set PATCHLOOM_SECRET={secret}&& exit /b 1\"")
    }
    #[cfg(not(windows))]
    {
        format!("PATCHLOOM_SECRET='{secret}' false")
    }
}

fn shell_test_exists(path: &Path) -> String {
    let path = path.display();
    #[cfg(windows)]
    {
        format!("if exist \"{path}\" (exit /b 0) else (exit /b 1)")
    }
    #[cfg(not(windows))]
    {
        format!("test -f '{path}'")
    }
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// completions
// ---------------------------------------------------------------------------

#[test]
fn test_completions_supported_shells() {
    for (shell, expected_marker) in [
        ("bash", "_patchloom"),
        ("zsh", "#compdef patchloom"),
        ("fish", "function __fish_patchloom_global_optspecs"),
        ("elvish", "edit:completion:arg-completer[patchloom]"),
    ] {
        Command::cargo_bin("patchloom")
            .unwrap()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected_marker));
    }
}

// ---------------------------------------------------------------------------
// --glob flag
// ---------------------------------------------------------------------------

#[test]
fn test_glob_filters_by_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("skip.txt"), "fn main() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--glob")
        .arg("*.rs")
        .arg("search")
        .arg("fn main")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("keep.rs"))
        .stdout(predicate::str::contains("skip.txt").not());
}

// ---------------------------------------------------------------------------
// --cwd flag
// ---------------------------------------------------------------------------

#[test]
fn test_cwd_search_finds_files_in_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("search")
        .arg("hello")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn test_cwd_nonexistent_directory_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg("/nonexistent/dir/12345")
        .arg("search")
        .arg("hello")
        .arg(".")
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// search --multiline
// ---------------------------------------------------------------------------

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
        .stdout(predicate::str::contains("}"));
}

// ---------------------------------------------------------------------------
// replace
// ---------------------------------------------------------------------------

#[test]
fn test_replace_apply_modifies_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old_text content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("old_text")
        .arg("--to")
        .arg("new_text")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

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
        .arg("--from")
        .arg("old_text")
        .arg("--to")
        .arg("new_text")
        .arg(&file)
        .assert()
        .success();

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
fn test_patch_apply_dry_run_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    // Without --apply, patchloom patch apply should show diff but NOT write.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg("--file")
        .arg(&patch_file)
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "line1\nold line\nline3\n",
        "file should be unchanged in dry-run mode"
    );
}

#[test]
fn test_patch_apply_with_apply_flag_writes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg("--file")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

#[test]
fn test_replace_if_exists_no_match_exit_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("nonexistent")
        .arg("--to")
        .arg("new")
        .arg("--if-exists")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// --quiet flag
// ---------------------------------------------------------------------------

#[test]
fn test_quiet_suppresses_replace_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("replace")
        .arg("--from")
        .arg("hello")
        .arg("--to")
        .arg("hi")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress text output, got: {stdout}"
    );

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hi world\n", "file should still be modified");
}

#[test]
fn test_quiet_suppresses_create_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("quiet_create.txt");

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress create output, got: {stdout}"
    );

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello", "file should still be created");
}

#[test]
fn test_quiet_suppresses_search_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("search_quiet.txt");
    fs::write(&file, "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("search")
        .arg("hello")
        .arg(&file)
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress search output, got: {stdout}"
    );
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
        .stdout(predicate::str::contains(":2"));
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
fn test_quiet_suppresses_hygiene_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("no_newline.txt");
    fs::write(&file, "missing newline").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("hygiene")
        .arg("check")
        .arg(dir.path())
        .assert()
        .code(2); // CHANGES_DETECTED

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress hygiene check output, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// search: incompatible flags
// ---------------------------------------------------------------------------

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
}

// ---------------------------------------------------------------------------
// replace: invalid regex
// ---------------------------------------------------------------------------

#[test]
fn test_replace_invalid_regex_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg("--from")
        .arg("[invalid(regex")
        .arg("--to")
        .arg("x")
        .arg(&file)
        .assert()
        .failure();
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
        .arg("--from")
        .arg(r"fn main\(\) \{.*\}")
        .arg("--to")
        .arg("fn main() { /* replaced */ }")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("/* replaced */"),
        "multiline replace should span newlines"
    );
}

// ---------------------------------------------------------------------------
// doc
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_has_existing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom","version":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("name")
        .assert()
        .success();
}

#[test]
fn test_doc_has_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("missing")
        .assert()
        .success()
        .stdout(predicate::str::contains("false"));
}

#[test]
fn test_doc_keys_lists_object_keys() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"alpha":1,"beta":2,"gamma":3}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma"));
}

#[test]
fn test_doc_set_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("2.0"));
}

#[test]
fn test_doc_delete_where() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"name":"keep"},{"name":"remove"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], serde_json::json!("keep"));
}

// ---------------------------------------------------------------------------
// md
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new content"),
        "should contain new content"
    );
    assert!(
        !content.contains("old content"),
        "should not contain old content"
    );
    assert!(
        content.contains("## Other"),
        "Other section heading should be intact"
    );
    assert!(
        content.contains("kept"),
        "Other section content should be intact"
    );
}

#[test]
fn test_md_table_append() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "## Table\n\n| H1 | H2 |\n|---|---|\n| A | B |\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("table-append")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Table")
        .arg("--row")
        .arg("| new | row |")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("| new | row |"),
        "new row should be present"
    );
    assert!(
        content.contains("| A | B |"),
        "existing row should be preserved"
    );
}

#[test]
fn test_md_insert_after_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(&file, "# Title\n\nExisting content\n\n## Other\n\nMore\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-after-heading")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("# Title")
        .arg("--content")
        .arg("Inserted line")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("Inserted line"),
        "inserted content should appear"
    );
    assert!(
        content.contains("Existing content"),
        "existing content should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_adds_new() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("new rule")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new rule"), "new bullet should be added");
    assert!(
        content.contains("- existing rule"),
        "existing bullet should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_skips_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    // Upsert the same bullet; should be idempotent.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("existing rule")
        .arg("--check")
        .assert()
        .success(); // no changes -> exit 0
}

// ---------------------------------------------------------------------------
// hygiene
// ---------------------------------------------------------------------------

#[test]
fn test_hygiene_check_detects_missing_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_hygiene_fix_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
        .arg("fix")
        .arg(&file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "file should end with a newline after fix"
    );
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

#[test]
fn test_create_new_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_create_refuses_overwrite() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("overwrite")
        .arg("--apply")
        .assert()
        .code(1);
}

// ---------------------------------------------------------------------------
// tx
// ---------------------------------------------------------------------------

#[test]
fn test_tx_multi_op_plan() {
    let dir = TempDir::new().unwrap();

    let txt_file = dir.path().join("test.txt");
    fs::write(&txt_file, "hello world\n").unwrap();

    let json_file = dir.path().join("config.json");
    fs::write(&json_file, r#"{"name":"old"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "from": "hello",
                "to": "hi"
            },
            {
                "op": "doc.set",
                "path": json_file.to_str().unwrap(),
                "key": "name",
                "value": "new"
            }
        ]
    });

    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert!(txt_content.contains("hi"), "text file should be modified");
    assert!(
        !txt_content.contains("hello"),
        "old text should be replaced"
    );

    let json_content = fs::read_to_string(&json_file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json_content).unwrap();
    assert_eq!(v["name"], serde_json::json!("new"));
}

#[test]
fn test_tx_rollback_on_failure() {
    let dir = TempDir::new().unwrap();

    let txt_file = dir.path().join("test.txt");
    fs::write(&txt_file, "hello world\n").unwrap();

    let nonexistent = dir.path().join("nonexistent.json");

    let plan = serde_json::json!({
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "from": "hello",
                "to": "hi"
            },
            {
                "op": "doc.set",
                "path": nonexistent.to_str().unwrap(),
                "key": "name",
                "value": "test"
            }
        ]
    });

    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(7);

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert_eq!(
        txt_content, "hello world\n",
        "file should not be modified on rollback"
    );
}

// ---------------------------------------------------------------------------
// tx: atomic rollback
// ---------------------------------------------------------------------------

#[test]
fn test_tx_rollback_preserves_original_content() {
    let dir = TempDir::new().unwrap();
    let file_a = dir.path().join("a.json");
    fs::write(&file_a, r#"{"key": "original"}"#).unwrap();
    // b.json does not exist, so doc.set on it will fail.

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.set", "path": "a.json", "key": "key", "value": "changed"},
            {"op": "doc.set", "path": "b.json", "key": "missing", "value": "fail"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // a.json should be unchanged (rolled back).
    let content = fs::read_to_string(&file_a).unwrap();
    assert_eq!(
        content, r#"{"key": "original"}"#,
        "a.json should be rolled back"
    );
}

#[test]
fn test_tx_success_applies_all() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "old", "version": 1}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.set", "path": "config.json", "key": "name", "value": "new"},
            {"op": "doc.set", "path": "config.json", "key": "version", "value": 2}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], serde_json::json!("new"));
    assert_eq!(v["version"], serde_json::json!(2));
}

#[test]
fn test_tx_check_mode_reports_changes_without_writing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.set", "path": "data.json", "key": "key", "value": "new"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--check")
        .assert()
        .code(2); // CHANGES_DETECTED

    // File should be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, r#"{"key": "old"}"#);
}

// ---------------------------------------------------------------------------
// doc: YAML and TOML
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_yaml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: patchloom\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("\"patchloom\"")
                .or(predicate::str::starts_with("patchloom")),
        );
}

#[test]
fn test_doc_set_yaml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: old\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new"), "YAML should contain updated value");
    assert!(
        !content.contains("old"),
        "YAML should not contain old value"
    );
}

#[test]
fn test_doc_get_toml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "[package]\nname = \"patchloom\"\nversion = \"1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("package.name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_set_toml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(&file, "[package]\nname = \"old\"\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("package.name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new"), "TOML should contain updated value");
}

#[test]
fn test_doc_len_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items":[1,2,3,4,5]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("items")
        .assert()
        .success()
        .stdout(predicate::str::contains("5"));
}

#[test]
fn test_doc_append_to_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"tags":["a","b"]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("tags")
        .arg(r#""c""#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
}

#[test]
fn test_doc_flatten_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a":1,"b":{"c":2},"d":[10,20]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a = 1"))
        .stdout(predicate::str::contains("b.c = 2"))
        .stdout(predicate::str::contains("d[0] = 10"))
        .stdout(predicate::str::contains("d[1] = 20"));
}

#[test]
fn test_doc_flatten_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"patchloom\""));
}

// ---------------------------------------------------------------------------
// doc diff
// ---------------------------------------------------------------------------

#[test]
fn test_doc_diff_shows_changes() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"name":"old","keep":1}"#).unwrap();
    fs::write(&b, r#"{"name":"new","keep":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(predicate::str::contains("~ name"));
}

// ---------------------------------------------------------------------------
// --check mode: exits 2 when changes detected, does NOT write
// ---------------------------------------------------------------------------

#[test]
fn test_replace_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("hello")
        .arg("--to")
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
fn test_hygiene_check_exits_2_with_issues() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_hygiene_check_exits_0_when_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "clean file\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
        .arg("check")
        .arg(&file)
        .assert()
        .success();
}

#[test]
fn test_patch_check_exits_0_when_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg("--file")
        .arg(&patch_file)
        .assert()
        .success();
}

#[test]
fn test_patch_check_exits_5_when_stale() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg("--file")
        .arg(&patch_file)
        .assert()
        .code(5);
}

#[test]
fn test_create_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--check")
        .assert()
        .code(2);

    assert!(!file.exists(), "file should not be created in --check mode");
}

// ---------------------------------------------------------------------------
// output format
// ---------------------------------------------------------------------------

#[test]
fn test_json_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .arg("--json")
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["matches"].is_array(), "matches should be an array");
}

#[test]
fn test_replace_json_check_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file.txt"), "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("--from")
        .arg("hello")
        .arg("--to")
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

// ---------------------------------------------------------------------------
// CLI flags
// ---------------------------------------------------------------------------

#[test]
fn test_version_flag() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_help_flag() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("search")
                .and(predicate::str::contains("replace"))
                .and(predicate::str::contains("doc")),
        );
}

// ---------------------------------------------------------------------------
// Parse smoke tests: every subcommand name is recognized by clap
// ---------------------------------------------------------------------------

#[test]
fn test_parse_subcommand_search() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_replace() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_patch() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["patch", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_md() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["md", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_doc() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_hygiene() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["hygiene", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_create() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["create", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_tx() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tx", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_global_flag_json() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "search", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_write_flag_ensure_final_newline() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["hygiene", "--ensure-final-newline", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_write_flag_normalize_eol() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--normalize-eol", "lf", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_unknown_subcommand_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("nonexistent-command")
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// doc: delete, merge, prepend, select, ensure, move, update
// ---------------------------------------------------------------------------

#[test]
fn test_doc_delete_removes_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"keep","remove_me":true}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("remove_me")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], serde_json::json!("keep"));
    assert!(v.get("remove_me").is_none(), "key should be removed");
}

#[test]
fn test_doc_merge_combines_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"b":2}"#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["a"], serde_json::json!(1));
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_prepend_inserts_at_front() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[2,3]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("1")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0], serde_json::json!(1));
    assert_eq!(items.len(), 3);
}

#[test]
fn test_doc_select_filters_by_predicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"status":"active","name":"a"},{"status":"done","name":"b"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=active]")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"a\""))
        .stdout(predicate::str::contains("\"b\"").not());
}

#[test]
fn test_doc_ensure_creates_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("b")
        .arg("2")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_ensure_noop_when_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    // ensure with --check when key already exists should exit 0 (no changes)
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("1")
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn test_doc_move_renames_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"old_key":"value"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("old_key")
        .arg("new_key")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["new_key"], serde_json::json!("value"));
    assert!(v.get("old_key").is_none(), "old key should be gone");
}

#[test]
fn test_doc_update_matching_nodes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"s":"a"},{"s":"b"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[*].s")
        .arg("\"x\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0]["s"], serde_json::json!("x"));
    assert_eq!(items[1]["s"], serde_json::json!("x"));
}

// ---------------------------------------------------------------------------
// md: dedupe-headings, lint-agents
// ---------------------------------------------------------------------------

#[test]
fn test_md_dedupe_headings_removes_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("dedupe-headings")
        .arg("--file")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let count = content.matches("## Dup").count();
    assert_eq!(count, 1, "duplicate heading should be removed");
}

#[test]
fn test_md_lint_agents_clean_file_exits_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# AGENTS.md\n\n## Build\n\nRun make\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg("--file")
        .arg(&file)
        .assert()
        .success();
}

#[test]
fn test_md_lint_agents_bad_file_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg("--file")
        .arg(&file)
        .assert()
        .code(2);
}

// ---------------------------------------------------------------------------
// global flags: --jsonl, --files-from, --context, --literal
// ---------------------------------------------------------------------------

#[test]
fn test_jsonl_output_produces_valid_json_lines() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\nhello again\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let _: serde_json::Value =
            serde_json::from_str(line).expect("each JSONL line should be valid JSON");
    }
    assert!(
        stdout.lines().count() >= 2,
        "should have at least 2 JSONL lines for 2 matches"
    );
}

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
// create --force
// ---------------------------------------------------------------------------

#[test]
fn test_create_force_overwrites_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("overwritten")
        .arg("--force")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "overwritten");
}

// ---------------------------------------------------------------------------
// error paths
// ---------------------------------------------------------------------------

#[test]
fn test_replace_no_match_without_if_exists_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("nonexistent_pattern_xyz")
        .arg("--to")
        .arg("new")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(3);
}

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
fn test_tx_file_create_and_delete() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "operations": [
            {"op": "file.create", "path": "new.txt", "content": "hello"},
            {"op": "file.delete", "path": "new.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // Create then immediately delete in same tx: file should not exist after.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete in same tx"
    );
}

#[test]
fn test_tx_check_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "operations": [
            {"op": "file.create", "path": "new.txt", "content": "hello"},
            {"op": "file.delete", "path": "new.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--check")
        .assert()
        .success();

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete no-op check"
    );
}

#[test]
fn test_tx_json_output_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "operations": [
            {"op": "file.create", "path": "new.txt", "content": "hello"},
            {"op": "file.delete", "path": "new.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_created"], 0);
    assert_eq!(json["files_deleted"], 0);
    assert_eq!(json["files_changed"], 0);
    assert_eq!(json["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn test_tx_file_delete_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "goodbye\n").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "file.delete", "path": "doomed.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    assert!(!file.exists(), "file should be deleted");
}

#[test]
fn test_tx_cli_ensure_final_newline_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "CLI write flag should add final newline"
    );
    assert!(
        String::from_utf8_lossy(&content).contains("has newline"),
        "replace should still work"
    );
}

#[test]
fn test_tx_plan_write_policy_overrides_cli_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
        "write_policy": {
            "ensure_final_newline": false
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        !content.ends_with(b"\n"),
        "plan write_policy should override conflicting CLI flag"
    );
    assert!(
        String::from_utf8_lossy(&content).contains("has newline"),
        "replace should still work"
    );
}

#[test]
fn test_tx_write_policy_ensure_final_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
        "write_policy": {
            "ensure_final_newline": true
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "write_policy should add final newline"
    );
    assert!(
        String::from_utf8_lossy(&content).contains("has newline"),
        "replace should still work"
    );
}

// ---------------------------------------------------------------------------
// doc/patch error paths
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_nonexistent_file_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg("/nonexistent/file_xyz.json")
        .arg("key")
        .assert()
        .failure();
}

#[test]
fn test_doc_get_unsupported_extension_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.ini");
    fs::write(&file, "key=value\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("key")
        .assert()
        .failure();
}

#[test]
fn test_patch_malformed_file_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "this is not a valid unified diff\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg("--file")
        .arg(&patch_file)
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// --jsonl + count/files-with-matches (bug fix: count_only + jsonl)
// ---------------------------------------------------------------------------

#[test]
fn test_jsonl_files_with_matches_emits_per_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "should have one JSONL line per matched file"
    );
    for line in &lines {
        assert!(
            line["path"].is_string(),
            "each line should have a path field"
        );
    }
}

#[test]
fn test_jsonl_count_emits_per_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["count"], serde_json::json!(2));
    assert!(v["path"].is_string());
}

// ---------------------------------------------------------------------------
// json + count combination
// ---------------------------------------------------------------------------

#[test]
fn test_json_count_produces_valid_envelope() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\nhello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["match_count"], serde_json::json!(3));
    assert_eq!(v["file_count"], serde_json::json!(1));
}

// ---------------------------------------------------------------------------
// tx: validate steps, doc operations in plan
// ---------------------------------------------------------------------------

#[test]
fn test_tx_validate_required_failure_exits_6() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "old", "to": "new"}
        ],
        "validate": [
            {"cmd": shell_false(), "required": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(6);
}

#[test]
fn test_tx_doc_delete_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"keep":1,"remove":2}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.delete", "path": "config.json", "key": "remove"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("remove").is_none());
    assert_eq!(v["keep"], serde_json::json!(1));
}

// ---------------------------------------------------------------------------
// doc: edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_doc_select_no_matches_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"status":"active"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=nonexistent]")
        .assert()
        .code(3);
}

#[test]
fn test_doc_move_missing_source_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("nonexistent")
        .arg("target")
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_doc_merge_nested_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"x":{"existing":"old"}}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"x":{"nested":"new"}}"#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["x"]["existing"],
        serde_json::json!("old"),
        "existing key preserved"
    );
    assert_eq!(v["x"]["nested"], serde_json::json!("new"), "new key merged");
}

#[test]
fn test_doc_ensure_noop_when_value_differs() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    // ensure a=99 when a already exists with value 1: should NOT change the value
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("99")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["a"],
        serde_json::json!(1),
        "ensure should not overwrite existing key"
    );
}

#[test]
fn test_doc_set_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("1.0"),
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// tx: file.delete on empty file (bug fix), validation optional step
// ---------------------------------------------------------------------------

#[test]
fn test_tx_file_delete_empty_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "file.delete", "path": "empty.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    assert!(!file.exists(), "empty file should be deleted");
}

#[test]
fn test_tx_optional_validation_failure_ignored() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "old", "to": "new"}
        ],
        "validate": [
            {"cmd": shell_false(), "required": false}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // Optional validation failure should still succeed.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new"),
        "file should be modified despite optional validation failure"
    );
}

// ---------------------------------------------------------------------------
// md --check leaves file untouched
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// hygiene fix --check
// ---------------------------------------------------------------------------

#[test]
fn test_hygiene_fix_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing whitespace   ").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
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

// ---------------------------------------------------------------------------
// md --apply --check: check takes priority (bug fix)
// ---------------------------------------------------------------------------

#[test]
fn test_md_apply_check_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg("--file")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "--check should prevent writing even with --apply"
    );
}

// ---------------------------------------------------------------------------
// tx: glob-based replace
// ---------------------------------------------------------------------------

#[test]
fn test_tx_glob_replace_only_matches_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("skip.rs"), "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "glob": "*.txt", "from": "hello", "to": "bye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    assert!(fs::read_to_string(dir.path().join("a.txt"))
        .unwrap()
        .contains("bye"));
    assert!(fs::read_to_string(dir.path().join("b.txt"))
        .unwrap()
        .contains("bye"));
    assert!(
        fs::read_to_string(dir.path().join("skip.rs"))
            .unwrap()
            .contains("hello"),
        ".rs file should not be modified"
    );
}

// ---------------------------------------------------------------------------
// tx: md operations in plan
// ---------------------------------------------------------------------------

#[test]
fn test_tx_md_replace_section_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(
        &file,
        "# Title\n\n## Changelog\n\nOld entry\n\n## Other\n\nKept\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "operations": [
            {
                "op": "md.replace_section",
                "path": "readme.md",
                "heading": "## Changelog",
                "content": "- v2.0 release"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("v2.0 release"), "new section content");
    assert!(!content.contains("Old entry"), "old content removed");
    assert!(content.contains("Kept"), "other section preserved");
}

// ---------------------------------------------------------------------------
// write policy CLI flags: normalize-eol, trim-trailing-whitespace, --diff
// ---------------------------------------------------------------------------

#[test]
fn test_replace_normalize_eol_lf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "old\r\ncontent\r\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("old")
        .arg("--to")
        .arg("new")
        .arg("--normalize-eol")
        .arg("lf")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

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
fn test_create_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("trimmed.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg("--file")
        .arg(&file)
        .arg("--content")
        .arg("hello   \nworld\t\n")
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello\nworld\n",
        "trailing whitespace should be trimmed"
    );
}

// ---------------------------------------------------------------------------
// multi-file replace --apply on directory
// ---------------------------------------------------------------------------

#[test]
fn test_replace_directory_modifies_all_matching_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello again\n").unwrap();
    fs::write(dir.path().join("c.txt"), "no match here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("hello")
        .arg("--to")
        .arg("bye")
        .arg(dir.path())
        .arg("--apply")
        .assert()
        .success();

    assert!(fs::read_to_string(dir.path().join("a.txt"))
        .unwrap()
        .contains("bye"));
    assert!(fs::read_to_string(dir.path().join("b.txt"))
        .unwrap()
        .contains("bye"));
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
fn test_editorconfig_final_newline() {
    let dir = TempDir::new().unwrap();

    // Create .editorconfig with insert_final_newline = true
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    )
    .unwrap();

    // Create a file without trailing newline
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("hygiene")
        .arg("fix")
        .arg(&file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "file should end with a newline after editorconfig-driven fix"
    );
}

// ---------------------------------------------------------------------------
// New features: tx plan ops, --nth, --case-insensitive, multi-glob, delete,
// md insert-before-heading, file.create force, format steps, patch.apply in tx
// ---------------------------------------------------------------------------

#[test]
fn test_replace_nth_replaces_only_nth_occurrence() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar foo baz foo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("foo")
        .arg("--to")
        .arg("REPLACED")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "foo bar REPLACED baz foo\n");
}

#[test]
fn test_replace_nth_no_match_when_out_of_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("foo")
        .arg("--to")
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
fn test_replace_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Hello HELLO hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--from")
        .arg("hello")
        .arg("--to")
        .arg("HI")
        .arg("-i")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "HI HI HI\n");
}

#[test]
fn test_multi_glob_filters_multiple_patterns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "match\n").unwrap();
    fs::write(dir.path().join("b.md"), "match\n").unwrap();
    fs::write(dir.path().join("c.txt"), "match\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("match")
        .arg("--glob")
        .arg("*.rs")
        .arg("--glob")
        .arg("*.md")
        .arg(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("b.md"));
    assert!(!stdout.contains("c.txt"));
}

#[test]
fn test_delete_removes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "bye\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert!(!file.exists());
}

#[test]
fn test_delete_check_mode_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "still here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(2);

    assert!(file.exists());
}

#[test]
fn test_delete_nonexistent_file_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ghost.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_md_insert_before_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\nIntro.\n\n## Section A\n\nContent A.\n\n## Section B\n\nContent B.\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Section B")
        .arg("--content")
        .arg("Inserted before B.")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before B.\n\n## Section B"));
}

#[test]
fn test_tx_file_create_force_overwrites() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "overwritten\n",
            "force": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "overwritten\n");
}

#[test]
fn test_tx_file_create_without_force_fails_on_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "should fail\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
}

#[test]
fn test_tx_format_step_runs_between_write_and_validate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "before\n").unwrap();

    // The format step creates a marker file to prove it ran.
    let marker = dir.path().join("format_ran");
    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "before",
            "to": "after"
        }],
        "format": [{"cmd": shell_touch(&marker)}],
        "validate": [{"cmd": shell_test_exists(&marker), "required": true}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");
    assert!(marker.exists(), "format step should have created marker");
}

#[test]
fn test_tx_doc_prepend_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2, 3]}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.prepend",
            "path": file.to_str().unwrap(),
            "key": "items",
            "value": 0
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["items"][0], 0);
    assert_eq!(v["items"][1], 1);
}

#[test]
fn test_tx_doc_set_selector_alias_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"nested": {"name": "old"}}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.set",
            "path": file.to_str().unwrap(),
            "selector": "nested.name",
            "value": "new"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["nested"]["name"], "new");
}

#[test]
fn test_tx_doc_ensure_selector_alias_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "test"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.ensure", "path": file.to_str().unwrap(), "selector": "version", "value": "1.0"},
            {"op": "doc.ensure", "path": file.to_str().unwrap(), "selector": "name", "value": "ignored"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["version"], "1.0");
    assert_eq!(v["name"], "test");
}

#[test]
fn test_tx_doc_ensure_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "test"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "doc.ensure", "path": file.to_str().unwrap(), "key": "version", "value": "1.0"},
            {"op": "doc.ensure", "path": file.to_str().unwrap(), "key": "name", "value": "ignored"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["version"], "1.0");
    assert_eq!(v["name"], "test"); // Not overwritten.
}

#[test]
fn test_tx_doc_move_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"old_key": "value"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.move",
            "path": file.to_str().unwrap(),
            "from": "old_key",
            "to": "new_key"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["new_key"], "value");
    assert!(v.get("old_key").is_none());
}

#[test]
fn test_tx_doc_delete_where_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"items": [{"name": "keep"}, {"name": "remove"}, {"name": "keep2"}]}"#,
    )
    .unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.delete_where",
            "path": file.to_str().unwrap(),
            "key": "items",
            "predicate": "name=remove"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "keep");
    assert_eq!(items[1]["name"], "keep2");
}

#[test]
fn test_tx_doc_update_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"items": [{"status": "open"}, {"status": "open"}, {"status": "closed"}]}"#,
    )
    .unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.update",
            "path": file.to_str().unwrap(),
            "key": "items[*].status",
            "value": "archived"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let items = v["items"].as_array().unwrap();
    assert!(items.iter().all(|i| i["status"] == "archived"));
}

#[test]
fn test_tx_md_insert_before_heading_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\n## Section\n\nContent.\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "md.insert_before_heading",
            "path": file.to_str().unwrap(),
            "heading": "Section",
            "content": "Inserted before."
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before.\n\n## Section"));
}

#[test]
fn test_tx_md_upsert_bullet_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Rules\n\n- existing rule\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "md.upsert_bullet",
            "path": file.to_str().unwrap(),
            "heading": "Rules",
            "bullet": "- new rule"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("- existing rule"));
    assert!(content.contains("- new rule"));
}

#[test]
fn test_tx_md_table_append_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Targets\n\n| Name | Desc |\n|------|------|\n| build | compile |\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "md.table_append",
            "path": file.to_str().unwrap(),
            "heading": "Targets",
            "row": "| test | run tests |"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("| test | run tests |"));
}

#[test]
fn test_tx_md_dedupe_headings_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\nFirst.\n\n## Dupe\n\nA.\n\n## Dupe\n\nB.\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "md.dedupe_headings",
            "path": file.to_str().unwrap()
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    // Only one "## Dupe" heading should remain.
    assert_eq!(content.matches("## Dupe").count(), 1);
}

#[test]
fn test_tx_replace_nth_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa bbb aaa ccc aaa\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "aaa",
            "to": "ZZZ",
            "nth": 2
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa bbb ZZZ ccc aaa\n");
}

#[test]
fn test_tx_patch_apply_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let diff = format!(
        "--- a/{f}\n+++ b/{f}\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
        f = file.to_str().unwrap()
    );

    let plan = serde_json::json!({
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "patch.apply",
            "diff": diff
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\n"
    );
}

#[test]
fn test_tx_validate_timeout_kills_hanging_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
        }],
        "validate": [{
            "cmd": shell_sleep_300(),
            "required": true,
            "timeout": 1
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .code(6); // VALIDATION_FAILED, not hanging forever
}

#[test]
fn test_tx_format_timeout_kills_hanging_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_sleep_300(),
            "timeout": 1
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .code(6); // VALIDATION_FAILED
}

#[test]
fn test_tx_strict_mode_reverts_on_format_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_exit_1()
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // File should be restored to original content.
    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
}

#[test]
fn test_tx_strict_mode_reverts_on_validate_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "validate": [{
            "cmd": shell_exit_1(),
            "required": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
}

#[test]
fn test_tx_strict_mode_restores_modified_empty_file_on_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "",
            "to": "changed"
        }],
        "validate": [{
            "cmd": shell_exit_1(),
            "required": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    assert!(file.exists(), "modified empty file should not be deleted");
    assert_eq!(fs::read_to_string(&file).unwrap(), "");
}

#[test]
fn test_tx_strict_mode_removes_created_files_on_failure() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("new.txt");

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "file.create",
            "path": new_file.to_str().unwrap(),
            "content": "should be removed"
        }],
        "validate": [{
            "cmd": shell_exit_1(),
            "required": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // Newly created file should be removed.
    assert!(!new_file.exists());
}

#[test]
fn test_tx_json_output_on_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 1);
    assert_eq!(json["changes"][0]["action"], "modified");
}

#[test]
fn test_tx_json_output_on_modified_empty_file_reports_modified() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "",
            "to": "changed"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["files_changed"], 1);
    assert_eq!(json["files_created"], 0);
    assert_eq!(json["changes"][0]["action"], "modified");
}

#[test]
fn test_tx_json_output_on_modified_empty_file_reports_modified_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "",
            "to": "changed"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["files_changed"], 1);
    assert_eq!(json["files_created"], 0);
    assert_eq!(json["changes"][0]["action"], "modified");
}

#[test]
fn test_tx_json_output_on_check() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    // --check with changes exits 2
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["files_changed"], 1);
}

#[test]
fn test_tx_json_output_with_create_and_delete() {
    let dir = TempDir::new().unwrap();
    let existing = dir.path().join("old.txt");
    fs::write(&existing, "content\n").unwrap();
    let new_file = dir.path().join("new.txt");

    let plan = serde_json::json!({
        "operations": [
            { "op": "file.create", "path": new_file.to_str().unwrap(), "content": "new" },
            { "op": "file.delete", "path": existing.to_str().unwrap() }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["files_created"], 1);
    assert_eq!(json["files_deleted"], 1);
    assert_eq!(json["files_changed"], 0);
}

#[test]
fn test_tx_json_output_on_operation_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "should fail"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7)); // ROLLBACK
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["error"]
        .as_str()
        .unwrap()
        .contains("file already exists"));
}

#[test]
fn test_tx_json_output_on_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // Default mode (no --apply or --check) is diff mode.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 1);
    // File should NOT be modified in diff mode.
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_json_output_on_parse_error() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("bad.json");
    fs::write(&plan_file, "{ not valid json }").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4)); // PARSE_ERROR
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert!(!json["error"].as_str().unwrap().is_empty());
    assert!(json["error"].as_str().unwrap().contains("parse_error"));
}

#[test]
fn test_tx_validation_failure_redacts_shell_command_in_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }],
        "validate": [{
            "cmd": shell_fail_with_secret(secret),
            "required": true,
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("required validation failed (step 1, exit code 1)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }],
        "validate": [{
            "cmd": shell_fail_with_secret(secret),
            "required": true,
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "validation_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("validation_failed"));
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }],
        "validate": [{
            "cmd": shell_false(),
            "required": true,
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6)); // VALIDATION_FAILED
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "validation_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("validation_failed"));
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_json_output_on_strict_validation_failure_preserves_reason() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }],
        "validate": [{
            "cmd": shell_false(),
            "required": true,
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7)); // ROLLBACK
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "validation_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("rollback"));
    assert!(error.contains("strict mode -- all changes reverted"));
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_strict_mode_restores_deleted_empty_file_on_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "file.delete",
            "path": file.to_str().unwrap()
        }],
        "validate": [{
            "cmd": shell_exit_1(),
            "required": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    assert!(file.exists(), "deleted empty file should be restored");
    assert_eq!(fs::read_to_string(&file).unwrap(), "");
}

#[test]
fn test_tx_strict_mode_restores_deleted_file_on_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "keep me\n").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "file.delete",
            "path": file.to_str().unwrap()
        }],
        "validate": [{
            "cmd": shell_exit_1(),
            "required": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // Deleted file should be restored.
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep me\n");
}

#[test]
fn test_tx_non_strict_format_failure_exits_6_not_7() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_exit_1()
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(6); // VALIDATION_FAILED (not ROLLBACK)

    // File should still be modified (non-strict doesn't revert).
    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
}

#[test]
fn test_tx_format_failure_redacts_shell_command_in_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_fail_with_secret(secret),
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("format step failed (step 1, exit code 1)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_format_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_fail_with_secret(secret),
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "format_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("validation_failed"));
    assert!(error.contains("format step failed (step 1, exit code 1)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_strict_format_failure_preserves_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_false(),
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7)); // ROLLBACK
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "format_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("rollback"));
    assert!(error.contains("strict mode -- all changes reverted"));
    assert!(error.contains("format step failed (step 1, exit code 1)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_respect_editorconfig_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    )
    .unwrap();
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "EditorConfig-driven tx write should add final newline"
    );
    assert!(
        String::from_utf8_lossy(&content).contains("has newline"),
        "replace should still work"
    );
}

#[test]
fn test_tx_write_policy_ensure_final_newline_on_file_create() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("newfile.txt");

    let plan = serde_json::json!({
        "write_policy": { "ensure_final_newline": true },
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "no trailing newline"
        }]
    });

    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "no trailing newline\n");
}

#[test]
fn test_tx_plan_from_stdin() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }]
    });

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg("-")
        .arg("--apply")
        .write_stdin(serde_json::to_string(&plan).unwrap())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "world\n");
}

#[test]
fn test_tx_create_after_delete_unmarks_deletion() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old content\n").unwrap();

    // Delete the file, then recreate it. The create should "win"
    // because it is the last operation on that file.
    let plan = serde_json::json!({
        "operations": [
            { "op": "file.delete", "path": file.to_str().unwrap() },
            { "op": "file.create", "path": file.to_str().unwrap(), "content": "new content", "force": true }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    // File should exist with the new content, not deleted.
    assert!(
        file.exists(),
        "file should not be deleted after a subsequent create"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "new content");
}

#[test]
fn test_tx_format_and_validate_success_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let marker = dir.path().join("format_ran.marker");

    let plan = serde_json::json!({
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
        }],
        "format": [{
            "cmd": shell_touch(&marker),
            "timeout": 10
        }],
        "validate": [{
            "cmd": shell_test_exists(&marker),
            "required": true,
            "timeout": 10
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
    assert!(
        marker.exists(),
        "format step should have created the marker"
    );
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
        .arg("--from")
        .arg(r"v\d+\.\d+")
        .arg("--to")
        .arg("vX.Y")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "v1.0 and vX.Y and v3.0\n");
}

#[test]
fn test_delete_default_dry_run_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "content\n").unwrap();

    // Default mode (no --apply, no --check) is a dry-run.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("would delete"));

    assert!(file.exists(), "dry-run should not delete the file");
}

#[test]
fn test_md_insert_before_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg("--file")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--content")
        .arg("text")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_tx_doc_prepend_on_non_array_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name": "not_an_array"}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.prepend",
            "path": file.to_str().unwrap(),
            "key": "name",
            "value": "oops"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // Original content unchanged.
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        r#"{"name": "not_an_array"}"#
    );
}

#[test]
fn test_tx_doc_update_no_match_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": []}"#).unwrap();

    let plan = serde_json::json!({
        "operations": [{
            "op": "doc.update",
            "path": file.to_str().unwrap(),
            "key": "items[*].status",
            "value": "archived"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK
}

#[test]
fn test_tx_multi_op_batch_all_new_ops() {
    // A realistic batch: replace + doc ops + md ops + file create in one plan.
    let dir = TempDir::new().unwrap();

    let txt = dir.path().join("code.txt");
    fs::write(&txt, "foo bar foo baz\n").unwrap();

    let json_file = dir.path().join("config.json");
    fs::write(
        &json_file,
        r#"{"name": "old", "items": [{"id": 1}, {"id": 2}]}"#,
    )
    .unwrap();

    let md_file = dir.path().join("agents.md");
    fs::write(
        &md_file,
        "# Rules\n\n- rule one\n\n## Targets\n\n| Name | Desc |\n|------|------|\n| build | compile |\n",
    )
    .unwrap();

    let new_file = dir.path().join("new.txt");

    let plan = serde_json::json!({
        "operations": [
            {"op": "replace", "path": txt.to_str().unwrap(), "from": "foo", "to": "XXX", "nth": 1},
            {"op": "doc.set", "path": json_file.to_str().unwrap(), "key": "name", "value": "new"},
            {"op": "doc.ensure", "path": json_file.to_str().unwrap(), "key": "version", "value": "1.0"},
            {"op": "md.upsert_bullet", "path": md_file.to_str().unwrap(), "heading": "Rules", "bullet": "- rule two"},
            {"op": "md.table_append", "path": md_file.to_str().unwrap(), "heading": "Targets", "row": "| test | run tests |"},
            {"op": "file.create", "path": new_file.to_str().unwrap(), "content": "created!\n"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("--plan")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    // Verify replace --nth 1.
    assert_eq!(fs::read_to_string(&txt).unwrap(), "XXX bar foo baz\n");

    // Verify doc.set + doc.ensure.
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json_file).unwrap()).unwrap();
    assert_eq!(v["name"], "new");
    assert_eq!(v["version"], "1.0");

    // Verify md.upsert_bullet + md.table_append.
    let md = fs::read_to_string(&md_file).unwrap();
    assert!(md.contains("- rule two"));
    assert!(md.contains("| test | run tests |"));

    // Verify file.create.
    assert_eq!(fs::read_to_string(&new_file).unwrap(), "created!\n");
}

// ---------------------------------------------------------------------------
// smoke tests: docs and examples
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example_plan_path(name: &str) -> PathBuf {
    repo_root().join("examples").join(name)
}

fn quickstart_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("quickstart.md")
}

fn installation_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("installation.md")
}

fn readme_path() -> PathBuf {
    repo_root().join("README.md")
}

fn patchloom_in(cwd: &Path) -> Command {
    let mut cmd = Command::cargo_bin("patchloom").unwrap();
    cmd.arg("--cwd").arg(cwd);
    cmd
}

fn write_fixture_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn seed_docs_smoke_fixture(root: &Path) {
    write_fixture_file(
        root,
        "Cargo.toml",
        "[package]\nname = \"smoke-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_fixture_file(
        root,
        "src/lib.rs",
        "pub mod write;\n\n// TODO: rename old_function\n// TODO: update docs\npub fn old_function() -> &'static str {\n    \"old\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/write.rs",
        "pub fn existing_write() -> &'static str {\n    \"write\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/rename.rs",
        "pub fn old_name() -> &'static str {\n    \"old_name\"\n}\n",
    );
    write_fixture_file(
        root,
        "README.md",
        "# Smoke Fixture\n\nCurrent release: v1.0.0\nDocs version: v0.1.0\n\n## Commands\n\n| Command | Description |\n|---|---|\n| `search` | Search repo |\n",
    );
    write_fixture_file(
        root,
        "CHANGELOG.md",
        "# Changelog\n\n## Unreleased\n\n- Existing entry\n\n## 0.1.0\n\n- Initial release\n",
    );
    write_fixture_file(
        root,
        "AGENTS.md",
        "# AGENTS.md\n\n## Safety rules\n\n- existing rule\n\n## Safety rules\n\n- duplicate rule\n",
    );
    write_fixture_file(
        root,
        "package.json",
        "{\n  \"name\": \"smoke-fixture\",\n  \"version\": \"1.0.0\"\n}\n",
    );
    write_fixture_file(
        root,
        "config.json",
        "{\n  \"database\": {\n    \"host\": \"localhost\",\n    \"port\": 3306\n  },\n  \"allowed_origins\": [\"http://localhost:3000\"],\n  \"deprecated_field\": true\n}\n",
    );
    write_fixture_file(
        root,
        "config.yaml",
        "users:\n  - name: alice\n    active: true\n  - name: bob\n    active: false\n",
    );
}

fn extract_markdown_code_block_after(markdown: &str, marker: &str, language: &str) -> String {
    let (_, after_marker) = markdown
        .split_once(marker)
        .expect("marker should exist in markdown");
    let fence = format!("```{language}\n");
    let (_, after_fence) = after_marker
        .split_once(&fence)
        .expect("fenced code block should exist after marker");
    let (block, _) = after_fence
        .split_once("\n```")
        .expect("fenced code block should terminate");
    block.to_string()
}

#[test]
fn test_smoke_example_01_basic_replace_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(example_plan_path("01-basic-replace.json"))
        .arg("--apply")
        .assert()
        .success();

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v2.0.0"));
    assert!(!readme.contains("v1.0.0"));
}

#[test]
fn test_smoke_example_02_multi_file_batch_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(example_plan_path("02-multi-file-batch.json"))
        .arg("--apply")
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("version = \"0.2.0\""));
    assert!(!cargo_toml.contains("version = \"0.1.0\""));

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "0.2.0");

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v0.2.0"));
    assert!(!readme.contains("v0.1.0"));
}

#[test]
fn test_smoke_example_03_markdown_editing_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(example_plan_path("03-markdown-editing.json"))
        .arg("--apply")
        .assert()
        .success();

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new feature"));
    assert!(changelog.contains("- Fixed bug in parser"));
    assert!(!changelog.contains("- Existing entry"));

    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert_eq!(agents.matches("## Safety rules").count(), 1);
    assert!(agents.contains("Always run `make check` before committing"));

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("| `new-cmd` | Description of the new command |"));
}

#[test]
fn test_smoke_example_04_doc_mutations_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(example_plan_path("04-doc-mutations.json"))
        .arg("--apply")
        .assert()
        .success();

    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["database"]["port"], 5432);
    assert_eq!(config["database"]["pool_size"], 10);
    assert_eq!(config["logging"]["level"], "info");
    assert_eq!(config["logging"]["format"], "json");
    assert!(config["allowed_origins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str() == Some("https://example.com")));
    assert!(config.get("deprecated_field").is_none());

    let yaml = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(yaml.contains("active: true"));
    assert!(!yaml.contains("active: false"));
    assert!(!yaml.contains("bob"));
}

#[test]
fn test_smoke_example_05_strict_mode_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(example_plan_path("05-strict-mode.json"))
        .arg("--apply")
        .assert()
        .success();

    let new_module = fs::read_to_string(dir.path().join("src/new_module.rs")).unwrap();
    assert!(new_module.contains("pub fn hello() -> &'static str"));

    let lib_rs = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("pub mod new_module;"));

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new_module"));
}

#[test]
fn test_doc_get_honors_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        "{\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));
}

#[test]
fn test_smoke_quickstart_command_flow() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    assert!(quickstart.contains("patchloom search 'TODO' src/"));
    assert!(quickstart.contains("patchloom search 'TODO' --count src/"));
    assert!(quickstart.contains("patchloom replace --from 'old_function' --to 'new_function' src/"));
    assert!(quickstart
        .contains("patchloom replace --from 'old_function' --to 'new_function' src/ --apply"));
    assert!(quickstart.contains("patchloom doc get package.json version"));
    assert!(quickstart.contains("patchloom doc set package.json version \"2.0.0\" --apply"));

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    let lib_path = dir.path().join("src/lib.rs");

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("--count")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains(":2"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("--from")
        .arg("old_function")
        .arg("--to")
        .arg("new_function")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("new_function"));
    assert!(fs::read_to_string(&lib_path)
        .unwrap()
        .contains("old_function"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("--from")
        .arg("old_function")
        .arg("--to")
        .arg("new_function")
        .arg("src/")
        .arg("--apply")
        .assert()
        .success();
    assert!(fs::read_to_string(&lib_path)
        .unwrap()
        .contains("new_function"));
    assert!(!fs::read_to_string(&lib_path)
        .unwrap()
        .contains("old_function"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("set")
        .arg("package.json")
        .arg("version")
        .arg("\"2.0.0\"")
        .arg("--apply")
        .assert()
        .success();

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "2.0.0");
}

#[test]
fn test_smoke_quickstart_transaction_snippet() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    let plan_text = extract_markdown_code_block_after(
        &quickstart,
        "Create a plan file called `bump.json`:",
        "json",
    );
    let expected_json: serde_json::Value = serde_json::from_str(
        &extract_markdown_code_block_after(&quickstart, "Returns:", "json"),
    )
    .unwrap();

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    let plan_file = dir.path().join("bump.json");
    fs::write(&plan_file, plan_text).unwrap();

    let diff_output = patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .output()
        .unwrap();
    assert!(diff_output.status.success());
    let diff_stdout = String::from_utf8_lossy(&diff_output.stdout);
    assert!(diff_stdout.contains("package.json"));
    assert!(diff_stdout.contains("README.md"));
    assert!(diff_stdout.contains("CHANGELOG.md"));

    let package_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_before["version"], "1.0.0");
    assert!(fs::read_to_string(dir.path().join("README.md"))
        .unwrap()
        .contains("v1.0.0"));
    assert!(!fs::read_to_string(dir.path().join("CHANGELOG.md"))
        .unwrap()
        .contains("- Bumped to v2.0.0"));

    let check_output = patchloom_in(dir.path())
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check_output.status.code(), Some(2));

    let apply_output = patchloom_in(dir.path())
        .arg("--json")
        .arg("tx")
        .arg("--plan")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(apply_output.status.success());
    let actual_json: serde_json::Value = serde_json::from_slice(&apply_output.stdout).unwrap();
    assert_eq!(actual_json, expected_json);

    let package_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after["version"], "2.0.0");
    assert!(fs::read_to_string(dir.path().join("README.md"))
        .unwrap()
        .contains("v2.0.0"));
    assert!(fs::read_to_string(dir.path().join("CHANGELOG.md"))
        .unwrap()
        .contains("- Bumped to v2.0.0"));
}

#[test]
fn test_smoke_shell_completion_docs_include_elvish() {
    for (path, label) in [
        (installation_path(), "installation guide"),
        (readme_path(), "README"),
    ] {
        let content = fs::read_to_string(path).unwrap();
        assert!(
            content.contains("patchloom completions elvish"),
            "{label} should document elvish completions"
        );
    }
}

#[test]
fn test_smoke_source_install_docs_use_cargo_install_path() {
    let source_install_flow = "git clone https://github.com/patchloom/patchloom.git\ncd patchloom\ncargo install --path .";

    for (path, label) in [
        (installation_path(), "installation guide"),
        (readme_path(), "README"),
    ] {
        let content = fs::read_to_string(path).unwrap();
        assert!(
            content.contains(source_install_flow),
            "{label} should document the first-run source install flow"
        );
    }
}

#[test]
fn test_smoke_readme_command_examples() {
    let readme = fs::read_to_string(readme_path()).unwrap();
    let merge_value = r#"{"settings": {"debug": true}}"#;
    let merge_command = format!("patchloom doc merge config.json --value '{merge_value}' --apply");
    assert!(readme.contains("patchloom search 'TODO' src/"));
    assert!(readme.contains("patchloom replace --from 'old_name' --to 'new_name' src/ --apply"));
    assert!(readme.contains("patchloom doc keys package.json ."));
    assert!(readme.contains(&merge_command));
    assert!(readme.contains("patchloom hygiene check src/"));
    assert!(readme.contains("patchloom hygiene fix . --ensure-final-newline --apply"));

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("--from")
        .arg("old_name")
        .arg("--to")
        .arg("new_name")
        .arg("src/")
        .arg("--apply")
        .assert()
        .success();
    let rename_module = fs::read_to_string(dir.path().join("src/rename.rs")).unwrap();
    assert!(rename_module.contains("new_name"));
    assert!(!rename_module.contains("old_name"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("keys")
        .arg("package.json")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("version"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("merge")
        .arg("config.json")
        .arg("--value")
        .arg(merge_value)
        .arg("--apply")
        .assert()
        .success();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["settings"]["debug"], true);

    let hygiene_target = dir.path().join("notes.txt");
    fs::write(&hygiene_target, "line with space \nsecond line").unwrap();

    patchloom_in(dir.path())
        .arg("hygiene")
        .arg("check")
        .arg("notes.txt")
        .assert()
        .code(2);

    patchloom_in(dir.path())
        .arg("hygiene")
        .arg("fix")
        .arg(".")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();
    assert!(fs::read(&hygiene_target).unwrap().ends_with(b"\n"));
}
