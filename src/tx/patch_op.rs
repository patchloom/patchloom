use super::execute::{TxState, read_file_content};
use crate::ops::patch::{ApplyHunksOptions, apply_patch_with_loader};
use crate::plan::Operation;

/// Execute a patch operation within a transaction.
pub(crate) fn execute_patch_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::PatchApply {
            diff,
            on_stale,
            allow_conflicts,
        } => {
            let options = ApplyHunksOptions {
                on_stale: *on_stale,
                allow_conflicts: *allow_conflicts,
            };
            // Conflicts without allow_conflicts surface as ConflictsError from
            // apply_patch_with_loader (ops/patch early-err). When allow_conflicts
            // is true, content includes conflict markers and is staged normally.
            let patched_files = apply_patch_with_loader(
                diff,
                |path| {
                    let file_path = tx.cwd.join(path);
                    Ok(read_file_content(tx.pending, tx.existed_before, &file_path)?.to_string())
                },
                options,
            )?;
            for result in patched_files {
                if result.is_deletion {
                    let file_path = tx.cwd.join(&result.path);
                    // File deletion via patch: mark for deletion.
                    tx.deletions.insert(file_path.clone());
                    tx.write_targets.insert(file_path);
                } else if let Some(ref from) = result.rename_from {
                    // Git rename: load was from `from`; stage rename + new content (#2101).
                    let from_path = tx.cwd.join(from);
                    let to_path = tx.cwd.join(&result.path);
                    // Refuse overwrite of an existing dest (parity with file.rename
                    // without force). Case-only renames still allowed.
                    let dest_exists = (tx.pending.contains_key(&to_path)
                        && !tx.deletions.contains(&to_path))
                        || (!tx.deletions.contains(&to_path)
                            && crate::ops::file::path_entry_exists(&to_path));
                    if crate::ops::patch::rename_would_clobber_dest(from, &result.path, dest_exists)
                    {
                        return Err(crate::exit::AlreadyExistsError {
                            msg: crate::ops::patch::rename_dest_exists_msg(&result.path),
                        }
                        .into());
                    }
                    // Ensure source is in pending (loader already did); record rename
                    // so commit uses fs::rename then write dest content.
                    if !tx.existed_before.contains(&from_path)
                        && crate::ops::file::path_entry_exists(&from_path)
                    {
                        tx.existed_before.insert(from_path.clone());
                    }
                    if crate::ops::file::path_entry_exists(&to_path)
                        && !tx.existed_before.contains(&to_path)
                    {
                        // Dest exists only for case-only renames after the check above.
                        let _ = read_file_content(tx.pending, tx.existed_before, &to_path)?;
                    }
                    tx.renames.push((from_path.clone(), to_path.clone()));
                    tx.write_file(&to_path, result.content);
                    // Stage source delete (same as file.rename).
                    if let Some((original, _)) = tx.pending.get(&from_path) {
                        let orig = original.clone();
                        tx.pending.insert(from_path.clone(), (orig, String::new()));
                    } else {
                        tx.pending
                            .insert(from_path.clone(), (String::new(), String::new()));
                    }
                    tx.deletions.insert(from_path);
                } else {
                    let file_path = tx.cwd.join(&result.path);
                    let dest_exists = (tx.pending.contains_key(&file_path)
                        && !tx.deletions.contains(&file_path))
                        || (!tx.deletions.contains(&file_path)
                            && crate::ops::file::path_entry_exists(&file_path));
                    if result.copy_from.is_some() && dest_exists {
                        return Err(crate::exit::AlreadyExistsError {
                            msg: crate::ops::patch::copy_dest_exists_msg(&result.path),
                        }
                        .into());
                    }
                    if result.is_creation
                        && result.copy_from.is_none()
                        && result.content.is_empty()
                        && dest_exists
                    {
                        return Err(crate::exit::AlreadyExistsError {
                            msg: crate::ops::patch::create_dest_exists_msg(&result.path),
                        }
                        .into());
                    }
                    tx.write_file(&file_path, result.content);
                }
            }
            Ok(0)
        }

        _ => unreachable!("execute_patch_op called with non-Patch operation"),
    }
}
