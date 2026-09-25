use super::execute::{
    TxState, enforce_expected_sha256, mark_write_target, read_file_content, update_file_content,
};
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
            // #2534: a directory expands with the same walker as CLI tidy fix,
            // including dotfiles. .git stays skipped inside the walker (#2639).
            // Do not treat path globs as dest-glob; only a real directory.
            if file_path.is_dir() {
                let files = crate::files::collect_file_paths(&file_path, true)?;
                let mut n = 0;
                for file in files {
                    let rel = file
                        .strip_prefix(tx.cwd)
                        .unwrap_or(file.as_path())
                        .to_string_lossy()
                        .replace('\\', "/");
                    enforce_expected_sha256(&rel, tx)?;
                    let child = Operation::TidyFix {
                        path: rel,
                        ensure_final_newline: *ensure_final_newline,
                        trim_trailing_whitespace: *trim_trailing_whitespace,
                        normalize_eol: normalize_eol.clone(),
                        collapse_blanks: *collapse_blanks,
                        dedent: dedent.clone(),
                        indent: indent.clone(),
                        lines: lines.clone(),
                    };
                    n += execute_tidy_op(&child, tx)?;
                }
                return Ok(n);
            }
            mark_write_target(tx.write_targets, &file_path);
            let content = read_file_content(tx.pending, tx.existed_before, &file_path)?.to_owned();
            // Precedence (#1840): CLI tidy-fix defaults (trim + final newline)
            // -> plan write_policy (if set) -> op-level fields (if Some).
            let mut policy = WritePolicy {
                ensure_final_newline: true,
                trim_trailing_whitespace: true,
                normalize_eol: EolMode::Keep,
                collapse_blanks: false,
                charset: crate::write::CharsetMode::Keep,
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
            policy.refuse_unsupported_charset()?;
            let mut new = crate::write::apply_policy(&content, &policy).into_owned();
            // Match CLI default tidy fix: unmix when neither the op nor
            // plan write_policy set normalize_eol.
            let plan_set_eol = tx
                .plan_write_policy
                .and_then(|ov| ov.normalize_eol.as_ref())
                .is_some();
            if normalize_eol.is_none() && !plan_set_eol {
                new = crate::write::unmix_eol(&new).into_owned();
            }

            // Apply dedent/indent after policy normalization.
            let line_range = lines
                .as_deref()
                .map(crate::ops::read::parse_line_range)
                .transpose()?;
            if let Some(spec) = dedent {
                crate::write::parse_dedent_spec(spec)?;
                new = crate::write::dedent_content(&new, spec, line_range);
            }
            if let Some(spec) = indent {
                crate::write::parse_indent_spec(spec)?;
                new = crate::write::indent_content(&new, spec, line_range);
            }

            if content != new {
                // Do not use write_file: that clears policy_finalized.
                update_file_content(tx.pending, tx.deletions, tx.write_targets, &file_path, new);
            }
            // #1847: content already reflects effective tidy policy (op wins).
            tx.policy_finalized.insert(file_path);
            Ok(0)
        }

        _ => unreachable!("execute_tidy_op called with non-Tidy operation"),
    }
}
