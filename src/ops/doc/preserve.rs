/// Small extracted helper for #840 thinning demo.
/// Full preserving can be moved here over time.
pub(crate) fn hoist_comments(original: &str, body: &str) -> String {
    let eol = crate::write::detect_eol(original);
    let comments: String = original
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.is_empty() || t.starts_with('#')
        })
        .collect::<Vec<_>>()
        .join(eol);
    if !comments.trim().is_empty() {
        let sep = if comments.ends_with('\n') || body.starts_with('\n') {
            ""
        } else {
            eol
        };
        format!("{}{}{}", comments, sep, body)
    } else {
        body.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoist_comments_prepends_comment_block() {
        let original = "# top comment\nkey = \"val\"\n";
        let body = "key = \"new\"\n";
        let result = hoist_comments(original, body);
        assert!(
            result.starts_with("# top comment"),
            "should prepend comment: {result}"
        );
        assert!(result.contains("key = \"new\""));
    }

    #[test]
    fn hoist_comments_returns_body_when_no_comments() {
        let original = "key = \"val\"\nother = 1\n";
        let body = "key = \"new\"\n";
        assert_eq!(hoist_comments(original, body), body);
    }

    #[test]
    fn hoist_comments_preserves_blank_lines_between_comments() {
        let original = "# group A\n\n# group B\nkey = 1\n";
        let body = "key = 2\n";
        let result = hoist_comments(original, body);
        assert!(
            result.contains("# group A\n\n# group B"),
            "blank line between comment groups should be preserved: {result}"
        );
    }

    #[test]
    fn hoist_comments_adds_separator_when_needed() {
        let original = "# comment\nkey = 1\n";
        let body = "key = 2";
        let result = hoist_comments(original, body);
        // The comment does not end with \n, and body does not start with \n,
        // so a separator newline should be inserted.
        assert!(
            result.contains("# comment\nkey = 2"),
            "separator newline between comment and body: {result}"
        );
    }

    #[test]
    fn hoist_comments_preserves_crlf_line_endings() {
        let original = "# top comment\r\n\r\n# second\r\nkey = \"val\"\r\n";
        let body = "key = \"new\"\r\n";
        let result = hoist_comments(original, body);
        assert!(
            result.contains("# top comment\r\n\r\n# second"),
            "CRLF should be preserved in comment join: {result:?}"
        );
    }

    /// Regression: the separator between comments and body must use
    /// the detected EOL, not a hardcoded "\n", to avoid mixed endings.
    #[test]
    fn hoist_comments_separator_uses_detected_eol() {
        // CRLF file where comments don't end with \n and body doesn't
        // start with \n, triggering the separator path.
        let original = "# config\r\nkey: value\r\n";
        let body = "key: new\r\n";
        let result = hoist_comments(original, body);
        // The separator between "# config" and "key: new" must be \r\n.
        assert!(
            result.contains("# config\r\nkey: new"),
            "separator must use CRLF for CRLF files: {result:?}"
        );
        // No bare LF should exist (would indicate mixed endings).
        let without_crlf = result.replace("\r\n", "");
        assert!(
            !without_crlf.contains('\n'),
            "no bare LF should exist in CRLF output: {result:?}"
        );
    }
}
