//! AST-aware symbol grouping: move symbols into named modules within a file.

use super::Language;
use super::symbols::{extract_symbol_text, extract_symbols, find_symbol, full_symbol_span};

/// Where to place a newly created module.
#[derive(Debug, Clone)]
pub enum GroupPosition {
    /// At the position of the first moved symbol (default).
    FirstSymbol,
    /// At the end of the file.
    End,
    /// After a named symbol.
    After(String),
}

/// Specification for a group operation.
#[derive(Debug)]
pub struct GroupSpec {
    pub module: String,
    pub symbols: Vec<String>,
    pub preamble: Option<String>,
    pub position: GroupPosition,
}

/// Result of a group operation.
#[derive(Debug)]
pub struct GroupResult {
    pub content: String,
    pub symbols_moved: usize,
    pub module_created: bool,
}

/// Group named symbols into a module within the same file.
pub fn group_symbols(
    source: &str,
    spec: &GroupSpec,
    lang: Language,
) -> anyhow::Result<GroupResult> {
    let eol = crate::write::detect_eol(source);
    let symbols = extract_symbols(source, lang);
    let lines: Vec<&str> = source.lines().collect();

    // Check if target module already exists
    let existing_mod = find_symbol(&symbols, &spec.module)
        .filter(|s| s.kind == super::symbols::SymbolKind::Module);
    let module_exists = existing_mod.is_some();

    // Find each symbol to move and collect their text + spans
    let mut to_move: Vec<(usize, usize, String)> = Vec::new(); // (start_0, end_0, text)
    let mut missing: Vec<&str> = Vec::new();

    for name in &spec.symbols {
        if let Some(sym) = find_symbol(&symbols, name) {
            // Check if already inside the target module
            if let Some(parent_mod) = &existing_mod
                && sym.start_line >= parent_mod.start_line
                && sym.end_line <= parent_mod.end_line
            {
                continue; // already inside target module
            }
            let (full_start, full_end) = full_symbol_span(source, sym, lang);
            let text = extract_symbol_text(source, sym, lang).to_string();
            to_move.push((
                full_start.saturating_sub(1),
                full_end.min(lines.len()),
                text,
            ));
        } else {
            missing.push(name);
        }
    }

    if !missing.is_empty() {
        anyhow::bail!("symbol(s) not found: {}", missing.join(", "));
    }

    if to_move.is_empty() {
        return Ok(GroupResult {
            content: source.to_string(),
            symbols_moved: 0,
            module_created: false,
        });
    }

    let symbols_moved = to_move.len();

    // Sort spans by start position (descending) for safe removal
    to_move.sort_by_key(|b| std::cmp::Reverse(b.0));

    // Build the module content: indent each symbol by 4 spaces
    let mut module_body = String::new();
    if let Some(preamble) = &spec.preamble {
        module_body.push_str(&indent_text(preamble, "    ", eol));
        module_body.push_str(eol);
        module_body.push_str(eol);
    }

    // Collect texts in original order (we sorted descending for removal)
    let mut texts_in_order: Vec<(usize, &str)> = to_move
        .iter()
        .map(|(start, _, text)| (*start, text.as_str()))
        .collect();
    texts_in_order.sort_by_key(|(start, _)| *start);

    for (i, (_, text)) in texts_in_order.iter().enumerate() {
        if i > 0 {
            module_body.push_str(eol);
        }
        let trimmed = text.trim_end_matches('\n');
        module_body.push_str(&indent_text(trimmed, "    ", eol));
        module_body.push_str(eol);
    }

    // Remove symbols from source (in reverse order to preserve offsets)
    let mut modified_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    for (start_0, end_0, _) in &to_move {
        // Remove the lines, also removing a trailing blank line if present
        let mut remove_end = *end_0;
        if remove_end < modified_lines.len() && modified_lines[remove_end].trim().is_empty() {
            remove_end += 1;
        }
        modified_lines.drain(*start_0..remove_end);
    }

    if module_exists {
        // Find the existing module's closing brace in modified_lines
        // Re-parse to find the module in modified source
        let modified_source = modified_lines.join(eol);
        let mod_symbols = extract_symbols(&modified_source, lang);
        if let Some(mod_sym) = find_symbol(&mod_symbols, &spec.module) {
            let close_line_0 = mod_sym.end_line.saturating_sub(1);
            // Insert before closing brace
            let mut result_lines: Vec<String> = Vec::new();
            let re_lines: Vec<&str> = modified_source.lines().collect();
            for (i, line) in re_lines.iter().enumerate() {
                if i == close_line_0 {
                    // Add a blank line before new content if previous line isn't blank
                    if i > 0 && !re_lines[i - 1].trim().is_empty() {
                        result_lines.push(String::new());
                    }
                    for body_line in module_body.lines() {
                        result_lines.push(body_line.to_string());
                    }
                }
                result_lines.push(line.to_string());
            }
            let mut result = result_lines.join(eol);
            if source.ends_with('\n') && !result.ends_with('\n') {
                result.push_str(eol);
            }
            return Ok(GroupResult {
                content: result,
                symbols_moved,
                module_created: false,
            });
        }
    }

    // Build the module block for a new module
    let module_block = format!("mod {} {{{eol}{}}}{eol}", spec.module, module_body);

    // Insert new module at the specified position
    let insert_idx = match &spec.position {
        GroupPosition::FirstSymbol => {
            // Position of the first moved symbol (in the original line indices)
            let first_start = texts_in_order.first().map(|(s, _)| *s).unwrap_or(0);
            // Map to modified_lines index: count how many lines were removed before this point
            let mut offset = 0usize;
            let mut spans_before: Vec<(usize, usize)> = to_move
                .iter()
                .map(|(s, e, _)| {
                    let mut end = *e;
                    if end < lines.len() && lines[end].trim().is_empty() {
                        end += 1;
                    }
                    (*s, end)
                })
                .collect();
            spans_before.sort_by_key(|(s, _)| *s);
            for (s, e) in &spans_before {
                if *s < first_start {
                    offset += e - s;
                }
            }
            first_start.saturating_sub(offset)
        }
        GroupPosition::End => modified_lines.len(),
        GroupPosition::After(sym_name) => {
            let modified_source = modified_lines.join(eol);
            let mod_symbols = extract_symbols(&modified_source, lang);
            if let Some(sym) = find_symbol(&mod_symbols, sym_name) {
                sym.end_line.min(modified_lines.len())
            } else {
                modified_lines.len()
            }
        }
    };

    let insert_idx = insert_idx.min(modified_lines.len());

    // Insert module block
    let block_lines: Vec<String> = module_block.lines().map(String::from).collect();
    // Add blank line before if needed
    if insert_idx > 0
        && insert_idx <= modified_lines.len()
        && !modified_lines[insert_idx - 1].trim().is_empty()
    {
        modified_lines.insert(insert_idx, String::new());
        let idx = insert_idx + 1;
        for (i, bl) in block_lines.iter().enumerate() {
            modified_lines.insert(idx + i, bl.clone());
        }
    } else {
        for (i, bl) in block_lines.iter().enumerate() {
            modified_lines.insert(insert_idx + i, bl.clone());
        }
    }

    let mut result = modified_lines.join(eol);
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push_str(eol);
    }
    if !source.ends_with('\n') && result.ends_with('\n') {
        result.truncate(result.len() - eol.len());
    }

    Ok(GroupResult {
        content: result,
        symbols_moved,
        module_created: true,
    })
}

fn indent_text(text: &str, indent: &str, eol: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join(eol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_basic() {
        let source = "fn foo() {}\n\nfn bar() {}\n\nfn baz() {}\n";
        let spec = GroupSpec {
            module: "helpers".into(),
            symbols: vec!["foo".into(), "bar".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert!(result.content.contains("mod helpers {"));
        assert!(result.content.contains("    fn foo()"));
        assert!(result.content.contains("    fn bar()"));
        assert!(result.content.contains("fn baz()")); // not grouped
        assert_eq!(result.symbols_moved, 2);
        assert!(result.module_created);
    }

    #[test]
    fn group_with_preamble() {
        let source = "fn test_a() {}\n\nfn test_b() {}\n";
        let spec = GroupSpec {
            module: "tests".into(),
            symbols: vec!["test_a".into(), "test_b".into()],
            preamble: Some("use super::*;".into()),
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert!(result.content.contains("use super::*;"));
        assert!(result.content.contains("mod tests {"));
    }

    #[test]
    fn group_symbol_not_found() {
        let source = "fn foo() {}\n";
        let spec = GroupSpec {
            module: "m".into(),
            symbols: vec!["nonexistent".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn group_preserves_attributes() {
        let source = "#[test]\nfn test_a() {}\n\nfn helper() {}\n";
        let spec = GroupSpec {
            module: "tests".into(),
            symbols: vec!["test_a".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert!(result.content.contains("#[test]"));
        assert!(result.content.contains("fn test_a()"));
    }

    #[test]
    fn group_position_end() {
        let source = "fn alpha() {}\n\nfn beta() {}\n";
        let spec = GroupSpec {
            module: "m".into(),
            symbols: vec!["alpha".into()],
            preamble: None,
            position: GroupPosition::End,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        // Module should be after beta
        let beta_pos = result.content.find("fn beta").unwrap();
        let mod_pos = result.content.find("mod m {").unwrap();
        assert!(mod_pos > beta_pos);
    }

    #[test]
    fn group_append_to_existing_module() {
        let source = "mod helpers {\n    fn existing() {}\n}\n\nfn new_helper() {}\n";
        let spec = GroupSpec {
            module: "helpers".into(),
            symbols: vec!["new_helper".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert!(result.content.contains("fn existing()"));
        assert!(result.content.contains("fn new_helper()"));
        assert!(!result.module_created);
        // Should only have one "mod helpers"
        assert_eq!(result.content.matches("mod helpers").count(), 1);
    }

    #[test]
    fn group_no_op_already_inside() {
        let source = "mod m {\n    fn foo() {}\n}\n";
        let spec = GroupSpec {
            module: "m".into(),
            symbols: vec!["foo".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert_eq!(result.symbols_moved, 0);
    }

    #[test]
    fn group_position_after_symbol() {
        let source = "fn anchor() {}\n\nfn target() {}\n\nfn trailing() {}\n";
        let spec = GroupSpec {
            module: "m".into(),
            symbols: vec!["target".into()],
            preamble: None,
            position: GroupPosition::After("anchor".into()),
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        let anchor_pos = result.content.find("fn anchor").unwrap();
        let mod_pos = result.content.find("mod m {").unwrap();
        let trailing_pos = result.content.find("fn trailing").unwrap();
        assert!(anchor_pos < mod_pos);
        assert!(mod_pos < trailing_pos);
        assert!(result.module_created);
        assert_eq!(result.symbols_moved, 1);
    }

    #[test]
    fn group_preserves_crlf_line_endings() {
        let source = "fn foo() {}\r\n\r\nfn bar() {}\r\n\r\nfn baz() {}\r\n";
        let spec = GroupSpec {
            module: "helpers".into(),
            symbols: vec!["foo".into(), "bar".into()],
            preamble: None,
            position: GroupPosition::FirstSymbol,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        let bytes = result.content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "bare LF at byte {i} in: {:?}",
                    result.content
                );
            }
        }
    }

    #[test]
    fn group_creates_new_module_when_name_matches_function() {
        // If a function "helpers" exists, group should create a new `mod helpers`
        // rather than inserting content inside the function body.
        let source = "fn helpers() {}\n\nfn target() {}\n";
        let spec = GroupSpec {
            module: "helpers".into(),
            symbols: vec!["target".into()],
            preamble: None,
            position: GroupPosition::End,
        };
        let result = group_symbols(source, &spec, Language::Rust).unwrap();
        assert!(result.module_created);
        assert!(result.content.contains("mod helpers {"));
        assert!(result.content.contains("fn helpers()")); // function preserved
        assert_eq!(result.symbols_moved, 1);
    }
}
