//! Symbol extraction from source files using tree-sitter AST parsing.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

/// The kind of symbol extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Method,
    Const,
    Type,
    Interface,
    Module,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Function => "fn",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Class => "class",
            Self::Method => "method",
            Self::Const => "const",
            Self::Type => "type",
            Self::Interface => "interface",
            Self::Module => "mod",
        };
        f.write_str(s)
    }
}

impl SymbolKind {
    /// Parse from a string like "function", "struct", etc.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "fn" | "func" | "function" => Some(Self::Function),
            "struct" => Some(Self::Struct),
            "enum" => Some(Self::Enum),
            "trait" => Some(Self::Trait),
            "impl" => Some(Self::Impl),
            "class" => Some(Self::Class),
            "method" => Some(Self::Method),
            "const" | "constant" => Some(Self::Const),
            "type" => Some(Self::Type),
            "interface" => Some(Self::Interface),
            "mod" | "module" => Some(Self::Module),
            _ => None,
        }
    }
}

/// A single extracted symbol definition.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolDef {
    /// The symbol name.
    pub name: String,
    /// The kind of symbol.
    pub kind: SymbolKind,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// One-line signature (up to opening brace or first line).
    pub signature: String,
    /// Nested symbols (e.g. methods inside impl, functions inside mod).
    pub children: Vec<SymbolDef>,
    /// Nesting depth (0 = top-level).
    pub depth: usize,
}

/// Extract all symbol definitions from source code.
pub fn extract_symbols(source: &str, lang: Language) -> Vec<SymbolDef> {
    let Some((tree, _)) = parse_source(source, lang) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let mut cursor = tree.walk();
    super::symbol_extract::visit_node(&mut cursor, source, lang, 0, &mut symbols);

    // Go-specific: group receiver methods under their receiver type.
    // Go methods with receivers (e.g. `func (s *Server) Start()`) are
    // syntactically top-level in the AST, unlike Rust impl blocks or
    // Python/TS class methods. Group them so qualified lookups like
    // `Server::Start` work.
    if lang == Language::Go {
        super::symbol_extract::group_go_receiver_methods(&mut symbols);
    }

    symbols
}

/// Read a file and extract symbols.
pub fn extract_symbols_from_file(path: &Path, lang_hint: Option<Language>) -> Vec<SymbolDef> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Vec::new();
    }
    // SoftSkip multi-path (#1894): binary / invalid UTF-8 → empty.
    let Some(source) = crate::files::read_text_file(path) else {
        return Vec::new();
    };
    extract_symbols(&source, lang)
}

/// Find a symbol by name, optionally qualified (e.g. "Impl::method").
pub fn find_symbol<'a>(symbols: &'a [SymbolDef], name: &str) -> Option<&'a SymbolDef> {
    if let Some((parent, rest)) = name.split_once("::") {
        // Qualified name: find parent, then recurse into children.
        // Try ALL matching parents (not just the first) because a struct
        // and its impl block share the same name but only the impl has
        // method children.
        for sym in symbols {
            if sym.name == parent
                && let Some(found) = find_symbol(&sym.children, rest)
            {
                return Some(found);
            }
        }
        None
    } else {
        // Unqualified: search top-level first (BFS), then recurse
        for sym in symbols {
            if sym.name == name {
                return Some(sym);
            }
        }
        for sym in symbols {
            if let Some(found) = find_symbol(&sym.children, name) {
                return Some(found);
            }
        }
        None
    }
}

/// Compute the full source range of a symbol including preceding attributes
/// and doc comments that are conventionally part of the symbol definition.
///
/// Returns `(full_start_line, end_line)` where both are 1-based and
/// `full_start_line` may be earlier than `sym.start_line`.
///
/// Language-aware: handles `#[...]` for Rust, `@decorator` for Python/TS/Java,
/// `///` and `//!` doc comments for Rust, `/** ... */` JSDoc for JS/TS/Java,
/// and `//` doc comments preceding Go functions.
pub fn full_symbol_span(source: &str, sym: &SymbolDef, lang: Language) -> (usize, usize) {
    let lines: Vec<&str> = source.lines().collect();
    let sym_start_0 = sym.start_line.saturating_sub(1); // convert to 0-based
    if sym_start_0 == 0 {
        return (sym.start_line, sym.end_line);
    }

    let mut first_line_0 = sym_start_0;

    // Walk backwards from the line before the symbol
    let mut i = sym_start_0;
    while i > 0 {
        i -= 1;
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            // Empty line: include only if the line above is also an attribute/doc
            // (i.e., don't include leading blank lines that aren't between attrs)
            if i > 0 && is_annotation_line(lines[i - 1].trim(), lang) {
                first_line_0 = i;
                continue;
            }
            break;
        }

        if is_annotation_line(trimmed, lang) {
            first_line_0 = i;
        } else if lang == Language::Rust {
            // Check if this line is part of a multiline `#[...]` attribute.
            // E.g., `#[cfg_attr(\n  feature = "serde",\n  derive(Serialize)\n)]`
            // Line `)]` doesn't match is_annotation_line but is the closing
            // of a multiline attribute block.
            if let Some(attr_start) = find_rust_multiline_attr_start(&lines, i) {
                first_line_0 = attr_start;
                // Jump the backward walk past all lines we've consumed
                i = attr_start;
                continue;
            }
            break;
        } else if lang == Language::Python
            || lang == Language::TypeScript
            || lang == Language::JavaScript
            || lang == Language::Java
            || lang == Language::Kotlin
        {
            // Check if this line is a continuation of a multiline decorator,
            // e.g. `@decorator(\n  arg1,\n  arg2\n)` (#1103).
            if let Some(dec_start) = find_multiline_decorator_start(&lines, i) {
                first_line_0 = dec_start;
                i = dec_start;
                continue;
            }
            break;
        } else {
            break;
        }
    }

    (first_line_0 + 1, sym.end_line) // back to 1-based
}

/// Detect overlapping spans in a set of 0-based `(start, end)` line ranges.
///
/// Returns `Err` listing the overlapping span pairs if any overlap, `Ok(())` otherwise.
/// Spans are overlapping when one range's start falls strictly inside another range
/// (i.e., `a.start < b.start < a.end` for sorted spans).
pub fn check_no_overlapping_spans(spans: &[(usize, usize)], names: &[&str]) -> anyhow::Result<()> {
    if spans.len() < 2 {
        return Ok(());
    }

    // Sort by start, then by end (descending) to detect containment
    let mut indexed: Vec<(usize, usize, usize)> = spans
        .iter()
        .enumerate()
        .map(|(i, &(s, e))| (s, e, i))
        .collect();
    indexed.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    let mut overlaps: Vec<String> = Vec::new();
    for window in indexed.windows(2) {
        let (s1, e1, i1) = window[0];
        let (s2, e2, i2) = window[1];
        // Overlap: second span starts before first span ends
        if s2 < e1 {
            let n1 = names.get(i1).copied().unwrap_or("?");
            let n2 = names.get(i2).copied().unwrap_or("?");
            overlaps.push(format!(
                "'{}' (lines {}-{}) and '{}' (lines {}-{})",
                n1,
                s1 + 1,
                e1,
                n2,
                s2 + 1,
                e2
            ));
        }
    }

    if overlaps.is_empty() {
        Ok(())
    } else {
        Err(anyhow::Error::new(crate::exit::InvalidInputError {
            msg: format!(
                "overlapping symbol spans would corrupt content: {}",
                overlaps.join("; ")
            ),
        }))
    }
}

/// Check if a line is an annotation/attribute/decorator/doc-comment for a symbol.
fn is_annotation_line(trimmed: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => {
            trimmed.starts_with("#[")
                || trimmed.starts_with("///")
                || trimmed.starts_with("/**")
                || trimmed.starts_with("* ")
                || trimmed == "*/"
                || trimmed == "*"
        }
        Language::Python => trimmed.starts_with('@'),
        Language::TypeScript | Language::JavaScript => {
            trimmed.starts_with('@')
                || trimmed.starts_with("/**")
                || trimmed.starts_with("* ")
                || trimmed == "*/"
                || trimmed == "*"
        }
        Language::Java | Language::Kotlin => {
            trimmed.starts_with('@')
                || trimmed.starts_with("/**")
                || trimmed.starts_with("* ")
                || trimmed == "*/"
                || trimmed == "*"
        }
        Language::Go => {
            // Go doc comments are // lines immediately preceding a declaration
            trimmed.starts_with("//")
        }
        _ => false,
    }
}

/// Check whether the lines starting at `from_0` (0-indexed, walking backwards)
/// form the interior/closing of a multiline Rust `#[...]` attribute.
///
/// Scans backwards from `from_0`, tracking bracket depth (`[` / `]`). If
/// we reach a line that starts with `#[` and bracket depth balances to zero,
/// this is a multiline attribute and we return the 0-based index of the
/// opening `#[` line. Otherwise returns `None`.
fn find_rust_multiline_attr_start(lines: &[&str], from_0: usize) -> Option<usize> {
    // Count `]` minus `[` on the starting line. A closing `)]` has depth +1.
    let mut depth: i32 = 0;
    for j in (0..=from_0).rev() {
        let trimmed = lines[j].trim();
        // Count brackets on this line (] increases depth, [ decreases it
        // since we're walking backwards from the closing side).
        for ch in trimmed.chars() {
            if ch == ']' {
                depth += 1;
            } else if ch == '[' {
                depth -= 1;
            }
        }
        if trimmed.starts_with("#[") && depth <= 0 {
            return Some(j);
        }
        // Safety: don't walk more than 20 lines back for a single attribute
        if from_0 - j > 20 {
            break;
        }
    }
    None
}

/// Check whether the lines starting at `from_0` form the interior/closing
/// of a multiline decorator (e.g. `@decorator(\n  arg1,\n  arg2\n)`).
///
/// Scans backwards from `from_0`, tracking parenthesis depth. If we reach
/// a line starting with `@` and parens balance, returns that line index.
fn find_multiline_decorator_start(lines: &[&str], from_0: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    for j in (0..=from_0).rev() {
        let trimmed = lines[j].trim();
        for ch in trimmed.chars() {
            if ch == ')' {
                depth += 1;
            } else if ch == '(' {
                depth -= 1;
            }
        }
        if trimmed.starts_with('@') && depth <= 0 {
            return Some(j);
        }
        if from_0 - j > 20 {
            break;
        }
    }
    None
}

/// Compute the byte offset of the start of each line in `source`.
/// Returns a vector where `offsets[i]` is the byte position of the start of
/// line `i` (0-indexed). `offsets[lines.len()]` is `source.len()`, so that
/// `source[offsets[i]..offsets[i+1]]` gives line `i` including its line ending.
pub(crate) fn compute_line_byte_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            offsets.push(i + 1);
            i += 1;
        } else if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            offsets.push(i + 2);
            i += 2;
        } else if bytes[i] == b'\r' {
            offsets.push(i + 1);
            i += 1;
        } else {
            i += 1;
        }
    }
    offsets
}

/// Extract the text of a symbol from source, using the full span (including
/// attributes and doc comments).
pub fn extract_symbol_text<'a>(source: &'a str, sym: &SymbolDef, lang: Language) -> &'a str {
    let (full_start, full_end) = full_symbol_span(source, sym, lang);
    let lines: Vec<&str> = source.lines().collect();
    let start_0 = full_start.saturating_sub(1);
    let end_0 = full_end.min(lines.len());

    // Compute byte offsets by scanning the actual source bytes rather than
    // assuming uniform line endings (files may mix \r\n and \n).
    let line_offsets = compute_line_byte_offsets(source);
    let byte_start = if start_0 < line_offsets.len() {
        line_offsets[start_0]
    } else {
        source.len()
    };
    let byte_end = if end_0 < line_offsets.len() {
        line_offsets[end_0]
    } else {
        source.len()
    };

    &source[byte_start..byte_end]
}

/// Parse a comma-separated kind filter string into a list of `SymbolKind`s.
pub fn parse_kind_filter(kind_arg: &Option<String>) -> Vec<SymbolKind> {
    match kind_arg {
        Some(s) => s
            .split(',')
            .filter_map(|k| SymbolKind::from_str_loose(k.trim()))
            .collect(),
        None => Vec::new(),
    }
}

/// Filter symbols by kind. Returns all symbols if filter is empty.
pub fn filter_symbols<'a>(
    symbols: &'a [SymbolDef],
    kind_filter: &[SymbolKind],
) -> Vec<&'a SymbolDef> {
    if kind_filter.is_empty() {
        return symbols.iter().collect();
    }
    symbols
        .iter()
        .filter(|s| kind_filter.contains(&s.kind))
        .collect()
}

#[cfg(test)]
mod tests;
