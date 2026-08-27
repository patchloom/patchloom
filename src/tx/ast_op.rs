use super::execute::{TxState, read_and_probe, read_file_content};
use crate::plan::Operation;

use std::path::{Path, PathBuf};

/// Walk a directory and collect files that have a tree-sitter grammar.
/// Used by ast.rename/ast.replace in execute_plan when the path is a directory.
///
/// Honors `.gitignore` / standard ignore filters and does **not** follow
/// symlinks (avoids vendor trees and symlink cycles; #2102). Parity with CLI
/// `ast rename` / MCP collection via WalkBuilder.
fn collect_ast_source_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    #[cfg(any(feature = "cli", feature = "files"))]
    {
        use ignore::WalkBuilder;
        let mut files = Vec::new();
        // Match CLI/library walks: never enter .git / .patchloom (even with
        // hidden=false so other dotfiles can still be considered).
        let walker = WalkBuilder::new(dir)
            .follow_links(false)
            .standard_filters(true)
            .hidden(false)
            .filter_entry(|e| !crate::files::should_skip_walk_dirname(e.file_name()))
            .build();
        for entry in walker {
            let entry = entry.map_err(|e| {
                anyhow::anyhow!("ast directory walk failed under {}: {e}", dir.display())
            })?;
            let path = entry.path();
            if path.is_file() && crate::ast::Language::from_path(path).has_grammar() {
                files.push(path.to_path_buf());
            }
        }
        files.sort();
        Ok(files)
    }
    #[cfg(not(any(feature = "cli", feature = "files")))]
    {
        // ast-only builds: no ignore crate. Still refuse symlink dirs.
        let mut files = Vec::new();
        collect_ast_source_files_simple(dir, &mut files)?;
        files.sort();
        Ok(files)
    }
}

#[cfg(not(any(feature = "cli", feature = "files")))]
fn collect_ast_source_files_simple(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" || name == ".patchloom" {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect_ast_source_files_simple(&path, out)?;
        } else if ft.is_file() && crate::ast::Language::from_path(&path).has_grammar() {
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
    old: &str,
    new: &str,
    lang_hint: Option<&str>,
) -> anyhow::Result<usize> {
    let content = read_file_content(tx.pending, tx.existed_before, abs)?;
    let lang_val = lang_hint
        .map(crate::ast::Language::from_name_or_ext)
        .unwrap_or_else(|| crate::ast::Language::from_path(abs));
    let result = crate::ast::rename::rename_in_source(content, old, new, lang_val);
    match result {
        Some(r) if r.replacements > 0 => {
            tx.write_file(abs, r.content);
            Ok(r.replacements)
        }
        Some(_) => Err(crate::exit::NoMatchError {
            msg: format!("no matches for '{}' in {}", old, abs.display()),
        }
        .into()),
        None => {
            // Tree-sitter couldn't parse. Fallback to word-boundary replace.
            let re = crate::ops::replace::compile_replace_regex(old, false, false, false, true)?;
            if let Some(re) = re {
                let new_content = re.replace_all(content, new).to_string();
                let count = re.find_iter(content).count();
                if count > 0 {
                    tx.write_file(abs, new_content);
                    return Ok(count);
                }
            }
            Err(crate::exit::NoMatchError {
                msg: format!("no matches for '{}' in {}", old, abs.display()),
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
                    // Match CLI ast rename: empty dir is no_matches (exit 3), not
                    // generic operation_failed (9).
                    return Err(crate::exit::NoMatchError {
                        msg: format!("no source files found in {path}"),
                    }
                    .into());
                }
                let mut total = 0usize;
                for file_path in &files {
                    // Expanded paths must pass PathGuard (MCP list/rename do;
                    // plan/tx must not skip expanded children).
                    if let Some(g) = tx.guard {
                        let rel = file_path
                            .strip_prefix(tx.cwd)
                            .unwrap_or(file_path.as_path())
                            .to_string_lossy();
                        g.check_path(rel.as_ref())
                            .map_err(crate::fallback::EditError::guard_rejected)?;
                    }
                    match ast_rename_single_file(tx, file_path, old, new, lang.as_deref()) {
                        Ok(n) => total = total.saturating_add(n),
                        // Soft miss in one file does not abort the directory walk.
                        Err(e) if crate::exit::is_no_match(&e) => {}
                        // Hard failures (binary, guard, IO) must not soft-succeed.
                        Err(e) => return Err(e),
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
                    tx.write_file(&abs, r.content);
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

        Operation::AstRewriteSignature {
            path,
            old,
            new_signature,
            visibility,
            parameters,
            return_type,
            lang,
        } => {
            let abs = tx.cwd.join(path);
            let content = read_file_content(tx.pending, tx.existed_before, &abs)?;
            let lang_val = lang
                .as_deref()
                .map(crate::ast::Language::from_name_or_ext)
                .unwrap_or_else(|| crate::ast::Language::from_path(&abs));
            let has_structured =
                visibility.is_some() || parameters.is_some() || return_type.is_some();
            if new_signature.is_none() && !has_structured {
                return Err(crate::exit::InvalidInputError {
                    msg: "ast.rewrite_signature requires new_signature and/or visibility/parameters/return_type".into(),
                }
                .into());
            }
            let new_content = if let Some(new_sig) = new_signature {
                let span = crate::ast::rewrite::find_function_span(content, old, lang_val)
                    .ok_or_else(|| crate::exit::NoMatchError {
                        msg: format!("function '{}' not found in {}", old, path),
                    })?;
                // Preserve original gap before `{` (or insert space if glued). #1503
                crate::ast::rewrite::splice_function_signature(
                    content,
                    span.signature_range,
                    new_sig,
                )
            } else {
                let edit = crate::ast::rewrite::FunctionSigEdit {
                    visibility: visibility.clone(),
                    parameters: parameters.clone(),
                    return_type: return_type.clone(),
                };
                crate::ast::rewrite::rewrite_function_signature(content, old, &edit, lang_val)
                    .ok_or_else(|| crate::exit::NoMatchError {
                        msg: format!("function '{}' not found in {}", old, path),
                    })?
            };
            if new_content == content {
                return Ok(0);
            }
            tx.write_file(&abs, new_content);
            Ok(1)
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
                None | Some("") | Some("end") => crate::ast::insert::InsertPosition::End,
                Some("start") => crate::ast::insert::InsertPosition::Start,
                Some(other) => {
                    return Err(crate::exit::InvalidInputError {
                        msg: format!("unknown ast.insert position {other:?}; use start or end"),
                    }
                    .into());
                }
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
            tx.write_file(&abs, result.content);
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
            tx.write_file(&abs, result.content);
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
                tx.write_file(&abs, file_content);
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
                tx.write_file(&abs, result.content);
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
            let pos = crate::ast::group::parse_group_position(position.as_deref())?;
            let spec = crate::ast::group::GroupSpec {
                module: module.clone(),
                symbols: sym_names.clone(),
                preamble: preamble.clone(),
                position: pos,
            };
            let result = crate::ast::group::group_symbols(file_content, &spec, lang_val)?;
            if result.symbols_moved > 0 {
                tx.write_file(&abs, result.content);
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
            update_imports,
            old_module_path,
            new_module_path,
        } => {
            let rewrite_mods =
                import_rewrite_modules(*update_imports, old_module_path, new_module_path)?;
            let abs_source = tx.cwd.join(path);
            let abs_target = tx.cwd.join(target);
            let source_content =
                read_file_content(tx.pending, tx.existed_before, &abs_source)?.to_string();
            let target_content = if tx.path_present_for_if_exists(&abs_target) {
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
            let pos = crate::ast::move_symbols::parse_position(position.as_deref())?;
            let result = crate::ast::move_symbols::move_symbols(
                &source_content,
                &target_content,
                sym_names,
                pos,
                lang_val,
            )?;
            tx.write_file(&abs_source, result.source_content);
            tx.write_file(&abs_target, result.target_content);
            if let Some((old, new)) = rewrite_mods {
                apply_consumer_import_rewrites(
                    tx,
                    &abs_source,
                    &abs_target,
                    lang_val,
                    &symbol_moves(sym_names, &old, &new),
                )?;
            }
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
            update_imports,
            old_module_path,
            new_module_path,
        } => {
            let rewrite_mods =
                import_rewrite_modules(*update_imports, old_module_path, new_module_path)?;
            let abs_source = tx.cwd.join(source);
            let abs_target = tx.cwd.join(target);
            if !force && tx.path_present_for_if_exists(&abs_target) {
                return Err(crate::exit::AlreadyExistsError {
                    msg: format!(
                        "target file '{target}' already exists (use force: true to overwrite)"
                    ),
                }
                .into());
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
            tx.write_file(&abs_source, result.source_content);
            tx.write_file(&abs_target, result.target_content);
            if let Some((old, new)) = rewrite_mods {
                apply_consumer_import_rewrites(
                    tx,
                    &abs_source,
                    &abs_target,
                    lang_val,
                    &symbol_moves(std::slice::from_ref(symbol), &old, &new),
                )?;
            }
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
            tx.write_file(&abs_source, result.source_content);
            for (target_path, target_content) in &result.targets {
                let abs_target = tx.cwd.join(target_path);
                tx.write_file(&abs_target, target_content.clone());
            }
            Ok(result.symbols_distributed)
        }

        _ => unreachable!("execute_ast_op called with non-AST operation"),
    }
}

fn import_rewrite_modules(
    update_imports: bool,
    old_module_path: &Option<String>,
    new_module_path: &Option<String>,
) -> anyhow::Result<Option<(String, String)>> {
    if !update_imports {
        return Ok(None);
    }
    let old = old_module_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let new = new_module_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (old, new) {
        (Some(old), Some(new)) => Ok(Some((old.to_string(), new.to_string()))),
        _ => Err(crate::exit::InvalidInputError {
            msg: "update_imports requires old_module_path and new_module_path".into(),
        }
        .into()),
    }
}

fn symbol_moves(
    symbols: &[String],
    old: &str,
    new: &str,
) -> Vec<crate::ast::import_rewrite::SymbolMove> {
    symbols
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|name| crate::ast::import_rewrite::SymbolMove {
            name: name.to_string(),
            old_module: old.to_string(),
            new_module: new.to_string(),
        })
        .collect()
}

/// After a successful move/extract, optionally rewrite consumer imports.
fn apply_consumer_import_rewrites(
    tx: &mut TxState<'_>,
    source: &Path,
    target: &Path,
    lang: crate::ast::Language,
    moves: &[crate::ast::import_rewrite::SymbolMove],
) -> anyhow::Result<()> {
    if moves.is_empty() {
        return Ok(());
    }

    let mut files = collect_ast_source_files(tx.cwd)?;
    // Same-plan creates exist only in pending until commit.
    for pending in tx.pending.keys() {
        if crate::ast::Language::from_path(pending) == lang && !files.iter().any(|f| f == pending) {
            files.push(pending.clone());
        }
    }
    files.sort();
    for file in files {
        if paths_equal(&file, source) || paths_equal(&file, target) {
            continue;
        }
        if tx.deletions.contains(&file) {
            continue;
        }
        if crate::ast::Language::from_path(&file) != lang {
            continue;
        }
        if is_skipped_consumer_path(&file, tx.cwd) {
            continue;
        }
        if let Some(g) = tx.guard {
            let rel = file.strip_prefix(tx.cwd).unwrap_or(file.as_path());
            g.check_path(&rel.to_string_lossy())
                .map_err(crate::fallback::EditError::guard_rejected)?;
        }
        if !read_and_probe(tx.pending, tx.existed_before, &file)? {
            continue;
        }
        let content = read_file_content(tx.pending, tx.existed_before, &file)?.to_string();
        if let Some(rewritten) =
            crate::ast::import_rewrite::rewrite_imports_in_source(&content, lang, moves)
        {
            tx.write_file(&file, rewritten);
        }
    }
    Ok(())
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn is_skipped_consumer_path(path: &Path, cwd: &Path) -> bool {
    let rel = path.strip_prefix(cwd).unwrap_or(path);
    rel.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("target" | "node_modules" | ".git" | ".patchloom")
        )
    })
}

#[cfg(all(test, any(feature = "cli", feature = "files")))]
mod tests {
    use super::collect_ast_source_files;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn collect_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        // ignore::WalkBuilder applies .gitignore when a git work tree exists.
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .expect("git init");
        fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(dir.path().join("kept.rs"), "fn kept() {}\n").unwrap();
        fs::write(dir.path().join("ignored.rs"), "fn ignored() {}\n").unwrap();
        let files = collect_ast_source_files(dir.path()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert!(names.contains(&"kept.rs".to_string()), "got {names:?}");
        assert!(
            !names.contains(&"ignored.rs".to_string()),
            "gitignore must exclude ignored.rs: {names:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn collect_does_not_follow_symlink_dirs() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new().unwrap();
        // Target lives *outside* the walk root so only a symlink would expose it.
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("inside.rs"), "fn inside() {}\n").unwrap();
        let link = dir.path().join("linkdir");
        symlink(outside.path(), &link).unwrap();
        fs::write(dir.path().join("root.rs"), "fn root() {}\n").unwrap();
        let files = collect_ast_source_files(dir.path()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert!(names.contains(&"root.rs".to_string()), "got {names:?}");
        // WalkBuilder follow_links(false): symlink dir not entered
        assert!(
            !names.contains(&"inside.rs".to_string()),
            "must not follow symlink dir: {names:?}"
        );
    }

    #[test]
    fn collect_skips_git_and_patchloom_dirs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/objects/fake.rs"), "fn fake() {}\n").unwrap();
        fs::create_dir_all(dir.path().join(".patchloom/backups")).unwrap();
        fs::write(
            dir.path().join(".patchloom/backups/also.rs"),
            "fn also() {}\n",
        )
        .unwrap();
        fs::write(dir.path().join("root.rs"), "fn root() {}\n").unwrap();
        let files = collect_ast_source_files(dir.path()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(names, vec!["root.rs".to_string()], "got {names:?}");
    }
}
