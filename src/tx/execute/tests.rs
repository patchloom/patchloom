use super::*;
use crate::cli::global::GlobalFlags;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tempfile::TempDir;

// ---- read_file_content ----

#[test]
fn read_file_content_from_disk() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    std::fs::write(&file, "hello world").unwrap();

    let mut pending = HashMap::new();
    let mut existed = HashSet::new();

    let content = read_file_content(&mut pending, &mut existed, &file).unwrap();
    assert_eq!(content, "hello world");
    assert!(existed.contains(&file));
    assert!(pending.contains_key(&file));
    // Original and current should both be "hello world".
    let (orig, cur) = &pending[&file];
    assert_eq!(orig, "hello world");
    assert_eq!(cur, "hello world");
}

#[test]
fn read_file_content_from_pending() {
    let path = PathBuf::from("/fake/already_loaded.txt");
    let mut pending = HashMap::new();
    pending.insert(
        path.clone(),
        ("original".to_string(), "modified".to_string()),
    );
    let mut existed = HashSet::new();

    let content = read_file_content(&mut pending, &mut existed, &path).unwrap();
    assert_eq!(content, "modified");
    // Should not add to existed_before since it was already in pending.
    assert!(!existed.contains(&path));
}

#[test]
fn read_file_content_missing_file_errors() {
    let mut pending = HashMap::new();
    let mut existed = HashSet::new();
    let path = PathBuf::from("/nonexistent/file.txt");

    let result = read_file_content(&mut pending, &mut existed, &path);
    result.expect_err("expected error");
}

// ---- read_and_probe ----

#[test]
fn read_and_probe_text_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("text.txt");
    std::fs::write(&file, "text content").unwrap();

    let mut pending = HashMap::new();
    let mut existed = HashSet::new();

    assert!(read_and_probe(&mut pending, &mut existed, &file).unwrap());
    assert!(pending.contains_key(&file));
}

#[test]
fn read_and_probe_binary_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    // Write bytes with NUL to trigger binary detection.
    std::fs::write(&file, b"\x00\x01\x02\x03").unwrap();

    let mut pending = HashMap::new();
    let mut existed = HashSet::new();

    assert!(!read_and_probe(&mut pending, &mut existed, &file).unwrap());
    assert!(!pending.contains_key(&file));
}

#[test]
fn read_and_probe_already_loaded_skips() {
    let path = PathBuf::from("/fake/loaded.txt");
    let mut pending = HashMap::new();
    pending.insert(path.clone(), ("orig".to_string(), "cur".to_string()));
    let mut existed = HashSet::new();

    assert!(read_and_probe(&mut pending, &mut existed, &path).unwrap());
}

// ---- update_file_content ----

#[test]
fn update_file_content_existing_entry() {
    let path = PathBuf::from("/fake/file.txt");
    let mut pending = HashMap::new();
    let mut deletions = HashSet::new();
    let mut write_targets = HashSet::new();
    pending.insert(path.clone(), ("original".to_string(), "old".to_string()));

    update_file_content(
        &mut pending,
        &mut deletions,
        &mut write_targets,
        &path,
        "new content".into(),
    );

    let (orig, cur) = &pending[&path];
    assert_eq!(orig, "original"); // original preserved
    assert_eq!(cur, "new content");
    assert!(write_targets.contains(&path));
}

#[test]
fn update_file_content_new_entry() {
    let path = PathBuf::from("/fake/new_file.txt");
    let mut pending = HashMap::new();
    let mut deletions = HashSet::new();
    let mut write_targets = HashSet::new();

    update_file_content(
        &mut pending,
        &mut deletions,
        &mut write_targets,
        &path,
        "content".into(),
    );

    let (orig, cur) = &pending[&path];
    assert!(orig.is_empty()); // no original for new files
    assert_eq!(cur, "content");
    assert!(write_targets.contains(&path));
}

#[test]
fn update_file_content_clears_deletion() {
    let path = PathBuf::from("/fake/file.txt");
    let mut pending = HashMap::new();
    let mut deletions = HashSet::new();
    let mut write_targets = HashSet::new();
    deletions.insert(path.clone());

    update_file_content(
        &mut pending,
        &mut deletions,
        &mut write_targets,
        &path,
        "revived".into(),
    );

    assert!(!deletions.contains(&path));
    assert!(write_targets.contains(&path));
}

// ---- path_err ----

#[test]
fn path_err_wraps_message() {
    let wrapper = path_err("config.yaml");
    let err = wrapper("invalid key");
    assert_eq!(format!("{err}"), "config.yaml: invalid key");
}

// ---- op_needs_doc_flush ----

#[test]
fn op_needs_doc_flush_for_replace() {
    let op = Operation::Replace {
        path: Some("f.txt".into()),
        glob: None,
        regex: false,
        old: "a".into(),
        new_text: Some("b".into()),
        nth: None,
        insert_before: None,
        insert_after: None,
        case_insensitive: false,
        multiline: false,
        whole_line: false,
        word_boundary: false,
        range: None,
        before_context: None,
        after_context: None,
        if_exists: false,
        unique: false,
        require_change: false,
        command_position: false,
    };
    assert!(op_needs_doc_flush(&op));
}

#[test]
fn op_needs_doc_flush_false_for_doc_set() {
    let op = Operation::DocSet {
        path: "f.json".into(),
        selector: "key".into(),
        value: serde_json::json!("val"),
    };
    assert!(!op_needs_doc_flush(&op));
}

#[test]
fn op_needs_doc_flush_for_read() {
    let op = Operation::Read {
        path: "f.txt".into(),
        lines: None,
    };
    assert!(op_needs_doc_flush(&op));
}

#[test]
fn op_needs_doc_flush_for_search() {
    let op = Operation::Search {
        path: "src".into(),
        pattern: "TODO".into(),
        regex: false,
        case_insensitive: false,
        multiline: false,
        invert_match: false,
        context: None,
        before_context: None,
        after_context: None,
        assert_count: None,
        literal: false,
        globs: Vec::new(),
        max_results: 0,
        exclude_patterns: Vec::new(),
        custom_ignore_filenames: Vec::new(),
    };
    assert!(op_needs_doc_flush(&op));
}

/// Regression: appending to a file deleted earlier in the same tx must
/// fail, not silently resurrect the file.
#[test]
fn file_append_to_deleted_file_errors() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("victim.txt");
    std::fs::write(&file, "original").unwrap();

    let mut f = TxStateFixture::new();
    // Simulate: file was loaded and then deleted in this tx.
    let _ = read_file_content(&mut f.pending, &mut f.existed_before, &file).unwrap();
    f.deletions.insert(file.clone());

    let mut tx = f.state(dir.path());

    let op = Operation::FileAppend {
        path: "victim.txt".into(),
        content: "new stuff".into(),
    };
    let result = execute_file_op(&op, &mut tx);
    assert!(
        result.is_err(),
        "append to deleted file should error, not resurrect"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("deleted earlier"),
        "error message should mention deletion: {msg}"
    );
}

/// Same regression for prepend.
#[test]
fn file_prepend_to_deleted_file_errors() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("victim.txt");
    std::fs::write(&file, "original").unwrap();

    let mut f = TxStateFixture::new();
    let _ = read_file_content(&mut f.pending, &mut f.existed_before, &file).unwrap();
    f.deletions.insert(file.clone());

    let mut tx = f.state(dir.path());

    let op = Operation::FilePrepend {
        path: "victim.txt".into(),
        content: "prefix".into(),
    };
    let result = execute_file_op(&op, &mut tx);
    assert!(
        result.is_err(),
        "prepend to deleted file should error, not resurrect"
    );
}

#[test]
fn md_move_section_same_file_by_path_equality() {
    // Regression: MdMoveSection with to=Some(same_path) must detect
    // same-file via path equality, not just canonicalize (which fails
    // for files created in-tx that don't exist on disk).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    std::fs::write(&file, "# A\ntext a\n# B\ntext b\n").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::MdMoveSection {
        path: "doc.md".into(),
        heading: "# A".into(),
        to: Some("doc.md".into()),
        before: None,
        after: Some("# B".into()),
    };
    execute_operation(&op, &mut tx).unwrap();
    drop(tx);
    // Section A should appear after B, not be duplicated
    let content = &f.pending[&file].1;
    let a_pos = content.find("# A").unwrap();
    let b_pos = content.find("# B").unwrap();
    assert!(a_pos > b_pos, "section A should be after B: {content}");
    // Section A should appear exactly once
    assert_eq!(
        content.matches("# A").count(),
        1,
        "section A should not be duplicated: {content}"
    );
}

#[test]
fn rename_deleted_source_is_rejected() {
    // Regression: FileRename of a source file deleted earlier in the
    // same transaction should error, not silently create an empty file.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("victim.txt");
    std::fs::write(&file, "content").unwrap();

    let mut f = TxStateFixture::new();
    // Simulate deletion
    f.pending
        .insert(file.clone(), ("content".to_string(), String::new()));
    f.deletions.insert(file);
    f.existed_before.insert(dir.path().join("victim.txt"));

    let mut tx = f.state(dir.path());

    let op = Operation::FileRename {
        from: "victim.txt".into(),
        to: "dest.txt".into(),
        force: false,
    };
    let result = execute_file_op(&op, &mut tx);
    assert!(result.is_err(), "rename of deleted file should error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("deleted earlier"),
        "error should mention deletion: {msg}"
    );
}

/// Regression: files loaded for Read/Search operations should not be
/// modified by write policy (e.g. ensure_final_newline) (#1108).
#[test]
fn read_only_files_skip_write_policy() {
    let dir = TempDir::new().unwrap();
    // Create a file WITHOUT a trailing newline.
    let file = dir.path().join("readonly.txt");
    std::fs::write(&file, "no trailing newline").unwrap();

    // Build a plan that only reads the file, with ensure_final_newline active.
    let plan = Plan {
        version: crate::plan::SCHEMA_VERSION,
        operations: vec![Operation::Read {
            path: "readonly.txt".into(),
            lines: None,
        }],
        write_policy: Some(crate::write::WritePolicyOverride {
            ensure_final_newline: Some(true),
            ..Default::default()
        }),
        strict: None,
        format: None,
        validate: None,
        verify: None,
        cwd: None,
        for_each: None,
    };

    let global = GlobalFlags {
        ensure_final_newline: true,
        ..GlobalFlags::default()
    };
    let ctx = crate::tx::context::EngineContext::from_global(&global, dir.path().to_path_buf());
    let result = execute_and_collect(&plan, &ctx, true, false, None).unwrap();

    // The file should NOT appear in changes since it was only read.
    assert!(
        result.changes.is_empty(),
        "read-only file should not be modified by write policy, got {} changes",
        result.changes.len()
    );
    // Verify the file on disk is unchanged.
    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        on_disk, "no trailing newline",
        "file on disk should be unchanged"
    );
}

#[test]
fn execute_and_collect_preserves_no_match_error_kind() {
    // Missing doc.update target must remain NoMatchError (exit 3 path), not
    // a plain anyhow operation_failed, even after op-label wrapping.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    std::fs::write(&file, r#"{"a":1}"#).unwrap();

    let plan = Plan {
        version: crate::plan::SCHEMA_VERSION,
        operations: vec![Operation::DocUpdate {
            path: "data.json".into(),
            selector: "missing.key".into(),
            value: serde_json::json!(2),
        }],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        cwd: None,
        for_each: None,
    };
    let ctx = crate::tx::context::EngineContext::from_global(
        &GlobalFlags::default(),
        dir.path().to_path_buf(),
    );
    match execute_and_collect(&plan, &ctx, true, false, None) {
        Ok(_) => panic!("expected NoMatch for missing selector"),
        Err(err) => {
            assert!(
                crate::exit::is_no_match(&err),
                "NoMatch must survive execute wrap: {err:#}"
            );
            let msg = err.to_string();
            assert!(
                msg.contains("doc.update")
                    || msg.contains("matched nothing")
                    || msg.contains("missing"),
                "detail should remain: {msg}"
            );
        }
    }
}
