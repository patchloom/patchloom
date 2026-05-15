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
    /// Only print file paths with matches.
    #[arg(long)]
    pub files_with_matches: bool,
    /// Only print match counts per file.
    #[arg(long)]
    pub count: bool,
    /// Show lines that do NOT match the pattern.
    #[arg(long, short = 'v')]
    pub invert_match: bool,
    /// Enable multiline matching (dot matches newlines).
    #[arg(long, short = 'U')]
    pub multiline: bool,
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
    let re = if args.multiline {
        regex::RegexBuilder::new(&pattern)
            .dot_matches_new_line(true)
            .build()?
    } else {
        Regex::new(&pattern)?
    };

    let mut all_matches: Vec<SearchMatch> = Vec::new();
    let mut file_match_counts: BTreeMap<String, usize> = BTreeMap::new();

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

        let path_str = path.to_string_lossy().to_string();
        let mut count = 0usize;

        if args.multiline {
            // Multiline mode: match against the full file content.
            for m in re.find_iter(&content) {
                count += 1;
                let line_num = content[..m.start()].matches('\n').count() + 1;
                let col = m.start() - content[..m.start()].rfind('\n').map_or(0, |pos| pos + 1) + 1;
                all_matches.push(SearchMatch {
                    path: path_str.clone(),
                    line: line_num,
                    column: col,
                    text: m.as_str().to_string(),
                    context_before: None,
                    context_after: None,
                });
            }
        } else {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let found = re.find(line);
                let is_match = if args.invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };

                if is_match {
                    count += 1;
                    let context_before = args.context.map(|n| {
                        let start = i.saturating_sub(n);
                        lines[start..i].iter().map(|s| s.to_string()).collect()
                    });
                    let context_after = args.context.map(|n| {
                        let end = (i + 1 + n).min(lines.len());
                        lines[i + 1..end].iter().map(|s| s.to_string()).collect()
                    });

                    all_matches.push(SearchMatch {
                        path: path_str.clone(),
                        line: i + 1,
                        column: found.map_or(1, |m| m.start() + 1),
                        text: line.to_string(),
                        context_before,
                        context_after,
                    });
                }
            }
        }

        if count > 0 {
            file_match_counts.insert(path_str, count);
        }
    }

    all_matches.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));

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
        for m in &results.matches {
            out.push_str(&serde_json::to_string(m)?);
            out.push('\n');
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
    if let Some(ref cwd) = global.cwd {
        std::env::set_current_dir(cwd)?;
    }

    if args.invert_match && args.multiline {
        bail!("--invert-match and --multiline cannot be combined");
    }

    let results = collect_matches(&args, global)?;

    if results.matches.is_empty() {
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
