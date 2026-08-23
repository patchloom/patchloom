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
            if crate::ops::begin_patch::looks_like_begin_patch(diff) {
                return execute_begin_patch(diff, tx);
            }
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
                    if let Some(msg) = crate::ops::patch::dest_clobber_msg(
                        &result.path,
                        dest_exists(tx, &to_path),
                        Some(from.as_str()),
                        false,
                        false,
                    ) {
                        return Err(crate::exit::AlreadyExistsError { msg }.into());
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
                    if let Some(msg) = crate::ops::patch::dest_clobber_msg(
                        &result.path,
                        dest_exists(tx, &file_path),
                        None,
                        result.copy_from.is_some(),
                        result.is_creation
                            && result.copy_from.is_none()
                            && result.content.is_empty(),
                    ) {
                        return Err(crate::exit::AlreadyExistsError { msg }.into());
                    }
                    tx.write_file(&file_path, result.content);
                }
            }
            Ok(0)
        }

        _ => unreachable!("execute_patch_op called with non-Patch operation"),
    }
}

fn execute_begin_patch(diff: &str, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    use crate::ops::begin_patch::{BeginPatchOp, apply_codex_hunks, parse_begin_patch};

    let ops = parse_begin_patch(diff)?;
    for op in ops {
        match op {
            BeginPatchOp::Add { path, content } => {
                let dest = tx.cwd.join(&path);
                if dest.is_dir() {
                    return Err(crate::exit::InvalidInputError {
                        msg: format!(
                            "{} is a directory; Begin Patch Add File needs a file path",
                            dest.display()
                        ),
                    }
                    .into());
                }
                tx.write_file(&dest, content);
            }
            BeginPatchOp::Delete { path } => {
                let dest = tx.cwd.join(&path);
                if dest.is_dir() {
                    return Err(crate::exit::InvalidInputError {
                        msg: format!("{} is a directory, not a file", dest.display()),
                    }
                    .into());
                }
                if !crate::ops::file::path_entry_exists(&dest) && !tx.pending.contains_key(&dest) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file not found: {path}"),
                    )
                    .into());
                }
                let _ = read_file_content(tx.pending, tx.existed_before, &dest)?;
                tx.deletions.insert(dest.clone());
                tx.write_targets.insert(dest);
            }
            BeginPatchOp::Update {
                path,
                hunks,
                move_to,
            } => {
                let dest = tx.cwd.join(&path);
                if dest.is_dir() {
                    return Err(crate::exit::InvalidInputError {
                        msg: format!(
                            "{} is a directory; Begin Patch Update File needs a file path",
                            dest.display()
                        ),
                    }
                    .into());
                }
                let original = read_file_content(tx.pending, tx.existed_before, &dest)?.to_string();
                let updated = apply_codex_hunks(&original, &hunks)?;
                if let Some(new_path) = move_to {
                    let new_dest = tx.cwd.join(&new_path);
                    if dest_exists(tx, &new_dest) {
                        return Err(crate::exit::AlreadyExistsError {
                            msg: format!(
                                "destination already exists: {} (Begin Patch Move refuses overwrite; remove dest)",
                                new_dest.display()
                            ),
                        }
                        .into());
                    }
                    tx.renames.push((dest.clone(), new_dest.clone()));
                    tx.write_file(&new_dest, updated);
                    if let Some((orig, _)) = tx.pending.get(&dest) {
                        let orig = orig.clone();
                        tx.pending.insert(dest.clone(), (orig, String::new()));
                    } else {
                        tx.pending
                            .insert(dest.clone(), (String::new(), String::new()));
                    }
                    tx.deletions.insert(dest);
                } else {
                    tx.write_file(&dest, updated);
                }
            }
        }
    }
    Ok(0)
}

fn dest_exists(tx: &TxState<'_>, path: &std::path::Path) -> bool {
    !tx.deletions.contains(path)
        && (tx.pending.contains_key(path) || crate::ops::file::path_entry_exists(path))
}
