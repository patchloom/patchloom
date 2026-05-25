use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Convert a path to a string safe for embedding in YAML/TOML values.
/// On Windows, backslashes in paths like `C:\Users\...` are interpreted
/// as escape sequences (`\U` = unicode escape). Forward slashes work
/// fine on Windows and avoid the problem.
fn portable_path_str(p: &Path) -> String {
    p.to_str().unwrap().replace('\\', "/")
}

fn nonexistent_path(name: &str) -> String {
    #[cfg(windows)]
    {
        format!("C:\\patchloom-nonexistent-{name}")
    }
    #[cfg(not(windows))]
    {
        format!("/tmp/patchloom-nonexistent-{name}")
    }
}

fn shell_false() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "false"
    }
}

fn shell_exit_1() -> &'static str {
    #[cfg(windows)]
    {
        "exit /b 1"
    }
    #[cfg(not(windows))]
    {
        "exit 1"
    }
}

fn shell_sleep_300() -> &'static str {
    #[cfg(windows)]
    {
        // ping localhost 301 times (1 second apart) to sleep ~300 seconds.
        // Redirect to nul to suppress output.
        "ping -n 301 127.0.0.1 > nul"
    }
    #[cfg(not(windows))]
    {
        "sleep 300"
    }
}

fn shell_touch(path: &Path) -> String {
    #[cfg(windows)]
    {
        let path = path.to_str().unwrap();
        // type nul produces empty output; > redirects it to create the file.
        format!("type nul > \"{path}\"")
    }
    #[cfg(not(windows))]
    {
        let path = path.display();
        format!("touch '{path}'")
    }
}

fn shell_fail_with_secret(secret: &str) -> String {
    #[cfg(windows)]
    {
        format!("cmd /C \"set PATCHLOOM_SECRET={secret}&& exit /b 1\"")
    }
    #[cfg(not(windows))]
    {
        format!("PATCHLOOM_SECRET='{secret}' false")
    }
}

fn shell_test_exists(path: &Path) -> String {
    #[cfg(windows)]
    {
        let path = path.to_str().unwrap();
        format!("if exist \"{path}\" (exit /b 0) else (exit /b 1)")
    }
    #[cfg(not(windows))]
    {
        let path = path.display();
        format!("test -f '{path}'")
    }
}

#[cfg(unix)]
fn run_patchloom_confirm_in_pty_with_env(
    args: &[&str],
    input: &str,
    env: &[(&str, &str)],
) -> std::process::Output {
    let python = r#"
import json, os, pty, subprocess, sys

args = json.loads(os.environ["PATCHLOOM_PTY_ARGS"])
cwd = os.environ["PATCHLOOM_PTY_CWD"]
input_data = os.environ["PATCHLOOM_PTY_INPUT"].encode()
child_env = os.environ.copy()
child_env.update(json.loads(os.environ["PATCHLOOM_PTY_ENV"]))

master_fd, slave_fd = pty.openpty()
proc = subprocess.Popen(
    args,
    cwd=cwd,
    stdin=slave_fd,
    stdout=slave_fd,
    stderr=slave_fd,
    env=child_env,
)
os.close(slave_fd)
if input_data:
    os.write(master_fd, input_data)

output = bytearray()
while True:
    try:
        chunk = os.read(master_fd, 4096)
        if not chunk:
            break
        output.extend(chunk)
    except OSError:
        break

status = proc.wait()
os.close(master_fd)
sys.stdout.buffer.write(output)
sys.exit(status)
"#;
    let full_args = std::iter::once(env!("CARGO_BIN_EXE_patchloom").to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    let env_json = env
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();

    std::process::Command::new("python3")
        .arg("-c")
        .arg(python)
        .env(
            "PATCHLOOM_PTY_ARGS",
            serde_json::to_string(&full_args).unwrap(),
        )
        .env("PATCHLOOM_PTY_CWD", repo_root())
        .env("PATCHLOOM_PTY_INPUT", input)
        .env(
            "PATCHLOOM_PTY_ENV",
            serde_json::to_string(&env_json).unwrap(),
        )
        .output()
        .unwrap()
}

#[cfg(unix)]
fn run_patchloom_confirm_in_pty(args: &[&str], input: &str) -> std::process::Output {
    run_patchloom_confirm_in_pty_with_env(args, input, &[])
}

fn assert_patch_apply_error_object(
    output: &std::process::Output,
    expected_exit_code: i32,
    expected_error_substring: &str,
) {
    assert_eq!(output.status.code(), Some(expected_exit_code));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains(expected_error_substring)
    );
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:{}\nstderr:{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo_with_committed_file(dir: &Path, file: &str, content: &str) {
    git_ok(dir, &["init"]);
    git_ok(dir, &["config", "user.email", "test@test.com"]);
    git_ok(dir, &["config", "user.name", "Test"]);
    fs::write(dir.join(file), content).unwrap();
    git_ok(dir, &["add", file]);
    git_ok(dir, &["commit", "-m", "init"]);
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

#[test]
fn test_search_finds_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"));
}

#[test]
fn test_search_no_matches_exit_3() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("nonexistent_xyz")
        .arg(dir.path())
        .assert()
        .code(3);
}

#[test]
fn test_search_jsonl_output_has_path_field() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path field must be a string");
    assert!(
        parsed["path"].as_str().unwrap().contains("hello.txt"),
        "path should contain filename"
    );
    assert!(parsed["line"].is_number(), "line field must be a number");
}

#[test]
fn test_search_json_output_reports_line_and_column_without_context() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("positions.txt"),
        "skip\nalpha needle\nneedle suffix\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("needle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let matches = parsed["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["line"], 2);
    assert_eq!(matches[0]["column"], 7);
    assert_eq!(matches[1]["line"], 3);
    assert_eq!(matches[1]["column"], 1);
}

#[test]
fn test_search_jsonl_count_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "aaa\naaa\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--count")
        .arg("aaa")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path must be a string");
    assert_eq!(parsed["count"], 2, "should find 2 matches");
}

#[test]
fn test_search_jsonl_files_with_matches_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("match.txt"), "needle\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--files-with-matches")
        .arg("needle")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert!(parsed["path"].is_string(), "path must be a string");
    assert!(
        parsed["path"].as_str().unwrap().contains("match.txt"),
        "should contain filename"
    );
}

#[test]
fn test_search_json_no_match_emits_valid_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("zzz_no_match_zzz")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["match_count"], 0);
    assert_eq!(parsed["file_count"], 0);
    assert!(parsed["matches"].as_array().unwrap().is_empty());
}

#[test]
fn test_replace_json_no_match_emits_valid_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("zzz_no_match_zzz")
        .arg("--to")
        .arg("replacement")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "should exit 3 (no matches)");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["match_count"], 0);
    assert_eq!(parsed["file_count"], 0);
    assert!(parsed["files"].as_array().unwrap().is_empty());
}

#[test]
fn test_create_rejects_content_and_stdin_together() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("dual-source.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("inline")
        .arg("--stdin")
        .write_stdin("stdin-data\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--content and --stdin cannot be combined",
        ));

    assert!(
        !file.exists(),
        "file should not be created on invalid input"
    );
}

// ---------------------------------------------------------------------------
// completions
// ---------------------------------------------------------------------------

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
fn test_agent_rules_outputs_markdown() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("agent-rules")
        .assert()
        .success()
        .stdout(predicates::str::contains("# Patchloom"))
        .stdout(predicates::str::contains(
            "## Batching (the main speed win)",
        ))
        .stdout(predicates::str::contains("## Structured edits"));
}

#[test]
fn test_agent_rules_includes_version() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("agent-rules")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Version from Cargo.toml is embedded in the output
    assert!(stdout.contains("Generated by patchloom v"));
    // Should not contain the raw template placeholder
    assert!(!stdout.contains("{{VERSION}}"));
}

#[test]
fn test_agent_rules_mode_cli_omits_mcp() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--mode", "cli"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## Batching"))
        .stdout(predicates::str::contains("## Structured edits"))
        .stdout(predicates::str::contains("## MCP mode").not());
}

#[test]
fn test_agent_rules_mode_mcp_omits_cli() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--mode", "mcp"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## MCP mode"))
        .stdout(predicates::str::contains("## Batching").not())
        .stdout(predicates::str::contains("## Structured edits").not());
}

#[test]
fn test_agent_rules_platform_linux_omits_windows() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--platform", "linux"])
        .assert()
        .success()
        .stdout(predicates::str::contains("<<'EOF'"))
        .stdout(predicates::str::contains("batch ops.txt").not());
}

#[test]
fn test_agent_rules_platform_windows_omits_heredoc() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--platform", "windows"])
        .assert()
        .success()
        .stdout(predicates::str::contains("batch ops.txt"))
        .stdout(predicates::str::contains("<<'EOF'").not());
}

#[test]
fn test_agent_rules_mode_and_platform_compose() {
    // --mode mcp should have no CLI sections regardless of platform
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--mode", "mcp", "--platform", "windows"])
        .assert()
        .success()
        .stdout(predicates::str::contains("## MCP mode"))
        .stdout(predicates::str::contains("## Batching").not())
        .stdout(predicates::str::contains("batch ops.txt").not());
}

// ---------------------------------------------------------------------------
// init command
// ---------------------------------------------------------------------------

#[test]
fn test_init_creates_agents_md_in_empty_dir() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let agents = dir.path().join("AGENTS.md");
    assert!(agents.exists(), "AGENTS.md should be created");
    let content = fs::read_to_string(&agents).unwrap();
    assert!(content.contains("patchloom"));
    assert!(content.contains("# Patchloom"));
}

#[test]
fn test_init_appends_to_existing_agents_md() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "# My Rules\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let content = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(
        content.starts_with("# My Rules\n"),
        "original content should be preserved"
    );
    assert!(
        content.contains("# Patchloom"),
        "patchloom rules should be appended"
    );
}

#[test]
fn test_init_appends_when_existing_agents_mentions_patchloom_without_generated_header() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("AGENTS.md"),
        "# Rules\nUse patchloom for edits.\n",
    )
    .unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("SHELL", "unknown")
        .output()
        .unwrap();
    assert!(output.status.success());
    let content = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(content.starts_with("# Rules\nUse patchloom for edits.\n"));
    assert!(content.contains("# Patchloom"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("appended patchloom rules"));
}

#[test]
fn test_init_skips_if_patchloom_already_present() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("AGENTS.md"),
        "# Rules\n<!-- Generated by patchloom v0.1.0 -->\n",
    )
    .unwrap();
    let before = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let after = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert_eq!(before, after, "file should not be modified");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already contains patchloom rules"));
}

#[test]
fn test_init_errors_on_non_utf8_existing_agents_md() {
    let dir = TempDir::new().unwrap();
    let agents = dir.path().join("AGENTS.md");
    fs::write(&agents, [0xff, 0xfe, 0x00]).unwrap();
    let before = fs::read(&agents).unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("SHELL", "unknown")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        fs::read(&agents).unwrap(),
        before,
        "file should not be modified"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reading existing"));
}

#[test]
fn test_init_appends_to_existing_claude_md() {
    let dir = TempDir::new().unwrap();
    let claude = dir.path().join("Claude.md");
    fs::write(&claude, "# Claude Rules\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("SHELL", "unknown")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!dir.path().join("AGENTS.md").exists());
    let content = fs::read_to_string(&claude).unwrap();
    assert!(content.starts_with("# Claude Rules\n"));
    assert!(content.contains("# Patchloom"));
}

#[test]
fn test_init_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--quiet", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    // File should still be created
    assert!(dir.path().join("AGENTS.md").exists());
    // But stderr should be empty
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "stderr should be empty with --quiet");
}

#[test]
fn test_init_falls_back_to_completion_command_when_completion_dir_creation_fails() {
    let dir = TempDir::new().unwrap();
    let fake_home = dir.path().join("fake-home");
    let blocking_file = fake_home.join(".config");
    fs::create_dir_all(&fake_home).unwrap();
    fs::write(&blocking_file, "not a directory\n").unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("HOME", &fake_home)
        .env("SHELL", "fish")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to prepare completion directory:"));
    assert!(
        stderr.contains("patchloom completions fish > ~/.config/fish/completions/patchloom.fish")
    );
}

#[cfg(unix)]
#[test]
fn test_init_confirm_eof_skips_agents_creation() {
    let dir = TempDir::new().unwrap();
    let output = run_patchloom_confirm_in_pty_with_env(
        &["init", "--cwd", dir.path().to_str().unwrap()],
        "\u{4}",
        &[("SHELL", "unknown")],
    );

    assert!(output.status.success());
    assert!(!dir.path().join("AGENTS.md").exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Create AGENTS.md? [Y/n]"));
    assert!(stdout.contains("skipped AGENTS.md"));
}

#[cfg(feature = "mcp")]
#[test]
fn test_init_shows_vscode_mcp_json_hint() {
    if !has_mcp_support() {
        return;
    }

    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".vscode")).unwrap();
    let home = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("HOME", home.path())
        .env("SHELL", "unknown")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("VS Code: create .vscode/mcp.json:"));
    assert!(stderr.contains(
        "\"servers\": { \"patchloom\": { \"command\": \"patchloom\", \"args\": [\"mcp-server\"] } }"
    ));
    assert!(!stderr.contains(".vscode/settings.json"));
}

#[cfg(feature = "mcp")]
#[test]
fn test_init_shows_cursor_mcp_json_hint() {
    if !has_mcp_support() {
        return;
    }

    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".cursor")).unwrap();
    let home = TempDir::new().unwrap();
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--yes", "--cwd"])
        .arg(dir.path())
        .env("HOME", home.path())
        .env("SHELL", "unknown")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cursor: create .cursor/mcp.json:"));
    assert!(stderr.contains(
        "\"servers\": { \"patchloom\": { \"command\": \"patchloom\", \"args\": [\"mcp-server\"] } }"
    ));
}

// ---------------------------------------------------------------------------
// --glob flag
// ---------------------------------------------------------------------------

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
        .stdout(predicate::str::contains("sub/keep.txt"))
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
        .failure();
}

// ---------------------------------------------------------------------------
// search --multiline
// ---------------------------------------------------------------------------

#[test]
fn test_search_assert_count_exact_match() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    // 3 matching lines total: 2 in a.txt + 1 in b.txt
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn test_search_assert_count_mismatch() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .assert()
        .code(2);
}

#[test]
fn test_search_assert_count_zero() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "no match here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("0")
        .arg("zzz")
        .arg(dir.path())
        .assert()
        .success();
}

#[test]
fn test_search_assert_count_zero_but_found() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("0")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .code(2);
}

#[test]
fn test_search_assert_count_json_success_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["assert_count"]["expected"], 3);
    assert_eq!(json["assert_count"]["actual"], 3);
    assert_eq!(json["assert_count"]["matched"], true);
}

#[test]
fn test_search_assert_count_json_mismatch_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["assert_count"]["expected"], 5);
    assert_eq!(json["assert_count"]["actual"], 2);
    assert_eq!(json["assert_count"]["matched"], false);
}

#[test]
fn test_search_assert_count_jsonl_success_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("3")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["assert_count"]["expected"], 3);
    assert_eq!(json["assert_count"]["actual"], 3);
    assert_eq!(json["assert_count"]["matched"], true);
}

#[test]
fn test_search_assert_count_jsonl_mismatch_returns_structured_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("search")
        .arg("--literal")
        .arg("--assert-count")
        .arg("5")
        .arg("foo")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["assert_count"]["expected"], 5);
    assert_eq!(json["assert_count"]["actual"], 2);
    assert_eq!(json["assert_count"]["matched"], false);
}

#[test]
fn test_search_multiline_spans_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg(r"fn main\(\).*\}")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fn main()"))
        .stdout(predicate::str::contains("}"));
}

#[test]
fn test_search_multiline_files_with_matches_and_assert_count_counts_all_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("multi.txt");
    fs::write(&file, "foo\nbar\nfoo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg("--files-with-matches")
        .arg("--assert-count")
        .arg("2")
        .arg("foo")
        .arg(&file)
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// replace
// ---------------------------------------------------------------------------

#[test]
fn test_replace_apply_modifies_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old_text content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old_text")
        .arg("--to")
        .arg("new_text")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new_text"), "file should contain new_text");
    assert!(
        !content.contains("old_text"),
        "file should not contain old_text"
    );
}

#[test]
fn test_replace_dry_run_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old_text content\n").unwrap();

    // Without --apply, patchloom should show a diff but NOT modify the file.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old_text")
        .arg("--to")
        .arg("new_text")
        .arg(&file)
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("old_text"),
        "file should be unchanged in dry-run mode"
    );
    assert!(
        !content.contains("new_text"),
        "file should not be modified in dry-run mode"
    );
}

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
        .success();

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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "line1\nnew line\nline3\n");
}

#[test]
fn test_replace_if_exists_no_match_exit_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent")
        .arg("--to")
        .arg("new")
        .arg("--if-exists")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// --quiet flag
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
        .arg("--to")
        .arg("hi")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

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
        .success();

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
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.is_empty(),
        "quiet mode should suppress search output, got: {stdout}"
    );
}

#[test]
fn test_search_json_quiet_still_emits_structured_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("search_quiet_json.txt");
    fs::write(&file, "hello world\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--quiet")
        .arg("search")
        .arg("hello")
        .arg(&file)
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["match_count"], 1);
    assert_eq!(parsed["file_count"], 1);
}

#[test]
fn test_search_count_returns_success_on_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("count_exit.txt");
    fs::write(&file, "hello world\nhello again\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains(":2"));
}

#[test]
fn test_search_files_with_matches_returns_success_on_match() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("fwm_exit.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("fwm_exit.txt"));
}

#[test]
fn test_search_count_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("count_no_match.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("zzz_no_match_zzz")
        .arg(&file)
        .assert()
        .code(3);
}

#[test]
fn test_search_files_with_matches_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("fwm_no_match.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--files-with-matches")
        .arg("zzz_no_match_zzz")
        .arg(&file)
        .assert()
        .code(3);
}

#[test]
fn test_search_nonexistent_path_warns_on_stderr() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("hello")
        .arg("totally_nonexistent_dir/")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("No such file or directory"));
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
fn test_search_invert_match_multiline_rejected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--multiline")
        .arg("-v")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--invert-match and --multiline"));
}

#[test]
fn test_search_count_and_files_with_matches_are_rejected_together() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\nhello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--count")
        .arg("--files-with-matches")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot be used with")
                .and(predicate::str::contains("--count"))
                .and(predicate::str::contains("--files-with-matches")),
        );
}

#[test]
fn test_search_literal_and_regex_are_rejected_together() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("--regex")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("cannot be used with")
                .and(predicate::str::contains("--literal"))
                .and(predicate::str::contains("--regex")),
        );
}

// ---------------------------------------------------------------------------
// replace: invalid regex
// ---------------------------------------------------------------------------

#[test]
fn test_replace_empty_from_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("")
        .arg("--to")
        .arg("X")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .failure();

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_tx_replace_empty_from_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "",
            "to": "X"
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
fn test_replace_invalid_regex_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg("[invalid(regex")
        .arg("--to")
        .arg("x")
        .arg(&file)
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// replace --multiline
// ---------------------------------------------------------------------------

#[test]
fn test_replace_multiline_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg("--multiline")
        .arg(r"fn main\(\) \{.*\}")
        .arg("--to")
        .arg("fn main() { /* replaced */ }")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("/* replaced */"),
        "multiline replace should span newlines"
    );
}

// ---------------------------------------------------------------------------
// doc
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_jsonl_compound_value_is_single_line_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"obj":{"name":"patchloom","version":1}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("obj")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["name"], "patchloom");
    assert_eq!(json["version"], 1);
}

#[test]
fn test_doc_get_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stdout.is_empty(),
        "quiet should suppress doc get output"
    );
}

#[test]
fn test_doc_get_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_has_existing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom","version":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(predicate::str::contains("true"));
}

#[test]
fn test_doc_has_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("has")
        .arg(&file)
        .arg("missing")
        .assert()
        .success()
        .stdout(predicate::str::contains("false"));
}

#[test]
fn test_doc_keys_jsonl_outputs_one_key_per_line() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"scripts":{"build":"tsc","lint":"eslint"}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg("scripts")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|v| v == "build"));
    assert!(lines.iter().any(|v| v == "lint"));
}

#[test]
fn test_doc_keys_lists_object_keys() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"alpha":1,"beta":2,"gamma":3}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("keys")
        .arg(&file)
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma"));
}

#[test]
fn test_doc_set_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("2.0"));
}

#[cfg(unix)]
#[test]
fn test_doc_set_confirm_eof_does_not_modify_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "doc",
            "set",
            file.to_str().unwrap(),
            "version",
            "\"2.0\"",
            "--confirm",
        ],
        "\u{4}",
    );

    assert!(output.status.success());
    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["version"], serde_json::json!("1.0"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Apply? [Y/n]"));
}

#[test]
fn test_doc_set_preserves_key_order() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    // Keys are intentionally NOT in alphabetical order.
    fs::write(&file, r#"{"z_last":1,"a_first":2,"m_middle":3}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("a_first")
        .arg("99")
        .arg("--apply")
        .assert()
        .success();

    // The written file must keep keys in the original insertion order,
    // not sorted alphabetically. If serde_json's preserve_order feature
    // is missing, keys would appear as a_first, m_middle, z_last.
    let content = fs::read_to_string(&file).unwrap();
    let z_pos = content.find("z_last").expect("z_last missing");
    let a_pos = content.find("a_first").expect("a_first missing");
    let m_pos = content.find("m_middle").expect("m_middle missing");
    assert!(
        z_pos < a_pos && a_pos < m_pos,
        "key order not preserved: z_last@{z_pos}, a_first@{a_pos}, m_middle@{m_pos}"
    );
}

#[test]
fn test_doc_set_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\n[server]\nhost = \"localhost\"\nport = 8080\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("server.port")
        .arg("9090")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    // Comments must survive.
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# DB"),
        "section comment stripped: {content}"
    );
    // Value must be updated.
    assert!(content.contains("9090"), "new value missing: {content}");
    assert!(!content.contains("8080"), "old value present: {content}");
    // Section order must be preserved.
    let server_pos = content.find("[server]").expect("[server] missing");
    let db_pos = content.find("[database]").expect("[database] missing");
    assert!(
        server_pos < db_pos,
        "section order changed: server@{server_pos} db@{db_pos}"
    );
}

#[test]
fn test_doc_merge_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\n\n[server]\nhost = \"localhost\"\nport = 8080 # default\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"logging": "debug"}"#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# default"),
        "inline comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(content.contains("logging"), "merged key missing: {content}");
}

#[test]
fn test_doc_delete_toml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "# Main config\nname = \"my-app\"\nversion = 1\n\n# Server\n[server]\nhost = \"localhost\"\nport = 8080\n\n# DB\n[database]\nurl = \"pg\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("version")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("# Main config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(
        !content.contains("version"),
        "deleted key still present: {content}"
    );
    assert!(content.contains("name"), "surviving key missing: {content}");
}

#[test]
fn test_doc_set_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("server.port")
        .arg("9090")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    // Output must be syntactically valid YAML.
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    // Comments must survive.
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
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    // Value must be updated.
    assert!(content.contains("9090"), "new value missing: {content}");
    assert!(!content.contains("8080"), "old value present: {content}");
    // Key order must be preserved.
    let server_pos = content.find("server:").expect("server: missing");
    let db_pos = content.find("database:").expect("database: missing");
    assert!(
        server_pos < db_pos,
        "key order changed: server@{server_pos} db@{db_pos}"
    );
}

#[test]
fn test_doc_merge_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"logging": "debug"}"#)
        .arg("--apply")
        .assert()
        .success();

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
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(content.contains("logging"), "merged key missing: {content}");
    assert!(content.contains("debug"), "merged value missing: {content}");
}

#[test]
fn test_doc_delete_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Main config\nname: my-app\nversion: 1\n\n# Server\nserver:\n  host: localhost\n  port: 8080 # default\n\n# DB\ndatabase:\n  url: pg\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("version")
        .arg("--apply")
        .assert()
        .success();

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
    assert!(content.contains("# DB"), "DB comment stripped: {content}");
    assert!(
        !content.contains("version:"),
        "deleted key still present: {content}"
    );
    assert!(
        content.contains("name: my-app"),
        "surviving key missing: {content}"
    );
}

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
            "version: \"1\"\noperations:\n  - op: doc.set\n    path: {}\n    selector: server.port\n    value: \"9090\"\n",
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
        .success();

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
fn test_doc_append_yaml_sequence_root() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("")
        .arg("item3")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(content.contains("# Items"), "comment stripped: {content}");
    assert!(content.contains("item1"), "item1 missing: {content}");
    assert!(content.contains("item2"), "item2 missing: {content}");
    assert!(
        content.contains("item3"),
        "appended item3 missing: {content}"
    );
}

#[test]
fn test_doc_set_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items list\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("[1]")
        .arg("updated")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Items list"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("item1"),
        "unchanged element lost: {content}"
    );
    assert!(
        content.contains("updated"),
        "updated element missing: {content}"
    );
}

#[test]
fn test_doc_delete_where_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(
        &file,
        "# Contact links\n- name: keep\n  url: keep.com\n- name: remove\n  url: remove.com\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Contact links"),
        "top comment stripped: {content}"
    );
    assert!(content.contains("keep"), "kept element missing: {content}");
    assert!(
        !content.contains("remove"),
        "removed element still present: {content}"
    );
}

#[test]
fn test_doc_prepend_yaml_produces_valid_output() {
    // Verifies that prepend produces valid YAML with comments preserved.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "# Config\nname: app\nitems:\n  - existing\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("\"first\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let parsed: serde_json::Value =
        serde_yaml_ng::from_str(&content).expect("output is not valid YAML");
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "first", "prepended item not at position 0");
    assert_eq!(items[1], "existing", "original item not at position 1");
}

#[test]
fn test_doc_update_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nitems:\n  - name: a\n    status: pending # TODO\n  - name: b\n    status: pending\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[*].status")
        .arg("done")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(content.contains("done"), "updated value missing: {content}");
    assert!(
        !content.contains("pending"),
        "old value still present: {content}"
    );
}

#[test]
fn test_doc_ensure_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Server\nserver:\n  host: localhost\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("server.port")
        .arg("8080")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(content.contains("8080"), "ensured value missing: {content}");
    assert!(
        content.contains("name: my-app"),
        "existing key missing: {content}"
    );
}

#[test]
fn test_doc_move_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nold_name: my-app\n\n# Server\nserver:\n  host: localhost\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("old_name")
        .arg("new_name")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Server"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("new_name"),
        "renamed key missing: {content}"
    );
    assert!(
        !content.contains("old_name"),
        "old key still present: {content}"
    );
    assert!(
        content.contains("my-app"),
        "value lost during move: {content}"
    );
}

#[test]
fn test_doc_prepend_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - existing\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("first")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("first"),
        "prepended item missing: {content}"
    );
    assert!(
        content.contains("existing"),
        "original item missing: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "first", "prepended item not at position 0");
    assert_eq!(items[1], "existing", "original item not at position 1");
}

#[test]
fn test_doc_delete_where_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - name: keep\n    val: 1\n  - name: remove\n    val: 2\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    assert!(
        content.contains("keep"),
        "surviving item missing: {content}"
    );
    assert!(
        !content.contains("remove"),
        "deleted item still present: {content}"
    );
}

#[test]
fn test_doc_append_yaml_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "# Config\nname: my-app\n\n# Items\nitems:\n  - existing\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("items")
        .arg("last")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Config"),
        "top comment stripped: {content}"
    );
    assert!(
        content.contains("# Items"),
        "section comment stripped: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let items = parsed.get("items").expect("items key missing");
    assert_eq!(items[0], "existing", "original item not at position 0");
    assert_eq!(items[1], "last", "appended item not at position 1");
}

#[test]
fn test_doc_prepend_yaml_sequence_root_preserves_comments() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("items.yaml");
    fs::write(&file, "# Items list\n- item1\n- item2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("")
        .arg("item0")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content)
        .expect("CST output is not valid YAML");
    assert!(
        content.contains("# Items list"),
        "comment stripped: {content}"
    );
    let parsed: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap();
    let arr = parsed.as_array().expect("root should be array");
    assert_eq!(arr[0], "item0", "prepended item not at position 0");
    assert_eq!(arr[1], "item1", "original item1 not at position 1");
    assert_eq!(arr[2], "item2", "original item2 not at position 2");
}

#[test]
fn test_doc_delete_where() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"name":"keep"},{"name":"remove"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete-where")
        .arg(&file)
        .arg("items")
        .arg("--predicate")
        .arg("name=remove")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], serde_json::json!("keep"));
}

// ---------------------------------------------------------------------------
// md
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(
        &file,
        "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new content"),
        "should contain new content"
    );
    assert!(
        !content.contains("old content"),
        "should not contain old content"
    );
    assert!(
        content.contains("## Other"),
        "Other section heading should be intact"
    );
    assert!(
        content.contains("kept"),
        "Other section content should be intact"
    );
}

#[test]
fn test_md_table_append() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "## Table\n\n| H1 | H2 |\n|---|---|\n| A | B |\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("table-append")
        .arg(&file)
        .arg("--heading")
        .arg("## Table")
        .arg("--row")
        .arg("| new | row |")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("| new | row |"),
        "new row should be present"
    );
    assert!(
        content.contains("| A | B |"),
        "existing row should be preserved"
    );
}

#[test]
fn test_md_insert_after_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readme.md");
    fs::write(&file, "# Title\n\nExisting content\n\n## Other\n\nMore\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-after-heading")
        .arg(&file)
        .arg("--heading")
        .arg("# Title")
        .arg("--content")
        .arg("Inserted line")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("Inserted line"),
        "inserted content should appear"
    );
    assert!(
        content.contains("Existing content"),
        "existing content should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_adds_new() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("new rule")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new rule"), "new bullet should be added");
    assert!(
        content.contains("- existing rule"),
        "existing bullet should be preserved"
    );
}

#[test]
fn test_md_upsert_bullet_skips_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("notes.md");
    fs::write(&file, "## Rules\n\n- existing rule\n").unwrap();

    // Upsert the same bullet; should be idempotent.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(&file)
        .arg("--heading")
        .arg("## Rules")
        .arg("--bullet")
        .arg("existing rule")
        .arg("--check")
        .assert()
        .success(); // no changes -> exit 0
}

// ---------------------------------------------------------------------------
// tidy
// ---------------------------------------------------------------------------

#[test]
fn test_tidy_check_detects_missing_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_tidy_fix_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "file should end with a newline after fix"
    );
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

// ── read command ────────────────────────────────────────────────────

#[test]
fn test_read_prints_file_contents() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("hello.txt");
    fs::write(&file, "line1\nline2\nline3\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .assert()
        .success()
        .stdout("line1\nline2\nline3\n");
}

#[test]
fn test_read_lines_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("five.txt");
    fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("2:4")
        .assert()
        .success()
        .stdout("b\nc\nd");
}

#[test]
fn test_read_nonexistent_file_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(nonexistent_path("file-xyz"))
        .assert()
        .code(1);
}

#[test]
fn test_read_invalid_lines_returns_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("0:1")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("line numbers are 1-based"));
}

#[test]
fn test_read_json_invalid_lines_returns_error_object() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("0:1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("invalid --lines value '0:1'"));
    assert!(error.contains("line numbers are 1-based"));
}

#[test]
fn test_read_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_lines"], 2);
    assert_eq!(json["start_line"], 1);
    assert_eq!(json["end_line"], 2);
    assert!(json["content"].as_str().unwrap().contains("hello"));
}

#[test]
fn test_read_json_lines_start_past_eof_clamps_metadata() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("short.txt");
    fs::write(&file, "a\nb\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(file.to_str().unwrap())
        .arg("--lines")
        .arg("5:9")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["content"].as_str().unwrap(), "");
    assert_eq!(json["total_lines"], 2);
    assert_eq!(json["start_line"], 0);
    assert_eq!(json["end_line"], 0);
}

#[test]
fn test_read_respects_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("inner.txt"), "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("read")
        .arg("inner.txt")
        .assert()
        .success()
        .stdout("content\n");
}

#[test]
fn test_read_multiple_files() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("a.txt");
    let f2 = dir.path().join("b.txt");
    fs::write(&f1, "alpha\n").unwrap();
    fs::write(&f2, "beta\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicates::str::contains("==> "))
        .stdout(predicates::str::contains("alpha"))
        .stdout(predicates::str::contains("beta"));
}

#[test]
fn test_read_multiple_files_json() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("x.txt");
    let f2 = dir.path().join("y.txt");
    fs::write(&f1, "one\n").unwrap();
    fs::write(&f2, "two\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 2);
    assert!(json[0]["content"].as_str().unwrap().contains("one"));
    assert!(json[1]["content"].as_str().unwrap().contains("two"));
}

#[test]
fn test_read_multiple_files_json_partial_failure_keeps_array() {
    let dir = TempDir::new().unwrap();
    let existing = dir.path().join("exists.txt");
    fs::write(&existing, "hello\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(existing.to_str().unwrap())
        .arg(nonexistent_path("no-such-file-json-array-shape"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["path"], existing.to_str().unwrap());
    assert_eq!(json[0]["content"], "hello\n");
}

#[test]
fn test_read_multiple_files_jsonl() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("p.txt");
    let f2 = dir.path().join("q.txt");
    fs::write(&f1, "first\n").unwrap();
    fs::write(&f2, "second\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let lines: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .trim()
        .lines()
        .collect();
    assert_eq!(lines.len(), 2);
    let j1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let j2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert!(j1["content"].as_str().unwrap().contains("first"));
    assert!(j2["content"].as_str().unwrap().contains("second"));
}

#[test]
fn test_read_partial_failure_succeeds() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("exists.txt");
    fs::write(&f1, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(nonexistent_path("no-such-file-xyz"))
        .assert()
        .success()
        .stdout(predicates::str::contains("hello"));
}

#[test]
fn test_read_all_fail_returns_failure() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(nonexistent_path("no-1-xyz"))
        .arg(nonexistent_path("no-2-xyz"))
        .assert()
        .code(1);
}

#[test]
fn test_read_json_all_fail_returns_error_object() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("read")
        .arg(nonexistent_path("no-1-json-read-fail"))
        .arg(nonexistent_path("no-2-json-read-fail"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("expected JSON output, got: {stdout}"));
    assert_eq!(json["ok"], false);
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("no-1-json-read-fail"));
    assert!(error.contains("no-2-json-read-fail"));
}

#[test]
fn test_read_jsonl_all_fail_returns_error_object() {
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("read")
        .arg(nonexistent_path("no-1-jsonl-read-fail"))
        .arg(nonexistent_path("no-2-jsonl-read-fail"))
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
    let error = json["error"].as_str().unwrap();
    assert!(error.contains("no-1-jsonl-read-fail"));
    assert!(error.contains("no-2-jsonl-read-fail"));
}

#[test]
fn test_read_multiple_files_with_lines() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("long.txt");
    let f2 = dir.path().join("short.txt");
    fs::write(&f1, "a\nb\nc\nd\ne\n").unwrap();
    fs::write(&f2, "x\ny\n").unwrap();

    // --lines 2:4 on a 5-line file gives lines 2-4; on a 2-line file gives line 2 only
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("read")
        .arg(f1.to_str().unwrap())
        .arg(f2.to_str().unwrap())
        .arg("--lines")
        .arg("2:4")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("b\nc\nd"));
    assert!(stdout.contains("y"));
    // "aaa" and "eee" should not appear (outside the range for the first file)
    assert!(!stdout.contains("aaa"));
    assert!(!stdout.contains("eee"));
}

// ── status command ─────────────────────────────────────────────────

#[test]
fn test_status_clean_repo() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .success(); // exit 0 = no changes
}

#[test]
fn test_status_outside_git_repo_shows_actionable_hint() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("git status failed:"))
        .stderr(predicate::str::contains(
            "hint: run `git init` first, or run patchloom status from inside an existing git repository",
        ));
}

#[test]
fn test_status_modified_file() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Modify the committed file.
    fs::write(dir.path().join("a.txt"), "changed\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .assert()
        .code(2); // exit 2 = changes detected
}

#[test]
fn test_status_jsonl_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    fs::write(dir.path().join("new.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_changes"], 1);
    assert!(
        json["created"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "new.txt")
    );
}

#[test]
fn test_status_json_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Create untracked file.
    fs::write(dir.path().join("new.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["total_changes"], 1);
    assert!(
        json["created"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap() == "new.txt")
    );
}

#[test]
fn test_status_deleted_file() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "doomed.txt", "bye\n");
    git_ok(dir.path(), &["rm", "doomed.txt"]);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code().unwrap(), 2); // CHANGES_DETECTED
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap() == "doomed.txt")
    );
}

#[test]
fn test_status_glob_matches_filename_with_spaces() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    fs::write(dir.path().join("file name.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--glob")
        .arg("*.txt")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(
        created.iter().any(|v| v.as_str() == Some("file name.txt")),
        "glob-filtered status should report the unquoted filename, got: {json}"
    );
}

#[test]
fn test_status_glob_matches_nested_relative_pattern() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/keep.txt"), "new\n").unwrap();
    fs::write(dir.path().join("other.txt"), "new\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--glob")
        .arg("sub/*.txt")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(created.iter().any(|v| v.as_str() == Some("sub/keep.txt")));
    assert!(!created.iter().any(|v| v.as_str() == Some("other.txt")));
}

// ── create command ─────────────────────────────────────────────────

#[test]
fn test_create_new_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_create_refuses_overwrite() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("overwrite")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("already exists"));

    // Original content must be preserved.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "original content\n");
}

// ---------------------------------------------------------------------------
// tx
// ---------------------------------------------------------------------------

#[test]
fn test_tx_multi_op_plan() {
    let dir = TempDir::new().unwrap();

    let txt_file = dir.path().join("test.txt");
    fs::write(&txt_file, "hello world\n").unwrap();

    let json_file = dir.path().join("config.json");
    fs::write(&json_file, r#"{"name":"old"}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "from": "hello",
                "to": "hi"
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
        .success();

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert!(txt_content.contains("hi"), "text file should be modified");
    assert!(
        !txt_content.contains("hello"),
        "old text should be replaced"
    );

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
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": txt_file.to_str().unwrap(),
                "from": "hello",
                "to": "hi"
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
        .code(7);

    let txt_content = fs::read_to_string(&txt_file).unwrap();
    assert_eq!(
        txt_content, "hello world\n",
        "file should not be modified on rollback"
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
            "version": "1",
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
        .code(7); // ROLLBACK

    // a.json should be unchanged (rolled back).
    let content = fs::read_to_string(&file_a).unwrap();
    assert_eq!(
        content, r#"{"key": "original"}"#,
        "a.json should be rolled back"
    );
}

#[test]
fn test_tx_success_applies_all() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "old", "version": 1}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

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
            "version": "1",
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
fn test_doc_get_yaml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: patchloom\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("name")
        .assert()
        .success()
        .stdout(
            predicate::str::starts_with("\"patchloom\"")
                .or(predicate::str::starts_with("patchloom")),
        );
}

#[test]
fn test_doc_get_yaml_merge_key_resolved() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(
        &file,
        "defaults: &d\n  timeout: 30\n  retries: 3\nstaging:\n  <<: *d\n",
    )
    .unwrap();

    // Inherited key via merge must be accessible.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("staging.retries")
        .assert()
        .success()
        .stdout(predicate::str::contains("3"));
}

#[test]
fn test_doc_set_yaml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.yaml");
    fs::write(&file, "name: old\nversion: 1\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new"), "YAML should contain updated value");
    assert!(
        !content.contains("old"),
        "YAML should not contain old value"
    );
}

#[test]
fn test_doc_get_toml() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(
        &file,
        "[package]\nname = \"patchloom\"\nversion = \"1.0\"\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("package.name")
        .assert()
        .success()
        .stdout(predicate::str::contains("patchloom"));
}

#[test]
fn test_doc_set_toml_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.toml");
    fs::write(&file, "[package]\nname = \"old\"\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("package.name")
        .arg("\"new\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("new"), "TOML should contain updated value");
}

#[test]
fn test_doc_len_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items":[1,2,3,4,5]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("len")
        .arg(&file)
        .arg("items")
        .assert()
        .success()
        .stdout(predicate::str::contains("5"));
}

#[test]
fn test_doc_append_to_array() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"tags":["a","b"]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("append")
        .arg(&file)
        .arg("tags")
        .arg(r#""c""#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
}

#[test]
fn test_doc_flatten_json() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a":1,"b":{"c":2},"d":[10,20]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a = 1"))
        .stdout(predicate::str::contains("b.c = 2"))
        .stdout(predicate::str::contains("d[0] = 10"))
        .stdout(predicate::str::contains("d[1] = 20"));
}

#[test]
fn test_doc_flatten_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name":"patchloom"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"patchloom\""));
}

// ---------------------------------------------------------------------------
// doc diff
// ---------------------------------------------------------------------------

#[test]
fn test_doc_diff_shows_changes() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"name":"old","keep":1}"#).unwrap();
    fs::write(&b, r#"{"name":"new","keep":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(predicate::str::contains("~ name"));
}

// ---------------------------------------------------------------------------
// --check mode: exits 2 when changes detected, does NOT write
// ---------------------------------------------------------------------------

#[test]
fn test_replace_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("hi")
        .arg(&file)
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello world\n",
        "file should be unchanged in --check mode"
    );
}

#[test]
fn test_tidy_check_exits_2_with_issues() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no trailing newline").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_tidy_check_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing spaces   \nno final newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json["issue_count"].as_u64().unwrap() >= 2);
    assert!(json["issues"].is_array());
}

#[test]
fn test_doc_flatten_jsonl_outputs_path_value_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1,"b":{"c":2}}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("flatten")
        .arg(&file)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    assert!(lines.iter().any(|v| v["path"] == "a" && v["value"] == 1));
    assert!(lines.iter().any(|v| v["path"] == "b.c" && v["value"] == 2));
}

#[test]
fn test_doc_diff_jsonl_outputs_one_entry_per_line() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    fs::write(&a, r#"{"name":"old","removed":true}"#).unwrap();
    fs::write(&b, r#"{"name":"new","added":"yes"}"#).unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("diff")
        .arg(&a)
        .arg(&b)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "changed" && v["path"] == "name")
    );
    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "removed" && v["path"] == "removed")
    );
    assert!(
        lines
            .iter()
            .any(|v| v["kind"] == "added" && v["path"] == "added")
    );
}

#[test]
fn test_tidy_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    fs::write(&a, "trailing   \n").unwrap();
    fs::write(&b, "no final newline").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&a)
        .arg(&b)
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert!(
        lines.len() >= 2,
        "should have at least one JSONL line per issue"
    );
    for line in &lines {
        assert!(line["path"].is_string());
        assert!(line["issue"].is_string());
    }
}

#[test]
fn test_tidy_check_exits_0_when_clean() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "clean file\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("check")
        .arg(&file)
        .assert()
        .success();
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

#[test]
fn test_patch_check_exits_0_when_clean() {
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
        .arg("check")
        .arg(&patch_file)
        .assert()
        .success();
}

#[test]
fn test_patch_check_json_output_clean() {
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

    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json["files"].is_array());
    assert_eq!(json["files"][0]["status"], "clean");
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

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(lines.len(), 1, "should have one JSONL line per patch file");
    assert_eq!(lines[0]["path"], "test.txt");
    assert_eq!(lines[0]["status"], "clean");
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

#[test]
fn test_patch_check_exits_5_on_directory_read_error() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("test.txt");
    fs::create_dir(&target).unwrap();

    let patch_file = dir.path().join("change.patch");
    fs::write(
        &patch_file,
        "--- a/test.txt\n+++ b/test.txt\n@@ -0,0 +1 @@\n+new line\n",
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
        .stderr(predicate::str::contains(
            "patch check: test.txt -- READ ERROR: failed to read",
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

    assert_eq!(output.status.code(), Some(5));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["files"][0]["status"], "error");
    assert!(
        json["files"][0]["error"]
            .as_str()
            .unwrap()
            .contains("failed to read")
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

    assert_eq!(output.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(lines.len(), 1, "should have one JSONL line per patch file");
    assert_eq!(lines[0]["path"], "test.txt");
    assert_eq!(lines[0]["status"], "error");
    assert!(
        lines[0]["error"]
            .as_str()
            .unwrap()
            .contains("failed to read")
    );
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
fn test_create_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello")
        .arg("--check")
        .assert()
        .code(2);

    assert!(!file.exists(), "file should not be created in --check mode");
}

#[test]
fn test_create_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .arg("--jsonl")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_create_check_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("new.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    // diff field should be absent in --check mode
    assert!(json.get("diff").is_none());
    assert!(json.get("applied").is_none());

    assert!(
        !file.exists(),
        "file should not be created in --check --json mode"
    );
}

#[cfg(unix)]
#[test]
fn test_create_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("confirmed.txt");

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "create",
            file.to_str().unwrap(),
            "--content",
            "hello",
            "--confirm",
        ],
        "y\n",
    );

    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert_eq!(json["applied"], true);
    assert!(json["diff"].as_str().unwrap().contains("+hello"));
}

#[cfg(unix)]
#[test]
fn test_create_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("declined.txt");

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "create",
            file.to_str().unwrap(),
            "--content",
            "hello",
            "--confirm",
        ],
        "\u{4}",
    );

    assert!(output.status.success());
    assert!(
        !file.exists(),
        "file should not be created when confirmation input ends at EOF"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["path"], file.to_str().unwrap());
    assert_eq!(json["applied"], false);
    assert!(json["diff"].as_str().unwrap().contains("+hello"));
}

// ---------------------------------------------------------------------------
// rename command
// ---------------------------------------------------------------------------

#[test]
fn test_rename_moves_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists(), "source should be removed");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");
}

#[test]
fn test_rename_check_does_not_move() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(src.exists(), "source should still exist in --check mode");
    assert!(
        !dst.exists(),
        "destination should not be created in --check mode"
    );
}

#[test]
fn test_rename_refuses_overwrite_without_force() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "source\n").unwrap();
    fs::write(&dst, "existing\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .assert()
        .failure();

    // Both files should remain untouched.
    assert_eq!(fs::read_to_string(&src).unwrap(), "source\n");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "existing\n");
}

#[test]
fn test_rename_same_path_without_force_is_noop() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("same.txt");
    fs::write(&path, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&path)
        .arg(&path)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "source and destination are the same",
        ));

    assert_eq!(fs::read_to_string(&path).unwrap(), "content\n");
}

#[test]
fn test_rename_force_overwrites() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "new content\n").unwrap();
    fs::write(&dst, "old content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "new content\n");
}

#[test]
fn test_rename_apply_undo_restores_original_paths() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists(), "source should be removed after rename");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&src).unwrap(), "content\n");
    assert!(
        !dst.exists(),
        "undo should remove the created destination file"
    );
}

#[test]
fn test_rename_force_apply_undo_restores_overwritten_destination() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "source content\n").unwrap();
    fs::write(&dst, "destination content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists(), "source should be removed after rename");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "source content\n");

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&src).unwrap(), "source content\n");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "destination content\n");
}

#[test]
fn test_rename_missing_source_fails() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(dir.path().join("nope.txt"))
        .arg(dir.path().join("dst.txt"))
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_rename_check_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("moved-folder");
    fs::create_dir(&src).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("source is not a file"));

    assert!(src.is_dir(), "source directory should remain in place");
    assert!(!dst.exists(), "destination should not be created");
}

#[test]
fn test_rename_force_directory_destination_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_force_directory_destination_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_force_directory_destination_fails_in_apply_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("folder");
    fs::write(&src, "hello\n").unwrap();
    fs::create_dir(&dst).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--force")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(src.is_file(), "source file should remain in place");
    assert!(dst.is_dir(), "destination directory should remain in place");
}

#[test]
fn test_rename_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("moved-folder");
    fs::create_dir(&src).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("source is not a file"));

    assert!(src.is_dir(), "source directory should remain in place");
    assert!(!dst.exists(), "destination should not be created");
}

#[test]
fn test_rename_binary_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("image.bin");
    let dst = dir.path().join("moved.bin");
    // Non-UTF-8 content.
    fs::write(&src, b"\x00\x01\x02\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"\x00\x01\x02\xff\xfe");
}

#[test]
fn test_rename_check_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("new.txt"))
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dir.path().join("new.txt").to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_rename_json_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");
}

#[test]
fn test_rename_check_json_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("new.txt"))
        .arg("--check")
        .output()
        .unwrap();

    // --check returns exit code 2 (CHANGES_DETECTED).
    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert!(json.get("applied").is_none());
    // Source should still exist in --check mode.
    assert!(src.exists());
}

#[cfg(unix)]
#[test]
fn test_rename_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "rename",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--confirm",
        ],
        "y\n",
    );

    assert!(output.status.success());
    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "content\n");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert_eq!(json["applied"], true);
    assert!(json["diff"].as_str().unwrap().contains("+content"));
}

#[cfg(unix)]
#[test]
fn test_rename_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    let dst = dir.path().join("new.txt");
    fs::write(&src, "content\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &[
            "--json",
            "rename",
            src.to_str().unwrap(),
            dst.to_str().unwrap(),
            "--confirm",
        ],
        "\u{4}",
    );

    assert!(output.status.success());
    assert!(
        src.exists(),
        "source should remain when confirmation input ends at EOF"
    );
    assert!(
        !dst.exists(),
        "destination should not be created when confirmation input ends at EOF"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], src.to_str().unwrap());
    assert_eq!(json["to"], dst.to_str().unwrap());
    assert_eq!(json["applied"], false);
    assert!(json["diff"].as_str().unwrap().contains("+content"));
}

#[test]
fn test_rename_same_path_without_force_json_output() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("same.txt");
    fs::write(&path, "content\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("rename")
        .arg(&path)
        .arg(&path)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["from"], path.to_str().unwrap());
    assert_eq!(json["to"], path.to_str().unwrap());
    assert!(json["diff"].is_null());
}

#[test]
fn test_rename_binary_file_diff_mode() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("binary.bin");
    fs::write(&src, b"\x00\x01\x02\xff").unwrap();

    // Default mode (--diff) should not crash on binary files.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("moved.bin"))
        .assert()
        .success();

    // Source should still exist (no --apply).
    assert!(src.exists());
}

#[test]
fn test_rename_with_write_policy() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("no-newline.txt");
    let dst = dir.path().join("fixed.txt");
    fs::write(&src, "no newline at end").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--ensure-final-newline")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "no newline at end\n");
}

#[test]
fn test_rename_binary_with_write_policy_includes_path_in_error() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("binary.bin");
    // Non-UTF-8 content triggers read_to_string failure in write-policy branch.
    fs::write(&src, b"\x00\x01\xff\xfe").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(dir.path().join("moved.bin"))
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("binary.bin"));
}

// ---------------------------------------------------------------------------
// rename: default diff preview and tx --check mode
// ---------------------------------------------------------------------------

#[test]
fn test_rename_default_diff_preview() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "hello\n").unwrap();
    let dst = dir.path().join("new.txt");

    // No --apply or --check: default mode shows diff preview.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .assert()
        .success()
        .stdout(predicate::str::contains("-hello"))
        .stdout(predicate::str::contains("+hello"));

    // Source should still exist (no mutation in preview mode).
    assert!(src.exists());
    assert!(!dst.exists());
}

#[test]
fn test_rename_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("flat.txt");
    let dst = dir.path().join("sub").join("dir").join("moved.txt");
    fs::write(&src, "data\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg(&src)
        .arg(&dst)
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst).unwrap(), "data\n");
}

// ---------------------------------------------------------------------------
// create --check parent directory verification
// ---------------------------------------------------------------------------

#[test]
fn test_create_check_fails_if_parent_missing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--check")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "create --check should fail when parent dir doesn't exist"
    );
}

#[test]
fn test_create_check_force_skips_parent_verification() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("nonexistent").join("sub").join("file.txt");

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "create --check --force should succeed even if parent doesn't exist"
    );
}

// ---------------------------------------------------------------------------
// output format
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
        .success();

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["matches"].is_array(), "matches should be an array");
}

#[test]
fn test_replace_json_check_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file.txt"), "hello world\n").unwrap();

    let result = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("bye")
        .arg("--check")
        .arg(dir.path())
        .assert()
        .code(2); // CHANGES_DETECTED

    let output = result.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["files"].is_array(), "files should list affected paths");
}

#[test]
fn test_replace_jsonl_check_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello again\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("bye")
        .arg("--check")
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect();

    assert_eq!(
        lines.len(),
        2,
        "should have one JSONL line per matched file"
    );
    for line in &lines {
        assert!(line["path"].is_string());
        assert!(line["match_count"].as_u64().unwrap() >= 1);
    }
}

// ---------------------------------------------------------------------------
// CLI flags
// ---------------------------------------------------------------------------

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
                .and(predicate::str::contains("doc")),
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
fn test_parse_subcommand_search() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["search", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_replace() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_patch() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["patch", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_md() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["md", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_doc() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["doc", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_tidy() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_create() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["create", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_tx() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tx", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_subcommand_init() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["init", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_global_flag_json() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "search", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_write_flag_ensure_final_newline() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tidy", "--ensure-final-newline", "--help"])
        .assert()
        .success();
}

#[test]
fn test_parse_write_flag_normalize_eol() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "--normalize-eol", "lf", "--help"])
        .assert()
        .success();
}

#[test]
fn test_confirm_conflicts_with_apply() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace",
            "foo",
            "--to",
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
            "--to",
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
fn test_parse_unknown_subcommand_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("nonexistent-command")
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// doc: delete, merge, prepend, select, ensure, move, update
// ---------------------------------------------------------------------------

#[test]
fn test_doc_delete_removes_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"name":"keep","remove_me":true}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("remove_me")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], serde_json::json!("keep"));
    assert!(v.get("remove_me").is_none(), "key should be removed");
}

#[test]
fn test_doc_merge_combines_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"b":2}"#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["a"], serde_json::json!(1));
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_prepend_inserts_at_front() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[2,3]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("prepend")
        .arg(&file)
        .arg("items")
        .arg("1")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0], serde_json::json!(1));
    assert_eq!(items.len(), 3);
}

#[test]
fn test_doc_select_filters_by_predicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(
        &file,
        r#"{"items":[{"status":"active","name":"a"},{"status":"done","name":"b"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=active]")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\""))
        .stdout(predicate::str::contains("\"a\""))
        .stdout(predicate::str::contains("\"b\"").not());
}

#[test]
fn test_doc_ensure_creates_missing_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("b")
        .arg("2")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["b"], serde_json::json!(2));
}

#[test]
fn test_doc_ensure_noop_when_exists() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    // ensure with --check when key already exists should exit 0 (no changes)
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("1")
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn test_doc_move_renames_key() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"old_key":"value"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("old_key")
        .arg("new_key")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["new_key"], serde_json::json!("value"));
    assert!(v.get("old_key").is_none(), "old key should be gone");
}

#[test]
fn test_doc_update_matching_nodes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"s":"a"},{"s":"b"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("update")
        .arg(&file)
        .arg("items[*].s")
        .arg("\"x\"")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items[0]["s"], serde_json::json!("x"));
    assert_eq!(items[1]["s"], serde_json::json!("x"));
}

// ---------------------------------------------------------------------------
// md: dedupe-headings, lint-agents
// ---------------------------------------------------------------------------

#[test]
fn test_md_dedupe_headings_removes_duplicate() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let count = content.matches("## Dup").count();
    assert_eq!(count, 1, "duplicate heading should be removed");
}

#[test]
fn test_md_dedupe_headings_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1);
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json.as_str().unwrap(), "## Dup");
}

#[test]
fn test_md_dedupe_headings_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    fs::write(&file, "# Title\n\n## Dup\n\nFirst\n\n## Dup\n\nSecond\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("md")
        .arg("dedupe-headings")
        .arg(&file)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_str().unwrap(), "## Dup");
}

#[test]
fn test_md_lint_agents_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# T\n\nUse git add .\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "quiet should suppress lint-agents output"
    );
}

#[test]
fn test_md_lint_agents_clean_file_exits_0() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(&file, "# AGENTS.md\n\n## Build\n\nRun make\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .success();
}

#[test]
fn test_md_lint_agents_bad_file_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .code(2);
}

#[test]
fn test_md_lint_agents_skips_fenced_code_blocks() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    // The dangerous command appears inside both backtick and tilde fences —
    // lint-agents must not flag it.
    fs::write(
        &file,
        "# Rules\n\n```bash\ngit add .\n```\n\n~~~bash\ngit add -A\n~~~\n\nStage explicitly.\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .assert()
        .success();
}

#[test]
fn test_md_lint_agents_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty());
    // Each issue must have an "issue" field
    assert!(arr[0].get("issue").is_some());
}

#[test]
fn test_md_lint_agents_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("AGENTS.md");
    fs::write(
        &file,
        "# AGENTS.md\n\n## Build\n\nRun make\n\n## Build\n\nDuplicate\n",
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("md")
        .arg("lint-agents")
        .arg(&file)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert!(!lines.is_empty());
    // Each line must be valid JSON with an "issue" field
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(parsed.get("issue").is_some());
    }
}

// ---------------------------------------------------------------------------
// global flags: --jsonl, --files-from, --context, --literal
// ---------------------------------------------------------------------------

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
        .success();

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
fn test_files_from_restricts_search() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("included.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("excluded.txt"), "findme\n").unwrap();

    let list = dir.path().join("filelist.txt");
    fs::write(&list, "included.txt\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg(&list)
        .arg("search")
        .arg("findme")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("included.txt"))
        .stdout(predicate::str::contains("excluded.txt").not());
}

#[test]
fn test_files_from_stdin_restricts_search() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("included.txt"), "findme\n").unwrap();
    fs::write(dir.path().join("excluded.txt"), "findme\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--files-from")
        .arg("-")
        .arg("search")
        .arg("findme")
        .arg(".")
        .write_stdin("included.txt\n")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("included.txt"));
    assert!(
        !stdout.contains("excluded.txt"),
        "excluded file should not appear"
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
fn test_search_context_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\nline2\nmatch_me\nline4\nline5\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-C")
        .arg("1")
        .arg("match_me")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("line2"))
        .stdout(predicate::str::contains("match_me"))
        .stdout(predicate::str::contains("line4"));
}

#[test]
fn test_search_before_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\nccc\ntarget\nddd\neee\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-B")
        .arg("2")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("bbb"))
        .stdout(predicate::str::contains("ccc"))
        .stdout(predicate::str::contains("target"))
        // no after-context lines
        .stdout(predicate::str::contains("ddd").not());
}

#[test]
fn test_search_after_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\ntarget\nccc\nddd\neee\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-A")
        .arg("2")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("ccc"))
        .stdout(predicate::str::contains("ddd"))
        // no before-context lines
        .stdout(predicate::str::contains("bbb").not());
}

#[test]
fn test_search_asymmetric_context() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\nbbb\nccc\ntarget\nddd\neee\nfff\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-B")
        .arg("1")
        .arg("-A")
        .arg("3")
        .arg("target")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("ccc"))
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("ddd"))
        .stdout(predicate::str::contains("eee"))
        .stdout(predicate::str::contains("fff"))
        // bbb is 2 lines before, should not appear with -B 1
        .stdout(predicate::str::contains("bbb").not());
}

#[test]
fn test_search_literal_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // dot in "foo.bar" should NOT match "fooXbar" with --literal
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("--literal")
        .arg("foo.bar")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("foo.bar"))
        .stdout(predicate::str::contains("fooXbar").not());
}

// ---------------------------------------------------------------------------
// search short aliases (-F, -l, -c)
// ---------------------------------------------------------------------------

#[test]
fn test_search_short_alias_literal() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo.bar\nfooXbar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-F")
        .arg("foo.bar")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("foo.bar"))
        .stdout(predicate::str::contains("fooXbar").not());
}

#[test]
fn test_search_short_alias_count() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\nhello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-c")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains(":2"));
}

#[test]
fn test_search_short_alias_files_with_matches() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-l")
        .arg("hello")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));
}

// ---------------------------------------------------------------------------
// create --force
// ---------------------------------------------------------------------------

#[test]
fn test_create_force_directory_target_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_directory_target_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_directory_target_fails_in_apply_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&target)
        .arg("--content")
        .arg("hello\n")
        .arg("--force")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_create_force_overwrites_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("overwritten")
        .arg("--force")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "overwritten");
}

// ---------------------------------------------------------------------------
// error paths
// ---------------------------------------------------------------------------

#[test]
fn test_replace_no_match_without_if_exists_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "some content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("nonexistent_pattern_xyz")
        .arg("--to")
        .arg("new")
        .arg(&file)
        .arg("--apply")
        .assert()
        .code(3);
}

#[test]
fn test_search_invert_match_normal() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "keep\nremove\nkeep2\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-v")
        .arg("remove")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("keep"))
        .stdout(predicate::str::contains("remove").not());
}

// ---------------------------------------------------------------------------
// tx: file.create, file.delete, write_policy, validate
// ---------------------------------------------------------------------------

#[test]
fn test_tx_file_create_and_delete() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete in same tx"
    );
}

#[test]
fn test_tx_check_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert!(
        !dir.path().join("new.txt").exists(),
        "file should not exist after create+delete no-op check"
    );
}

#[test]
fn test_tx_json_output_create_then_delete_is_noop() {
    let dir = TempDir::new().unwrap();
    let plan = serde_json::json!({
            "version": "1",
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
            "version": "1",
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
        .code(7)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_tx_file_delete_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "goodbye\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert!(!file.exists(), "file should be deleted");
}

#[test]
fn test_tx_cli_ensure_final_newline_flag() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "no newline").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
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
        .success();

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
            "version": "1",
        "write_policy": {
            "ensure_final_newline": false
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
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
        .success();

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
            "version": "1",
        "write_policy": {
            "ensure_final_newline": true
        },
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
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
        .success();

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

// ---------------------------------------------------------------------------
// doc/patch error paths
// ---------------------------------------------------------------------------

#[test]
fn test_doc_get_nonexistent_file_fails() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg("/nonexistent/file_xyz.json")
        .arg("key")
        .assert()
        .failure();
}

#[test]
fn test_doc_get_unsupported_extension_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.ini");
    fs::write(&file, "key=value\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("get")
        .arg(&file)
        .arg("key")
        .assert()
        .failure();
}

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
        .failure();
}

// ---------------------------------------------------------------------------
// --jsonl + count/files-with-matches (bug fix: count_only + jsonl)
// ---------------------------------------------------------------------------

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
        .success();

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
        .success();

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
        .success();

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
fn test_tx_validate_required_failure_exits_6() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "old", "to": "new"}
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
            "version": "1",
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("remove").is_none());
    assert_eq!(v["keep"], serde_json::json!(1));
}

// ---------------------------------------------------------------------------
// doc: edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_doc_select_no_matches_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"items":[{"status":"active"}]}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("select")
        .arg(&file)
        .arg("items[status=nonexistent]")
        .assert()
        .code(3);
}

#[test]
fn test_doc_move_missing_source_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("move")
        .arg(&file)
        .arg("nonexistent")
        .arg("target")
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_doc_merge_nested_objects() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"x":{"existing":"old"}}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"x":{"nested":"new"}}"#)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["x"]["existing"],
        serde_json::json!("old"),
        "existing key preserved"
    );
    assert_eq!(v["x"]["nested"], serde_json::json!("new"), "new key merged");
}

#[test]
fn test_doc_ensure_noop_when_value_differs() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    // ensure a=99 when a already exists with value 1: should NOT change the value
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("ensure")
        .arg(&file)
        .arg("a")
        .arg("99")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        v["a"],
        serde_json::json!(1),
        "ensure should not overwrite existing key"
    );
}

#[test]
fn test_doc_set_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.json");
    fs::write(&file, r#"{"version":"1.0"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("set")
        .arg(&file)
        .arg("version")
        .arg("\"2.0\"")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("1.0"),
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// tx: file.delete on empty file (bug fix), validation optional step
// ---------------------------------------------------------------------------

#[test]
fn test_tx_file_delete_empty_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert!(!file.exists(), "empty file should be deleted");
}

#[test]
fn test_tx_file_rename_directory_source_fails() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("folder");
    let dst = dir.path().join("new-name");
    fs::create_dir(&src).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .code(7)
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
            "version": "1",
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
        .success();

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
            "version": "1",
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

    // Should roll back (exit 7).
    assert_eq!(output.status.code(), Some(7));
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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .code(7)
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
            "version": "1",
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
        .code(7)
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
            "version": "1",
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
        .code(7)
        .stderr(predicate::str::contains("destination is not a file"));

    assert!(dir.path().join("old.txt").is_file());
    assert!(dir.path().join("folder").is_dir());
}

#[test]
fn test_tx_file_rename_same_path_is_noop() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("same.txt"), "keep me\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert_eq!(
        fs::read_to_string(dir.path().join("same.txt")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn test_batch_file_rename() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("file.rename old.txt new.txt\n")
        .assert()
        .success();

    assert!(!dir.path().join("old.txt").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn test_tx_optional_validation_failure_ignored() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "old\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "old", "to": "new"}
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("new"),
        "file should be modified despite optional validation failure"
    );
}

// ---------------------------------------------------------------------------
// md --check leaves file untouched
// ---------------------------------------------------------------------------

#[test]
fn test_md_replace_section_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n\n## Other\n\nkept\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// tidy fix --check
// ---------------------------------------------------------------------------

#[test]
fn test_tidy_fix_check_exits_2_no_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "trailing whitespace   ").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(&file)
        .arg("--ensure-final-newline")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "trailing whitespace   ",
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// md --apply --check: check takes priority (bug fix)
// ---------------------------------------------------------------------------

#[test]
fn test_md_apply_check_does_not_write() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.md");
    let original = "# Title\n\n## Section\n\nold content\n";
    fs::write(&file, original).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("replace-section")
        .arg(&file)
        .arg("--heading")
        .arg("## Section")
        .arg("--content")
        .arg("new content")
        .arg("--apply")
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, original,
        "--check should prevent writing even with --apply"
    );
}

// ---------------------------------------------------------------------------
// tx: glob-based replace
// ---------------------------------------------------------------------------

#[test]
fn test_tx_glob_replace_only_matches_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello\n").unwrap();
    fs::write(dir.path().join("skip.rs"), "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "replace", "glob": "*.txt", "from": "hello", "to": "bye"}
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
        .success();

    assert!(
        fs::read_to_string(dir.path().join("a.txt"))
            .unwrap()
            .contains("bye")
    );
    assert!(
        fs::read_to_string(dir.path().join("b.txt"))
            .unwrap()
            .contains("bye")
    );
    assert!(
        fs::read_to_string(dir.path().join("skip.rs"))
            .unwrap()
            .contains("hello"),
        ".rs file should not be modified"
    );
}

#[test]
fn test_tx_glob_replace_matches_file_created_earlier_in_transaction() {
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "file.create", "path": "new.txt", "content": "hello\n"},
            {"op": "replace", "glob": "*.txt", "from": "hello", "to": "bye"}
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
        .success();

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
            "version": "1",
        "operations": [
            {"op": "replace", "glob": "sub/*.txt", "from": "hello", "to": "bye"}
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
        .success();

    assert_eq!(
        fs::read_to_string(dir.path().join("sub/keep.txt")).unwrap(),
        "bye\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("other.txt")).unwrap(),
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
            "version": "1",
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("v2.0 release"), "new section content");
    assert!(!content.contains("Old entry"), "old content removed");
    assert!(content.contains("Kept"), "other section preserved");
}

// ---------------------------------------------------------------------------
// write policy CLI flags: normalize-eol, trim-trailing-whitespace, --diff
// ---------------------------------------------------------------------------

#[test]
fn test_replace_normalize_eol_lf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "old\r\ncontent\r\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old")
        .arg("--to")
        .arg("new")
        .arg("--normalize-eol")
        .arg("lf")
        .arg(&file)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        !content.windows(2).any(|w| w == b"\r\n"),
        "CRLF should be normalized to LF"
    );
    assert!(
        content.windows(3).any(|w| w == b"new"),
        "replacement should be applied"
    );
}

#[test]
fn test_create_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("trimmed.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("hello   \nworld\t\n")
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "hello\nworld\n",
        "trailing whitespace should be trimmed"
    );
}

#[test]
fn test_create_ensure_final_newline() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("newline.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("no trailing newline")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "no trailing newline\n",
        "final newline should be appended via File::create_new path"
    );
}

#[test]
fn test_create_normalize_eol_lf() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("eol.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("create")
        .arg(&file)
        .arg("--content")
        .arg("line1\r\nline2\r\n")
        .arg("--normalize-eol")
        .arg("lf")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "line1\nline2\n",
        "CRLF should be normalized to LF via File::create_new path"
    );
}

// ---------------------------------------------------------------------------
// multi-file replace --apply on directory
// ---------------------------------------------------------------------------

#[test]
fn test_replace_directory_modifies_all_matching_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "hello again\n").unwrap();
    fs::write(dir.path().join("c.txt"), "no match here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("bye")
        .arg(dir.path())
        .arg("--apply")
        .assert()
        .success();

    assert!(
        fs::read_to_string(dir.path().join("a.txt"))
            .unwrap()
            .contains("bye")
    );
    assert!(
        fs::read_to_string(dir.path().join("b.txt"))
            .unwrap()
            .contains("bye")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("c.txt")).unwrap(),
        "no match here\n",
        "file without match should be untouched"
    );
}

// ---------------------------------------------------------------------------
// --files-from error on bad path
// ---------------------------------------------------------------------------

#[test]
fn test_files_from_nonexistent_path_fails() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--files-from")
        .arg("/nonexistent/file_list_xyz.txt")
        .arg("search")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--files-from"));
}

// ---------------------------------------------------------------------------
// editorconfig integration
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
        .success();

    let content = fs::read(&file).unwrap();
    assert!(
        content.ends_with(b"\n"),
        "config should have enabled ensure_final_newline"
    );
}

#[test]
fn test_project_config_exclude_globs() {
    let dir = TempDir::new().unwrap();

    // Create .patchloom.toml with glob that limits to *.rs files.
    fs::write(
        dir.path().join(".patchloom.toml"),
        "[exclude]\nglobs = [\"*.rs\"]\n",
    )
    .unwrap();

    // Create both .rs and .txt files.
    fs::write(dir.path().join("code.rs"), "hello\n").unwrap();
    fs::write(dir.path().join("notes.txt"), "hello\n").unwrap();

    // Search should find matches in .rs but not .txt (glob from config).
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["search", "hello", ".", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("code.rs"))
        .stdout(predicates::str::contains("notes.txt").not());
}

// ── explain ──────────────────────────────────────────────────

#[test]
fn test_explain_prints_human_summary() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{
            "version": "1",
            "strict": true,
            "operations": [
                {"op": "file.create", "path": "test.txt", "content": "hi"},
                {"op": "file.delete", "path": "old.txt"}
            ],
            "validate": [{"cmd": "echo ok", "required": true}]
        }"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .success()
        .stdout(predicates::str::contains("2 operation(s) (strict mode)"))
        .stdout(predicates::str::contains("Create file test.txt"))
        .stdout(predicates::str::contains("Delete file old.txt"))
        .stdout(predicates::str::contains("Validate: echo ok (required)"));
}

#[test]
fn test_explain_json_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": "1", "operations": [{"op": "replace", "from": "a", "to": "b"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--json"])
        .arg(&plan)
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["operation_count"], 1);
    assert_eq!(json["strict"], false);
    assert!(json["has_write_policy"].is_boolean());
    assert_eq!(json["format_steps"], 0);
    assert_eq!(json["validate_steps"], 0);
    let ops = json["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["index"], 1);
    assert!(ops[0]["description"].as_str().unwrap().contains("Replace"));
}

#[test]
fn test_explain_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": "1", "operations": [{"op": "file.delete", "path": "x.txt"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--jsonl"])
        .arg(&plan)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["operation_count"], 1);
    assert_eq!(json["operations"][0]["description"], "Delete file x.txt");
}

#[test]
fn test_explain_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": "1", "operations": [{"op": "file.delete", "path": "x.txt"}]}"#,
    )
    .unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "explain"])
        .arg(&plan)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_explain_invalid_plan_fails() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("bad.json");
    fs::write(&plan, "not valid json at all").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain"])
        .arg(&plan)
        .assert()
        .failure();
}

#[test]
fn test_explain_stdin() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--stdin"])
        .write_stdin(r#"{"version": "1", "operations": [{"op": "file.delete", "path": "x.txt"}]}"#)
        .assert()
        .success()
        .stdout(predicates::str::contains("Delete file x.txt"));
}

#[test]
fn test_explain_stdin_takes_precedence_over_path() {
    let dir = TempDir::new().unwrap();
    let plan = dir.path().join("plan.json");
    fs::write(
        &plan,
        r#"{"version": "1", "operations": [{"op": "file.delete", "path": "from-file.txt"}]}"#,
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["explain", "--stdin"])
        .arg(&plan)
        .write_stdin(
            r#"{"version": "1", "operations": [{"op": "file.delete", "path": "from-stdin.txt"}]}"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains("Delete file from-stdin.txt"))
        .stdout(predicates::str::contains("from-file.txt").not());
}

// ── undo ─────────────────────────────────────────────────────

#[test]
fn test_undo_restores_replaced_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    // Apply a replace.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "goodbye", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "goodbye world\n");

    // Undo should restore the original.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello world\n");
}

#[test]
fn test_undo_list_shows_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    // Apply a replace to create a backup.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    // List should show the session.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test.txt"));
}

#[test]
fn test_undo_dry_run_by_default() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    // Apply.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    // Undo without --apply should show what would change but not restore.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(2) // CHANGES_DETECTED
        .stdout(predicates::str::contains("restore original"));

    // File should still be modified.
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified\n");
}

#[test]
fn test_undo_dry_run_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "undo", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified\n");
}

#[test]
fn test_undo_list_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--json", "--cwd"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("\"timestamp\""))
        .stdout(predicates::str::contains("\"entries\""));
}

#[test]
fn test_undo_list_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--jsonl", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "JSONL output should be one session per line"
    );
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert!(json["timestamp"].is_string());
    assert!(json["entries"].is_array());
    assert_eq!(json["entries"][0]["path"], "test.txt");
}

#[test]
fn test_undo_dry_run_json_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--json", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert!(json["session"].is_string());
    assert_eq!(json["file_count"], 1);
    assert_eq!(json["entries"][0]["path"], "test.txt");
    assert_eq!(json["entries"][0]["action"], "restore original");
}

#[test]
fn test_undo_dry_run_jsonl_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args([
            "replace", "original", "--to", "modified", "--apply", "--cwd",
        ])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--jsonl", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "changes_detected");
    assert_eq!(json["entries"][0]["path"], "test.txt");
    assert_eq!(json["entries"][0]["action"], "restore original");
}

#[test]
fn test_undo_list_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["replace", "hello", "--to", "hi", "--apply", "--cwd"])
        .arg(dir.path())
        .arg(portable_path_str(&file))
        .assert()
        .success();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--quiet", "undo", "--list", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn test_undo_tx_restores_multi_file() {
    let dir = TempDir::new().unwrap();
    let f1 = dir.path().join("a.txt");
    let f2 = dir.path().join("b.txt");
    fs::write(&f1, "alpha\n").unwrap();
    fs::write(&f2, "beta\n").unwrap();

    let plan_content = format!(
        r#"{{"version":"1","operations":[
            {{"op":"replace","path":"{}","from":"alpha","to":"omega"}},
            {{"op":"replace","path":"{}","from":"beta","to":"gamma"}}
        ]}}"#,
        portable_path_str(&f1),
        portable_path_str(&f2)
    );
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, &plan_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["tx", "--apply"])
        .arg(portable_path_str(&plan_file))
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&f1).unwrap(), "omega\n");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "gamma\n");

    // Undo should restore both files.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&f1).unwrap(), "alpha\n");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "beta\n");
}

#[test]
fn test_undo_no_sessions_exits_3() {
    let dir = TempDir::new().unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--list", "--cwd"])
        .arg(dir.path())
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_undo_list_json_empty_emits_array() {
    let dir = TempDir::new().unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["--json", "undo", "--list", "--cwd"])
        .arg(dir.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(parsed.is_array(), "empty undo --list --json should emit []");
    assert_eq!(parsed.as_array().unwrap().len(), 0);
}

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
        .success();

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
fn test_replace_nth_replaces_only_nth_occurrence() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar foo baz foo\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("foo")
        .arg("--to")
        .arg("REPLACED")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "foo bar REPLACED baz foo\n");
}

#[test]
fn test_replace_nth_zero_rejected() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("hi")
        .arg("--nth")
        .arg("0")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicates::str::contains("1-based"));

    // File must be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "hello world\n");
}

#[test]
fn test_replace_nth_no_match_when_out_of_range() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("foo")
        .arg("--to")
        .arg("REPLACED")
        .arg("--nth")
        .arg("5")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .code(3); // NO_MATCHES

    // File unchanged.
    assert_eq!(fs::read_to_string(&file).unwrap(), "foo bar\n");
}

#[test]
fn test_search_case_insensitive() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("test.txt"), "Hello World\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("-i")
        .arg("hello")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello World"));
}

#[test]
fn test_replace_insert_before() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "    /// Doc comment.\n    pub field: bool,\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("    /// Doc comment.")
        .arg("--insert-before")
        .arg("    // marker\n")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "    // marker\n    /// Doc comment.\n    pub field: bool,\n"
    );
}

#[test]
fn test_replace_insert_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");
    fs::write(&file, "line1\nanchor\nline3\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("anchor")
        .arg("--insert-after")
        .arg(" // tagged")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nanchor // tagged\nline3\n"
    );
}

#[test]
fn test_replace_insert_before_with_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("b+")
        .arg("--regex")
        .arg("--insert-before")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nXbbb\nccc\n");
}

#[test]
fn test_replace_insert_after_with_regex() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("b+")
        .arg("--regex")
        .arg("--insert-after")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nbbbX\nccc\n");
}

#[test]
fn test_replace_insert_before_nth() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "x a x a x\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("a")
        .arg("--insert-before")
        .arg("[")
        .arg("--nth")
        .arg("2")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "x a x [a x\n");
}

#[test]
fn test_replace_insert_before_and_to_conflict() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("world")
        .arg("--insert-before")
        .arg("X")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_tx_replace_insert_before_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.txt");
    fs::write(&file, "    /// Old doc.\n    pub val: i32,\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "cwd": dir.path().to_str().unwrap(),
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "    /// Old doc.",
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
        .success();

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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "mode": "regex",
            "from": "b+",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nXbbb\nccc\n");
}

#[test]
fn test_tx_replace_insert_after_with_regex_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "mode": "regex",
            "from": "b+",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa\nbbbX\nccc\n");
}

#[test]
fn test_tx_replace_rejects_both_insert_before_and_after() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "hello",
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
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "hello",
                "to": "goodbye",
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
fn test_replace_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "Hello HELLO hello\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("HI")
        .arg("-i")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "HI HI HI\n");
}

#[test]
fn test_multi_glob_filters_multiple_patterns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.rs"), "match\n").unwrap();
    fs::write(dir.path().join("b.md"), "match\n").unwrap();
    fs::write(dir.path().join("c.txt"), "match\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("match")
        .arg("--glob")
        .arg("*.rs")
        .arg("--glob")
        .arg("*.md")
        .arg(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("b.md"));
    assert!(!stdout.contains("c.txt"));
}

#[test]
fn test_delete_removes_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "bye\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    assert!(!file.exists());
}

#[test]
fn test_delete_jsonl_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "keep\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("safe.txt"));
}

#[test]
fn test_delete_json_apply_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(!file.exists());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], true);
    assert!(json["path"].as_str().unwrap().contains("doomed.txt"));
}

#[cfg(unix)]
#[test]
fn test_delete_json_confirm_output_reports_applied_after_confirmation() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("confirmed.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &["--json", "delete", file.to_str().unwrap(), "--confirm"],
        "y\n",
    );

    assert!(output.status.success());
    assert!(!file.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], true);
    assert!(json["path"].as_str().unwrap().contains("confirmed.txt"));
}

#[cfg(unix)]
#[test]
fn test_delete_json_confirm_output_reports_applied_false_on_tty_eof() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("declined.txt");
    fs::write(&file, "bye\n").unwrap();

    let output = run_patchloom_confirm_in_pty(
        &["--json", "delete", file.to_str().unwrap(), "--confirm"],
        "\u{4}",
    );

    assert!(output.status.success());
    assert!(
        file.exists(),
        "file should remain when confirmation input ends at EOF"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "bye\n");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("script output should contain JSON");
    let json: serde_json::Value = serde_json::from_str(&stdout[json_start..]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("declined.txt"));
}

#[test]
fn test_delete_json_check_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "keep\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(file.exists());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["applied"], false);
    assert!(json["path"].as_str().unwrap().contains("safe.txt"));
}

#[test]
fn test_delete_check_mode_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "still here\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(2);

    assert!(file.exists());
}

#[test]
fn test_delete_directory_target_fails_in_dry_run() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(target.to_str().unwrap())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_delete_directory_target_fails_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("folder");
    fs::create_dir(&target).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(target.to_str().unwrap())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_delete_nonexistent_file_fails() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("ghost.txt");

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .assert()
        .failure();
}

#[test]
fn test_md_insert_before_heading() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "# Title\n\nIntro.\n\n## Section A\n\nContent A.\n\n## Section B\n\nContent B.\n",
    )
    .unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Section B")
        .arg("--content")
        .arg("Inserted before B.")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before B.\n\n## Section B"));
}

#[test]
fn test_tx_file_create_new_file_writes_content() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("brand_new.txt");

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .code(7)
        .stderr(predicate::str::contains("target is not a file"));

    assert!(target.is_dir(), "directory should remain in place");
}

#[test]
fn test_tx_file_create_force_overwrites() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "overwritten\n");
}

#[test]
fn test_tx_file_create_without_force_fails_on_existing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .code(7); // ROLLBACK

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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "before",
            "to": "after"
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");
    assert!(marker.exists(), "format step should have created marker");
}

#[test]
fn test_tx_doc_prepend_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2, 3]}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["items"][0], 0);
    assert_eq!(v["items"][1], 1);
}

#[test]
fn test_tx_doc_set_selector_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"nested": {"name": "old"}}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["nested"]["name"], "new");
}

#[test]
fn test_tx_doc_ensure_selector_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "test"}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("Inserted before.\n\n## Section"));
}

#[test]
fn test_tx_md_upsert_bullet_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Rules\n\n- existing rule\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    // Only one "## Dupe" heading should remain.
    assert_eq!(content.matches("## Dupe").count(), 1);
}

#[test]
fn test_tx_md_insert_after_heading_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\nExisting content\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let result = fs::read_to_string(&file).unwrap();
    assert!(result.contains("Inserted line\nExisting content"));
}

#[test]
fn test_tx_tidy_fix_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    // File has no final newline.
    fs::write(&file, "line1\nline2").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "line1\nline2\n");
}

#[test]
fn test_tx_tidy_fix_trim_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("messy.txt");
    fs::write(&file, "hello   \nworld\t\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "hello\nworld\n");
}

#[test]
fn test_tx_tidy_fix_normalize_eol() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("crlf.txt");
    fs::write(&file, "line1\r\nline2\r\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

    let result = fs::read_to_string(&file).unwrap();
    assert_eq!(result, "line1\nline2\n");
}

#[test]
fn test_tx_doc_append_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"items": [1, 2]}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .success();

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
            "version": "1",
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
        .success();

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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "HI",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "HI HI HI\n");
}

#[test]
fn test_tx_replace_multiline_regex_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "start\nmiddle\nend\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "mode": "regex",
            "from": "start.middle",
            "to": "REPLACED",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "REPLACED\nend\n");
}

#[test]
fn test_tx_replace_nth_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa bbb aaa ccc aaa\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "aaa",
            "to": "ZZZ",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "aaa bbb ZZZ ccc aaa\n");
}

#[test]
fn test_tx_replace_apply_no_match_exits_3() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "zzz",
            "to": "ZZZ"
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "zzz",
            "to": "ZZZ"
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
fn test_tx_json_check_replace_no_match_exits_3_without_success_payload() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "zzz",
            "to": "ZZZ"
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
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn test_tx_replace_if_exists_no_match_succeeds() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "zzz",
            "to": "ZZZ",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "foo bar\n");
}

#[test]
fn test_tx_replace_if_exists_still_replaces_when_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "foo bar\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "foo",
            "to": "baz",
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
        .success();

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
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "zzz",
                "to": "ZZZ"
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
        .success();

    let config_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(config_value["version"], "1.0");
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
            "version": "1",
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
        .success();

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
            "version": "1",
        "cwd": dir.path().to_str().unwrap(),
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "old line",
                "to": "mid line"
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
        .success();

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "line1\nnew line\nline3\n"
    );
}

#[test]
fn test_tx_validate_timeout_kills_hanging_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
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
            "version": "1",
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
fn test_tx_read_empty_file_without_lines_matches_read_contract() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
        "operations": [
            {"op": "read", "path": file.to_str().unwrap()},
            {"op": "replace", "path": file.to_str().unwrap(), "from": "hello", "to": "goodbye"},
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

    assert!(output.status.success());
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
            "version": "1",
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
            "version": "1",
        "operations": [
            {"op": "search", "path": file.to_str().unwrap(), "pattern": "hello"},
            {"op": "replace", "path": file.to_str().unwrap(), "from": "hello", "to": "goodbye"}
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
        "version": "1",
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
fn test_tx_search_multiline_spans_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "hello\nworld\n").unwrap();

    let plan = serde_json::json!({
        "version": "1",
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
        "version": "1",
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
        "version": "1",
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
        "version": "1",
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
        "version": "1",
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
            "version": "1",
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
            "version": "1",
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
            "version": "1",
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
            "version": "1",
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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

#[test]
fn test_tx_json_output_on_modified_empty_file_reports_modified() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
            "version": "1",
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
            "version": "1",
        "operations": [
            {"op": "replace", "path": file.to_str().unwrap(), "from": "hello", "to": "world"}
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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
            "version": "1",
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
            "version": "1",
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

    assert_eq!(output.status.code(), Some(7)); // ROLLBACK
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], false);
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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

    assert!(output.status.success());
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
            "version": "1",
        "operations": [
            {
                "op": "replace",
                "path": file.to_str().unwrap(),
                "from": "hello"
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
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "test.txt",
                "from": "hello"
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
    assert!(error.contains("replace operation requires one of to, insert_before, or insert_after"));
}

#[test]
fn test_tx_json_output_on_replace_conflict_parse_error() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("bad.json");
    fs::write(
        &plan_file,
        serde_json::to_string(&serde_json::json!({
            "version": "1",
            "operations": [{
                "op": "replace",
                "path": "data.txt",
                "from": "hello",
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
    assert!(error.contains("insert_before and insert_after cannot both be set"));
}

#[test]
fn test_tx_validation_failure_redacts_shell_command_in_stderr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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
    assert!(stderr.contains("required validation failed (step 1, exit code 1)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_validation_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_json_output_on_strict_validation_failure_preserves_reason() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
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
    assert!(error.contains("required validation failed (step 1, exit code 1)"));
    assert!(error.contains("exit code 1"));
}

#[test]
fn test_tx_strict_mode_restores_deleted_empty_file_on_failure() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
            "version": "1",
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
fn test_tx_non_strict_format_failure_exits_6_not_7() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
    assert!(stderr.contains("format step failed (step 1, exit code 1)"));
    assert!(!stderr.contains(secret));
}

#[test]
fn test_tx_json_output_on_format_failure_redacts_shell_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let secret = "TOKEN=super-secret-value";
    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
    assert!(error.contains("validation_failed"));
    assert!(error.contains("format step failed (step 1, exit code 1)"));
    assert!(!error.contains(secret));
}

#[test]
fn test_tx_json_output_on_strict_format_failure_preserves_error_kind() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "original\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "strict": true,
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "original",
            "to": "changed"
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
    assert!(error.contains("format step failed (step 1, exit code 1)"));
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
            "version": "1",
        "operations": [
            {"op": "replace", "path": "test.txt", "from": "no", "to": "has"}
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
        .success();

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
            "version": "1",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "no trailing newline\n");
}

#[test]
fn test_tx_yaml_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "old value\n").unwrap();

    let yaml_plan = format!(
        "version: \"1\"\noperations:\n  - op: replace\n    path: \"{}\"\n    from: old\n    to: new\n",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "new value\n");
}

#[test]
fn test_tx_toml_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("target.txt");
    fs::write(&file, "hello world\n").unwrap();

    let toml_plan = format!(
        "version = \"1\"\n\n[[operations]]\nop = \"replace\"\npath = \"{}\"\nfrom = \"hello\"\nto = \"goodbye\"\n",
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "goodbye world\n");
}

#[test]
fn test_tx_yaml_plan_from_stdin() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.txt");
    fs::write(&file, "aaa\n").unwrap();

    let yaml_plan = format!(
        "version: \"1\"\noperations:\n  - op: replace\n    path: \"{}\"\n    from: aaa\n    to: bbb\n",
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
        .success();

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
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "hello",
            "to": "world"
        }]
    });

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tx")
        .arg("-")
        .arg("--apply")
        .write_stdin(serde_json::to_string(&plan).unwrap())
        .assert()
        .success();

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
            "version": "1",
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
        .success();

    // File should exist with the new content, not deleted.
    assert!(
        file.exists(),
        "file should not be deleted after a subsequent create"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "new content");
}

#[test]
fn test_tx_format_and_validate_success_path() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "content\n").unwrap();

    let marker = dir.path().join("format_ran.marker");

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "from": "content",
            "to": "changed"
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
        .success();

    assert_eq!(fs::read_to_string(&file).unwrap(), "changed\n");
    assert!(
        marker.exists(),
        "format step should have created the marker"
    );
}

#[test]
fn test_replace_nth_regex_replaces_only_nth() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "v1.0 and v2.0 and v3.0\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("--regex")
        .arg(r"v\d+\.\d+")
        .arg("--to")
        .arg("vX.Y")
        .arg("--nth")
        .arg("2")
        .arg("--apply")
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "v1.0 and vX.Y and v3.0\n");
}

#[test]
fn test_tx_replace_nth_regex_with_capture_groups_in_plan() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "version = \"1.2.3\"\nversion = \"4.5.6\"\n").unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [{
            "op": "replace",
            "path": file.to_str().unwrap(),
            "mode": "regex",
            "from": r#"version = "(\d+)\.(\d+)\.(\d+)""#,
            "to": r#"version = "$1.$2.99""#,
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
        .success();

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "version = \"1.2.3\"\nversion = \"4.5.99\"\n");
}

#[test]
fn test_delete_default_dry_run_does_not_remove() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("safe.txt");
    fs::write(&file, "content\n").unwrap();

    // Default mode (no --apply, no --check) is a dry-run.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("would delete"));

    assert!(file.exists(), "dry-run should not delete the file");
}

#[test]
fn test_delete_quiet_dry_run_suppresses_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("quiet_delete.txt");
    fs::write(&file, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("delete")
        .arg(file.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    assert!(file.exists(), "quiet dry-run should not delete the file");
}

#[test]
fn test_md_table_append_missing_file_includes_path_in_error() {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("table-append")
        .arg("does-not-exist.md")
        .arg("--heading")
        .arg("## T")
        .arg("--row")
        .arg("| a | b |")
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("does-not-exist.md"));
}

#[test]
fn test_md_insert_before_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-before-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--content")
        .arg("text")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_md_insert_after_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("insert-after-heading")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--content")
        .arg("text")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_md_upsert_bullet_heading_not_found() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\nContent.\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("md")
        .arg("upsert-bullet")
        .arg(file.to_str().unwrap())
        .arg("--heading")
        .arg("Nonexistent")
        .arg("--bullet")
        .arg("new item")
        .arg("--apply")
        .assert()
        .code(3); // NO_MATCHES
}

#[test]
fn test_tx_doc_prepend_on_non_array_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"name": "not_an_array"}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
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
        .code(7); // ROLLBACK

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
            "version": "1",
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
        .code(7); // ROLLBACK
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
            "version": "1",
        "operations": [
            {"op": "replace", "path": txt.to_str().unwrap(), "from": "foo", "to": "XXX", "nth": 1},
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
        .success();

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
fn test_tx_create_then_replace_on_same_file() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("created.txt");

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {
                "op": "file.create",
                "path": new_file.to_str().unwrap(),
                "content": "hello world\n"
            },
            {
                "op": "replace",
                "path": new_file.to_str().unwrap(),
                "from": "world",
                "to": "patchloom"
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
        .success();

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
            "version": "1",
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
        .success();

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
fn test_tx_doc_set_then_replace_on_same_file_flushes_cache() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"name": "old", "version": "1.0"}"#).unwrap();

    let plan = serde_json::json!({
            "version": "1",
        "operations": [
            {"op": "doc.set", "path": file.to_str().unwrap(), "selector": "name", "value": "new"},
            {"op": "replace", "path": file.to_str().unwrap(), "from": "1.0", "to": "2.0"}
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
        .success();

    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["name"], "new", "doc.set mutation lost after cache flush");
    assert_eq!(v["version"], "2.0", "replace did not see flushed content");
}

// ---------------------------------------------------------------------------
// smoke tests: docs and examples
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example_plan_path(name: &str) -> PathBuf {
    repo_root().join("examples").join(name)
}

fn quickstart_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("quickstart.md")
}

fn installation_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("installation.md")
}

fn agent_test_readme_path() -> PathBuf {
    repo_root().join("tests").join("agent").join("README.md")
}

fn concepts_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("getting-started")
        .join("concepts.md")
}

fn ci_workflow_path() -> PathBuf {
    repo_root().join(".github").join("workflows").join("ci.yml")
}

fn readme_path() -> PathBuf {
    repo_root().join("README.md")
}

fn launch_announcement_path() -> PathBuf {
    repo_root()
        .join("docs")
        .join("blog")
        .join("launch-announcement.md")
}

fn patchloom_in(cwd: &Path) -> Command {
    let mut cmd = Command::cargo_bin("patchloom").unwrap();
    cmd.arg("--cwd").arg(cwd);
    cmd
}

fn write_fixture_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn seed_docs_smoke_fixture(root: &Path) {
    write_fixture_file(
        root,
        "Cargo.toml",
        "[package]\nname = \"smoke-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_fixture_file(
        root,
        "src/lib.rs",
        "pub mod write;\n\n// TODO: rename old_function\n// TODO: update docs\npub fn old_function() -> &'static str {\n    \"old\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/write.rs",
        "pub fn existing_write() -> &'static str {\n    \"write\"\n}\n",
    );
    write_fixture_file(
        root,
        "src/rename.rs",
        "pub fn old_name() -> &'static str {\n    \"old_name\"\n}\n",
    );
    write_fixture_file(
        root,
        "README.md",
        "# Smoke Fixture\n\nCurrent release: v1.0.0\nDocs version: v0.1.0\n\n## Commands\n\n| Command | Description |\n|---|---|\n| `search` | Search repo |\n",
    );
    write_fixture_file(
        root,
        "CHANGELOG.md",
        "# Changelog\n\n## Unreleased\n\n- Existing entry\n\n## 0.1.0\n\n- Initial release\n",
    );
    write_fixture_file(
        root,
        "AGENTS.md",
        "# AGENTS.md\n\n## Safety rules\n\n- existing rule\n\n## Safety rules\n\n- duplicate rule\n",
    );
    write_fixture_file(
        root,
        "package.json",
        "{\n  \"name\": \"smoke-fixture\",\n  \"version\": \"1.0.0\"\n}\n",
    );
    write_fixture_file(
        root,
        "config.json",
        "{\n  \"database\": {\n    \"host\": \"localhost\",\n    \"port\": 3306\n  },\n  \"allowed_origins\": [\"http://localhost:3000\"],\n  \"deprecated_field\": true\n}\n",
    );
    write_fixture_file(
        root,
        "config.yaml",
        "users:\n  - name: alice\n    active: true\n  - name: bob\n    active: false\n",
    );
}

fn extract_markdown_code_block_after(markdown: &str, marker: &str, language: &str) -> String {
    let (_, after_marker) = markdown
        .split_once(marker)
        .expect("marker should exist in markdown");
    let fence = format!("```{language}\n");
    let (_, after_fence) = after_marker
        .split_once(&fence)
        .expect("fenced code block should exist after marker");
    let (block, _) = after_fence
        .split_once("\n```")
        .expect("fenced code block should terminate");
    block.to_string()
}

#[test]
fn test_smoke_example_01_basic_replace_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("01-basic-replace.json"))
        .arg("--apply")
        .assert()
        .success();

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v2.0.0"));
    assert!(!readme.contains("v1.0.0"));
}

#[test]
fn test_smoke_example_02_multi_file_batch_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("02-multi-file-batch.json"))
        .arg("--apply")
        .assert()
        .success();

    let cargo_toml = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo_toml.contains("version = \"0.2.0\""));
    assert!(!cargo_toml.contains("version = \"0.1.0\""));

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "0.2.0");

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("v0.2.0"));
    assert!(!readme.contains("v0.1.0"));
}

#[test]
fn test_smoke_example_03_markdown_editing_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("03-markdown-editing.json"))
        .arg("--apply")
        .assert()
        .success();

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new feature"));
    assert!(changelog.contains("- Fixed bug in parser"));
    assert!(!changelog.contains("- Existing entry"));

    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert_eq!(agents.matches("## Safety rules").count(), 1);
    assert!(agents.contains("Always run `make check` before committing"));

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(readme.contains("| `new-cmd` | Description of the new command |"));
}

#[test]
fn test_smoke_example_04_doc_mutations_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("04-doc-mutations.json"))
        .arg("--apply")
        .assert()
        .success();

    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["database"]["port"], 5432);
    assert_eq!(config["database"]["pool_size"], 10);
    assert_eq!(config["logging"]["level"], "info");
    assert_eq!(config["logging"]["format"], "json");
    assert!(
        config["allowed_origins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str() == Some("https://example.com"))
    );
    assert!(config.get("deprecated_field").is_none());

    let yaml = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(yaml.contains("active: true"));
    assert!(!yaml.contains("active: false"));
    assert!(!yaml.contains("bob"));
}

#[test]
fn test_smoke_example_05_strict_mode_plan() {
    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("05-strict-mode.json"))
        .arg("--apply")
        .assert()
        .success();

    let new_module = fs::read_to_string(dir.path().join("src/new_module.rs")).unwrap();
    assert!(new_module.contains("pub fn hello() -> &'static str"));

    let lib_rs = fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("pub mod new_module;"));

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("- Added new_module"));
}

#[test]
fn test_smoke_example_06_batch_version_bump() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the batch example operations.
    fs::write(
        dir.path().join("package.json"),
        "{\n  \"version\": \"1.9.0\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nversion = \"1.9.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nversion = \"1.9.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("config")).unwrap();
    fs::write(
        dir.path().join("config/settings.yaml"),
        "app:\n  version: \"1.9.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("README.md"),
        "# Project\n\n![version](https://img.shields.io/badge/version-1.9.0-blue)\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n## v1.9.0\n- Initial\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(example_plan_path("06-batch-version-bump.txt"))
        .arg("--apply")
        .assert()
        .success();

    let pkg = fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(pkg.contains("2.0.0"), "package.json not updated: {pkg}");

    let cargo = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(cargo.contains("2.0.0"), "Cargo.toml not updated: {cargo}");

    let readme = fs::read_to_string(dir.path().join("README.md")).unwrap();
    assert!(
        readme.contains("version-2.0.0-blue"),
        "README badge not updated: {readme}"
    );

    let pyproject = fs::read_to_string(dir.path().join("pyproject.toml")).unwrap();
    assert!(
        pyproject.contains("2.0.0"),
        "pyproject.toml not updated: {pyproject}"
    );

    let settings = fs::read_to_string(dir.path().join("config/settings.yaml")).unwrap();
    assert!(
        settings.contains("2.0.0"),
        "config/settings.yaml not updated: {settings}"
    );

    let changelog = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains("Bump version to 2.0.0"),
        "CHANGELOG bullet not added: {changelog}"
    );
}

#[test]
fn test_smoke_example_07_yaml_plan() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the YAML plan operations.
    fs::write(
        dir.path().join("config.yaml"),
        "app:\n  version: \"1.0.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("CHANGELOG.md"),
        "# Changelog\n\n## Unreleased\n\n- Initial\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("README.md"),
        "# Project v1.0.0\n\nUsing v1.0.0 everywhere.\n",
    )
    .unwrap();

    // --diff mode: plan should parse successfully.
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("07-yaml-plan.yaml"))
        .arg("--diff")
        .assert()
        .success();

    // --apply: skip format/validate (prettier/yamllint may not exist).
    // Instead verify the plan parses and operations apply.
    let _ = patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("07-yaml-plan.yaml"))
        .arg("--apply")
        .assert();

    let config = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
    assert!(
        config.contains("2.0.0"),
        "config.yaml version not updated: {config}"
    );
}

#[test]
fn test_smoke_example_09_patch_apply() {
    let dir = TempDir::new().unwrap();

    // Create the fixture file that the patch targets.
    fs::create_dir_all(dir.path().join("src")).unwrap();
    // Fixture must match the line numbers in the patch hunks:
    // Hunk 1: @@ -10,7  =>  pub struct Config { at line 10
    // Hunk 2: @@ -20,6  =>  Config { at line 20
    fs::write(
        dir.path().join("src/config.rs"),
        "// Config module\n\n\
         use std::time::Duration;\n\n\
         const MAX_RETRIES: u32 = 5;\n\
         const DEFAULT_PORT: u16 = 8080;\n\n\
         /// Application configuration.\n\
         #[derive(Debug, Clone)]\n\
         pub struct Config {\n\
         \x20\x20\x20\x20pub host: String,\n\
         \x20\x20\x20\x20pub port: u16,\n\
         \x20\x20\x20\x20pub timeout: u64,\n\
         \x20\x20\x20\x20pub retries: u32,\n\
         }\n\n\
         impl Default for Config {\n\
         \x20\x20\x20\x20fn default() -> Self {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20// Create with sensible defaults\n\
         \x20\x20\x20\x20\x20\x20\x20\x20Config {\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20host: \"localhost\".to_string(),\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20port: 8080,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20timeout: 30,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20retries: 3,\n\
         \x20\x20\x20\x20\x20\x20\x20\x20}\n\
         \x20\x20\x20\x20}\n\
         }\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("09-patch-apply.json"))
        .arg("--diff")
        .assert()
        .success();
}

#[test]
fn test_smoke_example_10_inspect_and_edit() {
    let dir = TempDir::new().unwrap();

    // Create fixture files matching the plan's operations.
    fs::write(
        dir.path().join("config.json"),
        "{\n  \"database\": {\n    \"port\": 5432\n  }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/db.rs"),
        "const DB_PORT: u16 = 5432;\n\nfn connect() {\n    // ...\n}\n",
    )
    .unwrap();

    // --diff mode: should parse and preview changes.
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("10-inspect-and-edit.json"))
        .arg("--diff")
        .assert()
        .success();

    // --apply: verify writes landed.
    patchloom_in(dir.path())
        .arg("tx")
        .arg(example_plan_path("10-inspect-and-edit.json"))
        .arg("--apply")
        .assert()
        .success();

    let config = fs::read_to_string(dir.path().join("config.json")).unwrap();
    assert!(
        config.contains("5433"),
        "config.json port not updated: {config}"
    );

    let db = fs::read_to_string(dir.path().join("src/db.rs")).unwrap();
    assert!(
        db.contains("DB_PORT: u16 = 5433"),
        "db.rs not updated: {db}"
    );
}

#[test]
fn test_smoke_example_08_mcp_tool_names_valid() {
    // Parse example 08 and verify every tool name exists in the MCP tool list
    // produced by `patchloom agent-rules --mode mcp`.
    let example = fs::read_to_string(example_plan_path("08-mcp-tool-call.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&example).unwrap();
    let tool_names: Vec<&str> = doc["examples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["tool"].as_str().unwrap())
        .collect();
    assert!(!tool_names.is_empty(), "example 08 has no tool examples");

    // Get the authoritative tool list from agent-rules --mode mcp.
    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .args(["agent-rules", "--mode", "mcp"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let rules = String::from_utf8(output.stdout).unwrap();

    for name in &tool_names {
        assert!(
            rules.contains(name),
            "example 08 references tool '{name}' which is not in agent-rules --mode mcp output"
        );
    }
}

// ── Batch integration tests ───────────────────────────────────────────

#[test]
fn test_batch_diff_mode_does_not_write() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .assert()
        .success();

    // File must be unchanged in default (diff) mode.
    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("1.0.0"),
        "file should be unchanged: {content}"
    );
}

#[test]
fn test_batch_apply_modifies_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("2.0.0"),
        "file should be updated: {content}"
    );
}

#[test]
fn test_batch_empty_input_succeeds() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicates::str::contains("no operations found"));
}

#[test]
fn test_batch_comment_only_input_succeeds() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("comments.txt");
    fs::write(&ops, "# This is a comment\n\n# Another comment\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success()
        .stderr(predicates::str::contains("no operations found"));
}

#[test]
fn test_batch_json_empty_input_returns_structured_success() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 0);
    assert_eq!(json["files_created"], 0);
    assert_eq!(json["files_deleted"], 0);
    assert_eq!(json["changes"].as_array().unwrap().len(), 0);
}

#[test]
fn test_batch_jsonl_empty_input_returns_structured_success() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "# only a comment\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--jsonl")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "JSONL output should be a single line");
    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 0);
}

#[test]
fn test_batch_malformed_line_fails() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad.txt");
    fs::write(&ops, "unknown.op foo bar\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("unknown operation"));
}

#[test]
fn test_batch_extra_args_fail() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("bad.txt");
    fs::write(&ops, "file.delete old.txt extra\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("requires exactly 1 arguments"));
}

#[test]
fn test_batch_nonexistent_target_file_rollback() {
    let dir = TempDir::new().unwrap();
    // Do NOT create missing.json.
    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set missing.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .code(7); // ROLLBACK
}

#[test]
fn test_batch_from_stdin() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"old"}"#).unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin("doc.set data.json name \"new\"\n")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains("new"),
        "stdin batch should update file: {content}"
    );
}

#[test]
fn test_batch_check_mode_reports_changes() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("pkg.json"), r#"{"version":"1.0.0"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set pkg.json version \"2.0.0\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--check")
        .assert()
        .code(2); // CHANGES_DETECTED

    // File must be unchanged in --check mode.
    let content = fs::read_to_string(dir.path().join("pkg.json")).unwrap();
    assert!(
        content.contains("1.0.0"),
        "file should be unchanged in check mode: {content}"
    );
}

#[test]
fn test_batch_quiet_suppresses_empty_message() {
    let dir = TempDir::new().unwrap();
    let ops = dir.path().join("empty.txt");
    fs::write(&ops, "").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "quiet should suppress stderr, got: {stderr}"
    );
}

#[test]
fn test_batch_json_output_on_apply() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"key":"old"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set data.json key \"new\"\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--json")
        .arg("--cwd")
        .arg(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .output()
        .unwrap();

    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["status"], "success");
    assert_eq!(json["files_changed"], 1);
}

#[test]
fn test_batch_empty_quoted_string_sets_empty_value() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"old"}"#).unwrap();

    let ops = dir.path().join("ops.txt");
    fs::write(&ops, "doc.set data.json name \"\"\n").unwrap();

    patchloom_in(dir.path())
        .arg("batch")
        .arg(&ops)
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("data.json")).unwrap();
    assert!(
        content.contains(r#""name": """#) || content.contains(r#""name":"""#),
        "empty quoted string should set value to empty string, got: {content}"
    );
}

#[test]
fn test_doc_get_honors_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        "{\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));
}

#[test]
fn test_md_insert_before_heading_honors_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\n## Section A\n\nBody A\n\n## Section B\n\nBody B\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("md")
        .arg("insert-before-heading")
        .arg("doc.md")
        .arg("--heading")
        .arg("Section B")
        .arg("--content")
        .arg("Inserted before B.")
        .arg("--apply")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(content.contains("Inserted before B.\n\n## Section B"));
}

#[test]
fn test_tx_honors_cwd_for_relative_plan_path() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("plan.toml"),
        "version = \"1\"\n\n[[operations]]\nop = \"file.create\"\npath = \"out.txt\"\ncontent = \"hello\\n\"\n",
    )
    .unwrap();

    patchloom_in(dir.path())
        .arg("tx")
        .arg("plan.toml")
        .arg("--apply")
        .assert()
        .success();

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
        "version = \"1\"\ncwd = \"nested\"\n\n[[operations]]\nop = \"file.create\"\npath = \"out.txt\"\ncontent = \"hello\\n\"\n",
    )
    .unwrap();

    patchloom_in(&repo)
        .arg("tx")
        .arg("plan.toml")
        .arg("--apply")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(nested.join("out.txt")).unwrap(),
        "hello\n"
    );
    assert!(!repo.join("out.txt").exists());
}

#[test]
fn test_smoke_quickstart_command_flow() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    assert!(quickstart.contains("patchloom search 'TODO' src/"));
    assert!(quickstart.contains("patchloom search 'TODO' --count src/"));
    assert!(quickstart.contains("patchloom replace 'old_function' --to 'new_function' src/"));
    assert!(
        quickstart.contains("patchloom replace 'old_function' --to 'new_function' src/ --apply")
    );
    assert!(quickstart.contains("patchloom init"));
    assert!(quickstart.contains("appends the rules to an existing agent instructions file"));
    assert!(quickstart.contains(".vscode/mcp.json"));
    assert!(quickstart.contains(".cursor/mcp.json"));
    assert!(quickstart.contains("patchloom doc get package.json version"));
    assert!(quickstart.contains("patchloom doc set package.json version \"2.0.0\" --apply"));
    assert!(quickstart.contains("patchloom batch <<'EOF'"));
    assert!(quickstart.contains("patchloom batch --apply <<'EOF'"));
    assert!(quickstart.contains("patchloom status"));
    assert!(quickstart.contains("`patchloom status` is git-backed."));
    assert!(quickstart.contains("patchloom undo"));
    assert!(quickstart.contains("patchloom undo --apply"));

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    git_ok(dir.path(), &["init"]);
    git_ok(dir.path(), &["config", "user.email", "test@test.com"]);
    git_ok(dir.path(), &["config", "user.name", "Test"]);
    git_ok(
        dir.path(),
        &[
            "add",
            "Cargo.toml",
            "src",
            "README.md",
            "CHANGELOG.md",
            "AGENTS.md",
            "package.json",
            "config.json",
            "config.yaml",
        ],
    );
    git_ok(dir.path(), &["commit", "-m", "init"]);
    let lib_path = dir.path().join("src/lib.rs");

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("--count")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains(":2"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_function")
        .arg("--to")
        .arg("new_function")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("new_function"));
    assert!(
        fs::read_to_string(&lib_path)
            .unwrap()
            .contains("old_function")
    );

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_function")
        .arg("--to")
        .arg("new_function")
        .arg("src/")
        .arg("--apply")
        .assert()
        .success();
    assert!(
        fs::read_to_string(&lib_path)
            .unwrap()
            .contains("new_function")
    );
    assert!(
        !fs::read_to_string(&lib_path)
            .unwrap()
            .contains("old_function")
    );

    patchloom_in(dir.path())
        .arg("doc")
        .arg("get")
        .arg("package.json")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("set")
        .arg("package.json")
        .arg("version")
        .arg("\"2.0.0\"")
        .arg("--apply")
        .assert()
        .success();

    let package: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package["version"], "2.0.0");

    let batch_preview = patchloom_in(dir.path())
        .arg("batch")
        .write_stdin(
            "doc.set package.json version \"3.0.0\"\nreplace README.md \"v1.0.0\" \"v3.0.0\"\nmd.insert_after_heading CHANGELOG.md \"## Unreleased\" \"- Bumped to v3.0.0\"\n",
        )
        .output()
        .unwrap();
    assert!(batch_preview.status.success());
    let batch_preview_stdout = String::from_utf8_lossy(&batch_preview.stdout);
    assert!(batch_preview_stdout.contains("package.json"));
    assert!(batch_preview_stdout.contains("README.md"));
    assert!(batch_preview_stdout.contains("CHANGELOG.md"));

    let batch_apply = patchloom_in(dir.path())
        .arg("batch")
        .arg("--apply")
        .write_stdin(
            "doc.set package.json version \"3.0.0\"\nreplace README.md \"v1.0.0\" \"v3.0.0\"\nmd.insert_after_heading CHANGELOG.md \"## Unreleased\" \"- Bumped to v3.0.0\"\n",
        )
        .output()
        .unwrap();
    assert!(batch_apply.status.success());

    let package_after_batch: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after_batch["version"], "3.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v3.0.0")
    );
    assert!(
        fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v3.0.0")
    );

    let status_output = patchloom_in(dir.path()).arg("status").output().unwrap();
    assert_eq!(status_output.status.code(), Some(2));
    let status_stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(status_stdout.contains("README.md"));
    assert!(status_stdout.contains("package.json"));

    patchloom_in(dir.path())
        .arg("undo")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Would restore session"));

    patchloom_in(dir.path())
        .arg("undo")
        .arg("--apply")
        .assert()
        .success();

    let package_after_undo: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after_undo["version"], "2.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v1.0.0")
    );
    assert!(
        !fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v3.0.0")
    );
}

#[test]
fn test_smoke_quickstart_transaction_snippet() {
    let quickstart = fs::read_to_string(quickstart_path()).unwrap();
    let plan_text = extract_markdown_code_block_after(
        &quickstart,
        "Create a plan file called `bump.json`:",
        "json",
    );
    let expected_json: serde_json::Value = serde_json::from_str(
        &extract_markdown_code_block_after(&quickstart, "Returns:", "json"),
    )
    .unwrap();

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());
    let plan_file = dir.path().join("bump.json");
    fs::write(&plan_file, plan_text).unwrap();

    let diff_output = patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .output()
        .unwrap();
    assert!(diff_output.status.success());
    let diff_stdout = String::from_utf8_lossy(&diff_output.stdout);
    assert!(diff_stdout.contains("package.json"));
    assert!(diff_stdout.contains("README.md"));
    assert!(diff_stdout.contains("CHANGELOG.md"));

    let package_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_before["version"], "1.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v1.0.0")
    );
    assert!(
        !fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v2.0.0")
    );

    let check_output = patchloom_in(dir.path())
        .arg("tx")
        .arg(&plan_file)
        .arg("--check")
        .output()
        .unwrap();
    assert_eq!(check_output.status.code(), Some(2));

    let apply_output = patchloom_in(dir.path())
        .arg("--json")
        .arg("tx")
        .arg(&plan_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(apply_output.status.success());
    let actual_json: serde_json::Value = serde_json::from_slice(&apply_output.stdout).unwrap();
    assert_eq!(actual_json, expected_json);

    let package_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("package.json")).unwrap())
            .unwrap();
    assert_eq!(package_after["version"], "2.0.0");
    assert!(
        fs::read_to_string(dir.path().join("README.md"))
            .unwrap()
            .contains("v2.0.0")
    );
    assert!(
        fs::read_to_string(dir.path().join("CHANGELOG.md"))
            .unwrap()
            .contains("- Bumped to v2.0.0")
    );
}

#[test]
fn test_smoke_shell_completion_docs_include_elvish() {
    // Shell completions docs live in the installation guide (not README).
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("patchloom completions elvish"),
        "installation guide should document elvish completions"
    );
}

#[test]
fn test_smoke_source_install_docs_use_cargo_install_path() {
    let source_install_flow = "git clone https://github.com/patchloom/patchloom.git\ncd patchloom\ncargo install --path .";

    for (path, label) in [
        (installation_path(), "installation guide"),
        (readme_path(), "README"),
    ] {
        let content = fs::read_to_string(path).unwrap();
        assert!(
            content.contains(source_install_flow),
            "{label} should document the first-run source install flow"
        );
    }
}

#[test]
fn test_smoke_installation_docs_cover_mcp_feature_paths() {
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("cargo install --path . --features mcp"),
        "installation guide should document MCP-capable source installs"
    );
    assert!(
        content.contains("cargo install patchloom --features mcp"),
        "installation guide should document MCP-capable crates.io installs"
    );
}

#[test]
fn test_smoke_installation_docs_cover_contributor_verification_loop() {
    let content = fs::read_to_string(installation_path()).unwrap();
    assert!(
        content.contains("make check-fast"),
        "installation guide should mention the fast contributor iteration loop"
    );
    assert!(
        content.contains("make check"),
        "installation guide should mention the full contributor verification gate"
    );
}

#[test]
fn test_agent_test_readme_uses_virtualenv_for_direct_install() {
    let content = fs::read_to_string(agent_test_readme_path()).unwrap();
    assert!(
        content.contains("python3 -m venv .venv"),
        "agent test readme should create a virtualenv before pip install"
    );
    assert!(
        content.contains(". .venv/bin/activate"),
        "agent test readme should show how to activate the virtualenv"
    );
}

#[test]
fn test_smoke_rust_version_docs_and_ci_match_cargo_metadata() {
    let cargo = fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let rust_version_line = cargo
        .lines()
        .find(|line| line.starts_with("rust-version = "))
        .expect("Cargo.toml should declare rust-version");
    let rust_version = rust_version_line
        .split('"')
        .nth(1)
        .expect("rust-version should be quoted");

    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains(&format!("Rust {rust_version}+")),
        "README should advertise the same Rust minimum version as Cargo.toml"
    );

    let contributing = fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap();
    assert!(
        contributing.contains(&format!("Rust {rust_version}+")),
        "CONTRIBUTING.md should advertise the same Rust minimum version as Cargo.toml"
    );

    let changelog = fs::read_to_string(repo_root().join("CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains(&format!("MSRV: Rust {rust_version}+")),
        "CHANGELOG.md should advertise the same Rust minimum version as Cargo.toml"
    );

    let installation = fs::read_to_string(installation_path()).unwrap();
    assert!(
        installation.contains(&format!("requires Rust {rust_version}+")),
        "installation guide should advertise the same Rust minimum version as Cargo.toml"
    );

    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(
        ci.contains(&format!("toolchain: \"{rust_version}\"")),
        "ci.yml should pin the MSRV job to the same Rust version as Cargo.toml"
    );
}

#[test]
fn test_contributing_make_targets_table_covers_key_targets() {
    let contributing = fs::read_to_string(repo_root().join("CONTRIBUTING.md")).unwrap();
    for target in [
        "make check",
        "make check-fast",
        "make build",
        "make fmt",
        "make test",
        "make integration-test",
        "make clippy",
        "make update-readme",
        "cargo check --all-targets",
    ] {
        assert!(
            contributing.contains(&format!("| `{target}`")),
            "CONTRIBUTING.md make-targets table should list {target}"
        );
    }
}

#[test]
fn test_makefile_has_audit_target() {
    let makefile = fs::read_to_string(repo_root().join("Makefile")).unwrap();
    assert!(
        makefile.contains("audit:"),
        "Makefile should have an audit target for local vulnerability scanning"
    );
}

#[test]
fn test_readme_documents_allow_shell_flag() {
    let readme = fs::read_to_string(repo_root().join("README.md")).unwrap();
    assert!(
        readme.contains("--allow-shell"),
        "README should mention --allow-shell for MCP server"
    );
}

#[test]
fn test_mcp_setup_documents_allow_shell_flag() {
    let doc = fs::read_to_string(repo_root().join("docs/getting-started/mcp-setup.md")).unwrap();
    assert!(
        doc.contains("--allow-shell"),
        "mcp-setup.md should document the --allow-shell flag"
    );
}

#[test]
fn test_ci_workflow_routes_macos_fork_prs_to_github_hosted_runners() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    let fork_safe_macos_runs_on = r#"runs-on: ${{ (github.event_name == 'pull_request' && github.event.pull_request.head.repo.fork) && 'macos-latest' || fromJson('["self-hosted","macOS","ARM64"]') }}"#;

    assert_eq!(
        ci.matches(fork_safe_macos_runs_on).count(),
        2,
        "ci.yml should route both macOS jobs to GitHub-hosted runners for fork PRs"
    );
}

#[test]
fn test_ci_workflow_uses_runner_temp_for_bench_fixtures() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();

    assert!(
        ci.contains("\"$RUNNER_TEMP/bench.json\"") || ci.contains("$RUNNER_TEMP/bench.json"),
        "ci.yml should keep benchmark JSON fixtures under RUNNER_TEMP"
    );
    assert!(
        ci.contains("\"$RUNNER_TEMP/bench.txt\"") || ci.contains("$RUNNER_TEMP/bench.txt"),
        "ci.yml should keep benchmark text fixtures under RUNNER_TEMP"
    );
    assert!(!ci.contains("/tmp/bench.json"));
    assert!(!ci.contains("/tmp/bench.txt"));
}

#[test]
fn test_ci_bench_steps_use_shared_threshold_script() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(
        ci.contains("benches/ci/check_threshold.py"),
        "ci.yml bench steps should call the shared check_threshold.py script"
    );
    assert!(
        !ci.contains("python3 - \"$bench_json\""),
        "ci.yml should not inline the threshold Python snippet"
    );
    assert!(
        repo_root().join("benches/ci/check_threshold.py").exists(),
        "benches/ci/check_threshold.py must exist on disk"
    );
}

#[test]
fn test_workflows_disable_persisted_checkout_credentials_by_default() {
    let ci = fs::read_to_string(ci_workflow_path()).unwrap();
    assert!(ci.matches("persist-credentials: false").count() >= 8);

    let security = fs::read_to_string(repo_root().join(".github/workflows/security.yml")).unwrap();
    assert_eq!(security.matches("persist-credentials: false").count(), 2);

    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert_eq!(bench.matches("persist-credentials: false").count(), 1);
}

#[test]
fn test_publish_crates_workflow_serializes_publishes_per_ref() {
    let publish =
        fs::read_to_string(repo_root().join(".github/workflows/publish-crates.yml")).unwrap();
    assert!(
        publish.contains("group: publish-crates-${{ github.ref }}"),
        "publish-crates workflow should serialize publishes per ref"
    );
    assert!(
        publish.contains("cancel-in-progress: false"),
        "publish-crates workflow should queue duplicate publishes instead of cancelling them"
    );
}

#[test]
fn test_bench_workflow_limits_artifact_retention() {
    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert!(
        bench.contains("retention-days: 14"),
        "benchmark artifacts should use a short explicit retention period"
    );
}

#[test]
fn test_bench_workflow_passes_dispatch_scales_via_env() {
    let bench = fs::read_to_string(repo_root().join(".github/workflows/bench.yml")).unwrap();
    assert!(
        bench.contains("BENCH_SCALES: ${{ inputs.scales || 'small medium' }}"),
        "bench workflow should pass dispatch scales through an environment variable"
    );
    assert!(
        bench.contains("bash run.sh \"$BENCH_SCALES\""),
        "bench workflow should quote BENCH_SCALES when invoking the benchmark runner"
    );
    assert!(
        !bench.contains("bash run.sh ${{ inputs.scales || 'small medium' }}"),
        "bench workflow should not interpolate workflow inputs directly into the shell command"
    );
}

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
fn test_readme_agent_test_count_matches_non_benchmark_scenarios() {
    let count = fs::read_dir(repo_root().join("tests/agent"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("test_") && name.ends_with(".py") && name != "test_bench.py"
        })
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .map(|content| {
            content
                .lines()
                .filter(|line| line.trim_start().starts_with("def test_"))
                .count()
        })
        .sum::<usize>();

    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains(&format!("`make agent-test` runs {count} pytest scenarios")),
        "README should document the non-benchmark agent scenario count run by make agent-test"
    );
}

#[test]
fn test_smoke_readme_command_examples() {
    // README links to the reference doc; detailed examples live there.
    let readme = fs::read_to_string(readme_path()).unwrap();
    assert!(
        readme.contains("patchloom init"),
        "README quick start should lead with patchloom init"
    );
    assert!(
        readme.contains("appends the rules to an existing agent instructions file"),
        "README should describe init append behavior for existing agent instruction files"
    );
    assert!(
        readme.contains(".vscode/mcp.json"),
        "README should mention the VS Code MCP config path"
    );
    assert!(
        readme.contains(".cursor/mcp.json"),
        "README should mention the Cursor MCP config path"
    );
    assert!(
        readme.contains("docs/reference/README.md"),
        "README should link to the command reference"
    );
    // Verify the reference doc contains the detailed examples.
    let reference = fs::read_to_string(repo_root().join("docs/reference/README.md")).unwrap();
    assert!(reference.contains(".vscode/mcp.json"));
    assert!(reference.contains(".cursor/mcp.json"));
    assert!(reference.contains("`search`"));
    assert!(reference.contains("`replace`"));
    assert!(reference.contains("`doc`"));
    assert!(reference.contains("`tidy`"));
    let launch = fs::read_to_string(launch_announcement_path()).unwrap();
    assert!(launch.contains("appends the rules to an existing agent instructions file"));
    assert!(launch.contains(".vscode/mcp.json"));
    assert!(launch.contains(".cursor/mcp.json"));
    assert!(launch.contains("1,100+ tests"));
    let merge_value = r#"{"settings": {"debug": true}}"#;

    let dir = TempDir::new().unwrap();
    seed_docs_smoke_fixture(dir.path());

    patchloom_in(dir.path())
        .arg("search")
        .arg("TODO")
        .arg("src/")
        .assert()
        .success()
        .stdout(predicate::str::contains("TODO"));

    patchloom_in(dir.path())
        .arg("replace")
        .arg("old_name")
        .arg("--to")
        .arg("new_name")
        .arg("src/")
        .arg("--apply")
        .assert()
        .success();
    let rename_module = fs::read_to_string(dir.path().join("src/rename.rs")).unwrap();
    assert!(rename_module.contains("new_name"));
    assert!(!rename_module.contains("old_name"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("keys")
        .arg("package.json")
        .arg(".")
        .assert()
        .success()
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("version"));

    patchloom_in(dir.path())
        .arg("doc")
        .arg("merge")
        .arg("config.json")
        .arg("--value")
        .arg(merge_value)
        .arg("--apply")
        .assert()
        .success();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("config.json")).unwrap()).unwrap();
    assert_eq!(config["settings"]["debug"], true);

    let tidy_target = dir.path().join("notes.txt");
    fs::write(&tidy_target, "line with space \nsecond line").unwrap();

    patchloom_in(dir.path())
        .arg("tidy")
        .arg("check")
        .arg("notes.txt")
        .assert()
        .code(2);

    patchloom_in(dir.path())
        .arg("tidy")
        .arg("fix")
        .arg(".")
        .arg("--ensure-final-newline")
        .arg("--apply")
        .assert()
        .success();
    assert!(fs::read(&tidy_target).unwrap().ends_with(b"\n"));
}

fn reference_path() -> PathBuf {
    repo_root().join("docs").join("reference").join("README.md")
}

fn extract_braced_block(source: &str, anchor: &str) -> String {
    let anchor_start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor `{anchor}` should exist"));
    let body_start = source[anchor_start..]
        .find('{')
        .map(|offset| anchor_start + offset)
        .expect("anchor should be followed by `{`");

    let mut depth = 0usize;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[body_start + 1..body_start + offset].to_string();
                }
            }
            _ => {}
        }
    }

    panic!("anchor `{anchor}` should have a matching closing brace");
}

fn read_anchored_block(path: &Path, anchor: &str) -> String {
    let source = fs::read_to_string(path).unwrap();
    extract_braced_block(&source, anchor)
}

fn camel_to_kebab(name: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if idx > 0 {
                out.push('-');
            }
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn snake_to_kebab(name: &str) -> String {
    name.replace('_', "-")
}

fn collect_enum_variant_cli_names(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    let re = regex::Regex::new(r"(?m)^\s*([A-Z][A-Za-z0-9]*)(?:\s*\(|\s*\{|,).*$").unwrap();
    re.captures_iter(&block)
        .map(|caps| camel_to_kebab(&caps[1]))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_struct_field_names(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    let re = regex::Regex::new(r"(?m)^\s*pub\s+([a-z_]+)\s*:").unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_serde_rename_values(path: &Path, anchor: &str) -> Vec<String> {
    let block = read_anchored_block(path, anchor);
    let re = regex::Regex::new(r#"#\[serde\(rename = "([^"]+)"\)\]"#).unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_batch_operation_names(path: &Path) -> Vec<String> {
    let block = read_anchored_block(path, "match op");
    let re = regex::Regex::new(r#"(?m)^\s*"([^"]+)"\s*=>"#).unwrap();
    re.captures_iter(&block)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn collect_source_ref_markers(path: &Path) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap();
    let re = regex::Regex::new(r"(?m)^\s*//\s*ref:([a-z0-9._:-]+)\s*$").unwrap();
    re.captures_iter(&source)
        .map(|caps| caps[1].to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn expected_reference_markers() -> Vec<String> {
    let root = repo_root();
    let cmd_dir = root.join("src").join("cmd");
    let global_path = root.join("src").join("cli").join("global.rs");
    let plan_path = root.join("src").join("plan.rs");

    let mut markers = std::collections::BTreeSet::new();

    for name in collect_enum_variant_cli_names(&cmd_dir.join("mod.rs"), "pub enum Command") {
        markers.insert(format!("command:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("doc.rs"), "pub enum DocAction") {
        markers.insert(format!("doc-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("md.rs"), "pub enum MdAction") {
        markers.insert(format!("md-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("patch.rs"), "pub enum PatchAction") {
        markers.insert(format!("patch-action:{name}"));
    }
    for name in collect_enum_variant_cli_names(&cmd_dir.join("tidy.rs"), "pub enum TidyAction") {
        markers.insert(format!("tidy-action:{name}"));
    }
    for name in collect_struct_field_names(&plan_path, "pub struct Plan") {
        markers.insert(format!("tx-field:{name}"));
    }
    for name in collect_serde_rename_values(&plan_path, "pub enum Operation") {
        markers.insert(format!("tx-op:{name}"));
    }

    let write_flags: std::collections::BTreeSet<String> =
        collect_struct_field_names(&global_path, "pub struct WriteFlags")
            .into_iter()
            .collect();
    for name in &write_flags {
        markers.insert(format!("write-flag:{}", snake_to_kebab(name)));
    }
    for name in collect_struct_field_names(&global_path, "pub struct GlobalFlags") {
        if !write_flags.contains(&name) {
            markers.insert(format!("global-flag:{}", snake_to_kebab(&name)));
        }
    }

    for entry in fs::read_dir(&cmd_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        for marker in collect_source_ref_markers(&path) {
            markers.insert(marker);
        }
    }

    markers.into_iter().collect()
}

fn reference_markers(reference: &str) -> Vec<(String, usize)> {
    let re = regex::Regex::new(r#"<!--\s*ref:([a-z0-9._:-]+)\s*-->"#).unwrap();
    re.captures_iter(reference)
        .map(|caps| (caps[1].to_string(), caps.get(0).unwrap().start()))
        .collect()
}

fn reference_section<'a>(reference: &'a str, markers: &[(String, usize)], marker: &str) -> &'a str {
    let idx = markers
        .iter()
        .position(|(name, _)| name == marker)
        .unwrap_or_else(|| panic!("marker `{marker}` should exist"));
    let start = markers[idx].1;
    let end = markers
        .get(idx + 1)
        .map(|(_, pos)| *pos)
        .unwrap_or(reference.len());
    &reference[start..end]
}

fn reference_doc_validation_errors(reference: &str) -> Vec<String> {
    let markers = reference_markers(reference);
    let actual_marker_names: std::collections::BTreeSet<String> =
        markers.iter().map(|(name, _)| name.clone()).collect();
    let expected_markers = expected_reference_markers();
    let missing_markers: Vec<String> = expected_markers
        .iter()
        .filter(|marker| !actual_marker_names.contains(*marker))
        .cloned()
        .collect();
    let mut errors = Vec::new();

    if !missing_markers.is_empty() {
        errors.push(format!(
            "reference doc is missing markers for:\n{}",
            missing_markers.join("\n")
        ));
    }

    for marker in expected_markers {
        if !actual_marker_names.contains(&marker) {
            continue;
        }

        let section = reference_section(reference, &markers, &marker);
        if !section.contains("**Use when:**") {
            errors.push(format!(
                "reference section `{marker}` must include a `Use when` stanza"
            ));
        }
    }

    errors
}

fn reference_without_use_when(reference: &str, marker: &str) -> String {
    let markers = reference_markers(reference);
    let section = reference_section(reference, &markers, marker);
    let use_when_start = section
        .find("- **Use when:**")
        .unwrap_or_else(|| panic!("reference section `{marker}` should include `Use when`"));
    let use_when_end = section[use_when_start..]
        .find('\n')
        .map(|offset| use_when_start + offset + 1)
        .unwrap_or(section.len());
    let broken_section = format!("{}{}", &section[..use_when_start], &section[use_when_end..]);

    reference.replacen(section, &broken_section, 1)
}

#[test]
fn test_reference_doc_covers_meaningful_feature_inventory() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let errors = reference_doc_validation_errors(&reference);

    assert!(errors.is_empty(), "{}", errors.join("\n\n"));
}

#[test]
fn test_concepts_doc_mentions_confirm_write_mode() {
    let concepts = fs::read_to_string(concepts_path()).unwrap();
    assert!(concepts.contains("Every write command supports four modes:"));
    assert!(concepts.contains("| `--confirm` | Show the diff, then prompt before writing |"));
}

#[test]
fn test_reference_doc_describes_status_and_undo_contracts() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    assert!(
        reference.contains("This command is git-backed, so it must run inside a git repository.")
    );
    assert!(reference.contains("In dry-run mode, `undo` reports what would be restored and exits with code `2` (`CHANGES_DETECTED`)."));
}

#[test]
fn test_reference_doc_describes_confirm_json_applied_contract_for_file_lifecycle_commands() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let contract = "When combined with `--confirm` and `--json` or `--jsonl`, the structured output includes `applied: true|false` so callers can tell whether the prompt was accepted.";
    assert_eq!(reference.matches(contract).count(), 3);
}

#[test]
fn test_reference_doc_describes_create_input_contract() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    assert!(reference.contains("Exactly one of `--content` or `--stdin` is required."));
    assert!(
        reference
            .contains("Passing both is rejected with `--content and --stdin cannot be combined`")
    );
    assert!(reference.contains(
        "passing neither is rejected with `either --content or --stdin must be provided`"
    ));
}

#[test]
fn test_reference_doc_requires_use_when_stanza() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let broken = reference_without_use_when(&reference, "patch-mode:file");
    let errors = reference_doc_validation_errors(&broken);

    assert!(
        errors.iter().any(|error| error
            .contains("reference section `patch-mode:file` must include a `Use when` stanza")),
        "expected missing `Use when` error, got:\n{}",
        errors.join("\n\n")
    );
}

#[test]
fn test_batch_reference_operation_count_matches_parser() {
    let reference = fs::read_to_string(reference_path()).unwrap();
    let markers = reference_markers(&reference);
    let section = reference_section(&reference, &markers, "command:batch");
    let listed = section
        .split("covers ")
        .nth(1)
        .and_then(|rest| rest.split(" operations (").next())
        .unwrap_or_else(|| panic!("batch reference section should contain an operation count"))
        .parse::<usize>()
        .unwrap();

    let actual =
        collect_batch_operation_names(&repo_root().join("src").join("cmd").join("batch.rs")).len();

    assert_eq!(
        listed, actual,
        "batch reference doc should list the same number of operations the parser supports"
    );
}

#[test]
fn test_agents_doc_project_inventory_matches_repo_state() {
    let agents = fs::read_to_string(repo_root().join("AGENTS.md")).unwrap();

    assert!(
        agents.contains(
            "cmd/mod.rs           Command enum (clap Subcommand), dispatch(), built-in agent-rules"
        ),
        "AGENTS.md should describe the current agent-rules implementation location"
    );
    assert!(
        agents.contains(
            "23 operation types including all doc/md/replace/tidy/file/patch/read/search ops"
        ),
        "AGENTS.md should describe the current tx operation count"
    );
}

// ── binary file handling ─────────────────────────────────────────────

#[test]
fn test_search_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "needle in text\n").unwrap();
    // Binary file: contains NUL byte.
    let bin_file = dir.path().join("data.bin");
    fs::write(&bin_file, b"needle\x00in binary").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("search")
        .arg("needle")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("text.txt"),
        "should find match in text file"
    );
    assert!(
        !stdout.contains("data.bin"),
        "should skip binary file, got: {stdout}"
    );
}

#[test]
fn test_replace_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "old value\n").unwrap();
    // Binary file with the same target text.
    let bin_file = dir.path().join("data.bin");
    let mut bin_content = b"old value".to_vec();
    bin_content.push(0);
    bin_content.extend_from_slice(b" more data");
    fs::write(&bin_file, &bin_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("old value")
        .arg("--to")
        .arg("new value")
        .arg(dir.path().to_str().unwrap())
        .arg("--apply")
        .assert()
        .success();

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "new value\n");

    // Binary file should be untouched.
    let bin_after = fs::read(&bin_file).unwrap();
    assert_eq!(bin_after, bin_content, "binary file should not be modified");
}

#[test]
fn test_tidy_skips_binary_files() {
    let dir = TempDir::new().unwrap();
    // Text file with trailing whitespace.
    let text_file = dir.path().join("text.txt");
    fs::write(&text_file, "hello   \n").unwrap();
    // Binary file with trailing spaces (should not be touched).
    let bin_file = dir.path().join("data.bin");
    let mut bin_content = b"hello   ".to_vec();
    bin_content.push(0);
    fs::write(&bin_file, &bin_content).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("tidy")
        .arg("fix")
        .arg(dir.path().to_str().unwrap())
        .arg("--trim-trailing-whitespace")
        .arg("--apply")
        .assert()
        .success();

    let text_content = fs::read_to_string(&text_file).unwrap();
    assert_eq!(text_content, "hello\n");

    let bin_after = fs::read(&bin_file).unwrap();
    assert_eq!(bin_after, bin_content, "binary file should not be modified");
}

// ---------------------------------------------------------------------------
// status: staged new file (porcelain code "A") shows as created
// ---------------------------------------------------------------------------

#[test]
fn test_status_staged_new_file_shows_as_created() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "init.txt", "init\n");

    // Stage a new file without committing (porcelain code "A ").
    fs::write(dir.path().join("new.txt"), "new content\n").unwrap();
    git_ok(dir.path(), &["add", "new.txt"]);

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2)); // CHANGES_DETECTED
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let created = json["created"].as_array().unwrap();
    assert!(
        created.iter().any(|v| v.as_str() == Some("new.txt")),
        "staged new file should appear in created list, got: {json}"
    );
}

// ---------------------------------------------------------------------------
// status --quiet suppresses output
// ---------------------------------------------------------------------------

#[test]
fn test_status_quiet_suppresses_output() {
    let dir = TempDir::new().unwrap();
    init_git_repo_with_committed_file(dir.path(), "a.txt", "hello\n");

    // Modify the committed file to produce changes.
    fs::write(dir.path().join("a.txt"), "changed\n").unwrap();

    let output = Command::cargo_bin("patchloom")
        .unwrap()
        .arg("--quiet")
        .arg("--cwd")
        .arg(dir.path().to_str().unwrap())
        .arg("status")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2)); // CHANGES_DETECTED
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

// ---------------------------------------------------------------------------
// completions: PowerShell produces output
// ---------------------------------------------------------------------------

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
        .failure();
}

// ---------------------------------------------------------------------------
// doc delete --check exits 2 when changes would be made
// ---------------------------------------------------------------------------

#[test]
fn test_doc_delete_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"key":"value","other":"keep"}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("delete")
        .arg(&file)
        .arg("key")
        .arg("--check")
        .assert()
        .code(2);

    // File should be unchanged in --check mode.
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("key"),
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// doc merge --check exits 2 when changes would be made
// ---------------------------------------------------------------------------

#[test]
fn test_doc_merge_check_exits_2() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("data.json");
    fs::write(&file, r#"{"a":1}"#).unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("doc")
        .arg("merge")
        .arg(&file)
        .arg("--value")
        .arg(r#"{"b":2}"#)
        .arg("--check")
        .assert()
        .code(2);

    let content = fs::read_to_string(&file).unwrap();
    assert!(
        !content.contains("b"),
        "file should be unchanged in --check mode"
    );
}

// ---------------------------------------------------------------------------
// --json error envelope (#227)
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
fn has_mcp_support() -> bool {
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("mcp-server")
        .arg("--help")
        .ok()
        .is_ok()
}

/// Spawn `patchloom mcp-server` in a tempdir and return a connected MCP client.
async fn spawn_mcp_client(cwd: &Path) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    spawn_mcp_client_opts(cwd, false).await
}

#[cfg(feature = "mcp")]
async fn spawn_mcp_client_with_shell(
    cwd: &Path,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    spawn_mcp_client_opts(cwd, true).await
}

async fn spawn_mcp_client_opts(
    cwd: &Path,
    allow_shell: bool,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    use rmcp::ServiceExt;
    use rmcp::transport::TokioChildProcess;

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("mcp-server").current_dir(cwd);
    if allow_shell {
        cmd.arg("--allow-shell");
    }

    let transport = TokioChildProcess::new(cmd).expect("failed to spawn patchloom mcp-server");
    ().serve(transport)
        .await
        .expect("failed to connect MCP client")
}

#[cfg(feature = "mcp")]
async fn spawn_mcp_client_process_and_cwd(
    process_dir: &Path,
    server_cwd: &Path,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    use rmcp::ServiceExt;
    use rmcp::transport::TokioChildProcess;

    let bin = assert_cmd::cargo::cargo_bin("patchloom");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("mcp-server")
        .arg("--cwd")
        .arg(server_cwd)
        .current_dir(process_dir);

    let transport = TokioChildProcess::new(cmd).expect("failed to spawn patchloom mcp-server");
    ().serve(transport)
        .await
        .expect("failed to connect MCP client")
}

/// Helper: call a tool and return the text content from the first Content item.
async fn call_tool_text(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: impl Into<String>,
    args: serde_json::Value,
) -> (bool, String) {
    let params = rmcp::model::CallToolRequestParams::new(tool.into())
        .with_arguments(serde_json::from_value(args).unwrap());
    let result = client.peer().call_tool(params).await.unwrap();
    let is_error = result.is_error.unwrap_or(false);
    let text = result
        .content
        .first()
        .and_then(|c| match &c.raw {
            rmcp::model::RawContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (is_error, text)
}

#[tokio::test]
async fn test_mcp_doc_set_round_trip() {
    if !has_mcp_support() {
        return; // binary built without --features mcp
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"old","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _text) = call_tool_text(
        &client,
        "doc_set",
        serde_json::json!({"path": "config.json", "selector": "name", "value": "new"}),
    )
    .await;
    assert!(!is_error, "doc_set should succeed");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["name"], "new", "doc_set did not update file: {content}");
    assert_eq!(
        v["version"], "1.0",
        "doc_set clobbered other keys: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_replace_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _text) = call_tool_text(
        &client,
        "replace_text",
        serde_json::json!({"path": "hello.txt", "from": "world", "to": "patchloom"}),
    )
    .await;
    assert!(!is_error, "replace should succeed");

    let content = fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert_eq!(content, "hello patchloom\n");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_read_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.txt"), "line1\nline2\nline3\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "read_file",
        serde_json::json!({"path": "data.txt"}),
    )
    .await;
    assert!(!is_error, "read should succeed");
    assert!(
        text.contains("line1"),
        "read should return file content: {text}"
    );
    assert!(
        text.contains("line3"),
        "read should return all lines: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.json"), r#"{"version":"1.0.0"}"#).unwrap();
    fs::write(dir.path().join("b.txt"), "old text\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _text) = call_tool_text(
        &client,
        "batch",
        serde_json::json!({
            "operations": ["doc.set a.json version \"2.0.0\"", "replace b.txt \"old\" \"new\""]
        }),
    )
    .await;
    assert!(!is_error, "batch should succeed");

    let a: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("a.json")).unwrap()).unwrap();
    assert_eq!(a["version"], "2.0.0", "batch doc.set failed");
    let b = fs::read_to_string(dir.path().join("b.txt")).unwrap();
    assert_eq!(b, "new text\n", "batch replace failed");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_set_nonexistent_file_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _text) = call_tool_text(
        &client,
        "doc_set",
        serde_json::json!({"path": "nope.json", "selector": "x", "value": 1}),
    )
    .await;
    assert!(is_error, "doc_set on nonexistent file should return error");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_finds_pattern() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("haystack.txt"),
        "first line\nsecond needle line\nthird line\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "search_files",
        serde_json::json!({"pattern": "needle", "paths": ["haystack.txt"]}),
    )
    .await;
    assert!(!is_error, "search should succeed: {text}");
    assert!(
        text.contains("needle"),
        "search output should contain the match: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_has_existing_key() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("data.json"), r#"{"name":"alice","age":30}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_has",
        serde_json::json!({"path": "data.json", "selector": "name"}),
    )
    .await;
    assert!(!is_error, "doc_has should succeed: {text}");
    assert!(
        text.contains("true"),
        "doc_has should return true for existing key: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_get_reads_value() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"version":"2.1.0","debug":false}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_get",
        serde_json::json!({"path": "config.json", "selector": "version"}),
    )
    .await;
    assert!(!is_error, "doc_get should succeed: {text}");
    assert!(
        text.contains("2.1.0"),
        "doc_get should return the value: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_rejects_absolute_path() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(
                serde_json::json!({"pattern": "secret", "paths": ["/etc/passwd"]}),
            )
            .unwrap(),
        );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "search with absolute path should be rejected as a path containment violation"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_search_rejects_conflicting_modes() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("search_files".to_string())
        .with_arguments(
            serde_json::from_value(serde_json::json!({
                "pattern": "hello",
                "files_with_matches": true,
                "count": true
            }))
            .unwrap(),
        );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "search with both files_with_matches and count should be rejected"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_batch_rejects_oversized_payload() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.json"), r#"{"k":"v"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Build a payload exceeding MAX_BATCH_OPERATIONS (10_000).
    let ops: Vec<String> = (0..10_001)
        .map(|i| format!("doc.set f.json key{i} \"v\""))
        .collect();
    let (is_error, text) =
        call_tool_text(&client, "batch", serde_json::json!({ "operations": ops })).await;
    assert!(is_error, "oversized batch should be rejected: {text}");
    assert!(
        text.contains("Too many operations"),
        "error should mention operation count: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_file_rename_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old_name.txt"), "content\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "move_file",
        serde_json::json!({"from": "old_name.txt", "to": "new_name.txt"}),
    )
    .await;
    assert!(!is_error, "rename should succeed: {text}");
    assert!(
        !dir.path().join("old_name.txt").exists(),
        "old file should not exist"
    );
    assert!(
        dir.path().join("new_name.txt").exists(),
        "new file should exist"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new_name.txt")).unwrap(),
        "content\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_create_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "create_file",
        serde_json::json!({"path": "new_file.txt", "content": "hello world\n"}),
    )
    .await;
    assert!(!is_error, "create should succeed: {text}");
    assert!(
        dir.path().join("new_file.txt").exists(),
        "file should exist after create"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("new_file.txt")).unwrap(),
        "hello world\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_create_existing_fails_without_force() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("existing.txt"), "original\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, _text) = call_tool_text(
        &client,
        "create_file",
        serde_json::json!({"path": "existing.txt", "content": "new content\n"}),
    )
    .await;
    assert!(
        is_error,
        "create should fail for existing file without force"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("existing.txt")).unwrap(),
        "original\n",
        "original content should be preserved"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_delete_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doomed.txt"), "bye\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "delete_file",
        serde_json::json!({"path": "doomed.txt"}),
    )
    .await;
    assert!(!is_error, "delete should succeed: {text}");
    assert!(
        !dir.path().join("doomed.txt").exists(),
        "file should not exist after delete"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_patch_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old line\n").unwrap();

    let diff = "--- a/target.txt\n+++ b/target.txt\n@@ -1 +1 @@\n-old line\n+new line\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) =
        call_tool_text(&client, "apply_patch", serde_json::json!({"diff": diff})).await;
    assert!(!is_error, "patch should succeed: {text}");
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "new line\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_md_insert_after_heading_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doc.md"), "# Title\n\nExisting body.\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_insert_after_heading",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Title",
            "content": "Inserted line.\n"
        }),
    )
    .await;
    assert!(!is_error, "md insert_after_heading should succeed: {text}");
    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("Inserted line."),
        "inserted content should be present: {content}"
    );
    // Existing body should still be there.
    assert!(
        content.contains("Existing body."),
        "existing body should be preserved: {content}"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_md_insert_before_heading_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# First\n\nBody one.\n\n## Second\n\nBody two.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_insert_before_heading",
        serde_json::json!({
            "path": "doc.md",
            "heading": "## Second",
            "content": "Preface text.\n"
        }),
    )
    .await;
    assert!(!is_error, "md insert_before_heading should succeed: {text}");
    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("Preface text."),
        "inserted content should be present: {content}"
    );
    // The preface should appear before ## Second.
    let preface_pos = content.find("Preface text.").unwrap();
    let heading_pos = content.find("## Second").unwrap();
    assert!(
        preface_pos < heading_pos,
        "preface should appear before ## Second"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
    fs::write(dir.path().join("b.txt"), "foo bar\n").unwrap();

    let plan = serde_json::json!({
        "version": "1",
        "operations": [
            {"op": "replace", "path": "a.txt", "from": "hello", "to": "goodbye"},
            {"op": "replace", "path": "b.txt", "from": "foo", "to": "baz"}
        ]
    });

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "transaction",
        serde_json::json!({"plan": plan.to_string()}),
    )
    .await;
    assert!(!is_error, "tx should succeed: {text}");
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "goodbye world\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "baz bar\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_rejects_escaping_cwd() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let plan = serde_json::json!({
        "version": "1",
        "cwd": "/tmp",
        "operations": [
            {"op": "replace", "path": "a.txt", "from": "a", "to": "b"}
        ]
    });

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("transaction".to_string()).with_arguments(
        serde_json::from_value(serde_json::json!({"plan": plan.to_string()})).unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "tx with escaping cwd should be rejected as a path containment violation"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_rejects_relative_cwd_that_escapes_server_root() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let process_dir = dir.path().join("sandbox");
    let repo_dir = dir.path().join("repo");
    fs::create_dir_all(&process_dir).unwrap();
    fs::create_dir_all(&repo_dir).unwrap();
    fs::write(repo_dir.join("inside.txt"), "inside\n").unwrap();
    fs::write(dir.path().join("outside.txt"), "outside\n").unwrap();

    let plan = serde_json::json!({
        "version": "1",
        "cwd": "..",
        "operations": [
            {"op": "replace", "path": "outside.txt", "from": "outside", "to": "pwned"}
        ]
    });

    let client = spawn_mcp_client_process_and_cwd(&process_dir, &repo_dir).await;
    let params = rmcp::model::CallToolRequestParams::new("transaction".to_string()).with_arguments(
        serde_json::from_value(serde_json::json!({"plan": plan.to_string()})).unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "tx with relative escaping cwd should be rejected"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("outside.txt")).unwrap(),
        "outside\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_rejects_relative_cwd_that_is_a_file() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let repo_dir = dir.path().join("repo");
    fs::create_dir_all(&repo_dir).unwrap();
    fs::write(repo_dir.join("not-a-dir"), "nope\n").unwrap();

    let plan = serde_json::json!({
        "version": "1",
        "cwd": "not-a-dir",
        "operations": [
            {"op": "replace", "path": "anything.txt", "from": "a", "to": "b"}
        ]
    });

    let client = spawn_mcp_client(&repo_dir).await;
    let params = rmcp::model::CallToolRequestParams::new("transaction".to_string()).with_arguments(
        serde_json::from_value(serde_json::json!({"plan": plan.to_string()})).unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "tx with a file-valued relative cwd should be rejected"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_yaml_format() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old\n").unwrap();

    let yaml_plan = "version: \"1\"\noperations:\n  - op: replace\n    path: target.txt\n    from: old\n    to: new\n";

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "transaction",
        serde_json::json!({"plan": yaml_plan, "format": "yaml"}),
    )
    .await;
    assert!(!is_error, "tx with yaml format should succeed: {text}");
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "new\n"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_invalid_plan_returns_error() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let params = rmcp::model::CallToolRequestParams::new("transaction".to_string()).with_arguments(
        serde_json::from_value(serde_json::json!({"plan": "this is not valid json or yaml"}))
            .unwrap(),
    );
    let result = client.peer().call_tool(params).await;
    assert!(
        result.is_err(),
        "tx with invalid plan should be rejected as a parse error"
    );
    client.cancel().await.unwrap();
}

#[cfg(feature = "mcp")]
#[tokio::test]
async fn test_mcp_tx_with_validate_lifecycle() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old\n").unwrap();

    // Plan with operations + a validate step that checks the file content.
    let plan = serde_json::json!({
        "version": "1",
        "operations": [
            {"op": "replace", "path": "target.txt", "from": "old", "to": "new"}
        ],
        "validate": [
            {"cmd": "grep -q new target.txt", "required": true}
        ]
    });

    // --allow-shell is required for plans with validate steps.
    let client = spawn_mcp_client_with_shell(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "transaction",
        serde_json::json!({"plan": plan.to_string()}),
    )
    .await;
    assert!(!is_error, "tx with passing validate should succeed: {text}");
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "new\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_tx_rejects_shell_steps_without_flag() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "old\n").unwrap();

    let plan = serde_json::json!({
        "version": "1",
        "operations": [
            {"replace": {"path": "target.txt", "from": "old", "to": "new"}}
        ],
        "validate": [
            {"cmd": "grep -q new target.txt", "required": true}
        ]
    });

    // Default client (no --allow-shell) should reject the plan.
    let client = spawn_mcp_client(dir.path()).await;
    let result = client
        .peer()
        .call_tool(
            rmcp::model::CallToolRequestParams::new("transaction").with_arguments(
                serde_json::from_value(serde_json::json!({"plan": plan.to_string()})).unwrap(),
            ),
        )
        .await;
    assert!(
        result.is_err(),
        "tx with shell steps should be rejected without --allow-shell"
    );
    // File must remain unchanged.
    assert_eq!(
        fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        "old\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_tidy_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // File missing final newline and with trailing whitespace.
    fs::write(dir.path().join("messy.txt"), "hello   \nworld").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "fix_whitespace",
        serde_json::json!({"path": "messy.txt"}),
    )
    .await;
    assert!(!is_error, "tidy should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("messy.txt")).unwrap();
    assert!(content.ends_with('\n'), "tidy should ensure final newline");
    assert!(
        !content.contains("   \n"),
        "tidy should trim trailing whitespace"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_upsert_bullet_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("doc.md"), "# Rules\n\n- Existing rule\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_upsert_bullet",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Rules",
            "bullet": "- New rule"
        }),
    )
    .await;
    assert!(!is_error, "md_upsert_bullet should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("- New rule"),
        "md_upsert_bullet should add the bullet: {content}"
    );
    assert!(
        content.contains("- Existing rule"),
        "md_upsert_bullet should preserve existing bullets: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_merge_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_merge",
        serde_json::json!({"path": "config.json", "value": {"debug": true}}),
    )
    .await;
    assert!(!is_error, "doc_merge should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["debug"], true, "doc_merge should add new key: {content}");
    assert_eq!(
        v["name"], "app",
        "doc_merge should preserve existing keys: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_delete_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","debug":true}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_delete",
        serde_json::json!({"path": "config.json", "selector": "debug"}),
    )
    .await;
    assert!(!is_error, "doc_delete should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("debug").is_none(), "doc_delete should remove key");
    assert_eq!(v["name"], "app", "doc_delete should preserve other keys");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_append_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_append",
        serde_json::json!({"path": "config.json", "selector": "tags", "value": "c"}),
    )
    .await;
    assert!(!is_error, "doc_append should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
    assert_eq!(v["tags"][2], "c");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_prepend_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_prepend",
        serde_json::json!({"path": "config.json", "selector": "tags", "value": "z"}),
    )
    .await;
    assert!(!is_error, "doc_prepend should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["tags"][0], "z", "doc_prepend should insert at front");
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_ensure_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"name":"app"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    // Ensure a missing key.
    let (is_error, text) = call_tool_text(
        &client,
        "doc_ensure",
        serde_json::json!({"path": "config.json", "selector": "debug", "value": false}),
    )
    .await;
    assert!(!is_error, "doc_ensure should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["debug"], false, "doc_ensure should set missing key");
    assert_eq!(v["name"], "app", "doc_ensure should preserve existing keys");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_update_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"items":[{"active":false},{"active":false}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_update",
        serde_json::json!({"path": "config.json", "selector": "items[*]", "value": {"active": true}}),
    )
    .await;
    assert!(!is_error, "doc_update should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(v["items"][0]["active"], true);
    assert_eq!(v["items"][1]["active"], true);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_move_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"old_name":"value"}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_move",
        serde_json::json!({"path": "config.json", "from": "old_name", "to": "new_name"}),
    )
    .await;
    assert!(!is_error, "doc_move should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(v.get("old_name").is_none(), "doc_move should remove source");
    assert_eq!(v["new_name"], "value", "doc_move should set destination");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_delete_where_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"items":[{"name":"keep"},{"name":"drop"},{"name":"keep2"}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_delete_where",
        serde_json::json!({"path": "config.json", "selector": "items", "predicate": "name=drop"}),
    )
    .await;
    assert!(!is_error, "doc_delete_where should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap();
    let items = v["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        2,
        "doc_delete_where should remove matching item"
    );
    assert_eq!(items[0]["name"], "keep");
    assert_eq!(items[1]["name"], "keep2");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_keys_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"name":"app","version":"1.0","debug":true}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_keys",
        serde_json::json!({"path": "config.json", "selector": "."}),
    )
    .await;
    assert!(!is_error, "doc_keys should succeed: {text}");
    assert!(
        text.contains("name"),
        "doc_keys output should contain 'name': {text}"
    );
    assert!(
        text.contains("version"),
        "doc_keys output should contain 'version': {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_len_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.json"), r#"{"tags":["a","b","c"]}"#).unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_len",
        serde_json::json!({"path": "config.json", "selector": "tags"}),
    )
    .await;
    assert!(!is_error, "doc_len should succeed: {text}");
    assert!(text.contains('3'), "doc_len should return 3: {text}");
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_select_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"users":[{"role":"admin","name":"alice"},{"role":"user","name":"bob"}]}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_select",
        serde_json::json!({"path": "config.json", "selector": "users[role=admin]"}),
    )
    .await;
    assert!(!is_error, "doc_select should succeed: {text}");
    assert!(
        text.contains("alice"),
        "doc_select should return matching item: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_flatten_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"db":{"host":"localhost","port":5432}}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_flatten",
        serde_json::json!({"path": "config.json"}),
    )
    .await;
    assert!(!is_error, "doc_flatten should succeed: {text}");
    assert!(
        text.contains("db.host"),
        "doc_flatten should contain 'db.host': {text}"
    );
    assert!(
        text.contains("db.port"),
        "doc_flatten should contain 'db.port': {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_doc_diff_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.json"),
        r#"{"name":"old","version":"1.0"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("b.json"),
        r#"{"name":"new","version":"1.0"}"#,
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "doc_diff",
        serde_json::json!({"file_a": "a.json", "file_b": "b.json"}),
    )
    .await;
    assert!(!is_error, "doc_diff should succeed: {text}");
    assert!(
        text.contains("name"),
        "doc_diff should report name difference: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_status_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Initialize a git repo so status has something to report.
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
    fs::write(dir.path().join("tracked.txt"), "hello\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Modify the tracked file to create a diff.
    fs::write(dir.path().join("tracked.txt"), "modified\n").unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(&client, "git_status", serde_json::json!({})).await;
    assert!(!is_error, "status should succeed: {text}");
    assert!(
        text.contains("tracked.txt"),
        "status should show modified file: {text}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_table_append_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Changelog\n\n| Version | Date |\n|---------|------|\n| 0.1.0 | 2024-01-01 |\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_table_append",
        serde_json::json!({
            "path": "doc.md",
            "heading": "# Changelog",
            "row": "| 0.2.0 | 2024-06-15 |"
        }),
    )
    .await;
    assert!(!is_error, "md_table_append should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("| 0.2.0 | 2024-06-15 |"),
        "md_table_append should add the row: {content}"
    );
    assert!(
        content.contains("| 0.1.0 | 2024-01-01 |"),
        "md_table_append should preserve existing rows: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_replace_section_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Title\n\nIntro text.\n\n## API\n\nOld API docs.\n\n## Usage\n\nUsage text.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_replace_section",
        serde_json::json!({
            "path": "doc.md",
            "heading": "## API",
            "content": "New API documentation.\n"
        }),
    )
    .await;
    assert!(!is_error, "md_replace_section should succeed: {text}");

    let content = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    assert!(
        content.contains("New API documentation."),
        "md_replace_section should insert new content: {content}"
    );
    assert!(
        !content.contains("Old API docs."),
        "md_replace_section should remove old content: {content}"
    );
    assert!(
        content.contains("## Usage"),
        "md_replace_section should preserve other sections: {content}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_mcp_md_lint_round_trip() {
    if !has_mcp_support() {
        return;
    }
    let dir = TempDir::new().unwrap();
    // Create a file with a duplicate heading (a lint issue).
    fs::write(
        dir.path().join("AGENTS.md"),
        "# Rules\n\nFirst section.\n\n# Rules\n\nDuplicate heading.\n",
    )
    .unwrap();

    let client = spawn_mcp_client(dir.path()).await;
    let (is_error, text) = call_tool_text(
        &client,
        "md_lint",
        serde_json::json!({
            "path": "AGENTS.md"
        }),
    )
    .await;
    assert!(!is_error, "md_lint should succeed: {text}");

    let issues: serde_json::Value = serde_json::from_str(&text).unwrap();
    let arr = issues
        .as_array()
        .expect("md_lint should return a JSON array");
    assert!(
        !arr.is_empty(),
        "md_lint should find issues in file with duplicate heading"
    );

    // Also test a clean file returns an empty array.
    fs::write(
        dir.path().join("clean.md"),
        "# Single Heading\n\nContent.\n",
    )
    .unwrap();
    let (is_error2, text2) = call_tool_text(
        &client,
        "md_lint",
        serde_json::json!({
            "path": "clean.md"
        }),
    )
    .await;
    assert!(!is_error2, "md_lint should succeed on clean file: {text2}");
    let clean: serde_json::Value = serde_json::from_str(&text2).unwrap();
    assert_eq!(
        clean.as_array().unwrap().len(),
        0,
        "md_lint should return empty array for clean file"
    );
    client.cancel().await.unwrap();
}

// ---------------------------------------------------------------------------
// Backup pruning integration tests (#371)
// ---------------------------------------------------------------------------

#[test]
fn test_replace_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello world\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("hello")
        .arg("--to")
        .arg("goodbye")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    // A backup session directory should exist under .patchloom/backups/.
    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(backup_dir.exists(), "backup directory should be created");

    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(sessions.len(), 1, "exactly one backup session expected");

    // The session should contain a manifest.json.
    let manifest = sessions[0].path().join("manifest.json");
    assert!(manifest.exists(), "manifest.json should exist in session");
}

#[test]
fn test_replace_apply_prunes_old_backup_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "aaa\n").unwrap();

    // Create a fake old backup session (8 days old).
    let old_session = dir.path().join(".patchloom/backups/old_session");
    fs::create_dir_all(&old_session).unwrap();
    fs::write(
        old_session.join("manifest.json"),
        r#"{"timestamp":"old_session","entries":[]}"#,
    )
    .unwrap();

    // Backdate the directory mtime to 8 days ago.
    let eight_days_ago =
        std::time::SystemTime::now() - std::time::Duration::from_secs(8 * 24 * 60 * 60);
    let times = std::fs::FileTimes::new().set_modified(eight_days_ago);
    let f = std::fs::File::open(&old_session).unwrap();
    f.set_times(times).unwrap();

    // Now run a replace --apply, which creates a new session and prunes old ones.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("aaa")
        .arg("--to")
        .arg("bbb")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let backup_dir = dir.path().join(".patchloom/backups");
    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    // The old session should have been pruned; only the new one remains.
    assert_eq!(
        sessions.len(),
        1,
        "old session should be pruned, only the new one remains"
    );
    assert!(
        !old_session.exists(),
        "the old_session directory should be removed"
    );
}

#[test]
fn test_replace_apply_keeps_recent_backup_sessions() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "first\n").unwrap();

    // First apply creates a backup.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("first")
        .arg("--to")
        .arg("second")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    // Small delay to ensure different timestamps.
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Second apply creates another backup. Both are recent.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("replace")
        .arg("second")
        .arg("--to")
        .arg("third")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .arg(file.to_str().unwrap())
        .assert()
        .success();

    let backup_dir = dir.path().join(".patchloom/backups");
    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    // Both sessions are recent; neither should be pruned.
    assert_eq!(
        sessions.len(),
        2,
        "both recent backup sessions should be kept"
    );
}

#[test]
fn test_delete_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, "original content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg(file.to_str().unwrap())
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!file.exists(), "file should be deleted");

    // A backup session should exist with the original content.
    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(
        backup_dir.exists(),
        "backup dir should exist after delete --apply"
    );

    let sessions: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    assert_eq!(sessions.len(), 1, "one backup session expected");
}

#[test]
fn test_delete_apply_undo_restores_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("precious.txt");
    fs::write(&file, "precious data\n").unwrap();

    // Delete the file.
    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("delete")
        .arg("precious.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!file.exists());

    // Undo should restore it.
    Command::cargo_bin("patchloom")
        .unwrap()
        .args(["undo", "--apply", "--cwd"])
        .arg(dir.path())
        .assert()
        .success();

    assert!(file.exists(), "undo should restore the deleted file");
    assert_eq!(fs::read_to_string(&file).unwrap(), "precious data\n");
}

#[test]
fn test_rename_apply_creates_backup_session() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("old.txt");
    fs::write(&src, "content\n").unwrap();

    Command::cargo_bin("patchloom")
        .unwrap()
        .arg("rename")
        .arg("old.txt")
        .arg("new.txt")
        .arg("--apply")
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .success();

    assert!(!src.exists(), "source should be gone");
    assert!(dir.path().join("new.txt").exists(), "dest should exist");

    let backup_dir = dir.path().join(".patchloom/backups");
    assert!(
        backup_dir.exists(),
        "backup dir should exist after rename --apply"
    );
}

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
