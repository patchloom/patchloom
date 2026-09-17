//! File read operation (with optional line range) for the library API.

use std::path::Path;

/// Read a file's content, optionally restricted to a line range.
///
/// Line numbers are 1-based inclusive. This is a read-only operation.
/// Uses **Strict** text load (#1894 / #1963): binary → `BinaryError`, invalid UTF-8 → `InvalidEncodingError`.
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
            let start_1 = start.unwrap_or(1);
            if let Some(end_1) = end
                && start_1 > end_1
            {
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: format!("end line {end_1} is before start line {start_1}"),
                }));
            }
            let start = start_1.saturating_sub(1); // convert to 0-based
            let lines: Vec<&str> = crate::ops::file::text_lines(&content).collect();
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
