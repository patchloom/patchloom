use super::*;

#[test]
fn test_completions_supported_shells() {
    for (shell, expected_marker) in [
        ("bash", "_patchloom"),
        ("zsh", "#compdef patchloom"),
        ("fish", "function __fish_patchloom_global_optspecs"),
        ("elvish", "edit:completion:arg-completer[patchloom]"),
    ] {
        Command::cargo_bin("patchloom")
            .unwrap()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected_marker));
    }
}

#[test]
fn test_glob_filters_by_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("skip.txt"), "fn main() {}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--glob")
        .arg("*.rs")
        .arg("search")
        .arg("fn main")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("keep.rs"))
        .stdout(predicate::str::contains("skip.txt").not());
}

#[test]
fn test_glob_filters_nested_relative_pattern() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/keep.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("other.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--glob")
        .arg("sub/*.txt")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "sub{}keep.txt",
            std::path::MAIN_SEPARATOR
        )))
        .stdout(predicate::str::contains("other.txt").not());
}

// ---------------------------------------------------------------------------
// --cwd flag
// ---------------------------------------------------------------------------

#[test]
fn test_cwd_search_finds_files_in_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("search")
        .arg("hello")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn test_cwd_nonexistent_directory_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg("/nonexistent/dir/12345")
        .arg("search")
        .arg("hello")
        .arg(".")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--cwd directory does not exist"));
}

// ---------------------------------------------------------------------------
// search --multiline
// ---------------------------------------------------------------------------

#[test]
fn test_quiet_suppresses_replace_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("replace")
        .arg("hello")
        .arg("--new")
        .arg("hi")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress text output, got: {stdout}"
    );

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hi world\n", "file should still be modified");
}

#[test]
fn test_quiet_suppresses_create_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("quiet_create.txt");

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress create output, got: {stdout}"
    );

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello", "file should still be created");
}

#[test]
fn test_quiet_suppresses_search_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("search_quiet.txt");
    fs::write(&file, "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("search")
        .arg("hello")
        .arg(&file)
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress search output, got: {stdout}"
    );
}

#[test]
fn test_quiet_suppresses_tidy_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("no_newline.txt");
    fs::write(&file, "missing newline").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("tidy")
        .arg("check")
        .arg(dir.path())
        .assert()
        .code(2); // CHANGES_DETECTED

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress tidy check output, got: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// search: incompatible flags
// ---------------------------------------------------------------------------

#[test]
fn test_json_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .arg("--json")
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["matches"].is_array(), "matches should be an array");
}

#[test]
fn test_version_flag() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_help_flag() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("search")
                .and(predicate::str::contains("replace"))
                .and(predicate::str::contains("doc ").or(predicate::str::contains("doc\n"))),
        );
}

#[test]
fn test_confirm_conflicts_with_apply() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace",
            "foo",
            "--new",
            "bar",
            "--confirm",
            "--apply",
            "src/",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn test_confirm_conflicts_with_check() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace",
            "foo",
            "--new",
            "bar",
            "--confirm",
            "--check",
            "src/",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn test_jsonl_output_produces_valid_json_lines() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\nhello again\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let _: serde_json::Value =
            serde_json::from_str(line).expect("each JSONL line should be valid JSON");
    }
    assert!(
        stdout.lines().count() >= 2,
        "should have at least 2 JSONL lines for 2 matches"
    );
}

#[test]
fn test_color_never_suppresses_ansi_codes() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--color=never")
        .arg("search")
        .arg("hello")
        .arg("--cwd")
        .arg(dir.path())
        .arg(".")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\x1b["),
        "ANSI escape codes should not appear with --color=never"
    );
    assert!(stdout.contains("hello"));
}

#[test]
fn test_color_always_forces_ansi_codes() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--color=always")
        .arg("search")
        .arg("hello")
        .arg("--cwd")
        .arg(dir.path())
        .arg(".")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\x1b["),
        "ANSI escape codes should appear with --color=always"
    );
}

#[test]
fn test_jsonl_files_with_matches_emits_per_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "should have one JSONL line per matched file"
    );
    for line in &lines {
        assert!(
            line["path"].is_string(),
            "each line should have a path field"
        );
    }
}

#[test]
fn test_jsonl_count_emits_per_file() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["count"], serde_json::json!(2));
    assert!(v["path"].is_string());
}

// ---------------------------------------------------------------------------
// json + count combination
// ---------------------------------------------------------------------------

#[test]
fn test_json_count_produces_valid_envelope() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\nhello\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(0);

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert_eq!(v["match_count"], serde_json::json!(3));
    assert_eq!(v["file_count"], serde_json::json!(1));
}

// ---------------------------------------------------------------------------
// tx: validate steps, doc operations in plan
// ---------------------------------------------------------------------------

#[test]
fn test_project_config_sets_write_policy_defaults() {
    let dir = TempDir::new().unwrap();

    // Create .patchloom.toml with write policy.
    fs::write(
        dir.path().join(".patchloom.toml"),
        "[write_policy]\nensure_final_newline = true\n",
    )
    .unwrap();

    // Create a file without trailing newline.
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    // Run tidy with --apply but without --ensure-final-newline flag.
    // Config should supply the default.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "fix"])
        .arg(&file)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "config should have enabled ensure_final_newline"
    );
}

#[test]
fn test_project_config_collapse_blanks() {
    let dir = TempDir::new().unwrap();

    // Create .patchloom.toml with collapse_blanks enabled.
    fs::write(
        dir.path().join(".patchloom.toml"),
        "[write_policy]\ncollapse_blanks = true\n",
    )
    .unwrap();

    // File where whole-line delete leaves consecutive blanks.
    let file = dir.path().join("test.txt");
    fs::write(&file, "keep\n\nremove\n\nalso keep\n").unwrap();

    // Run replace --whole-line without --collapse-blanks CLI flag.
    // Config should supply the default.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("remove")
        .arg("--whole-line")
        .arg("--new")
        .arg("")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(0);

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "keep\n\nalso keep\n",
        "config collapse_blanks should collapse consecutive blank lines"
    );
}

#[test]
fn test_project_config_exclude_globs() {
    let dir = TempDir::new().unwrap();

    // Create .patchloom.toml with exclude glob that filters out *.rs files.
    fs::write(
        dir.path().join(".patchloom.toml"),
        "[exclude]\nglobs = [\"*.rs\"]\n",
    )
    .unwrap();

    // Create both .rs and .txt files.
    fs::write(dir.path().join("code.rs"), "hello\n").unwrap();
    fs::write(dir.path().join("notes.txt"), "hello\n").unwrap();

    // Search should exclude .rs files and find matches only in .txt.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["search", "hello", ".", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("notes.txt"))
        .stdout(predicates::str::contains("code.rs").not());
}

// ── explain ──────────────────────────────────────────────────

#[test]
fn test_editorconfig_final_newline() {
    let dir = TempDir::new().unwrap();

    // Create .editorconfig with insert_final_newline = true
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    )
    .unwrap();

    // Create a file without trailing newline
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "file should end with a newline after editorconfig-driven fix"
    );
}

// ---------------------------------------------------------------------------
// New features: tx plan ops, --nth, --case-insensitive, multi-glob, delete,
// md insert-before-heading, file.create force, format steps, patch.apply in tx
// ---------------------------------------------------------------------------

#[test]
fn test_cli_bench_runner_validates_requested_scales() {
    let runner = fs::read_to_string(repo_root().join("benches/cli/run.sh")).unwrap();
    assert!(
        runner.contains("read -r -a SCALES <<< \"$SCALES_INPUT\""),
        "bench runner should split requested scales without re-parsing shell syntax"
    );
    assert!(
        runner.contains("small|medium|large"),
        "bench runner should allow only known benchmark scales"
    );
    assert!(
        runner.contains("invalid scale '$SCALE'"),
        "bench runner should fail fast on unexpected scale names"
    );
}

#[test]
fn test_completions_powershell() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

// ---------------------------------------------------------------------------
// completions: invalid shell name fails
// ---------------------------------------------------------------------------

#[test]
fn test_completions_invalid_shell_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["completions", "nonexistent"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}

// ---------------------------------------------------------------------------
// doc delete --check exits 2 when changes would be made
// ---------------------------------------------------------------------------

#[test]
fn test_json_error_envelope_on_doc_get_nonexistent_file() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("get")
        .arg(nonexistent_path("json-error-test.json"))
        .arg("key")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
    assert!(!json["error"].as_str().unwrap().is_empty());
}

#[test]
fn test_jsonl_error_envelope_on_delete_nonexistent_file() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("delete")
        .arg(nonexistent_path("jsonl-error-del.txt"))
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL error should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|_| panic!("expected JSON line, got: {}", lines[0]));
    assert_eq!(json["ok"], false);
    assert!(json["error"].as_str().unwrap().contains("file not found"));
}

#[test]
fn test_json_error_envelope_on_delete_nonexistent_file() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("delete")
        .arg(nonexistent_path("json-error-del.txt"))
        .arg("--apply")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
}

#[test]
fn test_json_error_envelope_on_doc_append_non_array_target() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("name")
        .arg("\"x\"")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("doc append: target at 'name' is not an array")
    );
}

#[test]
fn test_jsonl_error_envelope_on_doc_prepend_non_array_target() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("name")
        .arg("\"x\"")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL error should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0])
        .unwrap_or_else(|_| panic!("expected JSON line, got: {}", lines[0]));
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("doc prepend: target at 'name' is not an array")
    );
}

// ---------------------------------------------------------------------------
// MCP server integration tests
// ---------------------------------------------------------------------------
// These tests spawn `patchloom mcp-server` as a real subprocess and
// communicate via the MCP stdio transport, verifying end-to-end tool calls.

/// Check if the patchloom binary was built with MCP support.

#[test]
fn test_config_malformed_toml_warns_on_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".patchloom.toml"),
        "this is not valid { toml [",
    )
    .unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    // Running any command in a dir with malformed config should warn on stderr.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg("--cwd")
        .arg(dir.path())
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: malformed"));
}

/// Verify the Python agent driver's `_PATCHLOOM_SUBCOMMANDS` set stays in
/// sync with the CLI's actual subcommand list. This test exists because the
/// set drifted (missing `explain`, `undo`, `init`) and was only caught by
/// manual review in improvement cycle 3.

#[test]
fn test_verbose_flag_emits_to_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom]"));
}

#[test]
fn test_verbose_env_var_emits_to_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .env("PATCHLOOM_LOG", "1")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom]"));
}

#[test]
fn test_no_verbose_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .env_remove("PATCHLOOM_LOG")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

// ---------------------------------------------------------------------------
// Verbose coverage: verify --verbose emits diagnostic traces for key commands.
// Each test checks that the command-specific verbose prefix appears on stderr,
// ensuring the verbose! calls added in #1117 are reachable.
// ---------------------------------------------------------------------------

#[test]
fn test_verbose_replace_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("replace")
        .arg("aaa")
        .arg("--new")
        .arg("bbb")
        .arg("f.txt")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(2) // CHANGES_DETECTED: preview mode
        .stderr(predicate::str::contains("[patchloom] replace:"));
}

#[test]
fn test_verbose_tx_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "old\n").unwrap();

    let plan =
        "version: 1\noperations:\n  - op: replace\n    path: f.txt\n    old: old\n    new: new\n";
    fs::write(dir.path().join("plan.yaml"), plan).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("tx")
        .arg(dir.path().join("plan.yaml"))
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("[patchloom] tx:")
                .and(predicate::str::contains("[patchloom] tx: executing plan")),
        );
}

#[test]
fn test_verbose_create_emits_trace() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("create")
        .arg(dir.path().join("new.txt"))
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] create:"));
}

#[test]
fn test_verbose_delete_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "x\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("delete")
        .arg(dir.path().join("f.txt"))
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] delete:"));
}

#[test]
fn test_verbose_tidy_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "no newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("tidy")
        .arg("check")
        .arg(dir.path().join("f.txt"))
        .assert()
        .stderr(predicate::str::contains("[patchloom] tidy:"));
}

#[test]
fn test_verbose_batch_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();

    let batch_input = "replace f.txt aaa bbb\n";
    fs::write(dir.path().join("ops.batch"), batch_input).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("batch")
        .arg(dir.path().join("ops.batch"))
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("[patchloom] batch:"));
}

#[test]
fn test_verbose_md_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\nHello\n\n## Section\n\nBody\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("md")
        .arg("replace-section")
        .arg(dir.path().join("doc.md"))
        .arg("--heading")
        .arg("Section")
        .arg("--content")
        .arg("New body\n")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("[patchloom] md:"));
}

#[test]
fn test_verbose_patch_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();

    let diff_text = "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+LINE2\n line3\n";
    fs::write(dir.path().join("fix.patch"), diff_text).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("patch")
        .arg("apply")
        .arg(dir.path().join("fix.patch"))
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(predicate::str::contains("[patchloom] patch:"));
}

#[test]
fn test_verbose_status_emits_trace() {
    // status requires a git repo
    let dir = TempDir::new().unwrap();

    // init a git repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    fs::write(dir.path().join("f.txt"), "initial\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("status")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] status:"));
}

#[test]
fn test_verbose_read_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\nworld\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("read")
        .arg("f.txt")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] read:"));
}

#[test]
fn test_verbose_read_with_lines_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "a\nb\nc\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("read")
        .arg("f.txt")
        .arg("--lines")
        .arg("1-2")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(
            predicate::str::contains("[patchloom] read:").and(predicate::str::contains("lines=")),
        );
}

#[test]
fn test_verbose_doc_get_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("data.json"),
        r#"{"name": "test", "version": "1.0"}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("get")
        .arg("data.json")
        .arg("name")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(
            predicate::str::contains("[patchloom] doc:")
                .and(predicate::str::contains("doc: get/select")),
        );
}

#[test]
fn test_verbose_doc_set_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name": "test"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("set")
        .arg("data.json")
        .arg("name")
        .arg("updated")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("[patchloom] doc:").and(predicate::str::contains("doc: set")),
        );
}

#[test]
fn test_verbose_doc_has_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("d.json"), r#"{"a": 1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("has")
        .arg("d.json")
        .arg("a")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] doc: has"));
}

#[test]
fn test_verbose_doc_keys_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("d.json"), r#"{"a": 1, "b": 2}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("keys")
        .arg("d.json")
        .arg(".")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] doc: keys"));
}

#[test]
fn test_verbose_doc_flatten_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("d.json"), r#"{"a": {"b": 1}}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("flatten")
        .arg("d.json")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] doc: flatten"));
}

#[test]
fn test_verbose_doc_len_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("d.json"), r#"{"items": [1, 2, 3]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("len")
        .arg("d.json")
        .arg("items")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] doc: len"));
}

#[test]
fn test_verbose_doc_diff_emits_trace() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.json"), r#"{"x": 1}"#).unwrap();
    fs::write(dir.path().join("b.json"), r#"{"x": 2}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("doc")
        .arg("diff")
        .arg("a.json")
        .arg("b.json")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .stderr(predicate::str::contains("[patchloom] doc: diff"));
}

#[test]
fn test_verbose_init_emits_trace() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("init")
        .arg("--yes")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] init:"));
}

#[test]
fn test_verbose_explain_emits_trace() {
    let dir = TempDir::new().unwrap();
    let plan = "version: 1\noperations:\n  - op: file.create\n    path: f.txt\n    content: hi\n";
    fs::write(dir.path().join("plan.yaml"), plan).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("explain")
        .arg(dir.path().join("plan.yaml"))
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] explain:"));
}

#[test]
fn test_verbose_schema_emits_trace() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--verbose")
        .arg("schema")
        .arg("--quiet")
        .assert()
        .success()
        .stderr(predicate::str::contains("[patchloom] schema:"));
}

// ---------------------------------------------------------------------------
// schema command
// ---------------------------------------------------------------------------

#[test]
fn test_editorconfig_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\ntrim_trailing_whitespace = true\n",
    )
    .unwrap();

    let file = dir.path().join("messy.txt");
    fs::write(&file, "hello   \nworld  \n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello\nworld\n",
        "trailing whitespace should be trimmed"
    );
}

#[test]
fn test_editorconfig_end_of_line_crlf() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\nend_of_line = crlf\n",
    )
    .unwrap();

    let file = dir.path().join("unix.txt");
    fs::write(&file, "line1\nline2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let bytes = fs::read(&file).unwrap();
    let content = String::from_utf8_lossy(&bytes);
    assert!(
        content.contains("\r\n"),
        "EditorConfig end_of_line=crlf should convert LF to CRLF"
    );
    assert!(
        !content.contains("\n\n"),
        "should not have bare LF after CRLF conversion"
    );
}

#[test]
fn test_editorconfig_replace_apply_respects_final_newline() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*]\ninsert_final_newline = true\n",
    )
    .unwrap();

    let file = dir.path().join("noeol.txt");
    fs::write(&file, "old value").unwrap(); // no trailing newline

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old")
        .arg("--new")
        .arg("new")
        .arg(&file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.ends_with('\n'),
        "EditorConfig insert_final_newline should add trailing newline on replace"
    );
    assert!(content.contains("new value"));
}

#[test]
fn test_editorconfig_per_extension_override() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n\n[*.md]\ninsert_final_newline = true\n\n[*.txt]\ninsert_final_newline = false\n",
    )
    .unwrap();

    let md_file = dir.path().join("doc.md");
    let txt_file = dir.path().join("notes.txt");
    fs::write(&md_file, "# Title").unwrap(); // no trailing newline
    fs::write(&txt_file, "notes").unwrap(); // no trailing newline

    // Fix the .md file: should get a newline.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&md_file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let md_content = fs::read_to_string(&md_file).unwrap();
    assert!(
        md_content.ends_with('\n'),
        ".md file should get final newline from EditorConfig"
    );

    // The .txt file should NOT get a newline (insert_final_newline = false).
    // tidy fix with only --respect-editorconfig and no explicit flags should
    // not add a newline when EditorConfig says false.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&txt_file)
        .arg("--respect-editorconfig")
        .arg("--apply")
        .assert()
        .code(0);

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert!(
        !txt_content.ends_with('\n'),
        ".txt file should NOT get final newline when EditorConfig says false"
    );
}

// ---------------------------------------------------------------------------
// Concurrent MCP requests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_mcp_concurrent_doc_set() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Use separate files to avoid Windows tempfile rename races on a shared target
    // (the previous same-file version tolerated partial success for that reason).
    for key in ["a", "b", "c"] {
        fs::write(
            dir.path().join(format!("{key}.json")),
            format!(r#"{{"value":"old_{key}"}}"#),
        )
        .unwrap();
    }

    let client = spawn_mcp_client(dir.path()).await;

    // Fire three doc_set calls concurrently to *different* files.
    let params_a = rmcp::model::CallToolRequestParams::new("doc_set".to_string()).with_arguments(
        serde_json::from_value(
            serde_json::json!({"path": "a.json", "selector": "value", "value": "new_a"}),
        )
        .unwrap(),
    );
    let params_b = rmcp::model::CallToolRequestParams::new("doc_set".to_string()).with_arguments(
        serde_json::from_value(
            serde_json::json!({"path": "b.json", "selector": "value", "value": "new_b"}),
        )
        .unwrap(),
    );
    let params_c = rmcp::model::CallToolRequestParams::new("doc_set".to_string()).with_arguments(
        serde_json::from_value(
            serde_json::json!({"path": "c.json", "selector": "value", "value": "new_c"}),
        )
        .unwrap(),
    );

    let (r1, r2, r3) = tokio::join!(
        client.peer().call_tool(params_a),
        client.peer().call_tool(params_b),
        client.peer().call_tool(params_c),
    );

    // All three must succeed now that files are distinct (no shared rename race).
    r1.expect("a.json write failed");
    r2.expect("b.json write failed");
    r3.expect("c.json write failed");

    // Verify each file.
    for (key, expected_new) in [("a", "new_a"), ("b", "new_b"), ("c", "new_c")] {
        let content = fs::read_to_string(dir.path().join(format!("{key}.json"))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["value"], expected_new, "wrong value in {key}.json");
    }

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_concurrent_replace_different_files() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    for i in 0..5 {
        fs::write(dir.path().join(format!("f{i}.txt")), "old_value\n").unwrap();
    }

    let client = spawn_mcp_client(dir.path()).await;

    // Fire 5 replace calls in parallel, each targeting a different file.
    let (r0, r1, r2, r3, r4) = tokio::join!(
        call_tool_value(
            &client,
            "replace_text",
            serde_json::json!({"path": "f0.txt", "old": "old_value", "new": "new_0"}),
        ),
        call_tool_value(
            &client,
            "replace_text",
            serde_json::json!({"path": "f1.txt", "old": "old_value", "new": "new_1"}),
        ),
        call_tool_value(
            &client,
            "replace_text",
            serde_json::json!({"path": "f2.txt", "old": "old_value", "new": "new_2"}),
        ),
        call_tool_value(
            &client,
            "replace_text",
            serde_json::json!({"path": "f3.txt", "old": "old_value", "new": "new_3"}),
        ),
        call_tool_value(
            &client,
            "replace_text",
            serde_json::json!({"path": "f4.txt", "old": "old_value", "new": "new_4"}),
        ),
    );
    assert!(!r0.0, "replace on f0.txt should succeed: {}", r0.1);
    assert!(!r1.0, "replace on f1.txt should succeed: {}", r1.1);
    assert!(!r2.0, "replace on f2.txt should succeed: {}", r2.1);
    assert!(!r3.0, "replace on f3.txt should succeed: {}", r3.1);
    assert!(!r4.0, "replace on f4.txt should succeed: {}", r4.1);
    // Verify structured JSON response fields on concurrent calls.
    assert_eq!(r0.1["ok"], true, "r0 ok: {}", r0.1);
    assert_eq!(r1.1["ok"], true, "r1 ok: {}", r1.1);
    assert_eq!(r2.1["ok"], true, "r2 ok: {}", r2.1);
    assert_eq!(r3.1["ok"], true, "r3 ok: {}", r3.1);
    assert_eq!(r4.1["ok"], true, "r4 ok: {}", r4.1);

    // Verify each file was updated.
    for i in 0..5 {
        let content = fs::read_to_string(dir.path().join(format!("f{i}.txt"))).unwrap();
        assert_eq!(content, format!("new_{i}\n"), "f{i}.txt should be updated");
    }

    client.cancel().await.unwrap();
}

// ── AST surfaces integration tests (#663, #664, #665) ──

#[test]
fn test_format_flag_failure_is_reported() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();

    patchloom_in(dir.path())
        .args([
            "replace",
            "aaa",
            "--new",
            "bbb",
            "--apply",
            "--format",
            shell_false(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("format command failed"));
}
