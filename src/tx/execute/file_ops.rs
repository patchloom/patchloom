//! File create/append/prepend/delete/rename for the tx engine.
use super::{
    TxState, read_file_content, read_file_content_for_force_create, read_file_content_for_path_op,
};
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
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file was deleted earlier in this transaction: {path}"),
                )
                .into());
            }
            if !file_path.exists() && !tx.pending.contains_key(&file_path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file does not exist: {path}"),
                )
                .into());
            }
            // On-disk binary only (in-tx text create then append is fine).
            if !tx.pending.contains_key(&file_path) {
                crate::ops::file::ensure_not_binary_file(&file_path, path)?;
            }
            let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let combined = crate::ops::file::append_content(existing, content);
            tx.write_file(&file_path, combined);
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
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file was deleted earlier in this transaction: {path}"),
                )
                .into());
            }
            if !file_path.exists() && !tx.pending.contains_key(&file_path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file does not exist: {path}"),
                )
                .into());
            }
            if !tx.pending.contains_key(&file_path) {
                crate::ops::file::ensure_not_binary_file(&file_path, path)?;
            }
            let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
            let combined = crate::ops::file::prepend_content(existing, content);
            tx.write_file(&file_path, combined);
        }

        Operation::FileCreate {
            path,
            content,
            force,
        } => {
            let file_path = tx.cwd.join(path);
            // Dangling symlinks are present entries (Path::exists is false).
            // Use classify/path_entry_exists so create matches delete/rename
            // and preview/apply already_exists stay dual-path honest.
            use crate::ops::file::{PathEntryKind, classify_path_entry, path_entry_exists};
            let entry = classify_path_entry(&file_path);
            if entry == PathEntryKind::RealDirectory {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("target is not a file: {path}"),
                }
                .into());
            }
            // Reject ancestors that exist as files before staging so commit
            // does not leave a bare tempfile error or a backup for an unwritten path.
            crate::ops::file::ensure_parent_components_are_directories(&file_path)?;
            if force.unwrap_or(false) {
                // Force overwrite: soft-load prior text for backup/diff. Binary,
                // invalid UTF-8, or unreadable prior → empty original so hosts
                // need no remove+recreate loop (#1962). PathGuard still applies
                // at plan entry; hardlink-preserving write stays on commit path.
                // path_entry_exists covers dangling symlink entries.
                if tx.pending.contains_key(&file_path) || path_entry_exists(&file_path) {
                    let _ = read_file_content_for_force_create(
                        tx.pending,
                        tx.existed_before,
                        &file_path,
                    )?;
                }
                tx.write_file(&file_path, content.clone());
            } else {
                let exists_in_tx =
                    tx.pending.contains_key(&file_path) && !tx.deletions.contains(&file_path);
                if exists_in_tx
                    || (!tx.deletions.contains(&file_path) && path_entry_exists(&file_path))
                {
                    return Err(crate::exit::AlreadyExistsError {
                        msg: format!("file already exists: {path} (use force to overwrite)"),
                    }
                    .into());
                }
                tx.write_file(&file_path, content.clone());
            }
        }

        Operation::FileDelete { path } => {
            let file_path = tx.cwd.join(path);
            // Allow regular files, symlinks (unlink only), FIFO/socket/device.
            // Refuse real directories only (#2087). Use path_entry_exists so
            // dangling symlinks are still unlinkable (Path::exists follows).
            if crate::ops::file::path_entry_exists(&file_path) {
                crate::ops::file::ensure_unlinkable_not_directory(&file_path, path)?;
            }
            let created_in_tx = match tx.pending.get(&file_path) {
                Some((original, _)) => {
                    original.is_empty() && !crate::ops::file::path_entry_exists(&file_path)
                }
                None => {
                    if !crate::ops::file::path_entry_exists(&file_path) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("file not found: {path}"),
                        )
                        .into());
                    }
                    tx.existed_before.insert(file_path.clone());
                    // Soft text load for rollback snapshot: binary / invalid
                    // UTF-8 / unreadable / special node → empty pair
                    // (#1163 / #1894 SoftSkip / #2087).
                    if crate::ops::file::is_regular_file_for_backup(&file_path)
                        && let Some(content) = crate::files::read_text_file(&file_path)
                    {
                        tx.pending
                            .insert(file_path.clone(), (content.clone(), content));
                    } else {
                        tx.pending
                            .insert(file_path.clone(), (String::new(), String::new()));
                    }
                    false
                }
            };

            if created_in_tx {
                tx.pending.remove(&file_path);
                tx.deletions.remove(&file_path);
            } else {
                tx.write_file(&file_path, String::new());
                tx.deletions.insert(file_path.clone());
            }
            // Deleting a rename dest must drop the pair so commit does not
            // fs::rename the source back into existence.
            tx.renames.retain(|(_, to)| to != &file_path);
        }

        Operation::FileRename { from, to, force } => {
            let src_path = tx.cwd.join(from);
            let dst_path = tx.cwd.join(to);

            if tx.deletions.contains(&src_path) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("source file was deleted earlier in this transaction: {from}"),
                )
                .into());
            }
            // Special nodes (symlinks, FIFO, …) are renameable; only real
            // directories are refused. One classify per path (#2087 / #2091).
            // Keep "source"/"destination" wording (not generic "target") for
            // agent/CLI error matching.
            use crate::ops::file::{PathEntryKind, classify_path_entry};
            let src_kind = classify_path_entry(&src_path);
            let dst_kind = classify_path_entry(&dst_path);
            if src_kind == PathEntryKind::RealDirectory {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("source is not a file: {from}"),
                }
                .into());
            }
            if dst_kind == PathEntryKind::RealDirectory {
                return Err(crate::exit::InvalidInputError {
                    msg: format!("destination is not a file: {to}"),
                }
                .into());
            }
            crate::ops::file::ensure_parent_components_are_directories(&dst_path)?;

            // If source and destination resolve to the same file, no-op.
            // Allow case-only renames on case-insensitive filesystems (#1167).
            let case_only = src_path != dst_path
                && src_path.parent() == dst_path.parent()
                && src_path.file_name().map(|n| n.to_ascii_lowercase())
                    == dst_path.file_name().map(|n| n.to_ascii_lowercase());
            if !case_only
                && (src_path == dst_path
                    || matches!(
                        (
                            crate::containment::safe_canonicalize(&src_path),
                            crate::containment::safe_canonicalize(&dst_path)
                        ),
                        (Ok(ref s), Ok(ref d)) if s == d
                    ))
            {
                return Ok(0);
            }

            // Read source into pending (validates it exists). Soft-load binary /
            // invalid UTF-8 as empty text snapshot (#2031): path rename is
            // recorded in `tx.renames` so commit uses `fs::rename` (bytes stay
            // intact). Hard fail only for missing / not-a-file (above) and
            // unreadable IO that is not a content SoftSkip.
            let content = read_file_content_for_path_op(tx.pending, tx.existed_before, &src_path)?
                .to_string();

            // Check destination does not already exist (unless force or
            // case-only rename on case-insensitive FS).
            if !force && !case_only {
                let dst_exists = (tx.pending.contains_key(&dst_path)
                    && !tx.deletions.contains(&dst_path))
                    || (!tx.deletions.contains(&dst_path) && dst_kind.exists());
                if dst_exists {
                    return Err(crate::exit::AlreadyExistsError {
                        msg: format!("destination already exists: {to} (use force to overwrite)"),
                    }
                    .into());
                }
            }

            // If destination exists on disk, load it into pending first so
            // existed_before is populated and commit uses atomic_write (not
            // atomic_create_new which would fail on existing files). Soft-load
            // non-text / special-node dest the same way as source (#2031 / #2091).
            if (*force || case_only) && !tx.pending.contains_key(&dst_path) && dst_kind.exists() {
                let _ = read_file_content_for_path_op(tx.pending, tx.existed_before, &dst_path)?;
            }

            // Record renames so commit can use fs::rename and preserve hardlinks
            // even when a later op edits the dest (#1739 follow-up:
            // rename-then-replace). Also chain renames where the source is only
            // a prior rename dest in this plan (a->c then c->d; c is not on disk
            // yet during staging). file.create-then-rename stays content-staged
            // (source neither on disk nor a rename dest).
            //
            // Force overwrite (dest already exists) must also record the pair:
            // commit's rename_or_copy replaces the dest entry while keeping the
            // source inode (and its hardlink siblings). Skipping the record falls
            // back to create-dest + delete-src and splits hardlinks (#1746).
            //
            // Case-only renames (readme.md → README.md) must also record the
            // pair. On case-insensitive FS, write-dest + delete-src removes the
            // only inode. CLI bypasses the engine (#1167); plan/MCP must not.
            // Dangling / special sources need the fs::rename record so commit
            // does not materialize empty files.
            let src_on_disk = src_kind.exists();
            let src_is_rename_dest = tx.renames.iter().any(|(_, to)| to == &src_path);
            let dst_on_disk = dst_kind.exists();
            if case_only {
                if src_on_disk || src_is_rename_dest {
                    tx.renames.push((src_path.clone(), dst_path.clone()));
                }
            } else if (src_on_disk || src_is_rename_dest) && (*force || !dst_on_disk) {
                // Non-force with an on-disk dest is rejected earlier. Force may
                // overwrite; non-force only records when dest is not on disk.
                tx.renames.push((src_path.clone(), dst_path.clone()));
            }

            // Write content to destination.
            tx.write_file(&dst_path, content);

            // Delete source (same logic as file.delete for tx-created files).
            let created_in_tx = match tx.pending.get(&src_path) {
                Some((original, _)) => original.is_empty() && !src_kind.exists(),
                None => false,
            };
            if created_in_tx {
                tx.pending.remove(&src_path);
                tx.deletions.remove(&src_path);
            } else {
                tx.write_file(&src_path, String::new());
                tx.deletions.insert(src_path);
            }
        }

        _ => anyhow::bail!("execute_file_op called with non-file operation"),
    }
    Ok(0)
}
