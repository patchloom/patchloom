/// Small extracted helper for #840 thinning demo.
/// Full preserving can be moved here over time.
pub(crate) fn hoist_comments(original: &str, body: &str) -> String {
    let comments: String = original
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.is_empty() || t.starts_with('#')
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !comments.trim().is_empty() {
        let sep = if comments.ends_with('\n') || body.starts_with('\n') {
            ""
        } else {
            "\n"
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
}
