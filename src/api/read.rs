//! File read operation (with optional line range) for the library API.

use std::path::Path;

/// Read a file's content, optionally restricted to a line range.
///
/// Line numbers are 1-based inclusive. This is a read-only operation.
/// Uses **Strict** text load (#1894): binary / invalid UTF-8 → `InvalidInputError`.
pub fn read(
    path: &Path,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> anyhow::Result<String> {
    let display = path.to_string_lossy();
    let content = crate::files::load_text_strict(path, &display)?;

    match (start_line, end_line) {
        (None, None) => Ok(content),
        (start, end) => {
            let start = start.unwrap_or(1).saturating_sub(1); // convert to 0-based
            let lines: Vec<&str> = content.lines().collect();
            let end = end.unwrap_or(lines.len()).min(lines.len());
            if start >= lines.len() {
                return Ok(String::new());
            }
            let selected: Vec<&str> = lines[start..end].to_vec();
            let mut result = selected.join("\n");
            if !result.is_empty() {
                result.push('\n');
            }
            Ok(result)
        }
    }
}
