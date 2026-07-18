use super::execute::{TxState, mark_write_target, read_file_content, update_file_content};
use crate::cli::global::EolMode;
use crate::plan::Operation;
use crate::write::WritePolicy;

/// Execute a tidy operation within a transaction.
pub(crate) fn execute_tidy_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::TidyFix {
            path,
            ensure_final_newline,
            trim_trailing_whitespace,
            normalize_eol,
            collapse_blanks,
            dedent,
            indent,
            lines,
        } => {
            let file_path = tx.cwd.join(path);
            mark_write_target(tx.write_targets, &file_path);
            let content = read_file_content(tx.pending, tx.existed_before, &file_path)?.to_owned();
            // Precedence (#1840): CLI tidy-fix defaults (trim + final newline)
            // -> plan write_policy (if set) -> op-level fields (if Some).
            let mut policy = WritePolicy {
                ensure_final_newline: true,
                trim_trailing_whitespace: true,
                normalize_eol: EolMode::Keep,
                collapse_blanks: false,
            };
            if let Some(ov) = tx.plan_write_policy {
                policy.apply_override(ov)?;
            }
            if let Some(v) = *ensure_final_newline {
                policy.ensure_final_newline = v;
            }
            if let Some(v) = *trim_trailing_whitespace {
                policy.trim_trailing_whitespace = v;
            }
            if let Some(eol) = normalize_eol {
                policy.normalize_eol = crate::write::parse_eol_mode(eol)?;
            }
            if let Some(v) = *collapse_blanks {
                policy.collapse_blanks = v;
            }
            let mut new = crate::write::apply_policy(&content, &policy).into_owned();

            // Apply dedent/indent after policy normalization.
            let line_range = lines
                .as_deref()
                .map(crate::ops::read::parse_line_range)
                .transpose()?;
            if let Some(spec) = dedent {
                new = crate::write::dedent_content(&new, spec, line_range);
            }
            if let Some(spec) = indent {
                new = crate::write::indent_content(&new, spec, line_range);
            }

            if content != new {
                update_file_content(tx.pending, tx.deletions, tx.write_targets, &file_path, new);
            }
            Ok(0)
        }

        _ => unreachable!("execute_tidy_op called with non-Tidy operation"),
    }
}
