//! Resource limits and input validation for MCP tools.

use rmcp::model::ErrorData as McpError;

pub(super) fn default_strict_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Maximum size for content/diff fields (10 MiB).
pub(super) const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum size for pattern/selector string fields (1 MiB).
pub(super) const MAX_PARAM_BYTES: usize = 1024 * 1024;

/// Maximum number of files in a batch operation.
pub(super) const MAX_BATCH_FILES: usize = 1000;

/// Maximum nesting depth for JSON value parameters.
pub(super) const MAX_JSON_DEPTH: usize = 64;

pub(super) fn validate_size(field: &str, value: &str, max_bytes: usize) -> Result<(), McpError> {
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

pub(super) fn validate_content_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_CONTENT_BYTES)
}

pub(super) fn validate_param_size(field: &str, value: &str) -> Result<(), McpError> {
    validate_size(field, value, MAX_PARAM_BYTES)
}

pub(super) fn validate_json_depth(field: &str, value: &serde_json::Value) -> Result<(), McpError> {
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

pub(super) fn validate_batch_size(field: &str, count: usize) -> Result<(), McpError> {
    if count > MAX_BATCH_FILES {
        return Err(McpError::invalid_params(
            format!("{field} exceeds maximum batch size ({count}, limit {MAX_BATCH_FILES})"),
            None,
        ));
    }
    Ok(())
}

/// Apply the same content / param / batch / json-depth limits as `handle_simple_op`.
///
/// MCP `execute_plan` is a custom handler and never walked inline ops (#2463).
/// CLI `tx` does not call this.
pub(super) fn validate_plan_ops_size_limits(
    ops: &[crate::plan::Operation],
) -> Result<(), McpError> {
    validate_batch_size("operations", ops.len())?;
    for op in ops {
        validate_operation_size_limits(op)?;
    }
    Ok(())
}

fn validate_operation_size_limits(op: &crate::plan::Operation) -> Result<(), McpError> {
    let value = serde_json::to_value(op).map_err(|e| {
        McpError::invalid_params(format!("failed to serialize operation: {e}"), None)
    })?;
    let Some(obj) = value.as_object() else {
        return Ok(());
    };
    const CONTENT_FIELDS: &[&str] = &[
        "content",
        "diff",
        "fragment",
        "new",
        "insert_before",
        "insert_after",
        "before_context",
        "after_context",
        "replacement",
        "prepend",
        "source_suffix",
        "preamble",
        "wrapper",
        "new_signature",
    ];
    const PARAM_FIELDS: &[&str] = &[
        "old",
        "selector",
        "pattern",
        "heading",
        "predicate",
        "from",
        "to",
        "after",
        "before",
        "instruction",
        "bullet",
        "row",
        "symbol",
        "query",
    ];
    for field in CONTENT_FIELDS {
        if let Some(s) = obj.get(*field).and_then(|v| v.as_str()) {
            validate_content_size(field, s)?;
        }
    }
    for field in PARAM_FIELDS {
        if let Some(s) = obj.get(*field).and_then(|v| v.as_str()) {
            validate_param_size(field, s)?;
        }
    }
    if let Some(val) = obj.get("value") {
        validate_json_depth("value", val)?;
    }
    if let Some(arr) = obj.get("files").and_then(|v| v.as_array()) {
        validate_batch_size("files", arr.len())?;
    }
    Ok(())
}
