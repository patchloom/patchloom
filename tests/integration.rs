use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

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
fn test_completions_bash() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("completions")
        .arg("bash")
        .assert()
        .success()
        .stdout(predicate::str::contains("_patchloom"));
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
            {"cmd": "false", "required": true}
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
            {"cmd": "false", "required": false}
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
