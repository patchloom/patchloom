//! `notebook.edit` for the tx engine.
use super::{TxState, read_file_content};

pub(super) fn execute(
    path: &str,
    cell_id: &str,
    source: &str,
    tx: &mut TxState<'_>,
) -> anyhow::Result<usize> {
    let file_path = tx.cwd.join(path);
    let existing = read_file_content(tx.pending, tx.existed_before, &file_path)?;
    let updated = crate::ops::notebook::replace_cell_source(existing, cell_id, source)?;
    if updated == existing {
        return Ok(0);
    }
    tx.write_file(&file_path, updated);
    Ok(1)
}
