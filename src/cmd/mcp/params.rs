use super::*;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocSetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Selector for the value to set (e.g., `version`, `db.pool`, `items[0].name`).
    pub selector: String,
    /// Value to set (string, number, boolean, object, or array).
    pub value: serde_json::Value,
    /// Roll back all writes when format/validate lifecycle steps fail.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocDeleteParams {
    /// File path.
    pub path: String,
    /// Selector for the value to delete.
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocMergeParams {
    /// File path.
    pub path: String,
    /// Object to deep-merge into the document root.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocArrayParams {
    /// File path.
    pub path: String,
    /// Selector pointing to the target array.
    pub selector: String,
    /// Value to append or prepend.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocEnsureParams {
    /// File path.
    pub path: String,
    /// Selector for the value to ensure exists.
    pub selector: String,
    /// Value to set only if the selector path is missing.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocUpdateParams {
    /// File path.
    pub path: String,
    /// Wildcard selector for items to update (e.g., `items[*]`).
    pub selector: String,
    /// Value to set on each matched item.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocMoveParams {
    /// File path.
    pub path: String,
    /// Source selector path to move from.
    pub from: String,
    /// Destination selector path to move to.
    pub to: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocDeleteWhereParams {
    /// File path.
    pub path: String,
    /// Selector pointing to the target array.
    pub selector: String,
    /// Predicate for items to delete (e.g., "name=obsolete").
    pub predicate: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct ReplaceParams {
    /// File path.
    pub path: String,
    /// Text to find.
    pub from: String,
    /// Text to replace with. Mutually exclusive with insert_before/insert_after.
    pub to: Option<String>,
    /// Insert text before each match instead of replacing. Mutually exclusive with to/insert_after.
    pub insert_before: Option<String>,
    /// Insert text after each match instead of replacing. Mutually exclusive with to/insert_before.
    pub insert_after: Option<String>,
    /// Use regex mode for the `from` pattern.
    #[serde(default)]
    pub regex: bool,
    /// Replace only the Nth match (1-based). Default: replace all.
    pub nth: Option<usize>,
    /// Case-insensitive matching.
    #[serde(default)]
    pub case_insensitive: bool,
    /// Enable multiline matching (dot matches newlines in regex mode).
    #[serde(default)]
    pub multiline: bool,
    /// Return success even if no matches found (idempotent mode).
    #[serde(default)]
    pub if_exists: bool,
    /// Replace the entire line containing each match, not just the matched span.
    /// When combined with to="" this deletes matching lines.
    #[serde(default)]
    pub whole_line: bool,
    /// Restrict matching to a line range (e.g. "10:50"). Requires whole_line=true.
    pub range: Option<String>,
    /// Match only at word boundaries. Prevents 'SetupFile' from matching
    /// inside 'BenchSetupFile'. Auto-escapes regex metacharacters.
    #[serde(default)]
    pub word_boundary: bool,
    /// Context line(s) before the target. Enables anchor-based fallback
    /// matching when the exact `from` text is not found.
    pub before_context: Option<String>,
    /// Context line(s) after the target. Enables anchor-based fallback
    /// matching when the exact `from` text is not found.
    pub after_context: Option<String>,
    /// Roll back all writes when format/validate lifecycle steps fail.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocGetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Selector for the value to read (e.g., "version", "db.pool").
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct ReadFileParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Optional line range (e.g., "10:20"). Omit to read the entire file.
    pub lines: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdLintAgentsParams {
    /// Markdown file path (typically AGENTS.md).
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdUpsertBulletParams {
    /// Markdown file path.
    pub path: String,
    /// Heading to find (e.g., "## Rules").
    pub heading: String,
    /// Bullet text to add (e.g., "- New rule"). Skipped if already present.
    pub bullet: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdTableAppendParams {
    /// Markdown file path.
    pub path: String,
    /// Heading above the target table.
    pub heading: String,
    /// Table row to append (e.g., "| col1 | col2 |").
    pub row: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdReplaceSectionParams {
    /// Markdown file path.
    pub path: String,
    /// Heading of the section to replace.
    pub heading: String,
    /// New content for the section body.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdInsertParams {
    /// Markdown file path.
    pub path: String,
    /// Heading to target (e.g., "## Changelog").
    pub heading: String,
    /// Content to insert.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct MdMoveSectionParams {
    /// Source file path containing the section to move.
    pub path: String,
    /// Heading of the section to move (e.g., "## FAQ").
    pub heading: String,
    /// Destination file path. Omit for same-file reorder.
    #[serde(default)]
    pub to: Option<String>,
    /// Insert before this heading at the destination.
    #[serde(default)]
    pub before: Option<String>,
    /// Insert after this heading at the destination.
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct TidyParams {
    /// File path to normalize.
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct FileRenameParams {
    /// Source file path (relative to working directory).
    pub from: String,
    /// Destination file path (relative to working directory).
    pub to: String,
    /// If true, overwrite the destination if it already exists.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AppendFileParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Content to append to the file.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct CreateFileParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Content to write to the new file.
    pub content: String,
    /// If true, overwrite an existing file instead of failing.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DeleteFileParams {
    /// File path (relative to working directory).
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct PatchParams {
    pub diff: String,
    #[serde(default)]
    pub on_stale: crate::ops::patch::OnStale,
    #[serde(default)]
    pub allow_conflicts: bool,
    /// Roll back all writes when format/validate lifecycle steps fail.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct SearchParams {
    /// Pattern to search for.
    pub pattern: String,
    /// Paths to search in (defaults to current directory).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Treat pattern as a literal string instead of regex.
    #[serde(default)]
    pub literal: bool,
    /// Case-insensitive matching.
    #[serde(default)]
    pub case_insensitive: bool,
    /// Lines of context around matches (shorthand for before_context + after_context).
    pub context: Option<usize>,
    /// Lines of context before each match.
    pub before_context: Option<usize>,
    /// Lines of context after each match.
    pub after_context: Option<usize>,
    /// Only return file paths with matches (not match details).
    #[serde(default)]
    pub files_with_matches: bool,
    /// Only return match counts per file.
    #[serde(default)]
    pub count: bool,
    /// Enable multiline matching (dot matches newlines in regex mode).
    #[serde(default)]
    pub multiline: bool,
    /// Show lines that do NOT match the pattern.
    #[serde(default)]
    pub invert_match: bool,
    /// Assert that the total match count equals N. Returns exit code 0 if exact, 2 otherwise.
    pub assert_count: Option<usize>,
    /// Glob include patterns (may be repeated). Supports parity with CLI --glob and library SearchOptions.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Exclude glob patterns (in addition to ignore files).
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    /// Custom ignore filenames (e.g. .blineignore).
    #[serde(default)]
    pub custom_ignore_filenames: Vec<String>,
    /// Max detailed results (0 = unlimited).
    #[serde(default)]
    pub max_results: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocQueryParams {
    /// Query action: "has" (check existence), "keys" (list keys), "len" (count),
    /// "select" (filter array), or "flatten" (list all paths).
    pub action: String,
    /// File path (relative to working directory).
    pub path: String,
    /// Selector to query. Required for has/keys/len/select; ignored for flatten.
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct DocDiffParams {
    /// First file path.
    pub file_a: String,
    /// Second file path.
    pub file_b: String,
}

// ---------------------------------------------------------------------------
// Homogeneous batch parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct BatchReplaceParams {
    /// File paths to apply the replacement to (relative to working directory).
    pub files: Vec<String>,
    /// Text to find in each file.
    pub from: String,
    /// Text to replace with.
    pub to: String,
    /// Use regex mode for the `from` pattern.
    #[serde(default)]
    pub regex: bool,
    /// Case-insensitive matching.
    #[serde(default)]
    pub case_insensitive: bool,
    /// Enable multiline matching (dot matches newlines in regex mode).
    #[serde(default)]
    pub multiline: bool,
    /// Match only at word boundaries. Prevents 'SetupFile' from matching
    /// inside 'BenchSetupFile'. Auto-escapes regex metacharacters.
    #[serde(default)]
    pub word_boundary: bool,
    /// Roll back all writes when format/validate lifecycle steps fail.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct BatchTidyParams {
    /// File paths to normalize (relative to working directory).
    pub files: Vec<String>,
    /// Roll back all writes when format/validate lifecycle steps fail.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}

/// No-parameter wrapper for tools that need no input (e.g., git_status).
/// Required because rmcp 1.8+ validates that `inputSchema` has a root
/// `type: "object"` field, which `serde_json::Value` does not provide.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct EmptyParams {}

/// Parameters for executing a full multi-step transaction plan.
/// This is the MCP equivalent of `patchloom tx`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct ExecutePlanParams {
    /// Full inline plan object (preferred for agents; same schema as CLI tx plans).
    /// Must contain at minimum "version" and "operations".
    pub plan: Option<Plan>,
    /// Path (relative to cwd) to a plan file (JSON, YAML, or TOML).
    /// Used only if `plan` is not provided.
    pub plan_path: Option<String>,
    /// Enforce strict mode (rollback on format/validate failure). Defaults to true.
    /// Overrides plan's strict field if provided.
    #[serde(default = "default_strict_true")]
    pub strict: bool,
}
