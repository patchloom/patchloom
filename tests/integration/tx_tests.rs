use super::*;

#[test]
fn test_tx_replace_empty_from_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "",
            "new": "X"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_ops_alias_accepted() {
    // Agents often emit "ops" instead of the canonical "operations" field.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "ops": [{
            "op": "replace",
            "path": "test.txt",
            "old": "hello",
            "new": "hi"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hi world\n");
}

#[test]
fn test_tx_replace_whole_line_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "alpha\nbeta match\ngamma\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "match",
            "new": "replaced line",
            "whole_line": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "alpha\nreplaced line\ngamma\n"
    );
}

#[test]
fn test_tx_replace_command_position_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "sudo pip install x\nuv pip install\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "pip",
            "new": "uv",
            "command_position": true,
            "require_change": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "sudo uv install x\nuv pip install\n"
    );
}

#[test]
fn test_tx_replace_command_position_rejects_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("install.sh");
    fs::write(&file, "sudo pip install\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "pip",
            "new": "uv",
            "regex": true,
            "command_position": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "command_position cannot be combined",
        ));

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "sudo pip install\n",
        "conflict must not mutate the file"
    );
}

#[test]
fn test_tx_replace_unique_ambiguous_exit_5() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "a a\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "a",
            "new": "b",
            "unique": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(5) // AMBIGUOUS, not OPERATION_FAILED (9)
        .stderr(predicate::str::contains("ambiguous match"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "a a\n");
}

#[test]
fn test_tx_replace_require_change_no_match_exit_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "missing",
            "new": "x",
            "require_change": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(3) // NO_MATCHES, not OPERATION_FAILED (9)
        .stderr(predicate::str::contains("no matches"));

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

// -- whole_line CR/CRLF tests (#999) ----------------------------------------

#[test]
fn test_tx_replace_whole_line_cr_endings_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    // Write raw bytes to avoid any line-ending conversion.
    fs::write(&file, b"alpha\rbeta match\rgamma\r").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "match",
            "new": "replaced line",
            "whole_line": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let got = fs::read(&file).unwrap();
    assert_eq!(got, b"alpha\rreplaced line\rgamma\r");
}

#[test]
fn test_tx_replace_whole_line_crlf_preserves_endings() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, b"alpha\r\nbeta match\r\ngamma\r\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "match",
            "new": "replaced line",
            "whole_line": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let got = fs::read(&file).unwrap();
    assert_eq!(got, b"alpha\r\nreplaced line\r\ngamma\r\n");
}

// ---------------------------------------------------------------------------
// doc
// ---------------------------------------------------------------------------

#[test]
fn test_tx_yaml_doc_set_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n",
    )
    .unwrap();

    let plan = dir.path().join("plan.yaml");
    fs::write(
        &plan,
        format!(
            "version: 1\noperations:\n  - op: doc.set\n    path: {}\n    selector: server.port\n    value: \"9090\"\n",
            file.display()
        ),
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(&plan)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("9090"), "new value missing: {content}");
    assert!(!content.contains("8080"), "old value present: {content}");
}

#[test]
fn test_tx_multi_op_plan() {
    let dir = TempDir::new().unwrap();

    let txt_file = dir.path().join("test.txt");
    fs::write(&txt_file, "hello world\n").unwrap();

    let json_file = dir.path().join("config.json");
    fs::write(&json_file, r#"{"name":"old"}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "old": "hello",
                "new": "hi"
            },
            {
                "op": "doc.set",
                "path": json_file.to_str().unwrap(),
                "selector": "name",
                "value": "new"
            }
        ]
    });

    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert_eq!(txt_content, "hi world\n", "replace should swap hello->hi");

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
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "old": "hello",
                "new": "hi"
            },
            {
                "op": "doc.set",
                "path": nonexistent.to_str().unwrap(),
                "selector": "name",
                "value": "test"
            }
        ]
    });

    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(1); // not_found on missing doc path

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert_eq!(
        txt_content, "hello world\n",
        "file should not be modified on operation failure"
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
            "version": 1,
        "operations": [
            {"op": "doc.set", "path": "a.json", "selector": "key", "value": "changed"},
            {"op": "doc.set", "path": "b.json", "selector": "missing", "value": "fail"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(1); // not_found on missing b.json

    // a.json should be unchanged (no writes occurred).
    let content = fs::read_to_string(&file_a).unwrap();
    assert_eq!(
        content, r#"{"key": "original"}"#,
        "a.json should be rolled back"
    );
}

#[test]
fn test_tx_mid_commit_failure_rolls_back() {
    let dir = TempDir::new().unwrap();
    let good = dir.path().join("good.txt");
    fs::write(&good, "original\n").unwrap();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, "not-a-directory").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "file.create", "path": "good.txt", "content": "changed\n", "force": true},
            {"op": "file.create", "path": "blocker/child.txt", "content": "fail\n"}
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
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(7)); // ROLLBACK after mid-commit failure
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "rollback");
    assert!(json["backup_session"].is_string());
    assert_eq!(fs::read_to_string(&good).unwrap(), "original\n");
    assert!(!dir.path().join("blocker/child.txt").exists());
}

/// End-to-end tx path when mid-commit restore cannot complete (exit 1,
/// `rollback_failed`). Uses [`patchloom::cmd::tx::RestoreFailGuard`] because
/// racing filesystem permissions against restore is unreliable.
///
/// This test is unrelated to library embedding (#792); it exists to cover
/// internal tx rollback error paths. The "z_blocker" / "a_good" naming
/// ensures a successful write precedes the failing one (lexical sort),
/// making the restore-fail case reliably hit across platforms / temp dirs.

#[test]
fn test_tx_rollback_failed_when_restore_incomplete() {
    let dir = TempDir::new().unwrap();
    // Use names so lexical sort in changes puts the successful write first,
    // ensuring a real on-disk change exists before the failing write attempt.
    // This makes restore path (and rollback_failed case) reliably exercised
    // independent of temp dir internals / macOS symlink behavior in /tmp etc.
    let good = dir.path().join("a_good.txt");
    fs::write(&good, "original\n").unwrap();
    let blocker = dir.path().join("z_blocker");
    fs::write(&blocker, "not-a-directory").unwrap();

    let plan: patchloom::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "file.create", "path": "a_good.txt", "content": "changed\n", "force": true},
            {"op": "file.create", "path": "z_blocker/child.txt", "content": "fail\n"}
        ]
    }))
    .unwrap();

    let _guard = patchloom::cmd::tx::RestoreFailGuard::engage();
    let (code, json_str) = patchloom::cmd::tx::execute_plan_direct(plan, dir.path(), None).unwrap();

    assert_eq!(code, 1, "json: {json_str}");
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json["error_kind"], "rollback_failed");
    assert!(json["backup_session"].is_string());
}

/// Guard rejection during upfront declared_paths check in execute_plan_direct.
/// Covers the PathGuard/tx validation path added for #755 (previously only
/// exercised with `None` guard in integration, or unit tests in containment.rs).

#[test]
fn test_tx_guard_rejects_escaping_declared_path() {
    let dir = TempDir::new().unwrap();
    let guard = patchloom::containment::PathGuard::new(
        dir.path().to_path_buf(),
        patchloom::containment::AbsolutePathPolicy::Reject,
    )
    .unwrap();

    // Relative escape via declared path (rename cross "file" uses both)
    let plan: patchloom::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "file.rename", "from": "ok.txt", "to": "../escape.txt", "force": false}
        ]
    }))
    .unwrap();

    let res = patchloom::cmd::tx::execute_plan_direct(plan, dir.path(), Some(&guard));
    let msg = res
        .expect_err("guard should reject before any apply")
        .to_string();
    assert!(
        msg.contains("path rejected by workspace guard") || msg.contains("Escaped"),
        "unexpected error: {msg}"
    );
}

/// PatchApply declared_paths now parses the embedded diff and returns file
/// paths, so the upfront PathGuard check catches out-of-boundary patches (#1363).
#[test]
fn test_tx_guard_rejects_patch_apply_escaping_path() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.txt"), "hello\n").unwrap();
    let guard = patchloom::containment::PathGuard::new(
        dir.path().to_path_buf(),
        patchloom::containment::AbsolutePathPolicy::Reject,
    )
    .unwrap();

    // Diff targeting a path that escapes the workspace via ../
    let plan: patchloom::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "patch.apply",
                "diff": "--- a/../escape.txt\n+++ b/../escape.txt\n@@ -1 +1 @@\n-old\n+new\n"
            }
        ]
    }))
    .unwrap();

    let res = patchloom::cmd::tx::execute_plan_direct(plan, dir.path(), Some(&guard));
    let msg = res
        .expect_err("guard should reject patch targeting escaped path")
        .to_string();
    assert!(
        msg.contains("path rejected by workspace guard") || msg.contains("Escaped"),
        "unexpected error: {msg}"
    );
}

/// PatchApply with paths inside the workspace should work normally with a guard.
#[test]
fn test_tx_guard_allows_patch_apply_within_boundary() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file.txt"), "old\n").unwrap();
    let guard = patchloom::containment::PathGuard::new(
        dir.path().to_path_buf(),
        patchloom::containment::AbsolutePathPolicy::Reject,
    )
    .unwrap();

    let plan: patchloom::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "patch.apply",
                "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n"
            }
        ]
    }))
    .unwrap();

    let result = patchloom::cmd::tx::execute_plan_direct(plan, dir.path(), Some(&guard));
    assert!(
        result.is_ok(),
        "patch within boundary should succeed: {result:?}"
    );
}

/// Glob-replace with a PathGuard validates expanded paths at runtime (#1361).
/// Verifies the guard is correctly threaded through execute_and_collect → TxState
/// and that glob-expanded files inside the boundary are processed normally.
#[test]
fn test_tx_guard_allows_glob_replace_within_boundary() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "old_value\n").unwrap();
    fs::write(dir.path().join("b.txt"), "old_value\n").unwrap();
    let guard = patchloom::containment::PathGuard::new(
        dir.path().to_path_buf(),
        patchloom::containment::AbsolutePathPolicy::Reject,
    )
    .unwrap();

    let plan: patchloom::plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "replace",
                "glob": "*.txt",
                "old": "old_value",
                "new": "new_value"
            }
        ]
    }))
    .unwrap();

    let result = patchloom::cmd::tx::execute_plan_direct(plan, dir.path(), Some(&guard));
    assert!(
        result.is_ok(),
        "glob replace within boundary should succeed: {result:?}"
    );
}

#[test]
fn test_tx_success_applies_all() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "old", "version": 1}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "doc.set", "path": "config.json", "selector": "name", "value": "new"},
            {"op": "doc.set", "path": "config.json", "selector": "version", "value": 2}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
        "operations": [
            {"op": "doc.set", "path": "data.json", "selector": "key", "value": "new"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
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
fn test_tx_file_create_and_delete() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": 1,
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
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete in same tx"
    );
}

#[test]
fn test_tx_check_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": 1,
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
        .arg(&plan_file)
        .arg("--check")
        .assert()
        .code(0);

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete no-op check"
    );
}

#[test]
fn test_tx_json_output_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": 1,
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
fn test_tx_file_delete_directory_target_fails() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.delete", "path": "folder"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .assert()
        .code(1) // invalid_input: not a file
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_tx_file_delete_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "goodbye\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!file.exists(), "file should be deleted");
}

#[test]
fn test_tx_cli_ensure_final_newline_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "no", "new": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
        "write_policy": {
            "ensure_final_newline": false
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "no", "new": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
        "write_policy": {
            "ensure_final_newline": true
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "no", "new": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

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

#[test]
fn test_tx_write_policy_collapse_blanks() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "keep\n\nremove\n\nalso keep\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "write_policy": {
            "collapse_blanks": true
        },
        "operations": [{
            "op": "replace",
            "path": "test.txt",
            "old": "remove",
            "new": "",
            "whole_line": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "keep\n\nalso keep\n");
}

// ---------------------------------------------------------------------------
// doc/patch error paths
// ---------------------------------------------------------------------------

#[test]
fn test_tx_validate_required_failure_exits_6() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "old", "new": "new"}
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
            "version": 1,
        "operations": [
            {"op": "doc.delete", "path": "config.json", "selector": "remove"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("remove").is_none());
    assert_eq!(v["keep"], serde_json::json!(1));
}

// ---------------------------------------------------------------------------
// doc: edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_tx_file_delete_empty_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!file.exists(), "empty file should be deleted");
}

#[test]
fn test_tx_file_rename_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("new-name");
    fs::create_dir(&src).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "folder", "to": "new-name"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .assert()
        .code(1) // invalid_input
        .stderr(predicate::str::contains("source is not a file"));

    assert!(src.is_dir(), "source directory should remain in place");
    assert!(!dst.exists(), "destination should not be created");
}

#[test]
fn test_tx_file_rename_moves_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "new.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be deleted after rename");
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "content\n"
    );
}

#[test]
fn test_tx_file_rename_fails_if_dst_exists() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "src\n").unwrap();
    fs::write(dir.path().join("new.txt"), "existing\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "new.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();

    // Operation fails before commit (exit 1 = already_exists).
    assert_eq!(output.status.code(), Some(1));
    // Both files should be untouched.
    assert_eq!(
        fs::read_to_string(dir.path().join("old.txt")).unwrap(),
        "src\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "existing\n"
    );
}

#[test]
fn test_tx_file_rename_force_overwrites() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "new content\n").unwrap();
    fs::write(dir.path().join("existing.txt"), "old content\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "existing.txt", "force": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!dir.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("existing.txt")).unwrap(),
        "new content\n"
    );
}

#[test]
fn test_tx_file_rename_force_directory_destination_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "folder", "force": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .assert()
        .code(1) // invalid_input
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(dir.path().join("old.txt").is_file());
    assert!(dir.path().join("folder").is_dir());
}

#[test]
fn test_tx_file_rename_force_directory_destination_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "folder", "force": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--check")
        .assert()
        .code(1) // invalid_input
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(dir.path().join("old.txt").is_file());
    assert!(dir.path().join("folder").is_dir());
}

#[test]
fn test_tx_file_rename_force_directory_destination_fails_in_apply_mode() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "old.txt", "to": "folder", "force": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(1) // invalid_input
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(dir.path().join("old.txt").is_file());
    assert!(dir.path().join("folder").is_dir());
}

#[test]
fn test_tx_file_rename_same_path_is_noop() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("same.txt"), "keep me\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.rename", "from": "same.txt", "to": "same.txt"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("same.txt")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn test_tx_file_rename_different_path_string_is_noop() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("same.txt"), "keep me\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "file.rename", "from": "same.txt", "to": "./same.txt", "force": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert!(
        dir.path().join("same.txt").exists(),
        "file must not be deleted"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("same.txt")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn test_tx_optional_validation_failure_ignored() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "old", "new": "new"}
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
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "new\n",
        "file should be modified despite optional validation failure"
    );
}

// ---------------------------------------------------------------------------
// md --check leaves file untouched
// ---------------------------------------------------------------------------

#[test]
fn test_tx_glob_replace_only_matches_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("skip.rs"), "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "replace", "glob": "*.txt", "old": "hello", "new": "bye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "bye\n",
        "a.txt should be updated by glob replace"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "bye\n",
        "b.txt should be updated by glob replace"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("skip.rs")).unwrap(),
        "hello\n",
        ".rs file should not be modified by *.txt glob"
    );
}

#[test]
fn test_tx_glob_replace_matches_file_created_earlier_in_transaction() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "file.create", "path": "new.txt", "content": "hello\n"},
            {"op": "replace", "glob": "*.txt", "old": "hello", "new": "bye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "bye\n"
    );
}

#[test]
fn test_tx_glob_replace_matches_nested_relative_pattern() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/keep.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("other.txt"), "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "replace", "glob": "sub/*.txt", "old": "hello", "new": "bye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("sub/keep.txt")).unwrap(),
        "bye\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("other.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_tx_glob_replace_no_matching_files_exits_3() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("only.rs"), "hello\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "replace", "glob": "*.nonexistent", "old": "hello", "new": "bye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(3),
        "glob matching zero files should exit 3 (NO_MATCHES)"
    );

    // Source file should be untouched.
    assert_eq!(
        fs::read_to_string(dir.path().join("only.rs")).unwrap(),
        "hello\n"
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
            "version": 1,
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
        .arg(&plan_file)
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("v2.0 release"), "new section content");
    assert!(!content.contains("Old entry"), "old content removed");
    assert!(content.contains("Kept"), "other section preserved");
}

// ---------------------------------------------------------------------------
// write policy CLI flags: normalize-eol, trim-trailing-whitespace, --diff
// ---------------------------------------------------------------------------

#[test]
fn test_tx_replace_insert_before_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "    /// Old doc.\n    pub val: i32,\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "    /// Old doc.",
                "insert_before": "    // ref:marker\n"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "    // ref:marker\n    /// Old doc.\n    pub val: i32,\n"
    );
}

#[test]
fn test_tx_replace_insert_before_with_regex_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "regex": true,
            "old": "b+",
            "insert_before": "X"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nXbbb\nccc\n");
}

#[test]
fn test_tx_replace_insert_after_with_regex_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "regex": true,
            "old": "b+",
            "insert_after": "X"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nbbbX\nccc\n");
}

#[test]
fn test_tx_replace_rejects_both_insert_before_and_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "hello",
                "insert_before": "X",
                "insert_after": "Y"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_replace_rejects_to_with_insert() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "hello",
                "new": "goodbye",
                "insert_before": "X"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_file_create_new_file_writes_content() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("brand_new.txt");

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "created via tx\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "created via tx\n",
        "file.create via tx should write correct content through File::create_new"
    );
}

#[test]
fn test_tx_file_create_force_directory_target_fails() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "file.create",
            "path": target.to_str().unwrap(),
            "content": "hello\n",
            "force": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .assert()
        .code(1) // invalid_input: not a file
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_tx_file_create_force_overwrites() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "overwritten\n");
}

#[test]
fn test_tx_file_create_without_force_fails_on_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(1); // already_exists

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
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "before",
            "new": "after"
        }],
        "format": [{"cmd": shell_touch(&marker)}],
        "validate": [{"cmd": shell_test_exists(&marker), "required": true}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");
    assert!(marker.exists(), "format step should have created marker");
}

#[test]
fn test_tx_doc_prepend_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2, 3]}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "doc.prepend",
            "path": file.to_str().unwrap(),
            "selector": "items",
            "value": 0
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["items"][0], 0);
    assert_eq!(v["items"][1], 1);
}

#[test]
fn test_tx_doc_set_unsupported_format_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "this is not a structured file\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "doc.set",
            "path": portable_path_str(&file),
            "selector": "key",
            "value": "val"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(9); // OPERATION_FAILED

    // Original content must be preserved.
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "this is not a structured file\n"
    );
}

#[test]
fn test_tx_doc_set_selector_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"nested": {"name": "old"}}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["nested"]["name"], "new");
}

#[test]
fn test_tx_doc_ensure_selector_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "test"}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
        "operations": [{
            "op": "doc.delete_where",
            "path": file.to_str().unwrap(),
            "selector": "items",
            "predicate": "name=remove"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
        "operations": [{
            "op": "doc.update",
            "path": file.to_str().unwrap(),
            "selector": "items[*].status",
            "value": "archived"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before.\n\n## Section"));
}

#[test]
fn test_tx_md_upsert_bullet_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Rules\n\n- existing rule\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    // Only one "## Dupe" heading should remain.
    assert_eq!(content.matches("## Dupe").count(), 1);
}

#[test]
fn test_tx_md_move_section_equivalent_path_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# A\na-body\n# B\nb-body\n# C\nc-body\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.move_section",
            "path": file.to_str().unwrap(),
            "heading": "C",
            "before": "B"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let a_pos = content.find("# A").unwrap();
    let c_pos = content.find("# C").unwrap();
    let b_pos = content.find("# B").unwrap();
    assert!(a_pos < c_pos, "A should come before C");
    assert!(c_pos < b_pos, "C should come before B after move");
}

#[test]
fn test_tx_md_move_section_explicit_to_equivalent_path_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# A\na-body\n# B\nb-body\n# C\nc-body\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.move_section",
            "path": file.to_str().unwrap(),
            "heading": "C",
            "to": file.to_str().unwrap(),
            "before": "B"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let a_pos = content.find("# A").unwrap();
    let c_pos = content.find("# C").unwrap();
    let b_pos = content.find("# B").unwrap();
    assert!(a_pos < c_pos, "A should come before C");
    assert!(c_pos < b_pos, "C should come before B after move");
    assert_eq!(
        content.matches("# C").count(),
        1,
        "section must not be duplicated"
    );
}

#[test]
fn test_tx_md_move_section_cross_file_in_plan() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("source.md");
    let dst = dir.path().join("dest.md");
    fs::write(&src, "# Keep\nkept\n# Move\nmoved\n").unwrap();
    fs::write(&dst, "# Intro\nintro\n# End\nend\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.move_section",
            "path": src.to_str().unwrap(),
            "heading": "Move",
            "to": dst.to_str().unwrap(),
            "before": "End"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let src_content = fs::read_to_string(&src).unwrap();
    assert!(
        !src_content.contains("# Move"),
        "section removed from source"
    );
    assert!(src_content.contains("# Keep"), "other content preserved");

    let dst_content = fs::read_to_string(&dst).unwrap();
    assert!(dst_content.contains("# Move"), "section added to dest");
    assert!(dst_content.contains("moved"), "section body added to dest");
    let move_pos = dst_content.find("# Move").unwrap();
    let end_pos = dst_content.find("# End").unwrap();
    assert!(move_pos < end_pos, "moved section before End");
}

#[test]
fn test_tx_md_lint_agents_reports_issues() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    // Duplicate heading + dangerous command outside code fence
    fs::write(
        &file,
        "# Rules\nRun git add . to stage\n# Rules\nMore stuff\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.lint_agents",
            "path": portable_path_str(&file)
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let lints = json["lints"].as_array().unwrap();
    assert_eq!(lints.len(), 1);
    assert!(lints[0]["issue_count"].as_u64().unwrap() >= 2);
    // Should find duplicate heading and dangerous command
    let issues = lints[0]["issues"].as_array().unwrap();
    let issue_types: Vec<&str> = issues
        .iter()
        .map(|i| i["issue"].as_str().unwrap())
        .collect();
    assert!(
        issue_types.iter().any(|t| t.contains("duplicate")),
        "expected duplicate heading issue: {issue_types:?}"
    );
    assert!(
        issue_types.iter().any(|t| t.contains("dangerous")),
        "expected dangerous command issue: {issue_types:?}"
    );
}

#[test]
fn test_tx_md_lint_agents_clean_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# Rules\nUse explicit staging.\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.lint_agents",
            "path": portable_path_str(&file)
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let lints = json["lints"].as_array().unwrap();
    assert_eq!(lints.len(), 1);
    assert_eq!(lints[0]["issue_count"], 0);
}

#[test]
fn test_tx_md_insert_after_heading_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\nExisting content\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "md.insert_after_heading",
            "path": file.to_str().unwrap(),
            "heading": "Title",
            "content": "Inserted line\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert!(result.contains("Inserted line\nExisting content"));
}

#[test]
fn test_tx_md_replace_section_nonexistent_file_rolls_back() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "md.replace_section",
            "path": nonexistent_path("doc.md"),
            "heading": "Title",
            "content": "New content\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(1); // not_found (missing path), was OPERATION_FAILED (9)
}

#[test]
fn test_tx_md_replace_section_missing_heading_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\nSome content\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "md.replace_section",
            "path": file.to_str().unwrap(),
            "heading": "Nonexistent Heading",
            "content": "New content\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES (heading not found)

    // Original content must be preserved.
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# Title\nSome content\n"
    );
}

#[test]
fn test_tx_tidy_fix_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    // File has no final newline.
    fs::write(&file, "line1\nline2").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "tidy.fix",
            "path": file.to_str().unwrap(),
            "ensure_final_newline": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "line1\nline2\n");
}

#[test]
fn test_tx_tidy_fix_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    fs::write(&file, "hello   \nworld\t\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "tidy.fix",
            "path": file.to_str().unwrap(),
            "trim_trailing_whitespace": true,
            "ensure_final_newline": false
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "hello\nworld\n");
}

#[test]
fn test_tx_tidy_fix_normalize_eol() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "line1\r\nline2\r\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "tidy.fix",
            "path": file.to_str().unwrap(),
            "normalize_eol": "lf",
            "ensure_final_newline": false
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "line1\nline2\n");
}

#[test]
fn test_tx_doc_append_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2]}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "doc.append",
            "path": file.to_str().unwrap(),
            "selector": "items",
            "value": 3
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(result["items"], serde_json::json!([1, 2, 3]));
}

#[test]
fn test_tx_doc_merge_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a": 1, "b": {"c": 2}}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "doc.merge",
            "path": file.to_str().unwrap(),
            "value": {"b": {"d": 3}, "e": 4}
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(result["a"], 1);
    assert_eq!(result["b"]["c"], 2);
    assert_eq!(result["b"]["d"], 3);
    assert_eq!(result["e"], 4);
}

#[test]
fn test_tx_replace_case_insensitive_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Hello HELLO hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "HI",
            "case_insensitive": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "HI HI HI\n");
}

#[test]
fn test_tx_replace_multiline_regex_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "start\nmiddle\nend\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "regex": true,
            "old": "start.middle",
            "new": "REPLACED",
            "multiline": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "REPLACED\nend\n");
}

#[test]
fn test_tx_replace_nth_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa bbb aaa ccc aaa\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "aaa",
            "new": "ZZZ",
            "nth": 2
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa bbb ZZZ ccc aaa\n");
}

#[test]
fn test_tx_replace_apply_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "zzz",
            "new": "ZZZ"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(3);

    assert_eq!(fs::read_to_string(&file).unwrap(), "foo bar\n");
}

#[test]
fn test_tx_replace_check_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "zzz",
            "new": "ZZZ"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(3);
}

#[test]
fn test_tx_json_check_replace_no_match_exits_3_with_no_matches_payload() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "zzz",
            "new": "ZZZ"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("tx --json should emit JSON on no-match");
    assert_eq!(v["status"], "no_matches", "status should be no_matches");
}

#[test]
fn test_tx_replace_if_exists_no_match_succeeds() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "zzz",
            "new": "ZZZ",
            "if_exists": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "foo bar\n");
}

#[test]
fn test_tx_replace_if_exists_still_replaces_when_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "foo",
            "new": "baz",
            "if_exists": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "baz bar\n");
}

#[test]
fn test_tx_replace_no_match_does_not_hide_other_changes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    let config = dir.path().join("config.json");
    fs::write(&file, "foo bar\n").unwrap();
    fs::write(&config, r#"{"name":"test"}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "zzz",
                "new": "ZZZ"
            },
            {
                "op": "doc.ensure",
                "path": config.to_str().unwrap(),
                "selector": "version",
                "value": "1.0"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let config_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(config_value["version"], "1.0");
}

/// Non-Replace operations that return non-zero counts must not mask a
/// Replace no-match condition.  Before #1105, `total_replace_matches`
/// mixed counts from all operations, so a plan with a read-only
/// operation (which returns a non-zero count) plus a failing Replace
/// would exit 0 instead of 3.
#[test]
fn test_tx_non_replace_count_does_not_mask_replace_no_match() {
    let dir = TempDir::new().unwrap();
    let txt_file = dir.path().join("target.txt");
    fs::write(&txt_file, "hello world\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "search",
                "path": txt_file.to_str().unwrap(),
                "pattern": "hello"
            },
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "old": "nonexistent_string_xyz",
                "new": "replacement"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // Should exit 3 (NO_MATCHES) because Replace found no matches,
    // regardless of search returning a non-zero match count.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(3);
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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\n"
    );
}

#[test]
fn test_tx_patch_apply_uses_pending_file_state() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nold line\nline3\n").unwrap();

    let diff = format!(
        "--- a/{f}\n+++ b/{f}\n@@ -1,3 +1,3 @@\n line1\n-mid line\n+new line\n line3\n",
        f = file.to_str().unwrap()
    );

    let plan = serde_json::json!({
            "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "old line",
                "new": "mid line"
            },
            {
                "op": "patch.apply",
                "diff": diff
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\n"
    );
}

#[test]
fn test_tx_patch_apply_on_stale_merge() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nold line\nline3\nextra\n").unwrap();

    let diff =
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";
    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "patch.apply",
            "diff": diff,
            "on_stale": "merge"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\nextra\n"
    );
}

#[test]
fn test_tx_patch_apply_merge_conflict_without_allow_conflicts() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let diff =
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";
    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "patch.apply",
            "diff": diff,
            "on_stale": "merge"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let assert = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(8); // CONFLICTS
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(
        v["error_kind"], "conflicts",
        "tx patch.apply merge conflicts must set error_kind conflicts: {stdout}"
    );
    assert!(
        v["error"].as_str().unwrap_or("").contains("conflict(s)"),
        "error should mention conflicts: {stdout}"
    );
}

#[test]
fn test_tx_patch_apply_merge_conflict_with_allow_conflicts() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

    let diff =
        "--- a/test.txt\n+++ b/test.txt\n@@ -1,3 +1,3 @@\n line1\n-old line\n+new line\n line3\n";
    let plan = serde_json::json!({
        "version": 1,
        "cwd": dir.path().to_str().unwrap(),
        "operations": [{
            "op": "patch.apply",
            "diff": diff,
            "on_stale": "merge",
            "allow_conflicts": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("<<<<<<< patchloom (ours)"),
        "should have ours marker: {content}"
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
fn test_tx_validate_timeout_kills_hanging_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "content",
            "new": "changed"
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
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "content",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .code(6); // VALIDATION_FAILED
}

#[test]
fn test_tx_read_operation_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "read", "path": file.to_str().unwrap()}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["reads"].as_array().unwrap().len(), 1);
    assert!(
        json["reads"][0]["content"]
            .as_str()
            .unwrap()
            .contains("line1")
    );
    assert_eq!(json["reads"][0]["total_lines"], 3);
}

#[test]
fn test_tx_read_nonexistent_file_returns_error_json() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{"op": "read", "path": nonexistent_path("missing.txt")}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["error"].as_str().unwrap().contains("failed to read"));
}

#[test]
fn test_tx_read_empty_file_without_lines_matches_read_contract() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "read", "path": file.to_str().unwrap()}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["reads"][0]["content"].as_str().unwrap(), "");
    assert_eq!(json["reads"][0]["total_lines"], 0);
    assert_eq!(json["reads"][0]["start_line"], 0);
    assert_eq!(json["reads"][0]["end_line"], 0);
}

#[test]
fn test_tx_read_without_lines_preserves_crlf_content() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "a\r\nb\r\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "read", "path": file.to_str().unwrap()}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["reads"][0]["content"].as_str().unwrap(), "a\r\nb\r\n");
    assert_eq!(json["reads"][0]["start_line"], 1);
    assert_eq!(json["reads"][0]["end_line"], 2);
    assert_eq!(json["reads"][0]["total_lines"], 2);
}

#[test]
fn test_tx_read_with_lines_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\nddd\neee\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "read", "path": file.to_str().unwrap(), "lines": "2:4"}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let content = json["reads"][0]["content"].as_str().unwrap();
    assert!(content.contains("bbb"));
    assert!(content.contains("ccc"));
    assert!(content.contains("ddd"));
    assert!(!content.contains("aaa"));
    assert!(!content.contains("eee"));
    assert_eq!(json["reads"][0]["start_line"], 2);
    assert_eq!(json["reads"][0]["end_line"], 4);
}

#[test]
fn test_tx_read_lines_start_past_eof_clamps_metadata() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("short.txt");
    fs::write(&file, "a\nb\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "read", "path": file.to_str().unwrap(), "lines": "5:9"}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["reads"][0]["content"].as_str().unwrap(), "");
    assert_eq!(json["reads"][0]["total_lines"], 2);
    assert_eq!(json["reads"][0]["start_line"], 0);
    assert_eq!(json["reads"][0]["end_line"], 0);
}

#[test]
fn test_tx_read_sees_in_plan_state() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "read", "path": file.to_str().unwrap()},
            {"op": "replace", "path": file.to_str().unwrap(), "old": "hello", "new": "goodbye"},
            {"op": "read", "path": file.to_str().unwrap()}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "tx with replace should exit 2 in preview mode"
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let reads = json["reads"].as_array().unwrap();
    assert_eq!(reads.len(), 2);
    // First read sees original content
    assert!(reads[0]["content"].as_str().unwrap().contains("hello"));
    // Second read sees post-replace content
    assert!(reads[1]["content"].as_str().unwrap().contains("goodbye"));
}

#[test]
fn test_tx_search_operation_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "alpha\nbeta\ngamma\nalpha two\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{"op": "search", "path": file.to_str().unwrap(), "pattern": "alpha"}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["searches"].as_array().unwrap().len(), 1);
    assert_eq!(json["searches"][0]["match_count"], 2);
    assert_eq!(json["searches"][0]["matches"][0]["line"], 1);
    assert_eq!(json["searches"][0]["matches"][1]["line"], 4);
}

#[test]
fn test_tx_search_then_replace_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello world\nfoo bar\nhello again\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "search", "path": file.to_str().unwrap(), "pattern": "hello"},
            {"op": "replace", "path": file.to_str().unwrap(), "old": "hello", "new": "goodbye"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Search found 2 matches before replace
    assert_eq!(json["searches"][0]["match_count"], 2);
    // Replace changed the file
    assert_eq!(json["files_changed"], 1);
    // File on disk is replaced
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "goodbye world\nfoo bar\ngoodbye again\n"
    );
}

#[test]
fn test_tx_search_directory_path() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("src");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("main.rs"), "fn main() { hello(); }\n").unwrap();
    fs::write(sub.join("lib.rs"), "pub fn hello() {}\npub fn world() {}\n").unwrap();
    fs::write(dir.path().join("README.md"), "No match here\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{"op": "search", "path": portable_path_str(&sub), "pattern": "hello"}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let searches = json["searches"].as_array().unwrap();
    assert_eq!(searches.len(), 1);
    // Should find "hello" in both main.rs and lib.rs (2 matches total)
    assert_eq!(searches[0]["match_count"], 2);
    // Multi-file text includes the file path prefix.
    let texts: Vec<&str> = searches[0]["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("lib.rs:")),
        "expected path prefix in multi-file text: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("main.rs:")),
        "expected path prefix in multi-file text: {texts:?}"
    );
}

#[test]
fn test_tx_search_directory_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("mixed");
    fs::create_dir_all(&sub).unwrap();
    // Text file containing the search pattern
    fs::write(sub.join("code.rs"), "fn hello() {}\n").unwrap();
    // Binary file: embed a NUL byte so is_binary_file detects it
    let mut binary_content = b"fn hello() {}\n".to_vec();
    binary_content.insert(5, 0x00);
    fs::write(sub.join("data.bin"), &binary_content).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{"op": "search", "path": portable_path_str(&sub), "pattern": "hello"}]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let searches = json["searches"].as_array().unwrap();
    assert_eq!(searches.len(), 1);
    // Should only find the match in code.rs, not the binary file
    assert_eq!(searches[0]["match_count"], 1);
    let text = searches[0]["matches"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("code.rs:"),
        "expected match from code.rs only, got: {text}"
    );
}

#[test]
fn test_tx_search_multiline_spans_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "search",
            "path": portable_path_str(&file),
            "pattern": "hello.*world",
            "regex": true,
            "multiline": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let searches = json["searches"].as_array().unwrap();
    assert_eq!(searches.len(), 1);
    assert_eq!(
        searches[0]["match_count"], 1,
        "multiline regex should match across lines"
    );
}

#[test]
fn test_tx_search_invert_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\ngoodbye\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "search",
            "path": portable_path_str(&file),
            "pattern": "hello",
            "invert_match": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let searches = json["searches"].as_array().unwrap();
    assert_eq!(searches.len(), 1);
    // "hello" matches line 1, so invert_match returns lines 2 and 3
    assert_eq!(
        searches[0]["match_count"], 2,
        "invert_match should return non-matching lines"
    );
    let texts: Vec<&str> = searches[0]["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap())
        .collect();
    assert!(
        texts.iter().all(|t| !t.contains("hello")),
        "inverted results should not contain 'hello': {texts:?}"
    );
}

#[test]
fn test_tx_search_assert_count_pass() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\naaa\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "search",
            "path": portable_path_str(&file),
            "pattern": "aaa",
            "assert_count": 2
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "assert_count=2 should pass when there are 2 matches; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_tx_search_assert_count_fail() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\naaa\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "search",
            "path": portable_path_str(&file),
            "pattern": "aaa",
            "assert_count": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "assert_count=5 should fail when there are only 2 matches"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("assert_count") || stdout.contains("expected 5"),
        "error should mention assert_count: {stdout}"
    );
}

#[test]
fn test_tx_search_invert_match_multiline_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "search",
            "path": portable_path_str(&file),
            "pattern": "hello",
            "invert_match": true,
            "multiline": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "invert_match + multiline should be rejected"
    );
}

#[test]
fn test_tx_strict_mode_reverts_on_format_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
            "version": 1,
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
            "version": 1,
        "strict": true,
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "changed",
            "force": true
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
            "version": 1,
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
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
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

/// #1439: tx --json includes mutations for delete / delete-where (incl. no-ops).
#[test]
fn test_tx_json_doc_delete_mutations_summary() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("t.json"),
        r#"{"items":[{"name":"a"},{"name":"b"},{"name":"a"}]}"#,
    )
    .unwrap();
    fs::write(dir.path().join("u.json"), r#"{"x":1}"#).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {
                "op": "doc.delete_where",
                "path": "t.json",
                "selector": "items",
                "predicate": "name=a"
            },
            {
                "op": "doc.delete",
                "path": "u.json",
                "selector": "missing"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path())
        .arg("--json")
        .arg("tx")
        .arg("plan.json")
        .arg("--apply")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "payload: {json}");
    assert_eq!(json["files_changed"], 1, "payload: {json}");
    assert_eq!(json["changed"], true, "payload: {json}");
    assert_eq!(json["removed"], 2, "payload: {json}");
    let mutations = json["mutations"].as_array().expect("mutations");
    assert_eq!(mutations.len(), 2, "payload: {json}");
    assert_eq!(mutations[0]["path"], "t.json");
    assert_eq!(mutations[0]["op"], "doc.delete_where");
    assert_eq!(mutations[0]["removed"], 2);
    assert_eq!(mutations[0]["changed"], true);
    assert_eq!(mutations[1]["path"], "u.json");
    assert_eq!(mutations[1]["op"], "doc.delete");
    assert_eq!(mutations[1]["removed"], 0);
    assert_eq!(mutations[1]["changed"], false);
}

/// #1439: --check still reports mutation summaries without writing.
#[test]
fn test_tx_json_check_doc_delete_mutations_no_write() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("t.json"),
        r#"{"items":[{"name":"a"},{"name":"b"}]}"#,
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "doc.delete_where",
            "path": "t.json",
            "selector": "items",
            "predicate": "name=a"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path())
        .arg("--json")
        .arg("tx")
        .arg("plan.json")
        .arg("--check")
        .output()
        .unwrap();
    // --check with changes → exit 2
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "payload: {json}");
    assert_eq!(json["changed"], true, "payload: {json}");
    assert_eq!(json["removed"], 1, "payload: {json}");
    assert_eq!(json["mutations"][0]["op"], "doc.delete_where");
    // File must be unchanged under --check.
    let on_disk = fs::read_to_string(dir.path().join("t.json")).unwrap();
    assert!(
        on_disk.contains("\"name\":\"a\""),
        "must not write: {on_disk}"
    );
}

/// #1439: for_each-expanded delete-where produces one mutation row per file.
#[test]
fn test_tx_json_for_each_delete_where_mutations() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("d");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("a.json"), r#"{"items":[{"name":"x"}]}"#).unwrap();
    fs::write(sub.join("b.json"), r#"{"items":[{"name":"x"}]}"#).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {"glob": "d/*.json", "path_var": "path"},
        "operations": [{
            "op": "doc.delete_where",
            "path": "{path}",
            "selector": "items",
            "predicate": "name=x"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .current_dir(dir.path())
        .arg("--json")
        .arg("tx")
        .arg("plan.json")
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true, "payload: {json}");
    assert_eq!(json["files_changed"], 2, "payload: {json}");
    assert_eq!(json["removed"], 2, "payload: {json}");
    let mutations = json["mutations"].as_array().expect("mutations");
    assert_eq!(mutations.len(), 2, "payload: {json}");
    let mut paths: Vec<&str> = mutations
        .iter()
        .map(|m| m["path"].as_str().unwrap())
        .collect();
    paths.sort();
    assert_eq!(paths, vec!["d/a.json", "d/b.json"]);
    for m in mutations {
        assert_eq!(m["op"], "doc.delete_where");
        assert_eq!(m["removed"], 1);
        assert_eq!(m["changed"], true);
    }
}

#[test]
fn test_tx_json_output_on_modified_empty_file_reports_modified() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "changed",
            "force": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
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
            "version": 1,
        "operations": [{
            "op": "file.create",
            "path": file.to_str().unwrap(),
            "content": "changed",
            "force": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
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
fn test_tx_jsonl_output_on_check() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "replace", "path": file.to_str().unwrap(), "old": "hello", "new": "world"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("tx")
        .arg(&plan_file)
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["files_changed"], 1);
}

#[test]
fn test_tx_json_output_on_check() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
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
            "version": 1,
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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1)); // already_exists
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "already_exists");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("file already exists")
    );
}

#[test]
fn test_tx_json_output_on_diff() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // Default mode (no --apply or --check) is diff mode.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "tx default mode with changes should exit 2"
    );
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4)); // PARSE_ERROR
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "parse_error");
    assert!(!json["error"].as_str().unwrap().is_empty());
    assert!(json["error"].as_str().unwrap().contains("parse_error"));
}

#[test]
fn test_tx_replace_requires_replacement_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "old": "hello"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4);

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_json_output_on_replace_missing_mode_parse_error() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("bad.json");
    fs::write(
        &plan_file,
        serde_json::to_string(&serde_json::json!({
            "version": 1,
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "old": "hello"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "parse_error");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("parse_error"));
    assert!(error.contains("one of 'to', 'insert_before', or 'insert_after' must be provided"));
}

#[test]
fn test_tx_json_output_on_replace_conflict_parse_error() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("bad.json");
    fs::write(
        &plan_file,
        serde_json::to_string(&serde_json::json!({
            "version": 1,
            "operations": [{
                "op": "replace",
                "path": "data.txt",
                "old": "hello",
                "insert_before": "X",
                "insert_after": "Y"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert_eq!(json["error_kind"], "parse_error");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("parse_error"));
    assert!(error.contains("'insert_before' and 'insert_after' cannot be combined"));
}

#[test]
fn test_tx_validation_failure_redacts_shell_command_in_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("required validation failed (step 1, exit code 1, cwd: .)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1, cwd: .)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1, cwd: .)"));
    assert!(error.contains("cwd: ."));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_json_output_on_strict_validation_failure_preserves_reason() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1, cwd: .)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_strict_mode_restores_deleted_empty_file_on_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": 1,
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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // Deleted file should be restored.
    assert_eq!(fs::read_to_string(&file).unwrap(), "keep me\n");
}

#[test]
fn test_tx_default_strict_reverts_on_format_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(7);

    assert_eq!(fs::read_to_string(&file).unwrap(), "original\n");
}

#[test]
fn test_tx_no_strict_cli_overrides_default_on_format_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .arg("--no-strict")
        .assert()
        .code(6);

    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
}

#[test]
fn test_tx_non_strict_format_failure_exits_6_not_7() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("format step failed (step 1, exit code 1, cwd: .)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_format_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "format_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("format_failed"));
    assert!(error.contains("format step failed (step 1, exit code 1, cwd: .)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_strict_format_failure_preserves_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
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
    assert!(error.contains("format step failed (step 1, exit code 1, cwd: .)"));
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
            "version": 1,
        "operations": [
            {"op": "replace", "path": "test.txt", "old": "no", "new": "has"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "no trailing newline\n");
}

#[test]
fn test_tx_yaml_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "old value\n").unwrap();

    let yaml_plan = format!(
        "version: 1\noperations:\n  - op: replace\n    path: \"{}\"\n    old: old\n    new: new\n",
        portable_path_str(&file)
    );
    let plan_file = dir.path().join("plan.yaml");
    fs::write(&plan_file, &yaml_plan).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "new value\n");
}

#[test]
fn test_tx_toml_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "hello world\n").unwrap();

    let toml_plan = format!(
        "version = 1\n\n[[operations]]\nop = \"replace\"\npath = \"{}\"\nold = \"hello\"\nnew = \"goodbye\"\n",
        portable_path_str(&file)
    );
    let plan_file = dir.path().join("plan.toml");
    fs::write(&plan_file, &toml_plan).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "goodbye world\n");
}

#[test]
fn test_tx_yaml_plan_from_stdin() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\n").unwrap();

    let yaml_plan = format!(
        "version: 1\noperations:\n  - op: replace\n    path: \"{}\"\n    old: aaa\n    new: bbb\n",
        portable_path_str(&file)
    );

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("-")
        .arg("--plan-format")
        .arg("yaml")
        .arg("--apply")
        .write_stdin(yaml_plan)
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "bbb\n");
}

#[test]
fn test_tx_malformed_yaml_returns_parse_error() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("bad.yaml");
    fs::write(&plan_file, "this: is: not: valid: yaml: [").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .assert()
        .code(4);
}

#[test]
fn test_tx_plan_from_stdin() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }]
    });

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("-")
        .arg("--apply")
        .write_stdin(serde_json::to_string(&plan).unwrap())
        .assert()
        .code(0);

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
            "version": 1,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    // File should exist with the new content, not deleted.
    assert!(
        file.exists(),
        "file should not be deleted after a subsequent create"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "new content");
}

#[test]
fn test_tx_file_rename_after_delete_succeeds() {
    // Regression: FileRename dst_exists check did not consult deletions set.
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("new.txt");
    let dst = dir.path().join("old.txt");
    fs::write(&src, "source\n").unwrap();
    fs::write(&dst, "dest\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            { "op": "file.delete", "path": portable_path_str(&dst) },
            { "op": "file.rename", "from": portable_path_str(&src), "to": portable_path_str(&dst) }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert!(!src.exists(), "source should be gone after rename");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "source\n");
}

#[test]
fn test_tx_format_and_validate_success_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let marker = dir.path().join("format_ran.marker");

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "content",
            "new": "changed"
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
    assert!(
        marker.exists(),
        "format step should have created the marker"
    );
}

#[test]
fn test_tx_replace_nth_regex_with_capture_groups_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "version = \"1.2.3\"\nversion = \"4.5.6\"\n").unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "regex": true,
            "old": r#"version = "(\d+)\.(\d+)\.(\d+)""#,
            "new": r#"version = "$1.$2.99""#,
            "nth": 2
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "version = \"1.2.3\"\nversion = \"4.5.99\"\n");
}

#[test]
fn test_tx_doc_prepend_on_non_array_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name": "not_an_array"}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [{
            "op": "doc.prepend",
            "path": file.to_str().unwrap(),
            "selector": "name",
            "value": "oops"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(1); // type_error

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
            "version": 1,
        "operations": [{
            "op": "doc.update",
            "path": file.to_str().unwrap(),
            "selector": "items[*].status",
            "value": "archived"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES (selector matched nothing)
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
            "version": 1,
        "operations": [
            {"op": "replace", "path": txt.to_str().unwrap(), "old": "foo", "new": "XXX", "nth": 1},
            {"op": "doc.set", "path": json_file.to_str().unwrap(), "selector": "name", "value": "new"},
            {"op": "doc.ensure", "path": json_file.to_str().unwrap(), "selector": "version", "value": "1.0"},
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

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

#[test]
fn test_tx_create_then_replace_on_equivalent_path() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("created.txt");

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {
                "op": "file.create",
                "path": new_file.to_str().unwrap(),
                "content": "hello world\n"
            },
            {
                "op": "replace",
                "path": new_file.to_str().unwrap(),
                "old": "world",
                "new": "patchloom"
            }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&new_file).unwrap(), "hello patchloom\n");
}

#[test]
fn test_tx_multiple_doc_set_on_same_yaml_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# App config\nname: myapp\nversion: \"1.0\"\nport: 8080\n",
    )
    .unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "doc.set", "path": portable_path_str(&file), "selector": "version", "value": "2.0"},
            {"op": "doc.set", "path": portable_path_str(&file), "selector": "port", "value": 9090},
            {"op": "doc.set", "path": portable_path_str(&file), "selector": "debug", "value": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let result = fs::read_to_string(&file).unwrap();
    // All three mutations applied correctly.
    assert!(
        result.contains("version: '2.0'") || result.contains("version: \"2.0\""),
        "version not updated: {result}"
    );
    assert!(result.contains("9090"), "port not updated: {result}");
    assert!(
        result.contains("debug: true") || result.contains("debug: True"),
        "debug not added: {result}"
    );
    // YAML comment preserved (verifies serialize_value_preserving worked).
    assert!(
        result.contains("# App config"),
        "YAML comment lost: {result}"
    );
}

#[test]
fn test_tx_doc_set_then_replace_on_equivalent_path_flushes_cache() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "old", "version": "1.0"}"#).unwrap();

    let plan = serde_json::json!({
            "version": 1,
        "operations": [
            {"op": "doc.set", "path": file.to_str().unwrap(), "selector": "name", "value": "new"},
            {"op": "replace", "path": file.to_str().unwrap(), "old": "1.0", "new": "2.0"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["name"], "new", "doc.set mutation lost after cache flush");
    assert_eq!(v["version"], "2.0", "replace did not see flushed content");
}

// ---------------------------------------------------------------------------
// smoke tests: docs and examples
// ---------------------------------------------------------------------------

#[test]
fn test_tx_honors_cwd_for_relative_plan_path() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("plan.toml"),
        "version = 1\n\n[[operations]]\nop = \"file.create\"\npath = \"out.txt\"\ncontent = \"hello\\n\"\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg("plan.toml")
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("out.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_tx_relative_plan_cwd_resolves_from_invocation_root() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    let nested = repo.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        repo.join("plan.toml"),
        "version = 1\ncwd = \"nested\"\n\n[[operations]]\nop = \"file.create\"\npath = \"out.txt\"\ncontent = \"hello\\n\"\n",
    )
    .unwrap();

    patchloom_in(&repo)
        .arg("tx")
        .arg("plan.toml")
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(nested.join("out.txt")).unwrap(),
        "hello\n"
    );
    assert!(!repo.join("out.txt").exists());
}

#[test]
fn test_tx_json_output_reports_lifecycle_cwd_for_relative_plan_cwd() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    let nested = repo.join("nested");
    fs::create_dir_all(&nested).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "strict": false,
        "cwd": "nested",
        "operations": [{
            "op": "file.create",
            "path": "out.txt",
            "content": "hello\n"
        }],
        "validate": [{
            "cmd": shell_false(),
            "required": true,
            "timeout": 5
        }]
    });
    let plan_file = repo.join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = patchloom_in(&repo)
        .arg("--json")
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "validation_failed");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("required validation failed (step 1, exit code 1, cwd: nested)"));
    assert_eq!(
        fs::read_to_string(nested.join("out.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_tx_cli_rejects_plan_cwd_that_is_a_file() {
    let dir = TempDir::new().unwrap();
    let not_a_dir = dir.path().join("not-a-dir");
    fs::write(&not_a_dir, "nope\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": "not-a-dir",
        "operations": [{
            "op": "replace",
            "path": "anything.txt",
            "old": "a",
            "new": "b"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .assert()
        .code(4) // PARSE_ERROR
        .stderr(predicate::str::contains("plan cwd is not a directory"));
}

#[test]
fn test_tx_cli_rejects_plan_cwd_that_is_a_file_json_output() {
    let dir = TempDir::new().unwrap();
    let not_a_dir = dir.path().join("not-a-dir");
    fs::write(&not_a_dir, "nope\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": "not-a-dir",
        "operations": [{
            "op": "replace",
            "path": "anything.txt",
            "old": "a",
            "new": "b"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = patchloom_in(dir.path())
        .arg("--json")
        .arg("tx")
        .arg(&plan_file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "parse_error");
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("plan cwd is not a directory"));
}

#[test]
fn test_tx_cli_rejects_plan_cwd_nonexistent() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "cwd": "does-not-exist",
        "operations": [{
            "op": "replace",
            "path": "anything.txt",
            "old": "a",
            "new": "b"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .assert()
        .code(4) // PARSE_ERROR
        .stderr(predicate::str::contains("plan cwd is not a directory"));
}

#[test]
fn test_tx_lifecycle_stderr_captured_in_validation_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    // Use a command that writes to stderr before failing.
    let stderr_cmd = "echo 'CUSTOM_ERROR: something went wrong' >&2 && exit 1";
    let plan = serde_json::json!({
        "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }],
        "validate": [{
            "cmd": stderr_cmd,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("CUSTOM_ERROR: something went wrong"),
        "lifecycle error should include captured stderr: {error}"
    );
}

#[test]
fn test_tx_lifecycle_stderr_captured_in_format_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let stderr_cmd = "echo 'FMT_FAIL: bad formatting' >&2 && exit 1";
    let plan = serde_json::json!({
        "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "original",
            "new": "changed"
        }],
        "format": [{
            "cmd": stderr_cmd,
            "timeout": 5
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("FMT_FAIL: bad formatting"),
        "format step stderr should include captured output: {stderr}"
    );
}

#[test]
fn test_tx_lifecycle_stderr_truncated_when_exceeding_limit() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    // Generate >512 bytes of stderr output. Each line is ~80 chars;
    // 10 lines = ~800 bytes, well over the 512-byte capture limit.
    #[cfg(windows)]
    let stderr_cmd = r#"powershell -NoProfile -Command "1..10 | ForEach-Object { [Console]::Error.WriteLine('LONGLINE_' + $_ + '_' + ('A' * 60)) }; exit 1""#;
    #[cfg(not(windows))]
    let stderr_cmd = "for i in 1 2 3 4 5 6 7 8 9 10; do echo \"LONGLINE_${i}_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\" >&2; done && exit 1";
    let plan = serde_json::json!({
        "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "hello",
            "new": "world"
        }],
        "validate": [{
            "cmd": stderr_cmd,
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
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["error"].as_str().unwrap();
    assert!(
        error.contains("LONGLINE_1_"),
        "truncated stderr should include the beginning: {error}"
    );
    assert!(
        error.contains("(truncated)"),
        "long stderr should be marked as truncated: {error}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_rename_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn old_name() {\n    old_name();\n}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": portable_path_str(&file),
            "old": "old_name",
            "new": "new_name"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new_name"),
        "ast.rename should rename identifier: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "old identifier should be gone: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_replace_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("app.rs");
    fs::write(
        &file,
        "fn compute() {\n    let x = 42;\n    println!(\"{}\", x);\n}\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.replace",
            "path": portable_path_str(&file),
            "symbol": "compute",
            "old": "42",
            "new": "99"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("let x = 99;"),
        "ast.replace should replace 42 with 99 in compute: {content}"
    );
    assert!(
        !content.contains("let x = 42;"),
        "old value 42 should be gone: {content}"
    );
}

#[test]
fn test_tx_replace_word_boundary_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "cat concatenate category\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "cat",
            "new": "dog",
            "word_boundary": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "dog concatenate category\n",
        "word_boundary should only replace whole-word match"
    );
}

// ── file.append (CLI, tx, batch, explain) ──────────────────────

#[test]
fn test_tx_file_append_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "alpha\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.append",
            "path": portable_path_str(&file),
            "content": "beta\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "alpha\nbeta\n");
}

#[test]
fn test_tx_file_append_missing_file_fails() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.append",
            "path": nonexistent_path("append-target"),
            "content": "data\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_tx_file_prepend_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "existing content\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.prepend",
            "path": portable_path_str(&file),
            "content": "# Header\n\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "# Header\n\nexisting content\n");
}

#[test]
fn test_tx_file_prepend_missing_file_fails() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.prepend",
            "path": nonexistent_path("prepend-target"),
            "content": "data\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn test_tx_file_prepend_with_append_atomic() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("script.sh");
    fs::write(&file, "echo hello\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            { "op": "file.prepend", "path": portable_path_str(&file), "content": "#!/bin/bash\n" },
            { "op": "file.append", "path": portable_path_str(&file), "content": "echo goodbye\n" }
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "#!/bin/bash\necho hello\necho goodbye\n");
}

#[test]
fn test_tx_file_prepend_empty_content_is_noop() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.prepend",
            "path": portable_path_str(&file),
            "content": ""
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "content\n");
}

#[test]
fn test_tx_file_prepend_canonical_name() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("x.txt");
    fs::write(&file, "body\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.prepend",
            "path": portable_path_str(&file),
            "content": "header\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(fs::read_to_string(&file).unwrap(), "header\nbody\n");
}

#[test]
fn test_tx_file_append_directory_path_fails() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.append",
            "path": portable_path_str(&sub),
            "content": "data"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicates::str::contains("target is not a file"));
}

#[test]
fn test_tx_file_prepend_directory_path_fails() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("subdir");
    fs::create_dir(&sub).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.prepend",
            "path": portable_path_str(&sub),
            "content": "data"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(predicates::str::contains("target is not a file"));
}

#[test]
fn test_tx_format_flag_runs_after_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "replace", "path": "f.txt", "old": "aaa", "new": "bbb"}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .args([
            "tx",
            plan_file.to_str().unwrap(),
            "--apply",
            "--format",
            &shell_touch(&marker),
        ])
        .assert()
        .code(0);

    assert!(
        marker.exists(),
        "--format command should have run after tx --apply"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "bbb\n");
}

// ── MCP HTTP transport tests ────────────────────────────────────────

#[cfg(feature = "mcp-http")]
#[tokio::test]
async fn test_mcp_http_search_files_round_trip() {
    if !has_mcp_http_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut child = tokio::process::Command::new(&bin)
        .args(["mcp-server", "--http", "--port", "0"])
        .current_dir(dir.path())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn mcp-server --http");

    // Read stderr to discover the actual bound port
    let stderr = child.stderr.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stderr);
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
        .await
        .expect("failed to read server banner");

    // Parse "MCP HTTP server listening on http://127.0.0.1:PORT/mcp"
    let url = line.trim().rsplit("on ").next().expect("no URL in banner");
    assert!(
        url.starts_with("http://"),
        "expected http:// URL in banner: {line}"
    );

    // Connect an MCP client over Streamable HTTP
    use rmcp::ServiceExt;
    let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(url);
    let client: rmcp::service::RunningService<rmcp::RoleClient, ()> =
        ().serve(transport).await.expect("HTTP client connect");

    // List tools
    let tools = client.peer().list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(
        names.contains(&"search_files"),
        "search_files tool should be listed over HTTP"
    );

    // Call search_files
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(serde_json::json!({"pattern": "hello", "paths": ["."]}))
                .unwrap(),
        );
    let result = client.peer().call_tool(params).await.unwrap();
    assert!(
        !result.is_error.unwrap_or(false),
        "search_files should succeed"
    );
    let text = result
        .content
        .first()
        .and_then(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        text.contains("hello world"),
        "search result should contain match: {text}"
    );

    client.cancel().await.unwrap();
    child.kill().await.ok();
}

/// Verify that a write operation (replace_text) works over HTTP transport.
/// The read-only round-trip test above covers search_files; this test
/// ensures mutations also work end-to-end over Streamable HTTP.
#[cfg(feature = "mcp-http")]
#[tokio::test]
async fn test_mcp_http_replace_text_round_trip() {
    if !has_mcp_http_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old_value\n").unwrap();

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut child = tokio::process::Command::new(&bin)
        .args(["mcp-server", "--http", "--port", "0"])
        .current_dir(dir.path())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn mcp-server --http");

    let stderr = child.stderr.take().unwrap();
    let mut reader = tokio::io::BufReader::new(stderr);
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
        .await
        .expect("failed to read server banner");

    let url = line.trim().rsplit("on ").next().expect("no URL in banner");

    use rmcp::ServiceExt;
    let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(url);
    let client: rmcp::service::RunningService<rmcp::RoleClient, ()> =
        ().serve(transport).await.expect("HTTP client connect");

    // Call replace_text to mutate a file
    let params = rmcp::model::CallToolRequestParams::new("replace_text".to_string())
        .with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "target.txt",
                "old": "old_value",
                "new": "new_value"
            }))
            .unwrap(),
        );
    let result = client.peer().call_tool(params).await.unwrap();
    assert!(
        !result.is_error.unwrap_or(false),
        "replace_text should succeed over HTTP"
    );

    // Verify the file was actually mutated on disk
    let content = fs::read_to_string(dir.path().join("target.txt")).unwrap();
    assert_eq!(
        content, "new_value\n",
        "file should be mutated by HTTP tool call"
    );

    client.cancel().await.unwrap();
    child.kill().await.ok();
}

#[test]
fn test_tx_replace_range_without_whole_line_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "line2",
            "new": "replaced",
            "range": "1:3",
            "whole_line": false
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4)
        .stderr(predicates::str::contains("range requires whole_line"));

    // File must not be modified
    assert_eq!(fs::read_to_string(&file).unwrap(), "line1\nline2\nline3\n");
}

#[test]
fn test_tx_replace_whole_line_and_multiline_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "old": "line2",
            "new": "replaced",
            "whole_line": true,
            "multiline": true
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(4)
        .stderr(predicates::str::contains(
            "whole_line and multiline cannot be combined",
        ));

    // File must not be modified
    assert_eq!(fs::read_to_string(&file).unwrap(), "line1\nline2\nline3\n");
}

// ── ast.reorder (tx plan) ──────────────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_reorder_alphabetical_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn zebra() {}\nfn alpha() {}\nfn mid() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.reorder",
            "path": portable_path_str(&file),
            "order": "alphabetical"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let alpha_pos = content.find("fn alpha").expect("alpha should exist");
    let mid_pos = content.find("fn mid").expect("mid should exist");
    let zebra_pos = content.find("fn zebra").expect("zebra should exist");
    assert!(
        alpha_pos < mid_pos && mid_pos < zebra_pos,
        "symbols should be in alphabetical order: {content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_reorder_custom_order_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(&file, "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.reorder",
            "path": portable_path_str(&file),
            "order": ["c", "a", "b"]
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    let c_pos = content.find("fn c").unwrap();
    let a_pos = content.find("fn a").unwrap();
    let b_pos = content.find("fn b").unwrap();
    assert!(
        c_pos < a_pos && a_pos < b_pos,
        "custom order c,a,b should apply: {content}"
    );
}

// ── ast.group (tx plan) ────────────────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_group_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("lib.rs");
    fs::write(
        &file,
        "fn public_api() {}\n\nfn test_one() {}\n\nfn test_two() {}\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.group",
            "path": portable_path_str(&file),
            "module": "tests",
            "symbols": ["test_one", "test_two"],
            "preamble": "use super::*;"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("mod tests"),
        "should create mod tests: {content}"
    );
    assert!(
        content.contains("use super::*;"),
        "should include preamble: {content}"
    );
    assert!(
        content.contains("fn public_api"),
        "non-grouped symbol should remain: {content}"
    );
}

// ── ast.move (tx plan) ─────────────────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_move_in_plan() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.rs");
    let target = dir.path().join("target.rs");
    fs::write(&source, "fn keep() {}\nfn moveme() { let x = 1; }\n").unwrap();
    fs::write(&target, "fn existing() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.move",
            "path": portable_path_str(&source),
            "target": portable_path_str(&target),
            "symbols": ["moveme"]
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let src_content = fs::read_to_string(&source).unwrap();
    let tgt_content = fs::read_to_string(&target).unwrap();
    assert!(
        !src_content.contains("moveme"),
        "moveme should be removed from source: {src_content}"
    );
    assert!(
        src_content.contains("fn keep"),
        "keep should remain in source: {src_content}"
    );
    assert!(
        tgt_content.contains("fn moveme"),
        "moveme should appear in target: {tgt_content}"
    );
    assert!(
        tgt_content.contains("fn existing"),
        "existing should remain in target: {tgt_content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_move_creates_target() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.rs");
    let target = dir.path().join("new_target.rs");
    fs::write(&source, "fn stay() {}\nfn go() { 42 }\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.move",
            "path": portable_path_str(&source),
            "target": portable_path_str(&target),
            "symbols": ["go"],
            "target_prepend": "use crate::prelude::*;\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert!(target.exists(), "target file should be created");
    let tgt = fs::read_to_string(&target).unwrap();
    assert!(
        tgt.contains("use crate::prelude::*;"),
        "prepend should be present: {tgt}"
    );
    assert!(tgt.contains("fn go"), "symbol should be present: {tgt}");
}

// ── ast.extract_to_file (tx plan) ──────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_extract_to_file_in_plan() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("lib.rs");
    let target = dir.path().join("tests.rs");
    fs::write(
        &source,
        "fn main() {}\n\nmod tests {\n    fn test_a() {}\n}\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.extract_to_file",
            "source": portable_path_str(&source),
            "symbol": "tests",
            "target": portable_path_str(&target),
            "replacement": "mod tests;",
            "prepend": "use super::*;\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let src = fs::read_to_string(&source).unwrap();
    let tgt = fs::read_to_string(&target).unwrap();
    assert!(
        src.contains("mod tests;"),
        "replacement should be in source: {src}"
    );
    assert!(
        !src.contains("fn test_a"),
        "extracted symbol body should be removed from source: {src}"
    );
    assert!(
        tgt.contains("fn test_a"),
        "extracted content should be in target: {tgt}"
    );
    assert!(
        tgt.contains("use super::*;"),
        "prepend should be in target: {tgt}"
    );
}

// ── ast.split (tx plan) ────────────────────────────────────────

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_split_in_plan() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("big.rs");
    let types_file = dir.path().join("types.rs");
    let utils_file = dir.path().join("utils.rs");
    fs::write(&source, "struct Config {}\nfn helper() {}\nfn main() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.split",
            "source": portable_path_str(&source),
            "targets": [
                {
                    "path": portable_path_str(&types_file),
                    "symbols": ["Config"],
                    "prepend": "use super::*;\n"
                },
                {
                    "path": portable_path_str(&utils_file),
                    "symbols": ["helper"]
                }
            ],
            "keep_in_source": ["main"],
            "source_suffix": "mod types;\nmod utils;\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let src = fs::read_to_string(&source).unwrap();
    let types = fs::read_to_string(&types_file).unwrap();
    let utils = fs::read_to_string(&utils_file).unwrap();

    assert!(src.contains("fn main"), "main should stay in source: {src}");
    assert!(
        src.contains("mod types;"),
        "source_suffix should be appended: {src}"
    );
    assert!(
        !src.contains("struct Config"),
        "Config should be moved out: {src}"
    );
    assert!(
        types.contains("struct Config"),
        "Config should be in types.rs: {types}"
    );
    assert!(
        types.contains("use super::*;"),
        "prepend should be in types.rs: {types}"
    );
    assert!(
        utils.contains("fn helper"),
        "helper should be in utils.rs: {utils}"
    );
}

// ── for_each glob batch (tx plan) ──────────────────────────────

#[test]
fn test_tx_for_each_glob_expansion_with_templates() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "old_value\n").unwrap();
    fs::write(src.join("b.txt"), "old_value\n").unwrap();
    fs::write(src.join("skip.md"), "old_value\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "src/*.txt"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "old_value",
            "new": "new_value in {stem}"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let a = fs::read_to_string(src.join("a.txt")).unwrap();
    let b = fs::read_to_string(src.join("b.txt")).unwrap();
    let md = fs::read_to_string(src.join("skip.md")).unwrap();

    assert!(
        a.contains("new_value in a"),
        "template {{stem}} should expand to 'a': {a}"
    );
    assert!(
        b.contains("new_value in b"),
        "template {{stem}} should expand to 'b': {b}"
    );
    assert_eq!(
        md.trim(),
        "old_value",
        "skip.md should not be modified (non-matching glob): {md}"
    );
}

#[test]
fn test_tx_for_each_exclude_filters_files() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("keep.txt"), "original\n").unwrap();
    fs::write(src.join("skip.txt"), "original\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "src/*.txt",
            "exclude": ["src/skip.txt"]
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "original",
            "new": "changed"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let keep = fs::read_to_string(src.join("keep.txt")).unwrap();
    let skip = fs::read_to_string(src.join("skip.txt")).unwrap();

    assert!(
        keep.contains("changed"),
        "keep.txt should be modified: {keep}"
    );
    assert_eq!(
        skip.trim(),
        "original",
        "skip.txt should be excluded: {skip}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_for_each_filter_has_symbol() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("with_tests.rs"),
        "fn main() {}\nmod tests { fn test_a() {} }\n",
    )
    .unwrap();
    fs::write(src.join("no_tests.rs"), "fn helper() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "src/*.rs",
            "filter": "has_symbol(tests)"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "fn main",
            "new": "fn entry"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let with = fs::read_to_string(src.join("with_tests.rs")).unwrap();
    let without = fs::read_to_string(src.join("no_tests.rs")).unwrap();

    assert!(
        with.contains("fn entry"),
        "file with tests should be modified: {with}"
    );
    assert!(
        without.contains("fn helper"),
        "file without tests should not be modified: {without}"
    );
}

#[test]
fn test_tx_for_each_check_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "hello\n").unwrap();
    fs::write(src.join("b.txt"), "hello\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "src/*.txt"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "hello",
            "new": "world"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // --check should report changes without modifying files
    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(2); // CHANGES_DETECTED

    // Files should be unchanged
    assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(src.join("b.txt")).unwrap(), "hello\n");
}

#[test]
fn test_tx_for_each_no_matches_produces_empty_plan() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "nonexistent/**/*.xyz"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "a",
            "new": "b"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    // No matching files means no operations, success with no changes
    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);
}

#[test]
fn test_tx_for_each_escaped_braces_produce_literal() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "old value\n").unwrap();

    // `{{path}}` should become literal `{path}` in the replacement text,
    // NOT the matched file path.
    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "*.txt"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "old value",
            "new": "literal {{path}} here"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert!(
        content.contains("literal {path} here"),
        "escaped braces should produce literal braces: {content}"
    );
    assert!(
        !content.contains("literal a.txt here"),
        "escaped braces should NOT be substituted: {content}"
    );
}

#[test]
fn test_tx_for_each_mixed_escaped_and_template_vars() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.rs"), "placeholder\n").unwrap();

    // Mix of template variables (substituted) and escaped braces (literal).
    let plan = serde_json::json!({
        "version": 1,
        "for_each": {
            "glob": "*.rs"
        },
        "operations": [{
            "op": "replace",
            "path": "{path}",
            "old": "placeholder",
            "new": "file={stem} ext={ext} literal={{name}}"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(dir.path().join("test.rs")).unwrap();
    assert!(
        content.contains("file=test ext=rs literal={name}"),
        "mixed escaped and template vars: {content}"
    );
}

// ---- #1111.7: strict rollback restores collateral files ----

/// Regression (#1111.7): when a format step modifies files outside the
/// transaction and a subsequent validate step fails, strict mode should
/// restore not only the tx-tracked files but also the collateral files
/// that the formatter touched.
#[test]
fn test_tx_strict_rollback_restores_collateral_files() {
    let dir = TempDir::new().unwrap();

    // File modified by the tx.
    let tx_file = dir.path().join("target.txt");
    fs::write(&tx_file, "original tx content\n").unwrap();

    // Bystander file that the "formatter" will modify.
    let bystander = dir.path().join("bystander.txt");
    fs::write(&bystander, "original bystander content\n").unwrap();

    // The format step modifies the bystander then succeeds.
    // The validate step fails, triggering strict rollback.
    // Use relative paths so the plan cwd (the tempdir) is the walk root.
    let format_cmd = "printf 'reformatted bystander\\n' > bystander.txt";

    let plan = serde_json::json!({
        "version": 1,
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": "target.txt",
            "old": "original tx content",
            "new": "modified tx content"
        }],
        "format": [{
            "cmd": format_cmd
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
        .current_dir(dir.path())
        .arg("tx")
        .arg("plan.json")
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK

    // The tx file should be restored.
    assert_eq!(
        fs::read_to_string(&tx_file).unwrap(),
        "original tx content\n",
        "tx file should be restored on strict rollback"
    );
    // The bystander (collateral) file should also be restored.
    assert_eq!(
        fs::read_to_string(&bystander).unwrap(),
        "original bystander content\n",
        "collateral file modified by formatter should be restored on strict rollback"
    );
}

/// Verify that when strict mode is off, collateral files are NOT restored
/// (the formatter's changes are kept).
#[test]
fn test_tx_non_strict_keeps_collateral_files() {
    let dir = TempDir::new().unwrap();

    let tx_file = dir.path().join("target.txt");
    fs::write(&tx_file, "original\n").unwrap();

    let bystander = dir.path().join("bystander.txt");
    fs::write(&bystander, "untouched\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "strict": false,
        "operations": [{
            "op": "replace",
            "path": "target.txt",
            "old": "original",
            "new": "changed"
        }],
        "format": [{
            "cmd": "printf 'formatted\\n' > bystander.txt"
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
        .current_dir(dir.path())
        .arg("tx")
        .arg("plan.json")
        .arg("--apply")
        .assert()
        .code(6); // VALIDATION_FAILED, not ROLLBACK

    // In non-strict mode, the formatter's changes are kept.
    assert_eq!(
        fs::read_to_string(&bystander).unwrap(),
        "formatted\n",
        "in non-strict mode, formatter changes should persist"
    );
}

/// Global `--format` failure after commit must include a recovery hint
/// mentioning `patchloom undo` (#1159).
#[test]
fn test_tx_global_format_failure_includes_undo_hint() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "replace",
            "path": portable_path_str(&file),
            "old": "old",
            "new": "new"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .arg("--format")
        .arg("false")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("patchloom undo"),
        "error should mention `patchloom undo` for recovery: {stderr}"
    );
}

/// TX engine should resolve lang hint via lang_from_str (language names),
/// not just from_extension (#1170).
#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_list_with_lang_name_hint() {
    let dir = TempDir::new().unwrap();
    // File with non-standard extension but valid Rust code.
    let file = dir.path().join("code.txt");
    fs::write(&file, "fn hello() { let x = 1; }\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": portable_path_str(&file),
            "old": "hello",
            "new": "world",
            "lang": "rust"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("world"),
        "lang hint 'rust' should work via lang_from_str: {content}"
    );
}

// ---------------------------------------------------------------------------
// #1287: ast.rename with directory path walks recursively
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_rename_directory_walks_recursively() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("nested")).unwrap();
    fs::write(src.join("lib.rs"), "fn old_func() {\n    old_func();\n}\n").unwrap();
    fs::write(
        src.join("nested").join("mod.rs"),
        "fn old_func() -> i32 { 0 }\n",
    )
    .unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": "src",
            "old": "old_func",
            "new": "new_func"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    let lib_content = fs::read_to_string(src.join("lib.rs")).unwrap();
    assert!(
        lib_content.contains("new_func"),
        "lib.rs should contain renamed identifier: {lib_content}"
    );
    assert!(
        !lib_content.contains("old_func"),
        "lib.rs should not contain old identifier: {lib_content}"
    );

    let mod_content = fs::read_to_string(src.join("nested").join("mod.rs")).unwrap();
    assert!(
        mod_content.contains("new_func"),
        "nested/mod.rs should contain renamed identifier: {mod_content}"
    );
    assert!(
        !mod_content.contains("old_func"),
        "nested/mod.rs should not contain old identifier: {mod_content}"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_rename_directory_no_matches_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "fn keep_me() {}\n").unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": "src",
            "old": "nonexistent_func",
            "new": "renamed"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES (symbol not found)

    // File must be unchanged.
    assert_eq!(
        fs::read_to_string(src.join("lib.rs")).unwrap(),
        "fn keep_me() {}\n"
    );
}

#[test]
#[cfg(feature = "ast")]
fn test_tx_ast_rename_empty_directory_fails() {
    let dir = TempDir::new().unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": "empty",
            "old": "x",
            "new": "y"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(9); // OPERATION_FAILED
}

// ---------------------------------------------------------------------------
// CLI --contain on tx plans (MPI cycle 13)
// ---------------------------------------------------------------------------

#[test]
fn test_tx_contain_rejects_parent_escape_in_plan() {
    let dir = TempDir::new().unwrap();
    let escape_name = format!(
        "patchloom-tx-escape-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let outside = dir.path().parent().unwrap().join(&escape_name);
    let _ = fs::remove_file(&outside);

    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.create",
            "path": format!("../{escape_name}"),
            "content": "nope\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "tx"])
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("escapes")
                .or(predicate::str::contains("rejected"))
                .or(predicate::str::contains("workspace guard")),
        );

    assert!(
        !outside.exists(),
        "tx --contain must not create escaped path {outside:?}"
    );
    let _ = fs::remove_file(&outside);
}

#[test]
fn test_tx_contain_allows_in_workspace_relative_path() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.create",
            "path": "inside-tx.txt",
            "content": "ok\n"
        }]
    });
    fs::write(
        dir.path().join("plan.json"),
        serde_json::to_string(&plan).unwrap(),
    )
    .unwrap();

    // Relative plan under --contain.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "tx", "plan.json", "--apply"])
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("inside-tx.txt")).unwrap(),
        "ok\n"
    );
}

/// #1451: absolute plan path under --cwd is allowed with --contain (AllowIfContained).
#[test]
fn test_tx_contain_allows_absolute_plan_path_inside_workspace() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.create",
            "path": "abs-plan-created.txt",
            "content": "from-abs\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--contain", "tx"])
        .arg(plan_file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(dir.path().join("abs-plan-created.txt")).unwrap(),
        "from-abs\n"
    );
}

// ---------------------------------------------------------------------------
// ast.rewrite_signature (#1459) + NoMatch exit-code parity (MPI 2026-07-08)
// ---------------------------------------------------------------------------

#[cfg(feature = "ast")]
#[test]
fn test_tx_ast_rewrite_signature_apply() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn process(x: i32) {}\n").unwrap();
    let plan = r#"{
        "version": 1,
        "operations": [{
            "op": "ast.rewrite_signature",
            "path": "lib.rs",
            "old": "process",
            "parameters": "(x: u64)",
            "return_type": "-> u64"
        }]
    }"#;
    fs::write(dir.path().join("plan.json"), plan).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["tx", "plan.json", "--apply"])
        .assert()
        .code(0);

    let on_disk = fs::read_to_string(dir.path().join("lib.rs")).unwrap();
    assert!(
        on_disk.contains("u64"),
        "signature should be rewritten: {on_disk}"
    );
    assert!(
        on_disk.contains("-> u64 {"),
        "tx rewrite_signature must preserve body gap (#1503): {on_disk}"
    );
    assert!(
        !on_disk.contains("u64{"),
        "must not glue type to brace: {on_disk}"
    );
}

#[cfg(feature = "ast")]
#[test]
fn test_tx_ast_rewrite_signature_missing_function_exits_3() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn process() {}\n").unwrap();
    let plan = r#"{
        "version": 1,
        "operations": [{
            "op": "ast.rewrite_signature",
            "path": "lib.rs",
            "old": "missing_fn",
            "parameters": "(x: i32)"
        }]
    }"#;
    fs::write(dir.path().join("plan.json"), plan).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--json", "tx", "plan.json", "--apply"])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["error_kind"], "no_matches");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("missing_fn") || err.contains("not found"),
        "error should name the missing function, got: {err}"
    );
}

#[cfg(feature = "ast")]
#[test]
fn test_tx_ast_rename_missing_exits_3_with_detail() {
    // Regression: AST NoMatch used to become operation_failed (exit 9) with
    // a stripped "operation N failed" message and no symbol name.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn keep() {}\n").unwrap();
    let plan = r#"{
        "version": 1,
        "operations": [{
            "op": "ast.rename",
            "path": "lib.rs",
            "old": "gone",
            "new": "here"
        }]
    }"#;
    fs::write(dir.path().join("plan.json"), plan).unwrap();

    let out = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--cwd"])
        .arg(dir.path())
        .args(["--json", "tx", "plan.json", "--apply"])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["error_kind"], "no_matches");
    let err = v["error"].as_str().unwrap_or("");
    assert!(
        err.contains("gone") || err.contains("no matches") || err.contains("not found"),
        "error should identify the missing symbol, got: {err}"
    );
}

#[test]
fn test_tx_md_missing_file_json_not_found() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "md.replace_section",
            "path": "missing.md",
            "heading": "## H",
            "content": "body"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("tx")
        .arg(plan_file.to_str().unwrap())
        .arg("--cwd")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(
        json["error_kind"], "not_found",
        "tx missing file should be not_found not operation_failed: {json}"
    );
}

#[test]
fn test_tx_file_append_missing_json_not_found() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
        "version": 1,
        "operations": [{
            "op": "file.append",
            "path": "missing.txt",
            "content": "x\n"
        }]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "tx", plan_file.to_str().unwrap(), "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error_kind"], "not_found");
}
