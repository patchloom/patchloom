use super::execute::{TxState, read_file_content, update_file_content};
use crate::plan::Operation;

use std::path::{Path, PathBuf};

/// Walk a directory and collect files that have a tree-sitter grammar.
/// Used by ast.rename/ast.replace in execute_plan when the path is a directory.
fn collect_ast_source_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_ast_source_files_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_ast_source_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_ast_source_files_recursive(&path, out)?;
        } else if path.is_file() && crate::ast::Language::from_path(&path).has_grammar() {
            out.push(path);
        }
    }
    Ok(())
}

/// Rename identifiers in a single file using AST-aware renaming with
/// word-boundary fallback. Used by the AstRename handler (both single-file
/// and directory expansion paths).
fn ast_rename_single_file(
    tx: &mut TxState<'_>,
    abs: &Path,
    old_name: &str,
    new_name: &str,
    lang_hint: Option<&str>,
) -> anyhow::Result<usize> {
    let content = read_file_content(tx.pending, tx.existed_before, abs)?;
    let lang_val = lang_hint
        .map(crate::ast::Language::from_name_or_ext)
        .unwrap_or_else(|| crate::ast::Language::from_path(abs));
    let result = crate::ast::rename::rename_in_source(content, old_name, new_name, lang_val);
    match result {
        Some(r) if r.replacements > 0 => {
            update_file_content(tx.pending, tx.deletions, tx.write_targets, abs, r.content);
            Ok(r.replacements)
        }
        Some(_) => Err(crate::exit::NoMatchError {
            msg: format!("no matches for '{}' in {}", old_name, abs.display()),
        }
        .into()),
        None => {
            // Tree-sitter couldn't parse. Fallback to word-boundary replace.
            let re =
                crate::ops::replace::compile_replace_regex(old_name, false, false, false, true)?;
            if let Some(re) = re {
                let new_content = re.replace_all(content, new_name).to_string();
                let count = re.find_iter(content).count();
                if count > 0 {
                    update_file_content(
                        tx.pending,
                        tx.deletions,
                        tx.write_targets,
                        abs,
                        new_content,
                    );
                    return Ok(count);
                }
            }
            Err(crate::exit::NoMatchError {
                msg: format!("no matches for '{}' in {}", old_name, abs.display()),
            }
            .into())
        }
    }
}

/// Execute an AST operation within a transaction.
pub(crate) fn execute_ast_op(op: &Operation, tx: &mut TxState<'_>) -> anyhow::Result<usize> {
    match op {
        Operation::AstRename {
            path,
            old,
            new,
            lang,
        } => {
            let abs = tx.cwd.join(path);

            // Directory support (#1287): expand to individual files.
            if abs.is_dir() {
                let files = collect_ast_source_files(&abs)?;
                if files.is_empty() {
                    anyhow::bail!("no source files found in {}", path);
                }
                let mut total = 0usize;
                for file_path in &files {
                    let count = ast_rename_single_file(tx, file_path, old, new, lang.as_deref());
                    if let Ok(n) = count {
                        total += n;
                    }
                }
                if total == 0 {
                    return Err(crate::exit::NoMatchError {
                        msg: format!("no matches for '{}' in {}", old, path),
                    }
                    .into());
                }
                return Ok(total);
            }

            ast_rename_single_file(tx, &abs, old, new, lang.as_deref())
        }

        Operation::AstReplace {
            path,
            symbol,
            old,
            new_text,
            regex,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let result = crate::ast::replace::replace_in_symbol(
                content, symbol, old, new_text, *regex, lang_val,
            )?;
            match result {
                Some(r) if r.replacements > 0 => {
                    update_file_content(
                        tx.pending,
                        tx.deletions,
                        tx.write_targets,
                        &abs,
                        r.content,
                    );
                    Ok(r.replacements)
                }
                Some(_) => Err(crate::exit::NoMatchError {
                    msg: format!(
                        "no matches for '{}' in symbol '{}' in {}",
                        old, symbol, path
                    ),
                }
                .into()),
                None => Err(crate::exit::NoMatchError {
                    msg: format!("symbol '{}' not found in {}", symbol, path),
                }
                .into()),
            }
        }

        Operation::AstInsert {
            path,
            content: insert_content,
            inside,
            after,
            before,
            position,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let pos = match position.as_deref() {
                Some("start") => crate::ast::insert::InsertPosition::Start,
                _ => crate::ast::insert::InsertPosition::End,
            };
            let result = crate::ast::insert::insert_code(
                file_content,
                insert_content,
                inside.as_deref(),
                after.as_deref(),
                before.as_deref(),
                pos,
                lang_val,
            )?;
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs,
                result.content,
            );
            Ok(1)
        }

        Operation::AstWrap {
            path,
            symbols: sym_names,
            lines: line_range,
            wrapper,
            preamble,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let result = crate::ast::wrap::wrap_code(
                file_content,
                sym_names.as_deref(),
                line_range.as_deref(),
                wrapper,
                preamble.as_deref(),
                lang_val,
            )?;
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs,
                result.content,
            );
            Ok(1)
        }

        Operation::AstImports {
            path,
            add,
            remove,
            dedupe,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let mut file_content =
                read_file_content(tx.pending, tx.existed_before, &abs)?.to_string();
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let mut total_changes = 0usize;

            if let Some(add_list) = add {
                let result = crate::ast::imports::add_imports(&file_content, add_list, lang_val);
                total_changes += result.added;
                file_content = result.content;
            }
            if let Some(remove_list) = remove {
                let result =
                    crate::ast::imports::remove_imports(&file_content, remove_list, lang_val);
                total_changes += result.removed;
                file_content = result.content;
            }
            if *dedupe {
                let result = crate::ast::imports::dedupe_imports(&file_content, lang_val);
                total_changes += result.deduped;
                file_content = result.content;
            }
            if total_changes > 0 {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &abs,
                    file_content,
                );
            }
            Ok(total_changes)
        }

        Operation::AstReorder {
            path,
            inside,
            order,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let strategy = crate::ast::reorder::parse_strategy(order)?;
            let result = crate::ast::reorder::reorder_symbols(
                file_content,
                inside.as_deref(),
                &strategy,
                lang_val,
            )?;
            if result.symbols_reordered > 0 {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &abs,
                    result.content,
                );
            }
            Ok(result.symbols_reordered)
        }

        Operation::AstGroup {
            path,
            module,
            symbols: sym_names,
            preamble,
            position,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let file_content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let pos = crate::ast::group::parse_group_position(position.as_deref());
            let spec = crate::ast::group::GroupSpec {
                module: module.clone(),
                symbols: sym_names.clone(),
                preamble: preamble.clone(),
                position: pos,
            };
            let result = crate::ast::group::group_symbols(file_content, &spec, lang_val)?;
            if result.symbols_moved > 0 {
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &abs,
                    result.content,
                );
            }
            Ok(result.symbols_moved)
        }

        Operation::AstMove {
            path,
            target,
            symbols: sym_names,
            position,
            target_prepend,
            lang,
        } => {
            let abs_source = tx.cwd.join(path);
            let abs_target = tx.cwd.join(target);
            let source_content =
                read_file_content(tx.pending, tx.existed_before, &abs_source)?.to_string();
            let target_content = if abs_target.exists() || tx.pending.contains_key(&abs_target) {
                read_file_content(tx.pending, tx.existed_before, &abs_target)?.to_string()
            } else {
                target_prepend
                    .as_ref()
                    .map(|p| format!("{p}\n\n"))
                    .unwrap_or_default()
            };
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs_source));
            let pos = crate::ast::move_symbols::parse_position(position.as_deref());
            let result = crate::ast::move_symbols::move_symbols(
                &source_content,
                &target_content,
                sym_names,
                pos,
                lang_val,
            )?;
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs_source,
                result.source_content,
            );
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs_target,
                result.target_content,
            );
            Ok(result.symbols_moved)
        }

        Operation::AstExtractToFile {
            source,
            symbol,
            target,
            replacement,
            unwrap,
            prepend,
            force,
            lang,
        } => {
            let abs_source = tx.cwd.join(source);
            let abs_target = tx.cwd.join(target);
            if !force && (abs_target.exists() || tx.pending.contains_key(&abs_target)) {
                anyhow::bail!(
                    "target file '{}' already exists (use force: true to overwrite)",
                    target
                );
            }
            let source_content = read_file_content(tx.pending, tx.existed_before, &abs_source)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs_source));
            let do_unwrap = unwrap.unwrap_or(true);
            let result = crate::ast::extract_to_file::extract_to_file(
                source_content,
                symbol,
                replacement.as_deref(),
                do_unwrap,
                prepend.as_deref(),
                lang_val,
            )?;
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs_source,
                result.source_content,
            );
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs_target,
                result.target_content,
            );
            Ok(1)
        }

        Operation::AstSplit {
            source,
            targets,
            keep_in_source,
            source_suffix,
            source_prefix,
            require_exhaustive,
            lang,
        } => {
            let abs_source = tx.cwd.join(source);
            let source_content = read_file_content(tx.pending, tx.existed_before, &abs_source)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs_source));
            let split_targets: Vec<crate::ast::split::SplitTarget> = targets
                .iter()
                .map(|t| crate::ast::split::SplitTarget {
                    path: t.path.clone(),
                    symbols: t.symbols.clone(),
                    prepend: t.prepend.clone(),
                })
                .collect();
            let exhaustive = require_exhaustive.unwrap_or(true);
            let result = crate::ast::split::split_file(
                source_content,
                &split_targets,
                keep_in_source,
                source_suffix.as_deref(),
                source_prefix.as_deref(),
                exhaustive,
                lang_val,
            )?;
            update_file_content(
                tx.pending,
                tx.deletions,
                tx.write_targets,
                &abs_source,
                result.source_content,
            );
            for (target_path, target_content) in &result.targets {
                let abs_target = tx.cwd.join(target_path);
                update_file_content(
                    tx.pending,
                    tx.deletions,
                    tx.write_targets,
                    &abs_target,
                    target_content.clone(),
                );
            }
            Ok(result.symbols_distributed)
        }

        _ => unreachable!("execute_ast_op called with non-AST operation"),
    }
}
