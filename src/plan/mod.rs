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
    /// Optional re-root for relative operation paths and lifecycle steps.
    ///
    /// Relative values resolve from the invocation / MCP server workspace root
    /// (not from the plan file location). Example: `"fixtures/complex"` with
    /// op path `"config.json"` targets `fixtures/complex/config.json`.
    ///
    /// On MCP, use a **relative** path that stays inside the server workspace;
    /// absolute path strings and `../` escapes are rejected. Do not combine
    /// with `for_each` (glob expansion is relative to the server root; use
    /// workspace-relative `{path}` templates without `cwd` instead).
    /// CLI and library callers may use absolute paths when PathGuard policy
    /// allows them.
    pub cwd: Option<String>,
    pub write_policy: Option<crate::write::WritePolicyOverride>,
    /// When omitted from the plan, defaults to strict mode at execution time.
    #[serde(default)]
    pub strict: Option<bool>,
    /// Operations to run. Accepts alias `ops` (common agent shorthand).
    #[serde(alias = "ops")]
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
                    other => {
                        return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                            msg: format!("unknown verify key: {other}"),
                        }));
                    }
                }
            } else {
                // Bare word like "function" treated as kind
                kind = Some(part.to_string());
            }
        }
        if let Some(kind) = kind {
            Ok(VerifyCheck::SymbolCount { kind, attr })
        } else {
            Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: "verify spec must contain 'kind=<type>' or a named check (unique_names, no_orphans)".into(),
            }))
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

mod operation;
pub use operation::Operation;
#[cfg(feature = "ast")]
pub use operation::SplitTargetSpec;

/// Convert a doc-family `Operation` into a `(path, DocMutation)` pair.
///
/// Returns `None` for non-doc operations. This is the single source of truth
/// for mapping `Operation::Doc*` variants to `DocMutation`, used by both the
/// tx engine (`tx/execute/`) and any future callers.
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
        | Operation::AstRewriteSignature { path, .. }
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
                return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                    msg: "for_each filter `has_symbol(...)` requires the `ast` feature".into(),
                }));
            }
        } else {
            return Err(anyhow::Error::new(crate::exit::InvalidInputError {
                msg: format!("for_each: unsupported filter expression: {filter}"),
            }));
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
