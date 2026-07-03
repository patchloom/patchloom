use super::execute::{TxState, read_file_content, update_file_content};
use crate::ops::patch::{ApplyHunksOptions, ApplyHunksStatus, apply_patch_with_loader};
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
            let patched_files = apply_patch_with_loader(
                diff,
                |path| {
                    let file_path = tx.cwd.join(path);
                    Ok(read_file_content(tx.pending, tx.existed_before, &file_path)?.to_string())
                },
                options,
            )?;
            for result in patched_files {
                if result.status == ApplyHunksStatus::Conflict && !allow_conflicts {
                    anyhow::bail!(
                        "patch apply: {} -- merge produced {} conflict(s); set allow_conflicts to write conflict markers",
                        result.path,
                        result.conflicts.len()
                    );
                }
                let file_path = tx.cwd.join(&result.path);
                if result.is_deletion {
                    // File deletion via patch: mark for deletion.
                    tx.deletions.insert(file_path.clone());
                    tx.write_targets.insert(file_path);
                } else {
                    update_file_content(
                        tx.pending,
                        tx.deletions,
                        tx.write_targets,
                        &file_path,
                        result.content,
                    );
                }
            }
            Ok(0)
        }

        _ => unreachable!("execute_patch_op called with non-Patch operation"),
    }
}
