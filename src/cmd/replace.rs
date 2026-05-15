use crate::cli::global::{EolMode, GlobalFlags};
use crate::diff::{format_diff_result, unified_diff, DiffResult};
use crate::exit;
use crate::write::{atomic_write, WritePolicy};
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
use regex::Regex;
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
    /// Treat --from as a regex.
    #[arg(long)]
    pub regex: bool,
    /// Return success even if no matches found (idempotent mode).
    #[arg(long)]
    pub if_exists: bool,
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

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB).
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

fn build_write_policy(global: &GlobalFlags) -> WritePolicy {
    WritePolicy {
        ensure_final_newline: global.ensure_final_newline,
        normalize_eol: global.normalize_eol.unwrap_or(EolMode::Keep),
        trim_trailing_whitespace: global.trim_trailing_whitespace,
    }
}

/// Count occurrences and produce replaced content.
fn replace_content(
    content: &str,
    from: &str,
    to: &str,
    use_regex: bool,
) -> anyhow::Result<(String, usize)> {
    if use_regex {
        let re = Regex::new(from)?;
        let count = re.find_iter(content).count();
        if count == 0 {
            return Ok((content.to_owned(), 0));
        }
        let replaced = re.replace_all(content, to).to_string();
        Ok((replaced, count))
    } else {
        let count = content.matches(from).count();
        if count == 0 {
            return Ok((content.to_owned(), 0));
        }
        let replaced = content.replace(from, to);
        Ok((replaced, count))
    }
}

/// Walk files and collect all replacements.
fn collect_replacements(
    args: &ReplaceArgs,
    global: &GlobalFlags,
) -> anyhow::Result<Vec<FileReplacement>> {
    let glob_matcher = global
        .glob
        .as_ref()
        .map(|p| Glob::new(p).map(|g| g.compile_matcher()))
        .transpose()?;

    let explicit_files = global.read_files_from();

    let paths: Vec<&str> = if args.paths.is_empty() {
        vec!["."]
    } else {
        args.paths.iter().map(String::as_str).collect()
    };

    let file_paths: Vec<std::path::PathBuf> = if let Some(ref files) = explicit_files {
        files.iter().map(std::path::PathBuf::from).collect()
    } else {
        let mut builder = WalkBuilder::new(paths[0]);
        for path in &paths[1..] {
            builder.add(path);
        }
        builder
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .map(|e| e.into_path())
            .collect()
    };

    let use_regex = args.regex;
    let mut replacements = Vec::new();

    for path_buf in &file_paths {
        let path = path_buf.as_path();

        if let Some(ref matcher) = glob_matcher {
            if !matcher.is_match(path) && !path.file_name().is_some_and(|n| matcher.is_match(n)) {
                continue;
            }
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

        let (replaced, count) = replace_content(&content, &args.from, &args.to, use_regex)?;

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
    if let Some(ref cwd) = global.cwd {
        std::env::set_current_dir(cwd)?;
    }

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
        } else {
            println!("{total_matches} match(es) in {file_count} file(s)");
            for f in &files {
                println!("  {}: {} match(es)", f.path, f.match_count);
            }
        }
        return Ok(exit::CHANGES_DETECTED);
    }

    // --apply mode: write changes using atomic_write with write policy.
    if global.apply {
        let policy = build_write_policy(global);
        for r in &replacements {
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
        } else {
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
        GlobalFlags {
            json: false,
            jsonl: false,
            diff: false,
            apply: false,
            check: false,
            cwd: None,
            glob: None,
            files_from: None,
            atomic: false,
            ensure_final_newline: false,
            normalize_eol: None,
            trim_trailing_whitespace: false,
        }
    }

    fn make_args(from: &str, to: &str, paths: Vec<String>) -> ReplaceArgs {
        ReplaceArgs {
            from: from.to_string(),
            to: to.to_string(),
            paths,
            literal: true,
            regex: false,
            if_exists: false,
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
        };
        let replacements = collect_replacements(&args, &default_global()).unwrap();

        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].match_count, 2);
        assert_eq!(replacements[0].replaced, "replacedbar\nreplacedbaz\n");
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
        };
        let mut global = default_global();
        global.apply = true;

        let code = run(args, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hi world\n");
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
}
