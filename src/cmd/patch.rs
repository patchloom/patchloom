use crate::cli::global::GlobalFlags;
use crate::diff::{DiffResult, format_diff_result, format_diff_result_colored, unified_diff};
use crate::exit;
use crate::ops::patch::{apply_hunks, parse_patch};
use crate::write::{atomic_write, policy_from_flags};
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom patch changes.patch
  patchloom patch changes.patch --apply
  patchloom patch changes.patch --check")]
pub struct PatchArgs {
    #[command(subcommand)]
    pub action: PatchAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum PatchAction {
    /// Check whether a patch applies cleanly.
    Check {
        // ref:patch-mode:file
        /// Path to a diff file.
        file: Option<String>,
        // ref:patch-mode:stdin
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
    /// Apply a unified diff.
    Apply {
        // ref:patch-mode:file
        /// Path to a diff file.
        file: Option<String>,
        // ref:patch-mode:stdin
        /// Read diff from stdin.
        #[arg(long)]
        stdin: bool,
    },
}

// ── Read diff input ────────────────────────────────────────────────

/// Read diff text from the source indicated by the user.
/// Error from reading diff input.
enum DiffReadError {
    /// No input source specified.
    NoSource,
    /// IO error reading the specified file.
    IoError(String, std::io::Error),
    /// IO error reading stdin.
    StdinError(std::io::Error),
}

fn read_diff_input(file: &Option<String>, stdin_flag: bool) -> Result<String, DiffReadError> {
    if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|e| DiffReadError::IoError(path.clone(), e))
    } else if stdin_flag {
        std::io::read_to_string(std::io::stdin()).map_err(DiffReadError::StdinError)
    } else {
        Err(DiffReadError::NoSource)
    }
}

// ── JSON output types ───────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct PatchCheckResult {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct PatchCheckOutput {
    ok: bool,
    files: Vec<PatchCheckResult>,
}

fn emit_error(global: &GlobalFlags, error: &str) -> anyhow::Result<()> {
    if global.emit_json(&serde_json::json!({
        "ok": false,
        "error": error,
    }))? {
        return Ok(());
    }

    eprintln!("{error}");
    Ok(())
}

// ── Public entry point ──────────────────────────────────────────────

pub fn run(args: PatchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    let (file, stdin_flag) = match &args.action {
        PatchAction::Check { file, stdin } => (file.clone(), *stdin),
        PatchAction::Apply { file, stdin } => (file.clone(), *stdin),
    };

    // Read diff input.
    let diff_text = match read_diff_input(&file, stdin_flag) {
        Ok(text) => text,
        Err(DiffReadError::NoSource) => {
            emit_error(global, "patch: must specify --file <path> or --stdin")?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::IoError(path, e)) => {
            let msg = format!("patch: failed to read '{path}': {e}");
            emit_error(global, &msg)?;
            return Ok(exit::PARSE_ERROR);
        }
        Err(DiffReadError::StdinError(e)) => {
            let msg = format!("patch: failed to read stdin: {e}");
            emit_error(global, &msg)?;
            return Ok(exit::PARSE_ERROR);
        }
    };

    // Parse the unified diff.
    let patch_files = match parse_patch(&diff_text) {
        Ok(pf) => pf,
        Err(msg) => {
            let msg = format!("patch: parse error: {msg}");
            emit_error(global, &msg)?;
            return Ok(exit::PARSE_ERROR);
        }
    };

    let root = global.resolve_cwd()?;

    match args.action {
        PatchAction::Check { .. } => {
            let mut all_clean = true;
            let mut results: Vec<PatchCheckResult> = Vec::new();
            for pf in &patch_files {
                let file_path = root.join(&pf.path);
                let original = match std::fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let msg = format!("file not found: {}", file_path.display());
                        results.push(PatchCheckResult {
                            path: pf.path.clone(),
                            status: "missing",
                            error: Some(msg.clone()),
                        });
                        if !global.json && !global.jsonl && !global.quiet {
                            eprintln!("patch check: {} -- MISSING: {}", pf.path, msg);
                        }
                        all_clean = false;
                        continue;
                    }
                    Err(e) => {
                        let msg = format!("failed to read {}: {}", file_path.display(), e);
                        results.push(PatchCheckResult {
                            path: pf.path.clone(),
                            status: "error",
                            error: Some(msg.clone()),
                        });
                        if !global.json && !global.jsonl && !global.quiet {
                            eprintln!("patch check: {} -- READ ERROR: {}", pf.path, msg);
                        }
                        all_clean = false;
                        continue;
                    }
                };
                match apply_hunks(&original, &pf.hunks) {
                    Ok(_) => {
                        results.push(PatchCheckResult {
                            path: pf.path.clone(),
                            status: "clean",
                            error: None,
                        });
                        if !global.json && !global.jsonl && !global.quiet {
                            eprintln!("patch check: {} -- clean", pf.path);
                        }
                    }
                    Err(msg) => {
                        results.push(PatchCheckResult {
                            path: pf.path.clone(),
                            status: "stale",
                            error: Some(msg.clone()),
                        });
                        if !global.json && !global.jsonl && !global.quiet {
                            eprintln!("patch check: {} -- STALE: {}", pf.path, msg);
                        }
                        all_clean = false;
                    }
                }
            }
            if global.json {
                let output = PatchCheckOutput {
                    ok: all_clean,
                    files: results,
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if global.jsonl {
                for r in &results {
                    println!("{}", serde_json::to_string(r)?);
                }
            }
            if all_clean {
                Ok(exit::SUCCESS)
            } else {
                Ok(exit::AMBIGUOUS)
            }
        }
        PatchAction::Apply { .. } => {
            let mut diffs = Vec::new();
            let mut file_changes: Vec<(std::path::PathBuf, String)> = Vec::new();

            for pf in &patch_files {
                let file_path = root.join(&pf.path);
                let original = match std::fs::read_to_string(&file_path) {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(e) => {
                        let msg = format!(
                            "patch apply: {} -- READ ERROR: failed to read {}: {}",
                            pf.path,
                            file_path.display(),
                            e
                        );
                        emit_error(global, &msg)?;
                        return Ok(exit::AMBIGUOUS);
                    }
                };
                let patched = match apply_hunks(&original, &pf.hunks) {
                    Ok(p) => p,
                    Err(msg) => {
                        let msg = format!("patch apply: {} -- STALE: {}", pf.path, msg);
                        emit_error(global, &msg)?;
                        return Ok(exit::AMBIGUOUS);
                    }
                };

                diffs.push(unified_diff(&pf.path, &original, &patched));
                file_changes.push((file_path, patched));
            }

            // --check mode: report whether changes would be made, exit 2.
            if global.check {
                let has_changes = diffs.iter().any(|d| d.has_changes);
                if has_changes {
                    if !global.quiet {
                        println!("{} file(s) would change", file_changes.len());
                    }
                    return Ok(exit::CHANGES_DETECTED);
                }
                return Ok(exit::SUCCESS);
            }

            // --apply mode: write files to disk.
            if global.apply {
                let mut backup = crate::backup::BackupSession::new(&root)?;
                for (file_path, _) in &file_changes {
                    backup.save_before_write(file_path)?;
                }
                for (file_path, patched) in &file_changes {
                    let policy = policy_from_flags(global, Some(file_path));
                    atomic_write(file_path, patched, &policy)?;
                    if !global.quiet {
                        eprintln!("patch apply: {} -- written", file_path.display());
                    }
                }
                backup.finalize()?;
                if global.diff {
                    let result = DiffResult {
                        diffs,
                        total_files_changed: patch_files.len(),
                    };
                    print!("{}", format_diff_result(&result));
                }
                return Ok(exit::SUCCESS);
            }

            // Default / --diff mode: show unified diffs without writing.
            let result = DiffResult {
                diffs,
                total_files_changed: patch_files.len(),
            };
            print!(
                "{}",
                format_diff_result_colored(&result, global.should_color())
            );
            if global.show_status() {
                eprintln!("{} file(s) changed", result.total_files_changed);
            }

            // --confirm: prompt after showing diff, then apply if confirmed.
            if global.should_apply() {
                let mut backup = crate::backup::BackupSession::new(&root)?;
                for (file_path, _) in &file_changes {
                    backup.save_before_write(file_path)?;
                }
                for (file_path, patched) in &file_changes {
                    let policy = policy_from_flags(global, Some(file_path));
                    atomic_write(file_path, patched, &policy)?;
                }
                backup.finalize()?;
            }

            Ok(exit::SUCCESS)
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use crate::ops::patch::{Hunk, PatchLine};
    use tempfile::TempDir;

    /// Helper: default `GlobalFlags` pointing at a directory.
    fn flags_for(dir: &std::path::Path) -> GlobalFlags {
        GlobalFlags {
            cwd: Some(dir.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        }
    }

    // ── parse_patch tests ───────────────────────────────────────────

    #[test]
    fn parse_simple_single_file_diff() {
        let diff = "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "hello.txt");
        assert_eq!(files[0].hunks.len(), 1);

        let h = &files[0].hunks[0];
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 3);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 3);
        assert_eq!(h.lines.len(), 4);
        assert_eq!(h.lines[0], PatchLine::Context("line1".into()));
        assert_eq!(h.lines[1], PatchLine::Remove("old line".into()));
        assert_eq!(h.lines[2], PatchLine::Add("new line".into()));
        assert_eq!(h.lines[3], PatchLine::Context("line3".into()));
    }

    #[test]
    fn parse_multi_file_diff() {
        let diff = "\
--- a/file1.txt
+++ b/file1.txt
@@ -1,2 +1,2 @@
 aaa
-bbb
+ccc
--- a/file2.txt
+++ b/file2.txt
@@ -1,2 +1,2 @@
 xxx
-yyy
+zzz
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "file1.txt");
        assert_eq!(files[1].path, "file2.txt");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[1].hunks.len(), 1);
    }

    // ── apply_hunks tests ───────────────────────────────────────────

    #[test]
    fn apply_simple_hunk_correctly() {
        let original = "line1\nold line\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "line1\nnew line\nline3\n");
    }

    #[test]
    fn stale_context_returns_error() {
        let original = "line1\ncompletely different\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("stale context"));
    }

    #[test]
    fn apply_with_fuzz_offset() {
        // The hunk says old_start=2, but the matching context is actually at
        // line 4 (0-based: 3).  Within fuzz range of 3, so it should still
        // apply.
        let original = "extra1\nextra2\nline1\nold line\nline3\n";
        let hunks = vec![Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: vec![
                PatchLine::Context("line1".into()),
                PatchLine::Remove("old line".into()),
                PatchLine::Add("new line".into()),
                PatchLine::Context("line3".into()),
            ],
        }];
        let result = apply_hunks(original, &hunks).unwrap();
        assert_eq!(result, "extra1\nextra2\nline1\nnew line\nline3\n");
    }

    // ── Integration: check subcommand ───────────────────────────────

    #[test]
    fn check_reports_clean_when_hunks_match() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\nold line\nline3\n").unwrap();

        let diff_path = tmp.path().join("change.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn check_reports_stale_context_with_exit_5() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\ncompletely different\nline3\n").unwrap();

        let diff_path = tmp.path().join("stale.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::AMBIGUOUS);
    }

    #[test]
    fn check_reports_missing_file() {
        let tmp = TempDir::new().unwrap();
        // Do NOT create hello.txt — it should be reported as missing.
        let diff_path = tmp.path().join("missing.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::AMBIGUOUS, "missing file should fail check");
    }

    // ── Integration: apply subcommand ───────────────────────────────

    #[test]
    fn apply_check_rejects_directory_target() {
        let tmp = TempDir::new().unwrap();
        let dir_target = tmp.path().join("hello.txt");
        std::fs::create_dir(&dir_target).unwrap();

        let diff_path = tmp.path().join("dir.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -0,0 +1 @@
+new line
",
        )
        .unwrap();

        let mut global = flags_for(tmp.path());
        global.check = true;
        let args = PatchArgs {
            action: PatchAction::Apply {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::AMBIGUOUS);
        assert!(dir_target.is_dir());
    }

    #[test]
    fn apply_rejects_directory_target() {
        let tmp = TempDir::new().unwrap();
        let dir_target = tmp.path().join("hello.txt");
        std::fs::create_dir(&dir_target).unwrap();

        let diff_path = tmp.path().join("dir.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -0,0 +1 @@
+new line
",
        )
        .unwrap();

        let mut global = flags_for(tmp.path());
        global.apply = true;
        let args = PatchArgs {
            action: PatchAction::Apply {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::AMBIGUOUS);
        assert!(dir_target.is_dir());
    }

    #[test]
    fn apply_writes_patched_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\nold line\nline3\n").unwrap();

        let diff_path = tmp.path().join("change.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        let mut global = flags_for(tmp.path());
        global.apply = true;
        let args = PatchArgs {
            action: PatchAction::Apply {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "line1\nnew line\nline3\n");
    }

    #[test]
    fn apply_dry_run_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        std::fs::write(&file, "line1\nold line\nline3\n").unwrap();

        let diff_path = tmp.path().join("change.patch");
        std::fs::write(
            &diff_path,
            "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 line1
-old line
+new line
 line3
",
        )
        .unwrap();

        // Without --apply, default is dry-run (show diff, don't write).
        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Apply {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        // File should NOT have been modified.
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content, "line1\nold line\nline3\n",
            "file should remain unchanged in dry-run mode"
        );
    }

    // ── Malformed diff ──────────────────────────────────────────────

    #[test]
    fn malformed_diff_returns_parse_error() {
        let tmp = TempDir::new().unwrap();
        let diff_path = tmp.path().join("bad.patch");
        std::fs::write(&diff_path, "this is not a diff at all\n").unwrap();

        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: Some(diff_path.to_string_lossy().into_owned()),
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    #[test]
    fn no_input_source_returns_parse_error() {
        let tmp = TempDir::new().unwrap();
        let global = flags_for(tmp.path());
        let args = PatchArgs {
            action: PatchAction::Check {
                file: None,
                stdin: false,
            },
            write: Default::default(),
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::PARSE_ERROR);
    }

    // ── Additional parser edge-case tests ───────────────────────────

    #[test]
    fn parse_diff_with_git_prefix_lines() {
        let diff = "\
diff --git a/foo.rs b/foo.rs
index abc1234..def5678 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,2 +1,2 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"world\");
";
        let files = parse_patch(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "foo.rs");
    }

    #[test]
    fn parse_hunk_without_comma_count() {
        // Single-line ranges like `@@ -1 +1 @@` (count defaults to 1).
        let diff = "\
--- a/one.txt
+++ b/one.txt
@@ -1 +1 @@
-old
+new
";
        let files = parse_patch(diff).unwrap();
        let h = &files[0].hunks[0];
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_count, 1);
    }
}
