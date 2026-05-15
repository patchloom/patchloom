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
