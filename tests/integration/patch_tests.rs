use super::*;

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
        .arg(&patch_file)
        .assert()
        .code(2);

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
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

/// Relative patch path is resolved under --cwd (parity with `tx` / `batch`).
#[test]
fn test_patch_relative_file_respects_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "line1\nold line\nline3\n").unwrap();
    fs::write(
        dir.path().join("change.patch"),
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg("change.patch")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("test.txt")).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

/// Under --contain, the patch *file path* must stay in the workspace (meta-input).
#[test]
fn test_patch_contain_rejects_patch_file_parent_escape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "line1\nold line\nline3\n").unwrap();
    let outside = dir.path().parent().unwrap().join(format!(
        "patchloom-patch-meta-{}.patch",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    fs::write(
        &outside,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "--contain",
            "patch",
            "apply",
            &format!("../{}", outside.file_name().unwrap().to_string_lossy()),
            "--apply",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert_eq!(
        fs::read_to_string(dir.path().join("test.txt")).unwrap(),
        "line1\nold line\nline3\n"
    );
    let _ = fs::remove_file(&outside);
}

/// Absolute patch file under --cwd is allowed with --contain (AllowIfContained).
#[test]
fn test_patch_contain_allows_absolute_patch_path_inside_workspace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "line1\nold line\nline3\n").unwrap();
    let patch = dir.path().join("change.patch");
    fs::write(
        &patch,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .args(["--contain", "patch", "apply"])
        .arg(patch.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("test.txt")).unwrap(),
        "line1\nnew line\nline3\n"
    );
}

#[test]
fn test_patch_apply_json_parse_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "not a patch\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 4, "patch: parse error:");
}

/// Missing patch file must be not_found (exit 1), not parse_error (exit 4).
/// Agents branch on error_kind; parse_error implies malformed content.
#[test]
fn test_patch_apply_missing_file_json_not_found() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("missing.diff");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&missing)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "missing patch file exit: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "not_found", "{v}");
    assert!(
        v["error"]
            .as_str()
            .is_some_and(|e| e.contains("failed to read")),
        "{v}"
    );
}

/// Blank patch path is invalid_input (exit 1), not parse_error (exit 4).
/// Parity with create/doc/read empty-path peels (#2152 family).
#[test]
fn test_patch_apply_blank_path_json_invalid_input() {
    let dir = TempDir::new().unwrap();
    for path in ["", "   ", "\t"] {
        let output = Command::cargo_bin("patchloom")
            .unwrap()
            .arg("--json")
            .arg("--cwd")
            .arg(dir.path())
            .arg("patch")
            .arg("apply")
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "blank path {path:?} exit: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(v["ok"], false, "path={path:?} {v}");
        assert_eq!(
            v["error_kind"], "invalid_input",
            "path={path:?} must not be parse_error: {v}"
        );
        let err = v["error"].as_str().unwrap_or("");
        assert!(
            err.contains("path must not be empty"),
            "path={path:?} message: {v}"
        );
    }
}

#[test]
fn test_patch_apply_json_stale_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 5, "patch apply: test.txt -- STALE:");
}

/// Deletion patches must fail closed when file content no longer matches the
/// hunk (check already reported stale; apply must not delete).
#[test]
fn test_patch_apply_stale_deletion_does_not_delete_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("t.txt");
    fs::write(&file, "changed\n").unwrap();
    let patch_file = dir.path().join("del.patch");
    fs::write(
        &patch_file,
        "--- a/t.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-line1\n-line2\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(5); // AMBIGUOUS / stale
    assert!(file.exists(), "stale deletion must not remove the file");
    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
}

#[test]
fn test_patch_check_exits_2_when_would_change() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    // Applicable patch that mutates content is CHANGES_DETECTED (2), not SUCCESS.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(2)
        .stdout(predicates::str::contains("would change"));
}

#[test]
fn test_patch_check_pure_rename_exits_2() {
    // 100% similarity rename: content unchanged but path moves.
    // Check must report would_change (exit 2), not success 0.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.rs"), "fn main() {}\n").unwrap();

    let patch_file = dir.path().join("rename.patch");
    fs::write(
        &patch_file,
        "diff --git a/old.rs b/new.rs\n\
         similarity index 100%\n\
         rename from old.rs\n\
         rename to new.rs\n",
    )
    .unwrap();

    // Check JSON: one would_change rename row with from/to (#2106).
    let check_out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .output()
        .unwrap();
    assert_eq!(check_out.status.code(), Some(2));
    let check_json: serde_json::Value = serde_json::from_slice(&check_out.stdout).unwrap();
    let check_files = check_json["files"].as_array().expect("check files");
    assert_eq!(
        check_files.len(),
        1,
        "check pure rename one row: {check_json}"
    );
    assert_eq!(check_files[0]["status"], "would_change");
    assert_eq!(check_files[0]["from"], "old.rs");
    assert_eq!(check_files[0]["to"], "new.rs");
    assert_eq!(check_files[0]["action"], "renamed");

    // Apply JSON: one renamed row with from/to (not two applied files) (#2106).
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "{json}");
    assert_eq!(json["applied"], true, "{json}");
    let files = json["files"].as_array().expect("files array");
    assert_eq!(
        files.len(),
        1,
        "pure rename must be one JSON row, got: {json}"
    );
    assert_eq!(files[0]["path"], "new.rs");
    assert_eq!(files[0]["from"], "old.rs");
    assert_eq!(files[0]["to"], "new.rs");
    assert_eq!(files[0]["action"], "renamed");
    assert_eq!(files[0]["status"], "applied");

    assert!(!dir.path().join("old.rs").exists(), "source removed");
    assert_eq!(
        fs::read_to_string(dir.path().join("new.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn test_patch_rename_refuses_existing_dest() {
    // file.rename refuses without --force; patch rename must match (not clobber).
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.rs"), "source\n").unwrap();
    fs::write(dir.path().join("new.rs"), "dest-existing\n").unwrap();

    let patch_file = dir.path().join("rename.patch");
    fs::write(
        &patch_file,
        "diff --git a/old.rs b/new.rs\n\
         similarity index 100%\n\
         rename from old.rs\n\
         rename to new.rs\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(1)
        .stdout(predicates::str::contains("already_exists"));

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(1)
        .stdout(predicates::str::contains("already_exists"));

    // Tree unchanged.
    assert_eq!(
        fs::read_to_string(dir.path().join("old.rs")).unwrap(),
        "source\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new.rs")).unwrap(),
        "dest-existing\n"
    );
}

fn patch_check_json(dir: &TempDir, patch: &std::path::Path) -> (i32, serde_json::Value) {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("check")
        .arg(patch)
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(-1);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout not JSON (exit {code}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (code, json)
}

#[test]
fn test_patch_check_copy_and_empty_create_dest_honesty() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("src.rs"), "keep\n").unwrap();
    fs::write(dir.path().join("dst.rs"), "existing\n").unwrap();
    fs::write(dir.path().join(".empty"), "present\n").unwrap();

    let copy_patch = dir.path().join("copy.patch");
    fs::write(
        &copy_patch,
        "diff --git a/src.rs b/dst.rs\n\
         similarity index 100%\n\
         copy from src.rs\n\
         copy to dst.rs\n",
    )
    .unwrap();
    let (code, json) = patch_check_json(&dir, &copy_patch);
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["error_kind"], "already_exists", "{json}");
    assert_eq!(json["applied"], false);
    assert_eq!(json["files"][0]["status"], "already_exists");
    assert_eq!(json["files"][0]["action"], "copied");
    assert_eq!(
        fs::read_to_string(dir.path().join("dst.rs")).unwrap(),
        "existing\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("src.rs")).unwrap(),
        "keep\n"
    );

    let create_exists = dir.path().join("create-exists.patch");
    fs::write(
        &create_exists,
        "diff --git a/.empty b/.empty\n\
         new file mode 100644\n\
         index 0000000..e69de29\n",
    )
    .unwrap();
    let (code, json) = patch_check_json(&dir, &create_exists);
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["error_kind"], "already_exists", "{json}");
    assert_eq!(json["files"][0]["status"], "already_exists");
    assert_eq!(
        fs::read_to_string(dir.path().join(".empty")).unwrap(),
        "present\n"
    );

    let create_new = dir.path().join("create-new.patch");
    fs::write(
        &create_new,
        "diff --git a/.brand-new b/.brand-new\n\
         new file mode 100644\n\
         index 0000000..e69de29\n",
    )
    .unwrap();
    let (code, json) = patch_check_json(&dir, &create_new);
    assert_eq!(code, 2, "{json}");
    assert_eq!(json["files"][0]["status"], "would_change", "{json}");
    assert_eq!(json["applied"], false);
    assert!(!dir.path().join(".brand-new").exists());

    let binary_meta = dir.path().join("bin.patch");
    fs::write(
        &binary_meta,
        "diff --git a/ok.txt b/secret.bin\n\
         GIT binary patch\n\
         literal 4\n\
         xxx\n",
    )
    .unwrap();
    let (code, json) = patch_check_json(&dir, &binary_meta);
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["error_kind"], "invalid_input", "{json}");
    assert_eq!(json["files"][0]["status"], "error");
    assert!(!dir.path().join("secret.bin").exists());
}

#[test]
fn test_patch_apply_empty_create_lists_dest_in_json() {
    // fixrealloop R9: apply wrote a 0-byte dest with applied:true but files[].
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("create-new.patch");
    fs::write(
        &patch_file,
        "diff --git a/.brand-new b/.brand-new\n\
         new file mode 100644\n\
         index 0000000..e69de29\n",
    )
    .unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();
    let code = output.status.code().unwrap_or(-1);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout not JSON (exit {code}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["applied"], true, "{json}");
    assert_eq!(
        json["files"][0]["path"], ".brand-new",
        "empty-create apply must list dest: {json}"
    );
    assert_eq!(json["files"][0]["status"], "applied", "{json}");
    assert_eq!(
        fs::read(dir.path().join(".brand-new")).unwrap(),
        b"",
        "dest must be empty"
    );
}

#[test]
fn test_patch_check_pure_rename_c_quoted_paths() {
    // Git C-quotes paths with spaces; check/apply must unquote and move.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old name.rs"), "fn main() {}\n").unwrap();

    let patch_file = dir.path().join("rename.patch");
    fs::write(
        &patch_file,
        "diff --git \"a/old name.rs\" \"b/new name.rs\"\n\
         similarity index 100%\n\
         rename from \"old name.rs\"\n\
         rename to \"new name.rs\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(2);

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!dir.path().join("old name.rs").exists(), "source removed");
    assert_eq!(
        fs::read_to_string(dir.path().join("new name.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn test_patch_apply_check_counts_only_changed_files() {
    let dir = TempDir::new().unwrap();
    // File with a real change.
    let changed = dir.path().join("changed.txt");
    fs::write(&changed, "line1\nold line\nline3\n").unwrap();
    // File where the patch is a no-op (replace "same" with "same").
    let same = dir.path().join("same.txt");
    fs::write(&same, "same\n").unwrap();

    let patch_file = dir.path().join("multi.patch");
    fs::write(
        &patch_file,
        "--- a/changed.txt\n\
         +++ b/changed.txt\n\
         @@ -1,3 +1,3 @@\n\
         \x20line1\n\
         -old line\n\
         +new line\n\
         \x20line3\n\
         --- a/same.txt\n\
         +++ b/same.txt\n\
         @@ -1 +1 @@\n\
         -same\n\
         +same\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(2) // CHANGES_DETECTED
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 file(s) would change"),
        "should count only files with actual changes, got: {stdout}"
    );
}

#[test]
fn test_patch_check_json_output_would_change() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--json")
        .output()
        .unwrap();

    // Align with patch apply preview: applicable patch that mutates content
    // is would_change + exit 2, not "clean"/0 (agents misread that as done).
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["files"].is_array());
    assert_eq!(json["files"][0]["status"], "would_change");
}

#[test]
fn test_patch_apply_jsonl_parse_error_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("bad.patch");
    fs::write(&patch_file, "not a patch\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .output()
        .unwrap();

    assert_patch_apply_error_object(&output, 4, "patch: parse error:");
}

#[test]
fn test_patch_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "jsonl check should report changes_detected"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    // Per-file row + type:summary trailer (applied:false for check).
    assert!(
        lines.len() >= 2,
        "file row + summary expected, got {lines:?}"
    );
    let file_row = lines
        .iter()
        .find(|v| v.get("path").is_some())
        .expect("file row");
    assert_eq!(file_row["path"], "test.txt");
    assert_eq!(file_row["status"], "would_change");
    let summary = lines
        .iter()
        .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("summary"))
        .expect("summary trailer");
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["applied"], false);
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
        .arg(&patch_file)
        .assert()
        .code(5);
}

/// fixrealloop: agents branch on error_kind; stale check must not leave it null.
#[test]
fn test_patch_check_json_stale_sets_error_kind_ambiguous() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "check"])
        .arg(&patch_file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5), "stale → exit 5");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error_kind"], "ambiguous", "{v}");
    assert_eq!(v["applied"], false, "{v}");
    assert_eq!(v["files"][0]["status"], "stale", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("stale"),
        "error summary should mention stale: {v}"
    );
}

#[test]
fn test_patch_check_stale_human_readable_output() {
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
        .arg(&patch_file)
        .assert()
        .code(5)
        .stderr(predicate::str::contains("STALE"));
}

#[test]
fn test_patch_merge_check_exits_8_on_conflict() {
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
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(8);
}

#[test]
fn test_patch_merge_check_allow_conflicts_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();
    let patch_file = dir.path().join("stale.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();
    // With --allow-conflicts, --check should report that changes would be
    // applied (CHANGES_DETECTED = exit 2), not CONFLICTS (exit 8).
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .arg("--allow-conflicts")
        .assert()
        .code(2); // CHANGES_DETECTED
}

#[test]
fn test_patch_merge_allow_conflicts_writes_markers() {
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
        .arg("merge")
        .arg(&patch_file)
        .arg("--apply")
        .arg("--allow-conflicts")
        .assert()
        .code(0);
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("<<<<<<< patchloom (ours)"),
        "should have conflict markers: {content}"
    );
    assert!(
        content.contains("======="),
        "should have separator: {content}"
    );
    assert!(
        content.contains(">>>>>>> patch (theirs)"),
        "should have theirs marker: {content}"
    );
}

#[test]
fn test_patch_merge_apply_writes_clean_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\nextra\n").unwrap();
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
        .arg("merge")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .code(0);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
}

#[test]
fn test_patch_apply_on_stale_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\nextra\n").unwrap();
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
        .arg(&patch_file)
        .arg("--on-stale")
        .arg("merge")
        .arg("--apply")
        .assert()
        .code(0);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
}

#[test]
fn test_patch_check_exits_1_on_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    // invalid_input / exit 1 (shared table), not ambiguous/exit 5.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "patch check: test.txt -- READ ERROR: target is not a file",
        ));
}

#[test]
fn test_patch_check_json_reports_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "invalid_input");
    assert_eq!(json["files"][0]["status"], "error");
    assert!(
        json["files"][0]["error"]
            .as_str()
            .unwrap()
            .contains("not a file"),
        "error={}",
        json["files"][0]["error"]
    );
}

#[test]
fn test_patch_check_jsonl_reports_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    // Per-file row + type:summary trailer with ok/error_kind.
    assert!(
        lines.len() >= 2,
        "file row + summary trailer expected, got {lines:?}"
    );
    let file_row = lines
        .iter()
        .find(|v| v.get("path").is_some())
        .expect("file row");
    assert_eq!(file_row["path"], "test.txt");
    assert_eq!(file_row["status"], "error");
    assert!(
        file_row["error"].as_str().unwrap().contains("not a file"),
        "error={}",
        file_row["error"]
    );
    let summary = lines
        .iter()
        .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("summary"))
        .expect("summary trailer");
    assert_eq!(summary["ok"], false);
    assert_eq!(summary["error_kind"], "invalid_input");
    assert_eq!(summary["applied"], false);
}

/// Missing patch target → not_found + exit 1 (not ambiguous/exit 5).
#[test]
fn test_patch_check_json_missing_target_not_found_exit_1() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/missing.txt\n+++ b/missing.txt\n@@ -0,0 +1 @@\n+new line\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("check")
        .arg(&patch_file)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(json["error_kind"], "not_found", "{json}");
    assert_eq!(json["files"][0]["status"], "missing");
}

/// Apply when target file is gone: not_found, not STALE/ambiguous.
#[test]
fn test_patch_apply_missing_target_json_not_found() {
    let dir = TempDir::new().unwrap();
    let patch_file = dir.path().join("change.patch");
    // Existing-file hunk (not new-file create): context requires a real target.
    fs::write(
        &patch_file,
        "--- a/gone.txt\n+++ b/gone.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--json")
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(json["error_kind"], "not_found", "{json}");
    assert_eq!(json["applied"], false, "{json}");
}

#[test]
fn test_patch_apply_check_quiet_suppresses_output() {
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
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--check")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nold line\nline3\n"
    );
}

#[test]
fn test_patch_help_examples_use_subcommand() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["patch", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("patchloom patch apply") || stdout.contains("patchloom patch check"),
        "patch help examples must use a subcommand (apply/check), got:\n{stdout}"
    );
    assert!(
        !stdout.contains("patchloom patch changes.patch"),
        "patch help examples must not show bare file arguments without a subcommand"
    );
}

// ---------------------------------------------------------------------------
// Parse smoke tests: every subcommand name is recognized by clap
// ---------------------------------------------------------------------------

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
        .arg(&patch_file)
        .assert()
        .code(4)
        .stderr(predicate::str::contains("parse error"));
}

// Regression: merge-check exit code was SUCCESS when both a conflict (resolved
// via --allow-conflicts) and a hard error (hunk not locatable) occurred in a
// multi-file patch.  The error was masked because `has_conflicts && allow_conflicts`
// short-circuited before checking for errors.
#[test]
fn test_patch_merge_check_allow_conflicts_still_fails_on_error() {
    let dir = TempDir::new().unwrap();
    // File A: will produce a conflict (content diverged from patch context).
    let file_a = dir.path().join("a.txt");
    fs::write(&file_a, "line1\ncompletely different\nline3\n").unwrap();
    // File B: does not exist, so merge will fail entirely (error status).

    let patch_file = dir.path().join("multi.patch");
    fs::write(
        &patch_file,
        "\
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
--- a/b.txt
+++ b/b.txt
@@ -1,3 +1,3 @@
 alpha
-beta
+gamma
 delta
",
    )
    .unwrap();
    // With --allow-conflicts, the conflict on a.txt is acceptable, but the
    // error on b.txt still fails. Kind is invalid_input (status error), exit 1
    // (shared agent table), not conflicts/8 and not always ambiguous/5.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("merge")
        .arg(&patch_file)
        .arg("--check")
        .arg("--allow-conflicts")
        .assert()
        .code(1);
}

// ---------------------------------------------------------------------------
// --jsonl + count/files-with-matches (bug fix: count_only + jsonl)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// patch apply with bare "-" as stdin (MPI 2026-07-07 cycle 1 QA / #1423)
// ---------------------------------------------------------------------------

#[test]
fn test_patch_apply_dash_reads_stdin() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "line1\nold line\nline3\n").unwrap();
    let patch =
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["patch", "apply", "-", "--apply"])
        .write_stdin(patch)
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("test.txt")).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

/// Preview/check patch JSON must set applied:false and would_change status (#1812).
#[test]
fn test_patch_preview_json_applied_false() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-hello\n+world\n",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "apply", "p.patch"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["applied"], false, "preview must not look like apply: {v}");
    let files = v["files"].as_array().expect("files");
    assert_eq!(files[0]["status"], "would_change", "{v}");
    assert_eq!(
        fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_patch_apply_json_applied_true() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-hello\n+world\n",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "apply", "p.patch", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["files"][0]["status"], "applied", "{v}");
    assert_eq!(
        fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "world\n"
    );
}

/// Patch *file* (diff input) that is binary peels `error_kind: binary` (#1896 / #1963).
#[test]
fn test_patch_diff_file_binary_is_binary_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("t.txt"), "hello\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        b"--- a/t.txt\n+++ b/t.txt\n\x00",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "check", "p.patch"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "binary", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("binary") || err.contains("UTF-8") || err.contains("utf"),
        "{v}"
    );
}

/// Patch check on invalid UTF-8 *target* peels `invalid_encoding` (#1896 / #1963).
#[test]
fn test_patch_check_invalid_utf8_target_is_invalid_encoding() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.txt"), b"line one\xff\nline two\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        "--- a/bad.txt\n+++ b/bad.txt\n@@ -1 +1 @@\n-line one\n+line ONE\n",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "check", "p.patch"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "invalid_encoding", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(err.contains("UTF-8") || err.contains("utf"), "{v}");
}

/// Patch check on binary *target* peels `error_kind: binary` (#1896 / #1963).
#[test]
fn test_patch_check_binary_target_is_binary_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bin_line.bin"), b"line one\x00line two\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        "--- a/bin_line.bin\n+++ b/bin_line.bin\n@@ -1 +1 @@\n-line one\n+line ONE\n",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "check", "p.patch"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "binary", "{v}");
    assert!(v["error"].as_str().unwrap_or("").contains("binary"), "{v}");
    assert_eq!(
        fs::read(dir.path().join("bin_line.bin")).unwrap(),
        b"line one\x00line two\n"
    );
}

/// Binary target must refuse with `error_kind: binary` (exit 1), not STALE/ambiguous.
/// Engine already load_text_strict; CLI error mapping was wrong (fixrealloop).
#[test]
fn test_patch_apply_binary_target_is_binary_error_kind() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bin_line.bin"), b"line one\x00line two\n").unwrap();
    fs::write(
        dir.path().join("p.patch"),
        "--- a/bin_line.bin\n+++ b/bin_line.bin\n@@ -1 +1 @@\n-line one\n+line ONE\n",
    )
    .unwrap();
    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "--cwd"])
        .arg(dir.path())
        .args(["patch", "apply", "p.patch", "--apply"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error_kind"], "binary", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(err.contains("binary"), "expected binary guidance, got: {v}");
    assert!(
        !err.contains("STALE"),
        "must not STALE-label invalid_input: {v}"
    );
    assert_eq!(
        fs::read(dir.path().join("bin_line.bin")).unwrap(),
        b"line one\x00line two\n"
    );
}

#[test]
fn test_patch_apply_begin_patch_writes_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "*** Begin Patch\n*** Update File: code.rs\n@@\n-fn old() {}\n+fn new() {}\n*** End Patch\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn new() {}\n"
    );
}

#[test]
fn test_patch_apply_begin_patch_ambiguous_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "x\nx\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "*** Begin Patch\n*** Update File: code.rs\n@@\n-x\n+y\n*** End Patch\n",
    )
    .unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5), "unique multi-match → exit 5");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON stdout, got stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(v["error_kind"], "ambiguous", "{v}");
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "x\nx\n"
    );
}

fn search_replace_doc(path: &str, old: &str, new: &str) -> String {
    format!("<<<<<<< SEARCH\n{path}\n-------\n{old}\n=======\n{new}\n>>>>>>> REPLACE\n")
}

#[test]
fn test_patch_apply_search_replace_writes_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
    let patch_file = dir.path().join("change.sr");
    fs::write(
        &patch_file,
        search_replace_doc("code.rs", "fn old() {}", "fn new() {}"),
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn new() {}\n"
    );
}

#[test]
fn test_patch_apply_search_replace_ambiguous_without_replace_all() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "x\nx\n").unwrap();
    let patch_file = dir.path().join("change.sr");
    fs::write(&patch_file, search_replace_doc("code.rs", "x", "y")).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(5), "unique multi-match → exit 5");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON stdout, got stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(v["error_kind"], "ambiguous", "{v}");
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "x\nx\n"
    );
}

#[test]
fn test_patch_apply_search_replace_replace_all() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "x\nx\n").unwrap();
    let patch_file = dir.path().join("change.sr");
    fs::write(&patch_file, search_replace_doc("code.rs", "x", "y")).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--replace-all")
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "y\ny\n"
    );
}

#[test]
fn test_patch_apply_replace_all_on_unified_diff_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "line1\nold line\nline3\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n",
    )
    .unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--replace-all")
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "--replace-all on unified → exit 1"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON stdout, got stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(v["error_kind"], "invalid_input", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("--replace-all is only valid for SEARCH/REPLACE"),
        "expected replace-all refuse, got {v}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("test.txt")).unwrap(),
        "line1\nold line\nline3\n"
    );
}

#[test]
fn test_patch_apply_replace_all_on_begin_patch_is_invalid_input() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "*** Begin Patch\n*** Update File: code.rs\n@@\n-fn old() {}\n+fn new() {}\n*** End Patch\n",
    )
    .unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--replace-all")
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "--replace-all on Begin Patch → exit 1"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON stdout, got stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(v["error_kind"], "invalid_input", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("--replace-all is only valid for SEARCH/REPLACE"),
        "expected replace-all refuse, got {v}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn old() {}\n"
    );
}

#[test]
fn test_patch_apply_mixed_search_replace_and_begin_patch_is_parse_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("code.rs"), "fn old() {}\n").unwrap();
    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "*** Begin Patch\n<<<<<<< SEARCH\ncode.rs\n-------\nfn old() {}\n=======\nfn new() {}\n>>>>>>> REPLACE\n*** End Patch\n",
    )
    .unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("patch")
        .arg("apply")
        .arg(&patch_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(4),
        "mixed grammar → parse_error exit 4"
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "expected JSON stdout, got stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(v["error_kind"], "parse_error", "{v}");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("mixed Begin Patch and SEARCH/REPLACE"),
        "expected mixed-grammar refuse, got {v}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn old() {}\n"
    );
}
