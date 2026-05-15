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
        .code(2);

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
