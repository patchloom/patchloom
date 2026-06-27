//! Transaction plan format parsing.

use serde::{Deserialize, Serialize};

/// Current plan schema version.
pub const SCHEMA_VERSION: &str = "1";

fn default_strict_true() -> bool {
    true
}

/// Resolve effective strict mode: `--no-strict` > plan field > config > default true.
pub fn effective_strict(
    plan_strict: Option<bool>,
    config_strict: Option<bool>,
    no_strict: bool,
) -> bool {
    if no_strict {
        false
    } else {
        plan_strict
            .or(config_strict)
            .unwrap_or_else(default_strict_true)
    }
}

/// A transaction plan containing multiple operations to execute atomically.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[non_exhaustive]
pub struct Plan {
    /// Schema version string. Validated against the supported version.
    pub version: String,
    pub cwd: Option<String>,
    pub write_policy: Option<crate::write::WritePolicyOverride>,
    /// When omitted from the plan, defaults to strict mode at execution time.
    #[serde(default)]
    pub strict: Option<bool>,
    pub operations: Vec<Operation>,
    pub format: Option<Vec<FormatStep>>,
    pub validate: Option<Vec<ValidationStep>>,
}

/// A format step to run after applying operations but before validation.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[non_exhaustive]
pub struct FormatStep {
    pub cmd: String,
    /// Timeout in seconds (default: 60).
    pub timeout: Option<u64>,
}

/// Backward-compatible alias for [`WritePolicyOverride`](crate::write::WritePolicyOverride).
#[deprecated(
    since = "0.6.0",
    note = "use crate::write::WritePolicyOverride instead"
)]
pub type PlanWritePolicy = crate::write::WritePolicyOverride;

/// A single operation within a plan.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[non_exhaustive]
#[serde(tag = "op")]
pub enum Operation {
    #[serde(rename = "replace", alias = "replace_text")]
    Replace {
        glob: Option<String>,
        path: Option<String>,
        mode: Option<String>,
        from: String,
        to: Option<String>,
        nth: Option<usize>,
        insert_before: Option<String>,
        insert_after: Option<String>,
        #[serde(default)]
        case_insensitive: bool,
        #[serde(default)]
        multiline: bool,
        #[serde(default)]
        if_exists: bool,
        #[serde(default)]
        whole_line: bool,
        /// Line range restriction (e.g. "10:50"). Only valid with whole_line.
        range: Option<String>,
        /// Match only at word boundaries (\b). Prevents partial matches.
        #[serde(default)]
        word_boundary: bool,
        /// Context line(s) before the target for anchor-based fallback matching.
        before_context: Option<String>,
        /// Context line(s) after the target for anchor-based fallback matching.
        after_context: Option<String>,
    },
    #[serde(rename = "doc.set", alias = "doc_set")]
    DocSet {
        path: String,
        selector: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete", alias = "doc_delete")]
    DocDelete { path: String, selector: String },
    #[serde(rename = "doc.merge", alias = "doc_merge")]
    DocMerge {
        path: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.append", alias = "doc_append")]
    DocAppend {
        path: String,
        selector: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.prepend", alias = "doc_prepend")]
    DocPrepend {
        path: String,
        selector: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.update", alias = "doc_update")]
    DocUpdate {
        path: String,
        selector: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.move", alias = "doc_move")]
    DocMove {
        path: String,
        from: String,
        to: String,
    },
    #[serde(rename = "doc.ensure", alias = "doc_ensure")]
    DocEnsure {
        path: String,
        selector: String,
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete_where", alias = "doc_delete_where")]
    DocDeleteWhere {
        path: String,
        selector: String,
        predicate: String,
    },
    #[serde(rename = "md.replace_section", alias = "md_replace_section")]
    MdReplaceSection {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.insert_after_heading", alias = "md_insert_after_heading")]
    MdInsertAfterHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(
        rename = "md.insert_before_heading",
        alias = "md_insert_before_heading"
    )]
    MdInsertBeforeHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.upsert_bullet", alias = "md_upsert_bullet")]
    MdUpsertBullet {
        path: String,
        heading: String,
        bullet: String,
    },
    #[serde(rename = "md.table_append", alias = "md_table_append")]
    MdTableAppend {
        path: String,
        heading: String,
        row: String,
    },
    #[serde(rename = "md.move_section", alias = "md_move_section")]
    MdMoveSection {
        path: String,
        heading: String,
        /// Destination file. When omitted, reorder within the same file.
        to: Option<String>,
        /// Insert before this heading at the destination.
        before: Option<String>,
        /// Insert after this heading at the destination.
        after: Option<String>,
    },
    #[serde(rename = "md.dedupe_headings", alias = "md_dedupe_headings")]
    MdDedupeHeadings { path: String },
    #[serde(rename = "tidy.fix", alias = "fix_whitespace")]
    TidyFix {
        path: String,
        ensure_final_newline: Option<bool>,
        trim_trailing_whitespace: Option<bool>,
        normalize_eol: Option<String>,
        /// Dedent specification: "4", "tab", or "auto".
        dedent: Option<String>,
        /// Indent specification: "4", "tab".
        indent: Option<String>,
        /// Line range restriction for dedent/indent: "10:50" (1-based inclusive).
        lines: Option<String>,
    },
    #[serde(rename = "file.create", alias = "create_file")]
    FileCreate {
        path: String,
        content: String,
        force: Option<bool>,
    },
    #[serde(rename = "file.append", alias = "append_file")]
    FileAppend { path: String, content: String },
    #[serde(rename = "file.delete", alias = "delete_file")]
    FileDelete { path: String },
    #[serde(rename = "file.rename", alias = "move_file")]
    FileRename {
        from: String,
        to: String,
        /// If true, overwrite the destination if it already exists.
        #[serde(default)]
        force: bool,
    },
    #[serde(rename = "patch.apply", alias = "apply_patch")]
    PatchApply {
        diff: String,
        #[serde(default)]
        on_stale: crate::ops::patch::OnStale,
        #[serde(default)]
        allow_conflicts: bool,
    },
    #[serde(rename = "search", alias = "search_files")]
    Search {
        path: String,
        pattern: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        case_insensitive: bool,
        /// Enable multiline matching (dot matches newlines in regex mode).
        #[serde(default)]
        multiline: bool,
        /// Show lines that do NOT match the pattern.
        #[serde(default)]
        invert_match: bool,
        context: Option<usize>,
        before_context: Option<usize>,
        after_context: Option<usize>,
        /// Assert that the total match count equals N. Fails the operation otherwise.
        assert_count: Option<usize>,
        #[serde(default)]
        literal: bool,
        #[serde(default)]
        globs: Vec<String>,
        #[serde(default)]
        max_results: usize,
        #[serde(default)]
        exclude_patterns: Vec<String>,
        #[serde(default)]
        custom_ignore_filenames: Vec<String>,
    },
    #[serde(rename = "read", alias = "read_file")]
    Read {
        path: String,
        /// Optional line range (e.g., "10:25").
        lines: Option<String>,
    },
    #[serde(rename = "md.lint_agents", alias = "md_lint")]
    MdLintAgents { path: String },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.rename", alias = "ast_rename")]
    AstRename {
        path: String,
        old_name: String,
        new_name: String,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.replace", alias = "ast_replace")]
    AstReplace {
        path: String,
        symbol: String,
        from: String,
        to: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        lang: Option<String>,
    },
}

impl Operation {
    /// Human-readable label for this operation (matches the serde tag name).
    pub fn label(&self) -> &'static str {
        match self {
            Operation::Replace { .. } => "replace",
            Operation::DocSet { .. } => "doc.set",
            Operation::DocDelete { .. } => "doc.delete",
            Operation::DocMerge { .. } => "doc.merge",
            Operation::DocAppend { .. } => "doc.append",
            Operation::DocPrepend { .. } => "doc.prepend",
            Operation::DocUpdate { .. } => "doc.update",
            Operation::DocMove { .. } => "doc.move",
            Operation::DocEnsure { .. } => "doc.ensure",
            Operation::DocDeleteWhere { .. } => "doc.delete_where",
            Operation::MdReplaceSection { .. } => "md.replace_section",
            Operation::MdInsertAfterHeading { .. } => "md.insert_after_heading",
            Operation::MdInsertBeforeHeading { .. } => "md.insert_before_heading",
            Operation::MdUpsertBullet { .. } => "md.upsert_bullet",
            Operation::MdTableAppend { .. } => "md.table_append",
            Operation::MdMoveSection { .. } => "md.move_section",
            Operation::MdDedupeHeadings { .. } => "md.dedupe_headings",
            Operation::TidyFix { .. } => "tidy.fix",
            Operation::FileAppend { .. } => "file.append",
            Operation::FileCreate { .. } => "file.create",
            Operation::FileDelete { .. } => "file.delete",
            Operation::FileRename { .. } => "file.rename",
            Operation::PatchApply { .. } => "patch.apply",
            Operation::Read { .. } => "read",
            Operation::Search { .. } => "search",
            Operation::MdLintAgents { .. } => "md.lint_agents",
            #[cfg(feature = "ast")]
            Operation::AstRename { .. } => "ast.rename",
            #[cfg(feature = "ast")]
            Operation::AstReplace { .. } => "ast.replace",
        }
    }

    /// Whether this operation requires flushing the doc cache before execution.
    ///
    /// Non-doc operations (replace, md, file, patch, read, search, tidy, ast)
    /// need the doc cache flushed because they read/write files directly and
    /// would see stale content if doc mutations are still buffered.
    pub fn needs_doc_flush(&self) -> bool {
        matches!(
            self,
            Operation::Replace { .. }
                | Operation::MdReplaceSection { .. }
                | Operation::MdInsertAfterHeading { .. }
                | Operation::MdInsertBeforeHeading { .. }
                | Operation::MdUpsertBullet { .. }
                | Operation::MdTableAppend { .. }
                | Operation::MdMoveSection { .. }
                | Operation::MdDedupeHeadings { .. }
                | Operation::PatchApply { .. }
                | Operation::FileAppend { .. }
                | Operation::Read { .. }
                | Operation::Search { .. }
                | Operation::MdLintAgents { .. }
                | Operation::TidyFix { .. }
        ) || {
            #[cfg(feature = "ast")]
            {
                matches!(
                    self,
                    Operation::AstRename { .. } | Operation::AstReplace { .. }
                )
            }
            #[cfg(not(feature = "ast"))]
            {
                false
            }
        }
    }

    /// Returns the file paths declared by this operation for containment
    /// validation. See [`declared_paths`] for details.
    pub fn declared_paths(&self) -> Vec<&str> {
        declared_paths(self)
    }
}

/// Convert a doc-family `Operation` into a `(path, DocMutation)` pair.
///
/// Returns `None` for non-doc operations. This is the single source of truth
/// for mapping `Operation::Doc*` variants to `DocMutation`, used by both the
/// tx engine (`tx/execute.rs`) and any future callers.
pub(crate) fn op_to_doc_mutation(op: &Operation) -> Option<(&str, crate::ops::doc::DocMutation)> {
    use crate::ops::doc::DocMutation;
    match op {
        Operation::DocSet {
            path,
            selector,
            value,
        } => Some((
            path,
            DocMutation::Set {
                selector: selector.clone(),
                value: value.clone(),
            },
        )),
        Operation::DocDelete { path, selector } => Some((
            path,
            DocMutation::Delete {
                selector: selector.clone(),
            },
        )),
        Operation::DocMerge { path, value } => Some((
            path,
            DocMutation::Merge {
                value: value.clone(),
            },
        )),
        Operation::DocAppend {
            path,
            selector,
            value,
        } => Some((
            path,
            DocMutation::Append {
                selector: selector.clone(),
                value: value.clone(),
            },
        )),
        Operation::DocPrepend {
            path,
            selector,
            value,
        } => Some((
            path,
            DocMutation::Prepend {
                selector: selector.clone(),
                value: value.clone(),
            },
        )),
        Operation::DocUpdate {
            path,
            selector,
            value,
        } => Some((
            path,
            DocMutation::Update {
                selector: selector.clone(),
                value: value.clone(),
            },
        )),
        Operation::DocMove { path, from, to } => Some((
            path,
            DocMutation::Move {
                from: from.clone(),
                to: to.clone(),
            },
        )),
        Operation::DocEnsure {
            path,
            selector,
            value,
        } => Some((
            path,
            DocMutation::Ensure {
                selector: selector.clone(),
                value: value.clone(),
            },
        )),
        Operation::DocDeleteWhere {
            path,
            selector,
            predicate,
        } => Some((
            path,
            DocMutation::DeleteWhere {
                selector: selector.clone(),
                predicate: predicate.clone(),
            },
        )),
        _ => None,
    }
}

/// Returns the file paths (as `&str`) that are declared by the operation
/// and should be subject to PathGuard / containment validation.
///
/// This eliminates duplication between:
/// - upfront checks in `execute_plan` (library use, #755)
/// - test validation logic in MCP
///
/// - `Replace`: includes `path` (if present) and `glob` pattern (if present).
/// - Cross-file ops (`FileRename`, `MdMoveSection`): includes both source
///   and destination file paths.
/// - `PatchApply`: returns empty; embedded paths are validated by the
///   caller (MCP always parses the diff and checks before execute).
/// - All other ops: their primary `path` (or equivalent).
/// - AST variants are included only when the `ast` feature is enabled.
pub(crate) fn declared_paths(op: &Operation) -> Vec<&str> {
    match op {
        Operation::Replace { path, glob, .. } => {
            let mut p = Vec::new();
            if let Some(s) = path {
                p.push(s.as_str());
            }
            if let Some(s) = glob {
                p.push(s.as_str());
            }
            p
        }
        Operation::FileRename { from, to, .. } => vec![from.as_str(), to.as_str()],
        Operation::MdMoveSection { path, to, .. } => {
            let mut p = vec![path.as_str()];
            if let Some(t) = to {
                p.push(t.as_str());
            }
            p
        }
        Operation::PatchApply { .. } => vec![],
        // Single-path operations (file, doc, md, read, search, tidy, lint, etc.)
        Operation::DocSet { path, .. }
        | Operation::DocDelete { path, .. }
        | Operation::DocMerge { path, .. }
        | Operation::DocAppend { path, .. }
        | Operation::DocPrepend { path, .. }
        | Operation::DocUpdate { path, .. }
        | Operation::DocMove { path, .. }
        | Operation::DocEnsure { path, .. }
        | Operation::DocDeleteWhere { path, .. }
        | Operation::MdReplaceSection { path, .. }
        | Operation::MdInsertAfterHeading { path, .. }
        | Operation::MdInsertBeforeHeading { path, .. }
        | Operation::MdUpsertBullet { path, .. }
        | Operation::MdTableAppend { path, .. }
        | Operation::MdDedupeHeadings { path, .. }
        | Operation::TidyFix { path, .. }
        | Operation::FileAppend { path, .. }
        | Operation::FileCreate { path, .. }
        | Operation::FileDelete { path, .. }
        | Operation::Read { path, .. }
        | Operation::Search { path, .. }
        | Operation::MdLintAgents { path, .. } => vec![path.as_str()],
        #[cfg(feature = "ast")]
        Operation::AstRename { path, .. } | Operation::AstReplace { path, .. } => {
            vec![path.as_str()]
        }
    }
}

/// A validation step to run after applying operations.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[non_exhaustive]
pub struct ValidationStep {
    pub cmd: String,
    pub required: Option<bool>,
    /// Timeout in seconds (default: 60).
    pub timeout: Option<u64>,
}

/// Parse a plan from a JSON string.
pub fn parse_plan(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = serde_json::from_str(input)?;
    Ok(plan)
}

/// Parse a plan from a YAML string.
pub fn parse_plan_yaml(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = serde_yaml_ng::from_str(input)?;
    Ok(plan)
}

/// Parse a plan from a TOML string.
pub fn parse_plan_toml(input: &str) -> anyhow::Result<Plan> {
    let plan: Plan = toml_edit::de::from_str(input)?;
    Ok(plan)
}

/// Detect plan format from a file path extension and parse accordingly.
pub fn parse_plan_auto(
    input: &str,
    path: Option<&str>,
    format_hint: Option<&str>,
) -> anyhow::Result<Plan> {
    let fmt = format_hint.or_else(|| {
        path.and_then(|p| {
            crate::ops::doc::detect_format(p).ok().map(|f| match f {
                crate::ops::doc::FileFormat::Yaml => "yaml",
                crate::ops::doc::FileFormat::Toml => "toml",
                crate::ops::doc::FileFormat::Json => "json",
            })
        })
    });
    match fmt {
        Some("yaml" | "yml") => parse_plan_yaml(input),
        Some("toml") => parse_plan_toml(input),
        _ => parse_plan(input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_plan() {
        let json = r#"{"version": "1", "operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert!(plan.cwd.is_none());
        assert!(plan.write_policy.is_none());
        assert!(plan.validate.is_none());
        assert_eq!(plan.version, "1");
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_version_field_accepted() {
        let json = r#"{"version": "1", "operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.version, "1");
    }

    #[test]
    fn parse_plan_without_version_fails() {
        let json = r#"{"operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn parse_plan_with_all_fields() {
        let json = r#"{
            "version": "1",
            "cwd": "/tmp",
            "write_policy": {"ensure_final_newline": true, "normalize_eol": "lf"},
            "operations": [{"op": "file.create", "path": "f.txt", "content": "hi"}],
            "validate": [{"cmd": "echo ok"}]
        }"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.cwd.as_deref(), Some("/tmp"));
        let wp = plan.write_policy.unwrap();
        assert_eq!(wp.ensure_final_newline, Some(true));
        assert_eq!(wp.normalize_eol.as_deref(), Some("lf"));
        assert!(plan.validate.unwrap()[0].required.is_none());
    }

    #[test]
    fn parse_plan_unknown_op_fails() {
        let json = r#"{"version": "1", "operations": [{"op": "unknown", "x": 1}]}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn parse_plan_missing_operations_fails() {
        let json = r#"{"version": "1", "cwd": "/tmp"}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    #[cfg(feature = "ast")]
    fn parse_all_operation_variants() {
        let json = r#"{"version": "1", "operations": [
            {"op": "replace", "from": "a", "to": "b"},
            {"op": "replace", "from": "a", "to": "b", "nth": 2},
            {"op": "doc.set", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc.delete", "path": "f.json", "selector": "k"},
            {"op": "doc.merge", "path": "f.json", "value": {}},
            {"op": "doc.append", "path": "f.json", "selector": "arr", "value": 1},
            {"op": "doc.prepend", "path": "f.json", "selector": "arr", "value": 0},
            {"op": "doc.update", "path": "f.json", "selector": "k", "value": 2},
            {"op": "doc.move", "path": "f.json", "from": "a", "to": "b"},
            {"op": "doc.ensure", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc.delete_where", "path": "f.json", "selector": "arr", "predicate": "name=x"},
            {"op": "md.replace_section", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_after_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.insert_before_heading", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md.upsert_bullet", "path": "f.md", "heading": "H", "bullet": "- item"},
            {"op": "md.table_append", "path": "f.md", "heading": "H", "row": "| a | b |"},
            {"op": "md.move_section", "path": "src.md", "heading": "FAQ", "before": "License"},
            {"op": "md.move_section", "path": "src.md", "heading": "Appendix", "to": "dest.md", "after": "Body"},
            {"op": "md.dedupe_headings", "path": "f.md"},
            {"op": "tidy.fix", "path": "f.txt"},
            {"op": "tidy.fix", "path": "f.txt", "trim_trailing_whitespace": true, "normalize_eol": "lf"},
            {"op": "file.append", "path": "f.txt", "content": "extra"},
            {"op": "file.create", "path": "f.txt", "content": "c"},
            {"op": "file.create", "path": "g.txt", "content": "c", "force": true},
            {"op": "file.delete", "path": "f.txt"},
            {"op": "file.rename", "from": "old.txt", "to": "new.txt"},
            {"op": "file.rename", "from": "a.txt", "to": "b.txt", "force": true},
            {"op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b"},
            {"op": "read", "path": "f.txt"},
            {"op": "read", "path": "f.txt", "lines": "1:10"},
            {"op": "search", "path": "f.txt", "pattern": "hello"},
            {"op": "search", "path": "f.txt", "pattern": "he.*o", "regex": true, "case_insensitive": true, "multiline": true},
            {"op": "search", "path": "f.txt", "pattern": "TODO", "invert_match": true, "assert_count": 5},
            {"op": "search", "path": ".", "pattern": "foo", "literal": true, "exclude_patterns": ["target/**"], "custom_ignore_filenames": [".blineignore"], "max_results": 10},
            {"op": "ast.rename", "path": "f.rs", "old_name": "Foo", "new_name": "Bar"},
            {"op": "ast.replace", "path": "f.rs", "symbol": "main", "from": "a", "to": "b"}
        ]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.operations.len(), 36);
    }

    #[test]
    #[cfg(feature = "ast")]
    fn parse_op_aliases_match_mcp_tool_names() {
        // MCP tools use underscores (doc_set, create_file). Plan ops use dots
        // (doc.set, file.create). Both forms should parse via serde aliases.
        let json = r#"{"version": "1", "operations": [
            {"op": "replace_text", "from": "a", "to": "b"},
            {"op": "doc_set", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc_delete", "path": "f.json", "selector": "k"},
            {"op": "doc_merge", "path": "f.json", "value": {}},
            {"op": "doc_append", "path": "f.json", "selector": "arr", "value": 1},
            {"op": "doc_ensure", "path": "f.json", "selector": "k", "value": 1},
            {"op": "doc_delete_where", "path": "f.json", "selector": "arr", "predicate": "x=1"},
            {"op": "md_move_section", "path": "f.md", "heading": "H", "before": "X"},
            {"op": "md_replace_section", "path": "f.md", "heading": "H", "content": "c"},
            {"op": "md_upsert_bullet", "path": "f.md", "heading": "H", "bullet": "- item"},
            {"op": "md_table_append", "path": "f.md", "heading": "H", "row": "| a |"},
            {"op": "fix_whitespace", "path": "f.txt"},
            {"op": "append_file", "path": "f.txt", "content": "extra"},
            {"op": "create_file", "path": "f.txt", "content": "c"},
            {"op": "delete_file", "path": "f.txt"},
            {"op": "move_file", "from": "a.txt", "to": "b.txt"},
            {"op": "apply_patch", "diff": "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b"},
            {"op": "search_files", "path": ".", "pattern": "x"},
            {"op": "read_file", "path": "f.txt"},
            {"op": "md_lint", "path": "f.md"},
            {"op": "ast_rename", "path": "f.rs", "old_name": "A", "new_name": "B"},
            {"op": "ast_replace", "path": "f.rs", "symbol": "main", "from": "x", "to": "y"}
        ]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.operations.len(), 22);
    }

    #[test]
    fn parse_plan_with_format_steps() {
        let json = r#"{
            "version": "1",
            "operations": [],
            "format": [{"cmd": "cargo fmt"}],
            "validate": [{"cmd": "make check"}]
        }"#;
        let plan = parse_plan(json).unwrap();
        let fmt = plan.format.unwrap();
        assert_eq!(fmt.len(), 1);
        assert_eq!(fmt[0].cmd, "cargo fmt");
    }

    // ── YAML / TOML / auto-detect ─────────────────────────────────

    #[test]
    fn parse_plan_yaml_basic() {
        let yaml = "version: \"1\"\noperations:\n  - op: replace\n    from: old\n    to: new\n";
        let plan = parse_plan_yaml(yaml).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            &plan.operations[0],
            Operation::Replace { from, to, .. } if from == "old" && to.as_deref() == Some("new")
        ));
    }

    #[test]
    fn parse_plan_toml_basic() {
        let toml =
            "version = \"1\"\n\n[[operations]]\nop = \"replace\"\nfrom = \"old\"\nto = \"new\"\n";
        let plan = parse_plan_toml(toml).unwrap();
        assert_eq!(plan.operations.len(), 1);
        assert!(matches!(
            &plan.operations[0],
            Operation::Replace { from, to, .. } if from == "old" && to.as_deref() == Some("new")
        ));
    }

    #[test]
    fn parse_plan_auto_detects_yaml() {
        let yaml = "version: \"1\"\noperations:\n  - op: replace\n    from: a\n    to: b\n";
        let plan = parse_plan_auto(yaml, Some("plan.yaml"), None).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_auto_format_hint_overrides_extension() {
        let yaml = "version: \"1\"\noperations:\n  - op: replace\n    from: a\n    to: b\n";
        // Extension says .json but hint says yaml.
        let plan = parse_plan_auto(yaml, Some("plan.json"), Some("yaml")).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_auto_defaults_to_json() {
        let json = r#"{"version": "1", "operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan_auto(json, Some("plan.txt"), None).unwrap();
        assert_eq!(plan.operations.len(), 1);
    }

    #[test]
    fn parse_plan_defaults_strict_when_omitted() {
        let json = r#"{"version": "1", "operations": [{"op": "replace", "from": "a", "to": "b"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.strict, None);
        assert!(effective_strict(plan.strict, None, false));
        assert!(!effective_strict(plan.strict, None, true));
        assert!(!effective_strict(Some(true), None, true));
        assert!(!effective_strict(None, Some(false), false));
        assert!(effective_strict(Some(true), Some(false), false));
    }

    #[test]
    fn parse_plan_strict_and_all_policy_fields() {
        let json = r#"{
            "version": "1",
            "strict": true,
            "write_policy": {
                "ensure_final_newline": true,
                "normalize_eol": "crlf",
                "trim_trailing_whitespace": true,
                "collapse_blanks": true
            },
            "operations": [],
            "format": [{"cmd": "fmt", "timeout": 30}],
            "validate": [{"cmd": "check", "required": true, "timeout": 120}]
        }"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.strict, Some(true));
        let wp = plan.write_policy.unwrap();
        assert_eq!(wp.ensure_final_newline, Some(true));
        assert_eq!(wp.normalize_eol.as_deref(), Some("crlf"));
        assert_eq!(wp.trim_trailing_whitespace, Some(true));
        assert_eq!(wp.collapse_blanks, Some(true));
        let fmt = &plan.format.unwrap()[0];
        assert_eq!(fmt.timeout, Some(30));
        let val = &plan.validate.unwrap()[0];
        assert_eq!(val.required, Some(true));
        assert_eq!(val.timeout, Some(120));
    }

    #[test]
    fn declared_paths_covers_operation_variants() {
        // Replace with path + glob (both collected for guard)
        let json = r#"{"version":"1","operations":[{"op":"replace","path":"src/main.rs","glob":"**/*.rs","from":"old","to":"new"}]}"#;
        let plan = parse_plan(json).unwrap();
        let ps = declared_paths(&plan.operations[0]);
        assert!(ps.contains(&"src/main.rs") && ps.contains(&"**/*.rs"));

        // FileRename (cross-file paths)
        let json = r#"{"version":"1","operations":[{"op":"file.rename","from":"old.txt","to":"new.txt","force":false}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(
            declared_paths(&plan.operations[0]),
            vec!["old.txt", "new.txt"]
        );

        // MdMoveSection same-file (to omitted)
        let json = r#"{"version":"1","operations":[{"op":"md.move_section","path":"doc.md","heading":"Section"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(declared_paths(&plan.operations[0]), vec!["doc.md"]);

        // MdMoveSection cross-file
        let json = r#"{"version":"1","operations":[{"op":"md.move_section","path":"src.md","heading":"H","to":"dst.md"}]}"#;
        let plan = parse_plan(json).unwrap();
        let ps = declared_paths(&plan.operations[0]);
        assert!(ps.contains(&"src.md") && ps.contains(&"dst.md"));

        // PatchApply: declared empty (diff paths handled by caller in MCP)
        let json = r#"{"version":"1","operations":[{"op":"patch.apply","diff":"--- a/x\n+++ b/x\n@@ -1 +1 @@\n- old\n+ new\n"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert!(declared_paths(&plan.operations[0]).is_empty());

        // Representative single-path ops
        let json = r#"{"version":"1","operations":[{"op":"doc.set","path":"c.json","selector":"v","value":42}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(declared_paths(&plan.operations[0]), vec!["c.json"]);

        let json = r#"{"version":"1","operations":[{"op":"read","path":"f.txt"}]}"#;
        let plan = parse_plan(json).unwrap();
        assert_eq!(declared_paths(&plan.operations[0]), vec!["f.txt"]);
    }

    #[test]
    fn op_to_doc_mutation_covers_all_doc_variants() {
        use crate::ops::doc::DocMutation;

        let cases = [
            r#"{"op":"doc.set","path":"f.json","selector":"k","value":1}"#,
            r#"{"op":"doc.delete","path":"f.json","selector":"k"}"#,
            r#"{"op":"doc.merge","path":"f.json","value":{}}"#,
            r#"{"op":"doc.append","path":"f.json","selector":"arr","value":1}"#,
            r#"{"op":"doc.prepend","path":"f.json","selector":"arr","value":0}"#,
            r#"{"op":"doc.update","path":"f.json","selector":"k","value":2}"#,
            r#"{"op":"doc.move","path":"f.json","from":"a","to":"b"}"#,
            r#"{"op":"doc.ensure","path":"f.json","selector":"k","value":1}"#,
            r#"{"op":"doc.delete_where","path":"f.json","selector":"arr","predicate":"n=x"}"#,
        ];

        for (i, case) in cases.iter().enumerate() {
            let json = format!(r#"{{"version":"1","operations":[{case}]}}"#);
            let plan = parse_plan(&json).unwrap();
            let result = op_to_doc_mutation(&plan.operations[0]);
            assert!(
                result.is_some(),
                "doc variant {i} should return Some, got None"
            );
            let (path, _mutation) = result.unwrap();
            assert_eq!(path, "f.json", "variant {i} path mismatch");
        }

        // Non-doc variants return None
        let non_doc = r#"{"version":"1","operations":[{"op":"replace","from":"a","to":"b"}]}"#;
        let plan = parse_plan(non_doc).unwrap();
        assert!(op_to_doc_mutation(&plan.operations[0]).is_none());

        // Verify the specific mutation variant matches
        let set_json = r#"{"version":"1","operations":[{"op":"doc.set","path":"x.json","selector":"key","value":"val"}]}"#;
        let plan = parse_plan(set_json).unwrap();
        let (_, mutation) = op_to_doc_mutation(&plan.operations[0]).unwrap();
        assert!(matches!(mutation, DocMutation::Set { .. }));
    }
}
