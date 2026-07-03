//! Multi-strategy fallback chain for edit resolution.
//!
//! When a primary edit operation fails (e.g., target text not found), this
//! module provides progressively looser matching strategies before giving up.
//!
//! The fallback chain:
//! 1. **Exact match** (primary, handled by the caller)
//! 2. **Anchor-based matching** (structural landmarks around the edit target)
//! 3. **Similarity scoring** (Jaro-Winkler on surrounding lines)
//! 4. **Structured error** (diagnosis with suggestions)
//!
//! # Examples
//!
//! Suggest similar targets when a search misses:
//!
//! ```rust
//! use patchloom::fallback::{EditError, EditErrorKind, find_similar_targets};
//!
//! let content = "fn process_request() {}\nfn process_response() {}\n";
//! let similar = find_similar_targets(content, "process_requst", 3);
//! assert!(similar.iter().any(|s| s.contains("process_request")));
//! ```
//!
//! Run the full fallback chain (exact, anchor, similarity, structured error):
//!
//! ```rust
//! use patchloom::fallback::{resolve_with_fallback, MatchStrategy};
//!
//! let content = "fn setup() {}\nfn proccess_data(x: i32) {}\nfn cleanup() {}\n";
//! let result = resolve_with_fallback(
//!     content,
//!     "fn process_data(x: i32) {}",
//!     Some("fn setup() {}"),
//!     Some("fn cleanup() {}"),
//! ).expect("anchor match should succeed");
//! assert_eq!(result.strategy, MatchStrategy::Anchor);
//! assert!(result.matched_text.contains("proccess_data"));
//! ```
//!
//! Validate an edit before applying it:
//!
//! ```rust
//! use patchloom::fallback::validate_edit;
//!
//! let json = r#"{"key": "value"}"#;
//! let result = validate_edit(json, "value", "new_value", Some("config.json"));
//! assert!(result.valid);
//! ```
//!
//! Validate with `--nth` semantics (only replace the Nth occurrence):
//!
//! ```rust
//! use patchloom::fallback::validate_edit_nth;
//!
//! let json = r#"{"a": "val", "b": "val"}"#;
//! let result = validate_edit_nth(json, "val", "new", Some("data.json"), Some(1));
//! assert!(result.valid);
//! ```
//!
//! size-waiver: domain bulk, see #1376. Multi-strategy edit recovery chain is one conceptual unit.

use serde::{Deserialize, Serialize};

/// Structured error type for edit operations with actionable diagnosis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditError {
    /// What kind of error occurred.
    pub kind: EditErrorKind,
    /// Human-readable error message.
    pub message: String,
    /// A suggestion for how to fix the issue (if available).
    pub suggestion: Option<String>,
    /// Similar targets found in the file (for "did you mean?" hints).
    pub similar_targets: Vec<String>,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)?;
        if let Some(ref suggestion) = self.suggestion {
            write!(f, " (suggestion: {suggestion})")?;
        }
        if !self.similar_targets.is_empty() {
            write!(f, " [similar: {}]", self.similar_targets.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for EditError {}

/// Classification of edit errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditErrorKind {
    /// The edit target matched multiple locations in the file.
    AmbiguousTarget,
    /// The edit target was not found in the file.
    NoMatch,
    /// The edit would produce invalid syntax.
    SyntaxInvalid,
    /// Another edit already modified this region (for batch operations).
    ConflictingEdit,
    /// The file could not be parsed.
    ParseError,
}

impl std::fmt::Display for EditErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditErrorKind::AmbiguousTarget => write!(f, "ambiguous_target"),
            EditErrorKind::NoMatch => write!(f, "no_match"),
            EditErrorKind::SyntaxInvalid => write!(f, "syntax_invalid"),
            EditErrorKind::ConflictingEdit => write!(f, "conflicting_edit"),
            EditErrorKind::ParseError => write!(f, "parse_error"),
        }
    }
}

/// Result of `validate_edit()`: whether the edit would produce valid output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the edit produces valid output.
    pub valid: bool,
    /// Syntax errors found (if invalid).
    pub errors: Vec<String>,
    /// Suspicious but technically valid issues.
    pub warnings: Vec<String>,
}

/// Validate whether a replacement would produce valid output, without applying.
///
/// Performs the replacement in memory and checks basic structural validity
/// of the resulting content (JSON, YAML, TOML validation for structured files).
///
/// When `nth` is `Some(n)`, only the Nth (1-based) occurrence is replaced,
/// matching the behavior of the replace command's `--nth` flag. When `None`,
/// all occurrences are replaced.
pub fn validate_edit(
    content: &str,
    from: &str,
    to: &str,
    file_path: Option<&str>,
) -> ValidationResult {
    validate_edit_nth(content, from, to, file_path, None)
}

/// Like [`validate_edit`] but with an explicit `nth` occurrence parameter.
pub fn validate_edit_nth(
    content: &str,
    from: &str,
    to: &str,
    file_path: Option<&str>,
    nth: Option<usize>,
) -> ValidationResult {
    if from.is_empty() {
        return ValidationResult {
            valid: false,
            errors: vec!["empty search pattern".into()],
            warnings: vec![],
        };
    }

    if !content.contains(from) {
        return ValidationResult {
            valid: false,
            errors: vec![format!(
                "pattern '{}' not found in content",
                truncate_str(from, 60)
            )],
            warnings: vec![],
        };
    }

    let new_content = match nth {
        Some(0) => {
            return ValidationResult {
                valid: false,
                errors: vec!["nth must be >= 1 (1-based indexing)".into()],
                warnings: vec![],
            };
        }
        Some(n) => {
            // Replace only the Nth (1-based) occurrence.
            let mut count = 0usize;
            let mut result = String::with_capacity(content.len());
            let mut remaining = content;
            while let Some(pos) = remaining.find(from) {
                count += 1;
                if count == n {
                    result.push_str(&remaining[..pos]);
                    result.push_str(to);
                    result.push_str(&remaining[pos + from.len()..]);
                    break;
                }
                result.push_str(&remaining[..pos + from.len()]);
                remaining = &remaining[pos + from.len()..];
            }
            if count < n {
                return ValidationResult {
                    valid: false,
                    errors: vec![format!(
                        "occurrence {n} not found (only {count} occurrence{} exist{})",
                        if count == 1 { "" } else { "s" },
                        if count == 1 { "s" } else { "" },
                    )],
                    warnings: vec![],
                };
            }
            result
        }
        None => content.replace(from, to),
    };
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // If we can detect the file format, validate the result.
    if let Some(path) = file_path
        && let Ok(fmt) = crate::ops::doc::detect_format(path)
    {
        let parse_err = match fmt {
            crate::ops::doc::FileFormat::Json => {
                serde_json::from_str::<serde_json::Value>(&new_content)
                    .err()
                    .map(|e| format!("result would be invalid JSON: {e}"))
            }
            crate::ops::doc::FileFormat::Yaml => {
                serde_yaml_ng::from_str::<serde_json::Value>(&new_content)
                    .err()
                    .map(|e| format!("result would be invalid YAML: {e}"))
            }
            crate::ops::doc::FileFormat::Toml => {
                toml_edit::de::from_str::<serde_json::Value>(&new_content)
                    .err()
                    .map(|e| format!("result would be invalid TOML: {e}"))
            }
        };
        if let Some(msg) = parse_err {
            errors.push(msg);
        }
    }

    // Warn if the replacement creates unbalanced brackets/braces.
    let open_parens =
        new_content.matches('(').count() as i64 - new_content.matches(')').count() as i64;
    let open_braces =
        new_content.matches('{').count() as i64 - new_content.matches('}').count() as i64;
    let open_brackets =
        new_content.matches('[').count() as i64 - new_content.matches(']').count() as i64;

    if open_parens != 0 {
        warnings.push(format!("unbalanced parentheses (delta: {open_parens})"));
    }
    if open_braces != 0 {
        warnings.push(format!("unbalanced braces (delta: {open_braces})"));
    }
    if open_brackets != 0 {
        warnings.push(format!("unbalanced brackets (delta: {open_brackets})"));
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Find similar text targets in file content using Jaro-Winkler similarity.
///
/// Extracts identifiers and substrings from the content and returns the top
/// `max_results` matches sorted by similarity score (descending).
pub fn find_similar_targets(content: &str, target: &str, max_results: usize) -> Vec<String> {
    if target.is_empty() || content.is_empty() {
        return vec![];
    }

    let mut candidates: Vec<(String, f64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Extract word-like tokens from the content.
    for line in content.lines() {
        for word in extract_identifiers(line) {
            if !seen.insert(word.clone()) {
                continue;
            }
            let score = strsim::jaro_winkler(&word, target);
            if score > 0.7 && word != target {
                candidates.push((word, score));
            }
        }
    }

    // Also try matching against whole lines for multi-word patterns.
    if target.contains(' ') || target.len() > 20 {
        for line in content.lines() {
            let trimmed = line.trim().to_string();
            if trimmed.is_empty() || seen.contains(&trimmed) {
                continue;
            }
            seen.insert(trimmed.clone());
            let score = strsim::jaro_winkler(&trimmed, target);
            if score > 0.7 && trimmed != target {
                candidates.push((trimmed, score));
            }
        }
    }

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(max_results);
    candidates.into_iter().map(|(s, _)| s).collect()
}

/// Try anchor-based matching: find the target text using surrounding context lines.
///
/// If `target` is not found exactly, looks for lines that share anchor text
/// (the lines immediately before and after the target in the original context)
/// and returns the matching region.
pub fn anchor_match(
    content: &str,
    target: &str,
    before_context: Option<&str>,
    after_context: Option<&str>,
) -> Option<AnchorMatchResult> {
    if target.is_empty() {
        return None;
    }

    // If exact match exists, return it directly.
    if let Some(pos) = content.find(target) {
        return Some(AnchorMatchResult {
            matched_text: target.to_string(),
            start_offset: pos,
            strategy: MatchStrategy::Exact,
        });
    }

    // Try anchor-based matching using before/after context.
    let lines: Vec<&str> = content.lines().collect();
    let target_lines: Vec<&str> = target.lines().collect();

    // Compute byte offset of each line start for accurate slicing,
    // avoiding CRLF vs LF mismatch (the old code used a global
    // heuristic that broke on mixed or CRLF line endings).
    let mut line_byte_starts: Vec<usize> = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' {
            line_byte_starts.push(i + 1);
        }
    }

    if target_lines.is_empty() {
        return None;
    }

    let first_target = target_lines[0].trim();
    let last_target = target_lines.last().map(|l| l.trim()).unwrap_or("");

    // Anchor matching requires at least one piece of structural context
    // (before_context or after_context). Without context, it would
    // degenerate into the same Jaro-Winkler line scan that the
    // similarity path already does, making the fallback chain redundant.
    if before_context.is_none() && after_context.is_none() {
        return None;
    }

    // Find candidate positions by matching the first line with anchors.
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check if this line is similar to the first target line.
        if strsim::jaro_winkler(trimmed, first_target) < 0.85 {
            continue;
        }

        // For multi-line targets, also verify the last line of the
        // candidate region is similar to the last target line.
        if target_lines.len() > 1 {
            let end_idx = i + target_lines.len();
            if end_idx > lines.len() {
                continue;
            }
            let candidate_last = lines[end_idx - 1].trim();
            if strsim::jaro_winkler(candidate_last, last_target) < 0.85 {
                continue;
            }
        }

        // If we have before_context, verify the preceding line matches.
        // For multi-line context, compare only the last line (the one
        // immediately before the target region).
        if let Some(before) = before_context {
            if i == 0 {
                continue;
            }
            let prev = lines[i - 1].trim();
            let before_line = before.lines().last().unwrap_or(before).trim();
            if strsim::jaro_winkler(prev, before_line) < 0.8 {
                continue;
            }
        }

        // If we have after_context, check the line after the candidate region.
        // For multi-line context, compare only the first line (the one
        // immediately after the target region).
        if let Some(after) = after_context {
            let end_idx = i + target_lines.len();
            if end_idx >= lines.len() {
                continue;
            }
            let next = lines[end_idx].trim();
            let after_line = after.lines().next().unwrap_or(after).trim();
            if strsim::jaro_winkler(next, after_line) < 0.8 {
                continue;
            }
        }

        // Found a match. Extract the matched region directly from the
        // original content so line endings are preserved exactly.
        let end_idx = (i + target_lines.len()).min(lines.len());
        let start_offset = line_byte_starts[i];
        let end_offset = if end_idx < line_byte_starts.len() {
            line_byte_starts[end_idx]
        } else {
            content.len()
        };
        // Strip exactly one trailing line ending to match the format
        // of exact-match results (which carry no trailing newline).
        let slice = &content[start_offset..end_offset];
        let matched_text = slice
            .strip_suffix("\r\n")
            .or_else(|| slice.strip_suffix('\n'))
            .unwrap_or(slice)
            .to_string();

        return Some(AnchorMatchResult {
            matched_text,
            start_offset,
            strategy: MatchStrategy::Anchor,
        });
    }

    None
}

/// Result of anchor-based matching.
#[derive(Debug, Clone)]
pub struct AnchorMatchResult {
    /// The text that was matched.
    pub matched_text: String,
    /// Byte offset of the match start in the content.
    pub start_offset: usize,
    /// Which matching strategy succeeded.
    pub strategy: MatchStrategy,
}

/// Which matching strategy found the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrategy {
    /// Exact literal match.
    Exact,
    /// Anchor-based matching using surrounding context.
    Anchor,
    /// Similarity-based fuzzy matching.
    Similarity,
}

impl std::fmt::Display for MatchStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchStrategy::Exact => write!(f, "exact"),
            MatchStrategy::Anchor => write!(f, "anchor"),
            MatchStrategy::Similarity => write!(f, "similarity"),
        }
    }
}

/// Run the full fallback chain: exact -> anchor -> similarity -> structured error.
///
/// Returns the first successful match or a structured error with diagnosis.
pub fn resolve_with_fallback(
    content: &str,
    target: &str,
    before_context: Option<&str>,
    after_context: Option<&str>,
) -> Result<AnchorMatchResult, EditError> {
    // Step 1: Exact match.
    if let Some(pos) = content.find(target) {
        return Ok(AnchorMatchResult {
            matched_text: target.to_string(),
            start_offset: pos,
            strategy: MatchStrategy::Exact,
        });
    }

    // Step 2: Anchor-based matching.
    if let Some(result) = anchor_match(content, target, before_context, after_context) {
        return Ok(result);
    }

    // Step 3: Similarity-based matching (find the most similar line block).
    let target_lines: Vec<&str> = target.lines().collect();
    if target_lines.len() == 1 {
        // Single-line target: find the most similar single line.
        let mut best_score = 0.0f64;
        let mut best_match = String::new();
        let mut best_offset = 0usize;
        let mut offset = 0usize;
        for line in content.lines() {
            let score = strsim::jaro_winkler(line.trim(), target.trim());
            if score > best_score {
                best_score = score;
                best_match = line.to_string();
                best_offset = offset;
            }
            // Advance past the line content and the actual line ending,
            // which may be \r\n (2 bytes) or \n (1 byte).
            offset += line.len();
            if content.as_bytes().get(offset) == Some(&b'\r') {
                offset += 1;
            }
            if content.as_bytes().get(offset) == Some(&b'\n') {
                offset += 1;
            }
        }
        if best_score > 0.85 {
            return Ok(AnchorMatchResult {
                matched_text: best_match,
                start_offset: best_offset,
                strategy: MatchStrategy::Similarity,
            });
        }
    }

    // Step 4: Structured error with diagnosis.
    let similar = find_similar_targets(content, target.lines().next().unwrap_or(target), 5);
    let suggestion = if !similar.is_empty() {
        Some(format!("did you mean: {}?", similar[0]))
    } else {
        None
    };

    Err(EditError {
        kind: EditErrorKind::NoMatch,
        message: format!("target not found: '{}'", truncate_str(target, 80)),
        suggestion,
        similar_targets: similar,
    })
}

/// Extract identifier-like tokens from a line of code.
fn extract_identifiers(line: &str) -> Vec<String> {
    let mut identifiers = Vec::new();
    let mut current = String::new();

    for ch in line.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else {
            if current.len() >= 3 {
                identifiers.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 3 {
        identifiers.push(current);
    }

    identifiers
}

/// Truncate a string for display in error messages.
///
/// Truncates at the last char boundary at or before `max_len` bytes,
/// so this is safe for multi-byte UTF-8 input.
pub(crate) fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find the last char boundary at or before max_len.
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_error_display() {
        let err = EditError {
            kind: EditErrorKind::NoMatch,
            message: "target not found".into(),
            suggestion: Some("try 'process_request'".into()),
            similar_targets: vec!["process_request".into()],
        };
        let display = err.to_string();
        assert!(display.contains("no_match"));
        assert!(display.contains("target not found"));
        assert!(display.contains("process_request"));
    }

    #[test]
    fn edit_error_kind_display() {
        assert_eq!(
            EditErrorKind::AmbiguousTarget.to_string(),
            "ambiguous_target"
        );
        assert_eq!(EditErrorKind::NoMatch.to_string(), "no_match");
        assert_eq!(EditErrorKind::SyntaxInvalid.to_string(), "syntax_invalid");
        assert_eq!(
            EditErrorKind::ConflictingEdit.to_string(),
            "conflicting_edit"
        );
        assert_eq!(EditErrorKind::ParseError.to_string(), "parse_error");
    }

    #[test]
    fn validate_edit_empty_pattern() {
        let result = validate_edit("content", "", "replacement", None);
        assert!(!result.valid);
        assert!(result.errors[0].contains("empty search pattern"));
    }

    #[test]
    fn validate_edit_pattern_not_found() {
        let result = validate_edit("hello world", "missing", "replacement", None);
        assert!(!result.valid);
        assert!(result.errors[0].contains("not found"));
    }

    #[test]
    fn validate_edit_valid_replacement() {
        let result = validate_edit("hello world", "hello", "goodbye", None);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn validate_edit_json_syntax_check() {
        let json = r#"{"key": "value"}"#;
        // Valid replacement.
        let result = validate_edit(json, "value", "new_value", Some("config.json"));
        assert!(result.valid);

        // Invalid replacement (breaks JSON).
        let result = validate_edit(json, "\"key\":", "broken", Some("config.json"));
        assert!(!result.valid);
        assert!(result.errors[0].contains("invalid JSON"));
    }

    #[test]
    fn validate_edit_yaml_syntax_check() {
        let yaml = "key: value\n";
        let result = validate_edit(yaml, "value", "new_value", Some("config.yaml"));
        assert!(result.valid);
    }

    #[test]
    fn validate_edit_warns_unbalanced_braces() {
        let content = "fn main() { hello }";
        let result = validate_edit(content, "{ hello }", "{ hello", None);
        assert!(result.valid); // Still valid (we can't know the language syntax).
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("unbalanced braces"))
        );
    }

    #[test]
    fn find_similar_targets_finds_typos() {
        let content = "fn process_request() {}\nfn process_response() {}\nfn handle_error() {}\n";
        let similar = find_similar_targets(content, "process_requst", 3);
        assert!(!similar.is_empty());
        assert!(similar.iter().any(|s| s.contains("process_request")));
    }

    #[test]
    fn find_similar_targets_empty_content() {
        let similar = find_similar_targets("", "target", 3);
        assert!(similar.is_empty());
    }

    #[test]
    fn find_similar_targets_empty_target() {
        let similar = find_similar_targets("content", "", 3);
        assert!(similar.is_empty());
    }

    #[test]
    fn anchor_match_exact() {
        let content = "line1\nline2\nline3\n";
        let result = anchor_match(content, "line2", None, None).unwrap();
        assert_eq!(result.matched_text, "line2");
        assert_eq!(result.strategy, MatchStrategy::Exact);
    }

    #[test]
    fn anchor_match_with_context() {
        // Simulate a case where the target line changed slightly.
        let content = "fn setup() {}\nfn proccess_data(x: i32) {}\nfn cleanup() {}\n";
        let result = anchor_match(
            content,
            "fn process_data(x: i32) {}",
            Some("fn setup() {}"),
            Some("fn cleanup() {}"),
        );
        let r = result.expect("anchor match should find a fuzzy match");
        assert_eq!(r.strategy, MatchStrategy::Anchor);
        assert!(r.matched_text.contains("proccess_data"));
    }

    #[test]
    fn anchor_match_no_match() {
        let content = "completely different content\n";
        let result = anchor_match(content, "not here at all", None, None);
        assert!(result.is_none());
    }

    #[test]
    fn anchor_match_requires_context_for_fuzzy() {
        // Without context, anchor matching skips the fuzzy path and returns
        // None even if a similar line exists. This prevents anchor from
        // duplicating the similarity path in the fallback chain.
        let content = "fn process_request(x: i32) {}\n";
        let result = anchor_match(content, "fn process_requst(x: i32) {}", None, None);
        assert!(result.is_none());
    }

    #[test]
    fn anchor_match_multi_line_verifies_last_line() {
        // Multi-line anchor matching must check both the first and last
        // lines of the candidate region, not just the first.
        let content = "fn setup() {}\nfn process(x: i32) {}\nfn teardown() {}\nfn other() {}\n";
        // Target has a matching first line but wrong last line.
        let result = anchor_match(
            content,
            "fn process(x: i32) {}\nfn completely_wrong() {}",
            Some("fn setup() {}"),
            None,
        );
        assert!(result.is_none());
    }

    #[test]
    fn anchor_match_crlf_offset() {
        let content = "line1\r\nline2\r\nline3\r\n";
        let result = anchor_match(content, "line2", Some("line1"), None).unwrap();
        // "line1\r\n" is 7 bytes, so line2 starts at offset 7.
        assert_eq!(result.start_offset, 7);
    }

    /// Regression: anchor match on CRLF content must produce a matched_text
    /// that is an exact substring of the original content. The old code
    /// joined lines with "\n" which produced LF-only text, wrong for CRLF.
    #[test]
    fn anchor_match_crlf_matched_text_preserves_endings() {
        let content =
            "fn setup() {}\r\nfn proccess_data(x: i32) {}\r\nfn more() {}\r\nfn cleanup() {}\r\n";
        let result = anchor_match(
            content,
            "fn process_data(x: i32) {}\nfn more() {}",
            Some("fn setup() {}"),
            Some("fn cleanup() {}"),
        )
        .unwrap();
        // matched_text must be verifiable against the original content.
        let end = result.start_offset + result.matched_text.len();
        assert_eq!(
            &content[result.start_offset..end],
            result.matched_text,
            "matched_text must be an exact slice of the original content"
        );
        // And it should contain the CRLF between lines.
        assert!(
            result.matched_text.contains("\r\n"),
            "matched text should preserve CRLF line endings"
        );
    }

    /// Regression: anchor match on mixed line endings (some CRLF, some LF)
    /// must produce correct offsets for each line, not a global decision.
    #[test]
    fn anchor_match_mixed_endings_correct_offset() {
        let content = "header\r\nfn proccess(x: i32) {}\nfooter\n";
        let result = anchor_match(
            content,
            "fn process(x: i32) {}",
            Some("header"),
            Some("footer"),
        )
        .unwrap();
        // "header\r\n" is 8 bytes, so line 2 starts at offset 8.
        assert_eq!(result.start_offset, 8);
        let end = result.start_offset + result.matched_text.len();
        assert_eq!(&content[result.start_offset..end], result.matched_text);
    }

    #[test]
    fn resolve_with_fallback_exact_match() {
        let content = "fn hello() {}\n";
        let result = resolve_with_fallback(content, "fn hello()", None, None).unwrap();
        assert_eq!(result.strategy, MatchStrategy::Exact);
    }

    /// Regression: similarity matching on CRLF content must compute
    /// correct byte offsets. The old code used `offset += line.len() + 1`
    /// which hardcoded LF (1 byte), wrong for CRLF (2 bytes).
    #[test]
    fn resolve_with_fallback_similarity_crlf_offset() {
        let content = "fn alpha() {}\r\nfn process_requets(data: &str) {}\r\nfn gamma() {}\r\n";
        let result =
            resolve_with_fallback(content, "fn process_requests(data: &str) {}", None, None)
                .unwrap();
        assert_eq!(result.strategy, MatchStrategy::Similarity);
        // "fn alpha() {}\r\n" is 15 bytes, so the match starts at offset 15.
        assert_eq!(result.start_offset, 15);
        // Verify the offset points to the correct position in content.
        assert!(
            content[result.start_offset..].starts_with(&result.matched_text),
            "start_offset must point to matched_text in content"
        );
    }

    #[test]
    fn resolve_with_fallback_similarity_match() {
        // Without context, anchor matching is skipped (it requires at least
        // before_context or after_context to avoid degenerating into the same
        // Jaro-Winkler scan that similarity already does). The similarity
        // path catches the misspelled target instead.
        let content = "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n";
        let result = resolve_with_fallback(
            content,
            "fn process_requets(data: &str) -> Result<()> {",
            None,
            None,
        );
        let r = result.expect("similarity fallback should succeed");
        assert_eq!(r.strategy, MatchStrategy::Similarity);
    }

    #[test]
    fn resolve_with_fallback_structured_error() {
        let content = "fn alpha() {}\nfn beta() {}\n";
        let result = resolve_with_fallback(content, "fn completely_unrelated_xyz()", None, None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, EditErrorKind::NoMatch);
        assert!(err.message.contains("target not found"));
    }

    #[test]
    fn resolve_with_fallback_anchor_over_similarity() {
        // When context is provided, anchor matching fires before similarity
        // and should win.
        let content = "fn setup() {}\nfn proces_data(x: i32) {}\nfn cleanup() {}\n";
        let result = resolve_with_fallback(
            content,
            "fn process_data(x: i32) {}",
            Some("fn setup() {}"),
            Some("fn cleanup() {}"),
        );
        let r = result.expect("anchor fallback should succeed");
        assert_eq!(r.strategy, MatchStrategy::Anchor);
    }

    #[test]
    fn edit_error_serializes_to_json() {
        let err = EditError {
            kind: EditErrorKind::AmbiguousTarget,
            message: "found 3 matches".into(),
            suggestion: Some("use --nth to select one".into()),
            similar_targets: vec!["match1".into(), "match2".into()],
        };
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: EditError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.kind, EditErrorKind::AmbiguousTarget);
        assert_eq!(deserialized.similar_targets.len(), 2);
    }

    #[test]
    fn match_strategy_display() {
        assert_eq!(MatchStrategy::Exact.to_string(), "exact");
        assert_eq!(MatchStrategy::Anchor.to_string(), "anchor");
        assert_eq!(MatchStrategy::Similarity.to_string(), "similarity");
    }

    #[test]
    fn extract_identifiers_from_code() {
        let line = "fn process_request(data: &str) -> Result<()> {";
        let ids = extract_identifiers(line);
        assert!(ids.contains(&"process_request".to_string()));
        assert!(ids.contains(&"data".to_string()));
        assert!(ids.contains(&"str".to_string()));
        assert!(ids.contains(&"Result".to_string()));
    }

    #[test]
    fn validation_result_serializes() {
        let result = ValidationResult {
            valid: true,
            errors: vec![],
            warnings: vec!["some warning".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ValidationResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized.valid);
        assert_eq!(deserialized.warnings.len(), 1);
    }

    #[test]
    fn truncate_safe_on_multibyte_utf8() {
        // "café" has bytes: c(1) a(1) f(1) é(2) = 5 bytes, 4 chars.
        let s = "café";
        assert_eq!(s.len(), 5);
        // Truncate at 4 would land in the middle of 'é' (bytes 3-4).
        // Must not panic; should truncate before the multi-byte char.
        let t = truncate_str(s, 4);
        assert_eq!(t, "caf");
        // Truncate at 5 returns the whole string.
        assert_eq!(truncate_str(s, 5), "café");
        // Truncate at 3 is a clean boundary.
        assert_eq!(truncate_str(s, 3), "caf");
        // Truncate at 0.
        assert_eq!(truncate_str(s, 0), "");
    }

    #[test]
    fn validate_edit_toml_syntax_check() {
        let toml = "[database]\nhost = \"localhost\"\n";
        // Valid replacement.
        let result = validate_edit(toml, "localhost", "remotehost", Some("config.toml"));
        assert!(result.valid);

        // Invalid replacement (breaks TOML).
        let result = validate_edit(
            toml,
            "[database]",
            "not valid toml {{{{",
            Some("config.toml"),
        );
        assert!(!result.valid);
        assert!(result.errors[0].contains("invalid TOML"));
    }

    #[test]
    fn find_similar_targets_multi_word_pattern() {
        let content = "fn process_all_requests(data: &str) -> bool {\n    true\n}\n";
        // Multi-word pattern triggers whole-line matching path.
        let similar = find_similar_targets(content, "fn process_all_reqests(data: &str)", 3);
        assert!(
            !similar.is_empty(),
            "should find similar multi-word targets"
        );
    }

    #[test]
    fn resolve_fallback_multi_line_no_similarity() {
        // Multi-line targets skip the similarity path and go to error.
        let content = "fn alpha() {}\nfn beta() {}\nfn gamma() {}\n";
        let result = resolve_with_fallback(content, "fn alphax() {}\nfn betax() {}", None, None);
        assert!(
            result.is_err(),
            "multi-line targets without context should not match via similarity"
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, EditErrorKind::NoMatch);
    }

    #[test]
    fn resolve_with_fallback_multi_line_anchor_match() {
        // Multi-line target with context should resolve via anchor matching
        // through the full fallback chain (not just the anchor_match helper).
        let content = "fn header() {}\nfn proccess_data(x: i32) {\n    x + 1\n}\nfn footer() {}\n";
        let result = resolve_with_fallback(
            content,
            "fn process_data(x: i32) {\n    x + 1\n}",
            Some("fn header() {}"),
            Some("fn footer() {}"),
        );
        let r = result.expect("multi-line anchor should succeed");
        assert_eq!(r.strategy, MatchStrategy::Anchor);
        assert!(r.matched_text.contains("proccess_data"));
        assert!(r.matched_text.contains("x + 1"));
    }

    #[test]
    fn validate_edit_yml_extension() {
        // The .yml extension (not just .yaml) should trigger YAML validation.
        let yaml = "key: value\nlist:\n  - item1\n";
        // Valid replacement.
        let result = validate_edit(yaml, "value", "new_value", Some("config.yml"));
        assert!(result.valid);

        // Invalid replacement that breaks YAML structure.
        let result = validate_edit(yaml, "key: value", ":\n  :\n  - :", Some("config.yml"));
        assert!(
            !result.valid,
            ".yml extension should trigger YAML validation"
        );
        assert!(
            result.errors[0].contains("invalid YAML"),
            "expected YAML error, got: {}",
            result.errors[0]
        );
    }

    // -- validate_edit_nth (#1061) -------------------------------------------

    #[test]
    fn validate_edit_nth_replaces_only_nth_occurrence() {
        // Two occurrences of "val"; replacing only the 1st is valid JSON.
        let json = r#"{"a": "val", "b": "val"}"#;
        let result = validate_edit_nth(json, "val", "new", Some("data.json"), Some(1));
        assert!(result.valid, "nth=1 should produce valid JSON");
    }

    #[test]
    fn validate_edit_nth_none_replaces_all() {
        // nth=None replaces all occurrences (same as validate_edit).
        let content = "aXbXc";
        let result = validate_edit_nth(content, "X", "Y", None, None);
        assert!(result.valid);
    }

    #[test]
    fn validate_edit_nth_out_of_range() {
        // nth=5 but only 2 occurrences: should return invalid with descriptive error.
        let content = "aXbXc";
        let result = validate_edit_nth(content, "X", "Y", None, Some(5));
        assert!(
            !result.valid,
            "nth beyond occurrence count should be invalid"
        );
        assert!(
            result.errors[0].contains("occurrence 5 not found"),
            "error should mention the missing occurrence: {:?}",
            result.errors
        );
    }

    #[test]
    fn validate_edit_nth_zero_rejected() {
        // nth=0 is invalid (1-based indexing); must not truncate content.
        let content = r#"{"a": "val"}"#;
        let result = validate_edit_nth(content, "val", "X", Some("f.json"), Some(0));
        assert!(!result.valid);
        assert!(result.errors[0].contains("nth must be >= 1"));
    }

    #[test]
    fn validate_edit_nth_detects_invalid_json_for_single_occurrence() {
        // Replacing only the 1st "value" with a broken fragment is invalid.
        let json = r#"{"a": "value", "b": "value"}"#;
        let result = validate_edit_nth(json, "\"value\"", "broken}", Some("config.json"), Some(1));
        assert!(!result.valid, "nth=1 should detect broken JSON");
    }

    // -- anchor_match and resolve_with_fallback edge cases (#978) -----------

    #[test]
    fn anchor_match_after_only_context() {
        // after_context without before_context hits the code path where
        // before_context is None but after_context is checked.
        let content = "fn setup() {}\nfn proccess_data(x: i32) {}\nfn cleanup() {}\n";
        let result = anchor_match(
            content,
            "fn process_data(x: i32) {}",
            None,
            Some("fn cleanup() {}"),
        );
        let r = result.expect("after-only context should find anchor match");
        assert_eq!(r.strategy, MatchStrategy::Anchor);
        assert!(r.matched_text.contains("proccess_data"));
    }

    #[test]
    fn anchor_match_multiple_candidates_returns_first() {
        // Target appears (fuzzily) twice; anchor_match returns the first match.
        let content = "fn header() {}\nfn proccess(x: i32) {}\nfn middle() {}\nfn proccess(y: bool) {}\nfn footer() {}\n";
        let result = anchor_match(
            content,
            "fn process(x: i32) {}",
            Some("fn header() {}"),
            Some("fn middle() {}"),
        );
        let r = result.expect("should match first candidate");
        assert_eq!(r.strategy, MatchStrategy::Anchor);
        // The first candidate contains "x: i32", not "y: bool".
        assert!(
            r.matched_text.contains("x: i32"),
            "should match first occurrence, got: {}",
            r.matched_text
        );
    }

    #[test]
    fn resolve_with_fallback_empty_content() {
        let result = resolve_with_fallback("", "fn hello()", None, None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, EditErrorKind::NoMatch);
    }

    #[test]
    fn anchor_match_multiline_before_context() {
        // Multi-line before_context should use the last line for matching,
        // not the entire multi-line string.
        let content = "fn setup() {}\nfn target() {}\nfn cleanup() {}\n";
        let result = anchor_match(
            content,
            "fn target() {}",
            Some("fn other() {}\nfn setup() {}"),
            None,
        );
        assert!(
            result.is_some(),
            "anchor_match should match using the last line of multi-line before_context"
        );
        assert_eq!(result.unwrap().matched_text, "fn target() {}");
    }

    #[test]
    fn anchor_match_multiline_after_context() {
        // Multi-line after_context should use the first line for matching.
        let content = "fn setup() {}\nfn target() {}\nfn cleanup() {}\n";
        let result = anchor_match(
            content,
            "fn target() {}",
            None,
            Some("fn cleanup() {}\nfn extra() {}"),
        );
        assert!(
            result.is_some(),
            "anchor_match should match using the first line of multi-line after_context"
        );
        assert_eq!(result.unwrap().matched_text, "fn target() {}");
    }

    // Static assertions: types must be Send + Sync.
    const _: () = {
        fn _assert<T: Send + Sync>() {}
        let _ = _assert::<EditError>;
        let _ = _assert::<EditErrorKind>;
        let _ = _assert::<ValidationResult>;
        let _ = _assert::<AnchorMatchResult>;
        let _ = _assert::<MatchStrategy>;
    };
}
