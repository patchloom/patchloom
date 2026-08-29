use super::*;
use std::fs;

use crate::containment::{AbsolutePathPolicy, PathGuard};
use tempfile::TempDir;

/// Process-global cwd is shared across parallel lib tests. Serialize any test
/// that calls `env::set_current_dir` and restore on drop (including panic).
/// Only used by relative-path tests that need `cli`/`files` absolutize.
#[cfg(any(feature = "cli", feature = "files"))]
mod cwd_guard {
    use std::sync::{Mutex, MutexGuard};

    static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

    pub(super) struct CwdGuard {
        prev: std::path::PathBuf,
        _lock: MutexGuard<'static, ()>,
    }

    impl CwdGuard {
        pub(super) fn enter(dir: &std::path::Path) -> Self {
            let lock = CWD_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = std::env::current_dir().expect("current_dir");
            std::env::set_current_dir(dir).expect("set_current_dir for test");
            Self { prev, _lock: lock }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev);
        }
    }
}

#[cfg(any(feature = "cli", feature = "files"))]
use cwd_guard::CwdGuard;

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

    let result = doc_merge(
        &file,
        serde_json::json!({"b": 2}),
        ApplyMode::Apply,
        None,
        None,
    )
    .unwrap();

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
fn doc_merge_multi_doc_selector_preserves_second_document() {
    // Library hosts must multi-doc merge without execute_plan (#1909).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("stream.yaml");
    fs::write(&file, "a: 1\nb: 2\n---\nx: 9\n").unwrap();

    let result = doc_merge(
        &file,
        serde_json::json!({"c": 3}),
        ApplyMode::Apply,
        None,
        Some("0"),
    )
    .unwrap();

    assert!(result.changed);
    assert!(result.applied);
    // Prefer typed gets over weak substring contains (MPI Test Auditor).
    assert_eq!(doc_get(&file, "0.a").unwrap(), serde_json::json!(1));
    assert_eq!(doc_get(&file, "0.b").unwrap(), serde_json::json!(2));
    assert_eq!(doc_get(&file, "0.c").unwrap(), serde_json::json!(3));
    assert_eq!(doc_get(&file, "1.x").unwrap(), serde_json::json!(9));
}

#[test]
fn doc_merge_multi_doc_bracket_index_selector() {
    // Docs claim Some("[0]") works; lock it at the library API (#1909 review).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("stream.yaml");
    fs::write(&file, "a: 1\n---\nx: 9\n").unwrap();

    let result = doc_merge(
        &file,
        serde_json::json!({"c": 3}),
        ApplyMode::Apply,
        None,
        Some("[0]"),
    )
    .unwrap();
    assert!(result.changed);
    assert_eq!(doc_get(&file, "0.c").unwrap(), serde_json::json!(3));
    assert_eq!(doc_get(&file, "1.x").unwrap(), serde_json::json!(9));
}

#[test]
fn doc_merge_multi_doc_root_object_overlay_is_type_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("stream.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let err = doc_merge(
        &file,
        serde_json::json!({"c": 3}),
        ApplyMode::Preview,
        None,
        None,
    )
    .unwrap_err();
    assert!(
        crate::exit::is_type_error(&err),
        "expected TypeErrorError, got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::TypeError),
        "root multi-doc merge must peel to TypeError for hosts (#1909)"
    );
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
fn file_create_force_soft_loads_unreadable_prior() {
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

    // Force create soft-loads unreadable prior (#1962). Write may still fail
    // if mode 000 blocks mutation; either success or a non-read staging error.
    let result = file_create(&file, "new", true, ApplyMode::Apply, None);
    // Restore permissions so TempDir cleanup succeeds.
    fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    match result {
        Ok(r) => {
            assert!(r.applied);
            assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        }
        Err(err) => {
            let msg = format!("{err:#}");
            // Must not fail only because prior read was for backup/diff.
            // OS may still refuse the write under mode 000.
            assert!(
                !msg.contains("target is a binary") && !msg.contains("UTF-8"),
                "force create must not hard-fail as content SoftSkip: {msg}"
            );
        }
    }
}

#[test]
fn file_create_force_overwrites_binary_prior() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("blob.bin");
    fs::write(&file, b"a\0b").unwrap();
    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();
    // Without force, dest-exists wins before content load.
    let err = file_create(&file, "text\n", false, ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert!(
        crate::fallback::is_already_exists(&err),
        "non-force create on existing binary is AlreadyExists: {err}"
    );
    let result = file_create(&file, "text\n", true, ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(result.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "text\n");
}

#[test]
fn file_create_force_overwrites_invalid_utf8_prior() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("bad.txt");
    fs::write(&file, b"hello\xffworld").unwrap();
    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();
    let result = file_create(&file, "fixed\n", true, ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(result.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "fixed\n");
}

#[test]
#[cfg(unix)]
fn file_create_force_binary_preserves_hardlinks() {
    // #1962: force overwrite of binary prior must not unlink+recreate (nlink stays).
    use std::os::unix::fs::MetadataExt;
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("blob.bin");
    let link = dir.path().join("blob-link.bin");
    fs::write(&file, b"a\0b").unwrap();
    std::fs::hard_link(&file, &link).unwrap();
    let nlink_before = fs::metadata(&file).unwrap().nlink();
    assert!(nlink_before >= 2, "hardlink setup: nlink={nlink_before}");
    let guard = PathGuard::builder(dir.path().to_path_buf())
        .allow_temp_directory()
        .build()
        .unwrap();
    let result = file_create(&file, "text\n", true, ApplyMode::Apply, Some(&guard)).unwrap();
    assert!(result.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "text\n");
    assert_eq!(
        fs::read_to_string(&link).unwrap(),
        "text\n",
        "hardlink sibling must see same content"
    );
    let nlink_after = fs::metadata(&file).unwrap().nlink();
    assert!(
        nlink_after >= 2,
        "force create must preserve hardlinks, nlink before={nlink_before} after={nlink_after}"
    );
}

#[test]
fn load_text_binary_peel_error_helper() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("x.bin");
    fs::write(&bin, b"x\0y").unwrap();
    let err = load_text(&bin).unwrap_err();
    let peeled = crate::fallback::peel_error(&err).expect("peel binary");
    assert_eq!(peeled.kind_str, "binary");
    assert!(peeled.message.contains("binary"), "msg: {}", peeled.message);
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn apply_content_edits_to_file_binary_is_binary() {
    use crate::api::{ContentEdit, apply_content_edits_to_file};
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("b.bin");
    fs::write(&file, b"a\x00b").unwrap();
    let edits = [ContentEdit::Replace {
        old: "a".into(),
        new: "z".into(),
        options: ReplaceOptions::default(),
    }];
    let err = apply_content_edits_to_file(&file, &edits, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::Binary),
        "content edits must not collapse Binary to OperationFailed: {err}"
    );
    assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn apply_content_edits_to_file_missing_is_not_found() {
    use crate::api::{ContentEdit, apply_content_edits_to_file};
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("gone.txt");
    let edits = [ContentEdit::Replace {
        old: "a".into(),
        new: "z".into(),
        options: ReplaceOptions::default(),
    }];
    let err = apply_content_edits_to_file(&file, &edits, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NotFound),
        "missing path must peel as NotFound not OperationFailed: {err}"
    );
    assert!(crate::api::is_not_found(&err), "is_not_found peel: {err}");
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
fn yaml_doc_set_pure_alias_becomes_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "shared: &shared\n  timeout: 30\n  retries: 3\nservice_a: *shared\nservice_b: *shared\n",
    )
    .unwrap();

    let result = doc_set(
        &file,
        "service_a.timeout",
        serde_json::json!(60),
        ApplyMode::Apply,
        None,
    )
    .unwrap();

    assert!(result.changed);
    let on_disk = fs::read_to_string(&file).unwrap();
    assert!(
        on_disk.contains("&shared") && on_disk.contains("<<: *shared"),
        "library doc_set must convert alias to merge:\n{on_disk}"
    );
    assert!(
        on_disk.contains("service_b: *shared"),
        "sibling alias must remain:\n{on_disk}"
    );
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
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "execute_plan PathGuard must peel as GuardRejected: {err}"
    );
    // No mutation
    assert!(fs::read_to_string(&file).unwrap().contains("content"));
}

/// #2169: library `ast,files` must expand for_each (not a silent no-op).
#[test]
#[cfg(feature = "files")]
fn execute_plan_for_each_expands_without_cli() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "old\n").unwrap();
    fs::write(dir.path().join("b.txt"), "old\n").unwrap();
    let plan = parse_plan(
        r#"{
            "version": 1,
            "for_each": {"glob": "*.txt"},
            "operations": [
                {"op": "replace", "path": "{path}", "old": "old", "new": "new"}
            ]
        }"#,
    )
    .unwrap();
    let report = execute_plan(plan, dir.path(), None).unwrap();
    assert!(report.ok, "for_each execute_plan must succeed: {report:?}");
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "new\n"
    );
    assert!(
        !dir.path().join("{path}").exists(),
        "unexpanded {{path}} must not be written"
    );
}

/// #2169: zero-match glob fails closed on the library execute_plan path.
#[test]
#[cfg(feature = "files")]
fn execute_plan_for_each_zero_match_is_no_match() {
    let dir = TempDir::new().unwrap();
    let plan = parse_plan(
        r#"{
            "version": 1,
            "for_each": {"glob": "*.nope"},
            "operations": [
                {"op": "replace", "path": "{path}", "old": "old", "new": "new"}
            ]
        }"#,
    )
    .unwrap();
    let err = execute_plan(plan, dir.path(), None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch),
        "zero-match for_each must peel no_matches: {err}"
    );
}

/// #2169: after for_each expand, PathGuard still rejects `../escape`
/// (order lock is execute_plan_direct: expand, then refuse_lifecycle, then ops).
#[test]
#[cfg(feature = "files")]
fn execute_plan_for_each_expands_before_path_guard() {
    let dir = TempDir::new().unwrap();
    let ws = dir.path().join("ws");
    fs::create_dir(&ws).unwrap();
    fs::write(ws.join("ok.txt"), "old\n").unwrap();
    let guard = PathGuard::new(ws.clone(), AbsolutePathPolicy::AllowIfContained).unwrap();
    let plan = parse_plan(
        r#"{
            "version": 1,
            "for_each": {"glob": "*.txt"},
            "operations": [
                {"op": "replace", "path": "{path}", "old": "old", "new": "new"},
                {"op": "file.create", "path": "../escape.txt", "content": "leaked"}
            ]
        }"#,
    )
    .unwrap();
    let err = execute_plan(plan, &ws, Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "expanded plan must still PathGuard ../escape.txt: {err}"
    );
    assert_eq!(
        fs::read_to_string(ws.join("ok.txt")).unwrap(),
        "old\n",
        "guard refuse must happen before commit"
    );
    assert!(
        !dir.path().join("escape.txt").exists(),
        "../escape.txt must not be created"
    );
    assert!(
        !ws.join("{path}").exists(),
        "unexpanded {{path}} must not be written"
    );
}

/// #2169: plan.cwd + for_each is rejected on the library execute path.
#[test]
#[cfg(feature = "files")]
fn execute_plan_for_each_rejects_plan_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "old\n").unwrap();
    let plan = parse_plan(
        r#"{
            "version": 1,
            "cwd": "nested",
            "for_each": {"glob": "*.txt"},
            "operations": [
                {"op": "replace", "path": "{path}", "old": "old", "new": "new"}
            ]
        }"#,
    )
    .unwrap();
    let err = execute_plan(plan, dir.path(), None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "cwd+for_each must peel invalid_input: {err}"
    );
    assert!(
        err.to_string().contains("for_each"),
        "expected cwd+for_each diagnostic: {err}"
    );
}

/// #2168: PathGuard refuses format redirects before commit.
#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn execute_plan_guard_refuses_format_redirect() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.txt"), "keep\n").unwrap();
    let escape = dir.path().join("escape.env");
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let plan = parse_plan(&format!(
        r#"{{
            "version": 1,
            "operations": [{{"op": "file.create", "path": "ok.txt", "content": "x", "force": true}}],
            "format": [{{"cmd": "printf secret > {}"}}]
        }}"#,
        escape.display().to_string().replace('\\', "/")
    ))
    .unwrap();
    let err = execute_plan(plan, dir.path(), Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "format redirect under PathGuard must peel GuardRejected: {err}"
    );
    assert!(
        !escape.exists(),
        "redirect must not run when PathGuard is set"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("ok.txt")).unwrap(),
        "keep\n"
    );
}

/// #2168: `true` / `cargo fmt`-shaped cmds still run under a guard.
#[test]
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
fn execute_plan_guard_allows_plain_true() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.txt"), "old\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let plan = parse_plan(
        r#"{
            "version": 1,
            "operations": [
                {"op": "replace", "path": "ok.txt", "old": "old", "new": "new"}
            ],
            "format": [{"cmd": "true"}]
        }"#,
    )
    .unwrap();
    let report = execute_plan(plan, dir.path(), Some(&guard)).unwrap();
    assert!(report.ok, "plain true format step must run: {report:?}");
    assert_eq!(
        fs::read_to_string(dir.path().join("ok.txt")).unwrap(),
        "new\n"
    );
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
    // One shared backup session across files (atomic multi-file apply).
    let sessions: Vec<_> = results
        .iter()
        .filter_map(|r| r.backup_session.clone())
        .collect();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0], sessions[1], "both files share one session");
}

/// Patch rename must not overwrite an existing destination (file.rename parity).
#[test]
fn apply_patch_file_rename_refuses_existing_dest() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.rs"), "source\n").unwrap();
    fs::write(dir.path().join("new.rs"), "dest-existing\n").unwrap();

    let patch = "\
diff --git a/old.rs b/new.rs\n\
similarity index 100%\n\
rename from old.rs\n\
rename to new.rs\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_already_exists(&err),
        "expected already_exists, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("old.rs")).unwrap(),
        "source\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new.rs")).unwrap(),
        "dest-existing\n"
    );
}

#[test]
fn apply_patch_file_copy_creates_dest_keeps_source() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.rs"), "body\n").unwrap();

    let patch = "\
diff --git a/foo.rs b/bar.rs\n\
similarity index 100%\n\
copy from foo.rs\n\
copy to bar.rs\n";

    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "bar.rs");
    assert!(results[0].changed);
    assert_eq!(
        fs::read_to_string(dir.path().join("foo.rs")).unwrap(),
        "body\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bar.rs")).unwrap(),
        "body\n"
    );
}

#[test]
fn apply_patch_file_copy_refuses_existing_dest() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("foo.rs"), "src\n").unwrap();
    fs::write(dir.path().join("bar.rs"), "taken\n").unwrap();

    let patch = "\
diff --git a/foo.rs b/bar.rs\n\
similarity index 100%\n\
copy from foo.rs\n\
copy to bar.rs\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_already_exists(&err),
        "expected already_exists, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("foo.rs")).unwrap(),
        "src\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bar.rs")).unwrap(),
        "taken\n"
    );
}

#[test]
fn apply_patch_file_copy_missing_source_is_not_found() {
    let dir = TempDir::new().unwrap();
    let patch = "\
diff --git a/foo.rs b/bar.rs\n\
similarity index 100%\n\
copy from foo.rs\n\
copy to bar.rs\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Preview, None).unwrap_err();
    assert!(
        crate::api::is_not_found(&err),
        "expected not_found, got: {err}"
    );
    assert!(!dir.path().join("bar.rs").exists());
}

#[test]
fn apply_patch_file_empty_create_refuses_existing_dest() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".env"), "keep\n").unwrap();
    let patch = "\
diff --git a/ok.txt b/.env\n\
new file mode 100644\n\
index 0000000..e69de29\n";
    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_already_exists(&err),
        "expected already_exists, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".env")).unwrap(),
        "keep\n"
    );
}

#[test]
fn apply_patch_file_second_create_same_dest_is_already_exists() {
    let dir = TempDir::new().unwrap();
    let patch = "\
--- /dev/null
+++ b/new.rs
@@ -0,0 +1 @@
+first
--- /dev/null
+++ b/new.rs
@@ -0,0 +1 @@
+second
";
    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_already_exists(&err),
        "expected already_exists, got: {err}"
    );
    assert!(
        !dir.path().join("new.rs").exists(),
        "apply must not write dest after refuse"
    );
}

#[test]
fn apply_patch_file_recreate_after_rename_from() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.rs"), "fn main() {}\n").unwrap();
    let patch = "\
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
diff --git a/old.rs b/old.rs
new file mode 100644
--- /dev/null
+++ b/old.rs
@@ -0,0 +1 @@
+replaced
";
    let preview = apply_patch_file(patch, dir.path(), ApplyMode::Preview, None).unwrap();
    assert!(
        preview
            .iter()
            .any(|r| r.path.ends_with("new.rs") || r.changed),
        "rename+recreate preview must not be already_exists: {preview:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("old.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn apply_patch_file_empty_create_writes_empty_dest() {
    let dir = TempDir::new().unwrap();
    let patch = "\
diff --git a/ok.txt b/.empty\n\
new file mode 100644\n\
index 0000000..e69de29\n";
    let preview = apply_patch_file(patch, dir.path(), ApplyMode::Preview, None).unwrap();
    assert!(
        preview[0].changed,
        "empty-create Preview must set changed=true"
    );
    assert!(!dir.path().join(".empty").exists());
    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, ".empty");
    assert!(results[0].changed);
    assert_eq!(fs::read_to_string(dir.path().join(".empty")).unwrap(), "");
}

#[test]
fn apply_patch_file_deleted_file_mode_unlinks() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("gone.rs"), "").unwrap();
    let patch = "\
diff --git a/gone.rs b/gone.rs\n\
deleted file mode 100644\n\
index e69de29..0000000\n";
    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].changed);
    assert!(!dir.path().join("gone.rs").exists());
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn execute_plan_empty_create_refuses_existing_dest() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".env"), "keep\n").unwrap();
    let plan = crate::plan::Plan {
        version: 1,
        cwd: Some(dir.path().to_string_lossy().into()),
        write_policy: None,
        strict: None,
        operations: vec![crate::plan::Operation::PatchApply {
            diff: "diff --git a/ok.txt b/.env\nnew file mode 100644\nindex 0000000..e69de29\n"
                .into(),
            on_stale: Default::default(),
            allow_conflicts: false,
            replace_all: false,
        }],
        format: None,
        validate: None,
        verify: None,
        for_each: None,
    };
    let report = execute_plan(plan, dir.path(), None).expect("plan report");
    assert!(!report.ok, "empty-create dest-exists must fail: {report:?}");
    assert_eq!(
        report.error_kind.as_deref(),
        Some("already_exists"),
        "{report:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".env")).unwrap(),
        "keep\n"
    );
}

#[test]
fn patch_dest_helpers_public() {
    assert_eq!(unquote_git_c_string("\\056env"), ".env");
    assert_eq!(parse_diff_file_path(r#"+++ "b/\056env""#), ".env");
    let paths = patch_declared_paths(
        "\
diff --git a/foo.rs b/bar.rs
similarity index 100%
copy from foo.rs
copy to bar.rs
",
    )
    .unwrap();
    assert!(paths.contains(&"bar.rs".into()));
    let files = parse_unified_diff(
        "\
diff --git a/foo.rs b/bar.rs
similarity index 100%
copy from foo.rs
copy to bar.rs
",
    )
    .unwrap();
    assert_eq!(files[0].path, "bar.rs");
    assert!(files[0].rename_from.is_none());
    assert_eq!(files[0].copy_from.as_deref(), Some("foo.rs"));
}

#[test]
fn apply_patch_file_c_quoted_bel_dest_is_not_letter_a() {
    // #2175: `\a` is BEL (0x07), not the letter `a`. Windows rejects
    // control characters in filenames, so persist is Unix-only.
    let dest_name = format!("foo{}bar.rs", '\u{7}');
    assert_eq!(parse_diff_file_path(r#"+++ "b/foo\abar.rs""#), dest_name);
    let dir = TempDir::new().unwrap();
    let patch = "\
--- /dev/null
+++ \"b/foo\\abar.rs\"
@@ -0,0 +1 @@
+secret
";
    #[cfg(unix)]
    {
        let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, dest_name);
        assert_eq!(
            fs::read_to_string(dir.path().join(&dest_name)).unwrap(),
            "secret\n"
        );
        assert!(!dir.path().join("fooabar.rs").exists());
    }
    #[cfg(windows)]
    {
        let _ = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None);
        assert!(!dir.path().join("fooabar.rs").exists());
    }
}

/// Pure rename Preview: content may be unchanged, but path moves so `changed`
/// must be true and path/dest_path must report old → new for host branching.
#[test]
fn apply_patch_file_pure_rename_preview_sets_changed_and_paths() {
    let dir = TempDir::new().unwrap();
    let old = dir.path().join("old.rs");
    fs::write(&old, "fn main() {}\n").unwrap();

    let patch = "\
diff --git a/old.rs b/new.rs\n\
similarity index 100%\n\
rename from old.rs\n\
rename to new.rs\n";

    let results = apply_patch_file(patch, dir.path(), ApplyMode::Preview, None).unwrap();
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(
        r.changed,
        "pure rename Preview must set changed=true even when content equals original"
    );
    assert!(!r.applied);
    assert_eq!(r.path, "old.rs");
    assert_eq!(r.dest_path.as_deref(), Some("new.rs"));
    assert_eq!(r.new_content, "fn main() {}\n");
    // Preview must not mutate the tree.
    assert!(old.exists(), "source still present in Preview");
    assert!(!dir.path().join("new.rs").exists());
}

/// C-quoted pure rename through the library API (spaces in paths).
#[test]
fn apply_patch_file_pure_rename_c_quoted_preview() {
    let dir = TempDir::new().unwrap();
    let old = dir.path().join("old name.rs");
    fs::write(&old, "body\n").unwrap();

    let patch = "\
diff --git \"a/old name.rs\" \"b/new name.rs\"\n\
similarity index 100%\n\
rename from \"old name.rs\"\n\
rename to \"new name.rs\"\n";

    let results = apply_patch_file(patch, dir.path(), ApplyMode::Preview, None).unwrap();
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(r.changed);
    assert_eq!(r.path, "old name.rs");
    assert_eq!(r.dest_path.as_deref(), Some("new name.rs"));

    let applied = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(applied.len(), 1);
    assert!(applied[0].changed && applied[0].applied);
    assert_eq!(applied[0].path, "old name.rs");
    assert_eq!(applied[0].dest_path.as_deref(), Some("new name.rs"));
    assert!(!old.exists(), "source removed on Apply");
    assert_eq!(
        fs::read_to_string(dir.path().join("new name.rs")).unwrap(),
        "body\n"
    );
}

/// Multi-file Apply must not leave a half-applied tree when a later file fails
/// hunk preflight (stale context). Previously wrote earlier files then Err.
#[test]
fn apply_patch_file_stale_second_file_leaves_first_unchanged() {
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
-not-the-content\n\
+BBB\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("stale") || msg.contains("patch apply"),
        "expected hunk failure, got: {msg}"
    );
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        "aaa\n",
        "first file must not be half-applied when later file fails preflight"
    );
    assert_eq!(fs::read_to_string(&b).unwrap(), "bbb\n");
}

/// Pure rename then a later write failure must restore source and remove dest
/// (rename pairs backed up as Deleted + Created; fixloop #2120 / MPI follow-up).
#[test]
fn apply_patch_file_mid_write_after_pure_rename_restores() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    fs::write(&a, "src-content\n").unwrap();
    // Parent that cannot host a child (blocks second create after rename).
    let nested = dir.path().join("nested");
    fs::write(&nested, "block\n").unwrap();

    let patch = "\
diff --git a/a.txt b/b.txt\n\
similarity index 100%\n\
rename from a.txt\n\
rename to b.txt\n\
--- /dev/null\n\
+++ b/nested/child.txt\n\
@@ -0,0 +1 @@\n\
+orphan\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nested")
            || msg.contains("Not a directory")
            || msg.contains("not a directory")
            || msg.contains("failed")
            || msg.contains("restore"),
        "expected mid-write failure after rename, got: {msg}"
    );
    assert!(
        a.exists(),
        "source a.txt must be restored after rename + later failure"
    );
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        "src-content\n",
        "restored source content"
    );
    assert!(
        !dir.path().join("b.txt").exists(),
        "rename dest b.txt must be removed on restore"
    );
}

/// Multi-file patch: create then a write that fails mid-batch must remove the
/// created path (Created backup). Without backing up missing paths, restore
/// left orphan creates (fixloop 2026-08-02).
#[test]
fn apply_patch_file_mid_write_after_create_restores_orphan() {
    let dir = TempDir::new().unwrap();
    // Parent path that cannot host a child file (regular file, not directory).
    let nested = dir.path().join("nested");
    fs::write(&nested, "block\n").unwrap();

    // First file: create ok. Second: create nested/child.txt fails because
    // parent `nested` is a file (preflight is creation-only; no load of parent).
    let patch = "\
--- /dev/null\n\
+++ b/new.txt\n\
@@ -0,0 +1 @@\n\
+created\n\
--- /dev/null\n\
+++ b/nested/child.txt\n\
@@ -0,0 +1 @@\n\
+orphan\n";

    let err = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nested")
            || msg.contains("Not a directory")
            || msg.contains("not a directory")
            || msg.contains("failed")
            || msg.contains("restore"),
        "expected mid-write failure, got: {msg}"
    );
    assert!(
        !dir.path().join("new.txt").exists(),
        "created new.txt must be restored (removed) when later write fails; orphan left behind"
    );
    assert!(
        !dir.path().join("nested").join("child.txt").exists(),
        "failed second create must not leave nested/child.txt"
    );
    assert_eq!(
        fs::read_to_string(&nested).unwrap(),
        "block\n",
        "blocking parent file must stay intact"
    );
}

/// Mid-write failure after a successful first write must restore all files.
#[test]
fn write_if_apply_many_restores_on_second_write_failure() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    // Missing parent: atomic_write does not mkdir, so NamedTempFile::new_in
    // fails after `a` is written. Dest-as-directory cannot be this fixture:
    // dest-dir refuse happens before any sibling write.
    let b = dir.path().join("no_such_dir").join("b.txt");
    fs::write(&a, "aaa\n").unwrap();

    let policy = crate::write::WritePolicy::default();
    let files: [(&std::path::Path, &str); 2] = [(&a, "AAA\n"), (&b, "BBB\n")];
    assert!(
        super::write_if_apply_many(&files, ApplyMode::Apply, &policy, None, dir.path()).is_err()
    );
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        "aaa\n",
        "first write must be rolled back when later write fails"
    );
}

/// Dest that is already a directory is refused before any sibling write.
#[test]
fn write_if_apply_many_dest_dir_does_not_write_sibling() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "aaa\n").unwrap();
    fs::create_dir(&b).unwrap();

    let policy = crate::write::WritePolicy::default();
    let files: [(&std::path::Path, &str); 2] = [(&a, "AAA\n"), (&b, "BBB\n")];
    let err = super::write_if_apply_many(&files, ApplyMode::Apply, &policy, None, dir.path())
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("directory"),
        "dest-dir must be invalid_input, got: {msg}"
    );
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        "aaa\n",
        "sibling must stay original when dest-dir is refused up front"
    );
}

/// Mutation failure after finalize must leave original content and a listable session.
#[test]
fn apply_mutation_restores_on_perform_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "original\n").unwrap();

    let err = super::apply_mutation(
        &file,
        ApplyMode::Apply,
        None,
        |backup| backup.save_before_write(&file),
        || anyhow::bail!("simulated write failure after backup finalize"),
    )
    .unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("simulated write failure"),
        "mutation error must remain visible: {msg}"
    );
    assert!(
        msg.contains("restored session") || msg.contains("backup finalize"),
        "hosts must see restore outcome + session, got: {msg}"
    );
    // #2127: hosts peel session without Display scrape.
    let session = super::backup_session_from_error(&err).expect("session after fail-restore");
    assert!(
        !session.is_empty(),
        "backup_session_from_error must return finalized session id"
    );
    let sessions = crate::backup::list_sessions(dir.path()).unwrap();
    assert!(
        sessions.iter().any(|s| s.timestamp == session),
        "peeled session {session:?} must match list_sessions: {sessions:?}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
    assert!(
        !sessions.is_empty(),
        "finalize-before-mutate must leave a discoverable session for undo"
    );
}

#[test]
fn backup_session_from_error_format_failed_with_session() {
    let err: anyhow::Error = super::FormatFailedError::new("format command failed")
        .with_backup_session(Some("fmt_1".into()))
        .into();
    assert_eq!(super::backup_session_from_error(&err), Some("fmt_1"));
    assert_eq!(super::format_failed_backup_session(&err), Some("fmt_1"));
}

#[test]
fn backup_session_from_error_none_for_no_match() {
    let err: anyhow::Error = crate::exit::NoMatchError {
        msg: "no matches".into(),
    }
    .into();
    assert_eq!(super::backup_session_from_error(&err), None);
}

#[test]
fn backup_session_from_error_none_for_guard_rejected() {
    let err: anyhow::Error = crate::fallback::EditError::new(
        crate::fallback::EditErrorKind::GuardRejected,
        "path outside workspace",
    )
    .into();
    assert_eq!(super::backup_session_from_error(&err), None);
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
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "original\n",
        "must not rewrite notb/foo.rs when the patch dest is b/foo.rs"
    );
    match result {
        Ok(r) => assert!(!r.changed, "b/foo.rs must not match notb/foo.rs: {r:?}"),
        Err(e) => {
            let msg = format!("{e:#}");
            assert!(
                msg.contains("b/foo.rs"),
                "mismatch must name the patch dest b/foo.rs, got: {msg}"
            );
            assert!(
                !msg.contains("notb/foo.rs"),
                "must not treat notb/foo.rs as the dest: {msg}"
            );
        }
    }
}

#[test]
fn make_write_policy_maps_options() {
    let opts = WritePolicyOptions {
        ensure_final_newline: true,
        normalize_eol: Some(EolMode::Lf),
        trim_trailing_whitespace: true,
        collapse_blanks: true,
        ..Default::default()
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
    assert!(
        result.backup_session.is_some(),
        "cross-file move must report one backup session covering both files"
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
fn doc_get_array_root_bare_key_is_type_error() {
    // Multi-doc YAML / top-level JSON array: bare key must not soft no_match.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let err = doc_get(&file, "a").unwrap_err();
    assert!(
        crate::exit::is_type_error(&err),
        "expected TypeErrorError, got: {err}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("0.a") || msg.contains("[0].a"),
        "index hint missing: {msg}"
    );
    // Library hosts branch on kind without scraping English (#1883).
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::TypeError),
        "TypeErrorError must peel to EditErrorKind::TypeError, not InvalidInput"
    );
    assert_eq!(doc_get(&file, "0.a").unwrap(), serde_json::json!(1));
}

#[test]
fn empty_replace_pattern_is_invalid_input_not_type_error() {
    // Sibling-path honesty for #1883: empty pattern stays InvalidInput.
    let err = replace_in_content("a b", "", "x", &ReplaceOptions::default()).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput)
    );
}

#[test]
fn load_text_api_rejects_binary_and_loads_utf8() {
    // api::load_text discoverability (#1910)
    let dir = TempDir::new().unwrap();
    let text = dir.path().join("ok.txt");
    fs::write(&text, "hello\n").unwrap();
    assert_eq!(load_text(&text).unwrap(), "hello\n");

    let bin = dir.path().join("blob.bin");
    fs::write(&bin, b"a\0b").unwrap();
    let err = load_text(&bin).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::Binary)
    );
    assert!(crate::fallback::is_binary(&err), "is_binary peel: {err}");
    assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
    assert!(
        is_binary_file(&bin),
        "is_binary_file should detect NUL probe"
    );
    assert!(!is_binary_file(&text));

    // Invalid UTF-8 peels to InvalidEncoding (#1963).
    let bad = dir.path().join("bad.txt");
    fs::write(&bad, b"hello\xffworld").unwrap();
    let err = load_text(&bad).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidEncoding),
        "invalid UTF-8 must be InvalidEncoding, got: {err}"
    );
    assert!(
        crate::fallback::is_invalid_encoding(&err),
        "is_invalid_encoding peel: {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("invalid_encoding")
    );
    let msg = format!("{err:#}");
    assert!(
        msg.contains("UTF-8") || msg.contains("utf-8") || msg.contains("invalid"),
        "message should name encoding issue: {msg}"
    );

    let missing = dir.path().join("no-such.txt");
    let err = load_text(&missing).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("failed to read"),
        "missing path must keep load_text_strict context: {msg}"
    );
    assert_ne!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "missing path is IO, not binary/UTF-8 InvalidInput: {msg}"
    );
}

#[test]
fn doc_set_multi_document_bare_key_is_type_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.yaml");
    fs::write(&file, "a: 1\n---\nb: 2\n").unwrap();

    let err = doc_set(&file, "a", serde_json::json!(9), ApplyMode::Preview, None).unwrap_err();
    assert!(
        crate::exit::is_type_error(&err),
        "expected TypeErrorError, got: {err}"
    );
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::TypeError)
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
fn replace_text_nth_out_of_range_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "foo bar foo\n").unwrap();

    let opts = ReplaceOptions {
        nth: Some(5),
        ..ReplaceOptions::default()
    };
    let err = replace_text(&file, "foo", "qux", &opts, ApplyMode::Preview, None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nth 5 is out of range") && msg.contains("matches 2 times"),
        "got: {msg}"
    );
    assert_eq!(
        crate::api::edit_error_kind(&err),
        Some(crate::api::EditErrorKind::InvalidInput)
    );
}

#[test]
fn replace_in_content_nth_out_of_range() {
    let opts = ReplaceOptions {
        nth: Some(3),
        whole_line: true,
        ..ReplaceOptions::default()
    };
    // Two matching lines; three substring "a"s.
    let err = replace_in_content("a a\na\n", "a", "X", &opts).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nth 3 is out of range") && msg.contains("matches 2 times"),
        "got: {msg}"
    );
}

#[test]
fn replace_text_insert_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "hello world\n").unwrap();

    // Leading space looks like a new-line payload (#1885): insert on next line.
    let opts = ReplaceOptions {
        insert_after: Some(" beautiful".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "hello", "", &opts, ApplyMode::Preview, None).unwrap();

    assert!(result.changed);
    assert_eq!(
        result.new_content, "hello\n beautiful world\n",
        "line-oriented insert_after must be exact"
    );
}

/// #2209 sibling: insert_after payload that already ends in a newline
/// must not add a blank line (CLI cargo-bin already locks this).
#[test]
fn replace_text_insert_after_trailing_nl_does_not_add_blank_line() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn foo() {\n  a();\n}\n").unwrap();

    let opts = ReplaceOptions {
        insert_after: Some("  bar();\n".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "fn foo() {", "", &opts, ApplyMode::Apply, None).unwrap();
    assert!(result.applied);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "fn foo() {\n  bar();\n  a();\n}\n",
        "trailing NL on insert_after must not insert a blank line"
    );
}

#[test]
fn replace_text_sole_binary_is_binary() {
    // #1894 / #1963: library replace_text matches CLI/MCP strict sole-path (Binary).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("bin.dat");
    fs::write(&file, b"hello\x00world").unwrap();

    let err = replace_text(
        &file,
        "hello",
        "HELLO",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap_err();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::Binary),
        "expected Binary, got {err:#}"
    );
    assert_eq!(error_kind_str(&err), Some("binary"));
    assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");
}

#[test]
fn replace_text_insert_after_preserves_crlf() {
    // fixrealloop: whole-line insert into CRLF files must use \r\n, not bare LF.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("win.txt");
    fs::write(&file, "line1\r\nTARGET\r\nline3\r\n").unwrap();

    let opts = ReplaceOptions {
        insert_after: Some("// post".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "TARGET", "", &opts, ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    assert!(result.applied);
    let on_disk = fs::read(&file).unwrap();
    assert_eq!(
        on_disk,
        b"line1\r\nTARGET\r\n// post\r\nline3\r\n",
        "insert_after must preserve CRLF separators: {:?}",
        String::from_utf8_lossy(&on_disk)
    );
}

#[test]
fn replace_text_insert_before_preserves_crlf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("win.txt");
    fs::write(&file, "A\r\nB\r\n").unwrap();

    let opts = ReplaceOptions {
        insert_before: Some("PRE".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "B", "", &opts, ApplyMode::Apply, None).unwrap();
    assert!(result.changed);
    let on_disk = fs::read(&file).unwrap();
    assert_eq!(
        on_disk,
        b"A\r\nPRE\r\nB\r\n",
        "insert_before must preserve CRLF: {:?}",
        String::from_utf8_lossy(&on_disk)
    );
}

/// CLI/tx already lock insert-before trailing NL. Library must match.
#[test]
fn replace_text_insert_before_trailing_nl_does_not_add_blank_line() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "    /// Doc comment.\n    pub field: bool,\n").unwrap();

    let opts = ReplaceOptions {
        insert_before: Some("    // marker\n".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(
        &file,
        "    /// Doc comment.",
        "",
        &opts,
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(result.applied);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "    // marker\n    /// Doc comment.\n    pub field: bool,\n",
        "trailing NL on insert_before must not insert a blank line"
    );
}

#[test]
fn replace_text_insert_after_midline_bare() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "hello world\n").unwrap();

    let opts = ReplaceOptions {
        insert_after: Some("X".to_string()),
        ..ReplaceOptions::default()
    };
    let result = replace_text(&file, "hello", "", &opts, ApplyMode::Preview, None).unwrap();
    assert!(result.changed);
    assert!(
        result.new_content.contains("helloX world"),
        "mid-line bare insert stays byte-exact: {}",
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
    std::fs::write(dir.path().join("hidden.txt"), "hidden secret\n").unwrap();

    // Create a custom agent/tool ignore file
    std::fs::write(dir.path().join(".agentignore"), "hidden.txt\n").unwrap();

    let opts = SearchOptions {
        regex: true,
        exclude_patterns: vec!["*ignore*".to_string()],
        custom_ignore_filenames: vec![".agentignore".to_string()],
        ..Default::default()
    };
    let results = search_directory(dir.path(), "keep|me|secret", &opts).unwrap();
    // Only the keep file should match; excludes + custom ignore filtered the rest.
    assert_eq!(results.len(), 1);
    assert!(results[0].line.contains("keep"));
}

#[test]
#[cfg(any(feature = "cli", feature = "files"))]
fn search_parity_custom_ignore_across_api_cli_and_plan() {
    // Cross-surface parity test for #821: same inputs via api, CLI collect, and tx plan Search
    // produce equivalent rich results (counts/paths) when using custom ignore + exclude + globs + max.
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("keep.rs"), "keep foo here\n").unwrap();
    std::fs::write(root.join("skip.rs"), "skip foo\n").unwrap();
    std::fs::write(root.join("hidden.txt"), "hidden foo secret\n").unwrap();
    std::fs::write(root.join("other.txt"), "other foo\n").unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("target/bad.rs"), "bad foo\n").unwrap();
    std::fs::write(root.join(".agentignore"), "hidden.txt\ntarget/\n").unwrap();
    std::fs::write(root.join(".gitignore"), "").unwrap();

    let pattern = "foo";
    let opts = SearchOptions {
        literal: true,
        globs: vec!["*.rs".to_string()],
        exclude_patterns: vec!["*skip*".to_string()],
        custom_ignore_filenames: vec![".agentignore".to_string()],
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
            files_without_match: false,
            count: false,
            invert_match: false,
            multiline: false,
            case_insensitive: false,
            assert_count: None,
            max_results: opts.max_results,
            unique: false,
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
            "custom_ignore_filenames": [".agentignore"],
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
        !api_paths.iter().any(|p| p.contains("hidden")
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

/// Preview / Check leave disk unchanged; Apply writes. One table covers all three.
#[test]
fn file_append_write_modes_matrix() {
    for (mode, expect_applied) in [
        (ApplyMode::Preview, false),
        (ApplyMode::Check, false),
        (ApplyMode::Apply, true),
    ] {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("modes.txt");
        fs::write(&file, "base").unwrap();
        let res = file_append(&file, " +more", mode, None).unwrap();
        assert!(res.changed, "{mode:?}");
        assert_eq!(res.applied, expect_applied, "{mode:?}");
        let on_disk = fs::read_to_string(&file).unwrap();
        if expect_applied {
            assert_eq!(
                on_disk, "base\n +more",
                "{mode:?} append inserts one EOL when the file has none"
            );
        } else {
            assert_eq!(on_disk, "base", "{mode:?} must not write");
            assert_eq!(
                res.new_content, "base\n +more",
                "{mode:?} preview new_content must be exact"
            );
        }
    }
}

/// Same Preview/Check/Apply contract as append (prepend only mutates on Apply).
#[test]
fn file_prepend_write_modes_matrix() {
    for (mode, expect_applied) in [
        (ApplyMode::Preview, false),
        (ApplyMode::Check, false),
        (ApplyMode::Apply, true),
    ] {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("pre_modes.txt");
        fs::write(&file, "base").unwrap();
        let res = file_prepend(&file, "HEAD\n", mode, None).unwrap();
        assert!(res.changed, "{mode:?}");
        assert_eq!(res.applied, expect_applied, "{mode:?}");
        let on_disk = fs::read_to_string(&file).unwrap();
        if expect_applied {
            assert_eq!(
                on_disk, "HEAD\nbase",
                "{mode:?} prepend must be exact (no extra blank)"
            );
        } else {
            assert_eq!(on_disk, "base", "{mode:?} must not write");
            assert_eq!(
                res.new_content, "HEAD\nbase",
                "{mode:?} preview new_content must be exact"
            );
        }
    }
}

/// New path: Preview/Check report change without creating; Apply creates.
#[test]
fn file_create_write_modes_matrix() {
    for (mode, expect_applied) in [
        (ApplyMode::Preview, false),
        (ApplyMode::Check, false),
        (ApplyMode::Apply, true),
    ] {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("new_modes.txt");
        assert!(!file.exists(), "fixture must start missing");
        let res = file_create(&file, "body\n", false, mode, None).unwrap();
        assert!(res.changed, "{mode:?}");
        assert_eq!(res.applied, expect_applied, "{mode:?}");
        if expect_applied {
            assert!(file.exists(), "{mode:?} should create");
            assert_eq!(fs::read_to_string(&file).unwrap(), "body\n");
        } else {
            assert!(!file.exists(), "{mode:?} must not create on disk");
            assert!(
                res.new_content.contains("body"),
                "{mode:?} new_content should preview create"
            );
        }
    }
}

/// Existing path without force: all modes refuse with already_exists (engine + no-files parity).
#[test]
fn file_create_existing_without_force_fails_all_modes() {
    for mode in [ApplyMode::Preview, ApplyMode::Check, ApplyMode::Apply] {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("exists.txt");
        fs::write(&file, "prior\n").unwrap();
        let err = file_create(&file, "new\n", false, mode, None).unwrap_err();
        assert!(
            crate::api::is_already_exists(&err),
            "{mode:?} expected already_exists, got kind={:?} err={err}",
            crate::fallback::edit_error_kind(&err)
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "prior\n",
            "{mode:?} must not overwrite without force"
        );
    }
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
        if_exists: false,
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
        if_exists: false,
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
        if_exists: false,
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
        if_exists: false,
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
        if_exists: false,
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
        fuzzy: false,
        min_fuzzy_score: None,
        allow_absent_old: false,
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
fn replace_options_for_agent_matches_documented_policy() {
    use crate::api::{AGENT_MIN_FUZZY_SCORE, ReplaceOptions};
    let d = ReplaceOptions::default();
    let a = ReplaceOptions::for_agent();
    assert!(a.unique, "for_agent unique");
    assert!(a.require_change, "for_agent require_change");
    assert!(a.fuzzy, "for_agent fuzzy");
    assert_eq!(a.min_fuzzy_score, Some(AGENT_MIN_FUZZY_SCORE));
    assert!((AGENT_MIN_FUZZY_SCORE - 0.90).abs() < f64::EPSILON);
    assert!(
        !a.allow_absent_old,
        "for_agent must keep allow_absent_old false (#1758 / #1965)"
    );
    assert!(
        a.refuse_suspicious_fuzzy,
        "for_agent must auto-refuse over-wide fuzzy (#2005)"
    );
    // Distinct from Default so hosts do not hand-roll half the fields.
    assert!(!d.unique && !d.require_change && !d.fuzzy);
    assert_eq!(d.min_fuzzy_score, None);
    assert!(!d.allow_absent_old);
    assert!(!d.refuse_suspicious_fuzzy);
    // Other fields stay at Default.
    assert!(!a.regex && !a.word_boundary && !a.command_position && !a.if_exists);
    assert!(a.nth.is_none());
    assert!(a.insert_before.is_none() && a.insert_after.is_none());
    assert!(a.post_write.is_none());
}

#[test]
fn replace_options_for_agent_struct_update_overrides() {
    use crate::api::ReplaceOptions;
    let replace_all = ReplaceOptions {
        unique: false,
        ..ReplaceOptions::for_agent()
    };
    assert!(!replace_all.unique);
    assert!(replace_all.require_change && replace_all.fuzzy);
    assert!(!replace_all.allow_absent_old);

    let recovery = ReplaceOptions {
        allow_absent_old: true,
        ..ReplaceOptions::for_agent()
    };
    assert!(recovery.allow_absent_old);
    assert!(recovery.unique && recovery.require_change);

    let word = ReplaceOptions {
        fuzzy: false,
        min_fuzzy_score: None,
        word_boundary: true,
        ..ReplaceOptions::for_agent()
    };
    assert!(!word.fuzzy);
    assert!(word.word_boundary);
    assert_eq!(word.min_fuzzy_score, None);
    assert!(word.unique && word.require_change);
}

#[test]
fn replace_options_for_agent_zero_match_is_no_match() {
    use crate::api::{ReplaceOptions, replace_in_content};
    use crate::fallback::{EditErrorKind, edit_error_kind};
    let err = replace_in_content("hello world", "missing", "x", &ReplaceOptions::for_agent())
        .unwrap_err();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::NoMatch),
        "for_agent require_change: {err}"
    );
}

#[test]
fn replace_options_for_agent_unique_multi_is_ambiguous() {
    use crate::api::{ReplaceOptions, replace_in_content};
    use crate::fallback::{EditErrorKind, edit_error_kind, is_ambiguous};
    let err =
        replace_in_content("foo bar foo", "foo", "baz", &ReplaceOptions::for_agent()).unwrap_err();
    assert_eq!(edit_error_kind(&err), Some(EditErrorKind::AmbiguousTarget));
    assert!(is_ambiguous(&err), "{err}");
}

#[test]
fn replace_options_for_agent_absent_old_fails_closed() {
    // #1758 via for_agent: exact old absent must not rewrite the live identifier.
    use crate::api::{ReplaceOptions, replace_in_content};
    let content = "def compute_checksum(payload: bytes) -> str:\n    return payload.hex()\n";
    let err = replace_in_content(
        content,
        "compute_cheksum",
        "compute_digest",
        &ReplaceOptions::for_agent(),
    )
    .expect_err("for_agent must refuse fuzzy rewrite without allow_absent_old");
    let msg = err.to_string();
    assert!(
        msg.contains("exact old absent") && msg.contains("compute_checksum"),
        "error must name candidate: {msg}"
    );
    let recovery = ReplaceOptions {
        allow_absent_old: true,
        ..ReplaceOptions::for_agent()
    };
    let r = replace_in_content(content, "compute_cheksum", "compute_digest", &recovery)
        .expect("opt-in recovery must apply fuzzy candidate");
    assert!(r.changed);
    assert!(r.new_content.contains("compute_digest"));
    assert!(!r.new_content.contains("compute_checksum"));
}

/// #1981: host refuse helper for over-wide fuzzy spans.
#[test]
fn fuzzy_span_suspicious_default_policy() {
    use crate::api::{
        AGENT_MIN_FUZZY_SCORE, FuzzySpanPolicy, fuzzy_span_suspicious,
        fuzzy_span_suspicious_with_policy,
    };

    // Token-scale match is fine.
    assert!(!fuzzy_span_suspicious(
        "process_data",
        Some("process_data"),
        Some(0.99),
    ));

    // Whole function body vs short identifier is suspicious under ratio cap.
    let wide = "fn process_data() {\n    // lots of body\n    do_work();\n    more();\n}\n";
    assert!(
        wide.chars().count() > 4 * "process_data".chars().count(),
        "fixture must exceed 4x"
    );
    assert!(fuzzy_span_suspicious(
        "process_data",
        Some(wide),
        Some(AGENT_MIN_FUZZY_SCORE),
    ));

    // Near-floor score + expansion strictly above 2x is also suspicious.
    let over_double = "abcdefghijabcdefghijk"; // 21 chars
    let old = "abcdefghij"; // 10 → ratio 2.1
    assert!(fuzzy_span_suspicious(old, Some(over_double), Some(0.92),));
    // Same span with high score is not in the near-floor band; 2.1x < 4x and
    // < old+40, so not suspicious under the wide-span cap alone.
    assert!(!fuzzy_span_suspicious(old, Some(over_double), Some(0.99)));

    // No matched_text → not suspicious.
    assert!(!fuzzy_span_suspicious("x", None, Some(0.9)));
    assert!(!fuzzy_span_suspicious("x", Some(""), Some(0.9)));

    // Empty old + non-empty match → suspicious.
    assert!(fuzzy_span_suspicious("", Some("anything"), None));

    // Custom policy can loosen the ratio.
    let loose = FuzzySpanPolicy {
        max_ratio: 100.0,
        abs_extra_chars: 10_000,
        near_floor_score_lo: 1.0,
        near_floor_score_hi: 1.0,
        near_floor_ratio: 100.0,
        ..FuzzySpanPolicy::default()
    };
    assert!(!fuzzy_span_suspicious_with_policy(
        "process_data",
        Some(wide),
        Some(0.91),
        &loose,
    ));

    // Boundaries: exclusive inequalities on the wide cap and near-floor band.
    // max_allowed = max(4*old, old+40). For short old, abs cap dominates.
    let old10 = "abcdefghij"; // 10 → ratio_cap=40, abs_cap=50 → max=50
    let exact_abs: String = "a".repeat(50);
    assert!(!fuzzy_span_suspicious(old10, Some(&exact_abs), None));
    let over_abs: String = "a".repeat(51);
    assert!(fuzzy_span_suspicious(old10, Some(&over_abs), None));
    // Just over 4x (41) is still under abs_cap=50 for old=10, so not suspicious.
    let just_over_4x_short: String = "a".repeat(41);
    assert!(!fuzzy_span_suspicious(
        old10,
        Some(&just_over_4x_short),
        None
    ));
    // For old large enough that 4x > old+40 (old=50 → ratio_cap=200, abs=90):
    let old50: String = "b".repeat(50);
    let exact_4x: String = "a".repeat(200);
    assert!(!fuzzy_span_suspicious(&old50, Some(&exact_4x), None));
    let just_over_4x: String = "a".repeat(201);
    assert!(fuzzy_span_suspicious(&old50, Some(&just_over_4x), None));
    // score == 0.95 is outside near-floor band; 2.1x alone not enough without score band.
    assert!(!fuzzy_span_suspicious(old10, Some(over_double), Some(0.95)));
    // score just below near_floor_score_lo is outside the band (inclusive lower bound).
    assert!(!fuzzy_span_suspicious(
        old10,
        Some(over_double),
        Some(AGENT_MIN_FUZZY_SCORE - 0.001)
    ));
    // ratio == 2.0 exactly in near-floor band is not suspicious (needs > 2).
    let exact_2x: String = "a".repeat(20);
    assert!(!fuzzy_span_suspicious(old10, Some(&exact_2x), Some(0.92)));
    // No score: only wide-cap path (2.1x is not wide enough).
    assert!(!fuzzy_span_suspicious(old10, Some(over_double), None));
    // Unicode: char count, not bytes. café = 4 chars / 5 bytes → abs_cap chars=44.
    // 45 ASCII matched: chars refuse (45>44), bytes would allow (45==45).
    assert!(!fuzzy_span_suspicious("café", Some("café"), Some(0.99)));
    assert!(fuzzy_span_suspicious("café", Some(&"x".repeat(45)), None));
    // NaN score: comparisons fail closed (near-floor band never fires).
    assert!(!fuzzy_span_suspicious(
        old10,
        Some(over_double),
        Some(f64::NAN)
    ));
}

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
    assert_eq!(
        result.new_content, "use std::io;\nuse std::fs;\n\nfn main() {}\n",
        "insert_after must be exact (no extra blank)"
    );
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
    assert_eq!(
        result.new_content, "use std::fs;\nuse std::io;\n\nfn main() {}\n",
        "insert_before must be exact (no extra blank)"
    );
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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

/// Floor reject must honor if_exists (soft miss), same as resolve failure (#1750).
#[test]
fn replace_in_content_min_fuzzy_floor_honors_if_exists() {
    let content = "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n";
    let misspelled = "fn process_requets(data: &str) -> Result<()> {";
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(1.0),
        allow_absent_old: true,
        if_exists: true,
        ..Default::default()
    };
    let result = replace::replace_in_content(content, misspelled, "REPLACED", &opts)
        .expect("if_exists must soften min_fuzzy_score floor reject");
    assert!(!result.changed);
    assert_eq!(result.match_count, 0);
    assert!(result.match_mode.is_none());
}

/// Disk path: floor reject + if_exists must not error (#1750).
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_min_fuzzy_floor_honors_if_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("src.rs");
    fs::write(
        &file,
        "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n",
    )
    .unwrap();
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(1.0),
        allow_absent_old: true,
        if_exists: true,
        ..Default::default()
    };
    let result = replace_text(
        &file,
        "fn process_requets(data: &str) -> Result<()> {",
        "REPLACED",
        &opts,
        ApplyMode::Preview,
        None,
    )
    .expect("if_exists must soften floor reject on disk path");
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
fn replace_in_content_fuzzy_insert_after_line_orients_comment() {
    // Fuzzy match + comment-like insert_after must still line-orient (#1885).
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: None,
        allow_absent_old: true,
        insert_after: Some("    // after body".to_string()),
        ..Default::default()
    };
    let result = replace::replace_in_content(content, "fn proccess_data() {}", "", &opts).unwrap();
    assert!(result.changed);
    assert!(
        result
            .new_content
            .contains("fn process_data() {}\n    // after body"),
        "fuzzy insert_after must insert on next line: {}",
        result.new_content
    );
    assert!(
        !result
            .new_content
            .contains("fn process_data() {}    // after body"),
        "must not glue comment onto function: {}",
        result.new_content
    );
}

#[test]
fn replace_in_content_fuzzy_with_insert_after() {
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "alpha\nTODO: fix\nbeta\ngamma\nDONE\ndelta\n",
        "before_context must replace only the TODO after gamma"
    );
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
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "alpha\nDONE\nbeta\ngamma\nTODO: fix\ndelta\n",
        "after_context must replace only the TODO before beta"
    );
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
    assert_eq!(
        result.new_content,
        "[database]\nhost = db.primary\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        "before_context must change only the database host"
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
    assert_eq!(
        result.new_content,
        "[database]\nhost = db.primary\nport = 5432\n\n[cache]\nhost = localhost\nport = 6379\n",
        "after_context must change only the database host"
    );
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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

/// #2008: span policy on file helper; Exact apply still writes + backup session.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_content_edits_to_file_span_policy_allows_exact() {
    use crate::api::{FuzzySpanPolicy, apply_content_edits_to_file_with_span_policy};
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "hello world\n").unwrap();
    let before = fs::read_to_string(&file).unwrap();
    let edits = [
        ContentEdit::Replace {
            old: "hello".into(),
            new: "hi".into(),
            options: ReplaceOptions::default(),
        },
        ContentEdit::Append {
            content: "tail\n".into(),
        },
    ];
    let policy = FuzzySpanPolicy::default();
    let r = apply_content_edits_to_file_with_span_policy(
        &file,
        &edits,
        ApplyMode::Apply,
        None,
        Some(&policy),
    )
    .expect("exact multi-op must apply with span policy");
    assert!(r.applied && r.changed);
    assert!(
        r.backup_session.is_some(),
        "successful Apply must still create backup_session"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "hi world\ntail\n");
    assert_ne!(fs::read_to_string(&file).unwrap(), before);
}

/// #2008: strict span policy refuses real fuzzy multi-op; file bytes unchanged.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_content_edits_to_file_span_policy_refuses_fuzzy_before_write() {
    use crate::api::{
        FuzzySpanPolicy, apply_content_edits_to_file_with_span_policy, is_fuzzy_span_suspicious,
    };
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("wide.txt");
    let original = "fn process_data() {}\nkeep exact\n";
    fs::write(&file, original).unwrap();
    // Multi-op: Exact first, then fuzzy typo. Strict policy refuses any fuzzy
    // expansion (near_floor_ratio 0) so we lock pre-write refuse without a
    // brittle engine-wide span fixture.
    let edits = [
        ContentEdit::Replace {
            old: "keep exact".into(),
            new: "KEEP EXACT".into(),
            options: ReplaceOptions::default(),
        },
        ContentEdit::Replace {
            old: "fn proccess_data() {}".into(),
            new: "fn handle_data() {}".into(),
            options: ReplaceOptions {
                fuzzy: true,
                min_fuzzy_score: None,
                allow_absent_old: true,
                require_change: true,
                refuse_suspicious_fuzzy: false,
                ..Default::default()
            },
        },
    ];
    let strict = FuzzySpanPolicy {
        max_ratio: 1.0,
        abs_extra_chars: 0,
        near_floor_score_lo: 0.0,
        near_floor_score_hi: 1.01,
        near_floor_ratio: 0.0,
    };
    let err = apply_content_edits_to_file_with_span_policy(
        &file,
        &edits,
        ApplyMode::Apply,
        None,
        Some(&strict),
    )
    .expect_err("strict policy must refuse fuzzy multi-op before write");
    assert!(is_fuzzy_span_suspicious(&err), "got: {err}");
    assert_eq!(
        crate::api::error_kind_str(&err),
        Some("fuzzy_span_suspicious")
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "refuse must leave file bytes unchanged"
    );
    // Without policy the same multi-op must write (control).
    let ok = apply_content_edits_to_file(&file, &edits, ApplyMode::Apply, None)
        .expect("without span policy fuzzy multi-op applies");
    assert!(ok.applied && ok.changed);
    assert_ne!(fs::read_to_string(&file).unwrap(), original);
}

/// #2008 / #2064: refuse_batch_if_suspicious_fuzzy blocks over-wide fuzzy
/// honesty (deterministic; public crate + api paths).
#[test]
fn refuse_batch_if_suspicious_fuzzy_rejects_wide_honesty() {
    use crate::api::{
        ContentEditHonesty, ContentEditsResult, FuzzySpanPolicy, is_fuzzy_span_suspicious,
        refuse_batch_if_suspicious_fuzzy,
    };

    let batch = ContentEditsResult {
        original: "x".into(),
        modified: "y".into(),
        diff: String::new(),
        changed: true,
        ops_applied: 1,
        match_count: 1,
        match_mode: Some(MatchMode::Fuzzy),
        match_score: Some(AGENT_MIN_FUZZY_SCORE),
        matched_text: Some("process_data_and_much_more_tail".into()),
        op_honesty: vec![ContentEditHonesty::fuzzy(
            0,
            "process_data",
            AGENT_MIN_FUZZY_SCORE,
            "process_data_and_much_more_tail",
        )],
    };
    // Crate-root re-export (host `use patchloom::refuse_batch_if_suspicious_fuzzy`).
    let err = crate::refuse_batch_if_suspicious_fuzzy(&batch, &FuzzySpanPolicy::default())
        .expect_err("wide fuzzy honesty must refuse before write");
    assert!(is_fuzzy_span_suspicious(&err));
    assert_eq!(
        crate::api::error_kind_str(&err),
        Some("fuzzy_span_suspicious")
    );
    assert!(
        err.to_string().contains("content edit 1"),
        "message should name 1-based op index: {err}"
    );

    // Unchanged batch must not refuse.
    let soft = ContentEditsResult {
        changed: false,
        modified: batch.original.clone(),
        ..batch.clone()
    };
    refuse_batch_if_suspicious_fuzzy(&soft, &FuzzySpanPolicy::default())
        .expect("unchanged batch must not refuse");

    // Exact honesty is not refused even when matched_text is long (#2064 Fuzzy-only).
    let exact_wide = ContentEditsResult {
        changed: true,
        match_mode: Some(MatchMode::Exact),
        match_score: None,
        matched_text: Some("process_data_and_much_more_tail".into()),
        op_honesty: vec![ContentEditHonesty::exact(
            0,
            "process_data",
            "process_data_and_much_more_tail",
        )],
        ..batch.clone()
    };
    refuse_batch_if_suspicious_fuzzy(&exact_wide, &FuzzySpanPolicy::default())
        .expect("exact match_mode must skip span refuse");

    // Safe token-scale fuzzy passes.
    let safe = ContentEditsResult {
        op_honesty: vec![ContentEditHonesty::fuzzy(
            0,
            "process_data",
            0.99,
            "process_data",
        )],
        matched_text: Some("process_data".into()),
        match_score: Some(0.99),
        ..batch
    };
    refuse_batch_if_suspicious_fuzzy(&safe, &FuzzySpanPolicy::default())
        .expect("token-scale fuzzy must pass default policy");
}

/// #2064: live buffer multi-op + public batch refuse (host EditEngine shape).
#[test]
fn refuse_batch_if_suspicious_fuzzy_after_live_content_edits() {
    use crate::api::{
        ContentEdit, FuzzySpanPolicy, apply_content_edits, is_fuzzy_span_suspicious,
        refuse_batch_if_suspicious_fuzzy,
    };
    let original = "fn process_data() {}\nkeep exact\n";
    let edits = [
        ContentEdit::Replace {
            old: "keep exact".into(),
            new: "KEEP EXACT".into(),
            options: ReplaceOptions::default(),
        },
        ContentEdit::Replace {
            old: "fn proccess_data() {}".into(),
            new: "fn handle_data() {}".into(),
            options: ReplaceOptions {
                fuzzy: true,
                min_fuzzy_score: None,
                allow_absent_old: true,
                require_change: true,
                refuse_suspicious_fuzzy: false,
                ..Default::default()
            },
        },
    ];
    let batch = apply_content_edits(original, &edits).expect("buffer multi-op applies");
    assert!(batch.changed);
    assert!(
        batch
            .op_honesty
            .iter()
            .any(|h| h.match_mode == Some(MatchMode::Fuzzy)),
        "expected a fuzzy honesty row: {:?}",
        batch.op_honesty
    );
    let strict = FuzzySpanPolicy {
        max_ratio: 1.0,
        abs_extra_chars: 0,
        near_floor_score_lo: 0.0,
        near_floor_score_hi: 1.01,
        near_floor_ratio: 0.0,
    };
    let err = refuse_batch_if_suspicious_fuzzy(&batch, &strict)
        .expect_err("strict policy must refuse fuzzy multi-op batch");
    assert!(is_fuzzy_span_suspicious(&err), "got: {err}");
    assert_eq!(
        crate::api::error_kind_str(&err),
        Some("fuzzy_span_suspicious")
    );
    // Host still holds original; no write occurred.
    assert_eq!(batch.original, original);
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
    assert!(
        crate::fallback::is_ambiguous(&err),
        "public is_ambiguous peel for unique multi-match: {err}"
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
                min_fuzzy_score: None,
                allow_absent_old: true,
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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

// ── #1658–#1666 LLM agent embedder API batch ──────────────────────────────

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
            min_fuzzy_score: None,
            allow_absent_old: true,
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
    assert_eq!(
        r.matched_text.as_deref(),
        Some("TODO: fix"),
        "anchored multi-match path must report matched_text (#1736 parity)"
    );
    assert_eq!(
        r.new_content, "alpha\nTODO: fix\nbeta\nTODO: done\n",
        "anchored replace must keep the first TODO and not add blanks"
    );
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
        min_fuzzy_score: None,
        allow_absent_old: true,
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
    // #1736: library hosts on the disk path need matched_text, not only mode/score.
    let matched = result
        .matched_text
        .as_deref()
        .expect("disk pure fuzzy must report matched_text");
    assert!(
        matched.contains("process_data"),
        "matched_text should be the live span, got {matched:?}"
    );
    assert_ne!(matched, "fn proccess_data() {}");
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

    let zero = find_files_with_symbol(
        dir.path(),
        "OldType",
        &SymbolSearchOptions {
            max_files: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        zero.is_empty(),
        "max_files=0 must not return hits (got {zero:?})"
    );

    let capped = find_files_with_symbol(
        dir.path(),
        "OldType",
        &SymbolSearchOptions {
            max_files: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(capped.len(), 1, "max_files=1: {capped:?}");

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
    assert!(
        results.iter().all(|r| r.result.is_ok()),
        "batch rename failed: {:?}",
        results
            .iter()
            .map(|r| match &r.result {
                Ok(e) => format!("{}:ok({})", r.path.display(), e.match_count),
                Err(e) => format!("{}:{:?}", r.path.display(), e.kind),
            })
            .collect::<Vec<_>>()
    );
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

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_exact_disk_match_mode_and_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("t.txt");
    fs::write(&file, "hello world\n").unwrap();
    let r = replace_text(
        &file,
        "world",
        "there",
        &ReplaceOptions::default(),
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(r.changed, "content should change");
    assert_eq!(
        r.match_count, 1,
        "exact disk replace must report match_count"
    );
    assert_eq!(
        r.match_mode,
        Some(MatchMode::Exact),
        "exact disk replace must report match_mode Exact, got {:?}",
        r.match_mode
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_exact_disk_multi_match_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("t.txt");
    fs::write(&file, "aa aa aa\n").unwrap();
    let r = replace_text(
        &file,
        "aa",
        "bb",
        &ReplaceOptions::default(),
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(r.changed);
    assert_eq!(r.match_count, 3, "multi exact match_count: {:?}", r);
    assert_eq!(r.match_mode, Some(MatchMode::Exact));
}

// ---------------------------------------------------------------------------

/// Engine parent-cwd must not collapse multi-component caller paths to basename.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn edit_result_path_preserves_caller_relative() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("src");
    fs::create_dir(&nested).unwrap();
    let file = nested.join("lib.rs");
    fs::write(&file, "fn old() {}\n").unwrap();
    // Absolute multi-component path (do not chdir: process-global, races tests).
    let r = replace_text(
        &file,
        "old",
        "new",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(
        r.path.contains("src") && r.path.contains("lib.rs"),
        "EditResult.path must keep multi-component path, not basename only; got {:?}",
        r.path
    );
    // Without display_path override this collapsed to "lib.rs" via parent cwd.
    assert_ne!(r.path, "lib.rs");
}

// Agent-host APIs #1686–#1690
// ---------------------------------------------------------------------------

/// #1686: Apply replace reports backup_session; restore_path_from_session works.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn edit_result_backup_session_enables_surgical_restore() {
    use crate::backup::restore_path_from_session;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("note.txt");
    fs::write(&file, "hello world\n").unwrap();

    let r = replace_text(
        &file,
        "world",
        "patchloom",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(r.applied);
    assert!(r.changed);
    let session = r
        .backup_session
        .as_deref()
        .expect("Apply must report backup_session (#1686)");
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello patchloom\n");

    // Preview must not create a session.
    let preview = replace_text(
        &file,
        "patchloom",
        "again",
        &ReplaceOptions::default(),
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(preview.backup_session.is_none());

    let cwd = file.parent().unwrap();
    assert!(
        restore_path_from_session(cwd, session, &file).unwrap(),
        "restore via reported session"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello world\n");
}

/// #1687: min_fuzzy_score rejects low-confidence fuzzy; exact still wins.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn min_fuzzy_score_rejects_weak_fuzzy_allows_exact() {
    let content = "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n";
    let misspelled = "fn process_requets(data: &str) -> Result<()> {";

    // First measure the natural fuzzy score, then set a floor just above it.
    let probe = replace_in_content(
        content,
        misspelled,
        "REPLACED",
        &ReplaceOptions {
            fuzzy: true,
            allow_absent_old: true,
            ..Default::default()
        },
    )
    .expect("unfloored fuzzy should match");
    assert_eq!(probe.match_mode, Some(MatchMode::Fuzzy));
    let natural = probe.match_score.expect("fuzzy score");
    assert!(
        natural > 0.85,
        "similarity path requires >0.85, got {natural}"
    );
    // Strict floor just above natural, clamped to the valid 0.0..=1.0 range.
    // When natural is already 1.0, any valid floor allows equality (score < min is false).
    let strict_floor = (natural + 1e-4).clamp(0.0, 1.0);
    if (strict_floor - natural).abs() < 1e-12 {
        // Natural is effectively 1.0; only out-of-range floors would reject, which is InvalidInput.
        // Covered by min_fuzzy_score_rejects_out_of_range.
    } else {
        let strict = ReplaceOptions {
            fuzzy: true,
            min_fuzzy_score: Some(strict_floor),
            allow_absent_old: true,
            ..Default::default()
        };
        let err = replace_in_content(content, misspelled, "REPLACED", &strict).unwrap_err();
        assert_eq!(
            edit_error_kind(&err),
            Some(EditErrorKind::NoMatch),
            "weak fuzzy must be NoMatch: {err}"
        );
        assert!(
            err.to_string().contains("min_fuzzy_score"),
            "message should name the floor: {err}"
        );
    }

    // Floor at or below natural score allows the same fuzzy match.
    let loose = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(natural),
        allow_absent_old: true,
        ..Default::default()
    };
    let ok = replace_in_content(content, misspelled, "REPLACED {", &loose).unwrap();
    assert!(ok.changed);
    assert_eq!(ok.match_mode, Some(MatchMode::Fuzzy));
    assert!(ok.match_score.is_some_and(|s| s >= natural - 1e-9));

    // Exact match is unaffected by a high floor.
    let exact_opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(1.0),
        allow_absent_old: true,
        ..Default::default()
    };
    let exact = replace_in_content("alpha beta gamma\n", "beta", "BETA", &exact_opts).unwrap();
    assert!(exact.changed);
    assert_eq!(exact.match_mode, Some(MatchMode::Exact));
    assert!(exact.new_content.contains("BETA"));
}

/// #1688: nested monorepo backups found by list_sessions_under.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn list_sessions_under_finds_nested_backup_roots() {
    use crate::backup::{ListSessionsOptions, list_sessions, list_sessions_under};

    let workspace = TempDir::new().unwrap();
    let crate_dir = workspace.path().join("crates").join("foo");
    fs::create_dir_all(&crate_dir).unwrap();
    let file = crate_dir.join("lib.txt");
    fs::write(&file, "old\n").unwrap();

    let r = replace_text(
        &file,
        "old",
        "new",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(
        r.backup_session.as_deref().is_some_and(|s| !s.is_empty()),
        "apply must expose non-empty backup_session: {:?}",
        r.backup_session
    );

    // Workspace-root list is empty (session lives under file parent).
    let root_sessions = list_sessions(workspace.path()).unwrap();
    assert!(
        root_sessions.is_empty(),
        "workspace list_sessions should miss nested root: {root_sessions:?}"
    );

    let listings = list_sessions_under(
        workspace.path(),
        &ListSessionsOptions {
            descendants: true,
            max_depth: Some(8),
            ancestors: false,
        },
    )
    .unwrap();
    assert!(
        !listings.is_empty(),
        "list_sessions_under must find nested sessions"
    );
    let found = listings.iter().any(|l| {
        l.sessions
            .iter()
            .any(|s| Some(&s.timestamp) == r.backup_session.as_ref())
    });
    assert!(
        found,
        "reported session must appear in nested listing: {listings:?}"
    );
}

/// #1689: ast_rename_project discovers + renames without a path list.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_project_discovers_and_renames() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    let empty = dir.path().join("empty.rs");
    fs::write(&a, "struct OldType {}\n").unwrap();
    fs::write(&b, "fn use_it(x: OldType) {}\n").unwrap();
    fs::write(&empty, "fn other() {}\n").unwrap();

    let report = ast_rename_project(
        dir.path(),
        "OldType",
        "NewType",
        &AstRenameProjectOptions {
            search: SymbolSearchOptions::default(),
            rename: AstRenameBatchOptions {
                mode: ApplyMode::Apply,
                ..Default::default()
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(
        report.paths_considered.len(),
        2,
        "{:?}",
        report.paths_considered
    );
    assert!(
        report.results.iter().all(|r| r.result.is_ok()),
        "rename results: {:?}",
        report.results
    );
    assert!(fs::read_to_string(&a).unwrap().contains("NewType"));
    assert!(fs::read_to_string(&b).unwrap().contains("NewType"));
    assert!(!fs::read_to_string(&empty).unwrap().contains("NewType"));

    let err = ast_rename_project(
        dir.path(),
        "DoesNotExistAnywhere",
        "X",
        &AstRenameProjectOptions::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(edit_error_kind(&err), Some(EditErrorKind::NoMatch));
}

/// #1690: post_write hooks on replace Apply; Preview skips hooks; fail reverts.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn post_write_hooks_on_replace_apply_and_preview() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("x.txt");
    fs::write(&file, "before\n").unwrap();

    // Preview must not run hooks (failing command would error).
    let preview_opts = ReplaceOptions {
        post_write: Some(PostWriteHooks {
            format_cmd: Some("false".into()),
            on_failure: PostWriteOnFailure::KeepWithError,
            ..Default::default()
        }),
        ..Default::default()
    };
    let preview = replace_text(
        &file,
        "before",
        "after",
        &preview_opts,
        ApplyMode::Preview,
        None,
    )
    .unwrap();
    assert!(!preview.applied);
    assert_eq!(fs::read_to_string(&file).unwrap(), "before\n");

    // Apply + success hook.
    let ok_opts = ReplaceOptions {
        post_write: Some(PostWriteHooks {
            format_cmd: Some("true".into()),
            on_failure: PostWriteOnFailure::KeepWithError,
            ..Default::default()
        }),
        post_write_cwd: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let ok = replace_text(&file, "before", "after", &ok_opts, ApplyMode::Apply, None).unwrap();
    assert!(ok.applied);
    assert!(
        ok.backup_session.as_deref().is_some_and(|s| !s.is_empty()),
        "apply must expose non-empty backup_session: {:?}",
        ok.backup_session
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");

    // Apply + failing hook with Revert restores prior content.
    fs::write(&file, "v1\n").unwrap();
    let fail_opts = ReplaceOptions {
        post_write: Some(PostWriteHooks {
            format_cmd: Some("false".into()),
            on_failure: PostWriteOnFailure::Revert,
            ..Default::default()
        }),
        post_write_cwd: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let err = replace_text(&file, "v1", "v2", &fail_opts, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::FormatFailed),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "v1\n",
        "Revert must restore pre-edit content"
    );
}

/// #1690 regression: hooks cwd may be workspace root while backup lives under
/// the file parent; Revert must still restore.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn post_write_revert_uses_file_parent_backup_root() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("pkg");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("x.txt");
    fs::write(&file, "v1\n").unwrap();

    // Hooks run at workspace root; backup session is under nested/.
    let fail_opts = ReplaceOptions {
        post_write: Some(PostWriteHooks {
            format_cmd: Some("false".into()),
            on_failure: PostWriteOnFailure::Revert,
            ..Default::default()
        }),
        post_write_cwd: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let err = replace_text(&file, "v1", "v2", &fail_opts, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::FormatFailed),
        "{err}"
    );
    assert!(
        !err.to_string().contains("also failed to revert"),
        "session restore must find backup under file parent: {err}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "v1\n",
        "content restored despite hooks cwd != backup root"
    );
}

/// #1694: fuzzy identifier typo keeps surrounding syntax (not whole-line replace).
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn fuzzy_identifier_typo_keeps_line_syntax() {
    let content = "const CONFIGURATION_VALUE_PRIMARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n";
    let r = replace_in_content(
        content,
        "CONFIGURATION_VALUE_PRIMRY",
        "CONFIGURATION_VALUE_SECONDARY",
        &ReplaceOptions {
            fuzzy: true,
            allow_absent_old: true,
            ..Default::default()
        },
    )
    .expect("fuzzy typo should apply");
    assert!(r.changed);
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    assert_eq!(
        r.new_content,
        "const CONFIGURATION_VALUE_SECONDARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n",
        "fuzzy typo must keep the rest of the line and the second site"
    );
}

/// Embedder default path: fuzzy + unique + require_change (Bline edit_file shape).
#[test]
fn fuzzy_embedder_options_identifier_typo_safe() {
    let content = "const CONFIGURATION_VALUE_PRIMARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        allow_absent_old: true,
        unique: true,
        require_change: true,
        min_fuzzy_score: Some(0.80),
        ..Default::default()
    };
    let r = replace_in_content(
        content,
        "CONFIGURATION_VALUE_PRIMRY",
        "CONFIGURATION_VALUE_SECONDARY",
        &opts,
    )
    .expect("embedder-style fuzzy typo must succeed");
    assert!(r.changed);
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    assert!(r.match_score.is_some_and(|s| s >= 0.80));
    assert_eq!(
        r.new_content,
        "const CONFIGURATION_VALUE_SECONDARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n",
        "embedder unique fuzzy must keep syntax and the second site"
    );
}

/// Multi-language identifier typos under replace_in_content.
#[test]
fn fuzzy_identifier_typo_matrix_replace_in_content() {
    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "def load_configuration_value():\n    return 1\n",
            "load_configration_value",
            "load_settings_value",
            "def load_settings_value():",
        ),
        (
            "const getUserProfile = () => null;\n",
            "getUserProfle",
            "getUserAccount",
            "const getUserAccount = () => null;",
        ),
        (
            "fn process_request(x: i32) {}\n",
            "process_requets",
            "handle_request",
            "fn handle_request(x: i32) {}",
        ),
        (
            "    obj.fetchUserDetails(id);\n",
            "fetchUserDetials",
            "fetchUserInfo",
            "    obj.fetchUserInfo(id);",
        ),
    ];
    for (content, typo, new, must_contain) in cases {
        let r = replace_in_content(
            content,
            typo,
            new,
            &ReplaceOptions {
                fuzzy: true,
                allow_absent_old: true,
                require_change: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("typo {typo:?} failed: {e}"));
        assert!(
            r.new_content.contains(must_contain),
            "typo={typo:?} want {must_contain:?} got {}",
            r.new_content
        );
        assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    }
}

/// unique must still fire on exact multi-match even when fuzzy is enabled.
#[test]
fn fuzzy_does_not_bypass_unique_on_exact_multi_match() {
    let content = "foo bar foo\n";
    let err = replace_in_content(
        content,
        "foo",
        "baz",
        &ReplaceOptions {
            fuzzy: true,
            allow_absent_old: true,
            unique: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::AmbiguousTarget),
        "exact multi-match must stay unique-ambiguous with fuzzy on: {err}"
    );
    assert!(
        err.to_string().contains("ambiguous"),
        "message should name ambiguity: {err}"
    );
}

/// Disk Apply path used by hosts (replace_text), same safety contract.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn fuzzy_identifier_typo_disk_apply_preserves_syntax() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("cfg.rs");
    fs::write(
        &file,
        "const CONFIGURATION_VALUE_PRIMARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n",
    )
    .unwrap();
    let r = replace_text(
        &file,
        "CONFIGURATION_VALUE_PRIMRY",
        "CONFIGURATION_VALUE_SECONDARY",
        &ReplaceOptions {
            fuzzy: true,
            allow_absent_old: true,
            unique: true,
            require_change: true,
            ..Default::default()
        },
        ApplyMode::Apply,
        None,
    )
    .unwrap();
    assert!(r.applied);
    assert!(
        r.backup_session.as_deref().is_some_and(|s| !s.is_empty()),
        "apply must expose non-empty backup_session: {:?}",
        r.backup_session
    );
    let on_disk = fs::read_to_string(&file).unwrap();
    assert_eq!(
        on_disk,
        "const CONFIGURATION_VALUE_SECONDARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n",
        "disk fuzzy apply must keep syntax and the second site: {on_disk}"
    );
}

/// #1687: out-of-range min_fuzzy_score is InvalidInput.
#[test]
fn min_fuzzy_score_rejects_out_of_range() {
    for bad in [f64::NAN, -0.1, 1.5, 2.0] {
        let opts = ReplaceOptions {
            fuzzy: true,
            min_fuzzy_score: Some(bad),
            allow_absent_old: true,
            ..Default::default()
        };
        let err = replace_in_content("hello world\n", "helo", "hi", &opts).unwrap_err();
        assert_eq!(
            edit_error_kind(&err),
            Some(EditErrorKind::InvalidInput),
            "bad={bad} err={err}"
        );
    }
}

/// #1690: tidy honors WritePolicyOptions.post_write.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn tidy_post_write_hooks_on_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    fs::write(&file, "line  \n").unwrap();

    let opts = WritePolicyOptions {
        trim_trailing_whitespace: true,
        post_write: Some(PostWriteHooks {
            format_cmd: Some("true".into()),
            ..Default::default()
        }),
        post_write_cwd: Some(dir.path().to_path_buf()),
        ..Default::default()
    };
    let r = tidy(&file, &opts, ApplyMode::Apply, None).unwrap();
    assert!(r.applied);
    assert!(r.changed);
    assert_eq!(fs::read_to_string(&file).unwrap(), "line\n");
}

/// #1736: fuzzy near-collision must expose matched_text so hosts can refuse.
#[test]
fn replace_in_content_fuzzy_near_collision_reports_matched_text() {
    let content = concat!(
        "def compute_checksum(payload: bytes) -> str:\n",
        "    return payload.hex()\n",
        "\n",
        "def compute_checksum_fast(payload: bytes) -> str:\n",
        "    return payload[:8].hex()\n",
    );
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(0.95),
        allow_absent_old: true,
        ..ReplaceOptions::default()
    };
    let r = replace_in_content(content, "compute_cheksum", "compute_digest", &opts)
        .expect("fuzzy should land on a near identifier");
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    assert!(
        r.match_score.is_some_and(|s| s >= 0.95),
        "{:?}",
        r.match_score
    );
    let matched = r
        .matched_text
        .as_deref()
        .expect("matched_text required for fuzzy");
    assert_ne!(
        matched, "compute_cheksum",
        "requested old is absent; matched_text must be the live span"
    );
    assert!(
        matched.contains("compute_checksum"),
        "expected live identifier in matched_text, got {matched:?}"
    );
    assert!(
        r.new_content.contains("compute_digest"),
        "replacement should apply to the matched span"
    );
    // #1981 host path: token-scale identifier fuzzy is not over-wide.
    assert!(
        !crate::api::fuzzy_span_suspicious("compute_cheksum", Some(matched), r.match_score),
        "near-collision identifier span must not trip default refuse policy: {matched:?}"
    );
}

/// #2005: for_agent auto-refuses over-wide fuzzy without a second host call.
#[test]
fn replace_in_content_for_agent_refuses_suspicious_fuzzy_span() {
    // Token-scale recovery must still succeed under for_agent refuse policy.
    let content = "fn process_data() {}\n";
    let agent = ReplaceOptions {
        allow_absent_old: true, // allow apply so refuse gate is reachable
        unique: false,
        ..ReplaceOptions::for_agent()
    };
    let r = replace_in_content(content, "proess_data", "process_items", &agent)
        .expect("token-scale fuzzy must not trip refuse");
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    assert!(r.changed);

    // Over-wide refuse is locked in maybe_refuse_suspicious_fuzzy_rejects_wide_span
    // (deterministic ContentEditResult; engine wide-span fixtures are brittle).

    // Explicit disable: struct update turns refuse off.
    let no_refuse = ReplaceOptions {
        refuse_suspicious_fuzzy: false,
        allow_absent_old: true,
        unique: false,
        ..ReplaceOptions::for_agent()
    };
    let r3 = replace_in_content(content, "proess_data", "process_items", &no_refuse)
        .expect("without refuse_suspicious_fuzzy still Ok");
    assert!(r3.changed);
}

/// #2005: refuse gate rejects a wide fuzzy ContentEditResult (deterministic).
#[test]
fn maybe_refuse_suspicious_fuzzy_rejects_wide_span() {
    use crate::api::ContentEditResult;
    use crate::api::replace::maybe_refuse_suspicious_fuzzy;
    let result = ContentEditResult {
        original: "x".into(),
        new_content: "y".into(),
        diff: String::new(),
        changed: true,
        match_count: 1,
        match_mode: Some(MatchMode::Fuzzy),
        match_score: Some(AGENT_MIN_FUZZY_SCORE),
        matched_text: Some("process_data_and_much_more_tail".into()),
    };
    let opts = ReplaceOptions {
        refuse_suspicious_fuzzy: true,
        ..Default::default()
    };
    let err = maybe_refuse_suspicious_fuzzy("process_data", result, &opts)
        .expect_err("wide near-floor fuzzy must refuse");
    assert!(crate::api::is_fuzzy_span_suspicious(&err));
    assert_eq!(
        crate::api::error_kind_str(&err),
        Some("fuzzy_span_suspicious")
    );

    let result_ok = ContentEditResult {
        original: "x".into(),
        new_content: "y".into(),
        diff: String::new(),
        changed: true,
        match_count: 1,
        match_mode: Some(MatchMode::Fuzzy),
        match_score: Some(0.99),
        matched_text: Some("process_data".into()),
    };
    maybe_refuse_suspicious_fuzzy("process_data", result_ok, &opts)
        .expect("token-scale high score must pass refuse gate");

    // Soft if_exists honesty (changed=false) must not become FuzzySpanSuspicious.
    let soft = ContentEditResult {
        original: "x".into(),
        new_content: "x".into(),
        diff: String::new(),
        changed: false,
        match_count: 0,
        match_mode: Some(MatchMode::Fuzzy),
        match_score: Some(AGENT_MIN_FUZZY_SCORE),
        matched_text: Some("process_data_and_much_more_tail".into()),
    };
    maybe_refuse_suspicious_fuzzy("process_data", soft, &opts)
        .expect("unchanged soft path must not refuse");
}

/// #2005: peel helpers for FuzzySpanSuspicious.
#[test]
fn fuzzy_span_suspicious_error_kind_peels() {
    use crate::fallback::{
        EditError, EditErrorKind, edit_error_kind, error_kind_str, is_fuzzy_span_suspicious,
    };
    let err: anyhow::Error = EditError::new(
        EditErrorKind::FuzzySpanSuspicious,
        "fuzzy span suspicious: old \"x\" matched \"yyyy\"",
    )
    .into();
    assert_eq!(
        edit_error_kind(&err),
        Some(EditErrorKind::FuzzySpanSuspicious)
    );
    assert!(is_fuzzy_span_suspicious(&err));
    assert_eq!(error_kind_str(&err), Some("fuzzy_span_suspicious"));
    assert_eq!(EditErrorKind::FuzzySpanSuspicious as u8, 16);
}

/// #2006: multi-op exposes per-op honesty with matching old.
#[test]
fn content_edits_op_honesty_per_replace() {
    use crate::api::{ContentEdit, apply_content_edits};
    let edits = [
        ContentEdit::Replace {
            old: "aaa".into(),
            new: "AAA".into(),
            options: ReplaceOptions::default(),
        },
        ContentEdit::Replace {
            old: "bbb".into(),
            new: "BBB".into(),
            options: ReplaceOptions {
                fuzzy: true,
                allow_absent_old: true,
                min_fuzzy_score: Some(0.5),
                ..Default::default()
            },
        },
    ];
    let r = apply_content_edits("aaa bbb\n", &edits).expect("batch apply");
    assert!(r.changed);
    assert_eq!(
        r.op_honesty.len(),
        2,
        "each replace reports honesty: {:?}",
        r.op_honesty
    );
    assert_eq!(r.op_honesty[0].op_index, 0);
    assert_eq!(r.op_honesty[0].old, "aaa");
    assert_eq!(r.op_honesty[0].match_mode, Some(MatchMode::Exact));
    assert_eq!(r.op_honesty[1].op_index, 1);
    assert_eq!(r.op_honesty[1].old, "bbb");
    // Host can refuse with matching old without rollup guesswork.
    for h in &r.op_honesty {
        let _ = crate::api::fuzzy_span_suspicious(&h.old, h.matched_text.as_deref(), h.match_score);
    }
}

/// #1981: host refuse helper after a real fuzzy `replace_in_content` Apply.
#[test]
fn replace_in_content_fuzzy_host_refuses_over_wide_matched_text() {
    // Live file has a short identifier; fuzzy old is a typo of a long line
    // that is not present. With allow_absent_old the engine may still land
    // on the short identifier (token scale) — that is not over-wide.
    // The over-wide case is asserted with the public helper on a synthetic
    // function-body span (same shape hosts see after a bad fuzzy).
    let old = "process_data";
    let wide = "fn process_data() {\n    // lots of body\n    do_work();\n    more();\n}\n";
    assert!(
        crate::api::fuzzy_span_suspicious(old, Some(wide), Some(crate::api::AGENT_MIN_FUZZY_SCORE)),
        "function-body span vs short identifier must be suspicious under default policy"
    );
    // Real engine path: identifier typo (not a substring of the live name)
    // stays token-scale and is trusted under the default refuse policy.
    let content = "fn process_data() {}\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        allow_absent_old: true,
        min_fuzzy_score: Some(0.90),
        require_change: true,
        ..Default::default()
    };
    let r = replace_in_content(content, "proess_data", "process_items", &opts)
        .expect("fuzzy typo must apply");
    assert_eq!(r.match_mode, Some(MatchMode::Fuzzy));
    let matched = r.matched_text.as_deref().expect("fuzzy matched_text");
    assert!(
        !crate::api::fuzzy_span_suspicious("proess_data", Some(matched), r.match_score),
        "token-scale fuzzy Apply must not look over-wide: {matched:?}"
    );
}

/// #1758: fuzzy must not rewrite a live identifier when exact old is absent.
#[test]
fn fuzzy_absent_old_fails_closed_by_default() {
    let content = "def compute_checksum(payload: bytes) -> str:\n    return payload.hex()\n";
    let opts = ReplaceOptions {
        fuzzy: true,
        min_fuzzy_score: Some(0.95),
        allow_absent_old: false,
        require_change: true,
        ..Default::default()
    };
    let err = replace_in_content(content, "compute_cheksum", "compute_digest", &opts)
        .expect_err("must refuse fuzzy rewrite of live identifier without opt-in");
    let msg = err.to_string();
    assert!(
        msg.contains("exact old absent") && msg.contains("compute_checksum"),
        "error must name candidate: {msg}"
    );
    // Opt-in restores historical apply-on-absent-old behavior.
    let opts_on = ReplaceOptions {
        allow_absent_old: true,
        ..opts
    };
    let r = replace_in_content(content, "compute_cheksum", "compute_digest", &opts_on)
        .expect("opt-in must apply fuzzy candidate");
    assert!(r.changed);
    assert!(r.new_content.contains("compute_digest"));
    assert!(!r.new_content.contains("compute_checksum"));
}

// ── #1935 structured EditErrorKind for file create/delete/rename ───────────

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_create_already_exists_is_already_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "x\n").unwrap();
    let err = file_create(&file, "y", false, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::AlreadyExists),
        "already-exists must peel without string scrape (#1947): {err}"
    );
    assert!(
        crate::fallback::is_already_exists(&err),
        "is_already_exists must see dest-exists: {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("already_exists"),
        "CLI-stable kind string (#1948): {err}"
    );
    assert!(
        err.to_string().contains("already exists"),
        "message should stay useful: {err}"
    );
    assert!(
        err.to_string().contains("force"),
        "create dest-exists should hint force: {err}"
    );
}

/// Dangling symlink is a present entry → already_exists without force (#2087).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_create_dangling_symlink_is_already_exists() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    let err = file_create(&link, "y\n", false, ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::fallback::is_already_exists(&err),
        "dangling create without force: {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("already_exists")
    );
    // force must not follow or replace a dest symlink
    let err = file_create(&link, "y\n", true, ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "force-create on dest symlink must be invalid_input, got: {err:#}"
    );
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
}

/// `file.create` force must not follow a dest symlink and overwrite the target.
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_create_force_refuses_dest_symlink() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("app.toml");
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("secret");
    fs::write(&outside_file, "do not overwrite").unwrap();
    std::os::unix::fs::symlink(&outside_file, &dest).unwrap();

    let err = file_create(&dest, "pwned\n", true, ApplyMode::Apply, None).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "force-create on dest symlink must be invalid_input, got: {err:#}"
    );
    assert_eq!(
        fs::read_to_string(&outside_file).unwrap(),
        "do not overwrite"
    );
    assert!(dest.symlink_metadata().unwrap().file_type().is_symlink());
}

#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_rename_refuses_dest_symlink() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.txt");
    let dest = dir.path().join("app.toml");
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("secret");
    fs::write(&src, "payload\n").unwrap();
    fs::write(&outside_file, "do not overwrite").unwrap();
    std::os::unix::fs::symlink(&outside_file, &dest).unwrap();

    for force in [false, true] {
        let err = file_rename(&src, &dest, force, ApplyMode::Apply, None).unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "file.rename dest symlink force={force} must be invalid_input, got: {err:#}"
        );
        assert_eq!(
            fs::read_to_string(&outside_file).unwrap(),
            "do not overwrite"
        );
        assert!(src.exists(), "source must remain when dest is a symlink");
        assert!(
            dest.symlink_metadata().unwrap().file_type().is_symlink(),
            "dest must remain a symlink"
        );
    }
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_rename_destination_exists_is_already_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    let dst = dir.path().join("b.txt");
    fs::write(&file, "x\n").unwrap();
    fs::write(&dst, "z\n").unwrap();
    let err = file_rename(&file, &dst, false, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::AlreadyExists),
        "dest exists must peel as AlreadyExists (#1947): {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("already_exists"),
        "CLI-stable kind string (#1948): {err}"
    );
    assert!(
        crate::fallback::is_already_exists(&err),
        "rename dest-exists must set is_already_exists: {err}"
    );
    assert!(err.to_string().contains("already exists"), "message: {err}");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_delete_missing_is_not_found() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("gone.txt");
    let err = file_delete(&missing, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NotFound),
        "missing delete must peel as NotFound not OperationFailed: {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("not_found"),
        "CLI-stable not_found (#1948 sibling): {err}"
    );
    assert!(
        crate::fallback::is_not_found(&err),
        "is_not_found bool peel for missing delete: {err}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_append_missing_is_not_found() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("gone.txt");
    let err = file_append(&missing, "x\n", ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NotFound),
        "missing append must peel as NotFound: {err}"
    );
    assert_eq!(
        crate::fallback::error_kind_str(&err),
        Some("not_found"),
        "CLI-stable not_found for append: {err}"
    );
    assert!(
        crate::fallback::is_not_found(&err),
        "is_not_found bool peel for missing append: {err}"
    );
}

/// Dangling symlink is present but not a regular file → invalid_input, not not_found.
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_append_dangling_symlink_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    let err = file_append(&link, "x\n", ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "dangling append must be invalid_input: {err}"
    );
    assert!(
        crate::fallback::is_invalid_input(&err),
        "is_invalid_input peel: {err}"
    );
    assert!(crate::ops::file::path_entry_exists(&link));
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn execute_plan_patch_merge_conflict_is_conflicts() {
    // Real library path: on_stale merge without allow_conflicts → ConflictsError.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
    let diff =
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";
    let plan = crate::plan::Plan {
        version: 1,
        cwd: Some(dir.path().to_string_lossy().into()),
        write_policy: None,
        strict: None,
        operations: vec![crate::plan::Operation::PatchApply {
            diff: diff.into(),
            on_stale: crate::ops::patch::OnStale::Merge,
            allow_conflicts: false,
            replace_all: false,
        }],
        format: None,
        validate: None,
        verify: None,
        for_each: None,
    };
    // Plan failures return Ok(PlanReport) with ok:false + error_kind (not Err).
    let report = execute_plan(plan, dir.path(), None).expect("plan report");
    assert!(!report.ok, "merge conflict plan must fail: {report:?}");
    assert_eq!(
        report.error_kind.as_deref(),
        Some("conflicts"),
        "PlanReport must carry CLI-stable conflicts kind: {report:?}"
    );
    // When hosts wrap the report error string as anyhow, typed peel still works
    // if the underlying ConflictsError remains in the chain; for plan reports
    // the string field is the primary branch point.
    assert!(
        report
            .error
            .as_deref()
            .is_some_and(|e| e.contains("conflict")),
        "error message should mention conflict: {report:?}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_delete_directory_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();
    let err = file_delete(&sub, ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "directory delete must peel as InvalidInput: {err}"
    );
    assert!(err.to_string().contains("not a file"), "message: {err}");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_append_binary_is_binary() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("b.bin");
    fs::write(&bin, b"x\x00y").unwrap();
    let err = file_append(&bin, "z\n", ApplyMode::Apply, None).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::Binary),
        "binary append must peel Binary (#1963): {err}"
    );
    assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
    assert!(
        err.to_string().to_ascii_lowercase().contains("binary"),
        "message: {err}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_rename_binary_path_only_succeeds() {
    // Path-only rename: binary must move without host OS fallback (#2031).
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("b.bin");
    let bin_dst = dir.path().join("c.bin");
    let bytes = b"x\x00y".as_slice();
    fs::write(&bin, bytes).unwrap();
    let r = file_rename(&bin, &bin_dst, false, ApplyMode::Apply, None)
        .expect("binary rename must succeed as path-only (#2031)");
    assert!(r.applied);
    assert_eq!(r.action, "rename");
    assert!(!bin.exists(), "source must be gone after rename");
    assert!(bin_dst.exists());
    assert_eq!(fs::read(&bin_dst).unwrap(), bytes);
    assert!(
        r.backup_session.is_some(),
        "path-only rename must still create backup"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_rename_invalid_utf8_path_only_succeeds() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("bad.txt");
    let dst = dir.path().join("ok.txt");
    // Invalid UTF-8 without NUL (not binary probe) still must path-rename.
    fs::write(&src, b"hello\xffworld").unwrap();
    let r = file_rename(&src, &dst, false, ApplyMode::Apply, None)
        .expect("invalid UTF-8 rename must succeed (#2031)");
    assert!(r.applied);
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"hello\xffworld");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_delete_binary_path_only_succeeds() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("b.bin");
    fs::write(&bin, b"x\x00y").unwrap();
    let r = file_delete(&bin, ApplyMode::Apply, None).expect("binary delete (#2031)");
    assert!(r.applied);
    assert!(!bin.exists());
    assert!(
        r.backup_session.as_ref().is_some_and(|s| !s.is_empty()),
        "binary delete must record backup session: {r:?}"
    );
}

/// FIFO delete under PathGuard (#2087).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_fifo_under_guard_succeeds() {
    use std::process::Command as StdCommand;

    let dir = TempDir::new().unwrap();
    let fifo = dir.path().join("pipe.fifo");
    // mkfifo via shell — std has no portable mkfifo helper.
    let status = StdCommand::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo available on unix CI");
    assert!(status.success(), "mkfifo must create {fifo:?}");
    assert!(fifo.exists());

    let guard = crate::containment::PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .expect("guard");
    let r = file_delete(&fifo, ApplyMode::Apply, Some(&guard)).expect("FIFO delete (#2087)");
    assert!(r.applied);
    assert!(!fifo.exists(), "FIFO must be unlinked");
    assert!(
        r.backup_session.is_some(),
        "special-node delete still creates a backup session"
    );

    // DryRun must not remove a FIFO.
    let fifo2 = dir.path().join("pipe2.fifo");
    assert!(
        StdCommand::new("mkfifo")
            .arg(&fifo2)
            .status()
            .unwrap()
            .success()
    );
    let preview = file_delete(&fifo2, ApplyMode::Preview, Some(&guard)).expect("preview");
    assert!(!preview.applied);
    assert!(fifo2.exists(), "preview must not unlink FIFO");
}

/// Symlink delete unlinks the link only, never the target (#2087).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_symlink_unlinks_link_not_target() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, "keep me\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let r = file_delete(&link, ApplyMode::Apply, None).expect("symlink delete");
    assert!(r.applied);
    assert!(!link.exists(), "symlink must be gone");
    assert!(target.exists(), "target must remain");
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep me\n");
}

/// Symlink to a path outside the workspace: entry guard allows unlink (#2115).
///
/// Follow-mode `check_path` would deny (resolved target escapes). Hosts must
/// not need parent-only precheck + `guard: None`.
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_symlink_to_outside_under_workspace_guard() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.env");
    fs::write(&secret, "SECRET=1\n").unwrap();
    let link = dir.path().join("link-out");
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    let guard = crate::containment::PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .expect("guard");
    // Sanity: follow-mode containment would reject the resolved target.
    assert!(
        guard.check_path(link.to_str().unwrap()).is_err(),
        "check_path must follow outside target and deny"
    );
    assert!(
        guard.would_allow_entry(link.to_str().unwrap()),
        "check_path_entry must allow the workspace link entry"
    );

    let r = file_delete(&link, ApplyMode::Apply, Some(&guard))
        .expect("entry guard must allow unlink of outside-target link");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(secret.exists(), "outside target must never be deleted");
    assert_eq!(fs::read_to_string(&secret).unwrap(), "SECRET=1\n");
}

/// Dangling symlink under workspace guard (#2115 entry mode).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_dangling_symlink_under_guard() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    let guard = crate::containment::PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .expect("guard");
    let r = file_delete(&link, ApplyMode::Apply, Some(&guard)).expect("dangling under guard");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
}

/// Dangling symlink is still unlinkable (#2087 path_entry_exists).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_dangling_symlink_succeeds() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    assert!(
        crate::ops::file::path_entry_exists(&link),
        "dangling link must count as an entry"
    );
    assert!(!link.exists(), "Path::exists follows and reports false");
    let r = file_delete(&link, ApplyMode::Apply, None).expect("dangling symlink delete");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
}

/// Symlink to a directory: unlink the link, leave the real dir (#2087).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_delete_symlink_to_directory() {
    let dir = TempDir::new().unwrap();
    let real_dir = dir.path().join("realdir");
    fs::create_dir(&real_dir).unwrap();
    fs::write(real_dir.join("inside.txt"), "x\n").unwrap();
    let link = dir.path().join("linkdir");
    std::os::unix::fs::symlink(&real_dir, &link).unwrap();
    let r = file_delete(&link, ApplyMode::Apply, None).expect("symlink-to-dir delete");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(real_dir.is_dir());
    assert_eq!(
        fs::read_to_string(real_dir.join("inside.txt")).unwrap(),
        "x\n"
    );
}

/// Rename of outside-target symlink under workspace guard (#2115).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_rename_symlink_to_outside_under_workspace_guard() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.env");
    fs::write(&secret, "SECRET=1\n").unwrap();
    let link = dir.path().join("link-out");
    let moved = dir.path().join("moved-out");
    std::os::unix::fs::symlink(&secret, &link).unwrap();

    let guard = crate::containment::PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .expect("guard");
    let r = file_rename(&link, &moved, false, ApplyMode::Apply, Some(&guard))
        .expect("entry guard must allow rename of outside-target link");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(moved.is_symlink());
    assert_eq!(fs::read_link(&moved).unwrap(), secret);
    assert_eq!(fs::read_to_string(&secret).unwrap(), "SECRET=1\n");
}

/// Rename moves the symlink entry only; never rewrites the target (#2091).
///
/// Regression: soft-loading symlink text then atomic_write resolved the link
/// and mutated the target when write policy (or engine rewrite) applied.
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_rename_symlink_does_not_mutate_target() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    let moved = dir.path().join("moved-link.txt");
    // No final newline: a write-policy rewrite would add one if it followed the link.
    fs::write(&target, "hello").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let r = file_rename(&link, &moved, false, ApplyMode::Apply, None).expect("symlink rename");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(
        moved.is_symlink(),
        "dest must remain a symlink, not a regular file"
    );
    assert_eq!(
        fs::read_link(&moved).unwrap(),
        target,
        "symlink target path must be unchanged"
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "hello",
        "target content must not gain a trailing newline or other rewrite"
    );
}

/// Dangling symlink rename succeeds via path_entry_exists (#2091).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_rename_dangling_symlink_succeeds() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling");
    let dest = dir.path().join("renamed-dangle");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    assert!(!link.exists(), "Path::exists follows and reports false");
    let r = file_rename(&link, &dest, false, ApplyMode::Apply, None).expect("dangling rename");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(
        crate::ops::file::path_entry_exists(&dest),
        "dangling link must move to dest"
    );
    assert!(dest.is_symlink());
}

/// Symlink-to-directory is renameable (move the link, leave the real dir) (#2091).
#[cfg(all(unix, any(feature = "cli", feature = "files")))]
#[test]
fn file_rename_symlink_to_directory() {
    let dir = TempDir::new().unwrap();
    let real_dir = dir.path().join("realdir");
    fs::create_dir(&real_dir).unwrap();
    fs::write(real_dir.join("inside.txt"), "x\n").unwrap();
    let link = dir.path().join("linkdir");
    let dest = dir.path().join("linkdir2");
    std::os::unix::fs::symlink(&real_dir, &link).unwrap();
    let r = file_rename(&link, &dest, false, ApplyMode::Apply, None).expect("symlink-dir rename");
    assert!(r.applied);
    assert!(!crate::ops::file::path_entry_exists(&link));
    assert!(dest.is_symlink());
    assert!(real_dir.is_dir());
    assert_eq!(
        fs::read_to_string(real_dir.join("inside.txt")).unwrap(),
        "x\n"
    );
}

/// Library doc_set keeps block-sequence item indent (`- name` / `value`).
/// Collapse would be `style_changed` (#2088); CST now leaves it false.
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn doc_set_keeps_yaml_sequence_item_indent() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("cfg.yaml");
    fs::write(&file, "env:\n  - name: FEATURE_FLAG\n    value: off\n").unwrap();
    let r = doc_set(
        &file,
        "env.0.value",
        serde_json::json!("on"),
        ApplyMode::Apply,
        None,
    )
    .expect("doc_set");
    assert!(r.applied);
    assert!(r.changed);
    assert_eq!(
        r.new_content,
        "env:\n  - name: FEATURE_FLAG\n    value: on\n"
    );
    assert!(
        !r.style_changed,
        "kept sequence indent must not flag style; new=\n{}",
        r.new_content
    );
    assert!(!crate::api::is_style_changed(&r));

    let r2 = doc_set(
        &file,
        "env.0.value",
        serde_json::json!("on2"),
        ApplyMode::Apply,
        None,
    )
    .expect("second doc_set");
    assert!(r2.applied);
    let txt = dir.path().join("n.txt");
    fs::write(&txt, "a\n").unwrap();
    let cr = file_create(&txt, "b\n", true, ApplyMode::Apply, None).expect("create");
    assert!(!cr.style_changed);
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_rename_force_binary_over_existing() {
    // Force overwrite dest when source is binary (#2031 force + non-text).
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    let bytes = b"bin\x00src";
    fs::write(&src, bytes).unwrap();
    fs::write(&dst, b"old\x00dst").unwrap();
    let r = file_rename(&src, &dst, true, ApplyMode::Apply, None)
        .expect("force binary rename over existing");
    assert!(r.applied);
    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), bytes);
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_create_guard_rejected_is_guard_rejected() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-file-create-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let err = file_create(&outside, "n", false, ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "engine PathGuard must peel as GuardRejected not InvalidInput: {err}"
    );
    assert!(
        err.to_string().contains("guard") || err.to_string().contains("escapes"),
        "message: {err}"
    );
    assert!(!outside.exists());
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_append_guard_rejected_is_guard_rejected() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-file-append-escape-{}.txt",
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
    let err = file_append(&outside, "x\n", ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "file_append guard: {err}"
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "secret\n");
    let _ = fs::remove_file(&outside);
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_delete_guard_rejected_is_guard_rejected() {
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-file-delete-escape-{}.txt",
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
    let err = file_delete(&outside, ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "file_delete guard: {err}"
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "secret\n");
    let _ = fs::remove_file(&outside);
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn file_rename_guard_rejected_is_guard_rejected() {
    let dir = TempDir::new().unwrap();
    let inside = dir.path().join("a.txt");
    fs::write(&inside, "x\n").unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-file-rename-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let err = file_rename(&inside, &outside, false, ApplyMode::Apply, Some(&guard)).unwrap_err();
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "file_rename dest guard: {err}"
    );
    assert!(inside.exists());
    assert!(!outside.exists());
}

// ── #1936 structured EditErrorKind for ast_rewrite_signature ───────────────

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_missing_function_is_no_match() {
    use crate::ast::rewrite::FunctionSigEdit;
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn keep() {}\n").unwrap();
    let edit = FunctionSigEdit {
        parameters: Some("(x: i32)".into()),
        ..Default::default()
    };
    let err = ast_rewrite_signature(&file, "missing", &edit, None, ApplyMode::Apply, None)
        .expect_err("missing function");
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::NoMatch),
        "must peel NoMatch without English scrape: {err}"
    );
    assert!(
        err.to_string().contains("missing") || err.to_string().contains("not found"),
        "message should name the function: {err}"
    );
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_binary_is_binary() {
    use crate::ast::rewrite::FunctionSigEdit;
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("x.bin");
    fs::write(&bin, b"\x00").unwrap();
    let edit = FunctionSigEdit {
        parameters: Some("(x: i32)".into()),
        ..Default::default()
    };
    let err =
        ast_rewrite_signature(&bin, "x", &edit, None, ApplyMode::Apply, None).expect_err("binary");
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::Binary),
        "binary AST rewrite must peel Binary (#1963): {err}"
    );
    assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_guard_rejected() {
    use crate::ast::rewrite::FunctionSigEdit;
    let dir = TempDir::new().unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-ast-sig-escape-{}.rs",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(&outside, "fn keep() {}\n").unwrap();
    let guard = PathGuard::new(
        dir.path().to_path_buf(),
        AbsolutePathPolicy::AllowIfContained,
    )
    .unwrap();
    let edit = FunctionSigEdit {
        parameters: Some("(x: i32)".into()),
        ..Default::default()
    };
    let err = ast_rewrite_signature(
        &outside,
        "keep",
        &edit,
        None,
        ApplyMode::Apply,
        Some(&guard),
    )
    .expect_err("outside path");
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::GuardRejected),
        "ast_rewrite_signature guard: {err}"
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "fn keep() {}\n");
    let _ = fs::remove_file(&outside);
}

#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rewrite_signature_empty_edit_is_invalid_input() {
    use crate::ast::rewrite::FunctionSigEdit;
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn keep() {}\n").unwrap();
    let edit = FunctionSigEdit::default();
    let err = ast_rewrite_signature(&file, "keep", &edit, None, ApplyMode::Apply, None)
        .expect_err("empty edit");
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "empty edit fields: {err}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_after_strips_markers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("t.rs");
    fs::write(&path, "fn foo() {\n  a();\n}\n").unwrap();
    let fragment = "// ... existing code ...\n  bar();\n// ... existing code ...\n";
    let r = apply_fragment_to_file(
        &path,
        fragment,
        FragmentPlacement::After("fn foo() {".into()),
        true,
        ApplyMode::Apply,
        None,
    )
    .expect("apply_fragment_to_file after (#2032)");
    assert!(r.applied);
    assert!(r.changed);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn foo() {\n  bar();\n  a();\n}\n",
        "markers stripped and insert after fn foo() {{"
    );
    assert!(
        r.backup_session.as_ref().is_some_and(|s| !s.is_empty()),
        "apply_fragment apply must record backup session: {r:?}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_after_anchor_includes_indent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("m.rs");
    fs::write(&path, "fn main() {\n    let x = 1;\n}\n").unwrap();
    let r = apply_fragment_to_file(
        &path,
        "let y = 2;",
        FragmentPlacement::After("    let x = 1;".into()),
        true,
        ApplyMode::Apply,
        None,
    )
    .expect("after indented anchor");
    assert!(r.applied);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn main() {\n    let x = 1;\n    let y = 2;\n}\n"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_before_anchor_includes_indent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("m.rs");
    fs::write(&path, "fn main() {\n    let x = 1;\n}\n").unwrap();
    let r = apply_fragment_to_file(
        &path,
        "let y = 2;",
        FragmentPlacement::Before("    let x = 1;".into()),
        true,
        ApplyMode::Apply,
        None,
    )
    .expect("before indented anchor");
    assert!(r.applied);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "fn main() {\n    let y = 2;\n    let x = 1;\n}\n"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_requires_unique_anchor() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("t.rs");
    fs::write(&path, "x\nx\n").unwrap();
    let err = apply_fragment_to_file(
        &path,
        "y\n",
        FragmentPlacement::After("x".into()),
        true,
        ApplyMode::Preview,
        None,
    )
    .expect_err("ambiguous after must fail closed");
    assert!(
        crate::fallback::is_ambiguous(&err)
            || crate::fallback::edit_error_kind(&err) == Some(EditErrorKind::AmbiguousTarget),
        "ambiguous peel: {err}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "x\nx\n");
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_empty_after_strip_invalid() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("t.rs");
    fs::write(&path, "fn foo() {}\n").unwrap();
    let err = apply_fragment_to_file(
        &path,
        "// ... existing code ...\n",
        FragmentPlacement::After("fn foo() {}".into()),
        true,
        ApplyMode::Apply,
        None,
    )
    .expect_err("empty fragment after strip");
    assert_eq!(
        crate::fallback::edit_error_kind(&err),
        Some(EditErrorKind::InvalidInput),
        "empty strip: {err}"
    );
}

#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn apply_fragment_to_file_replace_old() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("t.rs");
    fs::write(&path, "return 0;\n").unwrap();
    let r = apply_fragment_to_file(
        &path,
        "return 1;\n",
        FragmentPlacement::Replace("return 0;\n".into()),
        true,
        ApplyMode::Apply,
        None,
    )
    .expect("replace old");
    assert!(r.applied);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "return 1;\n",
        "replace must swap the whole old line, not prepend"
    );
}

#[test]
fn content_edit_honesty_constructors_for_hosts() {
    let exact = ContentEditHonesty::exact(0, "old", "old");
    assert_eq!(exact.op_index, 0);
    assert_eq!(exact.old, "old");
    assert_eq!(exact.matched_text.as_deref(), Some("old"));
    assert_eq!(exact.match_mode, Some(MatchMode::Exact));
    assert!(exact.match_score.is_none());

    let fuzzy = ContentEditHonesty::fuzzy(1, "a", 0.91, "aaaa");
    assert_eq!(fuzzy.op_index, 1);
    assert_eq!(fuzzy.match_mode, Some(MatchMode::Fuzzy));
    assert_eq!(fuzzy.match_score, Some(0.91));
    assert_eq!(fuzzy.matched_text.as_deref(), Some("aaaa"));
    assert_eq!(fuzzy.old, "a");
}

/// Prove library multi-component relative path (fixloop).
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn replace_text_relative_nested_path_does_not_double_join() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("src");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("lib.rs");
    fs::write(&file, "fn old() {}\n").unwrap();
    let _cwd = CwdGuard::enter(dir.path());
    let r = replace_text(
        Path::new("src/lib.rs"),
        "old",
        "new",
        &ReplaceOptions::default(),
        ApplyMode::Apply,
        None,
    )
    .expect("relative nested path should resolve");
    assert!(r.changed, "expected change: {:?}", r);
    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("fn new"), "got {content}");
}

/// Missing path peels NotFound (not OperationFailed) for library hosts.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_missing_path_is_not_found() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nope.rs");
    let err = ast_rename(&missing, "foo", "bar", ApplyMode::Preview, None).unwrap_err();
    assert!(
        crate::api::is_not_found(&err),
        "expected not_found peel, got kind={:?} err={err}",
        crate::fallback::edit_error_kind(&err)
    );
}

/// Batch remapper preserves Binary peel from sole-path load.
#[cfg(all(feature = "ast", any(feature = "cli", feature = "files")))]
#[test]
fn ast_rename_batch_binary_peels_binary() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("x.rs");
    fs::write(&bin, b"fn foo() {}\0").unwrap();
    let opts = AstRenameBatchOptions {
        mode: ApplyMode::Preview,
        continue_on_no_match: true,
        fail_fast: false,
        ..Default::default()
    };
    let results = ast_rename_batch(&[&bin], "foo", "bar", &opts, None).unwrap();
    assert_eq!(results.len(), 1);
    let err = results[0].result.as_ref().unwrap_err();
    assert_eq!(
        err.kind,
        EditErrorKind::Binary,
        "batch must not collapse Binary to OperationFailed: {err:?}"
    );
}

/// Library doc_set multi-component relative path (sibling of replace absolutize).
#[cfg(any(feature = "cli", feature = "files"))]
#[test]
fn doc_set_relative_nested_path_does_not_double_join() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("cfg");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("app.json");
    fs::write(&file, r#"{"v":1}"#).unwrap();
    let _cwd = CwdGuard::enter(dir.path());
    let r = doc_set(
        Path::new("cfg/app.json"),
        "v",
        serde_json::json!(2),
        ApplyMode::Apply,
        None,
    )
    .expect("relative nested doc path should resolve");
    assert!(r.changed, "{r:?}");
    let content = fs::read_to_string(&file).unwrap();
    let val: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("valid JSON: {e}; got {content}"));
    assert_eq!(val["v"], serde_json::json!(2), "got {content}");
}

#[test]
fn apply_patch_file_deletion_unlinks() {
    let dir = TempDir::new().unwrap();
    let f = dir.path().join("gone.txt");
    fs::write(&f, "bye\n").unwrap();
    let patch = "--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].changed);
    assert!(
        !f.exists(),
        "deletion patch must unlink, not leave empty file"
    );
}

#[test]
fn apply_patch_file_case_only_rename_preserves_inode_content() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("readme.md");
    fs::write(&src, "hello content\n").unwrap();
    let patch = "\
diff --git a/readme.md b/README.md\n\
similarity index 100%\n\
rename from readme.md\n\
rename to README.md\n";
    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].changed);
    // Content must survive on case-insensitive FS (write+delete would wipe it).
    let content = fs::read_to_string(dir.path().join("README.md"))
        .or_else(|_| fs::read_to_string(dir.path().join("readme.md")))
        .expect("content must exist after case-only rename");
    assert_eq!(content, "hello content\n");
}

#[test]
fn apply_patch_file_pure_rename_binary() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("logo.png");
    fs::write(&src, b"\x89PNG\r\n\x1a\n\x00\x00").unwrap();
    let patch = "\
diff --git a/logo.png b/assets/logo.png\n\
similarity index 100%\n\
rename from logo.png\n\
rename to assets/logo.png\n";
    let results = apply_patch_file(patch, dir.path(), ApplyMode::Apply, None).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].changed);
    assert!(!src.exists());
    let dest = dir.path().join("assets/logo.png");
    assert!(dest.exists(), "binary pure rename must move bytes");
    assert_eq!(fs::read(&dest).unwrap(), b"\x89PNG\r\n\x1a\n\x00\x00");
}
