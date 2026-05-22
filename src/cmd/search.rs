use crate::cli::global::GlobalFlags;
use crate::exit;
use anyhow::bail;
use clap::Args;
use memchr::memchr_iter;
use memchr::memmem;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom search 'TODO' src/
  patchloom search 'fn main' src/ --count
  patchloom search 'error|warn' --regex src/ -C 2
  patchloom search 'password' . --glob '*.yaml'")]
pub struct SearchArgs {
    /// Pattern to search for.
    pub pattern: String,
    /// Paths to search in.
    pub paths: Vec<String>,
    /// Treat pattern as a literal string.
    #[arg(long, short = 'F')]
    pub literal: bool,
    /// Treat pattern as a regex (default).
    #[arg(long)]
    pub regex: bool,
    /// Lines of context around matches (shorthand for -B N -A N).
    #[arg(long, short = 'C')]
    pub context: Option<usize>,
    /// Lines of context before each match.
    #[arg(long, short = 'B')]
    pub before_context: Option<usize>,
    /// Lines of context after each match.
    #[arg(long, short = 'A')]
    pub after_context: Option<usize>,
    // ref:search-mode:files-with-matches
    /// Only print file paths with matches.
    #[arg(long, short = 'l')]
    pub files_with_matches: bool,
    // ref:search-mode:count
    /// Only print match counts per file.
    #[arg(long, short = 'c')]
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
    // ref:search-mode:assert-count
    /// Assert that the total match count equals N. Exits 0 if exact, 2 otherwise.
    #[arg(long)]
    pub assert_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct SearchMatch {
    #[serde(serialize_with = "serialize_arc_str")]
    path: Arc<str>,
    line: usize,
    column: usize,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_after: Option<Vec<String>>,
}

fn serialize_arc_str<S: serde::Serializer>(s: &Arc<str>, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(s)
}

#[derive(Debug, Serialize)]
struct SearchOutput {
    ok: bool,
    matches: Vec<SearchMatch>,
    match_count: usize,
    file_count: usize,
}

#[derive(Debug, Serialize)]
struct SearchAssertCount {
    expected: usize,
    actual: usize,
    matched: bool,
}

#[derive(Debug, Serialize)]
struct SearchAssertCountOutput {
    ok: bool,
    status: &'static str,
    assert_count: SearchAssertCount,
}

struct SearchResults {
    matches: Vec<SearchMatch>,
    file_match_counts: BTreeMap<Arc<str>, usize>,
}

/// Matcher abstraction: either a compiled regex or a memchr literal finder.
enum Matcher {
    Regex(Regex),
    Literal(Box<memmem::Finder<'static>>),
}

impl Matcher {
    /// Find the first match in `text`, returning (start, end) byte offsets.
    fn find(&self, text: &str) -> Option<(usize, usize)> {
        match self {
            Matcher::Regex(re) => re.find(text).map(|m| (m.start(), m.end())),
            Matcher::Literal(finder) => {
                let start = finder.find(text.as_bytes())?;
                Some((start, start + finder.needle().len()))
            }
        }
    }

    /// Iterate all matches in `text` (for multiline mode).
    fn find_iter_positions(&self, text: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Regex(re) => re.find_iter(text).map(|m| (m.start(), m.end())).collect(),
            Matcher::Literal(finder) => {
                let bytes = text.as_bytes();
                let mut positions = Vec::new();
                let needle_len = finder.needle().len();
                let mut start = 0;
                while let Some(pos) = finder.find(&bytes[start..]) {
                    positions.push((start + pos, start + pos + needle_len));
                    start += pos + needle_len;
                }
                positions
            }
        }
    }
}

/// Build the right matcher for the search arguments.
fn build_matcher(args: &SearchArgs) -> anyhow::Result<Matcher> {
    // Use memchr for literal, case-sensitive, non-multiline searches.
    if args.literal && !args.case_insensitive && !args.multiline {
        return Ok(Matcher::Literal(Box::new(
            memmem::Finder::new(args.pattern.as_bytes()).into_owned(),
        )));
    }

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
    Ok(Matcher::Regex(re))
}

/// Per-file search result collected from parallel threads.
struct FileResult {
    path_str: Arc<str>,
    matches: Vec<SearchMatch>,
    count: usize,
}

fn line_and_column_for_offset(newline_offsets: &[usize], start: usize) -> (usize, usize) {
    let line_index = newline_offsets.partition_point(|&offset| offset < start);
    let line_start = if line_index == 0 {
        0
    } else {
        newline_offsets[line_index - 1] + 1
    };
    (line_index + 1, start - line_start + 1)
}

/// Search a single file and return matches/counts.
fn search_one_file(
    path: &std::path::Path,
    matcher: &Matcher,
    args: &SearchArgs,
    quiet: bool,
    cwd: &std::path::Path,
) -> Option<FileResult> {
    let content = crate::read_text_file(path, "search", quiet)?;

    let count_only = args.count || args.files_with_matches;
    let display = crate::files::relative_display(path, cwd);
    let path_str: Arc<str> = Arc::from(display.to_string_lossy().as_ref());
    let mut file_matches: Vec<SearchMatch> = Vec::new();
    let mut count = 0usize;

    if args.multiline {
        let newline_offsets: Vec<usize> = memchr_iter(b'\n', content.as_bytes()).collect();
        for (start, end) in matcher.find_iter_positions(&content) {
            count += 1;
            if count_only {
                if args.files_with_matches && args.assert_count.is_none() {
                    break;
                }
                continue;
            }
            let (line, column) = line_and_column_for_offset(&newline_offsets, start);
            file_matches.push(SearchMatch {
                path: path_str.clone(),
                line,
                column,
                text: content[start..end].to_string(),
                context_before: None,
                context_after: None,
            });
        }
    } else if count_only {
        // Fast path: no need to collect lines into a Vec for random access.
        for line in content.lines() {
            let found = matcher.find(line);
            let is_match = if args.invert_match {
                found.is_none()
            } else {
                found.is_some()
            };
            if is_match {
                count += 1;
                if args.files_with_matches && args.assert_count.is_none() {
                    break;
                }
            }
        }
    } else {
        let ctx_before = args.before_context.or(args.context).unwrap_or(0);
        let ctx_after = args.after_context.or(args.context).unwrap_or(0);
        let has_ctx = ctx_before > 0 || ctx_after > 0;
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().copied().enumerate() {
            let found = matcher.find(line);
            let is_match = if args.invert_match {
                found.is_none()
            } else {
                found.is_some()
            };
            if !is_match {
                continue;
            }
            count += 1;
            let column = found.map_or(1, |(s, _)| s + 1);
            let (ctx_b, ctx_a) = if has_ctx {
                let start = i.saturating_sub(ctx_before);
                let end = (i + 1 + ctx_after).min(lines.len());
                (
                    Some(lines[start..i].iter().map(|s| s.to_string()).collect()),
                    Some(lines[i + 1..end].iter().map(|s| s.to_string()).collect()),
                )
            } else {
                (None, None)
            };
            file_matches.push(SearchMatch {
                path: path_str.clone(),
                line: i + 1,
                column,
                text: line.to_string(),
                context_before: ctx_b,
                context_after: ctx_a,
            });
        }
    }

    if count > 0 {
        Some(FileResult {
            path_str,
            matches: file_matches,
            count,
        })
    } else {
        None
    }
}

fn collect_matches(args: &SearchArgs, global: &GlobalFlags) -> anyhow::Result<SearchResults> {
    let cwd = global.resolve_cwd()?;
    let glob_matcher = crate::build_glob_matcher(global)?;
    let matcher = build_matcher(args)?;
    let file_paths = crate::collect_file_paths_opts(&args.paths, global, false, Some(&cwd))?;
    let glob_roots = crate::collect_glob_roots(&args.paths, global, Some(&cwd))?;
    let count_only = args.count || args.files_with_matches;

    // Process files in parallel (#167).
    let cwd_ref = &cwd;
    let file_results: Vec<FileResult> =
        crate::par_process_files(&file_paths, glob_matcher.as_ref(), &glob_roots, |path| {
            search_one_file(path, &matcher, args, global.quiet, cwd_ref)
        });

    // Merge parallel results.
    let total_matches: usize = file_results.iter().map(|fr| fr.matches.len()).sum();
    let mut all_matches: Vec<SearchMatch> = Vec::with_capacity(total_matches);
    let mut file_match_counts: BTreeMap<Arc<str>, usize> = BTreeMap::new();
    for fr in file_results {
        file_match_counts.insert(fr.path_str, fr.count);
        all_matches.extend(fr.matches);
    }

    if !count_only {
        all_matches.sort_unstable_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
    }

    Ok(SearchResults {
        matches: all_matches,
        file_match_counts,
    })
}

fn format_results(
    results: SearchResults,
    args: &SearchArgs,
    global: &GlobalFlags,
) -> anyhow::Result<String> {
    use std::fmt::Write;
    let mut out = String::new();

    if global.json {
        let payload = SearchOutput {
            ok: true,
            match_count: results.file_match_counts.values().sum(),
            file_count: results.file_match_counts.len(),
            matches: results.matches,
        };
        out = serde_json::to_string_pretty(&payload)?;
        out.push('\n');
    } else if global.jsonl {
        if args.files_with_matches {
            // count_only mode: matches is empty, emit one line per file.
            for path in results.file_match_counts.keys() {
                out.push_str(&serde_json::to_string(
                    &serde_json::json!({"path": &**path}),
                )?);
                out.push('\n');
            }
        } else if args.count {
            for (path, count) in &results.file_match_counts {
                out.push_str(&serde_json::to_string(
                    &serde_json::json!({"path": &**path, "count": count}),
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
        let color = global.should_color();
        for path in results.file_match_counts.keys() {
            if color {
                let _ = writeln!(out, "\x1b[35m{path}\x1b[0m");
            } else {
                out.push_str(path);
                out.push('\n');
            }
        }
    } else if args.count {
        let color = global.should_color();
        for (path, count) in &results.file_match_counts {
            if color {
                let _ = writeln!(out, "\x1b[35m{path}\x1b[0m:{count}");
            } else {
                let _ = writeln!(out, "{path}:{count}");
            }
        }
    } else {
        let color = global.should_color();
        let (path_c, sep_c, ln_c, reset) = if color {
            ("\x1b[35m", "\x1b[36m", "\x1b[32m", "\x1b[0m")
        } else {
            ("", "", "", "")
        };
        let has_ctx =
            args.context.is_some() || args.before_context.is_some() || args.after_context.is_some();
        let mut first = true;
        for m in &results.matches {
            if has_ctx && !first {
                out.push_str("--\n");
            }
            first = false;
            if let Some(ref before) = m.context_before {
                for (j, ctx) in before.iter().enumerate() {
                    let ln = m.line - before.len() + j;
                    let _ = writeln!(
                        out,
                        "{path_c}{}{reset}{sep_c}-{reset}{ln_c}{ln}{reset}{sep_c}-{reset}{ctx}",
                        m.path
                    );
                }
            }
            let _ = writeln!(
                out,
                "{path_c}{}{reset}{sep_c}:{reset}{ln_c}{}{reset}{sep_c}:{reset}{}{sep_c}:{reset}{}",
                m.path, m.line, m.column, m.text
            );
            if let Some(ref after) = m.context_after {
                for (j, ctx) in after.iter().enumerate() {
                    let ln = m.line + 1 + j;
                    let _ = writeln!(
                        out,
                        "{path_c}{}{reset}{sep_c}-{reset}{ln_c}{ln}{reset}{sep_c}-{reset}{ctx}",
                        m.path
                    );
                }
            }
        }
    }

    Ok(out)
}

pub fn run(args: SearchArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    if args.invert_match && args.multiline {
        bail!("--invert-match and --multiline cannot be combined");
    }

    let results = collect_matches(&args, global)?;

    // --assert-count mode: succeed only if total count equals N.
    if let Some(expected) = args.assert_count {
        let actual: usize = results.file_match_counts.values().sum();
        let matches = actual == expected;
        let status = if matches {
            "success"
        } else {
            "changes_detected"
        };
        let code = if matches {
            exit::SUCCESS
        } else {
            exit::CHANGES_DETECTED
        };
        if global.emit_json(&SearchAssertCountOutput {
            ok: true,
            status,
            assert_count: SearchAssertCount {
                expected,
                actual,
                matched: matches,
            },
        })? {
            return Ok(code);
        }
        if !global.quiet {
            if matches {
                eprintln!("assert-count: {actual} matches (expected {expected})");
            } else {
                eprintln!("assert-count: expected {expected} matches, found {actual}");
            }
        }
        return Ok(code);
    }

    let has_matches = if args.count || args.files_with_matches {
        !results.file_match_counts.is_empty()
    } else {
        !results.matches.is_empty()
    };
    if !has_matches {
        if global.show_status() {
            let paths: Vec<&str> = args.paths.iter().map(|s| s.as_str()).collect();
            let path_desc = if paths.is_empty() {
                ".".to_string()
            } else {
                paths.join(", ")
            };
            eprintln!("no matches for '{}' in {path_desc}", args.pattern);
            if args.literal && crate::files::has_regex_metacharacters(&args.pattern) {
                eprintln!(
                    "hint: --literal is set but the pattern looks like a regex, try removing --literal"
                );
            }
            if !args.case_insensitive {
                eprintln!("hint: try -i for case-insensitive matching");
            }
        }
        return Ok(exit::NO_MATCHES);
    }

    if !global.quiet {
        let output = format_results(results, &args, global)?;
        print!("{output}");
    }

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            before_context: None,
            after_context: None,
            files_with_matches: false,
            count: false,
            invert_match: false,
            multiline: false,
            case_insensitive: false,
            assert_count: None,
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
        let output = format_results(results, &args, &default_global()).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("hello.txt"));
        // Pure paths – no colon-separated match fields after the filename
        assert!(
            lines[0].ends_with("hello.txt"),
            "expected bare path, got: {}",
            lines[0]
        );
    }

    #[test]
    fn count_output() {
        let dir = make_test_dir();
        let mut args = make_args("Hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.count = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        let output = format_results(results, &args, &default_global()).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(":2"));
        assert!(lines[0].contains("hello.txt:"));
    }

    #[test]
    fn line_and_column_for_offset_maps_multiline_positions() {
        let newline_offsets = vec![2, 5];
        assert_eq!(line_and_column_for_offset(&newline_offsets, 0), (1, 1));
        assert_eq!(line_and_column_for_offset(&newline_offsets, 2), (1, 3));
        assert_eq!(line_and_column_for_offset(&newline_offsets, 3), (2, 1));
        assert_eq!(line_and_column_for_offset(&newline_offsets, 6), (3, 1));
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
        assert_eq!(m.column, 1, "match starts at column 1");
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
        let output = format_results(results, &args, &global).unwrap();
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

    #[test]
    fn case_insensitive_finds_all_cases() {
        let dir = make_test_dir();
        // hello.txt has "Hello" (uppercase H); code.rs has "hello" (lowercase).
        let mut args = make_args("hello", vec![dir.path().to_string_lossy().into_owned()]);
        args.case_insensitive = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        // hello.txt: 2 lines with "Hello", code.rs: 1 line with "hello" => 3
        assert_eq!(results.matches.len(), 3);
    }

    #[test]
    fn case_insensitive_literal_escapes_regex_chars() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("special.txt"), "foo(BAR)\nbaz\n").unwrap();

        let mut args = make_args("foo(bar)", vec![dir.path().to_string_lossy().into_owned()]);
        args.literal = true;
        args.case_insensitive = true;
        let results = collect_matches(&args, &default_global()).unwrap();
        assert_eq!(results.matches.len(), 1);
        assert!(results.matches[0].text.contains("foo(BAR)"));
    }
}
