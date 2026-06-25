use crate::ops::replace::{ReplaceModeError, validate_replace_mode};
use crate::plan::{Operation, Plan};

/// Short label for an operation, used in error messages.
pub(super) fn op_label(op: &Operation) -> &'static str {
    match op {
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

pub(crate) fn validate_operation(op: &Operation) -> anyhow::Result<()> {
    match op {
        Operation::Replace {
            from,
            to,
            insert_before,
            insert_after,
            nth,
            whole_line,
            multiline,
            range,
            ..
        } => {
            if from.is_empty() {
                anyhow::bail!("replace operation requires a non-empty search pattern");
            }
            if *nth == Some(0) {
                anyhow::bail!("replace nth is 1-based; use 1 for the first occurrence");
            }
            if *whole_line && *multiline {
                anyhow::bail!("replace: whole_line and multiline cannot be combined");
            }
            if range.is_some() && !*whole_line {
                anyhow::bail!("replace: range requires whole_line=true");
            }
            match validate_replace_mode(
                to.is_some(),
                insert_before.is_some(),
                insert_after.is_some(),
            ) {
                Ok(()) => Ok(()),
                Err(ReplaceModeError::MissingMode) => {
                    anyhow::bail!(
                        "replace operation requires one of to, insert_before, or insert_after"
                    )
                }
                Err(ReplaceModeError::BothInsertModes) => {
                    anyhow::bail!("insert_before and insert_after cannot both be set")
                }
                Err(ReplaceModeError::ToWithInsert) => {
                    anyhow::bail!("to cannot be combined with insert_before or insert_after")
                }
            }
        }
        // Exhaustive match ensures the compiler flags new variants that may
        // need validation constraints.
        Operation::DocSet { .. }
        | Operation::DocDelete { .. }
        | Operation::DocMerge { .. }
        | Operation::DocAppend { .. }
        | Operation::DocPrepend { .. }
        | Operation::DocUpdate { .. }
        | Operation::DocMove { .. }
        | Operation::DocEnsure { .. }
        | Operation::DocDeleteWhere { .. }
        | Operation::MdReplaceSection { .. }
        | Operation::MdInsertAfterHeading { .. }
        | Operation::MdInsertBeforeHeading { .. }
        | Operation::MdUpsertBullet { .. }
        | Operation::MdTableAppend { .. }
        | Operation::MdDedupeHeadings { .. }
        | Operation::TidyFix { .. }
        | Operation::FileAppend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. }
        | Operation::Read { .. }
        | Operation::MdLintAgents { .. }
        | Operation::PatchApply { .. } => Ok(()),
        #[cfg(feature = "ast")]
        Operation::AstRename { .. } | Operation::AstReplace { .. } => Ok(()),
        Operation::MdMoveSection { before, after, .. } => {
            if before.is_none() && after.is_none() {
                anyhow::bail!("md.move_section requires either 'before' or 'after'");
            }
            if before.is_some() && after.is_some() {
                anyhow::bail!("md.move_section: 'before' and 'after' cannot both be set");
            }
            Ok(())
        }
        Operation::Search {
            invert_match,
            multiline,
            literal,
            regex,
            ..
        } => {
            if *invert_match && *multiline {
                anyhow::bail!("search: invert_match and multiline cannot be combined");
            }
            if *literal && *regex {
                anyhow::bail!("search: literal and regex cannot be combined");
            }
            Ok(())
        }
    }
}

pub(crate) fn validate_plan_operations(plan: &Plan) -> anyhow::Result<()> {
    for op in &plan.operations {
        validate_operation(op)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a minimal valid Replace operation, then override fields.
    fn replace_op(whole_line: bool, multiline: bool, range: Option<&str>) -> Operation {
        Operation::Replace {
            glob: None,
            path: Some("f.txt".into()),
            mode: None,
            from: "needle".into(),
            to: Some("replacement".into()),
            nth: None,
            insert_before: None,
            insert_after: None,
            case_insensitive: false,
            multiline,
            if_exists: false,
            whole_line,
            range: range.map(String::from),
            word_boundary: false,
            before_context: None,
            after_context: None,
        }
    }

    #[test]
    fn replace_whole_line_and_multiline_rejected() {
        let op = replace_op(true, true, None);
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string()
                .contains("whole_line and multiline cannot be combined"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replace_range_without_whole_line_rejected() {
        let op = replace_op(false, false, Some("10:50"));
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line=true"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replace_range_with_whole_line_accepted() {
        let op = replace_op(true, false, Some("10:50"));
        assert!(validate_operation(&op).is_ok());
    }

    #[test]
    fn replace_whole_line_without_multiline_accepted() {
        let op = replace_op(true, false, None);
        assert!(validate_operation(&op).is_ok());
    }

    #[test]
    fn replace_empty_from_rejected() {
        let op = Operation::Replace {
            glob: None,
            path: Some("f.txt".into()),
            mode: None,
            from: String::new(),
            to: Some("x".into()),
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
        };
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string().contains("non-empty search pattern"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replace_nth_zero_rejected() {
        let mut op = replace_op(false, false, None);
        if let Operation::Replace { ref mut nth, .. } = op {
            *nth = Some(0);
        }
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string().contains("nth is 1-based"),
            "unexpected error: {err}"
        );
    }
}
