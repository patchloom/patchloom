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

// ---------------------------------------------------------------------------
// doc: delete, merge, prepend, select, ensure, move, update
// ---------------------------------------------------------------------------
