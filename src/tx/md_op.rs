use super::execute::{TxState, read_file_content};
use super::output::TxLintResult;
use crate::ops::md::{
    dedupe_headings_in, insert_after_heading_in, insert_after_section_in, insert_before_heading_in,
    move_section_in, replace_section_in, upsert_bullet_in,
};
use crate::plan::Operation;

/// Apply a markdown heading operation (read file, transform, write back).
///
/// Five of the six md operations follow an identical pattern: resolve path,
/// read content, call a `(&str, &str, &str) -> Option<String>` transform,
/// error on `None`, and update pending state. This helper captures that pattern.
fn apply_md_heading_op(
    tx: &mut TxState<'_>,
    path: &str,
    heading: &str,
    extra: &str,
    op: impl FnOnce(&str, &str, &str) -> Option<String>,
    err_label: &str,
) -> anyhow::Result<()> {
    let file_path = tx.cwd.join(path);
    // Sole binary must not surface as heading no_matches (NUL is valid UTF-8).
    crate::ops::file::ensure_not_binary_file(&file_path, path)?;
    let file_content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
    let new_content =
        op(file_content, heading, extra).ok_or_else(|| crate::exit::NoMatchError {
            msg: format!("{err_label} not found: {heading}"),
        })?;
    tx.write_file(&file_path, new_content);
    Ok(())
}

/// Execute a markdown operation within a transaction.
pub(crate) fn execute_md_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::MdReplaceSection {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(tx, path, heading, content, replace_section_in, "heading")?;
        }

        Operation::MdInsertAfterHeading {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(
                tx,
                path,
                heading,
                content,
                insert_after_heading_in,
                "heading",
            )?;
        }

        Operation::MdInsertAfterSection {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(
                tx,
                path,
                heading,
                content,
                insert_after_section_in,
                "heading",
            )?;
        }

        Operation::MdInsertBeforeHeading {
            path,
            heading,
            content,
        } => {
            apply_md_heading_op(
                tx,
                path,
                heading,
                content,
                insert_before_heading_in,
                "heading",
            )?;
        }

        Operation::MdUpsertBullet {
            path,
            heading,
            bullet,
        } => {
            apply_md_heading_op(tx, path, heading, bullet, upsert_bullet_in, "heading")?;
        }

        Operation::MdTableAppend { path, heading, row } => {
            let file_path = tx.cwd.join(path);
            crate::ops::file::ensure_not_binary_file(&file_path, path)?;
            let file_content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let (body_start, body_end) = crate::ops::md::find_section(file_content, heading)
                .ok_or_else(|| crate::exit::NoMatchError {
                    msg: format!("heading not found: {heading}"),
                })?;
            let new_content =
                crate::ops::md::table_append_in(file_content, body_start, body_end, row).map_err(
                    |e| {
                        anyhow::Error::new(crate::exit::InvalidInputError {
                            msg: format!("{e} under heading {heading:?}"),
                        })
                    },
                )?;
            tx.write_file(&file_path, new_content);
        }

        Operation::MdMoveSection {
            path,
            heading,
            to,
            before,
            after,
        } => {
            let position = match (before.as_deref(), after.as_deref()) {
                (Some(b), None) => ("before", b),
                (None, Some(a)) => ("after", a),
                _ => {
                    return Err(crate::exit::InvalidInputError {
                        msg: "md.move_section requires exactly one of 'before' or 'after'".into(),
                    }
                    .into());
                }
            };
            let dest_path_str = to.as_deref().unwrap_or(path.as_str());
            let source_path = tx.cwd.join(path);
            let dest_path = tx.cwd.join(dest_path_str);
            crate::ops::file::ensure_not_binary_file(&source_path, path)?;
            if dest_path != source_path {
                crate::ops::file::ensure_not_binary_file(&dest_path, dest_path_str)?;
            }
            let same_file = to.is_none()
                || source_path == dest_path
                || matches!(
                    (source_path.canonicalize(), dest_path.canonicalize()),
                    (Ok(ref s), Ok(ref d)) if s == d
                );
            let source_content =
                read_file_content(tx.pending, tx.existed_before, &source_path)?.to_owned();
            let dest_content = if same_file {
                source_content.clone()
            } else {
                read_file_content(tx.pending, tx.existed_before, &dest_path)?.to_owned()
            };
            let (new_source, new_dest) =
                move_section_in(&source_content, heading, &dest_content, position, same_file)
                    .ok_or_else(|| crate::exit::NoMatchError {
                        msg: "md.move_section: heading or target not found".to_string(),
                    })?;
            tx.write_file(&source_path, new_source);
            if !same_file {
                tx.write_file(&dest_path, new_dest);
            }
        }

        Operation::MdDedupeHeadings { path } => {
            let file_path = tx.cwd.join(path);
            crate::ops::file::ensure_not_binary_file(&file_path, path)?;
            let file_content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let (new_content, _removed) = dedupe_headings_in(file_content);
            tx.write_file(&file_path, new_content);
        }

        Operation::MdLintAgents { path } => {
            let file_path = tx.cwd.join(path);
            crate::ops::file::ensure_not_binary_file(&file_path, path)?;
            let content = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let issues = crate::ops::md::lint_agents_content(content);
            tx.tx_lints.push(TxLintResult {
                path: path.clone(),
                issue_count: issues.len(),
                issues,
            });
        }

        _ => unreachable!("execute_md_op called with non-Md operation"),
    }

    Ok(0)
}
