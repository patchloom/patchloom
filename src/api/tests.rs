use super::*;
use std::fs;

use crate::containment::{AbsolutePathPolicy, PathGuard};
use tempfile::TempDir;

#[test]
fn doc_set_preview_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    let result = doc_set(
        &file,
        "version",
        serde_json::json!("2.0"),
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.new_content.contains("2.0"));
    // File should be unchanged on disk.
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("1.0"));
}

#[test]
fn doc_set_apply_writes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    let result = doc_set(
        &file,
        "version",
        serde_json::json!("2.0"),
        ApplyMode::Apply,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("2.0"));
}

#[test]
fn doc_set_respects_guard() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();

    let result = doc_set(
        &file,
        "version",
        serde_json::json!("2.0"),
        ApplyMode::Apply,
        Some(&guard),
    )
    .unwrap();

    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("2.0"));
}

#[test]
fn doc_get_reads_value() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"database": {"host": "localhost"}}"#).unwrap();

    let value = doc_get(&file, "database.host").unwrap();
    assert_eq!(value, serde_json::json!("localhost"));
}

#[test]
fn doc_has_returns_true_for_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    assert!(doc_has(&file, "version").unwrap());
    assert!(!doc_has(&file, "nonexistent").unwrap());
}

#[test]
fn doc_delete_removes_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"a": 1, "b": 2}"#).unwrap();

    let result = doc_delete(&file, "b", ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    assert!(result.applied);
    assert_eq!(
        result.removed, 1,
        "library EditResult must report removed (#1459)"
    );
    assert_eq!(result.action, "doc.delete");
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(!on_disk.contains("\"b\""));
}

#[test]
fn doc_delete_missing_key_reports_removed_zero() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let result = doc_delete(&file, "missing", ApplyMode::Apply, None).unwrap();
    assert!(!result.changed);
    assert!(!result.applied);
    assert_eq!(
        result.removed, 0,
        "idempotent delete must report removed: 0 (#1459 / #1439)"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), r#"{"a": 1}"#);
}

#[test]
fn doc_delete_where_reports_removed_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.json");
    fs::write(
        &file,
        r#"{"items":[{"name":"a"},{"name":"b"},{"name":"a"}]}"#,
    )
    .unwrap();

    let result = doc_delete_where(&file, "items", "name=a", ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    assert_eq!(result.removed, 2);
    assert_eq!(result.action, "doc.delete_where");

    let preview = doc_delete_where(&file, "items", "name=zzz", ApplyMode::Preview, None).unwrap();
    assert!(!preview.changed);
    assert_eq!(preview.removed, 0);
    assert!(!preview.applied);
}

#[test]
fn doc_merge_merges_values() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"a": 1}"#).unwrap();

    let result = doc_merge(&file, serde_json::json!({"b": 2}), ApplyMode::Apply, None).unwrap();

    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(
        parsed["a"],
        serde_json::json!(1),
        "merge must preserve existing keys"
    );
    assert_eq!(parsed["b"], serde_json::json!(2), "merge must add new keys");
}

#[test]
fn replace_text_preview() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn old_name() {}\n").unwrap();

    let result = replace_text(
        &file,
        "old_name",
        "new_name",
        &ReplaceOptions::default(),
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.new_content.contains("new_name"));
}

#[test]
fn replace_text_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn old_name() {}\n").unwrap();

    let result = replace_text(
        &file,
        "old_name",
        "new_name",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("new_name"));
}

#[test]
fn replace_text_with_relaxed_guard() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn old_name() {}\n").unwrap();

    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();

    let result = replace_text(
        &file,
        "old_name",
        "new_name",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        Some(&guard),
    )
    .unwrap();

    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("new_name"));
}

/// QA coverage: exercise actual write to a conventional temp path using high-level api
/// under .allow_temp_directory() guard (the main motivation for relaxed policy in
/// library use by agents). Uses literal /tmp (exercises #781 fix path + ancestor canon).
#[cfg(unix)]
#[test]
fn file_create_writes_to_literal_tmp_under_allow_temp_guard() {
    let dir = TempDir::new().unwrap();
    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();

    let tmp_path = format!("/tmp/patchloom_api_tmp_test_{}.txt", std::process::id());
    let _ = fs::remove_file(&tmp_path);

    let result = file_create(
        Path::new(&tmp_path),
        "temp content via guard\n",
        true, // force ok for test
        ApplyMode::Apply,
        Some(&guard),
    )
    .unwrap();

    assert!(result.applied);
    assert!(result.changed);
    let content = fs::read_to_string(&tmp_path).unwrap();
    assert!(content.contains("temp content via guard"));
    let _ = fs::remove_file(&tmp_path);
}

#[test]
fn md_replace_section_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("README.md");
    fs::write(
        &file,
        "## Section A\n\nOld body.\n\n## Section B\n\nKeep this.\n",
    )
    .unwrap();

    let result =
        md_replace_section(&file, "Section A", "New body.\n", ApplyMode::Preview, None).unwrap();

    assert!(result.changed);
    assert!(result.new_content.contains("New body."));
    assert!(result.new_content.contains("Keep this."));
}

#[test]
fn md_upsert_bullet_adds_new() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("CHANGELOG.md");
    fs::write(&file, "# Changes\n\n- Item 1\n").unwrap();

    let result =
        md_upsert_bullet(&file, "# Changes", "- Item 2", ApplyMode::Preview, None).unwrap();

    assert!(result.changed);
    assert!(result.new_content.contains("- Item 2"));
}

#[test]
fn file_create_and_delete() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    // Create
    let result = file_create(&file, "hello\n", false, ApplyMode::Apply, None).unwrap();
    assert!(result.applied);
    assert!(file.exists());

    // Delete
    let result = file_delete(&file, ApplyMode::Apply, None).unwrap();
    assert!(result.applied);
    assert!(!file.exists());
}

#[test]
#[cfg(unix)]
fn file_create_force_propagates_read_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("unreadable.txt");
    fs::write(&file, "original").unwrap();
    fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();

    // Root (common in Docker) can still read mode-000 files. Skip when
    // permissions do not actually block reading.
    if fs::read_to_string(&file).is_ok() {
        fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    let err = file_create(&file, "new", true, ApplyMode::Apply, None).unwrap_err();
    // Restore permissions so TempDir cleanup succeeds.
    fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to read"),
        "expected read error, got: {msg}"
    );
}

#[test]
fn file_rename_works() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();
    let result = file_rename(&src, &dst, false, ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(result.applied);
    assert_eq!(
        result.dest_path.as_deref(),
        Some(dst.to_string_lossy().as_ref())
    );
    assert!(!src.exists());
    assert!(dst.exists());
    let dst_content = fs::read_to_string(&dst).unwrap();
    assert_eq!(
        dst_content, "content\n",
        "rename must preserve file content"
    );
}

#[test]
fn file_rename_rejects_guard_on_destination() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    // Strict Reject guard: passing an absolute dst triggers rejection on the destination.
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    let abs_dst = dst.canonicalize().unwrap_or(dst);

    let err = file_rename(&src, &abs_dst, false, ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert!(err.to_string().contains("path rejected by workspace guard"));
}

#[test]
fn apply_mode_check_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    let result = doc_set(
        &file,
        "version",
        serde_json::json!("2.0"),
        ApplyMode::Check,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(!result.applied);
    // File should be unchanged.
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("1.0"));
}

#[test]
fn doc_append_to_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2]}"#).unwrap();

    let result = doc_append(&file, "items", serde_json::json!(3), ApplyMode::Apply, None).unwrap();

    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(parsed["items"], serde_json::json!([1, 2, 3]));
}

#[test]
fn doc_move_renames_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"old_key": "value"}"#).unwrap();

    let result = doc_move(&file, "old_key", "new_key", ApplyMode::Apply, None).unwrap();

    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("new_key"));
    assert!(!on_disk.contains("old_key"));
}

#[test]
fn replace_text_empty_pattern_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content").unwrap();

    let err = replace_text(
        &file,
        "",
        "x",
        &ReplaceOptions::default(),
        ApplyMode::Preview,
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("empty search pattern"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "library hosts must peel invalid_input without scraping English"
    );
}

#[test]
fn edit_result_unchanged_when_no_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "version: \"1.0\"\n").unwrap();

    // Set to the same value.
    let result = doc_set(
        &file,
        "version",
        serde_json::json!("1.0"),
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(!result.changed);
    assert!(result.diff.is_empty());
}

#[test]
fn file_append_with_guard_and_preview() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("log.txt");
    fs::write(&file, "start\n").unwrap();

    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();

    // Preview
    let result = file_append(&file, "more\n", ApplyMode::Preview, Some(&guard)).unwrap();
    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.diff.contains("+more"));

    // Apply
    let result = file_append(&file, "more\n", ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(result.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "start\nmore\n");
}

#[test]
fn yaml_doc_set_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nversion: \"1.0\"\n# Keep this comment\nname: test\n",
    )
    .unwrap();

    let result = doc_set(
        &file,
        "version",
        serde_json::json!("2.0"),
        ApplyMode::Apply,
        None,
    )
    .unwrap();

    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("2.0"));
    assert!(on_disk.contains("# Main config"));
    assert!(on_disk.contains("# Keep this comment"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn execute_plan_runs_operations() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let plan_json = r#"{
            "version": 1,
            "operations": [
                {
                    "op": "replace",
                    "path": "test.txt",
                    "mode": "literal",
                    "old": "hello",
                    "new": "goodbye"
                },
                {
                    "op": "file.append",
                    "path": "test.txt",
                    "content": "\n+appended"
                }
            ]
        }"#;

    let plan = parse_plan(plan_json).unwrap();
    let report: crate::api::PlanReport = execute_plan(plan, dir.path(), None).unwrap();
    // execute_plan now returns typed PlanReport directly for library users (#811).
    assert!(report.ok);
    assert_eq!(report.status, "success");
    assert!(!report.changes.is_empty()); // net file changes from the plan ops
    assert!(
        report
            .changes
            .iter()
            .any(|c| c.action == "modified" || c.action == "created")
    );
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("goodbye"));
    assert!(on_disk.contains("+appended"));
}

/// #1439: library PlanReport exposes doc delete mutation summaries.
#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn execute_plan_doc_delete_reports_mutations() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cfg.json"),
        r#"{"items":[{"name":"keep"},{"name":"drop"},{"name":"drop"}]}"#,
    )
    .unwrap();

    let plan = parse_plan(
        r#"{
            "version": 1,
            "operations": [
                {
                    "op": "doc.delete_where",
                    "path": "cfg.json",
                    "selector": "items",
                    "predicate": "name=drop"
                },
                {
                    "op": "doc.delete",
                    "path": "cfg.json",
                    "selector": "missing"
                }
            ]
        }"#,
    )
    .unwrap();

    let report = execute_plan(plan, dir.path(), None).unwrap();
    assert!(report.ok);
    assert_eq!(report.changed, Some(true));
    assert_eq!(report.removed, Some(2));
    assert_eq!(report.mutations.len(), 2);
    assert_eq!(report.mutations[0].op, "doc.delete_where");
    assert_eq!(report.mutations[0].removed, 2);
    assert!(report.mutations[0].changed);
    assert_eq!(report.mutations[1].op, "doc.delete");
    assert_eq!(report.mutations[1].removed, 0);
    assert!(!report.mutations[1].changed);

    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("cfg.json")).unwrap()).unwrap();
    assert_eq!(on_disk["items"].as_array().unwrap().len(), 1);
    assert_eq!(on_disk["items"][0]["name"], "keep");
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn execute_plan_respects_relaxed_guard() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("plan.txt");
    fs::write(&file, "old\n").unwrap();

    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();

    let plan_json = format!(
        r#"{{
            "version": 1,
            "operations": [
                {{
                    "op": "replace",
                    "path": "{}",
                    "mode": "literal",
                    "old": "old",
                    "new": "new"
                }}
            ]
        }}"#,
        file.to_string_lossy().replace('\\', "/")
    );

    let plan = parse_plan(&plan_json).unwrap();
    let report = execute_plan(plan, dir.path(), Some(&guard)).unwrap();
    assert!(report.ok);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("new"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn execute_plan_rejects_on_guard() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("guarded.txt");
    fs::write(&file, "content\n").unwrap();

    // Strict reject on abs path outside
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    let abs = file.canonicalize().unwrap_or_else(|_| file.clone());

    let plan_json = format!(
        r#"{{
            "version": 1,
            "operations": [
                {{
                    "op": "file.delete",
                    "path": "{}"
                }}
            ]
        }}"#,
        abs.to_string_lossy().replace('\\', "/")
    );

    let plan = parse_plan(&plan_json).unwrap();
    let err = execute_plan(plan, dir.path(), Some(&guard)).unwrap_err();
    assert!(err.to_string().contains("path rejected by workspace guard"));
    // No mutation
    assert!(fs::read_to_string(&file).unwrap().contains("content"));
}

#[test]
fn apply_patch_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

    let patch = format!(
        "--- a/{f}\n+++ b/{f}\n@@ -1,3 +1,3 @@\n fn hello() {{\n-    println!(\"hi\");\n+    println!(\"hello world\");\n }}\n",
        f = "code.rs"
    );

    let result = apply_patch(&file, &patch, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.new_content.contains("hello world"));

    // File should be unchanged on disk.
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("\"hi\""));
}

#[test]
fn md_table_append_adds_row() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("table.md");
    fs::write(
        &file,
        "# Data\n\n| Name | Value |\n|------|-------|\n| a    | 1     |\n",
    )
    .unwrap();

    let result =
        md_table_append(&file, "# Data", "| b    | 2     |", ApplyMode::Apply, None).unwrap();

    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("| b    | 2     |"));
}

#[test]
fn md_table_append_column_mismatch_gives_specific_error() {
    // #1231: library path should report "column mismatch", not "heading not found"
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("table.md");
    fs::write(
        &file,
        "# Data\n\n| A | B | C |\n|---|---|---|\n| 1 | 2 | 3 |\n",
    )
    .unwrap();

    let err = md_table_append(&file, "# Data", "| x |", ApplyMode::Preview, None).unwrap_err();
    // Engine wraps with context; use alternate Display for the full chain.
    let msg = format!("{err:#}");
    assert!(
        msg.contains("column"),
        "error should mention column mismatch, got: {msg}"
    );
    assert!(
        !msg.contains("heading not found"),
        "error should NOT say 'heading not found': {msg}"
    );
}

#[test]
fn md_table_append_no_table_gives_specific_error() {
    // #1231: library path should report "no markdown table", not "heading not found"
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Data\n\nJust text, no table.\n").unwrap();

    let err = md_table_append(&file, "# Data", "| x |", ApplyMode::Preview, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("no markdown table"),
        "error should mention 'no markdown table', got: {msg}"
    );
    assert!(
        !msg.contains("heading not found"),
        "error should NOT say 'heading not found': {msg}"
    );
}

#[test]
fn md_insert_before_heading_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# First\n\nBody 1.\n\n# Second\n\nBody 2.\n").unwrap();

    let result = md_insert_before_heading(
        &file,
        "Second",
        "Inserted before second.\n\n",
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.new_content.contains("Inserted before second."));
}

#[test]
fn md_insert_after_heading_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# First\n\nBody 1.\n\n# Second\n\nBody 2.\n").unwrap();

    let result = md_insert_after_heading(
        &file,
        "First",
        "Inserted after first.\n\n",
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(result.changed);
    assert!(!result.applied);
    assert!(result.new_content.contains("Inserted after first."));
    // Verify insertion is after the heading, not before.
    let pos_heading = result.new_content.find("# First").unwrap();
    let pos_insertion = result.new_content.find("Inserted after first.").unwrap();
    assert!(pos_insertion > pos_heading);
}

#[test]
fn apply_patch_file_applies_multi_file_patch() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "aaa\n").unwrap();
    fs::write(&b, "bbb\n").unwrap();

    let patch = "\
--- a/a.txt\n\
+++ b/a.txt\n\
@@ -1 +1 @@\n\
-aaa\n\
+AAA\n\
--- a/b.txt\n\
+++ b/b.txt\n\
@@ -1 +1 @@\n\
-bbb\n\
+BBB\n";

    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.changed && r.applied));
    assert_eq!(fs::read_to_string(&a).unwrap(), "AAA\n");
    assert_eq!(fs::read_to_string(&b).unwrap(), "BBB\n");
}

#[test]
fn apply_patch_path_matching_uses_path_components() {
    // Regression: string ends_with matched "notb/foo.rs" for a patch
    // targeting "b/foo.rs". Path::ends_with does component matching.
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("notb");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("foo.rs");
    fs::write(&file, "original\n").unwrap();

    // Patch targets "b/foo.rs" which should NOT match "notb/foo.rs"
    let patch = "\
--- a/b/foo.rs\n\
+++ b/b/foo.rs\n\
@@ -1 +1 @@\n\
-original\n\
+changed\n";

    let result = apply_patch(&file, patch, ApplyMode::Preview, None);
    assert!(result.is_err() || !result.unwrap().changed);
}

#[test]
fn make_write_policy_maps_options() {
    let opts = WritePolicyOptions {
        ensure_final_newline: true,
        normalize_eol: Some(EolMode::Lf),
        trim_trailing_whitespace: true,
        collapse_blanks: true,
    };
    let policy = make_write_policy(&opts);
    assert!(policy.ensure_final_newline);
    assert!(policy.trim_trailing_whitespace);
    assert!(policy.collapse_blanks);
    assert_eq!(
        policy.normalize_eol,
        EolMode::Lf,
        "should map EolMode::Lf correctly"
    );

    // Default options should produce default policy.
    let default_policy = make_write_policy(&WritePolicyOptions::default());
    assert!(!default_policy.ensure_final_newline);
    assert!(!default_policy.trim_trailing_whitespace);
    assert!(!default_policy.collapse_blanks);
}

// --- New API function tests (#573) ---

#[test]
fn doc_prepend_adds_to_front() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [2, 3]}"#).unwrap();

    let result = doc_prepend(&file, "items", serde_json::json!(1), ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(parsed["items"], serde_json::json!([1, 2, 3]));
}

#[test]
fn doc_update_changes_matching_values() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"items": [{"name": "a", "val": 1}, {"name": "b", "val": 2}]}"#,
    )
    .unwrap();

    let result = doc_update(
        &file,
        "items[*].val",
        serde_json::json!(99),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(parsed["items"][0]["val"], serde_json::json!(99));
    assert_eq!(parsed["items"][1]["val"], serde_json::json!(99));
}

#[test]
fn doc_ensure_sets_only_if_missing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"existing": "keep"}"#).unwrap();

    // Ensure a missing key - should be added.
    let result = doc_ensure(
        &file,
        "new_key",
        serde_json::json!("added"),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("added"));

    // Ensure an existing key - should NOT change.
    let result = doc_ensure(
        &file,
        "existing",
        serde_json::json!("overwrite"),
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(!result.changed);
}

#[test]
fn doc_delete_where_removes_matching_elements() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(
        &file,
        r#"{"items": [{"name": "keep"}, {"name": "remove"}, {"name": "keep2"}]}"#,
    )
    .unwrap();

    let result = doc_delete_where(&file, "items", "name=remove", ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(!on_disk.contains("remove"));
    assert!(on_disk.contains("keep"));
    assert!(on_disk.contains("keep2"));
}

#[test]
fn md_move_section_reorders() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# A\n\nBody A.\n\n# B\n\nBody B.\n\n# C\n\nBody C.\n",
    )
    .unwrap();

    // Move section A to after section C.
    let result =
        md_move_section(&file, "A", ("after", "C"), None, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    let pos_b = result.new_content.find("# B").unwrap();
    let pos_a = result.new_content.find("# A").unwrap();
    assert!(
        pos_a > pos_b,
        "A should appear after B after moving to after C"
    );
}

#[test]
fn md_move_section_cross_file_writes_dest() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.md");
    let dst = dir.path().join("dst.md");
    fs::write(&src, "# Keep\n\nStay.\n\n# Move\n\nGoing.\n").unwrap();
    fs::write(&dst, "# Target\n\nHere.\n").unwrap();

    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();

    let result = md_move_section(
        &src,
        "Move",
        ("after", "Target"),
        Some(&dst),
        ApplyMode::Apply,
        Some(&guard),
    )
    .unwrap();

    assert_eq!(result.path, src.to_string_lossy());
    assert_eq!(
        result.dest_path.as_deref(),
        Some(dst.to_string_lossy().as_ref())
    );
    let src_content = fs::read_to_string(&src).unwrap();
    let dst_content = fs::read_to_string(&dst).unwrap();
    assert!(
        !src_content.contains("# Move"),
        "section should be removed from source"
    );
    assert!(
        dst_content.contains("# Move"),
        "section should be inserted into destination"
    );
}

#[test]
fn md_move_section_cross_file_rejects_guard_on_destination() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.md");
    let dst = dir.path().join("dst.md");
    fs::write(&src, "# Keep\n\nStay.\n\n# Move\n\nGoing.\n").unwrap();
    fs::write(&dst, "# Target\n\nHere.\n").unwrap();

    // Strict Reject: absolute dst should be rejected by guard before writing dest.
    let guard = PathGuard::new(dir.path().to_path_buf(), AbsolutePathPolicy::Reject).unwrap();
    let abs_dst = dst.canonicalize().unwrap_or(dst);

    let err = md_move_section(
        &src,
        "Move",
        ("after", "Target"),
        Some(&abs_dst),
        ApplyMode::Apply,
        Some(&guard),
    )
    .unwrap_err();

    assert!(err.to_string().contains("path rejected by workspace guard"));
    // src should be untouched since dest was rejected first
    let src_content = fs::read_to_string(&src).unwrap();
    assert!(src_content.contains("# Move"));
}

#[test]
fn md_dedupe_headings_removes_duplicates() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\nBody 1.\n\n# Title\n\nBody 2.\n\n## Other\n\nKeep.\n",
    )
    .unwrap();

    let (result, removed) = md_dedupe_headings(&file, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    assert!(
        removed.iter().any(|r| r.contains("Title")),
        "should report removed duplicate heading"
    );
}

#[test]
fn md_lint_agents_finds_issues() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# Rules\n\nSome rules.\n\n# Rules\n\nDuplicate heading.\n",
    )
    .unwrap();

    let issues = md_lint_agents(&file).unwrap();
    assert!(
        !issues.is_empty(),
        "should find duplicate heading lint issue"
    );
}

#[test]
fn tidy_normalizes_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    fs::write(&file, "line1  \nline2\t \n\n\n\nline3").unwrap();

    let opts = WritePolicyOptions {
        trim_trailing_whitespace: true,
        ensure_final_newline: true,
        collapse_blanks: true,
        ..Default::default()
    };
    let result = tidy(&file, &opts, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    assert!(
        !result.new_content.contains("  \n"),
        "trailing whitespace should be removed"
    );
    assert!(
        result.new_content.ends_with('\n'),
        "should end with newline"
    );
    // Consecutive blank lines should be collapsed.
    assert!(
        !result.new_content.contains("\n\n\n"),
        "should collapse consecutive blanks"
    );
}

#[test]
fn search_finds_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\nfn alpha_beta() {}\n").unwrap();

    let matches = search(&file, "alpha", false, false).unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].line_number, 1);
    assert_eq!(matches[1].line_number, 3);
}

#[test]
fn search_regex_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "version = 1\nname = test\nversion = 2\n").unwrap();

    let matches = search(&file, r"version = \d+", true, false).unwrap();
    assert_eq!(matches.len(), 2);
}

#[test]
fn read_full_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let content = read(&file, None, None).unwrap();
    assert_eq!(content, "line1\nline2\nline3\n");
}

#[test]
fn read_line_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nline3\nline4\n").unwrap();

    let content = read(&file, Some(2), Some(3)).unwrap();
    assert_eq!(content, "line2\nline3\n");
}

#[test]
fn search_empty_pattern_returns_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();
    let err = search(&file, "", false, false).unwrap_err();
    assert!(
        err.to_string().contains("empty"),
        "expected empty pattern error, got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn search_invalid_regex_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();
    let err = search(&file, "(unclosed", true, false).unwrap_err();
    assert!(
        err.to_string().contains("regex parse error"),
        "expected regex parse error, got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn replace_in_content_invalid_regex_is_invalid_input() {
    let opts = ReplaceOptions {
        regex: true,
        ..Default::default()
    };
    let err = replace::replace_in_content("hello\n", "(unclosed", "x", &opts).unwrap_err();
    assert!(err.to_string().contains("regex parse error"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn search_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Hello World\nhello world\nHELLO\n").unwrap();
    let matches = search(&file, "hello", false, true).unwrap();
    assert_eq!(
        matches.len(),
        3,
        "case-insensitive should match all 3 lines"
    );
}

#[test]
fn replace_text_whole_line_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "keep this\nremove me\nalso keep\n").unwrap();

    let opts = ReplaceOptions {
        whole_line: true,
        ..Default::default()
    };
    let result = replace_text(&file, "remove", "", &opts, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    assert!(!result.new_content.contains("remove me"));
    assert!(result.new_content.contains("keep this"));
}

#[test]
fn replace_text_whole_line_with_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "a\nb\nc\nb\ne\n").unwrap();

    let opts = ReplaceOptions {
        whole_line: true,
        range: Some((1, Some(3))),
        ..Default::default()
    };
    let result = replace_text(&file, "b", "", &opts, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    // Only the 'b' on line 2 (within range 1:3) should be removed.
    // The 'b' on line 4 should remain.
    let lines: Vec<&str> = result.new_content.lines().collect();
    assert!(
        lines.contains(&"b"),
        "b outside range should remain: {:?}",
        lines
    );
}

#[test]
fn replace_text_if_exists_no_error_on_miss() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let opts = ReplaceOptions {
        if_exists: true,
        ..Default::default()
    };
    let result = replace_text(&file, "missing", "x", &opts, ApplyMode::Preview, None).unwrap();
    assert!(!result.changed);
    assert!(!result.applied);
}

#[test]
fn replace_text_range_requires_whole_line() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let opts = ReplaceOptions {
        range: Some((1, Some(5))),
        ..Default::default()
    };
    let err = replace_text(&file, "hello", "x", &opts, ApplyMode::Preview, None).unwrap_err();
    assert!(err.to_string().contains("range requires whole_line"));
}

// Static assertions: all public API types must be Send + Sync.
const _: () = {
    fn _assert<T: Send + Sync>() {}
    let _ = _assert::<EditResult>;
    let _ = _assert::<ApplyMode>;
    let _ = _assert::<ReplaceOptions>;
    let _ = _assert::<WritePolicyOptions>;
    let _ = _assert::<EolMode>;
};

#[test]
fn concurrent_edits_to_different_files() {
    let dir = TempDir::new().unwrap();
    let num_threads = 8;

    // Create files for each thread (start at 1 so i*100 always differs).
    for i in 0..num_threads {
        let file = dir.path().join(format!("file_{i}.json"));
        fs::write(&file, format!(r#"{{"value": {}}}"#, i + 1)).unwrap();
    }

    // Edit all files concurrently.
    std::thread::scope(|s| {
        let dir_path = dir.path();
        for i in 0..num_threads {
            s.spawn(move || {
                let file = dir_path.join(format!("file_{i}.json"));
                let result = doc_set(
                    &file,
                    "value",
                    serde_json::json!(i * 100 + 999),
                    ApplyMode::Apply,
                    None,
                )
                .unwrap();
                assert!(result.changed);
                assert!(result.applied);
            });
        }
    });

    // Verify all files were updated correctly.
    for i in 0..num_threads {
        let file = dir.path().join(format!("file_{i}.json"));
        let content = fs::read_to_string(&file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["value"], serde_json::json!(i * 100 + 999));
    }
}

#[test]
fn concurrent_backup_sessions_no_collision() {
    use crate::backup::{BackupSession, list_sessions};

    let dir = TempDir::new().unwrap();
    let num_threads = 16;

    // Create a file for each thread to back up.
    for i in 0..num_threads {
        let file = dir.path().join(format!("backup_{i}.txt"));
        fs::write(&file, format!("original_{i}")).unwrap();
    }

    // Create backup sessions concurrently.
    let timestamps: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    std::thread::scope(|s| {
        let dir_path = dir.path();
        let ts_ref = &timestamps;
        for i in 0..num_threads {
            s.spawn(move || {
                let file = dir_path.join(format!("backup_{i}.txt"));
                let mut session = BackupSession::new(dir_path).unwrap();
                session.save_before_write(&file).unwrap();
                let ts = session.finalize().unwrap().unwrap();
                ts_ref.lock().unwrap().push(ts);
            });
        }
    });

    let ts = timestamps.into_inner().unwrap();
    // All timestamps must be unique (no collisions).
    let mut sorted = ts.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        num_threads,
        "all backup session timestamps must be unique"
    );

    // All sessions should be listed.
    let sessions = list_sessions(dir.path()).unwrap();
    assert_eq!(sessions.len(), num_threads);
}

#[test]
fn concurrent_replace_text_different_files() {
    let dir = TempDir::new().unwrap();
    let num_threads = 8;

    for i in 0..num_threads {
        let file = dir.path().join(format!("code_{i}.rs"));
        fs::write(&file, format!("fn func_{i}() {{}}\n")).unwrap();
    }

    std::thread::scope(|s| {
        let dir_path = dir.path();
        for i in 0..num_threads {
            s.spawn(move || {
                let file = dir_path.join(format!("code_{i}.rs"));
                let result = replace_text(
                    &file,
                    &format!("func_{i}"),
                    &format!("renamed_{i}"),
                    &ReplaceOptions::default(),
                    ApplyMode::Apply,
                    None,
                )
                .unwrap();
                assert!(result.changed);
                assert!(result.applied);
            });
        }
    });

    for i in 0..num_threads {
        let file = dir.path().join(format!("code_{i}.rs"));
        let content = fs::read_to_string(&file).unwrap();
        assert!(
            content.contains(&format!("renamed_{i}")),
            "file {i} should contain renamed_{i}, got: {content}"
        );
    }
}

#[test]
fn concurrent_md_operations() {
    let dir = TempDir::new().unwrap();
    let num_threads = 4;

    for i in 0..num_threads {
        let file = dir.path().join(format!("doc_{i}.md"));
        fs::write(&file, format!("# Section {i}\n\nOriginal body.\n")).unwrap();
    }

    std::thread::scope(|s| {
        let dir_path = dir.path();
        for i in 0..num_threads {
            s.spawn(move || {
                let file = dir_path.join(format!("doc_{i}.md"));
                let result = md_replace_section(
                    &file,
                    &format!("Section {i}"),
                    &format!("Updated body {i}.\n"),
                    ApplyMode::Apply,
                    None,
                )
                .unwrap();
                assert!(result.changed);
                assert!(result.applied);
            });
        }
    });

    for i in 0..num_threads {
        let file = dir.path().join(format!("doc_{i}.md"));
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains(&format!("Updated body {i}.")));
    }
}

#[test]
fn doc_get_zero_match_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"version": "1.0"}"#).unwrap();

    let err = doc_get(&file, "nonexistent").unwrap_err();
    assert!(
        err.downcast_ref::<crate::exit::NoMatchError>().is_some(),
        "expected NoMatchError, got: {err}"
    );
}

#[test]
fn doc_get_multi_match_returns_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [{"name": "a"}, {"name": "b"}]}"#).unwrap();

    let value = doc_get(&file, "items[*].name").unwrap();
    assert_eq!(value, serde_json::json!(["a", "b"]));
}

#[test]
fn replace_text_regex_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "version = 1\nversion = 2\n").unwrap();

    let opts = ReplaceOptions {
        regex: true,
        ..ReplaceOptions::default()
    };
    let result = replace_text(
        &file,
        r"version = \d+",
        "version = 99",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();

    assert!(result.changed);
    // Regex replaces all matches
    assert_eq!(
        result.new_content.matches("version = 99").count(),
        2,
        "regex should replace all occurrences"
    );
}

#[test]
fn replace_text_nth_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "foo bar foo baz foo\n").unwrap();

    let opts = ReplaceOptions {
        nth: Some(2),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "foo", "qux", &opts, ApplyMode::Preview, None).unwrap();

    assert!(result.changed);
    assert_eq!(result.new_content, "foo bar qux baz foo\n");
}

#[test]
fn replace_text_insert_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "hello world\n").unwrap();

    let opts = ReplaceOptions {
        insert_after: Some(" beautiful".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "hello", "", &opts, ApplyMode::Preview, None).unwrap();

    assert!(result.changed);
    assert!(
        result.new_content.contains("hello beautiful"),
        "insert_after should add text after match: {}",
        result.new_content
    );
}

#[test]
fn concurrent_file_create() {
    let dir = TempDir::new().unwrap();
    let num_threads = 8;

    std::thread::scope(|s| {
        let dir_path = dir.path();
        for i in 0..num_threads {
            s.spawn(move || {
                let file = dir_path.join(format!("new_{i}.txt"));
                let result = file_create(
                    &file,
                    &format!("content_{i}\n"),
                    false,
                    ApplyMode::Apply,
                    None,
                )
                .unwrap();
                assert!(result.applied);
            });
        }
    });

    for i in 0..num_threads {
        let file = dir.path().join(format!("new_{i}.txt"));
        assert!(file.exists());
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, format!("content_{i}\n"));
    }
}

// --- api::search gap tests ---

#[test]
fn search_literal_returns_correct_line_numbers() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\nfn alpha_2() {}\n").unwrap();

    let matches = search(&file, "alpha", false, false).unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].line_number, 1);
    assert_eq!(matches[1].line_number, 3);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_basic() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("test.rs");
    fs::write(&f, "fn foo() {}\nfn bar() {}\nlet x = foo();\n").unwrap();

    let opts = SearchOptions::default();
    let results = search_directory(dir.path(), "foo", &opts).unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|r| r.line.contains("foo")));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_with_context_and_globs() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "fn main() { foo(); }\n// comment\nlet x = foo();\n",
    )
    .unwrap();
    std::fs::write(src.join("lib.txt"), "foo in text\n").unwrap(); // should be skipped by glob

    let opts = SearchOptions {
        context: Some(1),
        globs: vec!["*.rs".to_string()],
        ..Default::default()
    };
    let results = search_directory(dir.path(), "foo", &opts).unwrap();
    // multi-match per file now supported (addresses #779)
    assert_eq!(results.len(), 2);
    // first match "foo(); }" on line 1, no before, 1 after
    assert!(results[0].line.contains("foo()"));
    assert_eq!(results[0].context_before.len(), 0);
    assert_eq!(results[0].context_after, vec!["// comment".to_string()]);
    assert_eq!(results[0].column, 13); // position of 'f' in "foo();"
    // second match
    assert!(results[1].line.contains("foo()"));
    assert_eq!(results[1].context_before, vec!["// comment".to_string()]);
    assert_eq!(results[1].context_after.len(), 0);
    assert_eq!(results[1].column, 9);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_max_results_and_case() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a1.txt"), "Foo one\n").unwrap();
    std::fs::write(dir.path().join("a2.txt"), "foo two\n").unwrap();
    std::fs::write(dir.path().join("a3.txt"), "FOO three\n").unwrap();

    let opts = SearchOptions {
        case_insensitive: true,
        max_results: 2,
        ..Default::default()
    };
    let results = search_directory(dir.path(), "foo", &opts).unwrap();
    assert_eq!(results.len(), 2); // capped at 2 results (max_results limits matches)
    assert!(
        results
            .iter()
            .any(|r| r.line.contains("Foo") || r.line.contains("foo") || r.line.contains("FOO"))
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_empty_pattern_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "content\n").unwrap();
    let opts = SearchOptions::default();
    let err = search_directory(dir.path(), "", &opts).unwrap_err();
    assert!(err.to_string().contains("must not be empty"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_invalid_regex_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    // Must not soft-succeed with empty hits (agent-hostile "no matches").
    let err = search_directory(dir.path(), "(unclosed", &opts).unwrap_err();
    assert!(
        err.to_string().contains("regex parse error"),
        "expected regex parse error, got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_file_invalid_regex_errors() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let opts = SearchOptions {
        regex: true,
        ..Default::default()
    };
    let err = search_file(&file, "(unclosed", &opts).unwrap_err();
    assert!(err.to_string().contains("regex parse error"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_invalid_glob_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("f.txt"),
        "content
",
    )
    .unwrap();
    let opts = SearchOptions {
        globs: vec!["[unclosed".to_string()],
        ..Default::default()
    };
    let err = search_directory(dir.path(), "foo", &opts).unwrap_err();
    // exact message from glob parse (exercises build_glob_matcher error path)
    assert!(err.to_string().contains("parsing glob") || err.to_string().contains("unclosed"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_directory_exclude_patterns_and_custom_ignore() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("keep.txt"), "keep this\n").unwrap();
    std::fs::write(dir.path().join("ignore.txt"), "ignore me\n").unwrap();
    std::fs::write(dir.path().join("bline.txt"), "bline secret\n").unwrap();

    // Create a custom ignore file (blineignore style)
    std::fs::write(dir.path().join(".blineignore"), "bline.txt\n").unwrap();

    let opts = SearchOptions {
        regex: true,
        exclude_patterns: vec!["*ignore*".to_string()],
        custom_ignore_filenames: vec![".blineignore".to_string()],
        ..Default::default()
    };
    let results = search_directory(dir.path(), "keep|me|secret", &opts).unwrap();
    // Only the keep file should match; excludes + custom ignore filtered the rest.
    assert_eq!(results.len(), 1);
    assert!(results[0].line.contains("keep"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_parity_blineignore_across_api_cli_and_plan() {
    // Cross-surface parity test for #821: same inputs via api, CLI collect, and tx plan Search
    // produce equivalent rich results (counts/paths) when using custom ignore + exclude + globs + max.
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.rs"), "keep foo here\n").unwrap();
    std::fs::write(root.join("skip.rs"), "skip foo\n").unwrap();
    std::fs::write(root.join("bline.txt"), "bline foo secret\n").unwrap();
    std::fs::write(root.join("other.txt"), "other foo\n").unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("target/bad.rs"), "bad foo\n").unwrap();
    std::fs::write(root.join(".blineignore"), "bline.txt\ntarget/\n").unwrap();
    std::fs::write(root.join(".gitignore"), "").unwrap();

    let pattern = "foo";
    let opts = SearchOptions {
        literal: true,
        globs: vec!["*.rs".to_string()],
        exclude_patterns: vec!["*skip*".to_string()],
        custom_ignore_filenames: vec![".blineignore".to_string()],
        max_results: 10,
        ..Default::default()
    };

    // 1. API
    let api_res = search_directory(root, pattern, &opts).unwrap();
    assert!(!api_res.is_empty(), "api should find keep");
    let api_paths: Vec<_> = api_res
        .iter()
        .map(|r| r.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // 2. CLI collect path (via GlobalFlags + collect_matches) -- only when cli feature present
    #[cfg(feature = "cli")]
    let cli_total: usize = {
        let mut g = crate::cli::global::GlobalFlags::test_default();
        g.cwd = Some(root.to_string_lossy().into_owned());
        g.glob = opts.globs.clone();
        g.exclude = opts.exclude_patterns.clone();
        g.ignore_file = opts.custom_ignore_filenames.clone();
        let cli_args = crate::cmd::search::SearchArgs {
            pattern: pattern.to_string(),
            paths: vec![".".to_string()],
            literal: true,
            regex: false,
            context: None,
            before_context: None,
            after_context: None,
            files_with_matches: false,
            count: false,
            invert_match: false,
            multiline: false,
            case_insensitive: false,
            assert_count: None,
            max_results: opts.max_results,
        };
        let cli_results = crate::cmd::search::collect_matches(&cli_args, &g).unwrap();
        cli_results.file_match_counts.values().sum()
    };
    #[cfg(not(feature = "cli"))]
    let _cli_total: usize = api_res.len(); // fallback for pure-files matrix

    // 3. Plan / tx using execute_plan (library path) with Search op carrying new fields
    let plan_json = format!(
        r#"{{
        "version": 1,
        "operations": [{{
            "op": "search",
            "path": ".",
            "pattern": "{}",
            "literal": true,
            "globs": ["*.rs"],
            "exclude_patterns": ["*skip*"],
            "custom_ignore_filenames": [".blineignore"],
            "max_results": 10
        }}]
    }}"#,
        pattern
    );
    let plan = crate::plan::parse_plan_auto(&plan_json, None, None).unwrap();
    let report = crate::api::execute_plan(plan, root, None).expect("plan exec with search");
    let plan_total: usize = report.searches.iter().map(|s| s.match_count).sum();

    // Parity checks (using counts + that api found the keep file)
    assert!(
        api_paths.iter().any(|p| p == "keep.rs"),
        "api did not find keep: {:?}",
        api_paths
    );
    #[cfg(feature = "cli")]
    assert!(cli_total > 0, "cli should have matches");
    assert!(plan_total > 0, "plan should have recorded matches");
    // Ensure no leaked bad files in api results
    assert!(
        !api_paths.iter().any(|p| p.contains("bline")
            || p.contains("skip")
            || p.contains("target")
            || p.contains("bad")),
        "api leaked ignored files: {:?}",
        api_paths
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_file_and_format_and_context_builder_direct() {
    // Direct exercise of new public surface (#812 #815).
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("x.rs");
    fs::write(&f, "fn foo() {}\nlet y = foo();\n").unwrap();

    let opts = SearchOptions {
        context: Some(1),
        ..Default::default()
    };
    let res = search_file(&f, "foo", &opts).unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].column, 4); // 'f' in fn foo
    assert!(!res[0].context_after.is_empty() || !res[1].context_before.is_empty());

    // Direct low-level matcher (for custom walkers) - #812 follow-up
    let res2 = search_one_file(&f, "foo", &opts, dir.path());
    assert_eq!(res2.len(), 2);
    assert_eq!(res2[0].column, 4);

    // context builder directly
    let lines: Vec<&str> = "a\nb\nc\n".lines().collect();
    let (b, a) = build_context_lines(&lines, 1, 1, 1);
    assert_eq!(b, vec!["a".to_string()]);
    assert_eq!(a, vec!["c".to_string()]);
}

#[test]
fn build_context_lines_asymmetric() {
    let lines: Vec<&str> = "aaa\nbbb\nccc\nddd\neee".lines().collect();
    // 0 before, 2 after
    let (b, a) = build_context_lines(&lines, 2, 0, 2);
    assert!(b.is_empty());
    assert_eq!(a, vec!["ddd".to_string(), "eee".to_string()]);
    // 1 before, 0 after
    let (b, a) = build_context_lines(&lines, 2, 1, 0);
    assert_eq!(b, vec!["bbb".to_string()]);
    assert!(a.is_empty());
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_asymmetric_context_more_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(
        &file,
        "// line 1\n// line 2\nfn main() {\n    println!(\"hello\");\n    return;\n}\n// line 7\n",
    )
    .unwrap();

    let opts = SearchOptions {
        before_context: Some(1),
        after_context: Some(3),
        ..Default::default()
    };
    let results = search_one_file(&file, "fn main", &opts, dir.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].context_before.len(), 1);
    assert_eq!(results[0].context_after.len(), 3);
    assert!(results[0].context_before[0].contains("line 2"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_asymmetric_context_zero_before() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\nccc\nddd\neee\n").unwrap();

    let opts = SearchOptions {
        before_context: Some(0),
        after_context: Some(2),
        ..Default::default()
    };
    let results = search_one_file(&file, "ccc", &opts, dir.path());
    assert_eq!(results.len(), 1);
    assert!(results[0].context_before.is_empty());
    assert_eq!(results[0].context_after.len(), 2);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_asymmetric_overrides_symmetric() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\nccc\nddd\neee\n").unwrap();

    let opts = SearchOptions {
        context: Some(2),
        before_context: Some(0),
        after_context: Some(1),
        ..Default::default()
    };
    let results = search_one_file(&file, "ccc", &opts, dir.path());
    assert!(results[0].context_before.is_empty());
    assert_eq!(results[0].context_after.len(), 1);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_symmetric_context_still_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\nccc\nddd\neee\n").unwrap();

    let opts = SearchOptions {
        context: Some(1),
        ..Default::default()
    };
    let results = search_one_file(&file, "ccc", &opts, dir.path());
    assert_eq!(results[0].context_before.len(), 1);
    assert_eq!(results[0].context_after.len(), 1);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_invert_match_excludes_matching_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.py");
    fs::write(
        &file,
        "import os\nimport sys\ndef hello():\n    pass\nimport json\n",
    )
    .unwrap();

    let opts = SearchOptions {
        invert_match: true,
        ..Default::default()
    };
    let results = search_one_file(&file, "import", &opts, dir.path());
    assert!(results.iter().all(|r| !r.line.contains("import")));
    assert!(results.iter().any(|r| r.line.contains("def hello")));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_invert_match_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\naaa\nbbb\naaa\n").unwrap();

    let opts = SearchOptions {
        invert_match: true,
        ..Default::default()
    };
    let results = search_one_file(&file, "aaa", &opts, dir.path());
    assert_eq!(results.len(), 2); // only the two "bbb" lines
    assert!(results.iter().all(|r| r.line == "bbb"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_multiline_pattern_spans_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "struct Foo {\n    x: i32,\n    y: String,\n}\n").unwrap();

    let opts = SearchOptions {
        regex: true,
        multiline: true,
        ..Default::default()
    };
    let results = search_one_file(&file, r"struct Foo \{[^}]+\}", &opts, dir.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].line_number, 1);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_format_results_human_and_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("x.rs");
    fs::write(&file, "fn foo() {}\nfn bar() {}\n").unwrap();
    let opts = SearchOptions::default();
    let res = search_one_file(&file, "fn", &opts, dir.path());

    // formatter (human + json)
    let txt = format_search_results(&res, false);
    assert!(
        txt.contains("foo") && txt.contains("x.rs"),
        "human format must include path and match text: {txt:?}"
    );
    let js = format_search_results(&res, true);
    assert!(!js.trim().is_empty(), "json format must not be empty");
    let parsed: serde_json::Value =
        serde_json::from_str(js.trim()).expect("json format must be valid JSON");
    let arr = parsed
        .as_array()
        .expect("json format must be a top-level array");
    assert!(!arr.is_empty(), "json format must include matches");
    assert!(
        arr[0].get("column").is_some(),
        "each match must have column: {arr:?}"
    );
    assert_eq!(
        arr[0].get("text").and_then(|v| v.as_str()),
        Some("fn foo() {}"),
        "first match text: {arr:?}"
    );
}

/// Regression: format_search_results must print context_before lines
/// BEFORE the match line, not after.
#[test]
fn search_format_results_context_before_appears_before_match() {
    let results = vec![SearchResult {
        path: std::path::PathBuf::from("file.rs"),
        line_number: 5,
        line: "fn handle_error()".to_string(),
        column: 0,
        context_before: vec!["fn validate_input() {".to_string()],
        context_after: vec!["    Ok(())".to_string()],
    }];
    let txt = format_search_results(&results, false);
    let before_pos = txt.find("validate_input").expect("context_before missing");
    let match_pos = txt.find("handle_error").expect("match line missing");
    let after_pos = txt.find("Ok(())").expect("context_after missing");
    assert!(
        before_pos < match_pos,
        "context_before must appear before match line"
    );
    assert!(
        match_pos < after_pos,
        "context_after must appear after match line"
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_multiline_literal_auto_escapes_metacharacters() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Pattern "foo.bar" should match literally, not as regex foo<any>bar
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    let opts = SearchOptions {
        multiline: true,
        regex: false,
        ..Default::default()
    };
    let results = search_one_file(&file, "foo.bar", &opts, dir.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].line, "foo.bar");
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_multiline_regex_does_not_escape() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    let opts = SearchOptions {
        multiline: true,
        regex: true,
        ..Default::default()
    };
    // With regex: true, "foo.bar" matches both lines (dot = any char)
    let results = search_one_file(&file, "foo.bar", &opts, dir.path());
    assert_eq!(results.len(), 2);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn collect_with_ignores_direct() {
    // Direct test of #813 helper.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("keep.txt"), "keep\n").unwrap();
    fs::write(dir.path().join("skip.txt"), "skip\n").unwrap();
    fs::write(dir.path().join(".myignore"), "skip.txt\n").unwrap();

    let custom_ignores = vec![".myignore".to_string()];
    let exclude_patterns = vec!["*skip*".to_string()];
    let paths = crate::files::collect_file_paths_with_ignores(
        dir.path(),
        &custom_ignores,
        &exclude_patterns,
        false,
    )
    .unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("keep.txt"));
}

#[test]
fn search_no_match_returns_empty() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn hello() {}\n").unwrap();

    let matches = search(&file, "nonexistent", false, false).unwrap();
    assert!(matches.is_empty());
}

#[test]
fn file_append_and_prepend_basic() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "hello").unwrap();

    // append
    let res = file_append(&file, " world", ApplyMode::Apply, None).unwrap();
    assert!(res.changed);
    assert!(res.applied);
    assert_eq!(res.action, "append");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n world");

    // prepend (newline separator added because ">> " lacks trailing \n)
    let res2 = file_prepend(&file, ">> ", ApplyMode::Apply, None).unwrap();
    assert!(res2.changed);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        ">> \nhello\n world"
    );
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn file_append_prepend_empty_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    std::fs::write(&file, "").unwrap();

    let res = file_append(&file, "first", ApplyMode::Apply, None).unwrap();
    assert!(res.changed);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "first");

    let res2 = file_prepend(&file, "zero\n", ApplyMode::Apply, None).unwrap();
    assert!(res2.changed);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "zero\nfirst");
}

#[test]
fn file_append_preview_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("b.txt");
    std::fs::write(&file, "x").unwrap();

    let res = file_append(&file, "y", ApplyMode::Preview, None).unwrap();
    assert!(res.changed);
    assert!(!res.applied);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x");
}

#[test]
fn file_append_check_reports_without_writing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.txt");
    std::fs::write(&file, "base").unwrap();

    let res = file_append(&file, " +more", ApplyMode::Check, None).unwrap();
    assert!(res.changed);
    assert!(!res.applied);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "base"); // no write
    assert!(res.new_content.contains("+more"));
}

#[test]
fn file_append_respects_guard_and_relaxed() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("g.txt");
    std::fs::write(&file, "base").unwrap();

    // strict guard rejects outside? but inside ok
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let res = file_append(&file, " +append", ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(res.applied);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "base\n +append");

    // relaxed yolo/temp
    let yolo = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();
    let res2 = file_append(&file, " +yolo", ApplyMode::Apply, Some(&yolo)).unwrap();
    assert!(res2.applied);
}

#[test]
fn search_nonexistent_file_fails() {
    let err = search(
        Path::new("/tmp/nonexistent_patchloom_search.txt"),
        "x",
        false,
        false,
    )
    .unwrap_err();
    assert!(err.to_string().contains("failed to read"));
}

// --- api::read gap tests ---

#[test]
fn read_start_only_to_end() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "a\nb\nc\n").unwrap();

    let content = read(&file, Some(2), None).unwrap();
    assert_eq!(content, "b\nc\n");
}

#[test]
fn read_start_beyond_file_returns_empty() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "a\nb\n").unwrap();

    let content = read(&file, Some(100), None).unwrap();
    assert!(content.is_empty());
}

#[test]
fn read_nonexistent_file_fails() {
    let err = read(Path::new("/tmp/nonexistent_patchloom_read.txt"), None, None).unwrap_err();
    assert!(err.to_string().contains("failed to read"));
}

// --- make_write_policy CRLF variant ---

#[test]
fn make_write_policy_maps_crlf() {
    let opts = WritePolicyOptions {
        normalize_eol: Some(EolMode::Crlf),
        ..WritePolicyOptions::default()
    };
    let policy = make_write_policy(&opts);
    assert_eq!(
        policy.normalize_eol,
        EolMode::Crlf,
        "should map EolMode::Crlf correctly"
    );
}

// ---------------------------------------------------------------------------
// TX engine adapter (execute_as_edit_result)
// ---------------------------------------------------------------------------

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_preview_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    let op = crate::plan::Operation::DocSet {
        path: file.to_string_lossy().into(),
        selector: "key".into(),
        value: serde_json::json!("new"),
    };

    let result =
        super::execute_as_edit_result(op, ApplyMode::Preview, dir.path(), None, "doc.set", None)
            .unwrap();

    assert!(result.changed, "content should differ");
    assert!(!result.applied, "preview should not write");
    assert!(result.new_content.contains("new"));
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("old"), "file should be unchanged on disk");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_apply_writes_to_disk() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    let op = crate::plan::Operation::DocSet {
        path: file.to_string_lossy().into(),
        selector: "key".into(),
        value: serde_json::json!("new"),
    };

    let result =
        super::execute_as_edit_result(op, ApplyMode::Apply, dir.path(), None, "doc.set", None)
            .unwrap();

    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("new"), "file should be updated on disk");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_check_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    let op = crate::plan::Operation::DocSet {
        path: file.to_string_lossy().into(),
        selector: "key".into(),
        value: serde_json::json!("new"),
    };

    let result =
        super::execute_as_edit_result(op, ApplyMode::Check, dir.path(), None, "doc.set", None)
            .unwrap();

    assert!(result.changed);
    assert!(!result.applied, "check should not write");
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("old"), "file should be unchanged on disk");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_respects_guard() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    // Use AllowIfContained policy so absolute temp paths work with the guard.
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();

    let op = crate::plan::Operation::DocSet {
        path: file.to_string_lossy().into(),
        selector: "key".into(),
        value: serde_json::json!("new"),
    };

    let result = super::execute_as_edit_result(
        op,
        ApplyMode::Apply,
        dir.path(),
        Some(&guard),
        "doc.set",
        None,
    )
    .unwrap();

    assert!(result.applied);
    assert!(result.changed);
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_guard_rejects_outside_path() {
    let dir = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let file = other.path().join("test.json");
    fs::write(&file, r#"{"key": "old"}"#).unwrap();

    let guard = PathGuard::builder(dir.path().to_path_buf())
        .build()
        .unwrap();

    let op = crate::plan::Operation::DocSet {
        path: file.to_string_lossy().into(),
        selector: "key".into(),
        value: serde_json::json!("new"),
    };

    let err = super::execute_as_edit_result(
        op,
        ApplyMode::Apply,
        dir.path(),
        Some(&guard),
        "doc.set",
        None,
    );

    assert!(err.is_err(), "guard should reject path outside workspace");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn adapter_unchanged_returns_no_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Use a replace operation with a pattern that doesn't match
    // to test the no-change path cleanly.
    fs::write(&file, "hello world\n").unwrap();

    let op = crate::plan::Operation::Replace {
        path: Some(file.to_string_lossy().into()),
        glob: None,
        regex: false,
        old: "nonexistent_pattern".into(),
        new_text: Some("replacement".into()),
        nth: None,
        case_insensitive: false,
        insert_before: None,
        insert_after: None,
        whole_line: false,
        multiline: false,
        range: None,
        if_exists: true,
        word_boundary: false,
        before_context: None,
        after_context: None,
        unique: false,
        require_change: false,
        command_position: false,
    };

    let result =
        super::execute_as_edit_result(op, ApplyMode::Preview, dir.path(), None, "replace", None)
            .unwrap();

    assert!(
        !result.changed,
        "should not be changed when pattern doesn't match"
    );
    assert!(!result.applied);
}

// ── replace_in_content tests ──────────────────────────────────

#[test]
fn replace_in_content_literal() {
    let content = "fn hello() {}\nfn world() {}\n";
    let result =
        replace::replace_in_content(content, "hello", "greet", &ReplaceOptions::default()).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("fn greet()"));
    assert!(!result.new_content.contains("fn hello()"));
}

#[test]
fn replace_in_content_regex() {
    let content = "version = \"1.2.3\"\n";
    let opts = ReplaceOptions {
        regex: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        r#"version = "\d+\.\d+\.\d+""#,
        "version = \"2.0.0\"",
        &opts,
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("2.0.0"));
}

#[test]
fn replace_in_content_word_boundary() {
    let content = "let setup = setup_config();\n";
    let opts = ReplaceOptions {
        word_boundary: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "setup", "init", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.new_content, "let init = setup_config();\n");
}

#[test]
fn replace_in_content_nth() {
    let content = "aaa bbb aaa bbb aaa\n";
    let opts = ReplaceOptions {
        nth: Some(2),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "aaa", "xxx", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.new_content, "aaa bbb xxx bbb aaa\n");
}

#[test]
fn replace_in_content_insert_after() {
    let content = "use std::io;\n\nfn main() {}\n";
    let opts = ReplaceOptions {
        insert_after: Some("\nuse std::fs;".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "use std::io;", "", &opts).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("use std::io;\nuse std::fs;"));
}

#[test]
fn replace_in_content_insert_before() {
    let content = "use std::io;\n\nfn main() {}\n";
    let opts = ReplaceOptions {
        insert_before: Some("use std::fs;\n".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "use std::io;", "", &opts).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("use std::fs;\nuse std::io;"));
}

#[test]
fn replace_in_content_no_match_unchanged() {
    let content = "fn hello() {}\n";
    let result =
        replace::replace_in_content(content, "nonexistent", "x", &ReplaceOptions::default())
            .unwrap();
    assert!(!result.changed);
    assert_eq!(result.new_content, content);
}

#[test]
fn replace_in_content_whole_line() {
    let content = "line one\nremove this\nline three\n";
    let opts = ReplaceOptions {
        whole_line: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "remove this", "", &opts).unwrap();
    assert!(result.changed);
    assert!(!result.new_content.contains("remove this"));
    assert!(result.new_content.contains("line one"));
    assert!(result.new_content.contains("line three"));
}

#[test]
fn replace_in_content_range() {
    let content = "aaa\nbbb\naaa\nbbb\naaa\n";
    let opts = ReplaceOptions {
        whole_line: true,
        range: Some((2, Some(4))),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "aaa", "xxx", &opts).unwrap();
    assert!(result.changed);
    // Only the "aaa" on line 3 (within range 2-4) should be replaced
    let lines: Vec<&str> = result.new_content.lines().collect();
    assert_eq!(lines[0], "aaa"); // line 1, outside range
    assert_eq!(lines[2], "xxx"); // line 3, inside range
    assert_eq!(lines[4], "aaa"); // line 5, outside range
}

#[test]
fn replace_in_content_if_exists_no_match() {
    let content = "fn hello() {}\n";
    let opts = ReplaceOptions {
        if_exists: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "nonexistent", "x", &opts).unwrap();
    assert!(!result.changed);
    assert_eq!(result.new_content, content);
}

#[test]
fn replace_in_content_case_insensitive() {
    let content = "Hello World\n";
    let opts = ReplaceOptions {
        case_insensitive: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "hello", "Hi", &opts).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("Hi World"));
}

#[test]
fn replace_in_content_empty_pattern_errors() {
    let result = replace::replace_in_content("content", "", "x", &ReplaceOptions::default());
    assert!(result.is_err(), "expected error, got Ok: {result:?}");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("empty search pattern"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn replace_in_content_diff_is_populated() {
    let content = "old text\n";
    let result =
        replace::replace_in_content(content, "old", "new", &ReplaceOptions::default()).unwrap();
    assert!(result.changed);
    assert!(!result.diff.is_empty());
    assert!(result.diff.contains("-old text"));
    assert!(result.diff.contains("+new text"));
}

#[test]
fn replace_in_content_range_requires_whole_line() {
    let opts = ReplaceOptions {
        range: Some((1, Some(3))),
        ..Default::default()
    };
    let result = replace::replace_in_content("aaa\nbbb\n", "aaa", "xxx", &opts);
    assert!(result.is_err(), "expected error, got Ok: {result:?}");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("range requires whole_line"));
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn replace_in_content_whole_line_multiline_conflict() {
    let opts = ReplaceOptions {
        whole_line: true,
        multiline: true,
        ..Default::default()
    };
    let result = replace::replace_in_content("aaa\nbbb\n", "aaa", "xxx", &opts);
    assert!(result.is_err(), "expected error, got Ok: {result:?}");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("whole_line and multiline cannot be combined")
    );
}

// --- #1264: text_diff API ---

#[test]
fn text_diff_identical_returns_empty() {
    let result = text_diff("hello\nworld\n", "hello\nworld\n", None);
    assert!(result.is_empty());
}

#[test]
fn text_diff_with_changes_returns_unified_diff() {
    let result = text_diff("hello\nworld\n", "hello\nearth\n", None);
    assert!(result.contains("-world"));
    assert!(result.contains("+earth"));
    assert!(result.contains("--- a/<content>"));
    assert!(result.contains("+++ b/<content>"));
}

#[test]
fn text_diff_with_custom_path() {
    let result = text_diff("old\n", "new\n", Some("src/main.rs"));
    assert!(result.contains("--- a/src/main.rs"));
    assert!(result.contains("+++ b/src/main.rs"));
}

#[test]
fn text_diff_absolute_path_no_double_slash_headers() {
    // #1480: embedders pass absolute paths after canonicalize (macOS /private/tmp).
    let result = text_diff("old\n", "new\n", Some("/tmp/demo/lib.rs"));
    assert!(
        !result.contains("--- a//") && !result.contains("+++ b//"),
        "absolute path must not produce double-slash headers: {result}"
    );
    assert!(result.contains("--- a/tmp/demo/lib.rs"));
    assert!(result.contains("+++ b/tmp/demo/lib.rs"));
}

#[test]
fn text_diff_absolute_path_placeholder_and_relative_unchanged() {
    let abs = text_diff("a\n", "b\n", Some("/private/tmp/x.rs"));
    assert!(!abs.contains("--- a//") && !abs.contains("+++ b//"));
    assert!(abs.contains("--- a/private/tmp/x.rs"));

    let multi_slash = text_diff("a\n", "b\n", Some("///weird/path.rs"));
    assert!(!multi_slash.contains("--- a//") && !multi_slash.contains("+++ b//"));
    assert!(multi_slash.contains("--- a/weird/path.rs"));

    let rel = text_diff("a\n", "b\n", Some("src/main.rs"));
    assert!(rel.contains("--- a/src/main.rs"));
    assert!(rel.contains("+++ b/src/main.rs"));

    let none = text_diff("a\n", "b\n", None);
    assert!(none.contains("--- a/<content>"));
    assert!(none.contains("+++ b/<content>"));
}

#[test]
fn text_diff_empty_original() {
    let result = text_diff("", "new content\n", Some("file.txt"));
    assert!(!result.is_empty());
    assert!(result.contains("+new content"));
}

#[test]
fn text_diff_empty_modified() {
    let result = text_diff("old content\n", "", Some("file.txt"));
    assert!(!result.is_empty());
    assert!(result.contains("-old content"));
}

// --- #1265: match_count on ContentEditResult ---

#[test]
fn replace_in_content_match_count_single() {
    let result = replace::replace_in_content(
        "hello world\n",
        "hello",
        "goodbye",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert_eq!(result.match_count, 1);
    assert!(result.changed);
}

#[test]
fn replace_in_content_match_count_multiple() {
    let result = replace::replace_in_content(
        "aaa bbb aaa ccc aaa\n",
        "aaa",
        "xxx",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert_eq!(result.match_count, 3);
    assert!(result.changed);
}

#[test]
fn replace_in_content_match_count_zero() {
    let opts = ReplaceOptions {
        if_exists: true,
        ..Default::default()
    };
    let result = replace::replace_in_content("hello world\n", "missing", "x", &opts).unwrap();
    assert_eq!(result.match_count, 0);
    assert!(!result.changed);
}

#[test]
fn replace_in_content_match_count_with_nth() {
    let opts = ReplaceOptions {
        nth: Some(2),
        ..Default::default()
    };
    // nth replaces only the 2nd match, but match_count reflects how many
    // matches the ops layer found (1 for the nth path, since it stops early).
    let result = replace::replace_in_content("aaa bbb aaa\n", "aaa", "xxx", &opts).unwrap();
    assert!(result.changed);
    // nth=2 replaces exactly the 2nd occurrence. The ops layer returns count=1
    // (it found and replaced the nth match).
    assert_eq!(result.match_count, 1);
}

// --- #1265: unique mode ---

#[test]
fn replace_in_content_unique_single_match_succeeds() {
    let opts = ReplaceOptions {
        unique: true,
        ..Default::default()
    };
    let result = replace::replace_in_content("hello world\n", "hello", "goodbye", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_unique_multiple_matches_fails() {
    let opts = ReplaceOptions {
        unique: true,
        ..Default::default()
    };
    let err = replace::replace_in_content("aaa bbb aaa\n", "aaa", "xxx", &opts).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ambiguous match"),
        "expected ambiguous match error, got: {msg}"
    );
    assert!(
        msg.contains("2 times"),
        "should report match count, got: {msg}"
    );
}

#[test]
fn replace_in_content_unique_no_match_not_ambiguous() {
    // unique + if_exists: no matches should not trigger ambiguity error
    let opts = ReplaceOptions {
        unique: true,
        if_exists: true,
        ..Default::default()
    };
    let result = replace::replace_in_content("hello world\n", "missing", "x", &opts).unwrap();
    assert!(!result.changed);
    assert_eq!(result.match_count, 0);
}

#[test]
fn replace_in_content_unique_with_word_boundary() {
    let opts = ReplaceOptions {
        unique: true,
        word_boundary: true,
        ..Default::default()
    };
    // "set" appears as a word only once (not inside "reset")
    let result = replace::replace_in_content("set the reset\n", "set", "get", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 1);
}

// ── replace_in_content fuzzy fallback (#1286) ────────────────

#[test]
fn replace_in_content_fuzzy_resolves_typo() {
    // "proccess_data" is a typo for "process_data"; fuzzy should match
    // via Jaro-Winkler similarity (> 0.85).
    let content = "fn setup() {}\nfn process_data(x: i32) {}\nfn cleanup() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn proccess_data(x: i32) {}",
        "fn handle_data(x: i32) {}",
        &opts,
    )
    .unwrap();
    assert!(result.changed, "fuzzy should find a match for the typo");
    assert_eq!(result.match_count, 1);
    assert!(
        result.new_content.contains("handle_data"),
        "replacement should be applied: {}",
        result.new_content
    );
    assert!(
        !result.new_content.contains("process_data"),
        "original should be replaced: {}",
        result.new_content
    );
}

#[test]
fn replace_in_content_fuzzy_exact_match_preferred() {
    // When the exact match succeeds, fuzzy should not change behavior.
    let content = "fn hello() {}\nfn world() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        ..Default::default()
    };
    let result =
        replace::replace_in_content(content, "fn hello() {}", "fn greet() {}", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 1);
    assert!(result.new_content.contains("fn greet()"));
}

#[test]
fn replace_in_content_fuzzy_no_match_returns_error_with_hints() {
    // When fuzzy also fails, the error should include suggestions.
    let content = "fn alpha() {}\nfn beta() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        ..Default::default()
    };
    let err = replace::replace_in_content(
        content,
        "fn completely_unrelated_name_xyz() {}",
        "fn replaced() {}",
        &opts,
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no matches"),
        "error should say no matches: {msg}"
    );
}

#[test]
fn replace_in_content_fuzzy_with_if_exists_suppresses_error() {
    // fuzzy + if_exists: when fuzzy also fails, no error.
    let content = "fn alpha() {}\nfn beta() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        if_exists: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn completely_unrelated_name_xyz() {}",
        "fn replaced() {}",
        &opts,
    )
    .unwrap();
    assert!(!result.changed);
    assert_eq!(result.match_count, 0);
}

#[test]
fn replace_in_content_fuzzy_disabled_by_default() {
    // Default ReplaceOptions has fuzzy: false, so a near-miss should fail.
    let content = "fn process_data() {}\n";
    let result = replace::replace_in_content(
        content,
        "fn proccess_data() {}",
        "fn handle() {}",
        &ReplaceOptions::default(),
    )
    .unwrap();
    // Without fuzzy, the typo doesn't match and count == 0.
    assert!(!result.changed);
    assert_eq!(result.match_count, 0);
}

#[test]
fn replace_in_content_fuzzy_not_applied_in_regex_mode() {
    // fuzzy should be ignored when regex mode is enabled.
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        regex: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn proccess_data\\(\\) \\{\\}",
        "fn handle() {}",
        &opts,
    )
    .unwrap();
    assert!(!result.changed, "regex mode should bypass fuzzy");
}

#[test]
fn replace_in_content_fuzzy_with_unique() {
    // fuzzy + unique: fuzzy finds exactly one match, so unique is satisfied.
    let content = "fn setup() {}\nfn process_data(x: i32) {}\nfn cleanup() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        unique: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn proccess_data(x: i32) {}",
        "fn handle_data(x: i32) {}",
        &opts,
    )
    .unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_fuzzy_with_insert_before() {
    // fuzzy + insert_before: the matched text should be preserved after
    // the inserted text.
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        insert_before: Some("// TODO: refactor\n".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn proccess_data() {}",
        "", // ignored when insert_before is set
        &opts,
    )
    .unwrap();
    assert!(result.changed);
    assert!(
        result.new_content.contains("// TODO: refactor\n"),
        "insert_before text should appear: {}",
        result.new_content
    );
    assert!(
        result.new_content.contains("fn process_data() {}"),
        "original matched text should be preserved: {}",
        result.new_content
    );
}

#[test]
fn replace_in_content_fuzzy_with_insert_after() {
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        insert_after: Some("\n// end".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "fn proccess_data() {}", "", &opts).unwrap();
    assert!(result.changed);
    assert!(
        result.new_content.contains("fn process_data() {}\n// end"),
        "insert_after text should appear after matched text: {}",
        result.new_content
    );
}

#[test]
fn replace_in_content_fuzzy_uses_before_context() {
    // Two identical typos; before_context disambiguates via anchor matching
    let content = "fn alpha() {}\nfn proccess_data() {}\nfn beta() {}\nfn proccess_data() {}\nfn gamma() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        before_context: Some("fn beta()".to_string()),
        ..Default::default()
    };
    let result =
        replace::replace_in_content(content, "fn proccess_data() {}", "fn fixed() {}", &opts)
            .unwrap();
    assert!(result.changed);
    // The second occurrence (after beta) should be replaced
    assert!(
        result.new_content.contains("fn fixed()"),
        "fuzzy+before_context should replace: {}",
        result.new_content
    );
}

#[test]
fn replace_in_content_fuzzy_uses_after_context() {
    // Two identical typos; after_context disambiguates via anchor matching
    let content = "fn alpha() {}\nfn proccess_data() {}\nfn beta() {}\nfn proccess_data() {}\nfn gamma() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        after_context: Some("fn beta()".to_string()),
        ..Default::default()
    };
    let result =
        replace::replace_in_content(content, "fn proccess_data() {}", "fn fixed() {}", &opts)
            .unwrap();
    assert!(result.changed);
    // The first occurrence (before beta) should be replaced
    assert!(
        result.new_content.contains("fn fixed()"),
        "fuzzy+after_context should replace: {}",
        result.new_content
    );
}

#[test]
fn parse_unified_diff_basic() {
    let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"world\");
 }
";
    let files = parse_unified_diff(diff).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "src/main.rs");
    assert_eq!(files[0].hunks.len(), 1);
    let hunk = &files[0].hunks[0];
    assert_eq!(hunk.old_start, 1);
    assert_eq!(hunk.old_count, 3);
    assert_eq!(hunk.new_start, 1);
    assert_eq!(hunk.new_count, 3);
    assert!(
        hunk.lines
            .contains(&PatchLine::Remove("    println!(\"hello\");".into()))
    );
    assert!(
        hunk.lines
            .contains(&PatchLine::Add("    println!(\"world\");".into()))
    );
}

#[test]
fn parse_unified_diff_multiple_files() {
    let diff = "\
--- a/foo.txt
+++ b/foo.txt
@@ -1 +1 @@
-old
+new
--- a/bar.txt
+++ b/bar.txt
@@ -1,2 +1,2 @@
 keep
-remove
+add
";
    let files = parse_unified_diff(diff).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "foo.txt");
    assert_eq!(files[1].path, "bar.txt");
}

#[test]
fn parse_unified_diff_new_file() {
    let diff = "\
--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,2 @@
+line one
+line two
";
    let files = parse_unified_diff(diff).unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].is_creation);
    assert_eq!(files[0].path, "new_file.txt");
}

#[test]
fn parse_unified_diff_empty_input() {
    let err = parse_unified_diff("").unwrap_err();
    assert!(
        err.contains("no files"),
        "empty input should report no files, got: {err}"
    );
}

#[test]
fn parse_unified_diff_deleted_file() {
    let diff = "\
--- a/removed.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-line one
-line two
";
    let files = parse_unified_diff(diff).unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].is_deletion);
    assert!(!files[0].is_creation);
    assert_eq!(files[0].path, "removed.txt");
}

#[test]
fn parse_unified_diff_roundtrip_with_text_diff() {
    let original = "hello\nworld\n";
    let modified = "hello\nearth\n";
    let diff_text = text_diff(original, modified, Some("test.txt"));
    let files = parse_unified_diff(&diff_text).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "test.txt");
    let hunk = &files[0].hunks[0];
    assert!(hunk.lines.contains(&PatchLine::Remove("world".into())));
    assert!(hunk.lines.contains(&PatchLine::Add("earth".into())));
}

#[test]
fn replace_text_unique_fails_on_ambiguity() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar foo baz foo\n").unwrap();

    let opts = ReplaceOptions {
        unique: true,
        ..Default::default()
    };
    let err = replace_text(&file, "foo", "x", &opts, ApplyMode::Preview, None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ambiguous"),
        "expected ambiguous error, got: {msg}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_before_context_disambiguates() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "alpha\nTODO: fix\nbeta\ngamma\nTODO: fix\ndelta\n").unwrap();

    // "TODO: fix" appears twice; before_context selects the one after "gamma".
    let opts = ReplaceOptions {
        before_context: Some("gamma".to_string()),
        ..Default::default()
    };
    let result = replace_text(&file, "TODO: fix", "DONE", &opts, ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    let content = fs::read_to_string(&file).unwrap();
    // First occurrence should be untouched, second replaced.
    assert!(
        content.contains("TODO: fix"),
        "first occurrence should remain"
    );
    assert!(content.contains("DONE"), "second should be replaced");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_after_context_disambiguates() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "alpha\nTODO: fix\nbeta\ngamma\nTODO: fix\ndelta\n").unwrap();

    // after_context="beta" selects the first "TODO: fix" (the one before "beta").
    let opts = ReplaceOptions {
        after_context: Some("beta".to_string()),
        ..Default::default()
    };
    let result = replace_text(&file, "TODO: fix", "DONE", &opts, ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    let content = fs::read_to_string(&file).unwrap();
    // First occurrence replaced, second should remain.
    assert!(
        content.contains("TODO: fix"),
        "second occurrence should remain"
    );
    assert!(content.contains("DONE"), "first should be replaced");
}

// ── #1314: Unicode/multibyte text tests for replace_in_content ──

#[test]
fn replace_in_content_unicode_cjk_literal() {
    let content = "fn greet() { println!(\"こんにちは世界\"); }\n";
    let result = replace::replace_in_content(
        content,
        "こんにちは世界",
        "你好世界",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("你好世界"));
    assert!(!result.new_content.contains("こんにちは世界"));
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_unicode_emoji() {
    let content = "status: 🔴 failing\nother: 🟢 passing\n";
    let result = replace::replace_in_content(
        content,
        "🔴 failing",
        "🟢 passing",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert!(result.changed);
    assert_eq!(
        result.new_content, "status: 🟢 passing\nother: 🟢 passing\n",
        "emoji replacement should preserve byte boundaries"
    );
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_unicode_combining_marks() {
    // e\u{0301} is "e" + combining acute accent = "é" (2 code points)
    let content = "caf\u{0065}\u{0301} au lait\n";
    let result = replace::replace_in_content(
        content,
        "caf\u{0065}\u{0301}",
        "coffee",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("coffee au lait"));
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_unicode_mixed_scripts() {
    let content = "name: Привет мир\ntag: hello\n";
    let result = replace::replace_in_content(
        content,
        "Привет мир",
        "Здравствуй мир",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("Здравствуй мир"));
    assert_eq!(result.match_count, 1);
}

#[test]
fn replace_in_content_unicode_regex() {
    let content = "price: ¥1000\nprice: ¥2000\n";
    let opts = ReplaceOptions {
        regex: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, r"¥\d+", "€999", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 2);
    assert_eq!(result.new_content, "price: €999\nprice: €999\n");
}

#[test]
fn replace_in_content_unicode_case_insensitive() {
    // Turkish dotless i: İ (U+0130) lowercases to i in Unicode-aware mode
    let content = "ÜBER cool\n";
    let opts = ReplaceOptions {
        case_insensitive: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "über", "super", &opts).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("super cool"));
}

#[test]
fn replace_in_content_unicode_word_boundary() {
    // Word boundary with CJK: CJK chars don't have \b boundaries like Latin,
    // but the pattern should still work for Latin words in mixed content.
    let content = "日本語 hello 中文\n";
    let opts = ReplaceOptions {
        word_boundary: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "hello", "world", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.new_content, "日本語 world 中文\n");
}

#[test]
fn replace_in_content_unicode_whole_line() {
    let content = "普通の行\n削除する行\nもう一つの行\n";
    let opts = ReplaceOptions {
        whole_line: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "削除する", "", &opts).unwrap();
    assert!(result.changed);
    assert!(!result.new_content.contains("削除する行"));
    assert!(result.new_content.contains("普通の行"));
    assert!(result.new_content.contains("もう一つの行"));
}

#[test]
fn replace_in_content_unicode_nth() {
    let content = "café café café\n";
    let opts = ReplaceOptions {
        nth: Some(2),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "café", "tea", &opts).unwrap();
    assert!(result.changed);
    assert_eq!(result.new_content, "café tea café\n");
}

#[test]
fn replace_in_content_unicode_insert_after() {
    let content = "use 标准库;\n";
    let opts = ReplaceOptions {
        insert_after: Some("\nuse 扩展库;".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "use 标准库;", "", &opts).unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("use 标准库;\nuse 扩展库;"));
}

#[test]
fn replace_in_content_unicode_multiline_regex() {
    let content = "struct 数据 {\n    名前: String,\n}\n";
    let opts = ReplaceOptions {
        regex: true,
        multiline: true,
        ..Default::default()
    };
    let result =
        replace::replace_in_content(content, r"struct 数据 \{[^}]+\}", "struct Data {}", &opts)
            .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("struct Data {}"));
}

#[test]
fn replace_in_content_unicode_unique() {
    let content = "α β α γ\n";
    let opts = ReplaceOptions {
        unique: true,
        ..Default::default()
    };
    let err = replace::replace_in_content(content, "α", "x", &opts).unwrap_err();
    assert!(err.to_string().contains("ambiguous"));
}

#[test]
fn replace_in_content_unicode_fuzzy() {
    // Fuzzy match with Unicode: "процесс" vs "процесс" (typo: extra с)
    let content = "fn процесс_данных() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn процесс_данныхх() {}",
        "fn обработка() {}",
        &opts,
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("обработка"));
}

// ── #1315: replace_in_content context disambiguation and fallback ──

#[test]
fn replace_in_content_context_disambiguates_multi_match() {
    let content =
        "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n";
    let opts = ReplaceOptions {
        before_context: Some("[database]".to_string()),
        ..Default::default()
    };
    let result =
        replace::replace_in_content(content, "host = localhost", "host = db.primary", &opts)
            .unwrap();
    assert!(result.changed);
    assert_eq!(result.match_count, 1);
    assert!(result.new_content.contains("host = db.primary"));
    assert!(
        result.new_content.matches("host = localhost").count() == 1,
        "only one occurrence should be replaced"
    );
}

#[test]
fn replace_in_content_context_fallback_on_zero_match() {
    // context alone (without fuzzy) should trigger fallback on zero match.
    let content = "fn header() {}\nfn process(x: Vec<u8>) {}\nfn footer() {}\n";
    let opts = ReplaceOptions {
        before_context: Some("fn header()".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(
        content,
        "fn process(x: Vec<i32>) {}",
        "fn process(x: Vec<u8>, flag: bool) {}",
        &opts,
    )
    .unwrap();
    assert!(result.changed, "context fallback should find a match");
    assert_eq!(result.match_count, 1);
    assert!(result.new_content.contains("flag: bool"));
}

// ── #1315: replace_text fallback path tests (context + fuzzy) ──
// These tests exercise replace_text (file-based) with context and fuzzy
// options. Under --all-features they go through the tx engine; under
// --no-default-features they exercise the fixed fallback path.

#[test]
fn replace_text_before_context_disambiguates_any_path() {
    // Tests both full and fallback paths depending on features.
    // Full path: tx engine handles context via context_filtered_offset.
    // Fallback path: our fix in replace_write handles it directly.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.ini");
    fs::write(
        &file,
        "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
    )
    .unwrap();

    let opts = ReplaceOptions {
        before_context: Some("[database]".to_string()),
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "host = localhost",
        "host = db.primary",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(result.changed);
    assert!(
        result.new_content.contains("host = db.primary"),
        "database host should be replaced"
    );
    assert!(
        result.new_content.matches("host = localhost").count() == 1,
        "cache host should remain unchanged"
    );
}

#[test]
fn replace_text_after_context_disambiguates_any_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.ini");
    fs::write(
        &file,
        "[database]\nhost = localhost\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
    )
    .unwrap();

    let opts = ReplaceOptions {
        after_context: Some("port = 5432".to_string()),
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "host = localhost",
        "host = db.primary",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.new_content.contains("[database]\nhost = db.primary"));
    assert!(result.new_content.contains("[cache]\nhost = localhost"));
}

#[test]
fn replace_text_context_fallback_on_no_match_any_path() {
    // When exact match fails but context is provided, resolve_with_fallback
    // should find an anchor match. Both full and fallback paths support this.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(
        &file,
        "fn header() {\n}\nfn process(input: Vec<u8>) {\n}\nfn footer() {\n}\n",
    )
    .unwrap();

    let opts = ReplaceOptions {
        before_context: Some("fn process".to_string()),
        ..Default::default()
    };
    // Stale pattern (Vec<i32> instead of Vec<u8>): exact match fails,
    // context fallback should find the similar line.
    let result = replace_text(
        &file,
        "fn process(input: Vec<i32>) {",
        "fn process(input: Vec<u8>, flag: bool) {",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(result.changed, "context fallback should find a match");
    assert!(
        result.new_content.contains("flag: bool"),
        "replacement should be applied"
    );
}

#[test]
fn replace_text_fuzzy_with_context_any_path() {
    // fuzzy + context: when exact match fails, both paths use
    // resolve_with_fallback to find the anchor match.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(
        &file,
        "fn setup() {}\nfn process_data(x: i32) {}\nfn cleanup() {}\n",
    )
    .unwrap();

    let opts = ReplaceOptions {
        fuzzy: true,
        before_context: Some("fn setup()".to_string()),
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "fn proccess_data(x: i32) {}",
        "fn handle_data(x: i32) {}",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(result.changed, "fuzzy+context should resolve the typo");
    assert!(
        result.new_content.contains("handle_data"),
        "replacement should be applied: {}",
        result.new_content
    );
}

#[test]
fn replace_text_fuzzy_with_if_exists_any_path() {
    // fuzzy + if_exists + context: when all fail, no error.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn alpha() {}\nfn beta() {}\n").unwrap();

    let opts = ReplaceOptions {
        fuzzy: true,
        if_exists: true,
        before_context: Some("fn alpha()".to_string()),
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "fn completely_unrelated_xyz() {}",
        "fn replaced() {}",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(!result.changed);
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_api_updates_params() {
    use crate::ast::rewrite::FunctionSigEdit;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn process(x: i32) {}\n").unwrap();

    let edit = FunctionSigEdit {
        parameters: Some("(x: u64)".into()),
        return_type: Some("-> u64".into()),
        ..Default::default()
    };
    let result = ast_rewrite_signature(&file, "process", &edit, None, ApplyMode::Apply, None)
        .expect("rewrite should succeed");
    assert!(result.changed);
    assert!(result.applied);
    assert_eq!(result.action, "ast.rewrite_signature");
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("u64"), "got: {on_disk}");
    assert!(
        on_disk.contains("-> u64 {"),
        "structured API path must keep body gap (#1503): {on_disk}"
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_full_string_preserves_body_gap() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n").unwrap();
    let edit = crate::ast::rewrite::FunctionSigEdit::default();
    // Logical signature with no trailing space (agent/embedder style).
    let result = ast_rewrite_signature(
        &file,
        "add",
        &edit,
        Some("pub fn add(a: i32, b: i32, c: i32) -> i32"),
        ApplyMode::Apply,
        None,
    )
    .expect("full-string rewrite");
    assert!(result.changed && result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(
        on_disk.contains("-> i32 {"),
        "full new_signature must not glue to brace: {on_disk}"
    );
    assert!(!on_disk.contains("i32{"), "got: {on_disk}");
    assert!(on_disk.contains("c: i32"), "params updated: {on_disk}");
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_plan_execute() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("m.rs");
    fs::write(&file, "fn greet() {}\n").unwrap();

    let plan = parse_plan(
        r#"{
            "version": 1,
            "operations": [
                {
                    "op": "ast.rewrite_signature",
                    "path": "m.rs",
                    "old": "greet",
                    "new_signature": "fn greet(name: &str)",
                    "lang": "rust"
                }
            ]
        }"#,
    )
    .unwrap();
    let report = execute_plan(plan, dir.path(), None).unwrap();
    assert!(report.ok, "plan should succeed: status={}", report.status);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(
        on_disk.contains("name: &str"),
        "plan rewrite should update signature: {on_disk}"
    );
    // #1503: logical new_signature without trailing space must not glue to `{`.
    assert!(
        on_disk.contains("fn greet(name: &str) {"),
        "plan path must preserve body gap, got: {on_disk}"
    );
    assert!(
        !on_disk.contains("str){"),
        "must not glue type to brace: {on_disk}"
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_plan_missing_is_no_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("m.rs"), "fn keep() {}\n").unwrap();

    let plan = parse_plan(
        r#"{
            "version": 1,
            "operations": [
                {
                    "op": "ast.rewrite_signature",
                    "path": "m.rs",
                    "old": "missing_fn",
                    "parameters": "(x: i32)"
                }
            ]
        }"#,
    )
    .unwrap();
    let report = execute_plan(plan, dir.path(), None).unwrap();
    assert!(!report.ok, "missing function must fail the plan");
    assert_eq!(
        report.error_kind.as_deref(),
        Some("no_matches"),
        "library execute_plan must use no_matches not operation_failed: {report:?}"
    );
    let err = report.error.as_deref().unwrap_or("");
    assert!(
        err.contains("missing_fn") || err.contains("not found"),
        "error should name the function: {err}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_content_edits_to_file_writes_once() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.txt");
    fs::write(&file, "hello world\n").unwrap();

    let edits = [
        ContentEdit::Replace {
            old: "hello".into(),
            new: "hi".into(),
            options: ReplaceOptions::default(),
        },
        ContentEdit::Append {
            content: "done\n".into(),
        },
    ];
    let result =
        apply_content_edits_to_file(&file, &edits, ApplyMode::Apply, None).expect("file edits");
    assert!(result.changed);
    assert!(result.applied);
    assert_eq!(result.action, "content.edits");
    assert_eq!(
        result.match_count, 1,
        "file helper should surface rolled-up replace match_count"
    );
    let on_disk = fs::read_to_string(&file).unwrap();
    assert_eq!(on_disk, "hi world\ndone\n");
    // #1500: file helper must name the real path, not the buffer placeholder.
    assert!(
        result.diff.contains("notes.txt"),
        "diff headers should include target path, got:\n{}",
        result.diff
    );
    assert!(
        !result.diff.contains("<buffer>"),
        "file helper must not keep <buffer> label:\n{}",
        result.diff
    );
    assert!(
        !result.diff.contains("--- a//") && !result.diff.contains("+++ b//"),
        "absolute path headers must not double-slash:\n{}",
        result.diff
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_content_edits_to_file_diff_path_vs_buffer() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("cfg.toml");
    fs::write(&file, "name = old\n").unwrap();
    let edits = [ContentEdit::Replace {
        old: "old".into(),
        new: "new".into(),
        options: ReplaceOptions::default(),
    }];

    let file_result =
        apply_content_edits_to_file(&file, &edits, ApplyMode::Preview, None).expect("file preview");
    let buffer_result = apply_content_edits("name = old\n", &edits).expect("buffer");

    assert!(file_result.changed && buffer_result.changed);
    assert!(
        file_result.diff.contains("cfg.toml")
            && file_result.diff.contains("--- a/")
            && file_result.diff.contains("+++ b/"),
        "file EditResult.diff should header the real path:\n{}",
        file_result.diff
    );
    assert!(
        !file_result.diff.contains("<buffer>"),
        "file path headers must not use <buffer>:\n{}",
        file_result.diff
    );
    assert!(
        buffer_result.diff.contains("--- a/<buffer>")
            && buffer_result.diff.contains("+++ b/<buffer>"),
        "pure buffer helper keeps <buffer>:\n{}",
        buffer_result.diff
    );
    // Absolute path (TempDir) must not produce a// after path_for_diff_header.
    assert!(
        !file_result.diff.contains("a//") && !file_result.diff.contains("b//"),
        "no double-slash headers:\n{}",
        file_result.diff
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_content_edits_to_file_respects_guard() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-content-edit-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "secret\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let edits = [ContentEdit::Append {
        content: "x\n".into(),
    }];
    let err = apply_content_edits_to_file(&outside, &edits, ApplyMode::Apply, Some(&guard))
        .expect_err("must reject outside path");
    assert!(
        err.to_string().contains("guard") || err.to_string().contains("escapes"),
        "got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected)
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "secret\n");
    let _ = fs::remove_file(&outside);
}

// ── #1492 require_change + structured EditError ───────────────────────────

#[test]
fn replace_in_content_require_change_true_no_match() {
    let err = replace_in_content(
        "hello world",
        "missing",
        "x",
        &ReplaceOptions {
            require_change: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
    let e = crate::fallback::edit_error_ref(&err).expect("EditError in chain");
    assert!(e.similar_targets.is_empty() || !e.message.is_empty());
}

#[test]
fn replace_in_content_require_change_false_no_match_ok() {
    let r = replace_in_content("hello world", "missing", "x", &ReplaceOptions::default()).unwrap();
    assert!(!r.changed);
    assert_eq!(r.match_count, 0);
    assert_eq!(r.new_content, "hello world");
}

#[test]
fn replace_in_content_require_change_identity_match_is_ok() {
    // Matches exist but replacement equals the match: still Ok (not NoMatch).
    let r = replace_in_content(
        "hello hello",
        "hello",
        "hello",
        &ReplaceOptions {
            require_change: true,
            ..Default::default()
        },
    )
    .expect("require_change cares about zero matches, not identity replace");
    assert!(!r.changed);
    assert_eq!(r.match_count, 2);
}

#[test]
fn replace_in_content_unique_multi_is_ambiguous() {
    let err = replace_in_content(
        "a a a",
        "a",
        "b",
        &ReplaceOptions {
            unique: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::AmbiguousTarget)
    );
}

#[test]
fn replace_in_content_command_position_pip() {
    let r = replace_in_content(
        "pip install x\nuv pip install\npipenv install\n",
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            require_change: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.changed);
    assert_eq!(r.match_count, 1);
    assert_eq!(
        r.new_content,
        "uv install x\nuv pip install\npipenv install\n"
    );
}

#[test]
fn replace_in_content_command_position_timeout_and_nice() {
    let r = replace_in_content(
        "timeout 30 pip install x
nice -n 10 pip list
echo 30 pip
",
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            require_change: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.match_count, 2);
    assert_eq!(
        r.new_content,
        "timeout 30 uv install x
nice -n 10 uv list
echo 30 pip
"
    );
}

#[test]
fn replace_in_content_command_position_no_match_with_require_change() {
    let err = replace_in_content(
        "uv pip install\n",
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            require_change: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
}

#[test]
fn replace_in_content_if_exists_wins_over_require_change() {
    let r = replace_in_content(
        "hello",
        "missing",
        "x",
        &ReplaceOptions {
            require_change: true,
            if_exists: true,
            ..Default::default()
        },
    )
    .expect("if_exists must soften require_change");
    assert!(!r.changed);
    assert_eq!(r.match_count, 0);
}

#[test]
fn replace_in_content_command_position_rejects_regex() {
    let err = replace_in_content(
        "pip install\n",
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            regex: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn replace_in_content_command_position_rejects_case_insensitive() {
    let err = replace_in_content(
        "PIP install\n",
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            case_insensitive: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
    assert!(err.to_string().contains("case_insensitive"), "msg={}", err);
}

#[test]
fn replace_in_content_command_position_rejects_word_boundary_and_fuzzy() {
    for (label, opts) in [
        (
            "word_boundary",
            ReplaceOptions {
                command_position: true,
                word_boundary: true,
                ..Default::default()
            },
        ),
        (
            "fuzzy",
            ReplaceOptions {
                command_position: true,
                fuzzy: true,
                ..Default::default()
            },
        ),
        (
            "before_context",
            ReplaceOptions {
                command_position: true,
                before_context: Some("x".into()),
                ..Default::default()
            },
        ),
    ] {
        let err = replace_in_content("pip install\n", "pip", "uv", &opts).unwrap_err();
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(EditErrorKind::InvalidInput),
            "{label}"
        );
    }
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_require_change_file_no_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "hello\n").unwrap();
    let err = replace_text(
        &file,
        "missing",
        "x",
        &ReplaceOptions {
            require_change: true,
            ..Default::default()
        },
        ApplyMode::Preview,
        None,
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_if_exists_wins_over_require_change() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "hello\n").unwrap();
    let r = replace_text(
        &file,
        "missing",
        "x",
        &ReplaceOptions {
            require_change: true,
            if_exists: true,
            ..Default::default()
        },
        ApplyMode::Apply,
        None,
    )
    .expect("if_exists must win on file path too");
    assert!(!r.changed);
    assert!(!r.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_command_position_on_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "pip install x\nuv pip install\n").unwrap();
    let r = replace_text(
        &file,
        "pip",
        "uv",
        &ReplaceOptions {
            command_position: true,
            require_change: true,
            ..Default::default()
        },
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(r.applied);
    assert_eq!(r.match_count, 1);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "uv install x\nuv pip install\n"
    );
}

// ── #1493 AST file mutators + in-content signature ────────────────────────

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_api_apply_and_preview() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn foo() { let x = 1; foo(); }\n").unwrap();

    let preview = ast_rename(&file, "foo", "bar", ApplyMode::Preview, None).unwrap();
    assert!(preview.changed);
    assert!(!preview.applied);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn foo() { let x = 1; foo(); }\n"
    );

    let applied = ast_rename(&file, "foo", "bar", ApplyMode::Apply, None).unwrap();
    assert!(applied.applied);
    assert!(applied.match_count >= 1);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("fn bar"), "got: {on_disk}");
    assert!(!on_disk.contains("fn foo"));
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_no_match_is_structured() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn keep() {}\n").unwrap();
    let err = ast_rename(&file, "missing", "x", ApplyMode::Preview, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_replace_in_symbol_api() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(
        &file,
        "fn outer() {\n    let target = 1;\n}\nfn other() {\n    let target = 2;\n}\n",
    )
    .unwrap();
    let result = ast_replace_in_symbol(
        &file,
        "outer",
        "target",
        "value",
        &AstReplaceInSymbolOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("let value = 1"), "got: {on_disk}");
    assert!(
        on_disk.contains("let target = 2"),
        "other symbol untouched: {on_disk}"
    );
}

#[cfg(feature = "ast")]
#[test]
fn ast_rewrite_signature_in_content_structured() {
    use crate::ast::Language;
    use crate::ast::rewrite::FunctionSigEdit;

    let src = "fn process(x: i32) {}\n";
    let edit = FunctionSigEdit {
        parameters: Some("(x: u64)".into()),
        return_type: Some("-> u64".into()),
        ..Default::default()
    };
    // In-content API is on ast_write module (files/cli feature).
    #[cfg(any(feature = "cli", feature = "files"))]
    {
        let r =
            ast_rewrite_signature_in_content(src, "process", &edit, None, Language::Rust).unwrap();
        assert!(r.changed);
        assert!(r.new_content.contains("u64"), "got: {}", r.new_content);
    }
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_in_content_no_match() {
    use crate::ast::Language;
    use crate::ast::rewrite::FunctionSigEdit;

    let err = ast_rewrite_signature_in_content(
        "fn keep() {}\n",
        "missing",
        &FunctionSigEdit {
            parameters: Some("(x: i32)".into()),
            ..Default::default()
        },
        None,
        Language::Rust,
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
}

// ── #1495 batch AST rename ────────────────────────────────────────────────

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_two_files_success() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    fs::write(&a, "fn foo() {}\n").unwrap();
    fs::write(&b, "fn foo() { foo(); }\n").unwrap();

    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Apply,
        continue_on_no_match: true,
        fail_fast: false,
    };
    let results = ast_rename_batch(&[&a, &b], "foo", "bar", &opts, None).unwrap();
    assert_eq!(results.len(), 2);
    let r0 = results[0]
        .result
        .as_ref()
        .expect("first file should rename");
    let r1 = results[1]
        .result
        .as_ref()
        .expect("second file should rename");
    assert!(r0.changed);
    assert!(r1.changed);
    assert!(fs::read_to_string(&a).unwrap().contains("bar"));
    assert!(fs::read_to_string(&b).unwrap().contains("bar"));
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_continue_on_no_match() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    fs::write(&a, "fn foo() {}\n").unwrap();
    fs::write(&b, "fn other() {}\n").unwrap();

    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Apply,
        continue_on_no_match: true,
        ..Default::default()
    };
    let results = ast_rename_batch(&[&a, &b], "foo", "bar", &opts, None).unwrap();
    assert_eq!(results.len(), 2);
    results[0]
        .result
        .as_ref()
        .expect("matching file should rename");
    let err = results[1].result.as_ref().expect_err("no-match file");
    assert_eq!(err.kind, EditErrorKind::NoMatch);
    assert!(fs::read_to_string(&a).unwrap().contains("bar"));
    assert!(fs::read_to_string(&b).unwrap().contains("other"));
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_dedupes_paths() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    fs::write(&a, "fn foo() {}\n").unwrap();
    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Preview,
        ..Default::default()
    };
    let results = ast_rename_batch(&[&a, &a, &a], "foo", "bar", &opts, None).unwrap();
    assert_eq!(results.len(), 1, "duplicate paths processed once");
    assert!(
        results[0]
            .result
            .as_ref()
            .expect("deduped path should rename")
            .changed
    );
    // Preview: disk unchanged
    assert_eq!(fs::read_to_string(&a).unwrap(), "fn foo() {}\n");
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_fail_fast_stops_after_hard_error() {
    let dir = TempDir::new().unwrap();
    let good = dir.path().join("good.rs");
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-fail-fast-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let after = dir.path().join("after.rs");
    fs::write(&good, "fn foo() {}\n").unwrap();
    fs::write(&outside, "fn foo() {}\n").unwrap();
    fs::write(&after, "fn foo() {}\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Apply,
        continue_on_no_match: true,
        fail_fast: true,
    };
    // Process good first (ok), then outside (guard), then after must not run.
    let results = ast_rename_batch(
        &[&good, &outside, &after],
        "foo",
        "bar",
        &opts,
        Some(&guard),
    )
    .unwrap();
    assert_eq!(results.len(), 2, "fail_fast should stop before third path");
    results[0]
        .result
        .as_ref()
        .expect("in-workspace file should rename");
    assert_eq!(
        results[1]
            .result
            .as_ref()
            .expect_err("outside path should be guard rejected")
            .kind,
        EditErrorKind::GuardRejected
    );
    assert!(
        fs::read_to_string(&after).unwrap().contains("foo"),
        "third file must not be rewritten under fail_fast"
    );
    let _ = fs::remove_file(&outside);
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_continue_on_no_match_false_stops() {
    let dir = TempDir::new().unwrap();
    let miss = dir.path().join("miss.rs");
    let after = dir.path().join("after.rs");
    fs::write(&miss, "fn other() {}\n").unwrap();
    fs::write(&after, "fn foo() {}\n").unwrap();
    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Apply,
        continue_on_no_match: false,
        fail_fast: false,
    };
    let results = ast_rename_batch(&[&miss, &after], "foo", "bar", &opts, None).unwrap();
    assert_eq!(
        results.len(),
        1,
        "continue_on_no_match=false must stop after first NoMatch"
    );
    assert_eq!(
        results[0].result.as_ref().unwrap_err().kind,
        EditErrorKind::NoMatch
    );
    assert!(
        fs::read_to_string(&after).unwrap().contains("foo"),
        "later paths must not run after NoMatch stop"
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_empty_names_are_invalid_input() {
    let err = ast_rename_batch(&[], "", "x", &AstRenameBatchOptions::default(), None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_empty_paths_is_ok_empty() {
    let results = ast_rename_batch(&[], "foo", "bar", &AstRenameBatchOptions::default(), None)
        .expect("empty path list is valid");
    assert!(results.is_empty());
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_check_mode_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn foo() {}\n").unwrap();
    let r = ast_rename(&file, "foo", "bar", ApplyMode::Check, None).unwrap();
    assert!(r.changed);
    assert!(!r.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "fn foo() {}\n");
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_guard_rejects_outside() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-batch-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "fn foo() {}\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Apply,
        continue_on_no_match: false,
        fail_fast: false,
    };
    let results = ast_rename_batch(&[&outside], "foo", "bar", &opts, Some(&guard)).unwrap();
    assert_eq!(results.len(), 1);
    let err = results[0].result.as_ref().unwrap_err();
    assert_eq!(err.kind, EditErrorKind::GuardRejected);
    assert_eq!(fs::read_to_string(&outside).unwrap(), "fn foo() {}\n");
    let _ = fs::remove_file(&outside);
}

// ── #1494 restore from latest backup ──────────────────────────────────────

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn restore_path_from_latest_backup_after_apply() {
    use crate::backup::restore_path_from_latest_backup;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.txt");
    fs::write(&file, "original\n").unwrap();

    let result = replace_text(
        &file,
        "original",
        "modified",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified\n");

    // Library apply uses file parent as backup project root.
    let restored = restore_path_from_latest_backup(dir.path(), &file).unwrap();
    assert!(restored, "should find backup session for path");
    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
}

// ── #1658–#1666 Bline embedder API batch ──────────────────────────────────

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_replace_in_symbol_regex_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(
        &file,
        "fn parse_config() {\n    let foo_x_bar = 1;\n    let keep = 2;\n}\nfn other() {\n    let foo_x_bar = 9;\n}\n",
    )
    .unwrap();
    let opts = AstReplaceInSymbolOptions { regex: true };
    let result = ast_replace_in_symbol(
        &file,
        "parse_config",
        r"foo_.*_bar",
        "baz",
        &opts,
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.changed && result.applied);
    assert!(result.match_count >= 1);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(on_disk.contains("let baz = 1"), "got: {on_disk}");
    assert!(
        on_disk.contains("let foo_x_bar = 9"),
        "other symbol must stay: {on_disk}"
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_replace_in_symbol_missing_symbol_vs_pattern() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn real() {\n    let target = 1;\n}\n").unwrap();
    let opts = AstReplaceInSymbolOptions::default();

    let err = ast_replace_in_symbol(
        &file,
        "missing",
        "target",
        "x",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
    let msg = err.to_string();
    assert!(
        msg.contains("symbol") && msg.contains("missing"),
        "symbol miss should name symbol: {msg}"
    );

    let err = ast_replace_in_symbol(
        &file,
        "real",
        "no_such_pattern",
        "x",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch)
    );
    let msg = err.to_string();
    assert!(
        msg.contains("no matches") && msg.contains("no_such_pattern"),
        "pattern miss should name pattern: {msg}"
    );
}

#[test]
fn classify_error_without_anyhow() {
    use crate::fallback::{classify_error, classify_error_ref};
    use std::error::Error;

    let edit = EditError::new(EditErrorKind::NoMatch, "no matches for \"x\"")
        .with_similar(vec!["y".into()]);
    let boxed: Box<dyn Error + Send + Sync> = Box::new(edit.clone());
    assert_eq!(classify_error(boxed.as_ref()), Some(EditErrorKind::NoMatch));
    let got = classify_error_ref(boxed.as_ref()).expect("EditError");
    assert_eq!(got.similar_targets, vec!["y".to_string()]);

    let invalid: Box<dyn Error + Send + Sync> =
        Box::new(crate::exit::InvalidInputError { msg: "bad".into() });
    assert_eq!(
        classify_error(invalid.as_ref()),
        Some(EditErrorKind::InvalidInput)
    );

    let ambiguous: Box<dyn Error + Send + Sync> =
        Box::new(crate::exit::AmbiguousError { msg: "many".into() });
    assert_eq!(
        classify_error(ambiguous.as_ref()),
        Some(EditErrorKind::AmbiguousTarget)
    );

    // Bare EditError round-trips through classify without anyhow.
    let e = EditError::new(EditErrorKind::GuardRejected, "nope");
    assert_eq!(
        classify_error(&e as &(dyn Error + 'static)),
        Some(EditErrorKind::GuardRejected)
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn restore_path_from_session_single_file_only() {
    use crate::backup::{list_sessions, restore_path_from_session};

    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "A0").unwrap();
    fs::write(&b, "B0").unwrap();

    // One session backs up both files, then both are mutated.
    let mut session = crate::backup::BackupSession::new(dir.path()).unwrap();
    session.save_before_write(&a).unwrap();
    session.save_before_write(&b).unwrap();
    session.finalize().unwrap();
    fs::write(&a, "A1").unwrap();
    fs::write(&b, "B1").unwrap();

    let sessions = list_sessions(dir.path()).unwrap();
    assert!(!sessions.is_empty());
    let ts = sessions[0].timestamp.clone();

    let ok = restore_path_from_session(dir.path(), &ts, &a).unwrap();
    assert!(ok);
    assert_eq!(fs::read_to_string(&a).unwrap(), "A0");
    assert_eq!(
        fs::read_to_string(&b).unwrap(),
        "B1",
        "sibling path must remain edited"
    );

    // Path absent from session → Ok(false).
    let missing = dir.path().join("c.txt");
    fs::write(&missing, "C").unwrap();
    let ok = restore_path_from_session(dir.path(), &ts, &missing).unwrap();
    assert!(!ok);

    // Missing session → error.
    let err = restore_path_from_session(dir.path(), "no-such-session", &a).unwrap_err();
    assert!(
        err.to_string().contains("no backup session") || err.to_string().contains("no-such"),
        "got: {err}"
    );
}

#[test]
fn match_mode_exact_fuzzy_and_anchored() {
    // Exact
    let r = replace_in_content(
        "hello world\n",
        "world",
        "there",
        &ReplaceOptions::default(),
    )
    .unwrap();
    assert_eq!(r.match_mode, Some(MatchMode::Exact));
    assert!(r.match_score.is_none());

    // Fuzzy similarity
    let r = replace_in_content(
        "fn process_data() {}\n",
        "fn proccess_data() {}",
        "fn process_data() {}",
        &ReplaceOptions {
            fuzzy: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.changed, "fuzzy should land");
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    assert!(
        r.match_score.is_some_and(|s| s > 0.85),
        "fuzzy score expected, got {:?}",
        r.match_score
    );

    // Anchored multi-match disambiguation
    let content = "alpha\nTODO: fix\nbeta\nTODO: fix\n";
    let r = replace_in_content(
        content,
        "TODO: fix",
        "TODO: done",
        &ReplaceOptions {
            before_context: Some("beta".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.changed);
    assert_eq!(r.match_mode, Some(MatchMode::Anchored));
    assert_eq!(r.match_count, 1);
    assert!(r.new_content.contains("beta\nTODO: done"));
    assert!(r.new_content.contains("alpha\nTODO: fix"));
}

/// Disk `replace_text` must honor pure `fuzzy: true` (no context).
///
/// Regression: the files/cli path used to drop `fuzzy` because plan
/// `Operation::Replace` has no fuzzy field and tx only fell back on context.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_pure_fuzzy_without_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn process_data() {}\n").unwrap();
    let opts = ReplaceOptions {
        fuzzy: true,
        require_change: true,
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "fn proccess_data() {}",
        "fn handle_data() {}",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .expect("pure fuzzy must resolve on disk path");
    assert!(result.changed, "got: {}", result.new_content);
    assert!(
        result.new_content.contains("handle_data"),
        "got: {}",
        result.new_content
    );
    assert_eq!(
        result.match_mode,
        Some(MatchMode::Fuzzy),
        "disk fuzzy must report Fuzzy, not Exact"
    );
    assert!(
        result.match_score.is_some_and(|s| s > 0.85),
        "score: {:?}",
        result.match_score
    );
}

#[test]
fn apply_content_edits_path_label() {
    let edits = [ContentEdit::Replace {
        old: "a".into(),
        new: "b".into(),
        options: ReplaceOptions::default(),
    }];
    let unlabeled = apply_content_edits("a\n", &edits).unwrap();
    assert!(
        unlabeled.diff.contains("<buffer>") || unlabeled.diff.contains("a/<buffer>"),
        "default label: {}",
        unlabeled.diff
    );

    let labeled = apply_content_edits_with_label("a\n", &edits, Some("src/lib.rs")).unwrap();
    assert!(
        labeled.diff.contains("src/lib.rs"),
        "path label in headers: {}",
        labeled.diff
    );
    assert!(
        !labeled.diff.contains("<buffer>"),
        "must not keep buffer placeholder when labeled: {}",
        labeled.diff
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn find_files_with_symbol_and_batch_compose() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    let c = dir.path().join("c.rs");
    fs::write(&a, "struct OldType {}\n").unwrap();
    fs::write(&b, "fn use_it(x: OldType) {}\n").unwrap();
    fs::write(&c, "struct Other {}\n").unwrap();

    let paths =
        find_files_with_symbol(dir.path(), "OldType", &SymbolSearchOptions::default()).unwrap();
    assert!(paths.iter().any(|p| p.ends_with("a.rs")), "a.rs: {paths:?}");
    assert!(paths.iter().any(|p| p.ends_with("b.rs")), "b.rs: {paths:?}");
    assert!(
        !paths.iter().any(|p| p.ends_with("c.rs")),
        "c.rs must not match: {paths:?}"
    );

    let path_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
    let results = ast_rename_batch(
        &path_refs,
        "OldType",
        "NewType",
        &AstRenameBatchOptions {
            mode: ApplyMode::Apply,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert!(results.iter().all(|r| r.result.is_ok()));
    assert!(fs::read_to_string(&a).unwrap().contains("NewType"));
    assert!(fs::read_to_string(&b).unwrap().contains("NewType"));
    assert!(fs::read_to_string(&c).unwrap().contains("Other"));
}

#[test]
fn content_edit_command_position_honored() {
    // #1666: ContentEdit::Replace path must honor command_position end-to-end.
    let edits = [ContentEdit::Replace {
        old: "pip".into(),
        new: "uv".into(),
        options: ReplaceOptions {
            command_position: true,
            require_change: true,
            ..Default::default()
        },
    }];
    let r = apply_content_edits("sudo pip install\nuv pip list\npipenv run\n", &edits).unwrap();
    assert!(r.changed);
    assert!(r.modified.contains("sudo uv install"));
    assert!(
        r.modified.contains("uv pip list"),
        "argument pip must stay: {}",
        r.modified
    );
    assert!(
        r.modified.contains("pipenv"),
        "longer token must stay: {}",
        r.modified
    );

    // Incompatible combo → InvalidInput
    let bad = [ContentEdit::Replace {
        old: "pip".into(),
        new: "uv".into(),
        options: ReplaceOptions {
            command_position: true,
            regex: true,
            ..Default::default()
        },
    }];
    let err = apply_content_edits("pip install\n", &bad).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn signature_rewrite_params_and_return_no_host_post_pass() {
    // #1661: structured field rewrite alone is complete (no second write).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("sig.rs");
    fs::write(&file, "pub fn process(a: i32) -> i32 {\n    a\n}\n").unwrap();
    let edit = crate::ast::rewrite::FunctionSigEdit {
        parameters: Some("(a: i32, b: i32)".into()),
        return_type: Some("-> i64".into()),
        ..Default::default()
    };
    let result = ast_rewrite_signature(&file, "process", &edit, None, ApplyMode::Apply, None)
        .expect("structured rewrite");
    assert!(result.applied && result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(
        on_disk.contains("-> i64 {"),
        "must keep space before brace without host post-pass: {on_disk}"
    );
    assert!(!on_disk.contains("i64{") && !on_disk.contains("){"));
    assert!(on_disk.contains("b: i32"), "params updated: {on_disk}");
}
