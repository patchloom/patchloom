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
