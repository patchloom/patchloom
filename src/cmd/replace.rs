use crate::cli::global::GlobalFlags;
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_write, policy_from_flags};
use clap::Args;
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Args)]
pub struct ReplaceArgs {
    /// Text to find.
    #[arg(long)]
    pub from: String,
    /// Text to replace with.
    #[arg(long)]
    pub to: String,
    /// Paths to operate on.
    pub paths: Vec<String>,
    /// Treat --from as a literal string (default).
    #[arg(long)]
    pub literal: bool,
    // ref:replace-mode:regex
    /// Treat --from as a regex.
    #[arg(long)]
    pub regex: bool,
    // ref:replace-mode:if-exists
    /// Return success even if no matches found (idempotent mode).
    #[arg(long)]
    pub if_exists: bool,
    // ref:replace-mode:multiline
    /// Enable multiline matching (dot matches newlines in regex mode).
    #[arg(long, short = 'U')]
    pub multiline: bool,
    // ref:replace-mode:nth
    /// Replace only the Nth occurrence (1-based).
    #[arg(long)]
    pub nth: Option<usize>,
    // ref:replace-mode:case-insensitive
    /// Case-insensitive matching.
    #[arg(long, short = 'i')]
    pub case_insensitive: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Clone, Serialize)]
struct ReplaceFileResult {
    path: String,
    match_count: usize,
}

#[derive(Debug, Serialize)]
struct ReplaceOutput {
    ok: bool,
    match_count: usize,
    file_count: usize,
    files: Vec<ReplaceFileResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
}

/// Result of processing a single file.
struct FileReplacement {
    path: String,
    original: String,
    replaced: String,
    match_count: usize,
}

use crate::is_binary;

/// Count occurrences and produce replaced content.
fn replace_content(
    content: &str,
    from: &str,
    to: &str,
    compiled_re: Option<&Regex>,
    nth: Option<usize>,
) -> (String, usize) {
    if let Some(n) = nth {
        // Replace only the Nth occurrence (1-based).
        if let Some(re) = compiled_re {
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            for m in re.find_iter(content) {
                count += 1;
                if count == n {
                    result.push_str(&content[..m.start()]);
                    let mut dst = String::new();
                    if let Some(caps) = re.captures(&content[m.start()..]) {
                        caps.expand(to, &mut dst);
                    }
                    result.push_str(&dst);
                    result.push_str(&content[m.end()..]);
                    return (result, 1);
                }
            }
            (content.to_owned(), 0)
        } else {
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            for (start, _) in content.match_indices(from) {
                count += 1;
                if count == n {
                    result.push_str(&content[..start]);
                    result.push_str(to);
                    result.push_str(&content[start + from.len()..]);
                    return (result, 1);
                }
            }
            (content.to_owned(), 0)
        }
    } else if let Some(re) = compiled_re {
        // Single-pass: count replacements while producing the result.
        let mut count = 0usize;
        let replaced = re
            .replace_all(content, |caps: &regex::Captures| {
                count += 1;
                let mut dst = String::new();
                caps.expand(to, &mut dst);
                dst
            })
            .to_string();
        if count == 0 {
            return (content.to_owned(), 0);
        }
        (replaced, count)
    } else {
        let count = content.matches(from).count();
        if count == 0 {
            return (content.to_owned(), 0);
        }
        let replaced = content.replace(from, to);
        (replaced, count)
    }
}

/// Walk files and collect all replacements.
fn collect_replacements(
    args: &ReplaceArgs,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<FileReplacement>> {
    let glob_matcher = crate::build_glob_matcher(global)?;
    let file_paths = crate::collect_file_paths(&args.paths, global)?;

    let compiled_re = if args.regex {
        Some(
            RegexBuilder::new(&args.from)
                .dot_matches_new_line(args.multiline)
                .case_insensitive(args.case_insensitive)
                .build()?,
        )
    } else if args.case_insensitive {
        // For literal mode with case-insensitive, use regex with escaped pattern.
        Some(
            RegexBuilder::new(&regex::escape(&args.from))
                .case_insensitive(true)
                .build()?,
        )
    } else {
        None
    };
    let mut replacements = Vec::new();

    for path_buf in &file_paths {
        let path = path_buf.as_path();

        if !crate::matches_glob(path, glob_matcher.as_ref()) {
            continue;
        }

        let data = match fs::read(path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        if data.is_empty() || is_binary(&data) {
            continue;
        }

        let content = match String::from_utf8(data) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let (replaced, count) = replace_content(
            &content,
            &args.from,
            &args.to,
            compiled_re.as_ref(),
            args.nth,
        );

        if count > 0 {
            replacements.push(FileReplacement {
                path: path.to_string_lossy().to_string(),
                original: content,
                replaced,
                match_count: count,
            });
        }
    }

    replacements.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(replacements)
}

fn make_file_results(replacements: &[FileReplacement]) -> Vec<ReplaceFileResult> {
    replacements
        .iter()
        .map(|r| ReplaceFileResult {
            path: r.path.clone(),
            match_count: r.match_count,
        })
        .collect()
}

fn make_diff_output(replacements: &[FileReplacement]) -> String {
    let diffs: Vec<_> = replacements
        .iter()
        .map(|r| unified_diff(&r.path, &r.original, &r.replaced))
        .collect();
    let total_files_changed = diffs.iter().filter(|d| d.has_changes).count();
    let diff_result = DiffResult {
        diffs,
        total_files_changed,
    };
    format_diff_result(&diff_result)
}

pub fn run(args: ReplaceArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    std::env::set_current_dir(global.resolve_cwd()?)?;

    let replacements = collect_replacements(&args, global)?;

    if replacements.is_empty() {
        if args.if_exists {
            return Ok(exit::SUCCESS);
        }
        return Ok(exit::NO_MATCHES);
    }

    let total_matches: usize = replacements.iter().map(|r| r.match_count).sum();
    let file_count = replacements.len();
    let files = make_file_results(&replacements);

    // --check mode: report summary, exit 2 if changes needed.
    if global.check {
        if global.json {
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files,
                diff: None,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if global.jsonl {
            for f in &files {
                println!("{}", serde_json::to_string(f)?);
            }
        } else if !global.quiet {
            println!("{total_matches} match(es) in {file_count} file(s)");
            for f in &files {
                println!("  {}: {} match(es)", f.path, f.match_count);
            }
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: write changes using atomic_write with write policy.
    if global.apply {
        for r in &replacements {
            let policy = policy_from_flags(global, Some(Path::new(&r.path)));
            atomic_write(Path::new(&r.path), &r.replaced, &policy)?;
        }

        if global.json {
            let diff_text = if global.diff {
                Some(make_diff_output(&replacements))
            } else {
                None
            };
            let output = ReplaceOutput {
                ok: true,
                match_count: total_matches,
                file_count,
                files,
                diff: diff_text,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if global.jsonl {
            for f in &files {
                println!("{}", serde_json::to_string(f)?);
            }
        } else if global.diff {
            print!("{}", make_diff_output(&replacements));
        } else if !global.quiet {
            println!("replaced {total_matches} match(es) in {file_count} file(s)");
            for f in &files {
                println!("  {}: {} match(es)", f.path, f.match_count);
            }
        }
        return Ok(exit::SUCCESS);
    }

    // Default / --diff mode: show unified diff of changes.
    if global.json {
        let output = ReplaceOutput {
            ok: true,
            match_count: total_matches,
            file_count,
            files,
            diff: Some(make_diff_output(&replacements)),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if global.jsonl {
        for f in &files {
            println!("{}", serde_json::to_string(f)?);
        }
    } else {
        print!("{}", make_diff_output(&replacements));
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

    fn make_args(from: &str, to: &str, paths: Vec<String>) -> ReplaceArgs {
        ReplaceArgs {
            from: from.to_string(),
            to: to.to_string(),
            paths,
            literal: true,
            regex: false,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        }
    }

    #[test]
    fn literal_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\nhello again\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "hi world\nhi again\n");
    }

    #[test]
    fn regex_replace_works() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "foo123bar\nfoo456baz\n").unwrap();

        let args = ReplaceArgs {
            from: r"foo\d+".to_string(),
            to: "replaced".to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "replacedbar\nreplacedbaz\n");
    }

    #[test]
    fn regex_capture_groups_in_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "version = \"1.2.3\"\n").unwrap();

        let args = ReplaceArgs {
            from: r#"version = "(\d+)\.(\d+)\.(\d+)""#.to_string(),
            to: r#"version = "$1.$2.99""#.to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &default_global()).unwrap();
        assert_eq!(replacements.len(), 1);
        assert_eq!(
            replacements[0].replaced, "version = \"1.2.99\"\n",
            "capture groups $1/$2 should work in replacement text"
        );
    }

    #[test]
    fn no_matches_returns_exit_3() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "zzz_no_match_zzz",
            "replacement",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn diff_mode_produces_unified_diff() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "old line\n").unwrap();

        let args = make_args(
            "old",
            "new",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &default_global()).unwrap();
        let diff_output = make_diff_output(&replacements);

        assert!(diff_output.contains("--- a/"));
        assert!(diff_output.contains("+++ b/"));
        assert!(diff_output.contains("-old line"));
        assert!(diff_output.contains("+new line"));
    }

    #[test]
    fn apply_mode_writes_replacement() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multi_file_replace() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "hello from a\n").unwrap();
        fs::write(dir.path().join("b.txt"), "hello from b\n").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert_eq!(replacements.len(), 2);
        let total: usize = replacements.iter().map(|r| r.match_count).sum();
        assert_eq!(total, 2);
    }

    #[test]
    fn if_exists_returns_success_on_no_matches() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "zzz_no_match_zzz".to_string(),
            to: "replacement".to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::SUCCESS);
    }

    #[test]
    fn if_exists_still_replaces_when_found() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world\n").unwrap();

        let args = ReplaceArgs {
            from: "hello".to_string(),
            to: "hi".to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: true,
            regex: false,
            if_exists: true,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn binary_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let bin_file = dir.path().join("data.bin");
        // Write a file with NUL bytes (binary content).
        fs::write(&bin_file, b"hello\x00world").unwrap();

        let args = make_args(
            "hello",
            "replaced",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let replacements = collect_replacements(&args, &default_global()).unwrap();
        assert!(
            replacements.is_empty(),
            "binary files should be skipped, got {} matches",
            replacements.len()
        );
    }

    #[test]
    fn write_policy_ensure_final_newline_applied() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world").unwrap();

        let args = make_args(
            "hello",
            "hi",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let mut global = default_global();
        global.apply = true;
        global.ensure_final_newline = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
    }

    #[test]
    fn multiline_regex_spans_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: "replaced".to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: true,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 1);
        assert_eq!(replacements[0].replaced, "replaced\n");
    }

    #[test]
    fn multiline_false_does_not_span_newlines() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "start\nmiddle\nend\n").unwrap();

        let args = ReplaceArgs {
            from: r"start.*end".to_string(),
            to: "replaced".to_string(),
            paths: vec![dir.path().to_string_lossy().into_owned()],
            literal: false,
            regex: true,
            if_exists: false,
            multiline: false,
            nth: None,
            case_insensitive: false,
            write: Default::default(),
        };
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert!(
            replacements.is_empty(),
            "without multiline, dot should not match newlines"
        );
    }
}
