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
            ..
        } => {
            if from.is_empty() {
                anyhow::bail!("replace operation requires a non-empty search pattern");
            }
            if *nth == Some(0) {
                anyhow::bail!("replace nth is 1-based; use 1 for the first occurrence");
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
