//! Aider-style SEARCH/REPLACE and DiffFenced parse.
//!
//! Apply lives in [`crate::api::apply_search_replace_blocks`]. Hosts must not
//! `replacen(..., 1)` or raw `fs::write` for this format.

/// One SEARCH/REPLACE block (path + exact old / new).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchReplaceBlock {
    pub path: String,
    pub old: String,
    pub new: String,
}

/// Parse error for SEARCH/REPLACE / DiffFenced documents.
#[derive(Debug)]
pub struct SearchReplaceParseError {
    pub message: String,
    /// Complete blocks parsed before a truncated last block.
    pub complete: Vec<SearchReplaceBlock>,
    pub truncated: bool,
}

impl std::fmt::Display for SearchReplaceParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SearchReplaceParseError {}

impl SearchReplaceParseError {
    fn malformed(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            complete: Vec::new(),
            truncated: false,
        }
    }

    fn truncated(complete: Vec<SearchReplaceBlock>) -> Self {
        let n = complete.len();
        Self {
            message: format!(
                "truncated SEARCH/REPLACE: {n} complete block(s) before incomplete last block"
            ),
            complete,
            truncated: true,
        }
    }
}

/// Parse `<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE` blocks.
pub fn parse_search_replace(
    input: &str,
) -> Result<Vec<SearchReplaceBlock>, SearchReplaceParseError> {
    parse_search_replace_inner(input)
}

/// DiffFenced: unwrap fenced code blocks, then parse SEARCH/REPLACE.
pub fn parse_diff_fenced(input: &str) -> Result<Vec<SearchReplaceBlock>, SearchReplaceParseError> {
    let unwrapped = strip_fences_for_search_replace(input);
    parse_search_replace_inner(&unwrapped)
}

fn parse_search_replace_inner(
    response: &str,
) -> Result<Vec<SearchReplaceBlock>, SearchReplaceParseError> {
    let cleaned = strip_eos_tokens(response);
    let mut actions = Vec::new();
    let mut remaining: &str = &cleaned;

    while let Some(start) = remaining.find("<<<<<<< SEARCH") {
        let block = &remaining[start..];

        let (end, end_marker_len) = if let Some(pos) = block.find(">>>>>>> REPLACE") {
            (pos, ">>>>>>> REPLACE".len())
        } else if let Some(pos) = block.find(">>>>>>>") {
            let after = &block[pos + ">>>>>>>".len()..];
            let trimmed = after.trim_start();
            if trimmed.is_empty()
                || trimmed.starts_with('\n')
                || trimmed.starts_with("<<<<<<< SEARCH")
            {
                (pos, ">>>>>>>".len())
            } else if actions.is_empty() {
                return Err(SearchReplaceParseError::malformed(
                    "missing >>>>>>> REPLACE marker",
                ));
            } else {
                return Err(SearchReplaceParseError::truncated(actions));
            }
        } else if actions.is_empty() {
            return Err(SearchReplaceParseError::malformed(
                "missing >>>>>>> REPLACE marker",
            ));
        } else {
            return Err(SearchReplaceParseError::truncated(actions));
        };

        let block = &block[..end + end_marker_len];

        let separator = block
            .find("=======")
            .ok_or_else(|| SearchReplaceParseError::malformed("missing ======= separator"))?;

        let search_section = &block["<<<<<<< SEARCH".len()..separator];
        let search_section = search_section.trim_start_matches('\n');

        let (file, old_content) = if let Some(dash_pos) = search_section.find("-------") {
            let f = search_section[..dash_pos].trim();
            let c = search_section[dash_pos + "-------".len()..].trim_start_matches('\n');
            (f.to_string(), c.trim_end_matches('\n').to_string())
        } else {
            let mut lines = search_section.lines();
            let f = lines
                .next()
                .ok_or_else(|| SearchReplaceParseError::malformed("empty SEARCH section"))?
                .trim()
                .to_string();
            let c: String = lines.collect::<Vec<_>>().join("\n");
            (f, c)
        };

        let replace_section = &block[separator + "=======".len()..];
        let replace_section = replace_section
            .strip_prefix('\n')
            .unwrap_or(replace_section);
        let new_content = if let Some(stripped) = replace_section.strip_suffix("\n>>>>>>> REPLACE")
        {
            stripped.to_string()
        } else if let Some(stripped) = replace_section.strip_suffix("\n>>>>>>>") {
            stripped.to_string()
        } else if let Some(stripped) = replace_section.strip_suffix(">>>>>>> REPLACE") {
            stripped.to_string()
        } else if let Some(stripped) = replace_section.strip_suffix(">>>>>>>") {
            stripped.to_string()
        } else {
            replace_section.to_string()
        };

        actions.push(SearchReplaceBlock {
            path: file,
            old: old_content,
            new: new_content,
        });

        remaining = &remaining[start + end + end_marker_len..];
    }

    Ok(actions)
}

fn strip_eos_tokens(response: &str) -> String {
    const EOS_PATTERNS: &[&str] = &[
        "<|eos|>",
        "<|eot_id|>",
        "<|end|>",
        "<|im_end|>",
        "<|endoftext|>",
    ];
    let mut cleaned = response.to_string();
    for pat in EOS_PATTERNS {
        cleaned = cleaned.replace(pat, "");
    }
    cleaned
}

fn strip_fences_for_search_replace(input: &str) -> String {
    let mut unwrapped = String::with_capacity(input.len());
    let mut in_fence = false;

    for line in input.lines() {
        let trimmed = line.trim();
        if !in_fence && (trimmed == "```" || trimmed.starts_with("```")) {
            in_fence = true;
            continue;
        }
        if in_fence && trimmed == "```" {
            in_fence = false;
            continue;
        }
        unwrapped.push_str(line);
        unwrapped.push('\n');
    }

    unwrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_replace_valid() {
        let input = "\
<<<<<<< SEARCH
src/foo.rs
-------
old line
=======
new line
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("parse");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "src/foo.rs");
        assert_eq!(blocks[0].old, "old line");
        assert_eq!(blocks[0].new, "new line");
    }

    #[test]
    fn parse_search_replace_eos_token_stripped() {
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>><|eos|>
";
        let blocks = parse_search_replace(input).expect("eos");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].new, "new");
    }

    #[test]
    fn parse_search_replace_eot_id_stripped() {
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>><|eot_id|>
";
        let blocks = parse_search_replace(input).expect("eot");
        assert_eq!(blocks[0].path, "a.rs");
    }

    #[test]
    fn parse_search_replace_bare_close() {
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>>
";
        let blocks = parse_search_replace(input).expect("bare");
        assert_eq!(blocks[0].old, "old");
        assert_eq!(blocks[0].new, "new");
    }

    #[test]
    fn parse_search_replace_truncated_after_first_block() {
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>> REPLACE
<<<<<<< SEARCH
b.rs
-------
incomplete
";
        let err = parse_search_replace(input).expect_err("truncated");
        assert!(err.truncated);
        assert_eq!(err.complete.len(), 1);
        assert_eq!(err.complete[0].path, "a.rs");
    }

    #[test]
    fn parse_diff_fenced_unwraps_fence() {
        let input = "\
```
<<<<<<< SEARCH
a.rs
-------
old
=======
new
>>>>>>> REPLACE
```
";
        let blocks = parse_diff_fenced(input).expect("fenced");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "a.rs");
    }

    #[test]
    fn parse_search_replace_first_line_is_path_without_dashes() {
        let input = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("no dashes");
        assert_eq!(blocks[0].path, "only.rs");
        assert_eq!(blocks[0].old, "the old text");
    }
}
