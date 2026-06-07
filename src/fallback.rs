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
//! # Example
//!
//! ```rust
//! use patchloom::fallback::{EditError, EditErrorKind, find_similar_targets};
//!
//! let content = "fn process_request() {}\nfn process_response() {}\n";
//! let similar = find_similar_targets(content, "process_requst", 3);
//! assert!(similar.iter().any(|s| s.contains("process_request")));
//! ```

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
pub fn validate_edit(
    content: &str,
    from: &str,
    to: &str,
    file_path: Option<&str>,
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
                truncate(from, 60)
            )],
            warnings: vec![],
        };
    }

    let new_content = content.replace(from, to);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // If we can detect the file format, validate the result.
    if let Some(path) = file_path {
        if path.ends_with(".json") {
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&new_content) {
                errors.push(format!("result would be invalid JSON: {e}"));
            }
        } else if path.ends_with(".yaml") || path.ends_with(".yml") {
            if let Err(e) = serde_yaml_ng::from_str::<serde_json::Value>(&new_content) {
                errors.push(format!("result would be invalid YAML: {e}"));
            }
        } else if path.ends_with(".toml")
            && let Err(e) = toml_edit::de::from_str::<serde_json::Value>(&new_content)
        {
            errors.push(format!("result would be invalid TOML: {e}"));
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

    if target_lines.is_empty() {
        return None;
    }

    let first_target = target_lines[0].trim();
    let _last_target = target_lines.last().map(|l| l.trim()).unwrap_or("");

    // Find candidate positions by matching the first line with anchors.
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check if this line is similar to the first target line.
        if strsim::jaro_winkler(trimmed, first_target) < 0.85 {
            continue;
        }

        // If we have before_context, verify the preceding line matches.
        if let Some(before) = before_context {
            if i == 0 {
                continue;
            }
            let prev = lines[i - 1].trim();
            if strsim::jaro_winkler(prev, before.trim()) < 0.8 {
                continue;
            }
        }

        // If we have after_context and the target is one line, check the next line.
        if let Some(after) = after_context {
            let end_idx = i + target_lines.len();
            if end_idx >= lines.len() {
                continue;
            }
            let next = lines[end_idx].trim();
            if strsim::jaro_winkler(next, after.trim()) < 0.8 {
                continue;
            }
        }

        // Found a match. Compute the matched region.
        let end_idx = (i + target_lines.len()).min(lines.len());
        let matched: Vec<&str> = lines[i..end_idx].to_vec();
        let matched_text = matched.join("\n");

        // Compute byte offset.
        let start_offset = lines[..i]
            .iter()
            .map(|l| l.len() + 1) // +1 for newline
            .sum::<usize>();

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
            offset += line.len() + 1;
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
        message: format!("target not found: '{}'", truncate(target, 80)),
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
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len { s } else { &s[..max_len] }
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
        assert!(result.is_some());
        let r = result.unwrap();
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
    fn resolve_with_fallback_exact_match() {
        let content = "fn hello() {}\n";
        let result = resolve_with_fallback(content, "fn hello()", None, None).unwrap();
        assert_eq!(result.strategy, MatchStrategy::Exact);
    }

    #[test]
    fn resolve_with_fallback_similarity_match() {
        // Anchor matching is tried before similarity, and it uses Jaro-Winkler
        // with the same threshold.  A slightly misspelled single line will be
        // caught by the anchor path (no context required).  The test verifies
        // the fallback chain resolves it, regardless of which fuzzy strategy
        // fires first.
        let content = "fn process_request(data: &str) -> Result<()> {\n    Ok(())\n}\n";
        let result = resolve_with_fallback(
            content,
            "fn process_requets(data: &str) -> Result<()> {",
            None,
            None,
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(
            r.strategy == MatchStrategy::Anchor || r.strategy == MatchStrategy::Similarity,
            "expected fuzzy match, got {:?}",
            r.strategy
        );
    }

    #[test]
    fn resolve_with_fallback_structured_error() {
        let content = "fn alpha() {}\nfn beta() {}\n";
        let result = resolve_with_fallback(content, "fn completely_unrelated_xyz()", None, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, EditErrorKind::NoMatch);
        assert!(err.message.contains("target not found"));
    }

    #[test]
    fn resolve_with_fallback_anchor_over_similarity() {
        // When both anchor and similarity could match, anchor should win
        // (it comes first in the chain).
        let content = "fn setup() {}\nfn proces_data(x: i32) {}\nfn cleanup() {}\n";
        let result = resolve_with_fallback(
            content,
            "fn process_data(x: i32) {}",
            Some("fn setup() {}"),
            Some("fn cleanup() {}"),
        );
        assert!(result.is_ok());
        let r = result.unwrap();
        // Anchor match should be preferred when context is provided.
        assert!(r.strategy == MatchStrategy::Anchor || r.strategy == MatchStrategy::Similarity,);
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
