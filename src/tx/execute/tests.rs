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
fn read_and_probe_invalid_utf8_soft_skips() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("bad.txt");
    std::fs::write(&file, b"hello \xff world").unwrap();
    let mut pending = HashMap::new();
    let mut existed = HashSet::new();
    assert!(!read_and_probe(&mut pending, &mut existed, &file).unwrap());
    assert!(pending.is_empty());
}

#[test]
fn read_and_probe_missing_is_hard_err() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("missing.txt");
    let mut pending = HashMap::new();
    let mut existed = HashSet::new();
    let err = read_and_probe(&mut pending, &mut existed, &file).unwrap_err();
    assert!(
        err.to_string().contains("failed to read") || err.to_string().contains("missing"),
        "{err:#}"
    );
    // NotFound must remain classifiable through the chain (not a bare string).
    assert!(
        crate::exit::is_io_not_found(&err),
        "expected is_io_not_found, got: {err:#}"
    );
}

/// Strict sole-path load must refuse binary (not rewrite as text) (#1894).
#[test]
fn read_file_content_rejects_binary() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    std::fs::write(&file, b"hello\x00world").unwrap();

    let mut pending = HashMap::new();
    let mut existed = HashSet::new();

    let err = read_file_content(&mut pending, &mut existed, &file).unwrap_err();
    assert!(crate::exit::is_binary(&err), "{err:#}");
    assert!(err.to_string().contains("binary"), "{err}");
    assert!(pending.is_empty());
}

#[test]
fn read_file_content_rejects_invalid_utf8() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("bad.txt");
    std::fs::write(&file, b"hello \xff world").unwrap();

    let mut pending = HashMap::new();
    let mut existed = HashSet::new();

    let err = read_file_content(&mut pending, &mut existed, &file).unwrap_err();
    assert!(crate::exit::is_invalid_encoding(&err), "{err:#}");
    assert!(err.to_string().contains("UTF-8"), "{err}");
    assert!(pending.is_empty());
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
    let err = wrapper(anyhow::anyhow!("invalid key"));
    assert_eq!(err.to_string(), "config.yaml: invalid key");
}

#[test]
fn path_err_preserves_io_not_found() {
    let io_err: anyhow::Error = std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
    let err = path_err("missing.json")(io_err.context("failed to read"));
    assert!(
        crate::exit::is_io_not_found(&err),
        "path_err must keep NotFound: {err:#}"
    );
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
        fuzzy: false,
        min_fuzzy_score: None,
        allow_absent_old: false,
    };
    assert!(op_needs_doc_flush(&op));
}

#[test]
fn op_needs_doc_flush_false_for_doc_set() {
    let op = Operation::DocSet {
        path: "f.json".into(),
        selector: "key".into(),
        value: serde_json::json!("val"),
        if_exists: false,
    };
    assert!(!op_needs_doc_flush(&op));
}

#[test]
fn op_needs_doc_flush_for_read() {
    let op = Operation::Read {
        path: "f.txt".into(),
        lines: None,
        offset: None,
        limit: None,
        start_line: None,
        end_line: None,
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
fn file_append_rejects_whitespace_only() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FileAppend {
        path: "a.txt".into(),
        content: "   ".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError, got: {err:#}"
    );
    assert!(
        err.to_string().contains("whitespace-only"),
        "message should name whitespace: {err}"
    );
    assert!(
        f.pending.is_empty(),
        "must not stage a write for whitespace-only append"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn file_create_rejects_whitespace_only() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("a.txt");

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FileCreate {
        path: "a.txt".into(),
        content: "   ".into(),
        force: None,
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError, got: {err:#}"
    );
    assert!(
        err.to_string().contains("whitespace-only"),
        "message should name whitespace: {err}"
    );
    assert!(
        f.pending.is_empty(),
        "must not stage a write for whitespace-only create"
    );
    assert!(
        !dest.exists(),
        "dest file must not exist after failed create"
    );
}

#[test]
fn file_prepend_rejects_whitespace_only() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FilePrepend {
        path: "a.txt".into(),
        content: "\t".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(crate::exit::is_invalid_input(&err), "got: {err:#}");
    assert!(
        f.pending.is_empty(),
        "must not stage a write for whitespace-only prepend"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
}

/// append/prepend must not rewrite binary (NUL) files as text.
#[test]
fn file_append_rejects_binary_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.bin");
    std::fs::write(&file, b"hello\x00world").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FileAppend {
        path: "data.bin".into(),
        content: "evil\n".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_binary(&err),
        "expected BinaryError, got: {err:#}"
    );
    assert!(
        err.to_string().contains("binary file"),
        "message should name binary: {err}"
    );
    assert!(
        f.pending.is_empty(),
        "must not stage a write for a binary append"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"hello\x00world");
}

#[test]
fn file_prepend_rejects_binary_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.bin");
    std::fs::write(&file, b"hello\x00world").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FilePrepend {
        path: "data.bin".into(),
        content: "evil\n".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(crate::exit::is_binary(&err), "got: {err:#}");
    assert_eq!(std::fs::read(&file).unwrap(), b"hello\x00world");
}

/// Sole-path replace must refuse binary (NUL) files; MCP/tx used to rewrite them.
#[test]
fn replace_rejects_sole_binary_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.bin");
    std::fs::write(&file, b"hello\x00world").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::Replace {
        glob: None,
        path: Some("data.bin".into()),
        regex: false,
        old: "hello".into(),
        new_text: Some("HELLO".into()),
        nth: None,
        insert_before: None,
        insert_after: None,
        case_insensitive: false,
        multiline: false,
        if_exists: false,
        whole_line: false,
        range: None,
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
    let err = crate::tx::replace_op::execute_replace_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_binary(&err),
        "expected BinaryError, got: {err:#}"
    );
    assert!(
        err.to_string().contains("binary file"),
        "message should name binary: {err}"
    );
    assert!(
        f.pending.is_empty() && f.write_targets.is_empty(),
        "must not stage a write for binary replace"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"hello\x00world");
}

/// file.create through a path component that is a file must fail with
/// InvalidInputError before staging (no bare tempfile / false backup).
#[test]
fn file_create_rejects_parent_that_is_a_file() {
    let dir = TempDir::new().unwrap();
    let blocking = dir.path().join("notdir");
    std::fs::write(&blocking, "file\n").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());

    let op = Operation::FileCreate {
        path: "notdir/child.txt".into(),
        content: "x\n".into(),
        force: None,
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "expected InvalidInputError, got: {err:#}"
    );
    assert!(
        err.to_string().contains("not a directory"),
        "message should name the problem: {err}"
    );
    assert!(
        f.pending.is_empty(),
        "must not stage a write when parent is not a directory"
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

/// Case-only rename (readme.md → README.md) must stage `tx.renames` so commit
/// uses `fs::rename`. Without that, write-dest + delete-src removes the only
/// inode on case-insensitive filesystems (macOS APFS); agents hit this via
/// plan/MCP while CLI bypasses the engine (#1167).
#[test]
fn case_only_rename_records_tx_renames() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    std::fs::write(&file, "hello content\n").unwrap();

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let op = Operation::FileRename {
        from: "readme.md".into(),
        to: "README.md".into(),
        force: false,
    };
    execute_file_op(&op, &mut tx).expect("case-only rename should stage");
    drop(tx);
    assert!(
        !f.renames.is_empty(),
        "case-only rename must record tx.renames for fs::rename; got empty"
    );
    assert_eq!(
        f.renames[0].0.file_name().and_then(|n| n.to_str()),
        Some("readme.md")
    );
    assert_eq!(
        f.renames[0].1.file_name().and_then(|n| n.to_str()),
        Some("README.md")
    );
}

/// End-to-end: plan/tx case-only rename must leave content on disk (not delete).
#[test]
fn case_only_rename_plan_apply_preserves_content() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("readme.md");
    std::fs::write(&src, "hello content\n").unwrap();

    let plan = crate::plan::Plan {
        version: crate::plan::SCHEMA_VERSION,
        cwd: None,
        operations: vec![Operation::FileRename {
            from: "readme.md".into(),
            to: "README.md".into(),
            force: false,
        }],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        for_each: None,
        agent_preset: false,
        expected_sha256: None,
    };
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).expect("plan ok");
    assert!(
        report.ok,
        "case-only rename plan should succeed: {report:?}"
    );
    // Content must still exist under either spelling (case-insensitive FS).
    let content = std::fs::read_to_string(dir.path().join("README.md"))
        .or_else(|_| std::fs::read_to_string(&src))
        .expect("file must still exist after case-only rename");
    assert_eq!(content, "hello content\n");
}

/// On a case-sensitive volume, `Keep.txt` and `keep.txt` are two files.
/// Rename without force must be `already_exists`, not a case-only overwrite (#2473).
#[test]
fn case_sibling_rename_without_force_is_already_exists() {
    let dir = TempDir::new().unwrap();
    let keep = dir.path().join("Keep.txt");
    let lower = dir.path().join("keep.txt");
    std::fs::write(&keep, "KEEP\n").unwrap();
    if std::fs::write(&lower, "lower\n").is_err()
        || std::fs::read(&keep).ok().as_deref() != Some(b"KEEP\n".as_ref())
        || std::fs::read(&lower).ok().as_deref() != Some(b"lower\n".as_ref())
    {
        // Case-insensitive volume: the two names are one inode.
        let _ = std::fs::remove_file(&keep);
        let _ = std::fs::remove_file(&lower);
        return;
    }

    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let op = Operation::FileRename {
        from: "Keep.txt".into(),
        to: "keep.txt".into(),
        force: false,
    };
    let err = execute_file_op(&op, &mut tx).expect_err("must not overwrite case sibling");
    assert!(
        crate::exit::is_already_exists(&err),
        "expected already_exists, got: {err:#}"
    );
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "KEEP\n");
    assert_eq!(std::fs::read_to_string(&lower).unwrap(), "lower\n");
}

/// Binary delete + empty create must not pair as a rename (#2469).
#[test]
fn binary_delete_plus_empty_create_does_not_rename_bytes() {
    let dir = TempDir::new().unwrap();
    let png = dir.path().join("image.png");
    let notes = dir.path().join("notes.txt");
    let png_bytes: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\0\0\0\0\0";
    std::fs::write(&png, png_bytes).unwrap();

    let plan = crate::plan::Plan {
        version: crate::plan::SCHEMA_VERSION,
        cwd: None,
        operations: vec![
            Operation::FileDelete {
                path: "image.png".into(),
                if_exists: false,
            },
            Operation::FileCreate {
                path: "notes.txt".into(),
                content: String::new(),
                force: None,
            },
        ],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        for_each: None,
        agent_preset: false,
        expected_sha256: None,
    };
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).expect("plan ok");
    assert!(report.ok, "plan should succeed: {report:?}");
    assert!(!png.exists(), "binary source should be deleted");
    let dest = std::fs::read(&notes).expect("empty dest should exist");
    assert!(
        dest.is_empty(),
        "empty create must stay empty, not inherit binary bytes: {dest:?}"
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
            offset: None,
            limit: None,
            start_line: None,
            end_line: None,
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
        agent_preset: false,
        expected_sha256: None,
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
        agent_preset: false,
        expected_sha256: None,
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

#[test]
fn execute_and_collect_doc_update_typo_includes_did_you_mean() {
    // Strict doc.update no-match must keep NoMatchError and name the selector
    // plus a whole-key sibling hint (write path parity with doc get).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    std::fs::write(&file, r#"{"database":{"port":5432}}"#).unwrap();

    let plan = Plan {
        version: crate::plan::SCHEMA_VERSION,
        operations: vec![Operation::DocUpdate {
            path: "data.json".into(),
            selector: "databse".into(),
            value: serde_json::json!({"port": 1}),
        }],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        cwd: None,
        for_each: None,
        agent_preset: false,
        expected_sha256: None,
    };
    let ctx = crate::tx::context::EngineContext::from_global(
        &GlobalFlags::default(),
        dir.path().to_path_buf(),
    );
    match execute_and_collect(&plan, &ctx, true, false, None) {
        Ok(_) => panic!("expected NoMatch for typo selector"),
        Err(err) => {
            assert!(
                crate::exit::is_no_match(&err),
                "NoMatch must survive execute wrap: {err:#}"
            );
            let msg = err.to_string();
            assert!(
                msg.contains("databse"),
                "expected selector in doc.update miss, got: {msg}"
            );
            assert!(
                msg.contains("did you mean: database?"),
                "expected sibling-key hint, got: {msg}"
            );
        }
    }
}

/// rename a→b then create a: both paths must exist with correct content.
/// Stale tx.renames used to fs::rename a→b and skip writing resurrected a.
#[test]
fn rename_then_create_preserves_both_paths() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("lib.rs");
    std::fs::write(&a, "old body\n").unwrap();

    let plan = crate::plan::Plan {
        version: crate::plan::SCHEMA_VERSION,
        cwd: None,
        operations: vec![
            Operation::FileRename {
                from: "lib.rs".into(),
                to: "lib_old.rs".into(),
                force: false,
            },
            Operation::FileCreate {
                path: "lib.rs".into(),
                content: "new body\n".into(),
                force: Some(false),
            },
        ],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        for_each: None,
        agent_preset: false,
        expected_sha256: None,
    };
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).expect("plan ok");
    assert!(report.ok, "rename-then-create should succeed: {report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(),
        "new body\n",
        "resurrected source must keep create content"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lib_old.rs")).unwrap(),
        "old body\n",
        "rename dest must keep original content"
    );
}

/// rename a→b then delete b: neither path should remain.
#[test]
fn rename_then_delete_dest_removes_both() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "content\n").unwrap();

    let plan = crate::plan::Plan {
        version: crate::plan::SCHEMA_VERSION,
        cwd: None,
        operations: vec![
            Operation::FileRename {
                from: "a.txt".into(),
                to: "b.txt".into(),
                force: false,
            },
            Operation::FileDelete {
                path: "b.txt".into(),
                if_exists: false,
            },
        ],
        write_policy: None,
        strict: None,
        format: None,
        validate: None,
        verify: None,
        for_each: None,
        agent_preset: false,
        expected_sha256: None,
    };
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).expect("plan ok");
    assert!(
        report.ok,
        "rename-then-delete-dest should succeed: {report:?}"
    );
    assert!(!dir.path().join("a.txt").exists(), "source must be gone");
    assert!(
        !dir.path().join("b.txt").exists(),
        "dest delete must stick (stale rename must not re-create b)"
    );
}

#[cfg(unix)]
#[test]
fn append_dangling_symlink_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let op = Operation::FileAppend {
        path: "dangling.txt".into(),
        content: "x\n".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "dangling symlink append must be invalid_input not not_found, got: {err}"
    );
}

#[cfg(unix)]
#[test]
fn prepend_dangling_symlink_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let op = Operation::FilePrepend {
        path: "dangling.txt".into(),
        content: "x\n".into(),
    };
    let err = execute_file_op(&op, &mut tx).unwrap_err();
    assert!(
        crate::exit::is_invalid_input(&err),
        "dangling symlink prepend must be invalid_input not not_found, got: {err}"
    );
}

// ---- if_exists on file.delete / doc.set (#2231) ----

#[test]
fn file_delete_if_exists_missing_path_soft_skips() {
    let dir = TempDir::new().unwrap();
    let op = Operation::FileDelete {
        path: "missing.txt".into(),
        if_exists: true,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let count = execute_file_op(&op, &mut tx).expect("if_exists soft-skip");
    assert_eq!(count, 0);
    assert!(tx.deletions.is_empty(), "soft-skip must not stage a delete");
}

#[test]
fn file_delete_missing_path_without_if_exists_errors() {
    let dir = TempDir::new().unwrap();
    let op = Operation::FileDelete {
        path: "missing.txt".into(),
        if_exists: false,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let err = execute_file_op(&op, &mut tx).expect_err("missing without if_exists");
    assert!(
        crate::exit::is_io_not_found(&err),
        "expected NotFound, got: {err:#}"
    );
}

#[test]
fn path_present_for_if_exists_treats_deleted_as_absent() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("gone.txt");
    std::fs::write(&file, "x").unwrap();
    let mut f = TxStateFixture::new();
    f.deletions.insert(file.clone());
    let tx = f.state(dir.path());
    assert!(
        !tx.path_present_for_if_exists(&file),
        "deleted-in-tx must look absent"
    );
}

#[test]
fn path_present_for_if_exists_pending_without_disk() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("staged.txt");
    let mut f = TxStateFixture::new();
    f.pending.insert(file.clone(), (String::new(), "hi".into()));
    let tx = f.state(dir.path());
    assert!(
        tx.path_present_for_if_exists(&file),
        "pending create must look present"
    );
}

#[test]
fn file_delete_if_exists_still_deletes_when_present() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("gone.txt");
    std::fs::write(&file, "x\n").unwrap();
    let op = Operation::FileDelete {
        path: "gone.txt".into(),
        if_exists: true,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    execute_file_op(&op, &mut tx).expect("delete existing");
    assert!(
        tx.deletions.contains(&file),
        "existing file must still be staged for delete"
    );
}

#[test]
fn doc_set_if_exists_missing_file_soft_skips() {
    let dir = TempDir::new().unwrap();
    let op = Operation::DocSet {
        path: "missing.json".into(),
        selector: "k".into(),
        value: serde_json::json!(1),
        if_exists: true,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    execute_doc_op(&op, &mut tx).expect("if_exists soft-skip");
    assert!(
        tx.doc_cache.is_empty(),
        "missing file must not be parsed or written"
    );
}

#[test]
fn doc_set_if_exists_key_found_writes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.json");
    std::fs::write(&file, r#"{"k":1}"#).unwrap();
    let op = Operation::DocSet {
        path: "c.json".into(),
        selector: "k".into(),
        value: serde_json::json!(2),
        if_exists: true,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    execute_doc_op(&op, &mut tx).expect("set existing key");
    let cached = tx
        .doc_cache
        .get(&file)
        .expect("existing file should be cached");
    assert_eq!(cached.value["k"], serde_json::json!(2));
}

#[test]
fn doc_set_if_exists_key_missing_does_not_create() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.json");
    std::fs::write(&file, r#"{"k":1}"#).unwrap();
    let op = Operation::DocSet {
        path: "c.json".into(),
        selector: "missing".into(),
        value: serde_json::json!(2),
        if_exists: true,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    execute_doc_op(&op, &mut tx).expect("if_exists key miss");
    let cached = tx
        .doc_cache
        .get(&file)
        .expect("file is loaded before the key check");
    assert_eq!(
        cached.value,
        serde_json::json!({"k": 1}),
        "if_exists must not create a missing key"
    );
}

#[test]
fn doc_set_missing_file_without_if_exists_errors() {
    let dir = TempDir::new().unwrap();
    let op = Operation::DocSet {
        path: "missing.json".into(),
        selector: "k".into(),
        value: serde_json::json!(1),
        if_exists: false,
    };
    let mut f = TxStateFixture::new();
    let mut tx = f.state(dir.path());
    let err = execute_doc_op(&op, &mut tx).expect_err("missing without if_exists");
    assert!(
        crate::exit::is_io_not_found(&err),
        "expected NotFound, got: {err:#}"
    );
}

fn apply_ops(dir: &std::path::Path, ops: serde_json::Value) -> crate::tx::TxOutput {
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": ops
    }))
    .expect("plan json");
    crate::tx::execute_plan_direct(plan, dir, None).expect("plan returns output")
}

/// Delete dest then rename a binary source onto it. Commit must `fs::rename`
/// the bytes, not write the soft-empty snapshot (#2497).
#[test]
fn delete_then_rename_binary_source_keeps_bytes() {
    let dir = TempDir::new().unwrap();
    let src_bytes: &[u8] = b"\x00\x01\x02BIN";
    std::fs::write(dir.path().join("a.bin"), src_bytes).unwrap();
    std::fs::write(dir.path().join("b.bin"), b"old").unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.delete", "path": "b.bin"},
            {"op": "file.rename", "from": "a.bin", "to": "b.bin"}
        ]),
    );
    assert!(
        report.ok,
        "delete-then-rename binary should succeed: {report:?}"
    );
    assert!(
        !dir.path().join("a.bin").exists(),
        "source must be gone after rename"
    );
    let dest = std::fs::read(dir.path().join("b.bin")).expect("dest must exist");
    assert_eq!(
        dest, src_bytes,
        "dest must keep the original binary bytes, not a 0-byte rewrite"
    );
}

/// Same as binary: a symlink source must stay a symlink at dest (#2497).
#[cfg(unix)]
#[test]
fn delete_then_rename_symlink_source_keeps_link() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target.txt");
    std::fs::write(&target, "pointee\n").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("a.link")).unwrap();
    std::fs::write(dir.path().join("b.link"), b"old").unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.delete", "path": "b.link"},
            {"op": "file.rename", "from": "a.link", "to": "b.link"}
        ]),
    );
    assert!(
        report.ok,
        "delete-then-rename symlink should succeed: {report:?}"
    );
    assert!(
        !dir.path().join("a.link").exists(),
        "source link must be gone"
    );
    let dest = dir.path().join("b.link");
    let meta = std::fs::symlink_metadata(&dest).expect("dest must exist");
    assert!(
        meta.file_type().is_symlink(),
        "dest must remain a symlink, not a 0-byte regular file"
    );
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "pointee\n");
}

/// Text delete-then-rename must move the inode (hardlinks stay with dest) (#2497).
#[cfg(unix)]
#[test]
fn delete_then_rename_text_source_preserves_hardlink() {
    use std::os::unix::fs::MetadataExt;
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let sibling = dir.path().join("a_link.txt");
    std::fs::write(&a, "hello text\n").unwrap();
    std::fs::hard_link(&a, &sibling).unwrap();
    let before_ino = std::fs::metadata(&a).unwrap().ino();
    std::fs::write(dir.path().join("b.txt"), "old\n").unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.delete", "path": "b.txt"},
            {"op": "file.rename", "from": "a.txt", "to": "b.txt"}
        ]),
    );
    assert!(
        report.ok,
        "delete-then-rename text should succeed: {report:?}"
    );
    let dest = dir.path().join("b.txt");
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello text\n");
    assert_eq!(
        std::fs::metadata(&dest).unwrap().ino(),
        before_ino,
        "dest must keep the source inode so hardlink siblings stay shared"
    );
    assert_eq!(std::fs::read_to_string(&sibling).unwrap(), "hello text\n");
}

/// Rename a binary then create the source path. Dest must keep the original
/// bytes; dropping the rename pair used to commit dest as empty (#2498).
#[test]
fn rename_then_create_binary_source_keeps_dest_bytes() {
    let dir = TempDir::new().unwrap();
    let src_bytes: &[u8] = b"\x00\x01\x02BIN";
    std::fs::write(dir.path().join("a.bin"), src_bytes).unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.bin", "to": "b.bin"},
            {"op": "file.create", "path": "a.bin", "content": "new a\n"}
        ]),
    );
    assert!(
        report.ok,
        "rename-then-create binary should succeed: {report:?}"
    );
    let dest = std::fs::read(dir.path().join("b.bin")).expect("dest must exist");
    assert_eq!(
        dest, src_bytes,
        "dest must keep the original binary bytes after source resurrection"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.bin")).unwrap(),
        "new a\n",
        "resurrected source must hold create content"
    );
}

/// Append on a renamed binary dest must refuse (not overwrite with text) (#2499).
#[test]
fn append_on_renamed_binary_dest_is_binary() {
    let dir = TempDir::new().unwrap();
    let src_bytes: &[u8] = b"\x00\x01\x02BIN";
    std::fs::write(dir.path().join("a.bin"), src_bytes).unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.bin", "to": "b.bin"},
            {"op": "file.append", "path": "b.bin", "content": "hello\n"}
        ]),
    );
    assert!(!report.ok, "append on renamed binary must fail: {report:?}");
    assert_eq!(
        report.error_kind.as_deref(),
        Some("binary"),
        "expected binary error_kind, got {report:?}"
    );
    assert!(
        dir.path().join("a.bin").exists(),
        "failed plan must not apply the rename"
    );
    assert_eq!(std::fs::read(dir.path().join("a.bin")).unwrap(), src_bytes);
}

/// Replace on a renamed binary dest must refuse (#2499).
#[test]
fn replace_on_renamed_binary_dest_is_binary() {
    let dir = TempDir::new().unwrap();
    let src_bytes: &[u8] = b"\x00\x01\x02BIN";
    std::fs::write(dir.path().join("a.bin"), src_bytes).unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.bin", "to": "b.bin"},
            {"op": "replace", "path": "b.bin", "old": "^", "new": "hello\n", "regex": true}
        ]),
    );
    assert!(
        !report.ok,
        "replace on renamed binary must fail: {report:?}"
    );
    assert_eq!(
        report.error_kind.as_deref(),
        Some("binary"),
        "expected binary error_kind, got {report:?}"
    );
    assert_eq!(std::fs::read(dir.path().join("a.bin")).unwrap(), src_bytes);
}

/// Patch on a renamed binary dest must refuse (#2499).
#[test]
fn patch_on_renamed_binary_dest_is_binary() {
    let dir = TempDir::new().unwrap();
    let src_bytes: &[u8] = b"\x00\x01\x02BIN";
    std::fs::write(dir.path().join("a.bin"), src_bytes).unwrap();
    let diff = "--- a/b.bin\n+++ b/b.bin\n@@ -0,0 +1 @@\n+hello\n";

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.bin", "to": "b.bin"},
            {"op": "patch.apply", "diff": diff}
        ]),
    );
    assert!(!report.ok, "patch on renamed binary must fail: {report:?}");
    assert_eq!(
        report.error_kind.as_deref(),
        Some("binary"),
        "expected binary error_kind, got {report:?}"
    );
    assert_eq!(std::fs::read(dir.path().join("a.bin")).unwrap(), src_bytes);
}

/// Two-link chain (a->b->c) then delete c must not leave b on disk (#2500).
#[test]
fn delete_chained_rename_dest_two_link_removes_all() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "AAA\n").unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.txt", "to": "b.txt"},
            {"op": "file.rename", "from": "b.txt", "to": "c.txt"},
            {"op": "file.delete", "path": "c.txt"}
        ]),
    );
    assert!(report.ok, "two-link delete should succeed: {report:?}");
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            !dir.path().join(name).exists(),
            "{name} must not remain after deleting the chain dest"
        );
    }
}

/// Three-link chain a->b->c->d then delete d must not leave intermediates (#2500).
#[test]
fn delete_chained_rename_dest_three_link_removes_all() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "AAA\n").unwrap();

    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "file.rename", "from": "a.txt", "to": "b.txt"},
            {"op": "file.rename", "from": "b.txt", "to": "c.txt"},
            {"op": "file.rename", "from": "c.txt", "to": "d.txt"},
            {"op": "file.delete", "path": "d.txt"}
        ]),
    );
    assert!(report.ok, "three-link delete should succeed: {report:?}");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        assert!(
            !dir.path().join(name).exists(),
            "{name} must not remain after deleting the chain dest"
        );
    }
}

#[test]
fn read_returns_whole_file_sha256_and_offset_window() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\n").unwrap();
    let report = apply_ops(
        dir.path(),
        serde_json::json!([
            {"op": "read", "path": "f.txt", "offset": 2, "limit": 2}
        ]),
    );
    assert!(report.ok, "{report:?}");
    let read = report.reads.first().expect("read");
    assert_eq!(read.content, "b\nc");
    assert_eq!(read.start_line, 2);
    assert_eq!(read.sha256, crate::ops::read::sha256_hex(b"a\nb\nc\nd\n"));
}

#[test]
fn read_past_eof_is_no_matches() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [{"op": "read", "path": "f.txt", "offset": 99, "limit": 1}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(
        report.error_kind.as_deref(),
        Some("no_matches"),
        "{report:?}"
    );
    assert!(!report.ok, "{report:?}");
}

#[test]
fn expected_sha256_mismatch_is_stale_and_does_not_write() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "expected_sha256": {"f.txt": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "operations": [{"op": "replace", "path": "f.txt", "old": "hello", "new": "bye"}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(report.error_kind.as_deref(), Some("stale"), "{report:?}");
    assert!(!report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn expected_sha256_glob_mismatch_is_stale_and_does_not_write() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "hello\n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "expected_sha256": {"a.txt": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "operations": [{"op": "replace", "glob": "*.txt", "old": "hello", "new": "bye"}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(report.error_kind.as_deref(), Some("stale"), "{report:?}");
    assert!(!report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn expected_sha256_tidy_dir_mismatch_is_stale_and_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("sub").join("a.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "hello \n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "expected_sha256": {"sub/a.txt": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "operations": [{"op": "tidy.fix", "path": "sub"}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(report.error_kind.as_deref(), Some("stale"), "{report:?}");
    assert!(!report.applied, "{report:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello \n");
}

#[test]
fn tidy_fix_directory_includes_hidden_files() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(sub.join(".git")).unwrap();
    std::fs::write(sub.join(".gitignore"), "foo").unwrap();
    std::fs::write(sub.join("keep.txt"), "ok\n").unwrap();
    std::fs::write(sub.join(".git").join("config"), "bare").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [{"op": "tidy.fix", "path": "sub"}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(sub.join(".gitignore")).unwrap(),
        "foo\n"
    );
    assert_eq!(
        std::fs::read_to_string(sub.join(".git").join("config")).unwrap(),
        "bare"
    );
}

#[test]
fn expected_sha256_second_edit_uses_pre_plan_bytes() {
    let dir = TempDir::new().unwrap();
    let body = "hello\nworld\n";
    std::fs::write(dir.path().join("f.txt"), body).unwrap();
    let hash = crate::ops::read::sha256_hex(body.as_bytes());
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "expected_sha256": {"f.txt": hash},
        "operations": [
            {"op": "replace", "path": "f.txt", "old": "hello", "new": "HELLO"},
            {"op": "replace", "path": "f.txt", "old": "world", "new": "WORLD"}
        ]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "HELLO\nWORLD\n"
    );
}

#[test]
fn expected_sha256_for_each_two_edits_uses_pre_plan_bytes() {
    let dir = TempDir::new().unwrap();
    let body = "hello\nworld\n";
    std::fs::create_dir(dir.path().join("fe")).unwrap();
    std::fs::write(dir.path().join("fe/a.txt"), body).unwrap();
    let hash = crate::ops::read::sha256_hex(body.as_bytes());
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "expected_sha256": {"fe/a.txt": hash},
        "for_each": {"glob": "fe/*.txt"},
        "operations": [
            {"op": "replace", "path": "{path}", "old": "hello", "new": "HELLO"},
            {"op": "replace", "path": "{path}", "old": "world", "new": "WORLD"}
        ]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("fe/a.txt")).unwrap(),
        "HELLO\nWORLD\n"
    );
}

#[test]
fn agent_preset_rejects_a_second_match() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("f.txt"), "foo\nfoo\n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "agent_preset": true,
        "operations": [{"op": "replace", "path": "f.txt", "old": "foo", "new": "bar"}]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(
        report.error_kind.as_deref(),
        Some("ambiguous"),
        "{report:?}"
    );
    assert!(!report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "foo\nfoo\n"
    );
}

#[test]
fn agent_preset_command_position_replaces_one_token() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("sh.txt"), "pip install\n").unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "agent_preset": true,
        "operations": [{
            "op": "replace",
            "path": "sh.txt",
            "old": "pip",
            "new": "uv",
            "command_position": true
        }]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sh.txt")).unwrap(),
        "uv install\n"
    );
}

#[test]
fn command_position_explicit_fuzzy_stays_invalid_input() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("sh.txt"), "pip install\n").unwrap();
    for preset in [false, true] {
        let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
            "version": 1,
            "agent_preset": preset,
            "operations": [{
                "op": "replace",
                "path": "sh.txt",
                "old": "pip",
                "new": "uv",
                "command_position": true,
                "fuzzy": true
            }]
        }))
        .unwrap();
        let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
        assert_eq!(
            report.error_kind.as_deref(),
            Some("invalid_input"),
            "preset={preset}: {report:?}"
        );
        assert!(
            report
                .error
                .as_deref()
                .unwrap_or("")
                .contains("command_position cannot be combined"),
            "preset={preset}: {report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sh.txt")).unwrap(),
            "pip install\n"
        );
    }
}

const NOTEBOOK: &str = r#"{
 "cells": [
  {
   "cell_type": "code",
   "id": "load",
   "metadata": {},
   "outputs": [],
   "source": ["import pandas\n"]
  }
 ],
 "nbformat": 4,
 "nbformat_minor": 5
}
"#;

#[test]
fn notebook_edit_replaces_cell_source() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.ipynb"), NOTEBOOK).unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "notebook.edit",
            "path": "a.ipynb",
            "cell_id": "load",
            "source": "import pandas as pd\n"
        }]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert!(report.applied, "{report:?}");
    let text = std::fs::read_to_string(dir.path().join("a.ipynb")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["cells"][0]["source"][0], "import pandas as pd\n");
    assert_eq!(v["nbformat"], 4);
}

#[test]
fn notebook_edit_missing_cell_is_no_matches() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.ipynb"), NOTEBOOK).unwrap();
    let plan: crate::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "notebook.edit",
            "path": "a.ipynb",
            "id": "missing",
            "source": "x\n"
        }]
    }))
    .unwrap();
    let report = crate::tx::execute_plan_direct(plan, dir.path(), None).unwrap();
    assert_eq!(
        report.error_kind.as_deref(),
        Some("no_matches"),
        "{report:?}"
    );
    assert!(!report.applied, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.ipynb")).unwrap(),
        NOTEBOOK
    );
}
