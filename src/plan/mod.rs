//! Transaction plan format parsing.

use serde::{Deserialize, Serialize};

/// Current plan schema version.
pub const SCHEMA_VERSION: u32 = 1;

fn default_strict_true() -> bool {
    true
}

fn default_version() -> u32 {
    1
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
pub struct Plan {
    /// Schema version. Defaults to 1 when omitted.
    #[serde(default = "default_version")]
    pub version: u32,
    pub cwd: Option<String>,
    pub write_policy: Option<crate::write::WritePolicyOverride>,
    /// When omitted from the plan, defaults to strict mode at execution time.
    #[serde(default)]
    pub strict: Option<bool>,
    pub operations: Vec<Operation>,
    pub format: Option<Vec<FormatStep>>,
    pub validate: Option<Vec<ValidationStep>>,
    /// Pre/post-operation symbol verification checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify: Option<Vec<VerifyCheck>>,
    /// Glob-driven batch: expand operations once per matching file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub for_each: Option<ForEach>,
}

impl Plan {
    /// Returns `true` if the plan has any format or validate steps that could
    /// modify files outside the transaction scope.
    pub fn has_lifecycle_steps(&self) -> bool {
        self.format.as_ref().is_some_and(|v| !v.is_empty())
            || self.validate.as_ref().is_some_and(|v| !v.is_empty())
    }
}

/// Glob-driven batch expansion: apply the same set of operations to every
/// file matching a glob pattern, with template variable substitution.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ForEach {
    /// Glob pattern to expand (e.g. `src/**/*.rs`).
    pub glob: String,
    /// Glob patterns to exclude from the matched set.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Optional filter expression (e.g. `has_symbol(tests)`).
    #[serde(default)]
    pub filter: Option<String>,
}

/// A single verification check parsed from `--verify` or plan `verify` field.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum VerifyCheck {
    /// `{"kind": "function", "attr": "test"}` or `{"kind": "function"}`
    SymbolCount {
        kind: String,
        #[serde(default)]
        attr: Option<String>,
    },
    /// `{"check": "unique_names"}` or `{"check": "no_orphans"}`
    Named { check: String },
}

impl VerifyCheck {
    /// Parse a CLI `--verify` value like `kind=function,attr=test` or `unique_names`.
    #[cfg(feature = "cli")]
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        if s == "unique_names" || s == "no_orphans" {
            return Ok(VerifyCheck::Named {
                check: s.to_string(),
            });
        }
        let mut kind = None;
        let mut attr = None;
        for part in s.split(',') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                match k.trim() {
                    "kind" => kind = Some(v.trim().to_string()),
                    "attr" => attr = Some(v.trim().to_string()),
                    other => anyhow::bail!("unknown verify key: {other}"),
                }
            } else {
                // Bare word like "function" treated as kind
                kind = Some(part.to_string());
            }
        }
        if let Some(kind) = kind {
            Ok(VerifyCheck::SymbolCount { kind, attr })
        } else {
            anyhow::bail!(
                "verify spec must contain 'kind=<type>' or a named check (unique_names, no_orphans)"
            )
        }
    }
}

/// A format step to run after applying operations but before validation.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct FormatStep {
    #[serde(alias = "command")]
    pub cmd: String,
    /// Timeout in seconds (default: 60).
    pub timeout: Option<u64>,
}

/// A single operation within a plan.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "op")]
pub enum Operation {
    #[serde(rename = "replace")]
    Replace {
        /// Glob pattern to match files (e.g. "src/**/*.rs"). Mutually exclusive with path.
        glob: Option<String>,
        /// File path to replace in. Mutually exclusive with glob.
        path: Option<String>,
        /// Treat `old` as a regex pattern.
        #[serde(default)]
        regex: bool,
        /// Pattern to find (literal string or regex).
        old: String,
        /// Replacement text. Supports $1/$2 capture group references in regex mode.
        #[serde(rename = "new")]
        new_text: Option<String>,
        /// Replace only the Nth occurrence (1-based).
        nth: Option<usize>,
        /// Insert text before each match instead of replacing.
        insert_before: Option<String>,
        /// Insert text after each match instead of replacing.
        insert_after: Option<String>,
        /// Case-insensitive matching.
        #[serde(default)]
        case_insensitive: bool,
        /// Enable multiline matching (dot matches newlines in regex mode).
        #[serde(default)]
        multiline: bool,
        /// Return success even if no matches found (idempotent mode).
        #[serde(default)]
        if_exists: bool,
        /// Replace the entire line containing each match, not just the matched span.
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
        /// Fail if the pattern matches more than once (enforce unambiguous edits).
        #[serde(default)]
        unique: bool,
    },
    #[serde(rename = "doc.set")]
    DocSet {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path (e.g. "server.port", "env.0.value").
        selector: String,
        /// Value to set (any JSON type).
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete")]
    DocDelete {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to delete.
        selector: String,
    },
    #[serde(rename = "doc.merge")]
    DocMerge {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Object to deep-merge into the file.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.append")]
    DocAppend {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to an array.
        selector: String,
        /// Value to append to the array.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.prepend")]
    DocPrepend {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to an array.
        selector: String,
        /// Value to prepend to the array.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.update")]
    DocUpdate {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path (supports wildcards and predicates).
        selector: String,
        /// New value for all matching locations.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.move")]
    DocMove {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation source path.
        from: String,
        /// Dot-notation destination path.
        to: String,
    },
    #[serde(rename = "doc.ensure")]
    DocEnsure {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path.
        selector: String,
        /// Value to set only if the key does not already exist.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete_where")]
    DocDeleteWhere {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to an array.
        selector: String,
        /// Predicate in "field=value" format to match elements for deletion.
        predicate: String,
    },
    #[serde(rename = "md.replace_section")]
    MdReplaceSection {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.insert_after_heading")]
    MdInsertAfterHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.insert_before_heading")]
    MdInsertBeforeHeading {
        path: String,
        heading: String,
        content: String,
    },
    #[serde(rename = "md.upsert_bullet")]
    MdUpsertBullet {
        path: String,
        heading: String,
        bullet: String,
    },
    #[serde(rename = "md.table_append")]
    MdTableAppend {
        path: String,
        heading: String,
        row: String,
    },
    #[serde(rename = "md.move_section")]
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
    #[serde(rename = "md.dedupe_headings")]
    MdDedupeHeadings { path: String },
    #[serde(rename = "tidy.fix")]
    TidyFix {
        path: String,
        ensure_final_newline: Option<bool>,
        trim_trailing_whitespace: Option<bool>,
        normalize_eol: Option<String>,
        /// Collapse consecutive blank lines into a single blank line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        collapse_blanks: Option<bool>,
        /// Dedent specification: "4", "tab", or "auto".
        dedent: Option<String>,
        /// Indent specification: "4", "tab".
        indent: Option<String>,
        /// Line range restriction for dedent/indent: "10:50" (1-based inclusive).
        lines: Option<String>,
    },
    #[serde(rename = "file.create")]
    FileCreate {
        path: String,
        content: String,
        force: Option<bool>,
    },
    #[serde(rename = "file.append")]
    FileAppend { path: String, content: String },
    #[serde(rename = "file.prepend")]
    FilePrepend { path: String, content: String },
    #[serde(rename = "file.delete")]
    FileDelete { path: String },
    #[serde(rename = "file.rename")]
    FileRename {
        from: String,
        to: String,
        /// If true, overwrite the destination if it already exists.
        #[serde(default)]
        force: bool,
    },
    #[serde(rename = "patch.apply")]
    PatchApply {
        diff: String,
        #[serde(default)]
        on_stale: crate::ops::patch::OnStale,
        #[serde(default)]
        allow_conflicts: bool,
    },
    #[serde(rename = "search")]
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
    #[serde(rename = "read")]
    Read {
        path: String,
        /// Optional line range (e.g., "10:25").
        lines: Option<String>,
    },
    #[serde(rename = "md.lint_agents")]
    MdLintAgents { path: String },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.rename")]
    AstRename {
        /// File or directory to rename in. Directories are walked recursively.
        path: String,
        /// The identifier to rename.
        old_name: String,
        /// The new identifier name.
        new_name: String,
        /// Language hint (e.g. "rust", "go"). Overrides extension-based detection.
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.replace")]
    AstReplace {
        /// File to replace in.
        path: String,
        /// Symbol name to scope the replacement to.
        symbol: String,
        /// Text or pattern to find within the symbol body.
        old: String,
        /// Replacement text.
        #[serde(rename = "new")]
        new_text: String,
        /// Treat `old` as a regex pattern.
        #[serde(default)]
        regex: bool,
        /// Language hint (e.g. "rust", "go"). Overrides extension-based detection.
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.insert")]
    AstInsert {
        /// File to insert code into.
        path: String,
        /// Code to insert.
        content: String,
        /// Module/impl/struct to insert into.
        #[serde(default)]
        inside: Option<String>,
        /// Insert after this symbol.
        #[serde(default)]
        after: Option<String>,
        /// Insert before this symbol.
        #[serde(default)]
        before: Option<String>,
        /// Position within `inside`: "start" or "end" (default).
        #[serde(default)]
        position: Option<String>,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.wrap")]
    AstWrap {
        path: String,
        /// Symbols to wrap (mutually exclusive with `lines`).
        #[serde(default)]
        symbols: Option<Vec<String>>,
        /// Line range to wrap (mutually exclusive with `symbols`).
        #[serde(default)]
        lines: Option<String>,
        /// The wrapping construct (e.g. "mod foo", "impl Bar").
        wrapper: String,
        /// Content to insert at the top of the wrapped block.
        #[serde(default)]
        preamble: Option<String>,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.imports")]
    AstImports {
        path: String,
        /// Import statements to add.
        #[serde(default)]
        add: Option<Vec<String>>,
        /// Import statements to remove.
        #[serde(default)]
        remove: Option<Vec<String>>,
        /// Deduplicate imports.
        #[serde(default)]
        dedupe: bool,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.reorder")]
    AstReorder {
        path: String,
        /// Scope to reorder within (module/impl). Default: top-level.
        #[serde(default)]
        inside: Option<String>,
        /// Ordering: "alphabetical", "reverse", "kind-first", or array of names.
        order: serde_json::Value,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.group")]
    AstGroup {
        path: String,
        /// Module name to create or append to.
        module: String,
        /// Symbols to move into the module.
        symbols: Vec<String>,
        /// Code to insert at the top of the module (e.g., `use super::*;`).
        #[serde(default)]
        preamble: Option<String>,
        /// Where to place new module: "first-symbol" (default), "end", or "after:<symbol>".
        #[serde(default)]
        position: Option<String>,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.move")]
    AstMove {
        /// Source file.
        path: String,
        /// Target file.
        target: String,
        /// Symbols to move.
        symbols: Vec<String>,
        /// Position in target: "end" (default), "start", "after:<symbol>", "before:<symbol>".
        #[serde(default)]
        position: Option<String>,
        /// Content to prepend to target file if it's being created.
        #[serde(default)]
        target_prepend: Option<String>,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.extract_to_file")]
    AstExtractToFile {
        /// Source file containing the symbol.
        source: String,
        /// Name of the symbol to extract.
        symbol: String,
        /// Destination file path.
        target: String,
        /// Text to leave in place of the extracted block.
        #[serde(default)]
        replacement: Option<String>,
        /// If true (default for modules), remove the wrapper and un-indent.
        #[serde(default)]
        unwrap: Option<bool>,
        /// Content to prepend to the target file.
        #[serde(default)]
        prepend: Option<String>,
        /// Overwrite target if it exists.
        #[serde(default)]
        force: bool,
        #[serde(default)]
        lang: Option<String>,
    },
    #[cfg(feature = "ast")]
    #[serde(rename = "ast.split")]
    AstSplit {
        /// The file to split.
        source: String,
        /// Target file specs.
        targets: Vec<SplitTargetSpec>,
        /// Symbols to keep in the source file.
        #[serde(default)]
        keep_in_source: Vec<String>,
        /// Text to append to source after split (e.g., `mod` declarations).
        #[serde(default)]
        source_suffix: Option<String>,
        /// Text to prepend to source after split.
        #[serde(default)]
        source_prefix: Option<String>,
        /// Error if any symbol is unaccounted for (default: true).
        #[serde(default)]
        require_exhaustive: Option<bool>,
        #[serde(default)]
        lang: Option<String>,
    },
}

/// Target file specification for `ast.split`.
#[cfg(feature = "ast")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct SplitTargetSpec {
    /// Target file path.
    pub path: String,
    /// Symbols to move into this file.
    pub symbols: Vec<String>,
    /// Content to prepend to this target file.
    #[serde(default)]
    pub prepend: Option<String>,
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
            Operation::FilePrepend { .. } => "file.prepend",
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
            #[cfg(feature = "ast")]
            Operation::AstInsert { .. } => "ast.insert",
            #[cfg(feature = "ast")]
            Operation::AstWrap { .. } => "ast.wrap",
            #[cfg(feature = "ast")]
            Operation::AstImports { .. } => "ast.imports",
            #[cfg(feature = "ast")]
            Operation::AstReorder { .. } => "ast.reorder",
            #[cfg(feature = "ast")]
            Operation::AstGroup { .. } => "ast.group",
            #[cfg(feature = "ast")]
            Operation::AstMove { .. } => "ast.move",
            #[cfg(feature = "ast")]
            Operation::AstExtractToFile { .. } => "ast.extract_to_file",
            #[cfg(feature = "ast")]
            Operation::AstSplit { .. } => "ast.split",
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
                | Operation::FilePrepend { .. }
                | Operation::FileCreate { .. }
                | Operation::FileDelete { .. }
                | Operation::FileRename { .. }
                | Operation::Read { .. }
                | Operation::Search { .. }
                | Operation::MdLintAgents { .. }
                | Operation::TidyFix { .. }
        ) || {
            #[cfg(feature = "ast")]
            {
                matches!(
                    self,
                    Operation::AstRename { .. }
                        | Operation::AstReplace { .. }
                        | Operation::AstInsert { .. }
                        | Operation::AstWrap { .. }
                        | Operation::AstImports { .. }
                        | Operation::AstReorder { .. }
                        | Operation::AstGroup { .. }
                        | Operation::AstMove { .. }
                        | Operation::AstExtractToFile { .. }
                        | Operation::AstSplit { .. }
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
    pub fn declared_paths(&self) -> Vec<String> {
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
/// - `PatchApply`: parses the embedded diff via `parse_patch()` and returns
///   the file paths from `---`/`+++` headers. This ensures the upfront
///   PathGuard check in `execute_plan` catches out-of-boundary patches
///   (#1363). Returns empty on parse failure (error deferred to apply time).
/// - All other ops: their primary `path` (or equivalent).
/// - AST variants are included only when the `ast` feature is enabled.
pub(crate) fn declared_paths(op: &Operation) -> Vec<String> {
    match op {
        Operation::Replace { path, glob, .. } => {
            let mut p = Vec::new();
            if let Some(s) = path {
                p.push(s.clone());
            }
            if let Some(s) = glob {
                p.push(s.clone());
            }
            p
        }
        Operation::FileRename { from, to, .. } => vec![from.clone(), to.clone()],
        Operation::MdMoveSection { path, to, .. } => {
            let mut p = vec![path.clone()];
            if let Some(t) = to {
                p.push(t.clone());
            }
            p
        }
        Operation::PatchApply { diff, .. } => {
            // Parse the diff to extract file paths from ---/+++ headers.
            // If parsing fails, return empty (the error will surface at apply time).
            match crate::ops::patch::parse_patch(diff) {
                Ok(files) => files.into_iter().map(|pf| pf.path).collect(),
                Err(_) => vec![],
            }
        }
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
        | Operation::FilePrepend { path, .. }
        | Operation::FileCreate { path, .. }
        | Operation::FileDelete { path, .. }
        | Operation::Read { path, .. }
        | Operation::Search { path, .. }
        | Operation::MdLintAgents { path, .. } => vec![path.clone()],
        #[cfg(feature = "ast")]
        Operation::AstRename { path, .. }
        | Operation::AstReplace { path, .. }
        | Operation::AstInsert { path, .. }
        | Operation::AstWrap { path, .. }
        | Operation::AstImports { path, .. }
        | Operation::AstReorder { path, .. }
        | Operation::AstGroup { path, .. } => {
            vec![path.clone()]
        }
        #[cfg(feature = "ast")]
        Operation::AstMove { path, target, .. } => vec![path.clone(), target.clone()],
        #[cfg(feature = "ast")]
        Operation::AstExtractToFile { source, target, .. } => {
            vec![source.clone(), target.clone()]
        }
        #[cfg(feature = "ast")]
        Operation::AstSplit {
            source, targets, ..
        } => {
            let mut p = vec![source.clone()];
            for t in targets {
                p.push(t.path.clone());
            }
            p
        }
    }
}

/// A validation step to run after applying operations.
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ValidationStep {
    #[serde(alias = "command")]
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

// ---------------------------------------------------------------------------
// for_each expansion
// ---------------------------------------------------------------------------

/// Escape a string for safe embedding inside a JSON string literal.
///
/// The template substitution operates on the serialized JSON, replacing
/// `{path}` etc. inside already-quoted `"..."` values. If the substituted
/// text contains JSON-special characters (backslash, quote, newline, etc.),
/// the resulting JSON would be malformed. This function applies the same
/// escaping that `serde_json::to_string` would use for string content.
#[cfg(feature = "cli")]
fn json_escape(s: &str) -> String {
    // Use serde_json to produce `"escaped"`, then strip the surrounding quotes.
    let quoted = serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""));
    quoted[1..quoted.len() - 1].to_string()
}

/// Single-pass template substitution. Scans `template` left-to-right,
/// replacing each known placeholder from the original text. This prevents
/// cross-contamination where the replacement value of one placeholder
/// contains another placeholder name as a literal substring.
#[cfg(feature = "cli")]
fn substitute_single_pass(template: &str, vars: &[(&str, String)]) -> String {
    let mut result = String::with_capacity(template.len());
    let mut i = 0;
    let bytes = template.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut matched = false;
            for (placeholder, value) in vars {
                if template[i..].starts_with(placeholder) {
                    result.push_str(value);
                    i += placeholder.len();
                    matched = true;
                    break;
                }
            }
            if !matched {
                result.push('{');
                i += 1;
            }
        } else {
            // Advance by one full UTF-8 character, not one byte.
            // `bytes[i] as char` would interpret each byte of a multi-byte
            // sequence as a Latin-1 code point, corrupting non-ASCII text.
            let ch = template[i..]
                .chars()
                .next()
                .expect("i < len guarantees non-empty slice");
            result.push(ch);
            i += ch.len_utf8();
        }
    }
    result
}

/// Expand a plan's `for_each` block: match files via glob, apply exclude/filter,
/// substitute template variables into each operation, and flatten the result into
/// `plan.operations`. After this call, `plan.for_each` is `None`.
///
/// Template variables: `{path}`, `{dir}`, `{stem}`, `{ext}`, `{name}`.
///
/// Doubled braces (`{{` / `}}`) are treated as escape sequences and produce
/// literal `{` / `}` in the output. For example, `{{path}}` becomes the
/// literal string `{path}` rather than being substituted with the file path.
#[cfg(feature = "cli")]
pub fn expand_for_each(plan: &mut Plan, cwd: &std::path::Path) -> anyhow::Result<()> {
    let fe = match plan.for_each.take() {
        Some(fe) => fe,
        None => return Ok(()),
    };

    // 1. Collect matching files.
    let glob_set = crate::files::build_glob_matcher(std::slice::from_ref(&fe.glob))?
        .ok_or_else(|| anyhow::anyhow!("for_each: invalid glob pattern"))?;

    let all_files = crate::files::collect_file_paths(cwd, false)?;
    let mut matched: Vec<std::path::PathBuf> = all_files
        .into_iter()
        .filter(|p| {
            let rel = p.strip_prefix(cwd).unwrap_or(p);
            glob_set.is_match(rel)
        })
        .collect();
    matched.sort();

    // 2. Apply exclude patterns.
    if !fe.exclude.is_empty() {
        let excl = crate::files::build_glob_matcher(&fe.exclude)?;
        if let Some(excl_set) = excl {
            matched.retain(|p| {
                let rel = p.strip_prefix(cwd).unwrap_or(p);
                !excl_set.is_match(rel)
            });
        }
    }

    // 3. Apply filter (currently supports `has_symbol(NAME)`).
    if let Some(ref filter) = fe.filter {
        let filter = filter.trim();
        if let Some(sym_name) = filter
            .strip_prefix("has_symbol(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let sym_name = sym_name.trim();
            #[cfg(feature = "ast")]
            {
                matched.retain(|p| {
                    let syms = crate::ast::symbols::extract_symbols_from_file(p, None);
                    syms.iter().any(|s| s.name == sym_name)
                });
            }
            #[cfg(not(feature = "ast"))]
            {
                let _ = sym_name;
                anyhow::bail!("for_each filter `has_symbol(...)` requires the `ast` feature");
            }
        } else {
            anyhow::bail!("for_each: unsupported filter expression: {filter}");
        }
    }

    if matched.is_empty() {
        // No files matched; clear operations (nothing to do).
        plan.operations.clear();
        return Ok(());
    }

    // 4. Serialize template operations once, then substitute per file.
    let template_ops_json = serde_json::to_string(&plan.operations)?;

    // Protect escaped doubles `{{` / `}}` so they become literal braces
    // in the output rather than being interpreted as template variables.
    // Sentinel chars (\x00) are safe because they cannot appear in valid JSON.
    // Hoisted outside the loop since template_ops_json is invariant.
    let protected = template_ops_json
        .replace("{{", "\x00LBRACE\x00")
        .replace("}}", "\x00RBRACE\x00");

    let mut expanded = Vec::with_capacity(matched.len() * plan.operations.len());
    for file_path in &matched {
        let rel = file_path
            .strip_prefix(cwd)
            .unwrap_or(file_path)
            .to_string_lossy();
        let rel_str = rel.replace('\\', "/");

        let dir = std::path::Path::new(&rel_str)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = std::path::Path::new(&rel_str)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = std::path::Path::new(&rel_str)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = std::path::Path::new(&rel_str)
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();

        // JSON-escape all substitution values so file paths containing
        // quotes, backslashes, or control characters don't produce invalid JSON.
        // Use single-pass substitution to prevent cross-contamination: if a
        // file path contains a literal "{name}", sequential .replace() would
        // double-substitute it. Single-pass scans the template once and
        // replaces each placeholder from the original template text.
        let vars: &[(&str, String)] = &[
            ("{path}", json_escape(&rel_str)),
            ("{dir}", json_escape(&dir)),
            ("{stem}", json_escape(&stem)),
            ("{ext}", json_escape(&ext)),
            ("{name}", json_escape(&name)),
        ];
        let substituted = substitute_single_pass(&protected, vars);

        // Restore sentinels to literal single braces.
        let substituted = substituted
            .replace("\x00LBRACE\x00", "{")
            .replace("\x00RBRACE\x00", "}");

        let file_ops: Vec<Operation> = serde_json::from_str(&substituted)
            .map_err(|e| anyhow::anyhow!("for_each: template expansion failed: {e}"))?;
        expanded.extend(file_ops);
    }

    plan.operations = expanded;
    Ok(())
}

#[cfg(test)]
mod tests;
