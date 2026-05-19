//! MCP (Model Context Protocol) server for patchloom.
//!
//! Exposes patchloom operations as structured MCP tools that AI agents
//! can call directly, eliminating the shell-command construction tax.
//!
//! Run with: `patchloom mcp-server`

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData as McpError, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use serde::Deserialize;
use std::path::PathBuf;

use crate::plan::{Operation, Plan};

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocSetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Key/selector for the value to set (e.g., "version", "db.pool", "items[0].name").
    pub key: String,
    /// Value to set (string, number, boolean, object, or array).
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocDeleteParams {
    /// File path.
    pub path: String,
    /// Key/selector to delete.
    pub key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocMergeParams {
    /// File path.
    pub path: String,
    /// Object to deep-merge into the document root.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocArrayParams {
    /// File path.
    pub path: String,
    /// Key/selector pointing to the target array.
    pub key: String,
    /// Value to append or prepend.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocEnsureParams {
    /// File path.
    pub path: String,
    /// Key/selector to ensure exists.
    pub key: String,
    /// Value to set only if the key is missing.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocDeleteWhereParams {
    /// File path.
    pub path: String,
    /// Key/selector pointing to the target array.
    pub key: String,
    /// Predicate for items to delete (e.g., "name=obsolete").
    pub predicate: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplaceParams {
    /// File path.
    pub path: String,
    /// Text to find.
    pub from: String,
    /// Text to replace with.
    pub to: String,
    /// Use regex mode for the `from` pattern.
    #[serde(default)]
    pub regex: bool,
    /// Replace only the Nth match (1-based). Default: replace all.
    pub nth: Option<usize>,
    /// Case-insensitive matching.
    #[serde(default)]
    pub case_insensitive: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MdUpsertBulletParams {
    /// Markdown file path.
    pub path: String,
    /// Heading to find (e.g., "## Rules").
    pub heading: String,
    /// Bullet text to add (e.g., "- New rule"). Skipped if already present.
    pub bullet: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MdTableAppendParams {
    /// Markdown file path.
    pub path: String,
    /// Heading above the target table.
    pub heading: String,
    /// Table row to append (e.g., "| col1 | col2 |").
    pub row: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MdReplaceSectionParams {
    /// Markdown file path.
    pub path: String,
    /// Heading of the section to replace.
    pub heading: String,
    /// New content for the section body.
    pub content: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HygieneParams {
    /// File path to normalize.
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchParams {
    /// List of operations, one per string, in line-oriented batch format.
    /// Example: ["doc.set config.json version \"2.0.0\"", "replace README.md \"old\" \"new\""]
    pub operations: Vec<String>,
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

/// Validate all paths in a list of operations parsed from batch input.
fn validate_operation_paths(operations: &[Operation]) -> Result<(), McpError> {
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
            | Operation::HygieneFix { path, .. }
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
            Operation::PatchApply { .. } => vec![], // paths are in diff text
        };
        for path in paths {
            validate_path_contained(path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchloomService {
    #[allow(dead_code)] // Used by tool_router macro-generated code.
    tool_router: ToolRouter<Self>,
    cwd: PathBuf,
}

impl PatchloomService {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            cwd,
        }
    }
}

/// Execute a plan by invoking patchloom as a subprocess.
///
/// We use subprocess execution instead of calling the library directly because
/// the MCP server owns stdout for JSON-RPC. Any patchloom output to stdout
/// would corrupt the MCP protocol stream.
fn execute_plan(plan: Plan, cwd: &std::path::Path) -> Result<CallToolResult, McpError> {
    let plan_json = serde_json::to_string(&plan)
        .map_err(|e| McpError::internal_error(format!("failed to serialize plan: {e}"), None))?;

    let tmp = tempfile::NamedTempFile::new()
        .map_err(|e| McpError::internal_error(format!("failed to create temp file: {e}"), None))?;
    std::fs::write(tmp.path(), &plan_json)
        .map_err(|e| McpError::internal_error(format!("failed to write plan: {e}"), None))?;

    let exe = std::env::current_exe()
        .map_err(|e| McpError::internal_error(format!("failed to get current exe: {e}"), None))?;

    let output = std::process::Command::new(&exe)
        .args(["--json", "tx", "--plan"])
        .arg(tmp.path())
        .arg("--apply")
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| McpError::internal_error(format!("failed to run patchloom: {e}"), None))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let code = output.status.code().unwrap_or(1);

    if code == crate::exit::SUCCESS as i32 {
        let msg = if stdout.trim().is_empty() {
            "Operation completed successfully.".to_string()
        } else {
            stdout.trim().to_string()
        };
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Operation failed with exit code {code}.")
        };
        Ok(CallToolResult::error(vec![Content::text(msg)]))
    }
}

fn make_plan(operations: Vec<Operation>) -> Plan {
    Plan {
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
        description = "Set a key in a JSON, YAML, or TOML file. Parser-backed, preserves comments."
    )]
    async fn patchloom_doc_set(
        &self,
        Parameters(p): Parameters<DocSetParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocSet {
                path: p.path,
                key: p.key,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Delete a key from a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_delete(
        &self,
        Parameters(p): Parameters<DocDeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocDelete {
                path: p.path,
                key: p.key,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Deep-merge an object into a JSON, YAML, or TOML document.")]
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

    #[tool(description = "Append a value to an array in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_append(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocAppend {
                path: p.path,
                key: p.key,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Prepend a value to an array in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_prepend(
        &self,
        Parameters(p): Parameters<DocArrayParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocPrepend {
                path: p.path,
                key: p.key,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Set a key in JSON/YAML/TOML only if it does not already exist.")]
    async fn patchloom_doc_ensure(
        &self,
        Parameters(p): Parameters<DocEnsureParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocEnsure {
                path: p.path,
                key: p.key,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Remove array items matching a predicate from JSON/YAML/TOML.")]
    async fn patchloom_doc_delete_where(
        &self,
        Parameters(p): Parameters<DocDeleteWhereParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocDeleteWhere {
                path: p.path,
                key: p.key,
                predicate: p.predicate,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Replace text in a file. Supports literal and regex modes.")]
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
                to: Some(p.to),
                nth: p.nth,
                insert_before: None,
                insert_after: None,
                case_insensitive: p.case_insensitive,
                multiline: false,
                if_exists: false,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Add a bullet under a markdown heading. Skipped if already present.")]
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

    #[tool(description = "Append a row to a markdown table under a heading.")]
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

    #[tool(description = "Replace the body of a heading section in a markdown file.")]
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

    #[tool(description = "Fix whitespace: ensure final newline, trim trailing spaces.")]
    async fn patchloom_hygiene(
        &self,
        Parameters(p): Parameters<HygieneParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::HygieneFix {
                path: p.path,
                ensure_final_newline: Some(true),
                trim_trailing_whitespace: Some(true),
                normalize_eol: None,
            }]),
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

        validate_operation_paths(&operations)?;
        execute_plan(make_plan(operations), &self.cwd)
    }
}

#[tool_handler]
impl ServerHandler for PatchloomService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Patchloom provides structured file editing tools for JSON, YAML, TOML, \
                     and Markdown files. Use patchloom_doc_* for config file edits, \
                     patchloom_md_* for markdown edits, patchloom_replace for text replacement, \
                     and patchloom_batch for multi-file atomic edits.",
            );
        info.server_info.name = "patchloom".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info
    }
}

/// Run the MCP server on stdio.
pub fn run_mcp_server() -> anyhow::Result<u8> {
    let cwd = std::env::current_dir()?;
    let service = PatchloomService::new(cwd);

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
}
