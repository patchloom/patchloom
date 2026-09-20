//! Aider-style SEARCH/REPLACE and DiffFenced parse.
//!
//! Apply lives in [`crate::api::apply_search_replace_blocks`]. Hosts must not
//! `replacen(..., 1)` or raw `fs::write` for this format. CLI / MCP / tx
//! detect this grammar via [`looks_like_search_replace`] (#2221).

/// Shared refuse when `replace_all` is set on unified or Begin Patch.
pub(crate) const REPLACE_ALL_ONLY_FOR_SEARCH_REPLACE: &str =
    "replace_all is only valid for SEARCH/REPLACE documents";

/// True when any line trims to `<<<<<<< SEARCH`.
#[must_use]
pub fn has_search_replace_marker(input: &str) -> bool {
    input.lines().any(|l| l.trim() == "<<<<<<< SEARCH")
}

/// True when the payload is a SEARCH/REPLACE or DiffFenced document.
///
/// First non-empty line must be `<<<<<<< SEARCH` or a fence (` ``` `) so a
/// unified diff that happens to mention that marker as later content is
/// still parsed as a unified diff.
#[must_use]
pub fn looks_like_search_replace(input: &str) -> bool {
    if !has_search_replace_marker(input) {
        return false;
    }
    match input.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some("<<<<<<< SEARCH") => true,
        Some(l) if l.starts_with("```") => true,
        _ => false,
    }
}

/// True when SEARCH/REPLACE markers appear with Begin Patch or unified-diff
/// file headers in a document that is otherwise a SEARCH/REPLACE payload.
#[must_use]
pub fn has_mixed_search_replace_grammar(input: &str) -> bool {
    if !looks_like_search_replace(input) {
        return false;
    }
    crate::ops::begin_patch::looks_like_begin_patch(input) || has_unified_diff_headers(input)
}

fn has_unified_diff_headers(input: &str) -> bool {
    input
        .lines()
        .any(crate::ops::patch::line_looks_like_unified_file_header)
}

/// Parse SEARCH/REPLACE, or DiffFenced (fenced unwrap) when the document
/// wraps blocks in triple backticks.
pub fn parse_search_replace_document(
    input: &str,
) -> Result<Vec<SearchReplaceBlock>, SearchReplaceParseError> {
    if has_mixed_search_replace_grammar(input) {
        return Err(SearchReplaceParseError::malformed(
            "mixed SEARCH/REPLACE and unified-diff or Begin Patch grammar is not supported",
        ));
    }
    let fenced = input.lines().any(|l| {
        let t = l.trim();
        t == "```" || t.starts_with("```")
    });
    if fenced {
        parse_diff_fenced(input)
    } else {
        parse_search_replace(input)
    }
}

/// Dest paths declared in a SEARCH/REPLACE / DiffFenced document.
pub fn search_replace_declared_paths(input: &str) -> Result<Vec<String>, SearchReplaceParseError> {
    let mut paths = Vec::new();
    for block in parse_search_replace_document(input)? {
        if block.path.is_empty() {
            continue;
        }
        if !paths.iter().any(|p| p == &block.path) {
            paths.push(block.path);
        }
    }
    Ok(paths)
}

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

        let separator = find_whole_line_marker(block, "=======")
            .ok_or_else(|| SearchReplaceParseError::malformed("missing ======= separator"))?;

        let search_section = skip_one_eol(&block["<<<<<<< SEARCH".len()..separator]);

        // Dest is a whole line that is exactly `-------` (optional trailing
        // space / CR). Inline dashes in dest-less SEARCH stay dest-less.
        let (file, old_content) =
            if let Some(dash_pos) = find_whole_line_marker(search_section, "-------") {
                let f = lf_normalize(search_section[..dash_pos].trim());
                let c = after_whole_line(search_section, dash_pos);
                (f, lf_normalize(c).trim_end_matches('\n').to_string())
            } else {
                // Dest-less: every SEARCH line is old text. apply_patch supplies
                // dest via file_hint. Multi-file documents use the ------- form.
                (
                    String::new(),
                    lf_normalize(search_section)
                        .trim_end_matches('\n')
                        .to_string(),
                )
            };

        let replace_section = after_whole_line(block, separator);
        let new_raw = if let Some(stripped) = replace_section.strip_suffix("\n>>>>>>> REPLACE") {
            stripped
        } else if let Some(stripped) = replace_section.strip_suffix("\n>>>>>>>") {
            stripped
        } else if let Some(stripped) = replace_section.strip_suffix(">>>>>>> REPLACE") {
            stripped
        } else if let Some(stripped) = replace_section.strip_suffix(">>>>>>>") {
            stripped
        } else {
            replace_section
        };
        let new_content = lf_normalize(new_raw);

        actions.push(SearchReplaceBlock {
            path: file,
            old: old_content,
            new: new_content,
        });

        remaining = &remaining[start + end + end_marker_len..];
    }

    Ok(actions)
}

/// Byte offset of a whole line that is exactly `marker` (optional trailing whitespace).
fn find_whole_line_marker(block: &str, marker: &str) -> Option<usize> {
    let mut offset = 0;
    for line in block.split_inclusive('\n') {
        let without_nl = line.strip_suffix('\n').unwrap_or(line);
        let without_eol = without_nl.strip_suffix('\r').unwrap_or(without_nl);
        if without_eol.trim_end() == marker {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn after_whole_line(block: &str, line_start: usize) -> &str {
    let rest = &block[line_start..];
    match rest.find('\n') {
        Some(n) => &rest[n + 1..],
        None => "",
    }
}

fn skip_one_eol(s: &str) -> &str {
    s.strip_prefix("\r\n")
        .or_else(|| s.strip_prefix('\n'))
        .unwrap_or(s)
}

fn lf_normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "")
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
    fn looks_like_search_replace_first_line_or_fence() {
        assert!(looks_like_search_replace(
            "<<<<<<< SEARCH\nfile.rs\n-------\nold\n=======\nnew\n>>>>>>> REPLACE\n"
        ));
        assert!(looks_like_search_replace(
            "```\n<<<<<<< SEARCH\nfile.rs\n-------\nold\n=======\nnew\n>>>>>>> REPLACE\n```\n"
        ));
        assert!(!looks_like_search_replace(
            "--- a/file.rs\n+++ b/file.rs\n@@ -1 +1 @@\n-old\n+new\n"
        ));
        assert!(
            !looks_like_search_replace(
                "--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n context\n <<<<<<< SEARCH\n+keep\n"
            ),
            "unified diff that mentions SEARCH later is not SEARCH/REPLACE"
        );
        assert!(has_search_replace_marker("--- a/x\n<<<<<<< SEARCH\nkeep\n"));
        assert!(!has_search_replace_marker("--- a/x\n+++ b/x\n"));
    }

    #[test]
    fn mixed_search_replace_and_unified_headers_refused() {
        let input = "\
<<<<<<< SEARCH
file.rs
-------
old
=======
new
>>>>>>> REPLACE
--- a/file.rs
+++ b/file.rs
";
        assert!(has_mixed_search_replace_grammar(input));
        let err = parse_search_replace_document(input).expect_err("mixed");
        assert!(!err.truncated, "mixed grammar is malformed, not truncated");
        assert!(
            err.message.contains("mixed SEARCH/REPLACE"),
            "expected mixed-grammar refuse, got {}",
            err.message
        );
    }

    #[test]
    fn mixed_search_replace_and_backslash_unified_headers_refused() {
        let input = "\
<<<<<<< SEARCH
file.rs
-------
old
=======
new
>>>>>>> REPLACE
--- a\\file.rs
+++ b\\file.rs
";
        assert!(has_mixed_search_replace_grammar(input));
        let err = parse_search_replace_document(input).expect_err("mixed backslash");
        assert!(!err.truncated, "mixed grammar is malformed, not truncated");
        assert!(
            err.message.contains("mixed SEARCH/REPLACE"),
            "expected mixed-grammar refuse, got {}",
            err.message
        );
    }

    #[test]
    fn parse_search_replace_destless_keeps_first_line_as_search() {
        let input = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("dest-less");
        assert_eq!(
            blocks[0].path, "",
            "no ------- dest: first SEARCH line is body, not dest"
        );
        assert_eq!(blocks[0].old, "only.rs\nthe old text");
        assert_eq!(blocks[0].new, "the new text");
    }

    #[test]
    fn parse_search_replace_destless_code_line_is_not_dest() {
        let input = "\
<<<<<<< SEARCH
            pub fn set(&mut self, deg: u32) -> Result<u64, Error> {
        if deg == 0 {
=======
    pub fn set(&mut self, deg: u32) -> Result<u64, Error> {
        if deg == 0 {
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("dest-less code");
        assert!(
            blocks[0].path.is_empty(),
            "code line must not become dest, got {:?}",
            blocks[0].path
        );
        assert!(
            blocks[0]
                .old
                .contains("pub fn set(&mut self, deg: u32) -> Result<u64, Error> {"),
            "first SEARCH line stays in old, got {:?}",
            blocks[0].old
        );
    }

    #[test]
    fn search_replace_declared_paths_skips_destless_blocks() {
        let destless = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        assert_eq!(
            search_replace_declared_paths(destless).expect("dest-less"),
            Vec::<String>::new(),
            "dest-less must not declare the first SEARCH line as dest"
        );

        let mixed = "\
<<<<<<< SEARCH
keep.rs
-------
old
=======
new
>>>>>>> REPLACE
<<<<<<< SEARCH
skip.rs
body
=======
other
>>>>>>> REPLACE
";
        assert_eq!(
            search_replace_declared_paths(mixed).expect("mixed"),
            vec!["keep.rs".to_string()],
            "only ------- dests are declared"
        );
    }

    #[test]
    fn parse_search_replace_banner_equals_is_not_separator() {
        let input = "\
<<<<<<< SEARCH
f.py
-------
# ==========
title
=======
# ==========
renamed
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("banner equals");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "f.py");
        assert_eq!(blocks[0].old, "# ==========\ntitle");
        assert_eq!(blocks[0].new, "# ==========\nrenamed");
    }

    #[test]
    fn parse_search_replace_setext_underline_is_not_separator() {
        let input = "\
<<<<<<< SEARCH
doc.md
-------
Heading
==========
=======
Heading
==========
updated
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("setext underline");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "doc.md");
        assert_eq!(blocks[0].old, "Heading\n==========");
        assert_eq!(blocks[0].new, "Heading\n==========\nupdated");
    }

    #[test]
    fn parse_search_replace_separator_allows_trailing_whitespace() {
        let input = "\
<<<<<<< SEARCH
a.rs
-------
old
=======  
new
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("trailing ws on separator");
        assert_eq!(blocks[0].old, "old");
        assert_eq!(blocks[0].new, "new");
    }

    #[test]
    fn parse_search_replace_crlf_dest_present_matches_lf() {
        let lf = "\
<<<<<<< SEARCH
code.rs
-------
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
";
        let crlf = lf.replace('\n', "\r\n");
        let lf_blocks = parse_search_replace(lf).expect("lf");
        let crlf_blocks = parse_search_replace(&crlf).expect("crlf");
        assert_eq!(crlf_blocks, lf_blocks);
        assert_eq!(crlf_blocks[0].path, "code.rs");
        assert_eq!(crlf_blocks[0].old, "fn old() {}");
        assert_eq!(crlf_blocks[0].new, "fn new() {}");
        assert!(
            !crlf_blocks[0].old.contains('\r'),
            "old must not keep CR: {:?}",
            crlf_blocks[0].old
        );
        assert!(
            !crlf_blocks[0].new.contains('\r'),
            "new must not keep CR: {:?}",
            crlf_blocks[0].new
        );
    }

    #[test]
    fn parse_search_replace_crlf_destless_has_empty_path_without_cr() {
        let lf = "\
<<<<<<< SEARCH
only.rs
the old text
=======
the new text
>>>>>>> REPLACE
";
        let crlf = lf.replace('\n', "\r\n");
        let blocks = parse_search_replace(&crlf).expect("dest-less crlf");
        assert_eq!(blocks[0].path, "");
        assert_eq!(blocks[0].old, "only.rs\nthe old text");
        assert_eq!(blocks[0].new, "the new text");
        assert!(
            !blocks[0].old.contains('\r'),
            "old must not keep CR: {:?}",
            blocks[0].old
        );
        assert!(
            !blocks[0].new.contains('\r'),
            "new must not keep CR: {:?}",
            blocks[0].new
        );
    }

    #[test]
    fn parse_search_replace_inline_dashes_in_destless_search_are_not_dest() {
        let input = "\
<<<<<<< SEARCH
x = \"-------\"
=======
x = \"eq\"
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("inline dashes");
        assert_eq!(
            blocks[0].path, "",
            "inline ------- in SEARCH is not dest, got {:?}",
            blocks[0].path
        );
        assert_eq!(blocks[0].old, "x = \"-------\"");
        assert_eq!(blocks[0].new, "x = \"eq\"");
    }

    #[test]
    fn parse_search_replace_dest_present_old_may_contain_dashes() {
        let input = "\
<<<<<<< SEARCH
code.rs
-------
x = \"-------\"
=======
x = \"eq\"
>>>>>>> REPLACE
";
        let blocks = parse_search_replace(input).expect("dest-present dashes in old");
        assert_eq!(blocks[0].path, "code.rs");
        assert_eq!(blocks[0].old, "x = \"-------\"");
        assert_eq!(blocks[0].new, "x = \"eq\"");
    }
}
