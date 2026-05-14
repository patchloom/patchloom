use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;
use globset::Glob;
use ignore::WalkBuilder;
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
    let glob_matcher = global
        .glob
        .as_ref()
        .map(|p| Glob::new(p).map(|g| g.compile_matcher()))
        .transpose()?;

    let re = if args.literal {
        Regex::new(&regex::escape(&args.pattern))?
    } else {
        Regex::new(&args.pattern)?
    };

    let paths: Vec<&str> = if args.paths.is_empty() {
        vec!["."]
    } else {
        args.paths.iter().map(String::as_str).collect()
    };

    let mut builder = WalkBuilder::new(paths[0]);
    for path in &paths[1..] {
        builder.add(path);
    }

    let mut all_matches: Vec<SearchMatch> = Vec::new();
    let mut file_match_counts: BTreeMap<String, usize> = BTreeMap::new();

    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_type().is_none_or(|ft| !ft.is_file()) {
            continue;
        }

        let path = entry.path();

        if let Some(ref matcher) = glob_matcher {
            if !matcher.is_match(path) && !path.file_name().is_some_and(|n| matcher.is_match(n)) {
                continue;
            }
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        let path_str = path.to_string_lossy().to_string();
        let mut count = 0usize;

        for (i, line) in lines.iter().enumerate() {
            if let Some(m) = re.find(line) {
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
                    column: m.start() + 1,
                    text: line.to_string(),
                    context_before,
                    context_after,
                });
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

    let results = collect_matches(&args, global)?;

    if results.matches.is_empty() {
        return Ok(exit::NO_MATCHES);
    }

    let output = format_results(&results, &args, global)?;
    print!("{output}");

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
        GlobalFlags {
            json: false,
            jsonl: false,
            diff: false,
            apply: false,
            check: false,
            cwd: None,
            glob: None,
            atomic: false,
            ensure_final_newline: false,
            normalize_eol: None,
            trim_trailing_whitespace: false,
        }
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
    fn json_output_shape() {
        let dir = make_test_dir();
        let args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        let mut global = default_global();
        global.json = true;
        let results = collect_matches(&args, &global).unwrap();
        let output = format_results(&results, &args, &global).unwrap();
        let v: serde_json::Value = serde_json::from_str(&output).expect("valid JSON");
        assert_eq!(v["ok"], serde_json::json!(true));
        assert!(v["matches"].is_array());
        assert!(v["match_count"].as_u64().unwrap() > 0);
        assert!(v["file_count"].as_u64().unwrap() > 0);
        let first = &v["matches"][0];
        assert!(first["path"].is_string());
        assert!(first["line"].is_number());
        assert!(first["column"].is_number());
        assert!(first["text"].is_string());
    }
}
