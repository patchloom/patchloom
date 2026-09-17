use memchr::memmem;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// A single search match with location and context.
#[derive(Debug, Serialize)]
pub struct SearchMatch {
    #[serde(serialize_with = "serialize_arc_str")]
    pub path: Arc<str>,
    /// 1-based line number of the match.
    pub line: usize,
    /// 1-based byte offset from the start of the line to the match start.
    pub column: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_after: Option<Vec<String>>,
}

fn serialize_arc_str<S: serde::Serializer>(s: &Arc<str>, ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(s)
}

/// Aggregated search results across multiple files.
pub struct SearchResults {
    pub matches: Vec<SearchMatch>,
    pub file_match_counts: BTreeMap<Arc<str>, usize>,
}

impl SearchResults {
    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }
}

/// Matcher abstraction: either a compiled regex or a memchr literal finder.
pub enum Matcher {
    Regex(Regex),
    Literal(Box<memmem::Finder<'static>>),
}

impl Matcher {
    /// Find the first match in `text`, returning (start, end) byte offsets.
    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        match self {
            Matcher::Regex(re) => re.find(text).map(|m| (m.start(), m.end())),
            Matcher::Literal(finder) => {
                let start = finder.find(text.as_bytes())?;
                Some((start, start + finder.needle().len()))
            }
        }
    }

    /// Iterate all matches in `text` (for multiline mode).
    pub fn find_iter_positions(&self, text: &str) -> Vec<(usize, usize)> {
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

    /// Count matches in `text`, optionally stopping after the first match.
    pub fn count_matches(&self, text: &str, stop_after_first: bool) -> usize {
        match self {
            Matcher::Regex(re) => {
                if stop_after_first {
                    usize::from(re.find(text).is_some())
                } else {
                    re.find_iter(text).count()
                }
            }
            Matcher::Literal(finder) => {
                let bytes = text.as_bytes();
                if stop_after_first {
                    return usize::from(finder.find(bytes).is_some());
                }
                let needle_len = finder.needle().len();
                let mut count = 0;
                let mut start = 0;
                while let Some(pos) = finder.find(&bytes[start..]) {
                    count += 1;
                    start += pos + needle_len;
                }
                count
            }
        }
    }
}

/// Build the right matcher for the given search parameters.
pub fn build_matcher(
    pattern: &str,
    literal: bool,
    case_insensitive: bool,
    multiline: bool,
) -> anyhow::Result<Matcher> {
    // Use memchr for literal, case-sensitive, non-multiline searches.
    if literal && !case_insensitive && !multiline {
        return Ok(Matcher::Literal(Box::new(
            memmem::Finder::new(pattern.as_bytes()).into_owned(),
        )));
    }

    let escaped = if literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let re = if multiline || case_insensitive {
        crate::bounded_regex_build(
            crate::bounded_regex_builder(&escaped)
                .multi_line(true)
                .dot_matches_new_line(multiline)
                .case_insensitive(case_insensitive),
        )?
    } else {
        crate::bounded_regex_build(crate::bounded_regex_builder(&escaped).multi_line(true))?
    };
    Ok(Matcher::Regex(re))
}

/// Per-file search result collected from parallel threads.
pub struct FileResult {
    pub path_str: Arc<str>,
    pub matches: Vec<SearchMatch>,
    pub count: usize,
}

/// Parameters for searching a single file (avoids too-many-arguments).
pub struct SearchFileParams {
    pub multiline: bool,
    pub invert_match: bool,
    pub count_only: bool,
    pub files_with_matches: bool,
    pub files_without_match: bool,
    pub assert_count: Option<usize>,
    pub before_context: Option<usize>,
    pub after_context: Option<usize>,
    pub context: Option<usize>,
    pub quiet: bool,
    /// Cap detailed `matches` for this file. `count` stays exact.
    /// `None` or `Some(0)` means no per-file cap.
    pub max_results: Option<usize>,
}

/// Stop allocating detailed match objects once this file already has
/// `max_results` of them. `count` still increments so merge/assert stay exact.
fn skip_match_detail(params: &SearchFileParams, collected: usize) -> bool {
    match params.max_results {
        Some(cap) if cap > 0 => collected >= cap,
        _ => false,
    }
}

/// Path-only modes can stop after the first hit (membership is enough).
/// `assert_count` still needs a full occurrence total.
fn stop_after_first_hit(params: &SearchFileParams) -> bool {
    (params.files_with_matches || params.files_without_match) && params.assert_count.is_none()
}

/// Whole-buffer memmem is equivalent to the per-line loop only when the
/// needle cannot contain a line end. A `\n` / `\r` needle is a hit in the
/// raw file and never in `text_lines` (newlines are stripped).
fn literal_needle_is_line_safe(matcher: &Matcher) -> bool {
    match matcher {
        Matcher::Literal(finder) => {
            let needle = finder.needle();
            !needle.contains(&b'\n') && !needle.contains(&b'\r')
        }
        Matcher::Regex(_) => false,
    }
}

/// Cheap membership probe so zero-match files skip `Arc` + line split (#2550).
///
/// Literal: one memmem over the buffer when the needle is line-safe.
/// Regex: scan lines without collecting them (`$` on a CR-only line is
/// line-oriented, not a whole-buffer `$`).
fn forward_buffer_has_hit(matcher: &Matcher, content: &str) -> bool {
    match matcher {
        Matcher::Literal(_) if literal_needle_is_line_safe(matcher) => {
            matcher.find(content).is_some()
        }
        Matcher::Literal(_) => false,
        Matcher::Regex(_) => {
            crate::ops::file::text_lines(content).any(|line| matcher.find(line).is_some())
        }
    }
}

fn search_display_path(path: &std::path::Path, cwd: &std::path::Path) -> Arc<str> {
    #[cfg(any(feature = "cli", feature = "files"))]
    let display = crate::files::relative_display(path, cwd);
    #[cfg(not(any(feature = "cli", feature = "files")))]
    let display = path.strip_prefix(cwd).unwrap_or(path);
    Arc::from(display.to_string_lossy().as_ref())
}

fn finish_file_result(
    params: &SearchFileParams,
    path_str: Arc<str>,
    matches: Vec<SearchMatch>,
    count: usize,
) -> Option<FileResult> {
    if params.files_without_match {
        if count == 0 {
            Some(FileResult {
                path_str,
                matches,
                count: 0,
            })
        } else {
            None
        }
    } else if count > 0 {
        Some(FileResult {
            path_str,
            matches,
            count,
        })
    } else {
        None
    }
}

/// Compute a 1-based (line, column) pair from a byte offset into content.
///
/// `newline_offsets` is a list of byte positions where `\n` appears.
/// `start` is the byte offset of the match. Returns a 1-based line number
/// and a 1-based byte column within that line.
pub fn line_and_column_for_offset(newline_offsets: &[usize], start: usize) -> (usize, usize) {
    let line_index = newline_offsets.partition_point(|&offset| offset < start);
    let line_start = if line_index == 0 {
        0
    } else {
        newline_offsets[line_index - 1] + 1
    };
    (line_index + 1, start - line_start + 1)
}

/// Search a single file and return matches/counts.
pub fn search_one_file(
    path: &std::path::Path,
    matcher: &Matcher,
    params: &SearchFileParams,
    cwd: &std::path::Path,
) -> Option<FileResult> {
    #[cfg(feature = "cli")]
    let content = crate::files::read_text_file_logged(path, "search", params.quiet)?;
    #[cfg(not(feature = "cli"))]
    let content = crate::files::read_text_file(path)?;
    // Notepad/VS UTF-8 BOM is not line content. Strip so `^end` matches
    // the first line the same way md/doc already strip for parse (#2311).
    let content = crate::ops::file::strip_utf8_bom(&content);

    if params.multiline {
        let path_str = search_display_path(path, cwd);
        let mut file_matches: Vec<SearchMatch> = Vec::new();
        let mut count = 0usize;
        if params.count_only {
            count = matcher.count_matches(content, stop_after_first_hit(params));
        } else {
            for (start, end) in matcher.find_iter_positions(content) {
                count += 1;
                if skip_match_detail(params, file_matches.len()) {
                    continue;
                }
                let (line, column) = crate::ops::file::text_line_column(content, start);
                file_matches.push(SearchMatch {
                    path: path_str.clone(),
                    line,
                    column,
                    text: content[start..end].to_string(),
                    context_before: None,
                    context_after: None,
                });
            }
        }
        return finish_file_result(params, path_str, file_matches, count);
    }

    if params.invert_match {
        return search_one_file_invert(path, matcher, params, cwd, content);
    }

    if params.count_only {
        let count = match matcher {
            // Literal: one memmem over the file, not per line (#2550).
            // A CR/LF needle is never a line-oriented hit (same as listing).
            Matcher::Literal(_) if literal_needle_is_line_safe(matcher) => {
                matcher.count_matches(content, stop_after_first_hit(params))
            }
            Matcher::Literal(_) => 0,
            Matcher::Regex(_) => {
                let mut count = 0usize;
                for line in crate::ops::file::text_lines(content) {
                    let n = matcher.count_matches(line, stop_after_first_hit(params));
                    if n > 0 {
                        count += n;
                        if stop_after_first_hit(params) {
                            break;
                        }
                    }
                }
                count
            }
        };
        if count == 0 && !params.files_without_match {
            return None;
        }
        return finish_file_result(params, search_display_path(path, cwd), Vec::new(), count);
    }

    if !forward_buffer_has_hit(matcher, content) {
        if params.files_without_match {
            return finish_file_result(params, search_display_path(path, cwd), Vec::new(), 0);
        }
        return None;
    }
    if params.files_without_match {
        return None;
    }
    if stop_after_first_hit(params) {
        return Some(FileResult {
            path_str: search_display_path(path, cwd),
            matches: Vec::new(),
            count: 1,
        });
    }

    let path_str = search_display_path(path, cwd);
    let mut file_matches: Vec<SearchMatch> = Vec::new();
    let mut count = 0usize;
    let ctx_before = params.before_context.or(params.context).unwrap_or(0);
    let ctx_after = params.after_context.or(params.context).unwrap_or(0);
    let has_ctx = ctx_before > 0 || ctx_after > 0;
    let lines: Vec<&str> = crate::ops::file::text_lines(content).collect();

    for (i, line) in lines.iter().copied().enumerate() {
        // All non-overlapping occurrences on the line (parity with replace).
        for (start_b, _end_b) in matcher.find_iter_positions(line) {
            count += 1;
            if skip_match_detail(params, file_matches.len()) {
                continue;
            }
            let column = start_b + 1;
            let ctx_start = i.saturating_sub(ctx_before);
            let ctx_end = (i + 1 + ctx_after).min(lines.len());
            file_matches.push(SearchMatch {
                path: path_str.clone(),
                line: i + 1,
                column,
                text: line.to_string(),
                context_before: if has_ctx {
                    Some(lines[ctx_start..i].iter().map(|s| s.to_string()).collect())
                } else {
                    None
                },
                context_after: if has_ctx {
                    Some(
                        lines[i + 1..ctx_end]
                            .iter()
                            .map(|s| s.to_string())
                            .collect(),
                    )
                } else {
                    None
                },
            });
        }
    }

    finish_file_result(params, path_str, file_matches, count)
}

fn search_one_file_invert(
    path: &std::path::Path,
    matcher: &Matcher,
    params: &SearchFileParams,
    cwd: &std::path::Path,
    content: &str,
) -> Option<FileResult> {
    let path_str = search_display_path(path, cwd);
    let mut file_matches: Vec<SearchMatch> = Vec::new();
    let mut count = 0usize;

    if params.count_only {
        for line in crate::ops::file::text_lines(content) {
            if matcher.find(line).is_none() {
                count += 1;
                if stop_after_first_hit(params) {
                    break;
                }
            }
        }
        return finish_file_result(params, path_str, file_matches, count);
    }

    let ctx_before = params.before_context.or(params.context).unwrap_or(0);
    let ctx_after = params.after_context.or(params.context).unwrap_or(0);
    let has_ctx = ctx_before > 0 || ctx_after > 0;
    let lines: Vec<&str> = crate::ops::file::text_lines(content).collect();

    for (i, line) in lines.iter().copied().enumerate() {
        if matcher.find(line).is_some() {
            continue;
        }
        count += 1;
        if skip_match_detail(params, file_matches.len()) {
            continue;
        }
        let start = i.saturating_sub(ctx_before);
        let end = (i + 1 + ctx_after).min(lines.len());
        file_matches.push(SearchMatch {
            path: path_str.clone(),
            line: i + 1,
            column: 1,
            text: line.to_string(),
            context_before: if has_ctx {
                Some(lines[start..i].iter().map(|s| s.to_string()).collect())
            } else {
                None
            },
            context_after: if has_ctx {
                Some(lines[i + 1..end].iter().map(|s| s.to_string()).collect())
            } else {
                None
            },
        });
    }

    finish_file_result(params, path_str, file_matches, count)
}

/// Merge per-file results into a single [`SearchResults`].
///
/// Sorts detailed matches by (path, line) and applies `max_results` cap.
/// This is the aggregation step after parallel file processing.
pub fn merge_file_results(
    file_results: Vec<FileResult>,
    count_only: bool,
    max_results: usize,
) -> SearchResults {
    let total_matches: usize = file_results.iter().map(|fr| fr.matches.len()).sum();
    let mut all_matches: Vec<SearchMatch> = Vec::with_capacity(total_matches);
    let mut file_match_counts: BTreeMap<Arc<str>, usize> = BTreeMap::new();
    for fr in file_results {
        file_match_counts.insert(fr.path_str, fr.count);
        all_matches.extend(fr.matches);
    }

    if !count_only {
        // Tie-break by column so same-line hits stay left-to-right. Agents map
        // search index → replace --nth; unstable (path, line) alone can scramble.
        all_matches.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.column.cmp(&b.column))
        });
    }

    if max_results > 0 && !count_only {
        all_matches.truncate(max_results);
    }

    SearchResults {
        matches: all_matches,
        file_match_counts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_matcher_literal_case_sensitive() {
        let m = build_matcher("hello", true, false, false).unwrap();
        assert!(matches!(m, Matcher::Literal(_)));
        assert!(
            m.find("say hello world").is_some(),
            "literal 'hello' should match in 'say hello world'"
        );
        assert!(
            m.find("say Hello world").is_none(),
            "literal 'hello' should NOT match 'Hello' (case-sensitive)"
        );
    }

    #[test]
    fn build_matcher_literal_case_insensitive_uses_regex() {
        let m = build_matcher("hello", true, true, false).unwrap();
        assert!(matches!(m, Matcher::Regex(_)));
        assert!(
            m.find("say Hello world").is_some(),
            "case-insensitive literal should match 'Hello'"
        );
    }

    #[test]
    fn build_matcher_regex_pattern() {
        let m = build_matcher(r"hel+o", false, false, false).unwrap();
        assert!(
            m.find("hello").is_some(),
            "regex 'hel+o' should match 'hello'"
        );
        assert!(
            m.find("helllo").is_some(),
            "regex 'hel+o' should match 'helllo' (l+ matches multiple)"
        );
        assert!(
            m.find("heo").is_none(),
            "regex 'hel+o' should NOT match 'heo' (l+ requires at least one l)"
        );
    }

    #[test]
    fn line_and_column_for_offset_basic() {
        let offsets = vec![5, 10];
        assert_eq!(line_and_column_for_offset(&offsets, 0), (1, 1));
        assert_eq!(line_and_column_for_offset(&offsets, 5), (1, 6));
        assert_eq!(line_and_column_for_offset(&offsets, 6), (2, 1));
        assert_eq!(line_and_column_for_offset(&offsets, 11), (3, 1));
    }

    #[test]
    fn merge_file_results_sorts_by_path_then_line() {
        let fr1 = FileResult {
            path_str: Arc::from("b.txt"),
            matches: vec![SearchMatch {
                path: Arc::from("b.txt"),
                line: 1,
                column: 1,
                text: "b1".into(),
                context_before: None,
                context_after: None,
            }],
            count: 1,
        };
        let fr2 = FileResult {
            path_str: Arc::from("a.txt"),
            matches: vec![SearchMatch {
                path: Arc::from("a.txt"),
                line: 1,
                column: 1,
                text: "a1".into(),
                context_before: None,
                context_after: None,
            }],
            count: 1,
        };
        let results = merge_file_results(vec![fr1, fr2], false, 0);
        assert_eq!(results.matches[0].text, "a1");
        assert_eq!(results.matches[1].text, "b1");
    }

    #[test]
    fn merge_file_results_sorts_same_line_by_column() {
        // Scrambled same-line hits must re-order left-to-right for --nth.
        let fr = FileResult {
            path_str: Arc::from("t.txt"),
            matches: vec![
                SearchMatch {
                    path: Arc::from("t.txt"),
                    line: 1,
                    column: 7,
                    text: "hi".into(),
                    context_before: None,
                    context_after: None,
                },
                SearchMatch {
                    path: Arc::from("t.txt"),
                    line: 1,
                    column: 1,
                    text: "hi".into(),
                    context_before: None,
                    context_after: None,
                },
                SearchMatch {
                    path: Arc::from("t.txt"),
                    line: 1,
                    column: 4,
                    text: "hi".into(),
                    context_before: None,
                    context_after: None,
                },
            ],
            count: 3,
        };
        let results = merge_file_results(vec![fr], false, 0);
        assert_eq!(
            results.matches.iter().map(|m| m.column).collect::<Vec<_>>(),
            vec![1, 4, 7]
        );
    }

    #[test]
    fn merge_file_results_applies_max_results() {
        let fr = FileResult {
            path_str: Arc::from("a.txt"),
            matches: vec![
                SearchMatch {
                    path: Arc::from("a.txt"),
                    line: 1,
                    column: 1,
                    text: "l1".into(),
                    context_before: None,
                    context_after: None,
                },
                SearchMatch {
                    path: Arc::from("a.txt"),
                    line: 2,
                    column: 1,
                    text: "l2".into(),
                    context_before: None,
                    context_after: None,
                },
            ],
            count: 2,
        };
        let results = merge_file_results(vec![fr], false, 1);
        assert_eq!(results.matches.len(), 1);
        // file_match_counts still reflects the full count
        assert_eq!(*results.file_match_counts.get("a.txt").unwrap(), 2);
    }

    #[test]
    fn search_one_file_finds_literal_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello world\nGoodbye world\nHello again\n").unwrap();
        let matcher = build_matcher("Hello", true, false, false).unwrap();
        let params = SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results: None,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(result.count, 2);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line, 1);
        assert_eq!(result.matches[1].line, 3);
    }

    #[test]
    fn search_one_file_regex_caret_matches_after_utf8_bom() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("bom.txt");
        std::fs::write(&file, "\u{feff}end\r\nnext\r\n").unwrap();
        let matcher = build_matcher("^end", false, false, false).unwrap();
        let params = SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results: None,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path())
            .expect("^end must match the first line after a UTF-8 BOM");
        assert_eq!(result.count, 1);
        assert_eq!(result.matches[0].line, 1);
        assert_eq!(result.matches[0].column, 1);
        assert_eq!(result.matches[0].text, "end");
    }

    #[test]
    fn search_one_file_regex_dollar_matches_cr_only_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("cr.txt");
        std::fs::write(&file, "end\rnext\r").unwrap();
        let matcher = build_matcher("end$", false, false, false).unwrap();
        let params = SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results: None,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path())
            .expect("end$ must match a CR-only line the same way replace does");
        assert_eq!(result.count, 1);
        assert_eq!(result.matches[0].line, 1);
        assert_eq!(result.matches[0].text, "end");
    }

    #[test]
    fn search_one_file_counts_all_occurrences_on_a_line() {
        // Align with replace match_count (fixrealloop dogfood).
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("multi.txt");
        std::fs::write(&file, "hi hi hi\n").unwrap();
        let matcher = build_matcher("hi", true, false, false).unwrap();
        let params = SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results: None,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(result.count, 3);
        assert_eq!(result.matches.len(), 3);
        assert_eq!(result.matches[0].column, 1);
        assert_eq!(result.matches[1].column, 4);
        assert_eq!(result.matches[2].column, 7);

        let count_params = SearchFileParams {
            count_only: true,
            ..params
        };
        let counted = search_one_file(&file, &matcher, &count_params, dir.path()).unwrap();
        assert_eq!(counted.count, 3);
        assert!(counted.matches.is_empty());
    }

    #[test]
    fn search_one_file_count_only_skips_matches() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello\nHello\n").unwrap();
        let matcher = build_matcher("Hello", true, false, false).unwrap();
        let params = SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: true,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results: None,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(result.count, 2);
        assert!(
            result.matches.is_empty(),
            "count_only should skip match objects"
        );
    }

    fn params_with_cap(max_results: Option<usize>) -> SearchFileParams {
        SearchFileParams {
            multiline: false,
            invert_match: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
            max_results,
        }
    }

    #[test]
    fn search_one_file_max_results_caps_matches_not_count() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("hits.txt");
        std::fs::write(&file, "hit\nhit\nhit\nhit\nhit\n").unwrap();
        let matcher = build_matcher("hit", true, false, false).unwrap();
        let result =
            search_one_file(&file, &matcher, &params_with_cap(Some(2)), dir.path()).unwrap();
        assert_eq!(result.count, 5);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line, 1);
        assert_eq!(result.matches[1].line, 2);
    }

    #[test]
    fn per_file_max_results_does_not_change_multi_file_merge() {
        // Global top-4 spans both files (a has 2, b has 5). Per-file cap of 4
        // must keep the same sorted prefix as an uncapped scan.
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "hit\nhit\n").unwrap();
        std::fs::write(&b, "hit\nhit\nhit\nhit\nhit\n").unwrap();
        let matcher = build_matcher("hit", true, false, false).unwrap();

        let uncapped = merge_file_results(
            vec![
                search_one_file(&a, &matcher, &params_with_cap(None), dir.path()).unwrap(),
                search_one_file(&b, &matcher, &params_with_cap(None), dir.path()).unwrap(),
            ],
            false,
            4,
        );
        let capped = merge_file_results(
            vec![
                search_one_file(&a, &matcher, &params_with_cap(Some(4)), dir.path()).unwrap(),
                search_one_file(&b, &matcher, &params_with_cap(Some(4)), dir.path()).unwrap(),
            ],
            false,
            4,
        );
        let keys = |r: &SearchResults| {
            r.matches
                .iter()
                .map(|m| (m.path.as_ref().to_string(), m.line, m.column))
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(&uncapped), keys(&capped));
        assert_eq!(uncapped.matches.len(), 4);
        assert_eq!(
            *capped.file_match_counts.get("a.txt").unwrap()
                + *capped.file_match_counts.get("b.txt").unwrap(),
            7
        );
        assert_eq!(capped.matches[0].path.as_ref(), "a.txt");
        assert_eq!(capped.matches[1].path.as_ref(), "a.txt");
        assert_eq!(capped.matches[2].path.as_ref(), "b.txt");
        assert_eq!(capped.matches[3].path.as_ref(), "b.txt");
    }

    #[test]
    fn search_one_file_count_only_and_assert_count_ignore_match_cap() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("hits.txt");
        std::fs::write(&file, "hit\nhit\nhit\n").unwrap();
        let matcher = build_matcher("hit", true, false, false).unwrap();

        let mut count_params = params_with_cap(Some(1));
        count_params.count_only = true;
        let counted = search_one_file(&file, &matcher, &count_params, dir.path()).unwrap();
        assert_eq!(counted.count, 3);
        assert!(counted.matches.is_empty());

        let mut assert_params = params_with_cap(Some(1));
        assert_params.assert_count = Some(3);
        let detailed = search_one_file(&file, &matcher, &assert_params, dir.path()).unwrap();
        assert_eq!(detailed.count, 3);
        assert_eq!(detailed.matches.len(), 1);
    }

    #[test]
    fn search_one_file_zero_match_literal_is_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("miss.txt");
        std::fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();
        let matcher = build_matcher("zzz", true, false, false).unwrap();
        assert!(
            search_one_file(&file, &matcher, &params_with_cap(None), dir.path()).is_none(),
            "zero-match search must stay no_matches (no FileResult)"
        );

        let mut count_params = params_with_cap(None);
        count_params.count_only = true;
        assert!(
            search_one_file(&file, &matcher, &count_params, dir.path()).is_none(),
            "count-only zero-match must stay no_matches"
        );
    }

    #[test]
    fn search_one_file_files_without_match_keeps_zero_hit() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("miss.txt");
        std::fs::write(&file, "alpha\nbeta\n").unwrap();
        let matcher = build_matcher("zzz", true, false, false).unwrap();
        let mut params = params_with_cap(None);
        params.files_without_match = true;
        let result = search_one_file(&file, &matcher, &params, dir.path())
            .expect("-L must list a zero-hit file");
        assert_eq!(result.count, 0);
        assert!(result.matches.is_empty());
        assert_eq!(result.path_str.as_ref(), "miss.txt");
    }

    #[test]
    fn search_one_file_count_only_literal_counts_whole_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("multi.txt");
        std::fs::write(&file, "hi hi hi\nnope\nhi\n").unwrap();
        let matcher = build_matcher("hi", true, false, false).unwrap();
        let mut params = params_with_cap(None);
        params.count_only = true;
        let counted = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(counted.count, 4);
        assert!(counted.matches.is_empty());
    }

    #[test]
    fn search_one_file_literal_newline_needle_is_not_a_line_hit() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("lines.txt");
        std::fs::write(&file, "alpha\nbeta\n").unwrap();
        let matcher = build_matcher("\n", true, false, false).unwrap();
        assert!(
            search_one_file(&file, &matcher, &params_with_cap(None), dir.path()).is_none(),
            "newline needle must stay no_matches in line-oriented search"
        );

        let mut count_params = params_with_cap(None);
        count_params.count_only = true;
        assert!(
            search_one_file(&file, &matcher, &count_params, dir.path()).is_none(),
            "count-only newline needle must stay no_matches"
        );

        let mut list_params = params_with_cap(None);
        list_params.files_with_matches = true;
        assert!(
            search_one_file(&file, &matcher, &list_params, dir.path()).is_none(),
            "-l must not list files for a newline needle"
        );

        let mut without = params_with_cap(None);
        without.files_without_match = true;
        let result = search_one_file(&file, &matcher, &without, dir.path())
            .expect("-L must list a file with no line-oriented hit");
        assert_eq!(result.count, 0);
    }

    #[test]
    fn search_one_file_literal_crlf_span_needle_is_not_a_line_hit() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("span.txt");
        std::fs::write(&file, "foo\nbar\n").unwrap();
        let matcher = build_matcher("foo\nbar", true, false, false).unwrap();
        assert!(
            search_one_file(&file, &matcher, &params_with_cap(None), dir.path()).is_none(),
            "cross-line literal must stay no_matches without --multiline"
        );

        let mut count_params = params_with_cap(None);
        count_params.count_only = true;
        assert!(
            search_one_file(&file, &matcher, &count_params, dir.path()).is_none(),
            "count-only must not count a CR/LF-spanning literal"
        );
    }
}
