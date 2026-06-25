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
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(!on_disk.contains("\"b\""));
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
    assert!(
        err.to_string().contains("failed to read existing file"),
        "expected read error, got: {err}"
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
            "version": "1",
            "operations": [
                {
                    "op": "replace",
                    "path": "test.txt",
                    "mode": "literal",
                    "from": "hello",
                    "to": "goodbye"
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
    assert!(report.ok);
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
            "version": "1",
            "operations": [
                {{
                    "op": "replace",
                    "path": "{}",
                    "mode": "literal",
                    "from": "old",
                    "to": "new"
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
            "version": "1",
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

    // Create files for each thread.
    for i in 0..num_threads {
        let file = dir.path().join(format!("file_{i}.json"));
        fs::write(&file, format!(r#"{{"value": {i}}}"#)).unwrap();
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
                    serde_json::json!(i * 100),
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
        assert_eq!(parsed["value"], serde_json::json!(i * 100));
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
        err.to_string().contains("matched nothing"),
        "expected zero-match error, got: {err}"
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
        "version": "1",
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
    let (b, a) = build_context_lines(&lines, 1, 1);
    assert_eq!(b, vec!["a".to_string()]);
    assert_eq!(a, vec!["c".to_string()]);

    // formatter (human + json)
    let txt = format_search_results(&res, false);
    assert!(txt.contains("foo") || txt.contains("x.rs") || !txt.trim().is_empty());
    let js = format_search_results(&res, true);
    assert!(js.contains("\"column\"") || js.contains("column"));
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

    // prepend
    let res2 = file_prepend(&file, ">> ", ApplyMode::Apply, None).unwrap();
    assert!(res2.changed);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), ">> hello\n world");
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
