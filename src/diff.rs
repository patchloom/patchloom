//! Unified diff generation and rendering.

use similar::TextDiff;

/// Represents the diff for a single file.
#[derive(Debug)]
pub struct FileDiff {
    /// The file path.
    pub path: String,
    /// Each hunk as a unified diff text fragment.
    pub hunks: Vec<String>,
    /// Whether the file has any changes.
    pub has_changes: bool,
}

/// Represents the diff result across multiple files.
#[derive(Debug)]
pub struct DiffResult {
    /// The diffs for each file.
    pub diffs: Vec<FileDiff>,
    /// The total number of files that have changes.
    pub total_files_changed: usize,
}

/// Compute a unified diff between `old` and `new` content for the given file path.
///
/// Uses [`similar::TextDiff`] to compute the diff and formats it as a standard
/// unified diff with `--- a/<path>` and `+++ b/<path>` headers.
///
/// If there are no changes, returns a [`FileDiff`] with `has_changes: false`
/// and empty hunks.
pub fn unified_diff(path: &str, old: &str, new: &str) -> FileDiff {
    let diff = TextDiff::from_lines(old, new);
    let unified = diff.unified_diff();
    let hunks: Vec<String> = unified.iter_hunks().map(|h| h.to_string()).collect();
    let has_changes = !hunks.is_empty();

    FileDiff {
        path: path.to_string(),
        hunks,
        has_changes,
    }
}

/// Format a [`DiffResult`] into a single unified diff string.
///
/// Concatenates all file diffs that have changes, prepending the standard
/// `--- a/<path>` / `+++ b/<path>` header pair to each file's hunks.
/// Returns an empty string if no files changed.
pub fn format_diff_result(result: &DiffResult) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for diff in &result.diffs {
        if diff.has_changes {
            let _ = write!(output, "--- a/{}\n+++ b/{}\n", diff.path, diff.path);
            for hunk in &diff.hunks {
                output.push_str(hunk);
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_change_has_correct_header() {
        let result = unified_diff("test.txt", "hello\n", "world\n");
        assert!(result.has_changes);
        assert_eq!(result.path, "test.txt");
        assert!(!result.hunks.is_empty());

        let diff_result = DiffResult {
            diffs: vec![result],
            total_files_changed: 1,
        };
        let output = format_diff_result(&diff_result);
        assert!(output.contains("--- a/test.txt"));
        assert!(output.contains("+++ b/test.txt"));
    }

    #[test]
    fn multi_line_change_shows_correct_plus_and_minus() {
        let old = "line1\nold line\nline3\n";
        let new = "line1\nnew line\nline3\n";
        let result = unified_diff("multi.txt", old, new);
        assert!(result.has_changes);

        let hunk = &result.hunks[0];
        assert!(hunk.contains("-old line"));
        assert!(hunk.contains("+new line"));
    }

    #[test]
    fn addition_empty_old_non_empty_new() {
        let result = unified_diff("new_file.txt", "", "new content\n");
        assert!(result.has_changes);
        assert!(!result.hunks.is_empty());

        let hunk = &result.hunks[0];
        assert!(hunk.contains("+new content"));
    }

    #[test]
    fn deletion_non_empty_old_empty_new() {
        let result = unified_diff("deleted.txt", "old content\n", "");
        assert!(result.has_changes);
        assert!(!result.hunks.is_empty());

        let hunk = &result.hunks[0];
        assert!(hunk.contains("-old content"));
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
            total_files_changed: 2,
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
        let result = DiffResult {
            diffs: Vec::new(),
            total_files_changed: 0,
        };
        assert!(format_diff_result(&result).is_empty());
    }

    #[test]
    fn both_empty_produces_no_changes() {
        let result = unified_diff("empty.txt", "", "");
        assert!(!result.has_changes);
        assert!(result.hunks.is_empty());
    }
}
