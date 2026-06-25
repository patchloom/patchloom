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
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("nonexistent-command")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

// ---------------------------------------------------------------------------
// doc: delete, merge, prepend, select, ensure, move, update
// ---------------------------------------------------------------------------
