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

use expectrl::{Eof, Session};
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
fn spawn_pty(cmd: Command) -> Session {
    let mut session = Session::spawn(cmd).expect("failed to spawn PTY session");
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    session
}

/// Create a cross-platform `touch <path>` command string.
fn shell_touch(path: &std::path::Path) -> String {
    let path = path.display();
    format!("touch '{path}'")
}

// ── --confirm path tests ───────────────────────────────────────

#[test]
fn pty_replace_confirm_yes_applies() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.txt");
    fs::write(&file, "aaa\n").unwrap();

    let mut cmd = patchloom_cmd(dir.path());
    cmd.args(["replace", "aaa", "--to", "bbb", "--confirm"]);

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
    cmd.args(["replace", "aaa", "--to", "bbb", "--confirm"]);

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
        "--to",
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
        "--to",
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

    // Consume diff output before matching the prompt.
    session.expect("extra").expect("should see diff output");
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
    cmd.args(["replace", "aaa", "--to", "bbb", "--confirm"]);

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
