//! Plan `Operation` enum and helpers.
//!
//! Split from `plan/mod.rs` for file-size hygiene (#1376).

use serde::{Deserialize, Serialize};

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
        /// Alias `key` accepted because agents often emit that name (LLM prior).
        #[serde(alias = "key")]
        selector: String,
        /// Value to set (any JSON type).
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete")]
    DocDelete {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to delete.
        #[serde(alias = "key")]
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
        #[serde(alias = "key")]
        selector: String,
        /// Value to append to the array.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.prepend")]
    DocPrepend {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to an array.
        #[serde(alias = "key")]
        selector: String,
        /// Value to prepend to the array.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.update")]
    DocUpdate {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path (supports wildcards and predicates).
        #[serde(alias = "key")]
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
        #[serde(alias = "key")]
        selector: String,
        /// Value to set only if the key does not already exist.
        value: serde_json::Value,
    },
    #[serde(rename = "doc.delete_where")]
    DocDeleteWhere {
        /// Path to the JSON, YAML, or TOML file.
        path: String,
        /// Dot-notation selector path to an array.
        #[serde(alias = "key")]
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
        crate::plan::declared_paths(self)
    }
}
