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
            let policy = WritePolicy {
                ensure_final_newline: ensure_final_newline.unwrap_or(true),
                trim_trailing_whitespace: trim_trailing_whitespace.unwrap_or(false),
                normalize_eol: if let Some(eol) = normalize_eol {
                    crate::write::parse_eol_mode(eol)?
                } else {
                    EolMode::Keep
                },
                collapse_blanks: collapse_blanks.unwrap_or(false),
            };
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
