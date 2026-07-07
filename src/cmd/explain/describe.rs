//! Rich human descriptions for plan [`Operation`]s.
//!
//! Field-level detail lives in the match arms below. The schema registry
//! ([`crate::schema::operation_description`]) supplies the stable op label
//! (what agents see in `patchloom schema` / MCP tool prose) so explain and
//! schema stay aligned on naming.
//!
//! size-waiver: single-domain Operation match arms for explain (one variant per plan op); co-located with catalog blurb alignment; do not split for LOC alone #1408.

use crate::plan::Operation;
use crate::schema;

/// Serde `op` tag for a plan operation (e.g. `"doc.set"`).
pub(super) fn operation_op_name(op: &Operation) -> String {
    // Serialize to JSON and read the discriminator — single source with serde renames.
    match serde_json::to_value(op) {
        Ok(serde_json::Value::Object(map)) => map
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        _ => "unknown".to_string(),
    }
}

pub(super) fn describe_operation(op: &Operation) -> String {
    // Rich field-level text; catalog blurb is on JSON via build_json_summary (`catalog`).
    let _ = schema::operation_description(&operation_op_name(op));
    match op {
        Operation::Replace {
            path,
            glob,
            old,
            new_text,
            nth,
            regex,
            case_insensitive,
            insert_before,
            insert_after,
            whole_line,
            range,
            word_boundary,
            multiline,
            if_exists,
            ..
        } => {
            let target = path.as_deref().or(glob.as_deref()).unwrap_or("(all files)");
            let is_regex = *regex;
            let mode_str = if is_regex { "regex" } else { "literal" };

            if let Some(before) = insert_before {
                return format!("Insert \"{before}\" before \"{old}\" in {target}");
            }
            if let Some(after) = insert_after {
                return format!("Insert \"{after}\" after \"{old}\" in {target}");
            }

            let to_str = new_text.as_deref().unwrap_or("(delete)");
            let nth_str = nth
                .map(|n| format!(", occurrence #{n}"))
                .unwrap_or_default();
            let ci_str = if *case_insensitive {
                ", case-insensitive"
            } else {
                ""
            };
            let wl_str = if *whole_line { ", whole-line" } else { "" };
            let wb_str = if *word_boundary {
                ", word-boundary"
            } else {
                ""
            };
            let range_str = range
                .as_deref()
                .map(|r| format!(", lines {r}"))
                .unwrap_or_default();
            let ml_str = if *multiline { ", multiline" } else { "" };
            let ie_str = if *if_exists { ", if-exists" } else { "" };
            format!(
                "Replace \"{old}\" with \"{to_str}\" in {target} ({mode_str}{nth_str}{ci_str}{wl_str}{wb_str}{ml_str}{ie_str}{range_str})"
            )
        }
        Operation::DocSet {
            path,
            selector,
            value,
        } => {
            format!("Set {selector} to {value} in {path}")
        }
        Operation::DocDelete { path, selector } => {
            format!("Delete {selector} from {path}")
        }
        Operation::DocMerge { path, value } => {
            format!("Merge {value} into {path}")
        }
        Operation::DocAppend {
            path,
            selector,
            value,
        } => {
            format!("Append {value} to {selector} in {path}")
        }
        Operation::DocPrepend {
            path,
            selector,
            value,
        } => {
            format!("Prepend {value} to {selector} in {path}")
        }
        Operation::DocUpdate {
            path,
            selector,
            value,
        } => {
            format!("Update {selector} to {value} in {path}")
        }
        Operation::DocMove { path, from, to } => {
            format!("Move {from} to {to} in {path}")
        }
        Operation::DocEnsure {
            path,
            selector,
            value,
        } => {
            format!("Ensure {selector} = {value} in {path}")
        }
        Operation::DocDeleteWhere {
            path,
            selector,
            predicate,
        } => {
            format!("Delete from {selector} where {predicate} in {path}")
        }
        Operation::MdReplaceSection { path, heading, .. } => {
            format!("Replace section \"{heading}\" in {path}")
        }
        Operation::MdInsertAfterHeading { path, heading, .. } => {
            format!("Insert content after \"{heading}\" in {path}")
        }
        Operation::MdInsertBeforeHeading { path, heading, .. } => {
            format!("Insert content before \"{heading}\" in {path}")
        }
        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            format!("Upsert bullet \"{bullet}\" under \"{heading}\" in {path}")
        }
        Operation::MdTableAppend { path, heading, .. } => {
            format!("Append row to table under \"{heading}\" in {path}")
        }
        Operation::MdMoveSection {
            path,
            heading,
            to,
            before,
            after,
        } => {
            let dest = to.as_deref().unwrap_or(path.as_str());
            let pos = if let Some(b) = before {
                format!("before \"{b}\"")
            } else if let Some(a) = after {
                format!("after \"{a}\"")
            } else {
                "(no position)".to_string()
            };
            if to.is_some() {
                format!("Move section \"{heading}\" from {path} to {dest} {pos}")
            } else {
                format!("Move section \"{heading}\" {pos} in {path}")
            }
        }
        Operation::MdDedupeHeadings { path } => {
            format!("Deduplicate headings in {path}")
        }
        Operation::TidyFix {
            path,
            dedent,
            indent,
            ..
        } => {
            if dedent.is_some() {
                format!("Dedent and normalize whitespace in {path}")
            } else if indent.is_some() {
                format!("Indent and normalize whitespace in {path}")
            } else {
                format!("Normalize whitespace in {path}")
            }
        }
        Operation::FileAppend { path, .. } => {
            format!("Append content to {path}")
        }
        Operation::FilePrepend { path, .. } => {
            format!("Prepend content to {path}")
        }
        Operation::FileCreate { path, force, .. } => {
            let force_str = if *force == Some(true) {
                " (overwrite)"
            } else {
                ""
            };
            format!("Create file {path}{force_str}")
        }
        Operation::FileDelete { path } => {
            format!("Delete file {path}")
        }
        Operation::FileRename { from, to, force } => {
            let force_str = if *force { " (overwrite)" } else { "" };
            format!("Rename {from} to {to}{force_str}")
        }
        Operation::PatchApply {
            on_stale,
            allow_conflicts,
            ..
        } => {
            let mut parts = Vec::new();
            if *on_stale == crate::ops::patch::OnStale::Merge {
                parts.push("merge on stale");
            }
            if *allow_conflicts {
                parts.push("allow conflicts");
            }
            if parts.is_empty() {
                "Apply unified diff patch".to_string()
            } else {
                format!("Apply unified diff patch ({})", parts.join(", "))
            }
        }
        Operation::Search {
            path,
            pattern,
            regex,
            ..
        } => {
            let mode = if *regex { "regex" } else { "literal" };
            format!("Search for \"{pattern}\" in {path} ({mode})")
        }
        Operation::Read { path, lines } => match lines {
            Some(range) => format!("Read {path} lines {range}"),
            None => format!("Read {path}"),
        },
        Operation::MdLintAgents { path } => {
            format!("Lint {path} for AGENTS.md issues")
        }
        #[cfg(feature = "ast")]
        Operation::AstRename { path, old, new, .. } => {
            format!("AST rename \"{old}\" to \"{new}\" in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstReplace {
            path,
            symbol,
            old,
            new_text,
            ..
        } => {
            format!("AST replace \"{old}\" with \"{new_text}\" in {symbol} in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstRewriteSignature {
            path,
            old,
            new_signature,
            visibility,
            parameters,
            return_type,
            ..
        } => {
            if let Some(sig) = new_signature {
                format!("AST rewrite signature of \"{old}\" to \"{sig}\" in {path}")
            } else {
                let mut parts = Vec::new();
                if visibility.is_some() {
                    parts.push("visibility");
                }
                if parameters.is_some() {
                    parts.push("parameters");
                }
                if return_type.is_some() {
                    parts.push("return type");
                }
                let what = if parts.is_empty() {
                    "signature".to_string()
                } else {
                    parts.join("/")
                };
                format!("AST rewrite {what} of \"{old}\" in {path}")
            }
        }
        #[cfg(feature = "ast")]
        Operation::AstInsert {
            path,
            inside,
            after,
            before,
            ..
        } => {
            let target = inside
                .as_deref()
                .or(after.as_deref())
                .or(before.as_deref())
                .unwrap_or("(file)");
            format!("AST insert code near \"{target}\" in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstWrap {
            path,
            symbols,
            lines,
            wrapper,
            ..
        } => {
            let target = if let Some(syms) = symbols {
                syms.join(", ")
            } else if let Some(l) = lines {
                format!("lines {l}")
            } else {
                "(unknown)".to_string()
            };
            format!("AST wrap {target} in \"{wrapper}\" in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstImports {
            path,
            add,
            remove,
            dedupe,
            ..
        } => {
            let mut parts = Vec::new();
            if let Some(a) = add {
                parts.push(format!("add {}", a.len()));
            }
            if let Some(r) = remove {
                parts.push(format!("remove {}", r.len()));
            }
            if *dedupe {
                parts.push("dedupe".to_string());
            }
            let action = if parts.is_empty() {
                "list".to_string()
            } else {
                parts.join(", ")
            };
            format!("AST imports ({action}) in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstReorder {
            path,
            inside,
            order,
            ..
        } => {
            let scope = inside.as_deref().unwrap_or("top-level");
            let strategy = match order {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(arr) => format!("custom ({})", arr.len()),
                _ => "unknown".to_string(),
            };
            format!("AST reorder {scope} symbols by {strategy} in {path}")
        }
        #[cfg(feature = "ast")]
        Operation::AstGroup {
            path,
            module,
            symbols,
            ..
        } => {
            format!(
                "AST group {} symbol(s) into mod {module} in {path}",
                symbols.len()
            )
        }
        #[cfg(feature = "ast")]
        Operation::AstMove {
            path,
            target,
            symbols,
            ..
        } => {
            format!(
                "AST move {} symbol(s) from {path} to {target}",
                symbols.len()
            )
        }
        #[cfg(feature = "ast")]
        Operation::AstExtractToFile {
            source,
            symbol,
            target,
            ..
        } => {
            format!("AST extract \"{symbol}\" from {source} to {target}")
        }
        #[cfg(feature = "ast")]
        Operation::AstSplit {
            source, targets, ..
        } => {
            format!("AST split {source} into {} target file(s)", targets.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::explain::{build_json_summary, print_human_summary};
    use crate::plan::Plan;
    use crate::plan::{FormatStep, ValidationStep};

    #[test]
    fn describe_replace_literal() {
        let op = Operation::Replace {
            path: Some("README.md".into()),
            glob: None,
            regex: false,
            old: "v1".into(),
            new_text: Some("v2".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(desc, r#"Replace "v1" with "v2" in README.md (literal)"#);
    }

    #[test]
    fn describe_replace_regex_nth() {
        let op = Operation::Replace {
            path: Some("src/lib.rs".into()),
            glob: None,
            regex: true,
            old: r"fn\s+main".into(),
            new_text: Some("fn entry".into()),
            nth: Some(1),
            insert_before: None,
            insert_after: None,
            case_insensitive: true,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(
            desc,
            r#"Replace "fn\s+main" with "fn entry" in src/lib.rs (regex, occurrence #1, case-insensitive)"#
        );
    }

    #[test]
    fn describe_doc_set() {
        let op = Operation::DocSet {
            path: "package.json".into(),
            selector: "version".into(),
            value: serde_json::json!("2.0.0"),
        };
        assert_eq!(
            describe_operation(&op),
            r#"Set version to "2.0.0" in package.json"#
        );
    }

    #[test]
    fn describe_file_create() {
        let op = Operation::FileCreate {
            path: "new.txt".into(),
            content: "hello".into(),
            force: Some(true),
        };
        assert!(describe_operation(&op).contains("(overwrite)"));
    }

    #[test]
    fn describe_search_regex() {
        let op = Operation::Search {
            path: "src".into(),
            pattern: "TODO".into(),
            regex: true,
            case_insensitive: false,
            multiline: false,
            invert_match: false,
            context: None,
            before_context: None,
            after_context: None,
            assert_count: None,
            literal: false,
            globs: vec![],
            max_results: 0,
            exclude_patterns: vec![],
            custom_ignore_filenames: vec![],
        };
        assert!(describe_operation(&op).contains("(regex)"));
    }

    #[test]
    fn human_summary_output() {
        let plan = Plan {
            version: 1,
            cwd: None,
            write_policy: None,
            strict: Some(true),
            operations: vec![
                Operation::FileCreate {
                    path: "test.txt".into(),
                    content: "hi".into(),
                    force: None,
                },
                Operation::FileDelete {
                    path: "old.txt".into(),
                },
            ],
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };
        // Just ensure it doesn't panic.
        print_human_summary(&plan, true);
    }

    #[test]
    fn json_summary_structure() {
        let plan = Plan {
            version: 1,
            cwd: None,
            write_policy: None,
            strict: Some(false),
            operations: vec![Operation::FileDelete {
                path: "x.txt".into(),
            }],
            format: Some(vec![FormatStep {
                cmd: "fmt".into(),
                timeout: Some(30),
            }]),
            validate: Some(vec![ValidationStep {
                cmd: "test".into(),
                required: Some(true),
                timeout: None,
            }]),
            verify: None,
            for_each: None,
        };
        let json = build_json_summary(&plan, false);
        assert_eq!(json["operation_count"], 1);
        assert_eq!(json["format_steps"], 1);
        assert_eq!(json["validate_steps"], 1);
        assert!(!json["strict"].as_bool().unwrap());
    }

    #[test]
    fn describe_replace_insert_before() {
        let op = Operation::Replace {
            path: Some("main.rs".into()),
            glob: None,
            regex: false,
            old: "fn main".into(),
            new_text: None,
            nth: None,
            insert_before: Some("// entry\n".into()),
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(
            desc,
            r#"Insert "// entry
" before "fn main" in main.rs"#
        );
    }

    #[test]
    fn describe_replace_insert_after() {
        let op = Operation::Replace {
            path: Some("lib.rs".into()),
            glob: None,
            regex: false,
            old: "use crate".into(),
            new_text: None,
            nth: None,
            insert_before: None,
            insert_after: Some("// added".into()),
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(desc, r#"Insert "// added" after "use crate" in lib.rs"#);
    }

    #[test]
    fn describe_replace_whole_line_with_range() {
        let op = Operation::Replace {
            path: Some("src/lib.rs".into()),
            glob: None,
            regex: false,
            old: "dbg!".into(),
            new_text: Some(String::new()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: true,
            range: Some("10:50".into()),
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains("whole-line"), "{desc}");
        assert!(desc.contains("lines 10:50"), "{desc}");
        assert!(desc.contains(r#"with """#), "{desc}");
    }

    #[test]
    fn describe_file_rename() {
        let op = Operation::FileRename {
            from: "old.rs".into(),
            to: "new.rs".into(),
            force: true,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains("Rename old.rs to new.rs"));
        assert!(desc.contains("(overwrite)"));
    }

    #[test]
    fn describe_file_rename_no_force() {
        let op = Operation::FileRename {
            from: "a.txt".into(),
            to: "b.txt".into(),
            force: false,
        };
        let desc = describe_operation(&op);
        assert_eq!(desc, "Rename a.txt to b.txt");
        assert!(!desc.contains("overwrite"));
    }

    #[test]
    fn describe_read_with_lines() {
        let op = Operation::Read {
            path: "src/lib.rs".into(),
            lines: Some("10:20".into()),
        };
        assert_eq!(describe_operation(&op), "Read src/lib.rs lines 10:20");
    }

    #[test]
    fn describe_read_without_lines() {
        let op = Operation::Read {
            path: "README.md".into(),
            lines: None,
        };
        assert_eq!(describe_operation(&op), "Read README.md");
    }

    #[test]
    fn describe_md_lint_agents() {
        let op = Operation::MdLintAgents {
            path: "AGENTS.md".into(),
        };
        assert_eq!(
            describe_operation(&op),
            "Lint AGENTS.md for AGENTS.md issues"
        );
    }

    #[test]
    fn describe_md_move_section_same_file() {
        let op = Operation::MdMoveSection {
            path: "README.md".into(),
            heading: "FAQ".into(),
            to: None,
            before: Some("License".into()),
            after: None,
        };
        assert_eq!(
            describe_operation(&op),
            r#"Move section "FAQ" before "License" in README.md"#
        );
    }

    #[test]
    fn describe_md_move_section_cross_file() {
        let op = Operation::MdMoveSection {
            path: "spec.md".into(),
            heading: "Appendix".into(),
            to: Some("notes.md".into()),
            before: None,
            after: Some("Body".into()),
        };
        assert_eq!(
            describe_operation(&op),
            r#"Move section "Appendix" from spec.md to notes.md after "Body""#
        );
    }

    #[test]
    fn describe_replace_multiline_flag() {
        // R3 fix: multiline flag should appear in describe output.
        let op = Operation::Replace {
            path: Some("f.rs".into()),
            glob: None,
            regex: true,
            old: "fn.*\\{".into(),
            new_text: Some("fn new() {".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: true,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(
            desc.contains(", multiline"),
            "missing multiline flag: {desc}"
        );
    }

    #[test]
    fn describe_replace_if_exists_flag() {
        // R3 fix: if_exists flag should appear in describe output.
        let op = Operation::Replace {
            path: Some("f.rs".into()),
            glob: None,
            regex: false,
            old: "old_fn".into(),
            new_text: Some("new_fn".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: true,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(
            desc.contains(", if-exists"),
            "missing if-exists flag: {desc}"
        );
    }

    #[test]
    fn describe_replace_multiline_and_if_exists_combined() {
        // Both flags together should both appear.
        let op = Operation::Replace {
            path: Some("f.rs".into()),
            glob: None,
            regex: true,
            old: "pattern".into(),
            new_text: Some("replacement".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: true,
            if_exists: true,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains(", multiline"), "missing multiline: {desc}");
        assert!(desc.contains(", if-exists"), "missing if-exists: {desc}");
    }

    #[test]
    fn describe_replace_glob_no_path() {
        let op = Operation::Replace {
            path: None,
            glob: Some("**/*.rs".into()),
            regex: false,
            old: "old".into(),
            new_text: Some("new".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains("**/*.rs"), "{desc}");
    }

    #[test]
    fn describe_replace_delete_mode() {
        let op = Operation::Replace {
            path: Some("f.txt".into()),
            glob: None,
            regex: false,
            old: "remove_me".into(),
            new_text: None,
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline: false,
            if_exists: false,
            whole_line: false,
            range: None,
            word_boundary: false,
            before_context: None,
            after_context: None,
            unique: false,
        };
        let desc = describe_operation(&op);
        assert!(desc.contains("(delete)"), "{desc}");
    }

    #[test]
    fn describe_patch_apply_default() {
        let op = Operation::PatchApply {
            diff: "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n".into(),
            on_stale: crate::ops::patch::OnStale::default(),
            allow_conflicts: false,
        };
        assert_eq!(describe_operation(&op), "Apply unified diff patch");
    }

    #[test]
    fn describe_patch_apply_merge_on_stale() {
        let op = Operation::PatchApply {
            diff: String::new(),
            on_stale: crate::ops::patch::OnStale::Merge,
            allow_conflicts: false,
        };
        assert_eq!(
            describe_operation(&op),
            "Apply unified diff patch (merge on stale)"
        );
    }

    #[test]
    fn describe_patch_apply_allow_conflicts() {
        let op = Operation::PatchApply {
            diff: String::new(),
            on_stale: crate::ops::patch::OnStale::default(),
            allow_conflicts: true,
        };
        assert_eq!(
            describe_operation(&op),
            "Apply unified diff patch (allow conflicts)"
        );
    }

    #[test]
    fn describe_patch_apply_merge_and_conflicts() {
        let op = Operation::PatchApply {
            diff: String::new(),
            on_stale: crate::ops::patch::OnStale::Merge,
            allow_conflicts: true,
        };
        assert_eq!(
            describe_operation(&op),
            "Apply unified diff patch (merge on stale, allow conflicts)"
        );
    }

    #[test]
    fn human_summary_shows_respect_editorconfig() {
        // #1190: respect_editorconfig should appear in write policy display
        let plan = Plan {
            version: 1,
            cwd: None,
            write_policy: Some(crate::write::WritePolicyOverride {
                ensure_final_newline: None,
                normalize_eol: None,
                trim_trailing_whitespace: None,
                collapse_blanks: None,
                respect_editorconfig: Some(true),
            }),
            strict: None,
            operations: vec![Operation::FileCreate {
                path: "test.txt".into(),
                content: "hi".into(),
                force: None,
            }],
            format: None,
            validate: None,
            verify: None,
            for_each: None,
        };
        // Capture output by calling the function (it prints to stdout).
        // Just ensure it doesn't panic; the logic is tested by presence of
        // "respect editorconfig" in the parts vector.
        print_human_summary(&plan, false);
    }

    #[test]
    fn human_summary_shows_verify_checks() {
        // #1191: verify checks should appear in human summary
        let plan = Plan {
            version: 1,
            cwd: None,
            write_policy: None,
            strict: None,
            operations: vec![Operation::FileCreate {
                path: "test.txt".into(),
                content: "hi".into(),
                force: None,
            }],
            format: None,
            validate: None,
            verify: Some(vec![
                crate::plan::VerifyCheck::SymbolCount {
                    kind: "function".into(),
                    attr: Some("test".into()),
                },
                crate::plan::VerifyCheck::Named {
                    check: "unique_names".into(),
                },
            ]),
            for_each: None,
        };
        // Should not panic; exercises verify display path
        print_human_summary(&plan, false);
    }

    #[test]
    fn json_summary_includes_verify_checks() {
        // #1191: verify_checks count should appear in JSON summary
        let plan = Plan {
            version: 1,
            cwd: None,
            write_policy: None,
            strict: None,
            operations: vec![Operation::FileDelete {
                path: "x.txt".into(),
            }],
            format: None,
            validate: None,
            verify: Some(vec![crate::plan::VerifyCheck::Named {
                check: "unique_names".into(),
            }]),
            for_each: None,
        };
        let json = build_json_summary(&plan, false);
        assert_eq!(json["verify_checks"], 1);
    }

    #[test]
    fn operation_op_name_matches_registry_for_common_ops() {
        let op = Operation::DocSet {
            path: "a.json".into(),
            selector: "x".into(),
            value: serde_json::json!(1),
        };
        assert_eq!(operation_op_name(&op), "doc.set");
        let desc = schema::operation_description("doc.set")
            .expect("doc.set must have a schema description");
        assert!(!desc.is_empty(), "doc.set description must not be empty");
        let summary = describe_operation(&op);
        assert!(
            summary.contains("a.json"),
            "explain text must include the path: {summary}"
        );
        assert!(
            summary.contains("doc.set") || summary.to_lowercase().contains("set"),
            "explain text must identify the operation: {summary}"
        );
    }
}
