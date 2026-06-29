// Parameter structs for MCP tools with custom handler logic.
//
// Simple 1:1 Operation-mapped tools (doc_set, doc_delete, doc_merge, doc_append,
// doc_prepend, doc_ensure, doc_delete_where, doc_update, doc_move, read_file,
// md_upsert_bullet, md_table_append, md_replace_section, md_insert_after_heading,
// md_insert_before_heading, move_file, append_file, create_file, delete_file) are
// auto-generated from MCP_TOOL_REGISTRY; they use the Operation variant's schema
// directly and do not need dedicated params structs.

use super::*;
use serde::Deserialize;

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
pub(crate) struct MdLintAgentsParams {
    /// Markdown file path (typically AGENTS.md).
    pub path: String,
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
    /// Dedent: remove leading whitespace. Values: "auto", "tab", or a number (e.g. "4").
    #[serde(default)]
    pub dedent: Option<String>,
    /// Indent: add leading whitespace. Values: "tab" or a number (e.g. "4").
    #[serde(default)]
    pub indent: Option<String>,
    /// Restrict dedent/indent to a line range (1-based inclusive, e.g. "10:50").
    #[serde(default)]
    pub lines: Option<String>,
    /// Collapse consecutive blank lines into a single blank line.
    #[serde(default)]
    pub collapse_blanks: Option<bool>,
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

// ---------------------------------------------------------------------------
// AST parameter types
// ---------------------------------------------------------------------------

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstListParams {
    /// File or directory to list symbols from.
    pub path: String,
    /// Filter by symbol kind (comma-separated: function,struct,enum,class,method,trait,impl,const,type,interface,module).
    pub kind: Option<String>,
    /// Language hint (overrides extension detection). E.g. "rs", "py", "go", "ts", "java", "c", "cpp".
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstReadParams {
    /// File to read from.
    pub path: String,
    /// Symbol name (e.g. "run" or "Server::start").
    pub symbol: String,
    /// Number of context lines before/after the symbol.
    #[serde(default)]
    pub context: usize,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstRenameParams {
    /// The identifier to rename.
    pub old_name: String,
    /// The new identifier name.
    pub new_name: String,
    /// File or directory to rename in.
    pub path: String,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstValidateParams {
    /// File or directory to validate syntax.
    pub path: String,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstSearchParams {
    /// Tree-sitter S-expression query, or a code pattern (with pattern=true).
    pub query: String,
    /// File or directory to search.
    pub path: String,
    /// Treat the query as a code pattern with meta-variables ($VAR, $$$MULTI).
    #[serde(default)]
    pub pattern: bool,
    /// Language hint (required for pattern mode).
    pub lang: Option<String>,
    /// Maximum number of results.
    pub max_results: Option<usize>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstRefsParams {
    /// Symbol name to find references for.
    pub symbol: String,
    /// File or directory to search.
    pub path: String,
    /// Include the definition site in results.
    #[serde(default)]
    pub include_def: bool,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstDepsParams {
    /// File or directory to analyze.
    pub path: String,
    /// Show reverse dependencies (what imports this file).
    #[serde(default)]
    pub reverse: bool,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstMapParams {
    /// Directory to map.
    pub path: String,
    /// Maximum approximate token count for output.
    #[serde(default = "default_map_max_tokens")]
    pub max_tokens: usize,
    /// Boost symbols from these files (comma-separated paths).
    #[serde(default)]
    pub focus: Vec<String>,
    /// Boost these symbol names (comma-separated).
    #[serde(default)]
    pub boost: Vec<String>,
}

#[cfg(feature = "ast")]
fn default_map_max_tokens() -> usize {
    1024
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstDiffParams {
    /// File to diff.
    pub path: String,
    /// Git ref for the "old" version (default: HEAD).
    #[serde(default = "default_head")]
    pub from: String,
    /// Git ref for the "new" version (default: working tree).
    pub to: Option<String>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
fn default_head() -> String {
    "HEAD".to_string()
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstImpactParams {
    /// Symbol name to analyze.
    pub symbol: String,
    /// Directory to scan for references.
    pub path: String,
    /// Maximum traversal depth (1 = direct refs only).
    #[serde(default = "default_impact_depth")]
    pub depth: usize,
}

#[cfg(feature = "ast")]
fn default_impact_depth() -> usize {
    3
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstReplaceParams {
    /// File containing the symbol.
    pub path: String,
    /// Symbol name to scope the replacement to.
    pub symbol: String,
    /// Text or regex pattern to find.
    pub from: String,
    /// Replacement text.
    pub to: String,
    /// Treat --from as a regex pattern.
    #[serde(default)]
    pub regex: bool,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstInsertParams {
    /// File to insert code into.
    pub path: String,
    /// Code to insert.
    pub content: String,
    /// Module/impl/struct to insert into (mutually exclusive with after/before).
    #[serde(default)]
    pub inside: Option<String>,
    /// Insert after this symbol (mutually exclusive with inside/before).
    #[serde(default)]
    pub after: Option<String>,
    /// Insert before this symbol (mutually exclusive with inside/after).
    #[serde(default)]
    pub before: Option<String>,
    /// Position within `inside`: "start" or "end" (default).
    #[serde(default)]
    pub position: Option<String>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstWrapParams {
    /// File to modify.
    pub path: String,
    /// Symbols to wrap (mutually exclusive with lines).
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Line range to wrap (e.g. "10:50", mutually exclusive with symbols).
    #[serde(default)]
    pub lines: Option<String>,
    /// The wrapping construct (e.g. "mod foo", "impl Bar", "#[cfg(test)]").
    pub wrapper: String,
    /// Content to insert at the top of the wrapped block.
    #[serde(default)]
    pub preamble: Option<String>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstImportsParams {
    /// File to modify.
    pub path: String,
    /// Import statements to add (idempotent; skips if already present).
    #[serde(default)]
    pub add: Option<Vec<String>>,
    /// Import statements to remove.
    #[serde(default)]
    pub remove: Option<Vec<String>>,
    /// Deduplicate imports.
    #[serde(default)]
    pub dedupe: bool,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstReorderParams {
    /// File to reorder symbols in.
    pub path: String,
    /// Scope to reorder within (module/impl). Default: top-level.
    #[serde(default)]
    pub inside: Option<String>,
    /// Ordering: "alphabetical", "reverse", "kind-first", or array of names.
    pub order: serde_json::Value,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstGroupParams {
    /// File to modify.
    pub path: String,
    /// Module name to create or append to.
    pub module: String,
    /// Symbols to move into the module.
    pub symbols: Vec<String>,
    /// Code to insert at the top of the module (e.g., `use super::*;`).
    #[serde(default)]
    pub preamble: Option<String>,
    /// Where to place new module: "first-symbol" (default), "end", or "after:<symbol>".
    #[serde(default)]
    pub position: Option<String>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstMoveParams {
    /// Source file.
    pub path: String,
    /// Target file.
    pub target: String,
    /// Symbols to move.
    pub symbols: Vec<String>,
    /// Position in target: "end" (default), "start", "after:<symbol>", "before:<symbol>".
    #[serde(default)]
    pub position: Option<String>,
    /// Content to prepend to target file if creating it.
    #[serde(default)]
    pub target_prepend: Option<String>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstExtractToFileParams {
    /// Source file containing the symbol.
    pub source: String,
    /// Name of the symbol to extract.
    pub symbol: String,
    /// Destination file path.
    pub target: String,
    /// Text to leave in place of the extracted block.
    #[serde(default)]
    pub replacement: Option<String>,
    /// If true (default), remove wrapper and un-indent for modules.
    #[serde(default)]
    pub unwrap: Option<bool>,
    /// Content to prepend to the target file.
    #[serde(default)]
    pub prepend: Option<String>,
    /// Overwrite target if it exists.
    #[serde(default)]
    pub force: bool,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub(crate) struct AstSplitParams {
    /// The file to split.
    pub source: String,
    /// Target file specs.
    pub targets: Vec<AstSplitTargetParam>,
    /// Symbols to keep in the source file.
    #[serde(default)]
    pub keep_in_source: Vec<String>,
    /// Text to append to source after split (e.g., `mod` declarations).
    #[serde(default)]
    pub source_suffix: Option<String>,
    /// Text to prepend to source after split.
    #[serde(default)]
    pub source_prefix: Option<String>,
    /// Error if any symbol is unaccounted for (default: true).
    #[serde(default)]
    pub require_exhaustive: Option<bool>,
    /// Language hint.
    pub lang: Option<String>,
}

#[cfg(feature = "ast")]
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AstSplitTargetParam {
    /// Target file path.
    pub path: String,
    /// Symbols to move into this file.
    pub symbols: Vec<String>,
    /// Content to prepend to this target file.
    #[serde(default)]
    pub prepend: Option<String>,
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
