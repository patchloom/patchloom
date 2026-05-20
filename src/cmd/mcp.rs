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
pub struct DocUpdateParams {
    /// File path.
    pub path: String,
    /// Wildcard selector for items to update (e.g., "items[*]").
    pub key: String,
    /// Value to set on each matched item.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocMoveParams {
    /// File path.
    pub path: String,
    /// Selector of the key to move.
    pub from: String,
    /// New selector path for the key.
    pub to: String,
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocGetParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Key/selector for the value to read (e.g., "version", "db.pool").
    pub key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadFileParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Optional line range (e.g., "10:20"). Omit to read the entire file.
    pub lines: Option<String>,
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
pub struct BatchParams {
    /// List of operations, one per string, in line-oriented batch format.
    /// Example: ["doc.set config.json version \"2.0.0\"", "replace README.md \"old\" \"new\""]
    pub operations: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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
    /// Lines of context around matches.
    pub context: Option<usize>,
    /// Only return file paths with matches (not match details).
    #[serde(default)]
    pub files_with_matches: bool,
    /// Only return match counts per file.
    #[serde(default)]
    pub count: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocSelectorParams {
    /// File path (relative to working directory).
    pub path: String,
    /// Key/selector to query (e.g., "version", "db.pool", "items[*]").
    pub key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DocFileParams {
    /// File path (relative to working directory).
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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
    // Validate all operation paths for containment (syntactic + symlink).
    validate_operation_paths(&plan.operations, cwd)?;

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

/// Run a read-only patchloom subcommand and return its output as a tool result.
fn run_readonly_command(args: &[&str], cwd: &std::path::Path) -> Result<CallToolResult, McpError> {
    let exe = std::env::current_exe()
        .map_err(|e| McpError::internal_error(format!("failed to get current exe: {e}"), None))?;
    let output = std::process::Command::new(&exe)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| McpError::internal_error(format!("failed to run patchloom: {e}"), None))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        let msg = if stdout.trim().is_empty() {
            "No results.".to_string()
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
            format!(
                "Command failed with exit code {}.",
                output.status.code().unwrap_or(1)
            )
        };
        Ok(CallToolResult::error(vec![Content::text(msg)]))
    }
}

fn make_plan(operations: Vec<Operation>) -> Plan {
    Plan {
        version: None,
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

    #[tool(
        description = "Update all items matching a wildcard selector in a JSON, YAML, or TOML file."
    )]
    async fn patchloom_doc_update(
        &self,
        Parameters(p): Parameters<DocUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        execute_plan(
            make_plan(vec![Operation::DocUpdate {
                path: p.path,
                key: p.key,
                value: p.value,
            }]),
            &self.cwd,
        )
    }

    #[tool(description = "Move/rename a key in a JSON, YAML, or TOML file.")]
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

    #[tool(description = "Read a value from a JSON, YAML, or TOML file by key path.")]
    async fn patchloom_doc_get(
        &self,
        Parameters(p): Parameters<DocGetParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "get", &abs.to_string_lossy(), &p.key],
            &self.cwd,
        )
    }

    #[tool(description = "Check whether a key path exists in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_has(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "has", &abs.to_string_lossy(), &p.key],
            &self.cwd,
        )
    }

    #[tool(description = "List object keys at a path in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_keys(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "keys", &abs.to_string_lossy(), &p.key],
            &self.cwd,
        )
    }

    #[tool(
        description = "Count items in an array or keys in an object in a JSON, YAML, or TOML file."
    )]
    async fn patchloom_doc_len(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "len", &abs.to_string_lossy(), &p.key],
            &self.cwd,
        )
    }

    #[tool(description = "Filter array items by selector in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_select(
        &self,
        Parameters(p): Parameters<DocSelectorParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "select", &abs.to_string_lossy(), &p.key],
            &self.cwd,
        )
    }

    #[tool(description = "List all leaf key paths and values in a JSON, YAML, or TOML file.")]
    async fn patchloom_doc_flatten(
        &self,
        Parameters(p): Parameters<DocFileParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.path)?;
        validate_path_resolved(&p.path, &self.cwd)?;
        let abs = self.cwd.join(&p.path);
        run_readonly_command(
            &["--json", "doc", "flatten", &abs.to_string_lossy()],
            &self.cwd,
        )
    }

    #[tool(
        description = "Compare two structured files (JSON, YAML, or TOML) and show differences."
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
        run_readonly_command(
            &[
                "--json",
                "doc",
                "diff",
                &abs_a.to_string_lossy(),
                &abs_b.to_string_lossy(),
            ],
            &self.cwd,
        )
    }

    #[tool(
        description = "Search files for a pattern. Returns matches with line numbers and context."
    )]
    async fn patchloom_search(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        for path in &p.paths {
            validate_path_contained(path)?;
            validate_path_resolved(path, &self.cwd)?;
        }
        let mut args: Vec<String> = vec!["--json".into(), "search".into()];
        if p.literal {
            args.push("--literal".into());
        }
        if p.case_insensitive {
            args.push("-i".into());
        }
        if p.files_with_matches {
            args.push("--files-with-matches".into());
        }
        if p.count {
            args.push("--count".into());
        }
        if let Some(ctx) = p.context {
            args.push("-C".into());
            args.push(ctx.to_string());
        }
        args.push(p.pattern);
        if p.paths.is_empty() {
            args.push(".".into());
        } else {
            args.extend(p.paths);
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        run_readonly_command(&arg_refs, &self.cwd)
    }

    #[tool(description = "Show uncommitted file changes vs git HEAD.")]
    async fn patchloom_status(
        &self,
        #[allow(unused_variables)] Parameters(p): Parameters<serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        run_readonly_command(&["--json", "status"], &self.cwd)
    }

    #[tool(description = "Read file contents with optional line range.")]
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
        description = "Replace text in a file. Supports literal and regex modes, plus insert_before/insert_after."
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
        description = "Rename (move) a file. Handles binary files. Use force to overwrite an existing destination."
    )]
    async fn patchloom_file_rename(
        &self,
        Parameters(p): Parameters<FileRenameParams>,
    ) -> Result<CallToolResult, McpError> {
        validate_path_contained(&p.from)?;
        validate_path_contained(&p.to)?;
        validate_path_resolved(&p.from, &self.cwd)?;
        validate_path_resolved(&p.to, &self.cwd)?;

        // Use the standalone rename command (handles binary files via
        // fs::rename) instead of going through the tx engine.
        let exe = std::env::current_exe().map_err(|e| {
            McpError::internal_error(format!("failed to get current exe: {e}"), None)
        })?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(["--json", "rename", "--from"])
            .arg(&p.from)
            .arg("--to")
            .arg(&p.to)
            .arg("--apply");
        if p.force {
            cmd.arg("--force");
        }
        let output = cmd
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| McpError::internal_error(format!("failed to run patchloom: {e}"), None))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let code = output.status.code().unwrap_or(1);

        if code == crate::exit::SUCCESS as i32 {
            let msg = if stdout.trim().is_empty() {
                format!("Renamed {} -> {}", p.from, p.to)
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
                format!("Rename failed with exit code {code}.")
            };
            Ok(CallToolResult::error(vec![Content::text(msg)]))
        }
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
                     patchloom_md_* for markdown edits, patchloom_replace for text replacement, \
                     patchloom_search for file content search, patchloom_read for reading files, \
                     patchloom_status for git status, patchloom_file_rename for renaming files, \
                     patchloom_hygiene for whitespace fixes, and patchloom_batch for atomic edits.",
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
    use rmcp::ServiceExt;

    /// Spin up a PatchloomService over a duplex stream and return the
    /// connected client handle.
    async fn spawn_test_client(
        cwd: std::path::PathBuf,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let (server_transport, client_transport) = tokio::io::duplex(16384);
        let service = PatchloomService::new(cwd);
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
        assert!(names.contains(&"patchloom_hygiene"), "missing hygiene tool");
        assert!(
            names.contains(&"patchloom_file_rename"),
            "missing file_rename tool"
        );
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
                "key": "root",
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
