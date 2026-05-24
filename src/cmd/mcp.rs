//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData as McpError, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
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
pub struct TxParams {
    /// Transaction plan as a JSON, YAML, or TOML string. Must include
    /// "version" and "operations". May include "format", "validate",
    /// "write_policy", "strict", and "cwd".
    pub plan: String,
    /// Plan format: "json", "yaml", or "toml". Auto-detected if omitted.
    pub format: Option<String>,
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
pub struct BatchParams {
    /// List of operations, one per string, in line-oriented batch format.
    /// Example: ["doc.set config.json version \"2.0.0\"", "replace README.md \"old\" \"new\""]
    pub operations: Vec<String>,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocSelectorParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Selector to query (e.g., "version", "db.pool", "items[*]").
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocFileParams {
    /// File path (relative to working directory).
    pub path: String,
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
fn validate_path_resolved(path: &str, cwd: &std::path::Path) -> Result<(), McpError> {
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
    let canon_cwd = cwd
        .canonicalize()
        .map_err(|e| McpError::internal_error(format!("failed to canonicalize cwd: {e}"), None))?;
    let canon_path = check_target.canonicalize().map_err(|e| {
        McpError::internal_error(format!("failed to canonicalize path '{path}': {e}"), None)
    })?;
    if !canon_path.starts_with(&canon_cwd) {
        return Err(McpError::invalid_params(
            format!("resolved path escapes working directory: {path}"),
            None,
        ));
    }
    Ok(())
}

/// Validate all paths in a list of operations.
/// Checks both syntactic containment and symlink resolution.
fn validate_operation_paths(
    operations: &[Operation],
    cwd: &std::path::Path,
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
            | Operation::TidyFix { path, .. }
            | Operation::FileCreate { path, .. }
            | Operation::FileDelete { path, .. }
            | Operation::Read { path, .. }
            | Operation::Search { path, .. } => vec![path.as_str()],
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
                    validate_path_resolved(&pf.path, cwd)?;
                }
                vec![]
            }
        };
        for path in paths {
            validate_path_contained(path)?;
            validate_path_resolved(path, cwd)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchloomService {
    #[expect(dead_code)] // Used by tool_router macro-generated code.
    tool_router: ToolRouter<Self>,
    cwd: PathBuf,
    /// Whether tx plans may include format/validate lifecycle steps that
    /// execute shell commands. Controlled by `--allow-shell` on startup.
    allow_shell: bool,
}

impl PatchloomService {
    pub fn new(cwd: PathBuf, allow_shell: bool) -> Self {
        Self {
            tool_router: Self::tool_router(),
            cwd,
            allow_shell,
        }
    }
}

/// Execute a plan by calling the tx engine directly (no subprocess).
fn execute_plan(plan: Plan, cwd: &std::path::Path) -> Result<CallToolResult, McpError> {
    // Validate all operation paths for containment (syntactic + symlink).
    validate_operation_paths(&plan.operations, cwd)?;

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
        description = "Set a value in a JSON, YAML, or TOML file. Parser-backed, preserves comments. Use dot notation for nested paths (e.g. selector='server.port', value='8080')."
    )]
    async fn patchloom_doc_set(
        &self,
        Parameters(p): Parameters<DocSetParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocSet {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Delete a value from a JSON, YAML, or TOML file. Use dot notation for nested paths (e.g. selector='scripts.test')."
    )]
    async fn patchloom_doc_delete(
        &self,
        Parameters(p): Parameters<DocDeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocDelete {
                path: p.path,
                selector: p.selector,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Deep-merge an object into a JSON, YAML, or TOML document. The value is a JSON object that gets recursively merged (e.g. value='{\"server\": {\"port\": 8080}}')."
    )]
    async fn patchloom_doc_merge(
        &self,
        Parameters(p): Parameters<DocMergeParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocMerge {
                path: p.path,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Append a value to an array in a JSON, YAML, or TOML file. Specify the array path with selector (e.g. selector='dependencies', value='\"new-pkg\"')."
    )]
    async fn patchloom_doc_append(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
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
    async fn patchloom_doc_prepend(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocPrepend {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Set a value in JSON/YAML/TOML only if it does not already exist. Idempotent: no-op if the selector path is present."
    )]
    async fn patchloom_doc_ensure(
        &self,
        Parameters(p): Parameters<DocEnsureParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocEnsure {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Remove array items matching a predicate from JSON/YAML/TOML. Use selector for the array path and predicate as 'field=value' (e.g. selector='users', predicate='role=admin')."
    )]
    async fn patchloom_doc_delete_where(
        &self,
        Parameters(p): Parameters<DocDeleteWhereParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocDeleteWhere {
                path: p.path,
                selector: p.selector,
                predicate: p.predicate,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Update all items matching a wildcard selector in a JSON, YAML, or TOML file. Use [*] for wildcards (e.g. selector='servers[*].port', value='8080')."
    )]
    async fn patchloom_doc_update(
        &self,
        Parameters(p): Parameters<DocUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocUpdate {
                path: p.path,
                selector: p.selector,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Move/rename a path in a JSON, YAML, or TOML file. Moves the value from one selector to another (e.g. from='old_name', to='new_name')."
    )]
    async fn patchloom_doc_move(
        &self,
        Parameters(p): Parameters<DocMoveParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocMove {
                path: p.path,
                from: p.from,
                to: p.to,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Read a value from a JSON, YAML, or TOML file by selector path. Use dot notation for nesting, brackets for arrays (e.g. selector='servers[0].host')."
    )]
    async fn patchloom_doc_get(
        &self,
        Parameters(p): Parameters<DocGetParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Get {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Check whether a selector path exists in a JSON, YAML, or TOML file. Returns true/false."
    )]
    async fn patchloom_doc_has(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Has {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "List object keys at a selector path in a JSON, YAML, or TOML file. Returns an array of key names."
    )]
    async fn patchloom_doc_keys(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Keys {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Count items in an array or keys in an object in a JSON, YAML, or TOML file. Returns an integer."
    )]
    async fn patchloom_doc_len(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Len {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Filter array items by selector in a JSON, YAML, or TOML file. Use predicate selectors (e.g. selector='users[role=admin]')."
    )]
    async fn patchloom_doc_select(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Select {
            file: abs.to_string_lossy().into_owned(),
            selector: p.selector,
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "List all leaf selector paths and values in a JSON, YAML, or TOML file. Returns every path=value pair for exploring unknown file structure."
    )]
    async fn patchloom_doc_flatten(
        &self,
        Parameters(p): Parameters<DocFileParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let action = crate::cmd::doc::DocAction::Flatten {
            file: abs.to_string_lossy().into_owned(),
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Compare two structured files (JSON, YAML, or TOML) and show differences. Reports added, removed, and changed keys."
    )]
    async fn patchloom_doc_diff(
        &self,
        Parameters(p): Parameters<DocDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.file_a)?;
        validate_path_contained(&p.file_b)?;
        validate_path_resolved(&p.file_a, &self.cwd)?;
        validate_path_resolved(&p.file_b, &self.cwd)?;
        let abs_a = self.cwd.join(&p.file_a);
        let abs_b = self.cwd.join(&p.file_b);
        let action = crate::cmd::doc::DocAction::Diff {
            file_a: abs_a.to_string_lossy().into_owned(),
            file_b: abs_b.to_string_lossy().into_owned(),
        };
        doc_readonly(&action)
    }

    #[tool(
        description = "Search files for a pattern (regex by default, use literal=true for exact match). Returns matches with line numbers and context. Set files_with_matches=true for file paths only, count=true for counts."
    )]
    async fn patchloom_search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.files_with_matches && p.count {
            return Err(McpError::invalid_params(
                "files_with_matches and count cannot be combined",
                None,
            ));
        }
        for path in &p.paths {
            validate_path_contained(path)?;
            validate_path_resolved(path, &self.cwd)?;
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
            invert_match: false,
            multiline: p.multiline,
            case_insensitive: p.case_insensitive,
            assert_count: None,
        };
        let global = GlobalFlags {
            json: true,
            cwd: Some(self.cwd.to_string_lossy().into_owned()),
            ..GlobalFlags::default()
        };
        let results = crate::cmd::search::collect_matches(&search_args, &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

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
        description = "Show uncommitted file changes vs git HEAD. Returns lists of modified, created, and deleted files."
    )]
    async fn patchloom_status(
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
        description = "Read file contents with optional line range. Use lines='50:120' to read a specific range (1-based inclusive)."
    )]
    async fn patchloom_read(
        &self,
        Parameters(p): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        execute_plan(
            make_plan(vec![Operation::Read {
                path: p.path,
                lines: p.lines,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Replace text in a file. Literal by default; set regex=true for regex patterns. Use nth=N to replace only the Nth match. Use insert_before/insert_after instead of 'to' to insert around matches."
    )]
    async fn patchloom_replace(
        &self,
        Parameters(p): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        let mode = if p.regex {
            Some("regex".to_string())
        } else {
            None
        };
        execute_plan(
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
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Add a bullet under a markdown heading. Idempotent: skipped if the bullet text already exists under that heading."
    )]
    async fn patchloom_md_upsert_bullet(
        &self,
        Parameters(p): Parameters<MdUpsertBulletParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::MdUpsertBullet {
                path: p.path,
                heading: p.heading,
                bullet: p.bullet,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Append a row to a markdown table under a heading. The row is a pipe-separated string (e.g. row='| name | value |')."
    )]
    async fn patchloom_md_table_append(
        &self,
        Parameters(p): Parameters<MdTableAppendParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::MdTableAppend {
                path: p.path,
                heading: p.heading,
                row: p.row,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Replace the body of a heading section in a markdown file. Content replaces everything between this heading and the next heading of equal or higher level."
    )]
    async fn patchloom_md_replace_section(
        &self,
        Parameters(p): Parameters<MdReplaceSectionParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::MdReplaceSection {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Insert content immediately after a markdown heading. Preserves existing section body; the new content appears between the heading and the old body."
    )]
    async fn patchloom_md_insert_after_heading(
        &self,
        Parameters(p): Parameters<MdInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::MdInsertAfterHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Insert content immediately before a markdown heading. The new content appears above the heading line."
    )]
    async fn patchloom_md_insert_before_heading(
        &self,
        Parameters(p): Parameters<MdInsertParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::MdInsertBeforeHeading {
                path: p.path,
                heading: p.heading,
                content: p.content,
            }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Lint an AGENTS.md (or similar markdown rules file) for common problems: duplicate headings, dangerous git commands outside code fences, and missing final newline. Returns a JSON array of issues found (empty array means clean)."
    )]
    async fn patchloom_md_lint_agents(
        &self,
        Parameters(p): Parameters<MdLintAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        let content = std::fs::read_to_string(&abs)
            .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
        let issues = crate::cmd::md::lint_agents_content(&content);
        let json = serde_json::to_string_pretty(&issues)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Execute a full transaction plan. Accepts a JSON/YAML/TOML plan string with operations, optional format/validate lifecycle steps, strict mode, and write_policy. This is the most powerful tool: use it when you need atomic multi-file edits with post-write formatting (cargo fmt, prettier), validation (tests, lints), and rollback on failure."
    )]
    async fn patchloom_tx(
        &self,
        Parameters(p): Parameters<TxParams>,
    ) -> Result<CallToolResult, McpError> {
        let plan =
            crate::plan::parse_plan_auto(&p.plan, None, p.format.as_deref()).map_err(|e| {
                McpError::invalid_params(format!("failed to parse transaction plan: {e}"), None)
            })?;

        // Gate: format/validate lifecycle steps execute shell commands.
        // Require --allow-shell on the MCP server to permit them.
        let has_shell_steps = plan.format.as_ref().is_some_and(|f| !f.is_empty())
            || plan.validate.as_ref().is_some_and(|v| !v.is_empty());
        if has_shell_steps && !self.allow_shell {
            return Err(McpError::invalid_params(
                "this plan contains format/validate steps that execute shell commands; \
                 start the MCP server with --allow-shell to permit this"
                    .to_string(),
                None,
            ));
        }
        if has_shell_steps {
            eprintln!(
                "patchloom mcp: warning: executing shell commands from tx plan \
                 (format/validate lifecycle steps)"
            );
        }

        // Validate plan-level cwd for containment.
        if let Some(ref plan_cwd) = plan.cwd {
            let cwd_path = std::path::Path::new(plan_cwd);
            if cwd_path.is_absolute() {
                // Absolute cwd must be inside the server's working directory.
                let canon_server_cwd = self.cwd.canonicalize().map_err(|e| {
                    McpError::internal_error(
                        format!("failed to canonicalize server cwd: {e}"),
                        None,
                    )
                })?;
                let canon_plan_cwd = cwd_path.canonicalize().map_err(|e| {
                    McpError::invalid_params(
                        format!("plan cwd does not exist or is inaccessible: {e}"),
                        None,
                    )
                })?;
                if !canon_plan_cwd.starts_with(&canon_server_cwd) {
                    return Err(McpError::invalid_params(
                        format!("plan cwd must be inside the working directory: {plan_cwd}"),
                        None,
                    ));
                }
            } else {
                validate_path_contained(plan_cwd)?;
                validate_path_resolved(plan_cwd, &self.cwd)?;
            }
        }

        // Resolve effective cwd for path validation.
        let effective_cwd = if let Some(ref plan_cwd) = plan.cwd {
            let p = std::path::Path::new(plan_cwd);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                self.cwd.join(p)
            }
        } else {
            self.cwd.clone()
        };

        validate_operation_paths(&plan.operations, &effective_cwd)?;

        execute_plan(plan, &self.cwd)
    }

    #[tool(
        description = "Fix whitespace: ensures file ends with a newline and trims trailing spaces from all lines."
    )]
    async fn patchloom_tidy(
        &self,
        Parameters(p): Parameters<TidyParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
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
        description = "Rename (move) a file. Handles binary files. Use force to overwrite an existing destination."
    )]
    async fn patchloom_rename(
        &self,
        Parameters(p): Parameters<FileRenameParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.from)?;
        validate_path_contained(&p.to)?;
        validate_path_resolved(&p.from, &self.cwd)?;
        validate_path_resolved(&p.to, &self.cwd)?;

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
        description = "Create a new file with the given content. Fails if the file already exists unless force=true."
    )]
    async fn patchloom_create(
        &self,
        Parameters(p): Parameters<CreateFileParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;

        let abs = self.cwd.join(&p.path);
        match crate::cmd::create::apply_create(&abs, &p.content, p.force) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Created {}",
                p.path
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }

    #[tool(description = "Delete a file. Fails if the file does not exist.")]
    async fn patchloom_delete(
        &self,
        Parameters(p): Parameters<DeleteFileParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;

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
        description = "Apply a unified diff (patch). The diff parameter is the full unified diff text. Supports multi-file diffs."
    )]
    async fn patchloom_patch(
        &self,
        Parameters(p): Parameters<PatchParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate paths embedded in the diff.
        let patch_files = crate::ops::patch::parse_patch(&p.diff)
            .map_err(|e| McpError::invalid_params(format!("failed to parse diff: {e}"), None))?;
        for pf in &patch_files {
            validate_path_contained(&pf.path)?;
            validate_path_resolved(&pf.path, &self.cwd)?;
        }

        execute_plan(
            make_plan(vec![Operation::PatchApply { diff: p.diff }]),
            &self.cwd,
        )
    }

    #[tool(
        description = "Execute multiple operations atomically. Each string is one line in batch format: 'doc.set config.json version \"2.0\"'"
    )]
    async fn patchloom_batch(
        &self,
        Parameters(p): Parameters<BatchParams>,
    ) -> Result<CallToolResult, McpError> {
        if p.operations.len() > crate::cmd::batch::MAX_BATCH_OPERATIONS {
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Too many operations ({}, max {}).",
                p.operations.len(),
                crate::cmd::batch::MAX_BATCH_OPERATIONS
            ))]));
        }
        let mut operations = Vec::new();
        for (i, line) in p.operations.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match crate::cmd::batch::parse_line(trimmed, i + 1) {
                Ok(op) => operations.push(op),
                Err(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "Parse error: {e}",
                    ))]));
                }
            }
        }

        if operations.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "No operations provided.",
            )]));
        }

        validate_operation_paths(&operations, &self.cwd)?;
        execute_plan(make_plan(operations), &self.cwd)
    }
}

#[tool_handler]
impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Patchloom provides structured file editing and introspection tools for JSON, \
                     YAML, TOML, and Markdown files. Use patchloom_doc_set/delete/merge/append/\
                     prepend/update/move/ensure/delete_where for config file edits, \
                     patchloom_doc_get/has/keys/len/select/flatten/diff for read-only queries, \
                     patchloom_md_* for markdown edits (replace_section, insert_after_heading, \
                     insert_before_heading, upsert_bullet, table_append), \
                     patchloom_replace for text replacement, \
                     patchloom_search for file content search, patchloom_read for reading files, \
                     patchloom_status for git status, patchloom_rename for renaming files, \
                     patchloom_create for creating files, patchloom_delete for deleting files, \
                     patchloom_patch for applying unified diffs, \
                     patchloom_tidy for whitespace fixes, patchloom_batch for multi-op edits, \
                     and patchloom_tx for full transaction plans with format/validate lifecycle.",
            );
        info.server_info.name = "patchloom".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info
    }
}

/// Run the MCP server on stdio.
pub fn run_mcp_server(allow_shell: bool) -> anyhow::Result<u8> {
    let cwd = std::env::current_dir()?;
    let service = PatchloomService::new(cwd, allow_shell);

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
        let service = PatchloomService::new(cwd, allow_shell);
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
        assert!(names.contains(&"patchloom_doc_set"), "missing doc_set tool");
        assert!(names.contains(&"patchloom_doc_get"), "missing doc_get tool");
        assert!(names.contains(&"patchloom_doc_has"), "missing doc_has tool");
        assert!(
            names.contains(&"patchloom_doc_keys"),
            "missing doc_keys tool"
        );
        assert!(names.contains(&"patchloom_doc_len"), "missing doc_len tool");
        assert!(
            names.contains(&"patchloom_doc_select"),
            "missing doc_select tool"
        );
        assert!(
            names.contains(&"patchloom_doc_flatten"),
            "missing doc_flatten tool"
        );
        assert!(
            names.contains(&"patchloom_doc_diff"),
            "missing doc_diff tool"
        );
        assert!(names.contains(&"patchloom_read"), "missing read tool");
        assert!(names.contains(&"patchloom_search"), "missing search tool");
        assert!(names.contains(&"patchloom_status"), "missing status tool");
        assert!(names.contains(&"patchloom_replace"), "missing replace tool");
        assert!(names.contains(&"patchloom_batch"), "missing batch tool");
        assert!(names.contains(&"patchloom_tidy"), "missing tidy tool");
        assert!(names.contains(&"patchloom_rename"), "missing rename tool");
        assert!(names.contains(&"patchloom_create"), "missing create tool");
        assert!(names.contains(&"patchloom_delete"), "missing delete tool");
        assert!(names.contains(&"patchloom_patch"), "missing patch tool");
        assert!(
            names.contains(&"patchloom_md_insert_after_heading"),
            "missing md_insert_after_heading tool"
        );
        assert!(
            names.contains(&"patchloom_md_insert_before_heading"),
            "missing md_insert_before_heading tool"
        );
        assert!(names.contains(&"patchloom_tx"), "missing tx tool");
        assert_eq!(names.len(), 33, "expected 33 tools, got {}", names.len());
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
        let params = rmcp::model::CallToolRequestParams::new("patchloom_doc_set").with_arguments(
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
        }];
        let result = validate_operation_paths(&ops, dir.path());
        assert!(result.is_ok(), "PatchApply with safe paths should pass");
    }

    #[test]
    fn path_rejects_absolute() {
        assert!(validate_path_contained("/etc/passwd").is_err());
    }

    #[test]
    fn path_rejects_traversal() {
        assert!(validate_path_contained("../../etc/passwd").is_err());
    }

    #[test]
    fn path_rejects_deep_traversal() {
        assert!(validate_path_contained("a/b/../../../escape").is_err());
    }

    #[test]
    fn path_allows_relative() {
        assert!(validate_path_contained("src/main.rs").is_ok());
    }

    #[test]
    fn path_allows_dot_relative() {
        assert!(validate_path_contained("./foo/bar.json").is_ok());
    }

    #[test]
    fn path_allows_safe_parent() {
        // a/../b resolves within cwd
        assert!(validate_path_contained("a/../b").is_ok());
    }

    #[test]
    fn path_rejects_single_parent() {
        assert!(validate_path_contained("..").is_err());
    }

    #[test]
    fn resolved_path_allows_existing_file_inside_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("safe.txt"), "ok").unwrap();
        assert!(validate_path_resolved("safe.txt", dir.path()).is_ok());
    }

    #[test]
    fn resolved_path_allows_nonexistent_file() {
        let dir = tempfile::TempDir::new().unwrap();
        // Non-existent files with safe ancestors are allowed.
        assert!(validate_path_resolved("new_file.txt", dir.path()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn resolved_path_rejects_symlink_escaping_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("escape");
        std::os::unix::fs::symlink("/tmp", &link).unwrap();
        let result = validate_path_resolved("escape", dir.path());
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
        let result = validate_path_resolved("link_dir/new_file.txt", dir.path());
        assert!(
            result.is_err(),
            "new file through symlink escaping cwd should be rejected"
        );
    }
}
