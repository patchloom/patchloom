//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt, tool, tool_router};
use std::path::PathBuf;

use crate::cli::global::GlobalFlags;
use crate::containment::PathGuard;
use crate::exit;
use crate::plan::{Operation, Plan};

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

fn default_strict_true() -> bool {
    true
}

mod params {
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
} // close params mod

// Re-export so the rest of the file (impls, tests) can use the names without qualification.
use params::*;

use params::ExecutePlanParams;

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Maximum size for content/diff fields (10 MiB).
const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum size for pattern/selector string fields (1 MiB).
const MAX_PARAM_BYTES: usize = 1024 * 1024;

/// Maximum number of files in a batch operation.
const MAX_BATCH_FILES: usize = 1000;

/// Maximum nesting depth for JSON value parameters.
const MAX_JSON_DEPTH: usize = 64;

fn validate_size(field: &str, value: &str, max_bytes: usize) -> Result<(), McpError> {
    if value.len() > max_bytes {
        return Err(McpError::invalid_params(
            format!(
                "{field} exceeds maximum size ({} bytes, limit {max_bytes})",
                value.len(),
            ),
            None,
        ));
    }
    Ok(())
}

fn validate_content_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_CONTENT_BYTES)
}

fn validate_param_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_PARAM_BYTES)
}

fn validate_json_depth(field: &str, value: &serde_json::Value) -> Result<(), McpError> {
    fn measure(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Array(arr) => 1 + arr.iter().map(measure).max().unwrap_or(0),
            serde_json::Value::Object(obj) => 1 + obj.values().map(measure).max().unwrap_or(0),
            _ => 0,
        }
    }
    let depth = measure(value);
    if depth > MAX_JSON_DEPTH {
        return Err(McpError::invalid_params(
            format!("{field} exceeds maximum nesting depth ({depth}, limit {MAX_JSON_DEPTH})"),
            None,
        ));
    }
    Ok(())
}

fn validate_batch_size(field: &str, count: usize) -> Result<(), McpError> {
    if count > MAX_BATCH_FILES {
        return Err(McpError::invalid_params(
            format!("{field} exceeds maximum batch size ({count}, limit {MAX_BATCH_FILES})"),
            None,
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Path containment (delegated to crate::containment::PathGuard)
// ---------------------------------------------------------------------------

/// Validate all paths in a list of operations.
/// Checks both syntactic containment and symlink resolution.
#[cfg(test)]
fn validate_operation_paths(
    operations: &[Operation],
    cwd: &std::path::Path,
) -> Result<(), McpError> {
    let guard = crate::containment::PathGuard::new(
        cwd.to_path_buf(),
        crate::containment::AbsolutePathPolicy::Reject,
    )
    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
    for op in operations {
        for path in crate::plan::declared_paths(op) {
            guard
                .check_path(path)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        }
        if let Operation::PatchApply { diff, .. } = op {
            // Validate paths embedded in the unified diff text (special case
            // because paths live inside the diff payload, not as top-level
            // declared fields).
            let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
                McpError::invalid_params(
                    format!("failed to parse diff for path validation: {e}"),
                    None,
                )
            })?;
            for pf in &patch_files {
                guard
                    .check_path(&pf.path)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchloomService {
    tool_router: ToolRouter<Self>,
    /// Path guard: validates and canonicalizes paths relative to cwd.
    path_guard: crate::containment::PathGuard,
    /// Optional path for logging MCP tool calls as JSONL.
    /// Set via `--log <path>` or `PATCHLOOM_MCP_LOG` env var.
    call_log: Option<PathBuf>,
}

impl PatchloomService {
    pub fn new(cwd: PathBuf, log_flag: Option<String>) -> anyhow::Result<Self> {
        let path_guard = crate::containment::PathGuard::new(
            cwd.clone(),
            crate::containment::AbsolutePathPolicy::Reject,
        )
        .map_err(|e| anyhow::anyhow!("failed to initialize path guard: {e}"))?;
        // --log flag takes precedence over PATCHLOOM_MCP_LOG env var.
        let call_log = log_flag
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("PATCHLOOM_MCP_LOG").map(PathBuf::from));
        Ok(Self {
            tool_router: Self::tool_router(),
            path_guard,
            call_log,
        })
    }

    /// Validate a path for both syntactic containment and symlink resolution.
    /// Combines the two checks that must always be called together.
    fn check_path(&self, path: &str) -> Result<(), McpError> {
        self.path_guard
            .check_path(path)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(())
    }

    /// The workspace root directory (non-canonicalized).
    fn cwd(&self) -> &std::path::Path {
        self.path_guard.root()
    }

    /// Write a JSONL log entry for a tool call if logging is enabled.
    fn log_tool_call(
        &self,
        tool: &str,
        duration_ms: u64,
        result: &Result<CallToolResult, McpError>,
    ) {
        let Some(ref log_path) = self.call_log else {
            return;
        };
        let (ok, error) = match result {
            Ok(r) => (!r.is_error.unwrap_or(false), None),
            Err(e) => (false, Some(format!("{e}"))),
        };
        let ts = {
            let d = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            d.as_millis()
        };
        let mut entry = serde_json::json!({
            "ts": ts,
            "tool": tool,
            "duration_ms": duration_ms,
            "ok": ok,
        });
        if let Some(err_msg) = error {
            entry["error"] = serde_json::Value::String(err_msg);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{entry}");
        }
    }

    /// Helper to execute one or more operations as a plan.
    /// Reduces repetitive boilerplate across single-op and batch MCP tools.
    fn run_ops(
        &self,
        ops: Vec<Operation>,
        strict: Option<bool>,
    ) -> Result<CallToolResult, McpError> {
        execute_plan_validated(
            make_plan_strict(ops, strict),
            self.cwd(),
            Some(&self.path_guard),
        )
    }
}

/// Execute a plan whose paths have already been validated by the caller.
///
/// Each individual MCP tool validates paths via `check_path` before calling
/// this function, so no redundant validation is needed here.
fn execute_plan_validated(
    plan: Plan,
    cwd: &std::path::Path,
    guard: Option<&PathGuard>,
) -> Result<CallToolResult, McpError> {
    let (code, json) = crate::cmd::tx::execute_plan_direct(plan, cwd, guard)
        .map_err(|e| McpError::internal_error(format!("plan execution failed: {e}"), None))?;

    exit_code_to_result(code, &json, "Operation completed successfully.")
}

/// Convert an exit code + output string into a `CallToolResult`.
fn exit_code_to_result(
    code: u8,
    output: &str,
    success_fallback: &str,
) -> Result<CallToolResult, McpError> {
    if code == exit::SUCCESS {
        let msg = if output.trim().is_empty() {
            success_fallback.to_string()
        } else {
            output.trim().to_string()
        };
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    } else {
        let msg = if output.trim().is_empty() {
            format!("Operation failed with exit code {code}.")
        } else {
            output.trim().to_string()
        };
        Ok(CallToolResult::error(vec![Content::text(msg)]))
    }
}

/// Execute a read-only doc operation directly (no subprocess).
fn doc_readonly(action: &crate::cmd::doc::DocAction) -> Result<CallToolResult, McpError> {
    let (output, code) =
        crate::cmd::doc::execute_with_mode(action, crate::cmd::doc::OutputMode::Json)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    exit_code_to_result(code, &output, "No results.")
}

fn make_plan(operations: Vec<Operation>) -> Plan {
    make_plan_strict(operations, None)
}

fn make_plan_strict(operations: Vec<Operation>, strict: Option<bool>) -> Plan {
    Plan {
        version: crate::plan::SCHEMA_VERSION.to_string(),
        cwd: None,
        write_policy: None,
        strict,
        operations,
        format: None,
        validate: None,
    }
}

#[tool_router]
impl PatchloomService {
    #[tool(
        description = "Set a value in a JSON, YAML, or TOML file. Parser-backed, preserves comments. Use dot notation for nested paths. Example: {\"path\": \"package.json\", \"selector\": \"version\", \"value\": \"2.0.0\"}"
    )]
    async fn doc_set(
        &self,
        Parameters(p): Parameters<DocSetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocSet {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }],
            Some(p.strict),
        )
    }

    #[tool(
        description = "Delete a value from a JSON, YAML, or TOML file. Example: {\"path\": \"package.json\", \"selector\": \"scripts.test\"}"
    )]
    async fn doc_delete(
        &self,
        Parameters(p): Parameters<DocDeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        self.run_ops(
            vec![Operation::DocDelete {
                path: p.path,
                selector: p.selector,
            }],
            None,
        )
    }

    #[tool(
        description = "Deep-merge an object into a JSON, YAML, or TOML document. Example: {\"path\": \"config.yaml\", \"value\": {\"server\": {\"port\": 8080}}}"
    )]
    async fn doc_merge(
        &self,
        Parameters(p): Parameters<DocMergeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocMerge {
                path: p.path,
                value: p.value,
            }],
            None,
        )
    }

    #[tool(
        description = "Append a value to an array in a JSON, YAML, or TOML file. Example: {\"path\": \"package.json\", \"selector\": \"dependencies\", \"value\": \"new-pkg\"}"
    )]
    async fn doc_append(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocAppend {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }],
            None,
        )
    }

    #[tool(
        description = "Prepend a value to an array in a JSON, YAML, or TOML file. Inserts at position 0."
    )]
    async fn doc_prepend(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocPrepend {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }],
            None,
        )
    }

    #[tool(
        description = "Set a value in JSON/YAML/TOML only if it does not already exist. Idempotent: no-op if present. Example: {\"path\": \"config.json\", \"selector\": \"debug\", \"value\": false}"
    )]
    async fn doc_ensure(
        &self,
        Parameters(p): Parameters<DocEnsureParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocEnsure {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }],
            None,
        )
    }

    #[tool(
        description = "Remove array items matching a predicate from JSON/YAML/TOML. Example: {\"path\": \"config.yaml\", \"selector\": \"users\", \"predicate\": \"role=admin\"}"
    )]
    async fn doc_delete_where(
        &self,
        Parameters(p): Parameters<DocDeleteWhereParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_param_size("predicate", &p.predicate)?;
        self.run_ops(
            vec![Operation::DocDeleteWhere {
                path: p.path,
                selector: p.selector,
                predicate: p.predicate,
            }],
            None,
        )
    }

    #[tool(
        description = "Update all items matching a wildcard selector in a JSON, YAML, or TOML file. Example: {\"path\": \"config.yaml\", \"selector\": \"servers[*].port\", \"value\": 8080}"
    )]
    async fn doc_update(
        &self,
        Parameters(p): Parameters<DocUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        validate_json_depth("value", &p.value)?;
        self.run_ops(
            vec![Operation::DocUpdate {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }],
            None,
        )
    }

    #[tool(
        description = "Move/rename a key in a JSON, YAML, or TOML file. Example: {\"path\": \"config.json\", \"from\": \"old_name\", \"to\": \"new_name\"}"
    )]
    async fn doc_move(
        &self,
        Parameters(p): Parameters<DocMoveParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("from", &p.from)?;
        validate_param_size("to", &p.to)?;
        self.run_ops(
            vec![Operation::DocMove {
                path: p.path,
                from: p.from,
                to: p.to,
            }],
            None,
        )
    }

    #[tool(
        description = "Read a value from a JSON, YAML, or TOML file by selector. Example: {\"path\": \"package.json\", \"selector\": \"version\"}"
    )]
    async fn doc_get(
        &self,
        Parameters(p): Parameters<DocGetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("selector", &p.selector)?;
        let abs = self.cwd().join(&p.path);
        let action = crate::cmd::doc::DocAction::Get {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Query a JSON, YAML, or TOML file. Actions: \"has\" (check if selector exists, returns true/false), \"keys\" (list object keys at selector), \"len\" (count items at selector), \"select\" (filter array by predicate selector), \"flatten\" (list all leaf paths and values). Example: {\"action\": \"has\", \"path\": \"config.json\", \"selector\": \"database.host\"}"
    )]
    async fn doc_query(
        &self,
        Parameters(p): Parameters<DocQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        if let Some(ref sel) = p.selector {
            validate_param_size("selector", sel)?;
        }
        let abs = self.cwd().join(&p.path);
        let file = abs.to_string_lossy().into_owned();
        let action = match p.action.as_str() {
            "has" => {
                let selector = p.selector.ok_or_else(|| {
                    McpError::invalid_params("'has' action requires a selector".to_string(), None)
                })?;
                crate::cmd::doc::DocAction::Has { file, selector }
            }
            "keys" => {
                let selector = p.selector.ok_or_else(|| {
                    McpError::invalid_params("'keys' action requires a selector".to_string(), None)
                })?;
                crate::cmd::doc::DocAction::Keys { file, selector }
            }
            "len" => {
                let selector = p.selector.ok_or_else(|| {
                    McpError::invalid_params("'len' action requires a selector".to_string(), None)
                })?;
                crate::cmd::doc::DocAction::Len { file, selector }
            }
            "select" => {
                let selector = p.selector.ok_or_else(|| {
                    McpError::invalid_params(
                        "'select' action requires a selector".to_string(),
                        None,
                    )
                })?;
                crate::cmd::doc::DocAction::Select { file, selector }
            }
            "flatten" => crate::cmd::doc::DocAction::Flatten { file },
            other => {
                return Err(McpError::invalid_params(
                    format!(
                        "unknown action '{other}'; valid actions: has, keys, len, select, flatten"
                    ),
                    None,
                ));
            }
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Compare two structured files (JSON, YAML, or TOML) and show differences. Example: {\"file_a\": \"old.json\", \"file_b\": \"new.json\"}"
    )]
    async fn doc_diff(
        &self,
        Parameters(p): Parameters<DocDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.file_a)?;
        self.check_path(&p.file_b)?;
        let abs_a = self.cwd().join(&p.file_a);
        let abs_b = self.cwd().join(&p.file_b);
        let action = crate::cmd::doc::DocAction::Diff {
            file_a: abs_a.to_string_lossy().into_owned(),
            file_b: abs_b.to_string_lossy().into_owned(),
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for Bline parity: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results. Other options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".blineignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
    )]
    async fn search_files(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.files_with_matches && p.count {
            return Err(McpError::invalid_params(
                "files_with_matches and count cannot be combined",
                None,
            ));
        }
        if p.invert_match && p.multiline {
            return Err(McpError::invalid_params(
                "invert_match and multiline cannot be combined",
                None,
            ));
        }
        validate_param_size("pattern", &p.pattern)?;
        for path in &p.paths {
            self.check_path(path)?;
        }
        // Validate custom ignore filenames too (new in #821 for layered ignores).
        // Treat them as paths relative to cwd for containment (even if just names like ".blineignore").
        for f in &p.custom_ignore_filenames {
            self.check_path(f)?;
        }
        let search_args = crate::cmd::search::SearchArgs {
            pattern: p.pattern,
            paths: if p.paths.is_empty() {
                vec![".".into()]
            } else {
                p.paths
            },
            literal: p.literal,
            regex: !p.literal,
            context: p.context,
            before_context: p.before_context,
            after_context: p.after_context,
            files_with_matches: p.files_with_matches,
            count: p.count,
            invert_match: p.invert_match,
            multiline: p.multiline,
            case_insensitive: p.case_insensitive,
            assert_count: p.assert_count,
            max_results: p.max_results,
        };
        let global = GlobalFlags {
            json: true,
            cwd: Some(self.cwd().to_string_lossy().into_owned()),
            glob: p.globs,
            exclude: p.exclude_patterns,
            ignore_file: p.custom_ignore_filenames,
            ..GlobalFlags::default()
        };
        let results = crate::cmd::search::collect_matches(&search_args, &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        // --assert-count mode: return count comparison instead of matches.
        if let Some(expected) = p.assert_count {
            let actual: usize = results.file_match_counts.values().sum();
            let matched = actual == expected;
            let status = if matched {
                "success"
            } else {
                "changes_detected"
            };
            let code = if matched {
                exit::SUCCESS
            } else {
                exit::CHANGES_DETECTED
            };
            let output = serde_json::json!({
                "ok": true,
                "status": status,
                "assert_count": {
                    "expected": expected,
                    "actual": actual,
                    "matched": matched,
                }
            });
            return exit_code_to_result(code, &output.to_string(), "");
        }

        let has_matches = if search_args.count || search_args.files_with_matches {
            !results.file_match_counts.is_empty()
        } else {
            results.has_matches()
        };
        if !has_matches {
            return exit_code_to_result(exit::NO_MATCHES, "", "No matches found.");
        }

        let output = crate::cmd::search::format_results(results, &search_args, &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        exit_code_to_result(exit::SUCCESS, &output, "No results.")
    }

    #[tool(
        description = "Show uncommitted file changes vs git HEAD. Returns lists of modified, created, and deleted files. No parameters required."
    )]
    async fn git_status(
        &self,
        #[allow(unused_variables)] Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let global = GlobalFlags {
            cwd: Some(self.cwd().to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let status = crate::cmd::status::collect_status(&[], &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        let json = serde_json::to_string_pretty(&status)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Read file contents with optional line range. Example: {\"path\": \"src/main.rs\", \"lines\": \"1:50\"}"
    )]
    async fn read_file(
        &self,
        Parameters(p): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        execute_plan_validated(
            make_plan(vec![Operation::Read {
                path: p.path,
                lines: p.lines,
            }]),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with to=\"\" to delete lines). Example: {\"path\": \"README.md\", \"from\": \"1.0.0\", \"to\": \"2.0.0\"}"
    )]
    async fn replace_text(
        &self,
        Parameters(p): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("from", &p.from)?;
        if let Some(ref to) = p.to {
            validate_content_size("to", to)?;
        }
        if let Some(ref ib) = p.insert_before {
            validate_content_size("insert_before", ib)?;
        }
        if let Some(ref ia) = p.insert_after {
            validate_content_size("insert_after", ia)?;
        }

        // Mirror CLI validations (replace.rs:239-247).
        if p.nth == Some(0) {
            return Err(McpError::invalid_params("nth must be >= 1 (1-based)", None));
        }
        let mode_count =
            p.to.is_some() as u8 + p.insert_before.is_some() as u8 + p.insert_after.is_some() as u8;
        if mode_count > 1 {
            return Err(McpError::invalid_params(
                "to, insert_before, and insert_after are mutually exclusive",
                None,
            ));
        }
        if mode_count == 0 {
            return Err(McpError::invalid_params(
                "one of to, insert_before, or insert_after is required",
                None,
            ));
        }
        if p.whole_line && p.multiline {
            return Err(McpError::invalid_params(
                "whole_line and multiline cannot be combined",
                None,
            ));
        }
        if p.range.is_some() && !p.whole_line {
            return Err(McpError::invalid_params(
                "range requires whole_line=true",
                None,
            ));
        }

        // Tier 2: pre-validate structured file edits and collect warnings.
        let validation_warnings = if !p.regex {
            let abs = self.cwd().join(&p.path);
            if let Ok(content) = std::fs::read_to_string(&abs) {
                let to_str = p.to.as_deref().unwrap_or("");
                let result =
                    crate::fallback::validate_edit(&content, &p.from, to_str, Some(&p.path));
                let mut warnings = result.warnings;
                if !result.valid {
                    warnings.extend(result.errors);
                }
                warnings
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let mode = if p.regex {
            Some("regex".to_string())
        } else {
            None
        };
        let replace_op = Operation::Replace {
            glob: None,
            path: Some(p.path),
            mode,
            from: p.from,
            to: p.to,
            nth: p.nth,
            insert_before: p.insert_before,
            insert_after: p.insert_after,
            case_insensitive: p.case_insensitive,
            multiline: p.multiline,
            if_exists: p.if_exists,
            whole_line: p.whole_line,
            range: p.range,
            word_boundary: p.word_boundary,
            before_context: p.before_context,
            after_context: p.after_context,
        };
        let mut tool_result = self.run_ops(vec![replace_op], Some(p.strict))?;

        // Append validation warnings to the response.
        if !validation_warnings.is_empty() {
            let warning_text = format!(
                "\n\nWarnings:\n{}",
                validation_warnings
                    .iter()
                    .map(|w| format!("  - {w}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            tool_result.content.push(Content::text(warning_text));
        }

        Ok(tool_result)
    }

    #[tool(
        description = "Add a bullet under a markdown heading. Idempotent: skipped if already present. Example: {\"path\": \"CHANGELOG.md\", \"heading\": \"## Changes\", \"bullet\": \"- Added new feature\"}"
    )]
    async fn md_upsert_bullet(
        &self,
        Parameters(p): Parameters<MdUpsertBulletParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("bullet", &p.bullet)?;
        self.run_ops(
            vec![Operation::MdUpsertBullet {
                path: p.path,
                heading: p.heading,
                bullet: p.bullet,
            }],
            None,
        )
    }

    #[tool(
        description = "Append a row to a markdown table under a heading. Example: {\"path\": \"README.md\", \"heading\": \"## Commands\", \"row\": \"| deploy | Run deployment |\"}"
    )]
    async fn md_table_append(
        &self,
        Parameters(p): Parameters<MdTableAppendParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_param_size("row", &p.row)?;
        self.run_ops(
            vec![Operation::MdTableAppend {
                path: p.path,
                heading: p.heading,
                row: p.row,
            }],
            None,
        )
    }

    #[tool(
        description = "Replace the body of a markdown section. Content replaces everything between this heading and the next equal-or-higher heading. Example: {\"path\": \"README.md\", \"heading\": \"## Usage\", \"content\": \"Run `make build`.\\n\"}"
    )]
    async fn md_replace_section(
        &self,
        Parameters(p): Parameters<MdReplaceSectionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_content_size("content", &p.content)?;
        self.run_ops(
            vec![Operation::MdReplaceSection {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }],
            None,
        )
    }

    #[tool(
        description = "Insert content after a markdown heading. Preserves existing body. Example: {\"path\": \"README.md\", \"heading\": \"## Notes\", \"content\": \"Updated 2025-01-01.\\n\"}"
    )]
    async fn md_insert_after_heading(
        &self,
        Parameters(p): Parameters<MdInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_content_size("content", &p.content)?;
        self.run_ops(
            vec![Operation::MdInsertAfterHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }],
            None,
        )
    }

    #[tool(
        description = "Insert content before a markdown heading. The new content appears above the heading line. Example: {\"path\": \"doc.md\", \"heading\": \"## API\", \"content\": \"---\\n\"}"
    )]
    async fn md_insert_before_heading(
        &self,
        Parameters(p): Parameters<MdInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_content_size("content", &p.content)?;
        self.run_ops(
            vec![Operation::MdInsertBeforeHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }],
            None,
        )
    }

    #[tool(
        description = "Move a markdown heading section to a new position (same file reorder or cross-file). Exactly one of before or after is required. Omit to for same-file reorder. Example: {\"path\": \"spec.md\", \"heading\": \"## Appendix\", \"to\": \"notes.md\", \"before\": \"## References\"}"
    )]
    async fn md_move_section(
        &self,
        Parameters(p): Parameters<MdMoveSectionParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        if let Some(ref to) = p.to {
            self.check_path(to)?;
        }
        if p.before.is_none() && p.after.is_none() {
            return Err(McpError::invalid_params(
                "exactly one of 'before' or 'after' must be provided",
                None,
            ));
        }
        if p.before.is_some() && p.after.is_some() {
            return Err(McpError::invalid_params(
                "'before' and 'after' cannot both be set",
                None,
            ));
        }
        execute_plan_validated(
            make_plan(vec![Operation::MdMoveSection {
                path: p.path,
                heading: p.heading,
                to: p.to,
                before: p.before,
                after: p.after,
            }]),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    #[tool(
        description = "Lint a markdown rules file for duplicate headings, dangerous git commands, and missing final newline. Example: {\"path\": \"AGENTS.md\"}"
    )]
    async fn md_lint(
        &self,
        Parameters(p): Parameters<MdLintAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        let abs = self.cwd().join(&p.path);
        let content = std::fs::read_to_string(&abs)
            .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
        let issues = crate::ops::md::lint_agents_content(&content);
        let json = serde_json::to_string_pretty(&issues)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Fix whitespace in a file: trims trailing spaces and ensures final newline. Safe to call on any file (no-op if already clean). Example: {\"path\": \"dirty.txt\"}"
    )]
    async fn fix_whitespace(
        &self,
        Parameters(p): Parameters<TidyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        self.run_ops(
            vec![Operation::TidyFix {
                path: p.path,
                ensure_final_newline: Some(true),
                trim_trailing_whitespace: Some(true),
                normalize_eol: None,
            }],
            None,
        )
    }

    #[tool(
        description = "Rename (move) a file. Handles binary files. Example: {\"from\": \"old.txt\", \"to\": \"new.txt\"}"
    )]
    async fn move_file(
        &self,
        Parameters(p): Parameters<FileRenameParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.from)?;
        self.check_path(&p.to)?;

        let src = self.cwd().join(&p.from);
        let dst = self.cwd().join(&p.to);
        match crate::cmd::rename::apply_rename(&src, &dst, p.force, self.cwd()) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Renamed {} -> {}",
                p.from, p.to
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(
        description = "Append content to the end of an existing file. Inserts a newline separator if the file does not end with one. Fails if the file does not exist. Example: {\"path\": \"tests.rs\", \"content\": \"#[test]\\nfn new() {}\"}"
    )]
    async fn append_file(
        &self,
        Parameters(p): Parameters<AppendFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_content_size("content", &p.content)?;

        let abs = self.cwd().join(&p.path);
        match crate::cmd::append::apply_append(&abs, &p.content) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Appended to {}",
                p.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(
        description = "Create a new file with content. Fails if file exists unless force=true. Example: {\"path\": \"hello.txt\", \"content\": \"Hello, World!\"}"
    )]
    async fn create_file(
        &self,
        Parameters(p): Parameters<CreateFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;
        validate_content_size("content", &p.content)?;

        let abs = self.cwd().join(&p.path);
        match crate::cmd::create::apply_create(&abs, &p.content, p.force) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Created {}",
                p.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(
        description = "Delete a file. Fails if the file does not exist. Example: {\"path\": \"temp.txt\"}"
    )]
    async fn delete_file(
        &self,
        Parameters(p): Parameters<DeleteFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.check_path(&p.path)?;

        let abs = self.cwd().join(&p.path);
        match crate::cmd::delete::apply_delete(&abs, self.cwd()) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Deleted {}",
                p.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(
        description = "Apply a unified diff (patch). The diff parameter is the full unified diff text. Supports multi-file diffs. Use on_stale=merge for three-way merge on stale context; allow_conflicts=true writes conflict markers. Never commit files containing conflict markers. Example: {\"diff\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+new\", \"on_stale\": \"fail\"}"
    )]
    async fn apply_patch(
        &self,
        Parameters(p): Parameters<PatchParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_content_size("diff", &p.diff)?;
        // Validate paths embedded in the diff.
        let patch_files = crate::ops::patch::parse_patch(&p.diff)
            .map_err(|e| McpError::invalid_params(format!("failed to parse diff: {e}"), None))?;
        for pf in &patch_files {
            self.check_path(&pf.path)?;
        }

        execute_plan_validated(
            make_plan_strict(
                vec![Operation::PatchApply {
                    diff: p.diff,
                    on_stale: p.on_stale,
                    allow_conflicts: p.allow_conflicts,
                }],
                Some(p.strict),
            ),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    #[tool(
        description = "Replace the same text across multiple files in one call. Atomic: all files succeed or none change. Example: {\"files\": [\"Cargo.toml\", \"README.md\"], \"from\": \"0.1.0\", \"to\": \"0.2.0\"}"
    )]
    async fn batch_replace(
        &self,
        Parameters(p): Parameters<BatchReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.files.is_empty() {
            return Err(McpError::invalid_params(
                "files array must not be empty",
                None,
            ));
        }
        validate_batch_size("files", p.files.len())?;
        validate_param_size("from", &p.from)?;
        validate_content_size("to", &p.to)?;
        for f in &p.files {
            self.check_path(f)?;
        }
        let mode = if p.regex {
            Some("regex".to_string())
        } else {
            None
        };
        let ops: Vec<Operation> = p
            .files
            .into_iter()
            .map(|file| Operation::Replace {
                glob: None,
                path: Some(file),
                mode: mode.clone(),
                from: p.from.clone(),
                to: Some(p.to.clone()),
                nth: None,
                insert_before: None,
                insert_after: None,
                case_insensitive: p.case_insensitive,
                multiline: p.multiline,
                if_exists: false,
                whole_line: false,
                range: None,
                word_boundary: p.word_boundary,
                before_context: None,
                after_context: None,
            })
            .collect();
        execute_plan_validated(
            make_plan_strict(ops, Some(p.strict)),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    #[tool(
        description = "Fix whitespace in multiple files in one call: trims trailing spaces and ensures final newline. Atomic: all files succeed or none change. Example: {\"files\": [\"src/main.rs\", \"src/lib.rs\"]}"
    )]
    async fn batch_tidy(
        &self,
        Parameters(p): Parameters<BatchTidyParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.files.is_empty() {
            return Err(McpError::invalid_params(
                "files array must not be empty",
                None,
            ));
        }
        validate_batch_size("files", p.files.len())?;
        for f in &p.files {
            self.check_path(f)?;
        }
        let ops: Vec<Operation> = p
            .files
            .into_iter()
            .map(|file| Operation::TidyFix {
                path: file,
                ensure_final_newline: Some(true),
                trim_trailing_whitespace: Some(true),
                normalize_eol: None,
            })
            .collect();
        execute_plan_validated(
            make_plan_strict(ops, Some(p.strict)),
            self.cwd(),
            Some(&self.path_guard),
        )
    }

    #[tool(
        description = "Execute an arbitrary multi-step transaction plan atomically (MCP equivalent of `patchloom tx`). Provide either an inline 'plan' object or a 'plan_path' to a plan file. Supports mixed operations (doc.*, md.*, replace, file create/delete/rename, tidy, patch, etc). Strongly recommended for any multi-file or multi-op work to prevent races and ensure atomicity/rollback. See agent-rules --mode mcp or PATCHLOOM.md for plan schema examples."
    )]
    async fn execute_plan(
        &self,
        Parameters(p): Parameters<ExecutePlanParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut plan = if let Some(inline_plan) = p.plan {
            inline_plan
        } else if let Some(path) = &p.plan_path {
            self.check_path(path)?;
            let abs = self.cwd().join(path);
            let content = std::fs::read_to_string(&abs).map_err(|e| {
                McpError::internal_error(format!("failed to read plan_path: {e}"), None)
            })?;
            crate::plan::parse_plan_auto(&content, Some(path), None)
                .map_err(|e| McpError::invalid_params(format!("failed to parse plan: {e}"), None))?
        } else {
            return Err(McpError::invalid_params(
                "either 'plan' (inline) or 'plan_path' must be provided",
                None,
            ));
        };

        // Validate every path declared by operations against the PathGuard.
        // (The plan file itself was already checked above when plan_path was used.)
        for op in &plan.operations {
            for declared in crate::plan::declared_paths(op) {
                self.check_path(declared)?;
            }
        }

        // The `strict` parameter from the MCP invocation always controls the execution
        // (it defaults to true). This provides a simple, predictable experience for agents.
        plan.strict = Some(p.strict);

        execute_plan_validated(plan, self.cwd(), Some(&self.path_guard))
    }
}

impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use these tools for ALL file operations. Prefer 'execute_plan' (or tx plans) for any multi-op or multi-file work to ensure atomicity and avoid races from parallel calls on the same paths. Use batch_replace/batch_tidy only for uniform ops across files. Per-call success does not guarantee combined success if you issue conflicting parallel writes.",
            );
        info.server_info.name = "patchloom".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.clone();
        crate::verbose!("mcp: tool call -> {tool_name}");
        let start = std::time::Instant::now();
        let tc = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(tc).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        crate::verbose!(
            "mcp: {tool_name} completed in {duration_ms}ms (ok={})",
            result.is_ok()
        );
        self.log_tool_call(&tool_name, duration_ms, &result);
        result
    }
}

/// Run the MCP server on stdio.
pub(crate) fn run_mcp_server(global: &GlobalFlags, log: Option<String>) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let service = PatchloomService::new(cwd, log)?;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = service
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        server
            .waiting()
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(crate::exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::ServiceExt;

    /// Spin up a PatchloomService over a duplex stream and return the
    /// connected client handle.
    async fn spawn_test_client(
        cwd: std::path::PathBuf,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let (server_transport, client_transport) = tokio::io::duplex(16384);
        let service = PatchloomService::new(cwd, None).unwrap();
        tokio::spawn(async move {
            let server = service.serve(server_transport).await.unwrap();
            server.waiting().await.unwrap();
        });
        ().serve(client_transport).await.unwrap()
    }

    #[tokio::test]
    async fn mcp_lists_expected_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let tools = client.peer().list_all_tools().await.unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        let descriptions = tools
            .iter()
            .map(|t| (t.name.as_ref(), t.description.as_deref().unwrap_or("")))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert!(names.contains(&"doc_set"), "missing doc_set tool");
        assert!(names.contains(&"doc_get"), "missing doc_get tool");
        assert!(names.contains(&"doc_query"), "missing doc_query tool");
        assert!(names.contains(&"doc_diff"), "missing doc_diff tool");
        assert!(names.contains(&"read_file"), "missing read_file tool");
        assert!(names.contains(&"search_files"), "missing search_files tool");
        assert_eq!(
            descriptions.get("search_files"),
            Some(
                &"Search text files for a pattern (regex by default, use literal=true for exact match). Supports advanced layered ignores for Bline parity: globs (include), exclude_patterns, custom_ignore_filenames (e.g. .blineignore), max_results. Other options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count, before/after_context. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true, \"custom_ignore_filenames\": [\".blineignore\"], \"exclude_patterns\": [\"target/**\"], \"max_results\": 20}"
            ),
            "search_files description drifted"
        );
        assert!(names.contains(&"git_status"), "missing git_status tool");
        assert!(names.contains(&"replace_text"), "missing replace_text tool");
        assert_eq!(
            descriptions.get("replace_text"),
            Some(
                &"Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range, word_boundary. Set word_boundary=true to match only whole words (prevents 'SetupFile' matching inside 'BenchSetupFile'). Set whole_line=true to replace entire lines containing a match (use with to=\"\" to delete lines). Example: {\"path\": \"README.md\", \"from\": \"1.0.0\", \"to\": \"2.0.0\"}"
            ),
            "replace_text description drifted"
        );
        assert!(
            names.contains(&"fix_whitespace"),
            "missing fix_whitespace tool"
        );
        assert_eq!(
            descriptions.get("fix_whitespace"),
            Some(
                &"Fix whitespace in a file: trims trailing spaces and ensures final newline. Safe to call on any file (no-op if already clean). Example: {\"path\": \"dirty.txt\"}"
            ),
            "fix_whitespace description drifted"
        );
        assert!(names.contains(&"move_file"), "missing move_file tool");
        assert!(names.contains(&"create_file"), "missing create_file tool");
        assert!(names.contains(&"delete_file"), "missing delete_file tool");
        assert!(names.contains(&"apply_patch"), "missing apply_patch tool");
        assert!(
            names.contains(&"batch_replace"),
            "missing batch_replace tool"
        );
        assert!(names.contains(&"batch_tidy"), "missing batch_tidy tool");
        assert!(names.contains(&"execute_plan"), "missing execute_plan tool");
        assert!(
            names.contains(&"md_insert_after_heading"),
            "missing md_insert_after_heading tool"
        );
        assert!(
            names.contains(&"md_insert_before_heading"),
            "missing md_insert_before_heading tool"
        );
        assert!(
            names.contains(&"md_move_section"),
            "missing md_move_section tool"
        );
        assert!(names.contains(&"append_file"), "missing append_file tool");
        assert_eq!(names.len(), 32, "expected 32 tools, got {}", names.len());
        client.cancel().await.unwrap();
    }

    #[test]
    fn mcp_search_params_accept_new_bline_ignore_fields() {
        // Ensures schemars/serde accept the new fields added for #821 parity.
        let json = r#"{
            "pattern": "TODO",
            "paths": ["src/"],
            "globs": ["*.rs"],
            "exclude_patterns": ["target/**"],
            "custom_ignore_filenames": [".blineignore"],
            "max_results": 10,
            "literal": true
        }"#;
        let p: SearchParams =
            serde_json::from_str(json).expect("SearchParams deserial with new ignore/max fields");
        assert_eq!(p.globs.len(), 1);
        assert_eq!(p.max_results, 10);
    }

    #[tokio::test]
    async fn mcp_server_info_has_correct_name() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let info = client.peer_info().expect("peer info should be set");
        assert_eq!(info.server_info.name, "patchloom");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_path_traversal_rejected_via_protocol() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "../../etc/passwd",
                "selector": "root",
                "value": "hacked"
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "path traversal should be rejected");
        client.cancel().await.unwrap();
    }

    #[test]
    fn patch_apply_path_containment_validation() {
        let dir = tempfile::TempDir::new().unwrap();
        let evil_diff =
            "--- a/../../etc/passwd\n+++ b/../../etc/passwd\n@@ -1,1 +1,1 @@\n-root\n+hacked\n";
        let ops = vec![Operation::PatchApply {
            diff: evil_diff.to_string(),
            on_stale: crate::ops::patch::OnStale::Fail,
            allow_conflicts: false,
        }];
        let result = validate_operation_paths(&ops, dir.path());
        assert!(
            result.is_err(),
            "PatchApply with escaping paths should be rejected"
        );
    }

    #[test]
    fn patch_apply_safe_paths_pass_containment() {
        let dir = tempfile::TempDir::new().unwrap();
        let safe_diff = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let ops = vec![Operation::PatchApply {
            diff: safe_diff.to_string(),
            on_stale: crate::ops::patch::OnStale::Fail,
            allow_conflicts: false,
        }];
        let result = validate_operation_paths(&ops, dir.path());
        assert!(result.is_ok(), "PatchApply with safe paths should pass");
    }

    #[tokio::test]
    async fn mcp_example_argument_keys_match_schemas() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let tools = client.peer().list_all_tools().await.unwrap();

        // Build map of tool_name -> set of valid property keys from schemas
        let mut schema_keys: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for tool in &tools {
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                schema_keys.insert(tool.name.to_string(), props.keys().cloned().collect());
            }
        }

        // Read and validate the example file
        let example_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/08-mcp-tool-call.json");
        let example_content = std::fs::read_to_string(&example_path)
            .expect("failed to read examples/08-mcp-tool-call.json");
        let example_json: serde_json::Value = serde_json::from_str(&example_content)
            .expect("failed to parse examples/08-mcp-tool-call.json");

        let examples = example_json["examples"]
            .as_array()
            .expect("examples should be an array");

        let mut errors = Vec::new();
        for entry in examples {
            let tool_name = entry["tool"].as_str().expect("tool should be a string");
            let arguments = entry["arguments"]
                .as_object()
                .expect("arguments should be an object");

            if let Some(valid_keys) = schema_keys.get(tool_name) {
                for key in arguments.keys() {
                    if !valid_keys.contains(key) {
                        errors.push(format!(
                            "tool '{}': unknown argument '{}' (valid: {:?})",
                            tool_name, key, valid_keys
                        ));
                    }
                }
            } else {
                errors.push(format!("tool '{}': not found in MCP tool list", tool_name));
            }
        }

        assert!(
            errors.is_empty(),
            "MCP example validation errors:\n{}",
            errors.join("\n")
        );
        client.cancel().await.unwrap();
    }

    // Path containment unit tests (validate_path_contained, validate_path_resolved)
    // have been moved to crate::containment::tests. The MCP-level integration test
    // `mcp_path_traversal_rejected_via_protocol` above verifies the end-to-end
    // path rejection through the MCP protocol layer.

    /// Spawn a test client with JSONL logging enabled.
    async fn spawn_test_client_with_log(
        cwd: std::path::PathBuf,
        log_path: std::path::PathBuf,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let (server_transport, client_transport) = tokio::io::duplex(16384);
        let service =
            PatchloomService::new(cwd, Some(log_path.to_string_lossy().into_owned())).unwrap();
        tokio::spawn(async move {
            let server = service.serve(server_transport).await.unwrap();
            server.waiting().await.unwrap();
        });
        ().serve(client_transport).await.unwrap()
    }

    #[tokio::test]
    async fn mcp_log_writes_jsonl_on_tool_call() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        let client = spawn_test_client_with_log(dir.path().to_path_buf(), log_file.clone()).await;

        // Call read_file to trigger a log entry.
        let params = rmcp::model::CallToolRequestParams::new("read_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "test.txt",
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            result.is_error != Some(true),
            "read_file should succeed: {result:?}"
        );
        client.cancel().await.unwrap();

        // Verify the log file contains a valid JSONL entry.
        let log_content = std::fs::read_to_string(&log_file).expect("log file should exist");
        let lines: Vec<&str> = log_content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly 1 log line");
        let entry: serde_json::Value =
            serde_json::from_str(lines[0]).expect("log line should be valid JSON");
        assert_eq!(entry["tool"], "read_file");
        assert_eq!(entry["ok"], true);
        assert!(entry["ts"].is_number(), "ts should be a number");
        assert!(
            entry["duration_ms"].is_number(),
            "duration_ms should be a number"
        );
    }

    #[tokio::test]
    async fn mcp_log_records_error_on_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        let client = spawn_test_client_with_log(dir.path().to_path_buf(), log_file.clone()).await;

        // Call doc_set on a non-existent file to trigger an error result.
        let params = rmcp::model::CallToolRequestParams::new("doc_set").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "nonexistent.json",
                "selector": "key",
                "value": "val",
            }))
            .unwrap(),
        );
        let _result = client.peer().call_tool(params).await;
        client.cancel().await.unwrap();

        let log_content = std::fs::read_to_string(&log_file).expect("log file should exist");
        let lines: Vec<&str> = log_content.trim().lines().collect();
        assert_eq!(lines.len(), 1, "expected exactly 1 log line");
        let entry: serde_json::Value =
            serde_json::from_str(lines[0]).expect("log line should be valid JSON");
        assert_eq!(entry["tool"], "doc_set");
        assert_eq!(entry["ok"], false);
    }

    #[test]
    fn validate_content_size_accepts_small() {
        assert!(validate_content_size("field", "hello").is_ok());
    }

    #[test]
    fn validate_content_size_rejects_oversized() {
        let big = "x".repeat(MAX_CONTENT_BYTES + 1);
        let err = validate_content_size("content", &big).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("content"), "error should name the field");
        assert!(msg.contains("exceeds"), "error should say exceeds");
    }

    #[test]
    fn validate_param_size_accepts_small() {
        assert!(validate_param_size("selector", "a.b.c").is_ok());
    }

    #[test]
    fn validate_param_size_rejects_oversized() {
        let big = "x".repeat(MAX_PARAM_BYTES + 1);
        let err = validate_param_size("pattern", &big).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("pattern"), "error should name the field");
    }

    #[test]
    fn validate_batch_size_accepts_small() {
        assert!(validate_batch_size("files", 5).is_ok());
    }

    #[test]
    fn validate_batch_size_rejects_oversized() {
        let err = validate_batch_size("files", MAX_BATCH_FILES + 1).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("files"), "error should name the field");
    }

    #[test]
    fn validate_json_depth_accepts_shallow() {
        let val = serde_json::json!({"a": {"b": "c"}});
        assert!(validate_json_depth("value", &val).is_ok());
    }

    #[test]
    fn validate_json_depth_accepts_scalar() {
        let val = serde_json::json!("hello");
        assert!(validate_json_depth("value", &val).is_ok());
    }

    #[test]
    fn validate_json_depth_rejects_deeply_nested() {
        // Build a value nested deeper than MAX_JSON_DEPTH.
        let mut val = serde_json::json!("leaf");
        for _ in 0..MAX_JSON_DEPTH + 1 {
            val = serde_json::json!([val]);
        }
        let err = validate_json_depth("value", &val).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("nesting depth"), "error should mention depth");
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_content_via_protocol() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let big_content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let params = rmcp::model::CallToolRequestParams::new("create_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "big.txt",
                "content": big_content,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized content should be rejected");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_batch() {
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let files: Vec<String> = (0..MAX_BATCH_FILES + 1)
            .map(|i| format!("file{i}.txt"))
            .collect();
        let params = rmcp::model::CallToolRequestParams::new("batch_tidy").with_arguments(
            serde_json::from_value(serde_json::json!({
                "files": files,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized batch should be rejected");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_rejects_oversized_doc_query_selector() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("data.json"), r#"{"a":1}"#).unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;
        let big_selector = "x".repeat(MAX_PARAM_BYTES + 1);
        let params = rmcp::model::CallToolRequestParams::new("doc_query").with_arguments(
            serde_json::from_value(serde_json::json!({
                "action": "has",
                "path": "data.json",
                "selector": big_selector,
            }))
            .unwrap(),
        );
        let result = client.peer().call_tool(params).await;
        assert!(result.is_err(), "oversized selector should be rejected");
        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_no_log_without_flag() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_file = dir.path().join("mcp.log");
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        // Use default client (no log flag).
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        let params = rmcp::model::CallToolRequestParams::new("read_file").with_arguments(
            serde_json::from_value(serde_json::json!({
                "path": "test.txt",
            }))
            .unwrap(),
        );
        let _result = client.peer().call_tool(params).await.unwrap();
        client.cancel().await.unwrap();

        // Log file should not exist since no log path was configured.
        assert!(
            !log_file.exists(),
            "log file should not exist without --log"
        );
    }

    #[tokio::test]
    async fn mcp_execute_plan_mixed_ops_atomic() {
        // Test the new execute_plan tool with a mixed plan (doc + replace + create).
        // This is the core of #827: one call for atomic multi-op instead of many parallel/serial.
        let dir = tempfile::TempDir::new().unwrap();
        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Prepare initial file
        std::fs::write(dir.path().join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();

        let plan_json = serde_json::json!({
            "version": "1",
            "strict": true,
            "operations": [
                {
                    "op": "doc.set",
                    "path": "package.json",
                    "selector": "version",
                    "value": "2.0.0"
                },
                {
                    "op": "replace",
                    "path": "package.json",
                    "from": "2.0.0",
                    "to": "2.1.0"
                },
                {
                    "op": "file.create",
                    "path": "CREATED.md",
                    "content": "# Created via plan\n"
                }
            ]
        });

        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        assert!(
            !result.is_error.unwrap_or(false),
            "execute_plan should succeed: {:?}",
            result
        );

        // Verify results
        let pkg = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert!(
            pkg.contains("2.1.0"),
            "doc.set + replace should have updated to 2.1.0"
        );

        let created = std::fs::read_to_string(dir.path().join("CREATED.md")).unwrap();
        assert!(created.contains("Created via plan"));

        client.cancel().await.unwrap();
    }

    #[tokio::test]
    async fn mcp_execute_plan_strict_rollback_on_error() {
        // Verify that strict plan rolls back on failure (e.g. invalid op mid-plan).
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "original").unwrap();

        let client = spawn_test_client(dir.path().to_path_buf()).await;

        // Plan that will fail on second op (bad replace or non-existing for safety, but use doc on non structured? Use a replace that requires mode or simply a bad path? Better: use a plan that succeeds first, fails second.
        // For simplicity, use a plan with an op that causes parse/validate fail, but since plan itself is valid, use a mid failure like delete non existing in strict?
        // Simpler: a plan that does create (ok), then a replace that is invalid (no mode).
        // But to trigger runtime fail in tx, easier: use doc.set on a file that will cause later validate fail, but to keep simple use a non-existent for a delete in plan?
        // Actually for this, do two creates, then a doc.set on bad structured that may not rollback file create? File creates are part of tx.
        // Use a plan that the second op fails (e.g. move non-existing source).
        let plan_json = serde_json::json!({
            "version": "1",
            "strict": true,
            "operations": [
                { "op": "file.create", "path": "first.txt", "content": "one" },
                { "op": "file.delete", "path": "does-not-exist.txt" }  // will fail
            ]
        });

        let params = rmcp::model::CallToolRequestParams::new("execute_plan").with_arguments(
            serde_json::from_value(serde_json::json!({ "plan": plan_json })).unwrap(),
        );

        let result = client.peer().call_tool(params).await.unwrap();
        // Should report error
        assert!(
            result.is_error.unwrap_or(false),
            "plan with failing op under strict should error"
        );

        // first.txt should NOT exist (rollback)
        assert!(
            !dir.path().join("first.txt").exists(),
            "strict plan should have rolled back the first create"
        );

        client.cancel().await.unwrap();
    }
}
