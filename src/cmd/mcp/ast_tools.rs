//! AST MCP handler implementations, extracted from mod.rs (#928).
//!
//! Each `pub(super)` function contains the logic for one AST MCP tool.
//! The `#[tool]` stubs in `mod.rs` delegate directly to these functions.

use rmcp::model::{CallToolResult, Content, ErrorData as McpError};

use crate::cli::global::GlobalFlags;
use crate::exit;

use super::params::*;
use super::{PatchloomService, exit_code_to_result, validate_content_size, validate_param_size};

pub(super) fn handle_ast_list(
    svc: &PatchloomService,
    p: AstListParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);
    let kind_filter = crate::cmd::ast::parse_kind_filter(&p.kind);

    let mut results = Vec::new();

    if target.is_file() {
        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(&target));
        if !lang.has_grammar() {
            return exit_code_to_result(
                exit::NO_MATCHES,
                &format!(
                    "Unsupported language: {} (detected from {}). \
                     Supported: Rust, Python, TypeScript, JavaScript, Go, Java, \
                     C#, Ruby, PHP, Swift, Kotlin, C, C++, HCL, XML, Protobuf, \
                     TOML, YAML, JSON, Shell.",
                    lang, p.path,
                ),
                "",
            );
        }
        let symbols = crate::ast::symbols::extract_symbols_from_file(&target, Some(lang));
        let filtered = crate::cmd::ast::filter_symbols(&symbols, &kind_filter);
        if !filtered.is_empty() {
            for sym in &filtered {
                results.push(crate::cmd::ast::symbol_to_json(sym, &p.path));
            }
        }
    } else if target.is_dir() {
        let global = GlobalFlags::with_cwd(&cwd);
        let paths = crate::cmd::ast::collect_source_files(&target, &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        struct ListResult {
            entries: Vec<serde_json::Value>,
        }
        let par_results: Vec<ListResult> = crate::par_process_files(&paths, None, &[], |path| {
            let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
            let symbols = crate::ast::symbols::extract_symbols_from_file(path, Some(lang));
            let filtered = crate::cmd::ast::filter_symbols(&symbols, &kind_filter);
            if filtered.is_empty() {
                return None;
            }
            let display = crate::cmd::ast::display_path(path, &cwd);
            let entries = filtered
                .iter()
                .map(|sym| crate::cmd::ast::symbol_to_json(sym, &display))
                .collect();
            Some(ListResult { entries })
        });
        for r in par_results {
            results.extend(r.entries);
        }
    } else {
        return Err(McpError::invalid_params(
            format!("path not found: {}", p.path),
            None,
        ));
    }

    if results.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No symbols found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_read(
    svc: &PatchloomService,
    p: AstReadParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("symbol", &p.symbol)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);

    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);
    let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(&target));
    let source = std::fs::read_to_string(&target)
        .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
    let all_symbols = crate::ast::symbols::extract_symbols(&source, lang);
    let sym = crate::ast::symbols::find_symbol(&all_symbols, &p.symbol).ok_or_else(|| {
        McpError::invalid_params(
            format!("symbol '{}' not found in {}", p.symbol, p.path),
            None,
        )
    })?;

    let lines: Vec<&str> = source.lines().collect();
    let start = sym.start_line.saturating_sub(1 + p.context);
    let end = (sym.end_line + p.context).min(lines.len());
    let content: String = lines[start..end].iter().map(|l| format!("{l}\n")).collect();

    let obj = serde_json::json!({
        "file": p.path,
        "symbol": sym.name,
        "kind": sym.kind.to_string(),
        "start_line": sym.start_line,
        "end_line": sym.end_line,
        "signature": sym.signature,
        "content": content,
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_rename(
    svc: &PatchloomService,
    p: AstRenameParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("old_name", &p.old_name)?;
    validate_param_size("new_name", &p.new_name)?;
    if p.old_name == p.new_name {
        return exit_code_to_result(exit::NO_MATCHES, "", "old_name and new_name are identical.");
    }
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);

    let mut global = GlobalFlags::with_cwd_and_json(&cwd);
    global.apply = true;

    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    let mut total_replacements = 0usize;
    let mut files_changed = 0usize;
    let mut backup = crate::backup::BackupSession::new(&cwd)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

    for path in &paths {
        svc.check_path(
            &path
                .strip_prefix(&cwd)
                .unwrap_or(path)
                .display()
                .to_string(),
        )?;

        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
        let source = std::fs::read_to_string(path)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let result = if lang.has_grammar() {
            crate::ast::rename::rename_in_source(&source, &p.old_name, &p.new_name, lang)
        } else {
            None
        };

        let (new_content, count) = match result {
            Some(r) if r.replacements > 0 => (r.content, r.replacements),
            _ => {
                // Try word-boundary fallback
                let re = crate::ops::replace::compile_replace_regex(
                    &p.old_name,
                    false,
                    false,
                    false,
                    true,
                )
                .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
                if let Some(re) = re {
                    let count = re.find_iter(&source).count();
                    if count > 0 {
                        let new = re.replace_all(&source, p.new_name.as_str());
                        (new.into_owned(), count)
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
        };

        total_replacements += count;
        files_changed += 1;
        let policy = crate::write::policy_from_flags(&global, Some(path));
        backup
            .save_before_write(path)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        crate::write::atomic_write(path, &new_content, &policy)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    }

    backup
        .finalize()
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

    if total_replacements == 0 {
        return exit_code_to_result(exit::NO_MATCHES, "", "No matches found.");
    }

    let obj = serde_json::json!({
        "ok": true,
        "files_changed": files_changed,
        "replacements": total_replacements,
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_validate(
    svc: &PatchloomService,
    p: AstValidateParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    let results: Vec<serde_json::Value> = crate::par_process_files(&paths, None, &[], |path| {
        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
        if !lang.has_grammar() {
            return None;
        }
        let result = crate::ast::validate::validate_file(path, Some(lang)).ok()?;
        let display = crate::cmd::ast::display_path(path, &cwd);
        Some(serde_json::json!({
            "file": display,
            "valid": result.valid,
            "language": result.language,
            "errors": result.errors,
        }))
    });

    if results.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No files with grammars found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_search(
    svc: &PatchloomService,
    p: AstSearchParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("query", &p.query)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    struct SearchFileResult {
        entries: Vec<serde_json::Value>,
    }
    let par_results: Vec<SearchFileResult> = crate::par_process_files(&paths, None, &[], |path| {
        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
        let query_str = if p.pattern {
            crate::ast::search::compile_pattern_query(&p.query, lang).ok()?
        } else {
            p.query.clone()
        };
        let results =
            crate::ast::search::search_file(path, &query_str, Some(lang), p.max_results).ok()?;
        if results.is_empty() {
            return None;
        }
        let display = crate::cmd::ast::display_path(path, &cwd);
        let entries = results
            .iter()
            .map(|m| {
                serde_json::json!({
                    "file": display,
                    "line": m.line,
                    "column": m.column,
                    "text": m.text,
                    "captures": m.captures,
                })
            })
            .collect();
        Some(SearchFileResult { entries })
    });
    let all_matches: Vec<serde_json::Value> =
        par_results.into_iter().flat_map(|r| r.entries).collect();

    if all_matches.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No matches found.");
    }
    let json = serde_json::to_string_pretty(&all_matches)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_refs(
    svc: &PatchloomService,
    p: AstRefsParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("symbol", &p.symbol)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    let per_file: Vec<Vec<crate::ast::refs::SymbolRef>> =
        crate::par_process_files(&paths, None, &[], |path| {
            let display = crate::cmd::ast::display_path(path, &cwd);
            let refs = crate::ast::refs::find_refs_in_file(path, &p.symbol, lang_hint, &display);
            if refs.is_empty() { None } else { Some(refs) }
        });
    let mut all_refs: Vec<crate::ast::refs::SymbolRef> = per_file.into_iter().flatten().collect();

    if !p.include_def {
        all_refs.retain(|r| r.kind != crate::ast::refs::RefKind::Definition);
    }

    if all_refs.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No references found.");
    }

    let obj = serde_json::json!({
        "symbol": p.symbol,
        "references": all_refs,
        "count": all_refs.len(),
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_deps(
    svc: &PatchloomService,
    p: AstDepsParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    let mut results = Vec::new();

    if p.reverse {
        let target_name = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let scan_dir = if target.is_file() {
            target.parent().unwrap_or(&cwd).to_path_buf()
        } else {
            target.clone()
        };
        let all_files = crate::cmd::ast::collect_source_files(&scan_dir, &global)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        struct RevDepsResult {
            entries: Vec<serde_json::Value>,
        }
        let par_results: Vec<RevDepsResult> =
            crate::par_process_files(&all_files, None, &[], |path| {
                let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
                let matching: Vec<_> = imports
                    .iter()
                    .filter(|i| i.path.contains(target_name.as_str()))
                    .collect();
                if matching.is_empty() {
                    return None;
                }
                let display = crate::cmd::ast::display_path(path, &cwd);
                let entries = matching
                    .iter()
                    .map(|imp| {
                        serde_json::json!({
                            "file": display,
                            "imports": imp.path,
                            "line": imp.line,
                            "raw": imp.raw,
                        })
                    })
                    .collect();
                Some(RevDepsResult { entries })
            });
        for r in par_results {
            results.extend(r.entries);
        }
    } else {
        let par_results: Vec<serde_json::Value> =
            crate::par_process_files(&paths, None, &[], |path| {
                let imports = crate::ast::deps::extract_imports_from_file(path, lang_hint);
                if imports.is_empty() {
                    return None;
                }
                let display = crate::cmd::ast::display_path(path, &cwd);
                Some(serde_json::json!({
                    "file": display,
                    "imports": imports,
                }))
            });
        results.extend(par_results);
    }

    if results.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No imports found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_map(
    svc: &PatchloomService,
    p: AstMapParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    for s in &p.focus {
        validate_param_size("focus", s)?;
    }
    for s in &p.boost {
        validate_param_size("boost", s)?;
    }
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);

    if !target.is_dir() {
        return Err(McpError::invalid_params(
            format!("path must be a directory: {}", p.path),
            None,
        ));
    }

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::collect_source_files(&target, &global)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|fp| {
            let display = crate::cmd::ast::display_path(fp, &cwd);
            (fp.clone(), display)
        })
        .collect();

    let opts = crate::ast::map::MapOptions {
        max_tokens: p.max_tokens,
        focus: &p.focus,
        boost: &p.boost,
    };

    let entries = crate::ast::map::generate_map(&file_pairs, &opts);

    if entries.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No symbols found.");
    }

    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_diff(
    svc: &PatchloomService,
    p: AstDiffParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("from", &p.from)?;
    if let Some(ref to) = p.to {
        validate_param_size("to", to)?;
    }
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);
    let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(&target));

    let old_source = crate::cmd::ast::get_git_file_content(&cwd, &p.path, &p.from)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

    let new_source = if let Some(ref to_ref) = p.to {
        crate::cmd::ast::get_git_file_content(&cwd, &p.path, to_ref)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?
    } else {
        std::fs::read_to_string(&target)
            .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?
    };

    let changes = crate::ast::diff::structural_diff(&old_source, &new_source, lang);

    if changes.is_empty() {
        return exit_code_to_result(exit::NO_MATCHES, "", "No structural changes.");
    }

    let obj = serde_json::json!({
        "file": p.path,
        "from": p.from,
        "to": p.to.as_deref().unwrap_or("working tree"),
        "changes": changes,
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_impact(
    svc: &PatchloomService,
    p: AstImpactParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("symbol", &p.symbol)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);

    let global = GlobalFlags::with_cwd(&cwd);
    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    let file_pairs: Vec<(std::path::PathBuf, String)> = paths
        .iter()
        .map(|fp| {
            let display = crate::cmd::ast::display_path(fp, &cwd);
            (fp.clone(), display)
        })
        .collect();

    let nodes = crate::ast::impact::compute_impact(&p.symbol, &file_pairs, p.depth);

    if nodes.is_empty() {
        return exit_code_to_result(
            exit::NO_MATCHES,
            "",
            &format!("No references found for '{}'.", p.symbol),
        );
    }

    let obj = serde_json::json!({
        "symbol": p.symbol,
        "depth": p.depth,
        "impact": nodes,
        "direct_count": nodes.len(),
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

pub(super) fn handle_ast_replace(
    svc: &PatchloomService,
    p: AstReplaceParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("from", &p.from)?;
    validate_content_size("to", &p.to)?;
    validate_param_size("symbol", &p.symbol)?;
    let cwd = svc.cwd().to_path_buf();
    let target = cwd.join(&p.path);
    let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);
    let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(&target));

    let source = std::fs::read_to_string(&target)
        .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;

    let result =
        crate::ast::replace::replace_in_symbol(&source, &p.symbol, &p.from, &p.to, p.regex, lang)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

    let result = match result {
        Some(r) => r,
        None => {
            return exit_code_to_result(
                exit::NO_MATCHES,
                "",
                &format!("Symbol '{}' not found in {}.", p.symbol, p.path),
            );
        }
    };

    if result.replacements == 0 {
        return exit_code_to_result(exit::NO_MATCHES, "", "No matches within symbol.");
    }

    // Write the file
    let mut global = GlobalFlags::with_cwd(&cwd);
    global.apply = true;
    let policy = crate::write::policy_from_flags(&global, Some(&target));
    let mut backup = crate::backup::BackupSession::new(&cwd)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    backup
        .save_before_write(&target)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    crate::write::atomic_write(&target, &result.content, &policy)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    backup
        .finalize()
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

    let obj = serde_json::json!({
        "ok": true,
        "symbol": p.symbol,
        "replacements": result.replacements,
        "start_line": result.start_line,
        "end_line": result.end_line,
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
