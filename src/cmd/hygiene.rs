use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::unified_diff;
use crate::exit;
use crate::write::{apply_policy, atomic_write, WritePolicy};
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Args)]
pub struct HygieneArgs {
    #[command(subcommand)]
    pub action: HygieneAction,
}

#[derive(Debug, clap::Subcommand)]
pub enum HygieneAction {
    /// Report newline, EOL, and whitespace issues.
    Check { paths: Vec<String> },
    /// Apply normalization fixes.
    Fix { paths: Vec<String> },
}

/// A single hygiene issue found in a file.
#[derive(Debug, Clone, Serialize)]
pub struct HygieneIssue {
    pub path: String,
    pub issue: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB, the same heuristic Git uses).
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Check a single file for hygiene issues.
fn check_file(path: &Path) -> anyhow::Result<Vec<HygieneIssue>> {
    let data = std::fs::read(path)?;

    if data.is_empty() || is_binary(&data) {
        return Ok(Vec::new());
    }

    let path_str = path.to_string_lossy().to_string();
    let mut issues = Vec::new();

    // Check missing final newline.
    if !data.ends_with(b"\n") {
        issues.push(HygieneIssue {
            path: path_str.clone(),
            issue: "missing final newline".to_string(),
            line: None,
        });
    }

    // Check mixed line endings: file has both \r\n and bare \n.
    let has_crlf = data.windows(2).any(|w| w == b"\r\n");
    // A bare \n is any \n not preceded by \r.
    let has_bare_lf = data
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\n' && (i == 0 || data[i - 1] != b'\r'));
    if has_crlf && has_bare_lf {
        issues.push(HygieneIssue {
            path: path_str.clone(),
            issue: "mixed line endings".to_string(),
            line: None,
        });
    }

    // Check trailing whitespace per line.
    // We split on \n so each "line" may still contain a trailing \r from CRLF.
    for (line_idx, raw_line) in data.split(|&b| b == b'\n').enumerate() {
        // Strip trailing \r if present (from CRLF).
        let content = if raw_line.last() == Some(&b'\r') {
            &raw_line[..raw_line.len() - 1]
        } else {
            raw_line
        };
        // Skip completely empty lines and the phantom empty element after a
        // trailing newline.
        if content.is_empty() {
            continue;
        }
        if matches!(content.last(), Some(b' ' | b'\t')) {
            issues.push(HygieneIssue {
                path: path_str.clone(),
                issue: "trailing whitespace".to_string(),
                line: Some(line_idx + 1), // 1-based
            });
        }
    }

    Ok(issues)
}

/// Collect all issues from the given paths, honouring .gitignore and optional
/// glob filtering.
fn collect_issues(paths: &[String], global: &GlobalFlags) -> anyhow::Result<Vec<HygieneIssue>> {
    let root = if let Some(ref cwd) = global.cwd {
        std::path::PathBuf::from(cwd)
    } else {
        std::env::current_dir()?
    };

    let glob_matcher = match &global.glob {
        Some(pattern) => Some(Glob::new(pattern)?.compile_matcher()),
        None => None,
    };

    let explicit_files = global.read_files_from();

    let effective_paths: Vec<String> = if paths.is_empty() {
        vec![".".to_string()]
    } else {
        paths.to_vec()
    };

    let mut issues = Vec::new();

    let file_paths: Vec<std::path::PathBuf> = if let Some(ref files) = explicit_files {
        files.iter().map(|f| root.join(f)).collect()
    } else {
        let mut collected = Vec::new();
        for start in &effective_paths {
            let start_path = root.join(start);
            let walker = WalkBuilder::new(&start_path).hidden(false).build();
            for entry in walker {
                let entry = entry?;
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    collected.push(entry.into_path());
                }
            }
        }
        collected
    };

    for path_buf in &file_paths {
        let file_path = path_buf.as_path();

        // Apply glob filter if set.
        if let Some(ref matcher) = glob_matcher {
            let name = file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !matcher.is_match(&name) {
                let rel = file_path.strip_prefix(&root).unwrap_or(file_path);
                if !matcher.is_match(rel) {
                    continue;
                }
            }
        }

        let file_issues = check_file(file_path)?;
        issues.extend(file_issues);
    }

    Ok(issues)
}

/// Render issues to stdout.
fn render_issues(issues: &[HygieneIssue], global: &GlobalFlags) {
    if global.json {
        println!(
            "{}",
            serde_json::to_string_pretty(issues).expect("serialize issues")
        );
    } else if global.jsonl {
        for issue in issues {
            println!("{}", serde_json::to_string(issue).expect("serialize issue"));
        }
    } else {
        for issue in issues {
            if let Some(line) = issue.line {
                println!("{}:{}: {}", issue.path, line, issue.issue);
            } else {
                println!("{}: {}", issue.path, issue.issue);
            }
        }
    }
}

pub fn run(args: HygieneArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    match args.action {
        HygieneAction::Check { paths } => {
            let issues = collect_issues(&paths, global)?;
            render_issues(&issues, global);
            if issues.is_empty() {
                Ok(exit::SUCCESS)
            } else {
                Ok(exit::CHANGES_DETECTED)
            }
        }
        HygieneAction::Fix { paths } => {
            let root = if let Some(ref cwd) = global.cwd {
                std::path::PathBuf::from(cwd)
            } else {
                std::env::current_dir()?
            };

            let glob_matcher = match &global.glob {
                Some(pattern) => Some(Glob::new(pattern)?.compile_matcher()),
                None => None,
            };

            let explicit_fix_files = global.read_files_from();

            let effective_paths: Vec<String> = if paths.is_empty() {
                vec![".".to_string()]
            } else {
                paths.to_vec()
            };

            let policy = WritePolicy {
                ensure_final_newline: global.ensure_final_newline,
                normalize_eol: global.normalize_eol.unwrap_or(EolMode::Keep),
                trim_trailing_whitespace: global.trim_trailing_whitespace,
            };

            let mut any_changed = false;

            let fix_file_paths: Vec<std::path::PathBuf> =
                if let Some(ref files) = explicit_fix_files {
                    files.iter().map(|f| root.join(f)).collect()
                } else {
                    let mut collected = Vec::new();
                    for start in &effective_paths {
                        let start_path = root.join(start);
                        let walker = WalkBuilder::new(&start_path).hidden(false).build();
                        for entry in walker {
                            let entry = entry?;
                            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                                collected.push(entry.into_path());
                            }
                        }
                    }
                    collected
                };

            for path_buf in &fix_file_paths {
                let file_path = path_buf.as_path();

                if let Some(ref matcher) = glob_matcher {
                    let name = file_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !matcher.is_match(&name) {
                        let rel = file_path.strip_prefix(&root).unwrap_or(file_path);
                        if !matcher.is_match(rel) {
                            continue;
                        }
                    }
                }

                let data = std::fs::read(file_path)?;
                if data.is_empty() || is_binary(&data) {
                    continue;
                }

                let original = String::from_utf8_lossy(&data);
                let fixed = apply_policy(&original, &policy);

                if fixed == *original {
                    continue;
                }

                any_changed = true;

                let rel_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .to_string();

                if global.check {
                    println!("{rel_path}");
                } else if global.apply {
                    let noop = WritePolicy::default();
                    atomic_write(file_path, &fixed, &noop)?;
                } else {
                    let diff = unified_diff(&rel_path, &original, &fixed);
                    if diff.has_changes {
                        println!("--- a/{rel_path}");
                        println!("+++ b/{rel_path}");
                        for hunk in &diff.hunks {
                            print!("{hunk}");
                        }
                    }
                }
            }

            if any_changed {
                Ok(exit::CHANGES_DETECTED)
            } else {
                Ok(exit::SUCCESS)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    /// Helper: create a default `GlobalFlags` pointing at the given dir.
    fn flags_for(dir: &Path) -> GlobalFlags {
        GlobalFlags {
            json: false,
            jsonl: false,
            diff: false,
            apply: false,
            check: false,
            cwd: Some(dir.to_string_lossy().to_string()),
            glob: None,
            files_from: None,
            atomic: false,
            ensure_final_newline: false,
            normalize_eol: None,
            trim_trailing_whitespace: false,
        }
    }

    #[test]
    fn detects_missing_final_newline() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_newline.txt");
        std::fs::write(&file, b"hello").unwrap();

        let issues = check_file(&file).unwrap();
        assert!(
            issues.iter().any(|i| i.issue == "missing final newline"),
            "expected missing final newline issue, got: {issues:?}"
        );
    }

    #[test]
    fn detects_mixed_line_endings() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("mixed.txt");
        // First line uses CRLF, second uses bare LF.
        std::fs::write(&file, b"line1\r\nline2\nline3\n").unwrap();

        let issues = check_file(&file).unwrap();
        assert!(
            issues.iter().any(|i| i.issue == "mixed line endings"),
            "expected mixed line endings issue, got: {issues:?}"
        );
    }

    #[test]
    fn detects_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("trailing.txt");
        std::fs::write(&file, b"hello   \nworld\n").unwrap();

        let issues = check_file(&file).unwrap();
        let trailing: Vec<_> = issues
            .iter()
            .filter(|i| i.issue == "trailing whitespace")
            .collect();
        assert_eq!(trailing.len(), 1, "expected 1 trailing whitespace issue");
        assert_eq!(trailing[0].line, Some(1));
    }

    #[test]
    fn clean_file_produces_no_issues_and_exit_zero() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        std::fs::write(&file, b"hello\nworld\n").unwrap();

        let issues = check_file(&file).unwrap();
        assert!(issues.is_empty(), "expected no issues for clean file");

        let global = flags_for(tmp.path());
        let args = HygieneArgs {
            action: HygieneAction::Check {
                paths: vec![".".to_string()],
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn multiple_issues_in_one_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("multi.txt");
        // Missing final newline + trailing whitespace on line 1 + mixed endings.
        std::fs::write(&file, b"hello \r\nworld\nfoo").unwrap();

        let issues = check_file(&file).unwrap();
        let issue_types: Vec<&str> = issues.iter().map(|i| i.issue.as_str()).collect();
        assert!(
            issue_types.contains(&"missing final newline"),
            "missing final newline not found in {issue_types:?}"
        );
        assert!(
            issue_types.contains(&"mixed line endings"),
            "mixed line endings not found in {issue_types:?}"
        );
        assert!(
            issue_types.contains(&"trailing whitespace"),
            "trailing whitespace not found in {issue_types:?}"
        );
    }

    #[test]
    fn binary_files_are_skipped() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("binary.bin");
        // Binary content with NUL bytes, missing final newline, trailing ws.
        let mut data = b"hello \nworld".to_vec();
        data.insert(3, 0x00);
        std::fs::write(&file, &data).unwrap();

        let issues = check_file(&file).unwrap();
        assert!(
            issues.is_empty(),
            "expected no issues for binary file, got: {issues:?}"
        );
    }

    #[test]
    fn glob_filtering_works() {
        let tmp = TempDir::new().unwrap();
        // Create two files: one .rs and one .txt. The .txt has an issue.
        let rs_file = tmp.path().join("clean.rs");
        std::fs::write(&rs_file, b"fn main() {}\n").unwrap();

        let txt_file = tmp.path().join("dirty.txt");
        std::fs::write(&txt_file, b"no newline").unwrap();

        // Filter to only *.rs — should find no issues.
        let mut global = flags_for(tmp.path());
        global.glob = Some("*.rs".to_string());

        let issues = collect_issues(&[".".to_string()], &global).unwrap();
        assert!(
            issues.is_empty(),
            "expected no issues when filtering to *.rs, got: {issues:?}"
        );

        // Filter to only *.txt — should find the missing newline.
        global.glob = Some("*.txt".to_string());
        let issues = collect_issues(&[".".to_string()], &global).unwrap();
        assert!(
            !issues.is_empty(),
            "expected issues when filtering to *.txt"
        );
        assert!(issues.iter().any(|i| i.issue == "missing final newline"));
    }

    #[test]
    fn fix_adds_missing_newline() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("no_nl.txt");
        std::fs::write(&file, b"hello").unwrap();

        let mut global = flags_for(tmp.path());
        global.ensure_final_newline = true;
        global.apply = true;

        let args = HygieneArgs {
            action: HygieneAction::Fix {
                paths: vec![".".to_string()],
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        let content = std::fs::read(&file).unwrap();
        assert!(
            content.ends_with(b"\n"),
            "expected file to end with newline after fix"
        );
    }

    #[test]
    fn fix_normalizes_eol() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("crlf.txt");
        std::fs::write(&file, b"line1\r\nline2\r\n").unwrap();

        let mut global = flags_for(tmp.path());
        global.normalize_eol = Some(crate::cli::global::EolMode::Lf);
        global.apply = true;

        let args = HygieneArgs {
            action: HygieneAction::Fix {
                paths: vec![".".to_string()],
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        let content = std::fs::read(&file).unwrap();
        assert!(
            !content.windows(2).any(|w| w == b"\r\n"),
            "expected no CRLF sequences after fix"
        );
    }

    #[test]
    fn fix_trims_trailing_whitespace() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("trailing.txt");
        std::fs::write(&file, b"hello   \nworld\t\n").unwrap();

        let mut global = flags_for(tmp.path());
        global.trim_trailing_whitespace = true;
        global.apply = true;

        let args = HygieneArgs {
            action: HygieneAction::Fix {
                paths: vec![".".to_string()],
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::CHANGES_DETECTED);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn fix_is_idempotent_on_clean_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("clean.txt");
        std::fs::write(&file, b"hello\nworld\n").unwrap();

        let mut global = flags_for(tmp.path());
        global.ensure_final_newline = true;
        global.trim_trailing_whitespace = true;
        global.normalize_eol = Some(crate::cli::global::EolMode::Lf);
        global.apply = true;

        let args = HygieneArgs {
            action: HygieneAction::Fix {
                paths: vec![".".to_string()],
            },
        };
        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello\nworld\n");
    }
}
