use crate::ops::replace::{ReplaceValidationParams, validate_replace_args};
use crate::plan::{Operation, Plan};

/// Short label for an operation, used in error messages.
/// Delegates to [`Operation::label()`].
pub(super) fn op_label(op: &Operation) -> &'static str {
    op.label()
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
        } => validate_replace_args(&ReplaceValidationParams {
            pattern: from,
            has_to: to.is_some(),
            has_insert_before: insert_before.is_some(),
            has_insert_after: insert_after.is_some(),
            nth: *nth,
            whole_line: *whole_line,
            multiline: *multiline,
            has_range: range.is_some(),
        })
        .map_err(|e| anyhow::anyhow!("replace: {e}")),
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
        | Operation::FileAppend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. }
        | Operation::Read { .. }
        | Operation::MdLintAgents { .. }
        | Operation::PatchApply { .. } => Ok(()),
        #[cfg(feature = "ast")]
        Operation::AstRename { .. }
        | Operation::AstReplace { .. }
        | Operation::AstInsert { .. }
        | Operation::AstWrap { .. }
        | Operation::AstImports { .. } => Ok(()),
        Operation::TidyFix { dedent, indent, .. } => {
            if dedent.is_some() && indent.is_some() {
                anyhow::bail!("tidy.fix: 'dedent' and 'indent' cannot both be set");
            }
            Ok(())
        }
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
            err.to_string().contains("whole_line and multiline"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replace_range_without_whole_line_rejected() {
        let op = replace_op(false, false, Some("10:50"));
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string().contains("range requires whole_line"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn replace_range_with_whole_line_accepted() {
        let op = replace_op(true, false, Some("10:50"));
        validate_operation(&op).unwrap();
    }

    #[test]
    fn replace_whole_line_without_multiline_accepted() {
        let op = replace_op(true, false, None);
        validate_operation(&op).unwrap();
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
            err.to_string().contains("search pattern must not be empty"),
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
            err.to_string().contains("nth must be >= 1"),
            "unexpected error: {err}"
        );
    }
}
