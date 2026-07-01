//! AST MCP handler implementations, extracted from mod.rs (#928).
//!
//! Each `pub(super)` function contains the logic for one AST MCP tool.
//! The `#[tool]` stubs in `mod.rs` delegate directly to these functions.

use rmcp::model::{CallToolResult, ContentBlock, ErrorData as McpError};

use crate::cli::global::GlobalFlags;
use crate::exit;

use super::params::*;
use super::{
    PatchloomService, exit_code_to_result, no_results, validate_content_size, validate_param_size,
};

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
        return no_results("No symbols found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
    let start = sym
        .start_line
        .saturating_sub(1_usize.saturating_add(p.context));
    let end = sym.end_line.saturating_add(p.context).min(lines.len());
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
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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

    let global = GlobalFlags::with_cwd_and_json(&cwd);

    let paths = crate::cmd::ast::resolve_target_paths(&target, &p.path, &global)
        .map_err(|e| McpError::invalid_params(format!("{e}"), None))?;

    // Pre-filter to files with matches, build one AstRename op per file,
    // then execute as a batch through the tx engine for backup/rollback (#1100).
    let mut operations = Vec::new();
    for path in &paths {
        let rel = path
            .strip_prefix(&cwd)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        svc.check_path(&rel)?;

        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
        let source = std::fs::read_to_string(path)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;

        let has_match = if lang.has_grammar() {
            crate::ast::rename::rename_in_source(&source, &p.old_name, &p.new_name, lang)
                .is_some_and(|r| r.replacements > 0)
        } else {
            false
        } || {
            crate::ops::replace::compile_replace_regex(&p.old_name, false, false, false, true)
                .ok()
                .flatten()
                .is_some_and(|re| re.is_match(&source))
        };

        if has_match {
            operations.push(crate::plan::Operation::AstRename {
                path: rel,
                old_name: p.old_name.clone(),
                new_name: p.new_name.clone(),
                lang: p.lang.clone(),
            });
        }
    }

    if operations.is_empty() {
        return no_results("No matches found.");
    }

    svc.run_ops(operations, None)
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
        return no_results("No files with grammars found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
    // Pre-validate pattern query compilation before entering the parallel loop.
    // Without this, compile_pattern_query errors are silently swallowed via .ok()?,
    // causing "No matches found" instead of a useful error message.
    let precompiled_query = if p.pattern {
        let validation_lang = lang_hint.unwrap_or_else(|| {
            paths
                .first()
                .map(|p| crate::ast::Language::from_path(p))
                .unwrap_or(crate::ast::Language::Rust)
        });
        Some(
            crate::ast::search::compile_pattern_query(&p.query, validation_lang).map_err(|e| {
                McpError::invalid_params(format!("invalid pattern query: {e}"), None)
            })?,
        )
    } else {
        None
    };

    let par_results: Vec<SearchFileResult> = crate::par_process_files(&paths, None, &[], |path| {
        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(path));
        let query_str = if p.pattern {
            // Re-compile per language (different languages may have different grammars),
            // but compilation errors are now caught by pre-validation above.
            crate::ast::search::compile_pattern_query(&p.query, lang)
                .unwrap_or_else(|_| precompiled_query.clone().unwrap_or_default())
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
        return no_results("No matches found.");
    }
    let json = serde_json::to_string_pretty(&all_matches)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
        return no_results("No references found.");
    }

    let obj = serde_json::json!({
        "symbol": p.symbol,
        "references": all_refs,
        "count": all_refs.len(),
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
                    .filter(|i| {
                        // Match as a complete path segment to avoid false positives
                        // (e.g., "lib" should not match "stdlib" or "calibration").
                        let p = &i.path;
                        let t = target_name.as_str();
                        p == t
                            || p.ends_with(&format!("/{t}"))
                            || p.ends_with(&format!("::{t}"))
                            || p.ends_with(&format!("/{t}."))
                            || p.contains(&format!("/{t}/"))
                            || p.contains(&format!("::{t}::"))
                    })
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
        return no_results("No imports found.");
    }
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
        return no_results("No symbols found.");
    }

    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
        return no_results("No structural changes.");
    }

    let obj = serde_json::json!({
        "file": p.path,
        "from": p.from,
        "to": p.to.as_deref().unwrap_or("working tree"),
        "changes": changes,
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
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
        return no_results(&format!("No references found for '{}'.", p.symbol));
    }

    let obj = serde_json::json!({
        "symbol": p.symbol,
        "depth": p.depth,
        "impact": nodes,
        "total_count": nodes.len(),
    });
    let json = serde_json::to_string_pretty(&obj)
        .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}

pub(super) fn handle_ast_replace(
    svc: &PatchloomService,
    p: AstReplaceParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("old", &p.old)?;
    validate_content_size("new", &p.new_text)?;
    validate_param_size("symbol", &p.symbol)?;
    // Route through the tx engine for backup/rollback safety (#1100).
    let op = crate::plan::Operation::AstReplace {
        path: p.path,
        symbol: p.symbol,
        old: p.old,
        new_text: p.new_text,
        regex: p.regex,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_insert(
    svc: &PatchloomService,
    p: AstInsertParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_content_size("content", &p.content)?;
    let op = crate::plan::Operation::AstInsert {
        path: p.path,
        content: p.content,
        inside: p.inside,
        after: p.after,
        before: p.before,
        position: p.position,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_wrap(
    svc: &PatchloomService,
    p: AstWrapParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    validate_param_size("wrapper", &p.wrapper)?;
    let op = crate::plan::Operation::AstWrap {
        path: p.path,
        symbols: p.symbols,
        lines: p.lines,
        wrapper: p.wrapper,
        preamble: p.preamble,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_imports(
    svc: &PatchloomService,
    p: AstImportsParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;

    // List-only mode (no mutation)
    if p.add.is_none() && p.remove.is_none() && !p.dedupe {
        let cwd = svc.cwd().to_path_buf();
        let target = cwd.join(&p.path);
        let lang_hint = p.lang.as_deref().map(crate::cmd::ast::lang_from_str);
        let lang = lang_hint.unwrap_or_else(|| crate::ast::Language::from_path(&target));
        let source = std::fs::read_to_string(&target)
            .map_err(|e| McpError::internal_error(format!("reading {}: {e}", p.path), None))?;
        let imports = crate::ast::imports::list_imports(&source, lang);
        let obj = serde_json::json!({
            "file": p.path,
            "imports": imports.iter().map(|i| serde_json::json!({
                "text": i.text,
                "line": i.line,
            })).collect::<Vec<_>>(),
            "count": imports.len(),
        });
        let json = serde_json::to_string_pretty(&obj)
            .map_err(|e| McpError::internal_error(format!("{e}"), None))?;
        return Ok(CallToolResult::success(vec![ContentBlock::text(json)]));
    }

    let op = crate::plan::Operation::AstImports {
        path: p.path,
        add: p.add,
        remove: p.remove,
        dedupe: p.dedupe,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_reorder(
    svc: &PatchloomService,
    p: AstReorderParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    let op = crate::plan::Operation::AstReorder {
        path: p.path,
        inside: p.inside,
        order: p.order,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_group(
    svc: &PatchloomService,
    p: AstGroupParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    let op = crate::plan::Operation::AstGroup {
        path: p.path,
        module: p.module,
        symbols: p.symbols,
        preamble: p.preamble,
        position: p.position,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_move(
    svc: &PatchloomService,
    p: AstMoveParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.path)?;
    svc.check_path(&p.target)?;
    let op = crate::plan::Operation::AstMove {
        path: p.path,
        target: p.target,
        symbols: p.symbols,
        position: p.position,
        target_prepend: p.target_prepend,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_extract_to_file(
    svc: &PatchloomService,
    p: AstExtractToFileParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.source)?;
    svc.check_path(&p.target)?;
    let op = crate::plan::Operation::AstExtractToFile {
        source: p.source,
        symbol: p.symbol,
        target: p.target,
        replacement: p.replacement,
        unwrap: p.unwrap,
        prepend: p.prepend,
        force: p.force,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

pub(super) fn handle_ast_split(
    svc: &PatchloomService,
    p: AstSplitParams,
) -> Result<CallToolResult, McpError> {
    svc.check_path(&p.source)?;
    for t in &p.targets {
        svc.check_path(&t.path)?;
    }
    let targets: Vec<crate::plan::SplitTargetSpec> = p
        .targets
        .into_iter()
        .map(|t| crate::plan::SplitTargetSpec {
            path: t.path,
            symbols: t.symbols,
            prepend: t.prepend,
        })
        .collect();
    let op = crate::plan::Operation::AstSplit {
        source: p.source,
        targets,
        keep_in_source: p.keep_in_source,
        source_suffix: p.source_suffix,
        source_prefix: p.source_prefix,
        require_exhaustive: p.require_exhaustive,
        lang: p.lang,
    };
    svc.run_one_op(op, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock;
    use tempfile::TempDir;

    fn make_service(dir: &TempDir) -> PatchloomService {
        PatchloomService::new(dir.path().to_path_buf(), None).unwrap()
    }

    /// Extract text from the first ContentBlock item in a CallToolResult.
    fn extract_text(result: &CallToolResult) -> String {
        match &result.content[0] {
            ContentBlock::Text(t) => t.text.clone(),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    const RUST_SAMPLE: &str = r#"
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn origin() -> Self {
        Point { x: 0.0, y: 0.0 }
    }
}
"#;

    #[test]
    fn ast_list_single_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstListParams {
            path: "sample.rs".into(),
            kind: None,
            lang: Some("rs".into()),
        };

        let result = handle_ast_list(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("greet"));
        assert!(text.contains("Point"));
    }

    #[test]
    fn ast_list_kind_filter() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstListParams {
            path: "sample.rs".into(),
            kind: Some("struct".into()),
            lang: Some("rs".into()),
        };

        let result = handle_ast_list(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("Point"));
        assert!(!text.contains("\"greet\""));
    }

    #[test]
    fn ast_list_path_not_found() {
        let dir = TempDir::new().unwrap();
        let svc = make_service(&dir);
        let params = AstListParams {
            path: "nonexistent.rs".into(),
            kind: None,
            lang: None,
        };

        let result = handle_ast_list(&svc, params);
        assert!(result.is_err());
    }

    #[test]
    fn ast_read_symbol() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstReadParams {
            path: "sample.rs".into(),
            symbol: "greet".into(),
            context: 0,
            lang: Some("rs".into()),
        };

        let result = handle_ast_read(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("greet"));
        assert!(text.contains("Hello"));
    }

    #[test]
    fn ast_read_symbol_not_found() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstReadParams {
            path: "sample.rs".into(),
            symbol: "nonexistent_fn".into(),
            context: 0,
            lang: Some("rs".into()),
        };

        let result = handle_ast_read(&svc, params);
        assert!(result.is_err());
    }

    #[test]
    fn ast_validate_valid_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("valid.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstValidateParams {
            path: "valid.rs".into(),
            lang: Some("rs".into()),
        };

        let result = handle_ast_validate(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("\"valid\": true"));
    }

    #[test]
    fn ast_validate_syntax_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("bad.rs"), "fn broken( {").unwrap();

        let svc = make_service(&dir);
        let params = AstValidateParams {
            path: "bad.rs".into(),
            lang: Some("rs".into()),
        };

        let result = handle_ast_validate(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("\"valid\": false"));
    }

    #[test]
    fn ast_search_finds_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstSearchParams {
            path: "sample.rs".into(),
            query: "(function_item name: (identifier) @name)".into(),
            pattern: false,
            lang: Some("rs".into()),
            max_results: None,
        };

        let result = handle_ast_search(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("greet"));
    }

    #[test]
    fn ast_rename_replaces_symbol() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("rename.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstRenameParams {
            old_name: "greet".into(),
            new_name: "salute".into(),
            path: "rename.rs".into(),
            lang: Some("rs".into()),
        };

        let result = handle_ast_rename(&svc, params).unwrap();
        let text = extract_text(&result);
        assert!(text.contains("\"ok\": true"));

        let content = std::fs::read_to_string(dir.path().join("rename.rs")).unwrap();
        assert!(content.contains("salute"));
        assert!(!content.contains("greet"));
    }

    #[test]
    fn ast_rename_same_name_no_match() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.rs"), RUST_SAMPLE).unwrap();

        let svc = make_service(&dir);
        let params = AstRenameParams {
            old_name: "greet".into(),
            new_name: "greet".into(),
            path: "sample.rs".into(),
            lang: Some("rs".into()),
        };

        let result = handle_ast_rename(&svc, params).unwrap();
        assert!(result.is_error.unwrap_or(false));
    }
}
