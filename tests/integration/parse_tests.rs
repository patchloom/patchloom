use super::*;

#[test]
fn test_parse_subcommand_search() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_replace() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_patch() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["patch", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_md() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["md", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_doc() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_tidy() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_create() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["create", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_tx() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tx", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_subcommand_init() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_global_flag_json() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "search", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_write_flag_ensure_final_newline() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "--ensure-final-newline", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_write_flag_normalize_eol() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--normalize-eol", "lf", "--help"])
        .assert()
        .code(0);
}

#[test]
fn test_parse_unknown_subcommand_fails() {
    // Usage errors exit FAILURE (1), not CHANGES_DETECTED (2).
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("nonexistent-command")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_parse_missing_required_arg_exits_failure_not_changes_detected() {
    // Clap default is exit 2; patchloom remaps so scripts can branch on
    // CHANGES_DETECTED without false positives from usage mistakes.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["create"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("required arguments"));
}

#[test]
fn test_parse_unexpected_flag_exits_failure_not_changes_detected() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["schema", "--not-a-real-flag"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn test_parse_help_and_version_still_exit_success() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--help")
        .assert()
        .code(0)
        .stdout(predicate::str::contains("Usage"));
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--version")
        .assert()
        .code(0);
}

#[test]
fn test_parse_usage_error_with_json_emits_envelope() {
    // Agents pass --json before a bad flag/value; clap fails before dispatch,
    // but the envelope must still be machine-readable on stdout.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "schema", "--tier", "bogus"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "invalid_input");
    let err = json["error"].as_str().unwrap_or("");
    assert!(
        err.contains("invalid value") || err.contains("bogus"),
        "error should mention the bad value: {json}"
    );
    assert!(
        !err.starts_with("error: "),
        "JSON error should strip clap 'error: ' prefix: {json}"
    );
    assert!(
        !err.contains("For more information"),
        "JSON error should omit help footer: {json}"
    );
}

#[test]
fn test_parse_usage_error_with_jsonl_emits_compact_envelope() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--jsonl", "not-a-command"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let line = String::from_utf8_lossy(&output);
    assert!(
        !line.trim().contains('\n') || line.lines().count() == 1,
        "jsonl should be a single line: {line:?}"
    );
    let json: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["error_kind"], "invalid_input");
}

// ---------------------------------------------------------------------------
// doc: delete, merge, prepend, select, ensure, move, update
// ---------------------------------------------------------------------------
