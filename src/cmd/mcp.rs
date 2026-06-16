//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`

use anyhow::Context;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData as McpError, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt, tool, tool_router};
use serde::Deserialize;
use std::path::PathBuf;

use crate::cli::global::GlobalFlags;
use crate::exit;
use crate::plan::{Operation, Plan};

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Selector for the value to set (e.g., "version", "db.pool", "items[0].name").
    pub selector: String,
    /// Value to set (string, number, boolean, object, or array).
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocDeleteParams {
    /// File path.
    pub path: String,
    /// Selector for the value to delete.
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocMergeParams {
    /// File path.
    pub path: String,
    /// Object to deep-merge into the document root.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocArrayParams {
    /// File path.
    pub path: String,
    /// Selector pointing to the target array.
    pub selector: String,
    /// Value to append or prepend.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocEnsureParams {
    /// File path.
    pub path: String,
    /// Selector for the value to ensure exists.
    pub selector: String,
    /// Value to set only if the selector path is missing.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocUpdateParams {
    /// File path.
    pub path: String,
    /// Wildcard selector for items to update (e.g., "items[*]").
    pub selector: String,
    /// Value to set on each matched item.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocMoveParams {
    /// File path.
    pub path: String,
    /// Source selector path to move from.
    pub from: String,
    /// Destination selector path to move to.
    pub to: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocDeleteWhereParams {
    /// File path.
    pub path: String,
    /// Selector pointing to the target array.
    pub selector: String,
    /// Predicate for items to delete (e.g., "name=obsolete").
    pub predicate: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceParams {
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocGetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Selector for the value to read (e.g., "version", "db.pool").
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFileParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Optional line range (e.g., "10:20"). Omit to read the entire file.
    pub lines: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdLintAgentsParams {
    /// Markdown file path (typically AGENTS.md).
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdUpsertBulletParams {
    /// Markdown file path.
    pub path: String,
    /// Heading to find (e.g., "## Rules").
    pub heading: String,
    /// Bullet text to add (e.g., "- New rule"). Skipped if already present.
    pub bullet: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdTableAppendParams {
    /// Markdown file path.
    pub path: String,
    /// Heading above the target table.
    pub heading: String,
    /// Table row to append (e.g., "| col1 | col2 |").
    pub row: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdReplaceSectionParams {
    /// Markdown file path.
    pub path: String,
    /// Heading of the section to replace.
    pub heading: String,
    /// New content for the section body.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdInsertParams {
    /// Markdown file path.
    pub path: String,
    /// Heading to target (e.g., "## Changelog").
    pub heading: String,
    /// Content to insert.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MdMoveSectionParams {
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
pub struct TidyParams {
    /// File path to normalize.
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileRenameParams {
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
pub struct CreateFileParams {
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
pub struct DeleteFileParams {
    /// File path (relative to working directory).
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchParams {
    /// Unified diff text to apply.
    pub diff: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocQueryParams {
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
pub struct DocDiffParams {
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
pub struct BatchReplaceParams {
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchTidyParams {
    /// File paths to normalize (relative to working directory).
    pub files: Vec<String>,
}

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

fn validate_content_size(field: &str, value: &str) -> Result<(), McpError> {
    if value.len() > MAX_CONTENT_BYTES {
        return Err(McpError::invalid_params(
            format!(
                "{field} exceeds maximum size ({} bytes, limit {})",
                value.len(),
                MAX_CONTENT_BYTES
            ),
            None,
        ));
    }
    Ok(())
}

fn validate_param_size(field: &str, value: &str) -> Result<(), McpError> {
    if value.len() > MAX_PARAM_BYTES {
        return Err(McpError::invalid_params(
            format!(
                "{field} exceeds maximum size ({} bytes, limit {})",
                value.len(),
                MAX_PARAM_BYTES
            ),
            None,
        ));
    }
    Ok(())
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
// Path containment
// ---------------------------------------------------------------------------

/// Reject paths that would escape the MCP server's working directory.
///
/// The MCP server accepts tool calls from AI agents that may be influenced
/// by prompt injection. Without containment, a crafted path like
/// `/etc/shadow` or `../../etc/passwd` would let operations read or write
/// arbitrary files on the host.
fn validate_path_contained(path: &str) -> Result<(), McpError> {
    let p = std::path::Path::new(path);

    if p.is_absolute() {
        return Err(McpError::invalid_params(
            format!("absolute paths are not allowed: {path}"),
            None,
        ));
    }

    // Walk components and track depth relative to cwd. If depth ever goes
    // negative, the path escapes the working directory.
    let mut depth: i32 = 0;
    for component in p.components() {
        match component {
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(McpError::invalid_params(
                        format!("path must not escape working directory: {path}"),
                        None,
                    ));
                }
            }
            std::path::Component::Normal(_) => {
                depth += 1;
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(McpError::invalid_params(
                    format!("unexpected path component in: {path}"),
                    None,
                ));
            }
        }
    }

    Ok(())
}

/// Validate a resolved path does not escape the working directory via symlinks.
///
/// After the syntactic check, canonicalize the joined path and verify it
/// starts with the canonicalized cwd. This catches symlinks that point
/// outside the workspace (#231).
fn validate_path_resolved(
    path: &str,
    cwd: &std::path::Path,
    canon_cwd: &std::path::Path,
) -> Result<(), McpError> {
    let joined = cwd.join(path);
    // For existing paths, canonicalize directly.
    // For non-existent paths, canonicalize the nearest existing ancestor
    // to catch symlink directories that point outside the workspace.
    let check_target = if joined.exists() {
        joined.clone()
    } else {
        let mut ancestor = joined.as_path();
        loop {
            match ancestor.parent() {
                Some(p) if p.exists() => {
                    ancestor = p;
                    break;
                }
                Some(p) => ancestor = p,
                None => return Ok(()),
            }
        }
        ancestor.to_path_buf()
    };
    let canon_path = check_target.canonicalize().map_err(|e| {
        McpError::internal_error(format!("failed to canonicalize path '{path}': {e}"), None)
    })?;
    if !canon_path.starts_with(canon_cwd) {
        return Err(McpError::invalid_params(
            format!(
                "resolved path escapes working directory: {path} (cwd: {})",
                cwd.display()
            ),
            None,
        ));
    }
    Ok(())
}

/// Validate all paths in a list of operations.
/// Checks both syntactic containment and symlink resolution.
#[cfg(test)]
fn validate_operation_paths(
    operations: &[Operation],
    cwd: &std::path::Path,
    canon_cwd: &std::path::Path,
) -> Result<(), McpError> {
    for op in operations {
        let paths: Vec<&str> = match op {
            Operation::DocSet { path, .. }
            | Operation::DocDelete { path, .. }
            | Operation::DocAppend { path, .. }
            | Operation::DocPrepend { path, .. }
            | Operation::DocEnsure { path, .. }
            | Operation::DocDeleteWhere { path, .. }
            | Operation::DocUpdate { path, .. }
            | Operation::DocMove { path, .. }
            | Operation::MdReplaceSection { path, .. }
            | Operation::MdInsertAfterHeading { path, .. }
            | Operation::MdInsertBeforeHeading { path, .. }
            | Operation::MdUpsertBullet { path, .. }
            | Operation::MdTableAppend { path, .. }
            | Operation::MdDedupeHeadings { path, .. }
            | Operation::MdLintAgents { path, .. }
            | Operation::TidyFix { path, .. }
            | Operation::FileCreate { path, .. }
            | Operation::FileDelete { path, .. }
            | Operation::Read { path, .. }
            | Operation::Search { path, .. } => vec![path.as_str()],
            Operation::MdMoveSection { path, to, .. } => {
                let mut p = vec![path.as_str()];
                if let Some(to) = to {
                    p.push(to.as_str());
                }
                p
            }
            Operation::DocMerge { path, .. } => vec![path.as_str()],
            Operation::Replace { path, glob, .. } => {
                let mut p = Vec::new();
                if let Some(path) = path {
                    p.push(path.as_str());
                }
                if let Some(glob) = glob {
                    p.push(glob.as_str());
                }
                p
            }
            Operation::FileRename { from, to, .. } => vec![from.as_str(), to.as_str()],
            Operation::PatchApply { diff } => {
                // Validate paths embedded in the unified diff text (#229).
                let patch_files = crate::ops::patch::parse_patch(diff).map_err(|e| {
                    McpError::invalid_params(
                        format!("failed to parse diff for path validation: {e}"),
                        None,
                    )
                })?;
                for pf in &patch_files {
                    validate_path_contained(&pf.path)?;
                    validate_path_resolved(&pf.path, cwd, canon_cwd)?;
                }
                vec![]
            }
        };
        for path in paths {
            validate_path_contained(path)?;
            validate_path_resolved(path, cwd, canon_cwd)?;
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
    cwd: PathBuf,
    /// Pre-canonicalized cwd, computed once at startup to avoid a
    /// `realpath` syscall on every tool invocation.
    canon_cwd: PathBuf,
    /// Whether tx plans may include format/validate lifecycle steps that
    /// execute shell commands. Controlled by `--allow-shell` on startup.
    #[expect(dead_code)]
    allow_shell: bool,
    /// Optional path for logging MCP tool calls as JSONL.
    /// Set via `--log <path>` or `PATCHLOOM_MCP_LOG` env var.
    call_log: Option<PathBuf>,
}

impl PatchloomService {
    pub fn new(cwd: PathBuf, allow_shell: bool, log_flag: Option<String>) -> anyhow::Result<Self> {
        let canon_cwd = cwd
            .canonicalize()
            .with_context(|| format!("failed to canonicalize cwd: {}", cwd.display()))?;
        // --log flag takes precedence over PATCHLOOM_MCP_LOG env var.
        let call_log = log_flag
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("PATCHLOOM_MCP_LOG").map(PathBuf::from));
        Ok(Self {
            tool_router: Self::tool_router(),
            cwd,
            canon_cwd,
            allow_shell,
            call_log,
        })
    }

    /// Validate a path for both syntactic containment and symlink resolution.
    /// Combines the two checks that must always be called together.
    fn check_path(&self, path: &str) -> Result<(), McpError> {
        validate_path_contained(path).map_err(|_| {
            McpError::invalid_params(
                format!(
                    "path rejected: {path}; use a relative path within the working directory ({})",
                    self.cwd.display()
                ),
                None,
            )
        })?;
        validate_path_resolved(path, &self.cwd, &self.canon_cwd)
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
}

/// Execute a plan whose paths have already been validated by the caller.
///
/// Each individual MCP tool validates paths via `check_path` before calling
/// this function, so no redundant validation is needed here.
fn execute_plan_validated(plan: Plan, cwd: &std::path::Path) -> Result<CallToolResult, McpError> {
    let (code, json) = crate::cmd::tx::execute_plan_direct(plan, cwd)
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
    Plan {
        version: crate::plan::SCHEMA_VERSION.to_string(),
        cwd: None,
        write_policy: None,
        strict: false,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocSet {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocDelete {
                path: p.path,
                selector: p.selector,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocMerge {
                path: p.path,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocAppend {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocPrepend {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocEnsure {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocDeleteWhere {
                path: p.path,
                selector: p.selector,
                predicate: p.predicate,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocUpdate {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::DocMove {
                path: p.path,
                from: p.from,
                to: p.to,
            }]),
            &self.cwd,
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
        let abs = self.cwd.join(&p.path);
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
        let abs = self.cwd.join(&p.path);
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
        let abs_a = self.cwd.join(&p.file_a);
        let abs_b = self.cwd.join(&p.file_b);
        let action = crate::cmd::doc::DocAction::Diff {
            file_a: abs_a.to_string_lossy().into_owned(),
            file_b: abs_b.to_string_lossy().into_owned(),
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Search text files for a pattern (regex by default, use literal=true for exact match). Returns matches with line numbers and context. Options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true}"
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
        };
        let global = GlobalFlags {
            json: true,
            cwd: Some(self.cwd.to_string_lossy().into_owned()),
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
            cwd: Some(self.cwd.to_string_lossy().into_owned()),
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
            &self.cwd,
        )
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range. Set whole_line=true to replace entire lines containing a match (use with to=\"\" to delete lines). Example: {\"path\": \"README.md\", \"from\": \"1.0.0\", \"to\": \"2.0.0\"}"
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
        let mode = if p.regex {
            Some("regex".to_string())
        } else {
            None
        };
        execute_plan_validated(
            make_plan(vec![Operation::Replace {
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
            }]),
            &self.cwd,
        )
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
        execute_plan_validated(
            make_plan(vec![Operation::MdUpsertBullet {
                path: p.path,
                heading: p.heading,
                bullet: p.bullet,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::MdTableAppend {
                path: p.path,
                heading: p.heading,
                row: p.row,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::MdReplaceSection {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::MdInsertAfterHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
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
        execute_plan_validated(
            make_plan(vec![Operation::MdInsertBeforeHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
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
            &self.cwd,
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
        let abs = self.cwd.join(&p.path);
        let content = std::fs::read_to_string(&abs)
            .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
        let issues = crate::cmd::md::lint_agents_content(&content);
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
        execute_plan_validated(
            make_plan(vec![Operation::TidyFix {
                path: p.path,
                ensure_final_newline: Some(true),
                trim_trailing_whitespace: Some(true),
                normalize_eol: None,
            }]),
            &self.cwd,
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

        let src = self.cwd.join(&p.from);
        let dst = self.cwd.join(&p.to);
        match crate::cmd::rename::apply_rename(&src, &dst, p.force, &self.cwd) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Renamed {} -> {}",
                p.from, p.to
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

        let abs = self.cwd.join(&p.path);
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

        let abs = self.cwd.join(&p.path);
        match crate::cmd::delete::apply_delete(&abs, &self.cwd) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Deleted {}",
                p.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(
        description = "Apply a unified diff (patch). The diff parameter is the full unified diff text. Supports multi-file diffs. Example: {\"diff\": \"--- a/file.txt\\n+++ b/file.txt\\n@@ -1 +1 @@\\n-old\\n+new\"}"
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
            make_plan(vec![Operation::PatchApply { diff: p.diff }]),
            &self.cwd,
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
            })
            .collect();
        execute_plan_validated(make_plan(ops), &self.cwd)
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
        execute_plan_validated(make_plan(ops), &self.cwd)
    }
}

impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use these tools for ALL file operations. Use batch_replace and batch_tidy when applying the same operation to multiple files.",
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
pub fn run_mcp_server(
    global: &GlobalFlags,
    allow_shell: bool,
    log: Option<String>,
) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;
    let service = PatchloomService::new(cwd, allow_shell, log)?;

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
        spawn_test_client_with_shell(cwd, false).await
    }

    async fn spawn_test_client_with_shell(
        cwd: std::path::PathBuf,
        allow_shell: bool,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let (server_transport, client_transport) = tokio::io::duplex(16384);
        let service = PatchloomService::new(cwd, allow_shell, None).unwrap();
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
                &"Search text files for a pattern (regex by default, use literal=true for exact match). Returns matches with line numbers and context. Options: files_with_matches, count, case_insensitive, multiline, invert_match, assert_count. Example: {\"pattern\": \"TODO\", \"paths\": [\"src/\"], \"literal\": true}"
            ),
            "search_files description drifted"
        );
        assert!(names.contains(&"git_status"), "missing git_status tool");
        assert!(names.contains(&"replace_text"), "missing replace_text tool");
        assert_eq!(
            descriptions.get("replace_text"),
            Some(
                &"Replace text in a file. Literal by default; set regex=true for regex. Options: nth, insert_before, insert_after, case_insensitive, multiline, if_exists, whole_line, range. Set whole_line=true to replace entire lines containing a match (use with to=\"\" to delete lines). Example: {\"path\": \"README.md\", \"from\": \"1.0.0\", \"to\": \"2.0.0\"}"
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
        assert_eq!(names.len(), 30, "expected 30 tools, got {}", names.len());
        client.cancel().await.unwrap();
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
        }];
        let canon = dir.path().canonicalize().unwrap();
        let result = validate_operation_paths(&ops, dir.path(), &canon);
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
        }];
        let canon = dir.path().canonicalize().unwrap();
        let result = validate_operation_paths(&ops, dir.path(), &canon);
        assert!(result.is_ok(), "PatchApply with safe paths should pass");
    }

    #[test]
    fn path_rejects_absolute() {
        assert!(
            validate_path_contained("/etc/passwd").is_err(),
            "absolute path should be rejected"
        );
    }

    #[test]
    fn path_rejects_traversal() {
        assert!(
            validate_path_contained("../../etc/passwd").is_err(),
            "parent traversal should be rejected"
        );
    }

    #[test]
    fn path_rejects_deep_traversal() {
        assert!(
            validate_path_contained("a/b/../../../escape").is_err(),
            "deep traversal escaping cwd should be rejected"
        );
    }

    #[test]
    fn path_allows_relative() {
        assert!(
            validate_path_contained("src/main.rs").is_ok(),
            "simple relative path should be allowed"
        );
    }

    #[test]
    fn path_allows_dot_relative() {
        assert!(
            validate_path_contained("./foo/bar.json").is_ok(),
            "dot-relative path should be allowed"
        );
    }

    #[test]
    fn path_allows_safe_parent() {
        // a/../b resolves within cwd
        assert!(
            validate_path_contained("a/../b").is_ok(),
            "parent that resolves within cwd should be allowed"
        );
    }

    #[test]
    fn path_rejects_single_parent() {
        assert!(
            validate_path_contained("..").is_err(),
            "bare parent directory should be rejected"
        );
    }

    #[test]
    fn resolved_path_allows_existing_file_inside_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("safe.txt"), "ok").unwrap();
        let canon = dir.path().canonicalize().unwrap();
        assert!(
            validate_path_resolved("safe.txt", dir.path(), &canon).is_ok(),
            "existing file inside cwd should be allowed"
        );
    }

    #[test]
    fn resolved_path_allows_nonexistent_file() {
        let dir = tempfile::TempDir::new().unwrap();
        // Non-existent files with safe ancestors are allowed.
        let canon = dir.path().canonicalize().unwrap();
        assert!(
            validate_path_resolved("new_file.txt", dir.path(), &canon).is_ok(),
            "nonexistent file with safe ancestor should be allowed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolved_path_rejects_symlink_escaping_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("escape");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let result = validate_path_resolved("escape", dir.path(), &canon);
        assert!(result.is_err(), "symlink escaping cwd should be rejected");
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

    #[cfg(unix)]
    #[test]
    fn resolved_path_rejects_new_file_through_symlink_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("link_dir");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        // The file doesn't exist, but the parent is a symlink outside cwd.
        let canon = dir.path().canonicalize().unwrap();
        let result = validate_path_resolved("link_dir/new_file.txt", dir.path(), &canon);
        assert!(
            result.is_err(),
            "new file through symlink escaping cwd should be rejected"
        );
    }

    /// Spawn a test client with JSONL logging enabled.
    async fn spawn_test_client_with_log(
        cwd: std::path::PathBuf,
        log_path: std::path::PathBuf,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let (server_transport, client_transport) = tokio::io::duplex(16384);
        let service =
            PatchloomService::new(cwd, false, Some(log_path.to_string_lossy().into_owned()))
                .unwrap();
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
}
