//! File create/append/prepend/delete/rename for the tx engine.
use super::{TxState, read_file_content, update_file_content};
use crate::plan::Operation;

// op_to_doc_mutation moved to plan.rs as the single source of truth for
// Operation::Doc* -> DocMutation conversion (see #901).

pub(crate) fn execute_file_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::FileAppend { path, content } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {path}"),
                }
                .into());
            }
            if tx.deletions.contains(&file_path) {
                anyhow::bail!("file was deleted earlier in this transaction: {path}");
            }
            if !file_path.exists() && !tx.pending.contains_key(&file_path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file does not exist: {path}"),
                )
                .into());
            }
            let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let combined = crate::ops::file::append_content(existing, content);
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &file_path,
                combined,
            );
        }

        Operation::FilePrepend { path, content } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {path}"),
                }
                .into());
            }
            if tx.deletions.contains(&file_path) {
                anyhow::bail!("file was deleted earlier in this transaction: {path}");
            }
            if !file_path.exists() && !tx.pending.contains_key(&file_path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file does not exist: {path}"),
                )
                .into());
            }
            let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let combined = crate::ops::file::prepend_content(existing, content);
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &file_path,
                combined,
            );
        }

        Operation::FileCreate {
            path,
            content,
            force,
        } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {path}"),
                }
                .into());
            }
            if force.unwrap_or(false) {
                if tx.pending.contains_key(&file_path) || file_path.exists() {
                    let _ = read_file_content(tx.pending, tx.existed_before, &file_path)?;
                }
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &file_path,
                    content.clone(),
                );
            } else {
                let exists_in_tx =
                    tx.pending.contains_key(&file_path) && !tx.deletions.contains(&file_path);
                if exists_in_tx || (!tx.deletions.contains(&file_path) && file_path.exists()) {
                    return Err(crate::exit::AlreadyExistsError {
                        msg: format!("file already exists: {path}"),
                    }
                    .into());
                }
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &file_path,
                    content.clone(),
                );
            }
        }

        Operation::FileDelete { path } => {
            let file_path = tx.cwd.join(path);
            if file_path.exists() && !file_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {path}"),
                }
                .into());
            }
            let created_in_tx = match tx.pending.get(&file_path) {
                Some((original, _)) => original.is_empty() && !file_path.exists(),
                None => {
                    if !file_path.exists() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("file not found: {path}"),
                        )
                        .into());
                    }
                    tx.existed_before.insert(file_path.clone());
                    // Try to read as text for strict rollback; fall back to
                    // empty for binary files that cannot be represented as
                    // UTF-8 (#1163).
                    match std::fs::read_to_string(&file_path) {
                        Ok(content) => {
                            tx.pending
                                .insert(file_path.clone(), (content.clone(), content));
                        }
                        Err(_) => {
                            tx.pending
                                .insert(file_path.clone(), (String::new(), String::new()));
                        }
                    }
                    false
                }
            };

            if created_in_tx {
                tx.pending.remove(&file_path);
                tx.deletions.remove(&file_path);
            } else {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &file_path,
                    String::new(),
                );
                tx.deletions.insert(file_path);
            }
        }

        Operation::FileRename { from, to, force } => {
            let src_path = tx.cwd.join(from);
            let dst_path = tx.cwd.join(to);

            if tx.deletions.contains(&src_path) {
                anyhow::bail!("source file was deleted earlier in this transaction: {from}");
            }
            if src_path.exists() && !src_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("source is not a file: {from}"),
                }
                .into());
            }
            if dst_path.exists() && !dst_path.is_file() {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("destination is not a file: {to}"),
                }
                .into());
            }

            // If source and destination resolve to the same file, no-op.
            // Allow case-only renames on case-insensitive filesystems (#1167).
            let case_only = src_path != dst_path
                && src_path.parent() == dst_path.parent()
                && src_path.file_name().map(|n| n.to_ascii_lowercase())
                    == dst_path.file_name().map(|n| n.to_ascii_lowercase());
            if !case_only
                && (src_path == dst_path
                    || matches!(
                        (src_path.canonicalize(), dst_path.canonicalize()),
                        (Ok(ref s), Ok(ref d)) if s == d
                    ))
            {
                return Ok(0);
            }

            // Read source content into pending (validates it exists).
            let content = read_file_content(tx.pending, tx.existed_before, &src_path)?.to_string();

            // Check destination does not already exist (unless force or
            // case-only rename on case-insensitive FS).
            if !force && !case_only {
                let dst_exists = (tx.pending.contains_key(&dst_path)
                    && !tx.deletions.contains(&dst_path))
                    || (!tx.deletions.contains(&dst_path) && dst_path.exists());
                if dst_exists {
                    return Err(crate::exit::AlreadyExistsError {
                        msg: format!("destination already exists: {to}"),
                    }
                    .into());
                }
            }

            // If destination exists on disk, load it into pending first so
            // existed_before is populated and commit uses atomic_write (not
            // atomic_create_new which would fail on existing files).
            if (*force || case_only) && !tx.pending.contains_key(&dst_path) && dst_path.exists() {
                let _ = read_file_content(tx.pending, tx.existed_before, &dst_path)?;
            }

            // Write content to destination.
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &dst_path,
                content,
            );

            // Delete source (same logic as file.delete for tx-created files).
            let created_in_tx = match tx.pending.get(&src_path) {
                Some((original, _)) => original.is_empty() && !src_path.exists(),
                None => false,
            };
            if created_in_tx {
                tx.pending.remove(&src_path);
                tx.deletions.remove(&src_path);
            } else {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &src_path,
                    String::new(),
                );
                tx.deletions.insert(src_path);
            }
        }

        _ => anyhow::bail!("execute_file_op called with non-file operation"),
    }
    Ok(0)
}
