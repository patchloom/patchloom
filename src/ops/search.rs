use memchr::memchr_iter;
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
        regex::RegexBuilder::new(&escaped)
            .dot_matches_new_line(multiline)
            .case_insensitive(case_insensitive)
            .build()?
    } else {
        Regex::new(&escaped)?
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
    pub assert_count: Option<usize>,
    pub before_context: Option<usize>,
    pub after_context: Option<usize>,
    pub context: Option<usize>,
    pub quiet: bool,
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

    #[cfg(any(feature = "cli", feature = "files"))]
    let display = crate::files::relative_display(path, cwd);
    #[cfg(not(any(feature = "cli", feature = "files")))]
    let display = path.strip_prefix(cwd).unwrap_or(path);
    let path_str: Arc<str> = Arc::from(display.to_string_lossy().as_ref());
    let mut file_matches: Vec<SearchMatch> = Vec::new();
    let mut count = 0usize;

    if params.multiline {
        if params.count_only {
            count = matcher.count_matches(
                &content,
                params.files_with_matches && params.assert_count.is_none(),
            );
        } else {
            let newline_offsets: Vec<usize> = memchr_iter(b'\n', content.as_bytes()).collect();
            for (start, end) in matcher.find_iter_positions(&content) {
                count += 1;
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
        }
    } else if params.count_only {
        for line in content.lines() {
            let found = matcher.find(line);
            let is_match = if params.invert_match {
                found.is_none()
            } else {
                found.is_some()
            };
            if is_match {
                count += 1;
                if params.files_with_matches && params.assert_count.is_none() {
                    break;
                }
            }
        }
    } else {
        let ctx_before = params.before_context.or(params.context).unwrap_or(0);
        let ctx_after = params.after_context.or(params.context).unwrap_or(0);
        let has_ctx = ctx_before > 0 || ctx_after > 0;

        if has_ctx {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().copied().enumerate() {
                let found = matcher.find(line);
                let is_match = if params.invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };
                if !is_match {
                    continue;
                }
                count += 1;
                let column = found.map_or(1, |(s, _)| s + 1);
                let start = i.saturating_sub(ctx_before);
                let end = (i + 1 + ctx_after).min(lines.len());
                file_matches.push(SearchMatch {
                    path: path_str.clone(),
                    line: i + 1,
                    column,
                    text: line.to_string(),
                    context_before: Some(lines[start..i].iter().map(|s| s.to_string()).collect()),
                    context_after: Some(lines[i + 1..end].iter().map(|s| s.to_string()).collect()),
                });
            }
        } else {
            for (i, line) in content.lines().enumerate() {
                let found = matcher.find(line);
                let is_match = if params.invert_match {
                    found.is_none()
                } else {
                    found.is_some()
                };
                if !is_match {
                    continue;
                }
                count += 1;
                file_matches.push(SearchMatch {
                    path: path_str.clone(),
                    line: i + 1,
                    column: found.map_or(1, |(s, _)| s + 1),
                    text: line.to_string(),
                    context_before: None,
                    context_after: None,
                });
            }
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
        all_matches.sort_unstable_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
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
        assert!(m.find("say hello world").is_some());
        assert!(m.find("say Hello world").is_none());
    }

    #[test]
    fn build_matcher_literal_case_insensitive_uses_regex() {
        let m = build_matcher("hello", true, true, false).unwrap();
        assert!(matches!(m, Matcher::Regex(_)));
        assert!(m.find("say Hello world").is_some());
    }

    #[test]
    fn build_matcher_regex_pattern() {
        let m = build_matcher(r"hel+o", false, false, false).unwrap();
        assert!(m.find("hello").is_some());
        assert!(m.find("helllo").is_some());
        assert!(m.find("heo").is_none());
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
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(result.count, 2);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].line, 1);
        assert_eq!(result.matches[1].line, 3);
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
            assert_count: None,
            before_context: None,
            after_context: None,
            context: None,
            quiet: true,
        };
        let result = search_one_file(&file, &matcher, &params, dir.path()).unwrap();
        assert_eq!(result.count, 2);
        assert!(
            result.matches.is_empty(),
            "count_only should skip match objects"
        );
    }
}
