//! Shell command execution with proper process tree management.
//!
//! Provides platform-correct process spawning and cleanup:
//!
//! - **Unix**: spawns in a dedicated process group via `process_group(0)`,
//!   kills with `killpg(PGID, SIGKILL)` to catch compound commands.
//! - **Windows**: uses `taskkill /F /T /PID` to kill the entire process tree.
//!
//! The [`kill_process_tree`] function is the key correctness primitive: bare
//! `child.kill()` only terminates the shell process, leaving grandchildren
//! (pipelines, `&&` chains) alive as orphans.

use std::path::Path;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Maximum bytes of stderr captured from a shell command.
pub const STDERR_CAPTURE_MAX: usize = 512;

/// Build a platform-appropriate shell command.
///
/// - **Unix**: `sh -c <cmd>` with `process_group(0)` for clean signal delivery.
/// - **Windows**: `cmd /C <cmd>` with `raw_arg` for correct escaping.
pub fn shell_command(cmd: &str, cwd: &Path) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("cmd");
        // Use raw_arg so cmd.exe receives the command string without Rust's
        // MSVC-style argument quoting. Rust's .arg() wraps in double quotes
        // and escapes inner quotes with backslash, which cmd.exe does not
        // understand. raw_arg passes the string verbatim, letting cmd's /C
        // handler parse redirects (>), pipes (|), and inner quotes correctly.
        command.arg("/C").raw_arg(cmd).current_dir(cwd);
        command
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd).current_dir(cwd);
        // Spawn the child in its own process group so that
        // kill_process_tree can kill the entire group.
        command.process_group(0);
        command
    }
}

/// Result of a shell command execution with captured stderr.
#[derive(Debug)]
pub struct ShellResult {
    /// Exit status of the process.
    pub status: std::process::ExitStatus,
    /// First [`STDERR_CAPTURE_MAX`] bytes of stderr output (truncated for safety).
    pub stderr_head: String,
}

/// Run a shell command with a timeout (seconds).
///
/// Captures up to [`STDERR_CAPTURE_MAX`] bytes of stderr. Stdout is
/// discarded. Kills the process tree on timeout.
pub fn run_with_timeout(cmd: &str, timeout_secs: u64, cwd: &Path) -> anyhow::Result<ShellResult> {
    let mut child = shell_command(cmd, cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Read stderr in a background thread to avoid blocking if the pipe fills.
    let stderr_handle = child.stderr.take().expect("stderr piped");
    let reader_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = vec![0u8; STDERR_CAPTURE_MAX + 1];
        let mut reader = stderr_handle;
        let mut total = 0;
        loop {
            match reader.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if total > STDERR_CAPTURE_MAX {
                        // Drain remaining stderr so the child is not blocked.
                        let mut discard = [0u8; 4096];
                        while reader.read(&mut discard).unwrap_or(0) > 0 {}
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let cap = total.min(STDERR_CAPTURE_MAX);
        let text = String::from_utf8_lossy(&buf[..cap]).to_string();
        if total > STDERR_CAPTURE_MAX {
            format!("{text}... (truncated)")
        } else {
            text
        }
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(status) = child.try_wait()? {
            let stderr_head = reader_thread.join().unwrap_or_default();
            return Ok(ShellResult {
                status,
                stderr_head,
            });
        }
        if std::time::Instant::now() >= deadline {
            kill_process_tree(&mut child);
            let stderr_head = reader_thread.join().unwrap_or_default();
            anyhow::bail!("timed out after {timeout_secs}s: {stderr_head}");
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Kill a child process and all its descendants.
///
/// On Unix, the child is spawned in its own process group (via
/// `process_group(0)`), so we send SIGKILL to the entire group with
/// `libc::killpg`. This ensures compound commands (pipelines, `&&`
/// chains) have all descendants killed, not just the immediate `sh`
/// process.
///
/// On Windows, `child.kill()` calls `TerminateProcess` which only kills
/// the immediate `cmd.exe` process, leaving grandchildren running as
/// orphans. We use `taskkill /F /T /PID` to kill the entire process tree.
pub fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        // The child was spawned in its own process group, so its PID
        // equals its PGID. Kill the entire group.
        let pid = child.id() as i32;
        // Guard: if the child has already exited, id() returns 0.
        // killpg(0, SIGKILL) would kill our own process group.
        if pid > 0 {
            // SAFETY: killpg is a POSIX function that sends a signal to a
            // process group. We pass the child's PID (which is also its
            // PGID due to process_group(0)) and SIGKILL (9).
            #[expect(unsafe_code)]
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_command_sets_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmd = shell_command("echo hello", dir.path());
        assert_eq!(cmd.get_current_dir(), Some(dir.path()));
    }

    #[test]
    fn run_with_timeout_captures_exit_status() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = run_with_timeout("exit 0", 5, dir.path()).unwrap();
        assert!(result.status.success());
    }

    #[test]
    fn run_with_timeout_captures_nonzero_exit() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = run_with_timeout("exit 42", 5, dir.path()).unwrap();
        assert!(!result.status.success());
    }

    #[test]
    fn run_with_timeout_captures_stderr() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = run_with_timeout("echo oops >&2", 5, dir.path()).unwrap();
        assert!(result.stderr_head.contains("oops"));
    }

    #[test]
    fn run_with_timeout_kills_on_timeout() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = run_with_timeout("sleep 60", 1, dir.path());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("timed out"),
            "expected timeout error, got: {err}"
        );
    }

    #[test]
    fn kill_process_tree_handles_already_exited_child() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut child = shell_command("exit 0", dir.path()).spawn().unwrap();
        // Wait for the child to exit naturally.
        let _ = child.wait();
        // Calling kill_process_tree on an already-exited child should not panic.
        kill_process_tree(&mut child);
    }

    #[test]
    fn shell_result_is_send_and_sync() {
        const _: () = {
            fn _assert<T: Send + Sync>() {}
            let _ = _assert::<ShellResult>;
        };
    }

    #[test]
    fn run_with_timeout_truncates_large_stderr() {
        let dir = tempfile::TempDir::new().unwrap();
        // Generate stderr output much larger than STDERR_CAPTURE_MAX (512 bytes).
        // Use printf to emit 800 'X' characters to stderr.
        let result = run_with_timeout("printf '%0800d' 0 >&2", 5, dir.path()).unwrap();
        // The captured stderr should be at most STDERR_CAPTURE_MAX bytes
        // plus the "... (truncated)" suffix.
        assert!(
            result.stderr_head.len() <= STDERR_CAPTURE_MAX + 20,
            "stderr should be truncated, got {} bytes",
            result.stderr_head.len()
        );
        assert!(
            result.stderr_head.contains("(truncated)"),
            "expected truncation marker, got: {}",
            &result.stderr_head[..result.stderr_head.len().min(100)]
        );
    }
}
