//! Unified diff generation and rendering.

use similar::TextDiff;

/// Normalize a path for unified-diff headers after the fixed `a/` / `b/` prefix.
///
/// Absolute Unix paths start with `/` (sometimes more than one after sloppy
/// joins). Naively formatting `--- a/{path}` yields `--- a//tmp/file`.
/// Strip all leading `/` so headers never contain `a//` or `b//`.
/// Relative paths and placeholders (e.g. `<content>`) are unchanged.
/// Windows drive paths (`C:\...`) have no leading `/` and are unchanged.
/// A path that is only slashes (e.g. `/`) becomes `.`.
///
/// Used only for header display; [`FileDiff::path`] keeps the original string.
pub(crate) fn path_for_diff_header(path: &str) -> &str {
    match path.trim_start_matches('/') {
        "" if path.is_empty() => "",
        "" => ".", // was one or more `/` only
        other => other,
    }
}

/// Represents the diff for a single file.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// The file path.
    pub path: String,
    /// All hunks concatenated into a single string, avoiding intermediate
    /// `Vec<String>` allocation.
    pub hunks: String,
    /// Whether the file has any changes.
    pub has_changes: bool,
}

/// Represents the diff result across multiple files.
#[derive(Debug)]
pub struct DiffResult {
    /// The diffs for each file.
    pub diffs: Vec<FileDiff>,
}

/// Compute a unified diff between `old` and `new` content for the given file path.
///
/// Uses [`similar::TextDiff`] to compute the diff and formats it as a standard
/// unified diff with `--- a/<path>` and `+++ b/<path>` headers.
///
/// If there are no changes, returns a [`FileDiff`] with `has_changes: false`
/// and empty hunks.
pub fn unified_diff(path: &str, old: &str, new: &str) -> FileDiff {
    use std::fmt::Write;
    let diff = TextDiff::from_lines(old, new);
    let unified = diff.unified_diff();
    let mut hunks = String::new();
    for hunk in unified.iter_hunks() {
        let _ = write!(hunks, "{hunk}");
    }
    let has_changes = !hunks.is_empty();

    FileDiff {
        path: path.to_string(),
        hunks,
        has_changes,
    }
}

/// Format a [`DiffResult`] into a single unified diff string (no color).
///
/// Concatenates all file diffs that have changes, prepending the standard
/// `--- a/<path>` / `+++ b/<path>` header pair to each file's hunks.
/// Returns an empty string if no files changed.
pub fn format_diff_result(result: &DiffResult) -> String {
    format_diff_result_opt(result, false)
}

/// Format a [`DiffResult`] with optional ANSI color.
#[cfg(feature = "cli")]
pub fn format_diff_result_colored(result: &DiffResult, color: bool) -> String {
    format_diff_result_opt(result, color)
}

fn format_diff_result_opt(result: &DiffResult, color: bool) -> String {
    use std::fmt::Write;

    let (hdr, add, rem, hunk, reset) = if color {
        ("\x1b[1m", "\x1b[32m", "\x1b[31m", "\x1b[36m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };

    let mut output = String::new();
    for diff in &result.diffs {
        if diff.has_changes {
            let header_path = path_for_diff_header(&diff.path);
            let _ = write!(
                output,
                "{hdr}--- a/{header_path}{reset}\n{hdr}+++ b/{header_path}{reset}\n",
            );
            if color {
                for line in diff.hunks.lines() {
                    if line.starts_with('+') {
                        let _ = writeln!(output, "{add}{line}{reset}");
                    } else if line.starts_with('-') {
                        let _ = writeln!(output, "{rem}{line}{reset}");
                    } else if line.starts_with("@@") {
                        let _ = writeln!(output, "{hunk}{line}{reset}");
                    } else {
                        let _ = writeln!(output, "{line}");
                    }
                }
            } else {
                output.push_str(&diff.hunks);
            }
        }
    }
    output
}

/// Render a slice of [`FileDiff`]s as plain text (no ANSI color).
///
/// Convenience wrapper used by CLI commands that need a diff string
/// for JSON output.
#[cfg(feature = "cli")]
pub(crate) fn render_diffs_plain(diffs: &[FileDiff]) -> String {
    let result = DiffResult {
        diffs: diffs.to_vec(),
    };
    format_diff_result_colored(&result, false)
}

/// Render a slice of [`FileDiff`]s with optional ANSI color.
///
/// Convenience wrapper used by CLI commands that need colored diff
/// output for the terminal.
#[cfg(feature = "cli")]
pub(crate) fn render_diffs_colored(diffs: &[FileDiff], color: bool) -> String {
    let result = DiffResult {
        diffs: diffs.to_vec(),
    };
    format_diff_result_colored(&result, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_for_diff_header_strips_leading_slashes() {
        assert_eq!(path_for_diff_header("/tmp/demo/lib.rs"), "tmp/demo/lib.rs");
        assert_eq!(
            path_for_diff_header("/private/tmp/demo/src/a.rs"),
            "private/tmp/demo/src/a.rs"
        );
        assert_eq!(path_for_diff_header("src/main.rs"), "src/main.rs");
        assert_eq!(path_for_diff_header("<content>"), "<content>");
        assert_eq!(
            path_for_diff_header("C:\\Users\\x\\f.rs"),
            "C:\\Users\\x\\f.rs"
        );
        // Multiple leading slashes must not leave a leading / (would re-create a//).
        assert_eq!(path_for_diff_header("//unc/share"), "unc/share");
        assert_eq!(path_for_diff_header("///a"), "a");
        assert_eq!(path_for_diff_header("/"), ".");
        assert_eq!(path_for_diff_header(""), "");
    }

    #[test]
    fn single_line_change_has_correct_header() {
        let result = unified_diff("test.txt", "hello\n", "world\n");
        assert!(result.has_changes);
        assert_eq!(result.path, "test.txt");
        assert!(!result.hunks.is_empty(), "hunks should contain diff text");

        let diff_result = DiffResult {
            diffs: vec![result],
        };
        let output = format_diff_result(&diff_result);
        assert!(output.contains("--- a/test.txt"));
        assert!(output.contains("+++ b/test.txt"));
    }

    #[test]
    fn absolute_unix_path_header_has_no_double_slash() {
        let result = unified_diff("/tmp/demo/lib.rs", "old\n", "new\n");
        // Stored path stays absolute for callers.
        assert_eq!(result.path, "/tmp/demo/lib.rs");
        let output = format_diff_result(&DiffResult {
            diffs: vec![result],
        });
        assert!(
            !output.contains("--- a//") && !output.contains("+++ b//"),
            "headers must not double-slash absolute paths: {output}"
        );
        assert!(output.contains("--- a/tmp/demo/lib.rs"));
        assert!(output.contains("+++ b/tmp/demo/lib.rs"));
    }

    #[test]
    fn absolute_macos_private_tmp_header_normalized() {
        let path = "/private/tmp/demo/src/a.rs";
        let result = unified_diff(path, "old\n", "new\n");
        let output = format_diff_result(&DiffResult {
            diffs: vec![result],
        });
        assert!(!output.contains("a//"));
        assert!(!output.contains("b//"));
        assert!(output.contains("--- a/private/tmp/demo/src/a.rs"));
        assert!(output.contains("+++ b/private/tmp/demo/src/a.rs"));
    }

    #[test]
    fn multi_file_format_normalizes_each_absolute_path() {
        let d1 = unified_diff("/abs/one.txt", "a\n", "b\n");
        let d2 = unified_diff("rel/two.txt", "c\n", "d\n");
        let output = format_diff_result(&DiffResult {
            diffs: vec![d1, d2],
        });
        assert!(!output.contains("a//") && !output.contains("b//"));
        assert!(output.contains("--- a/abs/one.txt"));
        assert!(output.contains("--- a/rel/two.txt"));
    }

    #[test]
    fn multi_line_change_shows_correct_plus_and_minus() {
        let old = "line1\nold line\nline3\n";
        let new = "line1\nnew line\nline3\n";
        let result = unified_diff("multi.txt", old, new);
        assert!(result.has_changes);

        assert!(result.hunks.contains("-old line"));
        assert!(result.hunks.contains("+new line"));
    }

    #[test]
    fn addition_empty_old_non_empty_new() {
        let result = unified_diff("new_file.txt", "", "new content\n");
        assert!(result.has_changes);
        assert!(!result.hunks.is_empty());
        assert!(result.hunks.contains("+new content"));
    }

    #[test]
    fn deletion_non_empty_old_empty_new() {
        let result = unified_diff("deleted.txt", "old content\n", "");
        assert!(result.has_changes);
        assert!(!result.hunks.is_empty());
        assert!(result.hunks.contains("-old content"));
    }

    #[test]
    fn no_change_produces_empty_hunks() {
        let content = "same content\n";
        let result = unified_diff("same.txt", content, content);
        assert!(!result.has_changes);
        assert!(result.hunks.is_empty());
    }

    #[test]
    fn diff_result_with_multiple_files() {
        let diff1 = unified_diff("file1.txt", "old1\n", "new1\n");
        let diff2 = unified_diff("file2.txt", "old2\n", "new2\n");
        let diff3 = unified_diff("unchanged.txt", "same\n", "same\n");

        let result = DiffResult {
            diffs: vec![diff1, diff2, diff3],
        };

        let output = format_diff_result(&result);
        assert!(output.contains("--- a/file1.txt"));
        assert!(output.contains("+++ b/file1.txt"));
        assert!(output.contains("--- a/file2.txt"));
        assert!(output.contains("+++ b/file2.txt"));
        assert!(!output.contains("unchanged.txt"));
    }

    #[test]
    fn format_diff_result_empty_produces_empty_string() {
        let result = DiffResult { diffs: Vec::new() };
        assert!(format_diff_result(&result).is_empty());
    }

    #[test]
    fn both_empty_produces_no_changes() {
        let result = unified_diff("empty.txt", "", "");
        assert!(!result.has_changes);
        assert!(result.hunks.is_empty());
    }

    #[test]
    #[cfg(feature = "cli")]
    fn colored_diff_has_ansi_codes() {
        let diff = unified_diff("test.txt", "old\n", "new\n");
        let result = DiffResult { diffs: vec![diff] };
        let colored = format_diff_result_colored(&result, true);
        // Bold header
        assert!(colored.contains("\x1b[1m--- a/test.txt"));
        // Green addition
        assert!(colored.contains("\x1b[32m+new"));
        // Red removal
        assert!(colored.contains("\x1b[31m-old"));
        // Reset codes present
        assert!(colored.contains("\x1b[0m"));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn colored_false_has_no_ansi_codes() {
        let diff = unified_diff("test.txt", "old\n", "new\n");
        let result = DiffResult { diffs: vec![diff] };
        let plain = format_diff_result_colored(&result, false);
        assert!(!plain.contains("\x1b["));
        // But still has diff content
        assert!(plain.contains("--- a/test.txt"));
        assert!(plain.contains("-old"));
        assert!(plain.contains("+new"));
    }
}
