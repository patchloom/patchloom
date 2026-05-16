use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::bail;
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Pattern to search for.
    pub pattern: String,
    /// Paths to search in.
    pub paths: Vec<String>,
    /// Treat pattern as a literal string.
    #[arg(long)]
    pub literal: bool,
    /// Treat pattern as a regex (default).
    #[arg(long)]
    pub regex: bool,
    /// Lines of context around matches.
    #[arg(long, short = 'C')]
    pub context: Option<usize>,
    // ref:search-mode:files-with-matches
    /// Only print file paths with matches.
    #[arg(long)]
    pub files_with_matches: bool,
    // ref:search-mode:count
    /// Only print match counts per file.
    #[arg(long)]
    pub count: bool,
    // ref:search-mode:invert-match
    /// Show lines that do NOT match the pattern.
    #[arg(long, short = 'v')]
    pub invert_match: bool,
    // ref:search-mode:multiline
    /// Enable multiline matching (dot matches newlines).
    #[arg(long, short = 'U')]
    pub multiline: bool,
    // ref:search-mode:case-insensitive
    /// Case-insensitive matching.
    #[arg(long, short = 'i')]
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SearchMatch {
    path: String,
    line: usize,
    column: usize,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_after: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct SearchOutput {
    ok: bool,
    matches: Vec<SearchMatch>,
    match_count: usize,
    file_count: usize,
}

struct SearchResults {
    matches: Vec<SearchMatch>,
    file_match_counts: BTreeMap<String, usize>,
}

fn collect_matches(args: &SearchArgs, global: &GlobalFlags) -> anyhow::Result<SearchResults> {
    let glob_matcher = crate::build_glob_matcher(global)?;

    let pattern = if args.literal {
        regex::escape(&args.pattern)
    } else {
        args.pattern.clone()
    };
    let re = if args.multiline || args.case_insensitive {
        regex::RegexBuilder::new(&pattern)
            .dot_matches_new_line(args.multiline)
            .case_insensitive(args.case_insensitive)
            .build()?
    } else {
        Regex::new(&pattern)?
    };

    let mut all_matches: Vec<SearchMatch> = Vec::new();
    let mut file_match_counts: BTreeMap<String, usize> = BTreeMap::new();
    let count_only = args.count || args.files_with_matches;

    let file_paths = crate::collect_file_paths(&args.paths, global)?;

    for path in &file_paths {
        let path = path.as_path();

        if !crate::matches_glob(path, glob_matcher.as_ref()) {
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut count = 0usize;
        // Defer the allocation until we know the file has matches.
        let mut path_str: Option<String> = None;
        let mut get_path_str = || -> String {
            path_str
                .get_or_insert_with(|| path.to_string_lossy().to_string())
                .clone()
        };

        if args.multiline {
            for m in re.find_iter(&content) {
                count += 1;
                if count_only {
                    if args.files_with_matches {
                        break;
                    }
                    continue;
                }
                let line_num = content[..m.start()].matches('\n').count() + 1;
                let col = m.start() - content[..m.start()].rfind('\n').map_or(0, |pos| pos + 1) + 1;
                all_matches.push(SearchMatch {
                    path: get_path_str(),
                    line: line_num,
                    column: col,
                    text: m.as_str().to_string(),
                    context_before: None,
                    context_after: None,
                });
            }
        } else if let Some(context) = args.context {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().copied().enumerate() {
                let found = re.find(line);
                let is_match = if args.invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };

                if !is_match {
                    continue;
                }

                count += 1;
                if count_only {
                    if args.files_with_matches {
                        break;
                    }
                    continue;
                }

                let start = i.saturating_sub(context);
                let end = (i + 1 + context).min(lines.len());
                let column = found.map_or(1, |m| m.start() + 1);
                let context_before = Some(lines[start..i].iter().map(|s| s.to_string()).collect());
                let context_after = Some(lines[i + 1..end].iter().map(|s| s.to_string()).collect());

                all_matches.push(SearchMatch {
                    path: get_path_str(),
                    line: i + 1,
                    column,
                    text: line.to_string(),
                    context_before,
                    context_after,
                });
            }
        } else {
            for (i, line) in content.lines().enumerate() {
                let found = re.find(line);
                let is_match = if args.invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };

                if !is_match {
                    continue;
                }

                count += 1;
                if count_only {
                    if args.files_with_matches {
                        break;
                    }
                    continue;
                }

                let column = found.map_or(1, |m| m.start() + 1);
                all_matches.push(SearchMatch {
                    path: get_path_str(),
                    line: i + 1,
                    column,
                    text: line.to_string(),
                    context_before: None,
                    context_after: None,
                });
            }
        }

        if count > 0 {
            let key = path_str.unwrap_or_else(|| path.to_string_lossy().to_string());
            file_match_counts.insert(key, count);
        }
    }

    if !count_only {
        all_matches.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
    }

    Ok(SearchResults {
        matches: all_matches,
        file_match_counts,
    })
}

fn format_results(
    results: &SearchResults,
    args: &SearchArgs,
    global: &GlobalFlags,
) -> anyhow::Result<String> {
    let mut out = String::new();

    if global.json {
        let payload = SearchOutput {
            ok: true,
            match_count: results.file_match_counts.values().sum(),
            file_count: results.file_match_counts.len(),
            matches: results.matches.clone(),
        };
        out = serde_json::to_string_pretty(&payload)?;
        out.push('\n');
    } else if global.jsonl {
        if args.files_with_matches {
            // count_only mode: matches is empty, emit one line per file.
            for path in results.file_match_counts.keys() {
                out.push_str(&serde_json::to_string(&serde_json::json!({"path": path}))?);
                out.push('\n');
            }
        } else if args.count {
            for (path, count) in &results.file_match_counts {
                out.push_str(&serde_json::to_string(
                    &serde_json::json!({"path": path, "count": count}),
                )?);
                out.push('\n');
            }
        } else {
            for m in &results.matches {
                out.push_str(&serde_json::to_string(m)?);
                out.push('\n');
            }
        }
    } else if args.files_with_matches {
        for path in results.file_match_counts.keys() {
            out.push_str(path);
            out.push('\n');
        }
    } else if args.count {
        for (path, count) in &results.file_match_counts {
            out.push_str(&format!("{path}:{count}\n"));
        }
    } else {
        let has_ctx = args.context.is_some();
        let mut first = true;
        for m in &results.matches {
            if has_ctx && !first {
                out.push_str("--\n");
            }
            first = false;
            if let Some(ref before) = m.context_before {
                for (j, ctx) in before.iter().enumerate() {
                    let ln = m.line - before.len() + j;
                    out.push_str(&format!("{}-{ln}-{ctx}\n", m.path));
                }
            }
            out.push_str(&format!("{}:{}:{}:{}\n", m.path, m.line, m.column, m.text));
            if let Some(ref after) = m.context_after {
                for (j, ctx) in after.iter().enumerate() {
                    let ln = m.line + 1 + j;
                    out.push_str(&format!("{}-{ln}-{ctx}\n", m.path));
                }
            }
        }
    }

    Ok(out)
}

pub fn run(args: SearchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    std::env::set_current_dir(global.resolve_cwd()?)?;

    if args.invert_match && args.multiline {
        bail!("--invert-match and --multiline cannot be combined");
    }

    let results = collect_matches(&args, global)?;

    let has_matches = if args.count || args.files_with_matches {
        !results.file_match_counts.is_empty()
    } else {
        !results.matches.is_empty()
    };
    if !has_matches {
        return Ok(exit::NO_MATCHES);
    }

    if !global.quiet {
        let output = format_results(&results, &args, global)?;
        print!("{output}");
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("hello.txt"),
            "Hello, world!\nThis is a test file.\nHello again!\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("code.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        dir
    }

    fn default_global() -> GlobalFlags {
        GlobalFlags::default()
    }

    fn make_args(pattern: &str, paths: Vec<String>) -> SearchArgs {
        SearchArgs {
            pattern: pattern.to_string(),
            paths,
            literal: false,
            regex: false,
            context: None,
            files_with_matches: false,
            count: false,
            invert_match: false,
            multiline: false,
            case_insensitive: false,
        }
    }

    #[test]
    fn literal_match_finds_expected() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.literal = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        assert_eq!(results.matches.len(), 2);
        assert!(results.matches.iter().all(|m| m.text.contains("Hello")));
    }

    #[test]
    fn regex_match_works() {
        let dir = make_test_dir();
        let args = make_args(r"Hello.*!", vec![dir.path().to_string_lossy().into_owned()]);
        let results = collect_matches(&args, &default_global()).unwrap();
        assert_eq!(results.matches.len(), 2);
        for m in &results.matches {
            assert!(m.text.contains("Hello"));
            assert!(m.text.contains("!"));
        }
    }

    #[test]
    fn no_matches_returns_exit_code_3() {
        let dir = make_test_dir();
        let args = make_args(
            "zzz_no_match_zzz",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        let code = run(args, &default_global()).unwrap();
        assert_eq!(code, exit::NO_MATCHES);
    }

    #[test]
    fn files_with_matches_output() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_with_matches = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        let output = format_results(&results, &args, &default_global()).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("hello.txt"));
        // Pure paths – no colon-separated fields
        assert!(!lines[0].contains(':'));
    }

    #[test]
    fn count_output() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        let output = format_results(&results, &args, &default_global()).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(":2"));
        assert!(lines[0].contains("hello.txt:"));
    }

    #[test]
    fn count_only_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        // count_only optimization: matches vec should be empty
        assert!(
            results.matches.is_empty(),
            "count mode should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 2);
    }

    #[test]
    fn files_with_matches_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.files_with_matches = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        assert!(
            results.matches.is_empty(),
            "files_with_matches should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.len(), 1);
    }

    #[test]
    fn multiline_count_skips_match_construction() {
        let dir = make_test_dir();
        let mut args = make_args(
            r"fn main\(\).*\}",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        args.multiline = true;
        args.count = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        assert!(
            results.matches.is_empty(),
            "multiline count should not build SearchMatch objects"
        );
        assert_eq!(results.file_match_counts.values().sum::<usize>(), 1);
    }

    #[test]
    fn invert_match_excludes_matching_lines() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        // hello.txt has 3 lines, 2 match "Hello", so 1 non-matching line.
        // code.rs has 3 lines, 0 match "Hello" (case-sensitive), so 3 non-matching.
        // Total: 4 non-matching lines.
        assert_eq!(results.matches.len(), 4);
        for m in &results.matches {
            assert!(
                !m.text.contains("Hello"),
                "inverted match should not contain 'Hello': {}",
                m.text
            );
        }
    }

    #[test]
    fn multiline_match_spans_lines() {
        let dir = make_test_dir();
        let mut args = make_args(
            r"fn main\(\).*\}",
            vec![dir.path().to_string_lossy().into_owned()],
        );
        args.multiline = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        assert_eq!(results.matches.len(), 1);
        let m = &results.matches[0];
        assert!(
            m.path.ends_with("code.rs"),
            "should match code.rs: {}",
            m.path
        );
        assert!(m.text.contains("fn main()"), "text: {}", m.text);
        assert!(
            m.text.contains('}'),
            "text should span to closing brace: {}",
            m.text
        );
        assert_eq!(m.line, 1, "match starts on line 1");
    }

    #[test]
    fn invert_match_multiline_is_rejected() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.invert_match = true;
        args.multiline = true;
        let err = run(args, &default_global()).unwrap_err();
        assert!(
            err.to_string().contains("--invert-match and --multiline"),
            "should reject incompatible flags: {}",
            err
        );
    }

    #[test]
    fn context_lines_included_in_output() {
        let dir = make_test_dir();
        let mut args = make_args("test file", vec![dir.path().to_string_lossy().into_owned()]);
        args.context = Some(1);
        let results = collect_matches(&args, &default_global()).unwrap();
        assert_eq!(results.matches.len(), 1);
        let m = &results.matches[0];
        assert!(m.context_before.is_some(), "should have context_before");
        assert!(m.context_after.is_some(), "should have context_after");
        let before = m.context_before.as_ref().unwrap();
        assert_eq!(before.len(), 1);
        assert!(
            before[0].contains("Hello, world!"),
            "context before: {:?}",
            before
        );
        let after = m.context_after.as_ref().unwrap();
        assert_eq!(after.len(), 1);
        assert!(
            after[0].contains("Hello again!"),
            "context after: {:?}",
            after
        );
    }

    #[test]
    fn json_output_values() {
        let dir = make_test_dir();
        let args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        let mut global = default_global();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(&results, &args, &global).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        // "Hello" appears in two lines of hello.txt, one file.
        assert_eq!(v["match_count"], serde_json::json!(2));
        assert_eq!(v["file_count"], serde_json::json!(1));
        let matches = v["matches"].as_array().expect("matches is array");
        assert_eq!(matches.len(), 2);
        for m in matches {
            let path = m["path"].as_str().unwrap();
            assert!(path.ends_with("hello.txt"), "unexpected path: {path}");
            assert!(m["line"].as_u64().unwrap() > 0);
            assert!(m["column"].as_u64().unwrap() > 0);
            let text = m["text"].as_str().unwrap();
            assert!(text.contains("Hello"), "match text missing 'Hello': {text}");
        }
    }
}
