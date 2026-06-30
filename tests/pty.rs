//! PTY-based integration tests for interactive terminal behavior.
//!
//! These tests spawn the patchloom binary inside a pseudo-terminal so that
//! `is_terminal()` returns `true`, enabling the `--confirm` code path.
//! This lets us test the full interactive confirm + format pipeline that
//! is unreachable from normal `assert_cmd` tests (which run without a TTY).
//!
//! Run with: `cargo test --test pty --all-features -- --test-threads=1`
//!
//! These tests MUST run serially (`--test-threads=1`) because concurrent
//! PTY allocation can cause timeouts. The Makefile target handles this.

use expectrl::session::OsSession;
use expectrl::{Eof, Expect};
use std::fs;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

/// Build a `Command` for the patchloom binary with `--cwd` set.
fn patchloom_cmd(cwd: &std::path::Path) -> Command {
    let bin = env!("CARGO_BIN_EXE_patchloom");
    let mut cmd = Command::new(bin);
    cmd.arg("--cwd").arg(cwd);
    // Disable color to make output matching reliable.
    cmd.arg("--color=never");
    cmd
}

/// Spawn a PTY session with a generous timeout for CI reliability.
///
/// The 30-second timeout is deliberately long: under normal conditions each
/// `expect()` call returns in milliseconds, so the timeout adds zero latency.
/// But when PTY tests run right after 800+ integration tests in `make check-fast`,
/// system load can delay process startup enough to hit a 10-second ceiling.
fn spawn_pty(cmd: Command) -> OsSession {
    let mut session = OsSession::spawn(cmd).expect("failed to spawn PTY session");
    session.set_expect_timeout(Some(Duration::from_secs(30)));
    session
}

/// Create a cross-platform `touch <path>` command string.
fn shell_touch(path: &std::path::Path) -> String {
    let path = path.display();
    format!("touch '{path}'")
}

/// Cross-platform command that always fails (non-zero exit).
fn shell_false() -> &'static str {
    #[cfg(windows)]
    {
        "cmd /C exit 1"
    }
    #[cfg(not(windows))]
    {
        "false"
    }
}

// ── --confirm path tests ───────────────────────────────────────

#[test]
fn pty_replace_confirm_yes_applies() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["replace", "aaa", "--new", "bbb", "--confirm"]);

    let mut session = spawn_pty(cmd);

    // Expect the diff output (contains the replacement preview).
    session.expect("aaa").expect("should see old text in diff");

    // Expect the confirm prompt.
    session.expect("Apply?").expect("should see Apply? prompt");

    // Answer yes.
    session.send_line("y").expect("failed to send y");

    // Wait for process to exit.
    session.expect(Eof).expect("process should exit");

    // Verify the file was actually modified.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "bbb\n", "file should be modified after confirm y");
}

#[test]
fn pty_replace_confirm_no_does_not_apply() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["replace", "aaa", "--new", "bbb", "--confirm"]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("n").expect("failed to send n");
    session.expect(Eof).expect("process should exit");

    // File must be unchanged.
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "aaa\n",
        "file should NOT be modified after confirm n"
    );
}

// ── --confirm + --format tests ─────────────────────────────────

#[test]
fn pty_replace_confirm_yes_runs_format_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args([
        "replace",
        "aaa",
        "--new",
        "bbb",
        "--confirm",
        "--format",
        &shell_touch(&marker),
    ]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    assert!(
        marker.exists(),
        "--format command should have run after --confirm y"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "bbb\n");
}

#[test]
fn pty_replace_confirm_no_skips_format_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args([
        "replace",
        "aaa",
        "--new",
        "bbb",
        "--confirm",
        "--format",
        &shell_touch(&marker),
    ]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("n").expect("failed to send n");
    session.expect(Eof).expect("process should exit");

    assert!(
        !marker.exists(),
        "--format should NOT run when user declines"
    );
}

#[test]
fn pty_append_confirm_yes_runs_format_command() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "line\n").unwrap();
    let marker = dir.path().join("format_ran.marker");

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args([
        "append",
        "f.txt",
        "--content",
        "extra\n",
        "--confirm",
        "--format",
        &shell_touch(&marker),
    ]);

    let mut session = spawn_pty(cmd);

    // Consume diff output (header is reliably emitted early) before matching the prompt.
    // "extra" (added content) may race under high load; filename in diff header is stable.
    session
        .expect("f.txt")
        .expect("should see diff output (file header)");
    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    assert!(
        marker.exists(),
        "--format command should have run after append --confirm y"
    );
    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("extra"), "content should be appended");
}

#[test]
fn pty_create_confirm_yes_runs_format_command() {
    let dir = TempDir::new().unwrap();
    let marker = dir.path().join("format_ran.marker");

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args([
        "create",
        "new.txt",
        "--content",
        "hello\n",
        "--confirm",
        "--format",
        &shell_touch(&marker),
    ]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    assert!(
        marker.exists(),
        "--format command should have run after create --confirm y"
    );
    assert!(
        dir.path().join("new.txt").exists(),
        "file should be created"
    );
}

#[test]
fn pty_confirm_default_enter_accepts() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["replace", "aaa", "--new", "bbb", "--confirm"]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    // Press Enter with no input (default is Y).
    session.send_line("").expect("failed to send enter");
    session.expect(Eof).expect("process should exit");

    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(
        content, "bbb\n",
        "pressing Enter should accept the default (Y)"
    );
}

// ── tx + plan + --confirm lifecycle (format/validate steps) ───────

#[test]
fn pty_tx_confirm_yes_runs_plan_format_and_validate_steps() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "old\n").unwrap();
    let format_marker = dir.path().join("plan_format_ran.marker");
    let validate_marker = dir.path().join("plan_validate_ran.marker");

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "replace", "path": "f.txt", "old": "old", "new": "new"}
        ],
        "format": [
            {"cmd": shell_touch(&format_marker)}
        ],
        "validate": [
            {"cmd": shell_touch(&validate_marker), "required": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["tx", plan_file.to_str().unwrap(), "--confirm"]);

    let mut session = spawn_pty(cmd);

    // The tx will print a diff before the prompt.
    session
        .expect("new")
        .expect("should see replacement in diff");
    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    // Both plan format and validate steps must have executed.
    assert!(
        format_marker.exists(),
        "plan 'format' step should have run after tx --confirm y"
    );
    assert!(
        validate_marker.exists(),
        "plan 'validate' step should have run after tx --confirm y"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "new\n",
        "replace from plan should have been applied"
    );
}

#[test]
fn pty_tx_confirm_nonstrict_validate_failure_keeps_applied_changes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "old\n").unwrap();
    let format_marker = dir.path().join("plan_format_ran.marker");

    let plan = serde_json::json!({
        "version": 1,
        "operations": [
            {"op": "replace", "path": "f.txt", "old": "old", "new": "new"}
        ],
        "format": [
            {"cmd": shell_touch(&format_marker)}
        ],
        "validate": [
            {"cmd": shell_false(), "required": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args([
        "tx",
        plan_file.to_str().unwrap(),
        "--confirm",
        "--no-strict",
    ]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    // Format step ran (before validate), replace applied, but validate failed (non-strict).
    // Per design: changes are kept, exit code is VALIDATION_FAILED (not rolled back).
    assert!(
        format_marker.exists(),
        "plan format step should run even if later validate fails"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "new\n",
        "changes should remain after non-strict validate failure on --confirm"
    );
}

#[test]
fn pty_tx_confirm_strict_validate_failure_rolls_back() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "old\n").unwrap();
    let format_marker = dir.path().join("plan_format_ran.marker");

    let plan = serde_json::json!({
        "version": 1,
        "strict": true,
        "operations": [
            {"op": "replace", "path": "f.txt", "old": "old", "new": "new"}
        ],
        "format": [
            {"cmd": shell_touch(&format_marker)}
        ],
        "validate": [
            {"cmd": shell_false(), "required": true}
        ]
    });
    let plan_file = dir.path().join("plan.json");
    fs::write(&plan_file, serde_json::to_string(&plan).unwrap()).unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["tx", plan_file.to_str().unwrap(), "--confirm"]);

    let mut session = spawn_pty(cmd);

    session.expect("Apply?").expect("should see Apply? prompt");
    session.send_line("y").expect("failed to send y");
    session.expect(Eof).expect("process should exit");

    // Strict mode: on validate failure, changes rolled back.
    // External format side-effect (marker) remains.
    assert!(
        format_marker.exists(),
        "format side effect remains even on strict rollback"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "old\n",
        "file should be rolled back on strict validate failure during --confirm"
    );
}
