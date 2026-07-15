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
            old,
            new_text,
            insert_before,
            insert_after,
            nth,
            whole_line,
            multiline,
            range,
            ..
        } => validate_replace_args(&ReplaceValidationParams {
            pattern: old,
            has_to: new_text.is_some(),
            has_insert_before: insert_before.is_some(),
            has_insert_after: insert_after.is_some(),
            nth: *nth,
            whole_line: *whole_line,
            multiline: *multiline,
            has_range: range.is_some(),
        })
        .map_err(|e| {
            crate::exit::InvalidInputError {
                msg: format!("replace: {e}"),
            }
            .into()
        }),
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
        | Operation::MdInsertAfterSection { .. }
        | Operation::MdInsertBeforeHeading { .. }
        | Operation::MdUpsertBullet { .. }
        | Operation::MdTableAppend { .. }
        | Operation::MdDedupeHeadings { .. }
        | Operation::FileAppend { .. }
        | Operation::FilePrepend { .. }
        | Operation::FileCreate { .. }
        | Operation::FileDelete { .. }
        | Operation::FileRename { .. }
        | Operation::Read { .. }
        | Operation::MdLintAgents { .. }
        | Operation::PatchApply { .. } => Ok(()),
        #[cfg(feature = "ast")]
        Operation::AstRename { .. }
        | Operation::AstReplace { .. }
        | Operation::AstRewriteSignature { .. }
        | Operation::AstInsert { .. }
        | Operation::AstWrap { .. }
        | Operation::AstImports { .. }
        | Operation::AstReorder { .. }
        | Operation::AstGroup { .. }
        | Operation::AstMove { .. }
        | Operation::AstExtractToFile { .. }
        | Operation::AstSplit { .. } => Ok(()),
        Operation::TidyFix { dedent, indent, .. } => {
            if dedent.is_some() && indent.is_some() {
                return Err(crate::exit::InvalidInputError {
                    msg: "tidy.fix: 'dedent' and 'indent' cannot both be set".into(),
                }
                .into());
            }
            Ok(())
        }
        Operation::MdMoveSection { before, after, .. } => {
            if before.is_none() && after.is_none() {
                return Err(crate::exit::InvalidInputError {
                    msg: "md.move_section requires either 'before' or 'after'".into(),
                }
                .into());
            }
            if before.is_some() && after.is_some() {
                return Err(crate::exit::InvalidInputError {
                    msg: "md.move_section: 'before' and 'after' cannot both be set".into(),
                }
                .into());
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
                return Err(crate::exit::InvalidInputError {
                    msg: "search: invert_match and multiline cannot be combined".into(),
                }
                .into());
            }
            if *literal && *regex {
                return Err(crate::exit::InvalidInputError {
                    msg: "search: literal and regex cannot be combined".into(),
                }
                .into());
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
            regex: false,
            old: "needle".into(),
            new_text: Some("replacement".into()),
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
            unique: false,
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
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
            regex: false,
            old: String::new(),
            new_text: Some("x".into()),
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
            require_change: false,
            command_position: false,
            fuzzy: false,
            min_fuzzy_score: None,
            allow_absent_old: false,
        };
        let err = validate_operation(&op).unwrap_err();
        assert!(
            err.to_string()
                .contains("replace pattern must not be empty"),
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
