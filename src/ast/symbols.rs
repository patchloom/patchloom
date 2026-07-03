//! Symbol extraction from source files using tree-sitter AST parsing.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

// Re-export rewrite types for backward compatibility
pub use crate::ast::rewrite::{
    FunctionSigEdit, FunctionSpan, find_function_span, replace_function_signature,
    rewrite_function_signature,
};

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
    let Ok(source) = std::fs::read_to_string(path) else {
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
        anyhow::bail!(
            "overlapping symbol spans would corrupt content: {}",
            overlaps.join("; ")
        )
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
mod tests {
    use super::*;

    #[test]
    fn check_no_overlapping_spans_ok() {
        // Non-overlapping spans should pass
        let spans = vec![(0, 3), (4, 7), (8, 10)];
        let names = vec!["a", "b", "c"];
        check_no_overlapping_spans(&spans, &names).unwrap();
    }

    #[test]
    fn check_no_overlapping_spans_adjacent_ok() {
        // Adjacent (touching but not overlapping) spans should pass
        let spans = vec![(0, 3), (3, 6), (6, 9)];
        let names = vec!["a", "b", "c"];
        check_no_overlapping_spans(&spans, &names).unwrap();
    }

    #[test]
    fn check_no_overlapping_spans_detects_overlap() {
        // Overlapping spans should error
        let spans = vec![(0, 5), (3, 8)];
        let names = vec!["foo", "bar"];
        let err = check_no_overlapping_spans(&spans, &names).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("overlapping"), "error: {msg}");
        assert!(
            msg.contains("foo"),
            "error should mention first symbol: {msg}"
        );
        assert!(
            msg.contains("bar"),
            "error should mention second symbol: {msg}"
        );
    }

    #[test]
    fn check_no_overlapping_spans_detects_containment() {
        // One span fully inside another should error
        let spans = vec![(0, 10), (2, 5)];
        let names = vec!["outer", "inner"];
        let err = check_no_overlapping_spans(&spans, &names).unwrap_err();
        assert!(err.to_string().contains("overlapping"));
    }

    #[test]
    fn check_no_overlapping_spans_single_ok() {
        // Single span should always pass
        let spans = vec![(0, 5)];
        let names = vec!["only"];
        check_no_overlapping_spans(&spans, &names).unwrap();
    }

    #[test]
    fn check_no_overlapping_spans_empty_ok() {
        check_no_overlapping_spans(&[], &[]).unwrap();
    }

    #[test]
    fn extract_rust_symbols() {
        let source = r#"
struct Foo {
    x: i32,
}

fn bar() -> i32 {
    42
}

impl Foo {
    fn baz(&self) -> i32 {
        self.x
    }
}
"#;
        let symbols = extract_symbols(source, Language::Rust);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"bar"));
        // impl Foo should contain baz as a child
        let impl_sym = symbols.iter().find(|s| s.kind == SymbolKind::Impl).unwrap();
        assert_eq!(impl_sym.name, "Foo");
        assert_eq!(impl_sym.children.len(), 1);
        assert_eq!(impl_sym.children[0].name, "baz");
    }

    #[test]
    fn rust_impl_trait_extracts_type_not_trait() {
        // Regression: `impl Display for Foo` returned "Display" (the trait)
        // because child_text_by_kind found the first type_identifier.
        // The fix uses child_by_field_name("type") to get the target type.
        let source = r#"
use std::fmt;

struct Foo;

impl fmt::Display for Foo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Foo")
    }
}
"#;
        let symbols = extract_symbols(source, Language::Rust);
        let impl_sym = symbols.iter().find(|s| s.kind == SymbolKind::Impl).unwrap();
        assert_eq!(
            impl_sym.name, "Foo",
            "impl target should be Foo, not the trait"
        );
    }

    #[test]
    fn rust_impl_without_trait() {
        // Inherent impl (no trait) should still extract the type name.
        let source = "struct Bar;\nimpl Bar { fn go(&self) {} }\n";
        let symbols = extract_symbols(source, Language::Rust);
        let impl_sym = symbols.iter().find(|s| s.kind == SymbolKind::Impl).unwrap();
        assert_eq!(impl_sym.name, "Bar");
    }

    #[test]
    fn extract_python_symbols() {
        let source = r#"
class MyClass:
    def method(self):
        pass

def standalone():
    pass
"#;
        let symbols = extract_symbols(source, Language::Python);
        let class = symbols.iter().find(|s| s.name == "MyClass").unwrap();
        assert_eq!(class.kind, SymbolKind::Class);
        assert_eq!(class.children.len(), 1);
        assert_eq!(class.children[0].name, "method");
        assert_eq!(class.children[0].kind, SymbolKind::Method);

        let func = symbols.iter().find(|s| s.name == "standalone").unwrap();
        assert_eq!(func.kind, SymbolKind::Function);
    }

    #[test]
    fn extract_python_decorated_method() {
        // Regression: decorated methods have an extra `decorated_definition`
        // wrapper, so the grandparent check for `class_definition` failed.
        let source = "class Foo:\n    @staticmethod\n    def bar():\n        pass\n";
        let symbols = extract_symbols(source, Language::Python);
        let class = symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(class.children.len(), 1);
        assert_eq!(class.children[0].name, "bar");
        assert_eq!(
            class.children[0].kind,
            SymbolKind::Method,
            "decorated method should be classified as Method, not Function"
        );
    }

    #[test]
    fn extract_go_symbols() {
        let source = r#"
package main

func main() {
    fmt.Println("hello")
}

type Config struct {
    Host string
}
"#;
        let symbols = extract_symbols(source, Language::Go);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"Config"));
    }

    #[test]
    fn go_grouped_type_declaration() {
        // Regression: grouped `type (...)` blocks only returned the first type_spec.
        // The fix matches `type_spec` instead of `type_declaration` so visit_node
        // recurses into each spec independently.
        let source = "package main\n\ntype (\n\tPoint struct{ X, Y int }\n\tReader interface{ Read([]byte) (int, error) }\n\tAlias = int\n)\n";
        let symbols = extract_symbols(source, Language::Go);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Point"), "missing Point, got {:?}", names);
        assert!(names.contains(&"Reader"), "missing Reader, got {:?}", names);
        assert!(names.contains(&"Alias"), "missing Alias, got {:?}", names);
        let point = symbols.iter().find(|s| s.name == "Point").unwrap();
        assert_eq!(point.kind, SymbolKind::Struct);
        let reader = symbols.iter().find(|s| s.name == "Reader").unwrap();
        assert_eq!(reader.kind, SymbolKind::Interface);
    }

    #[test]
    fn find_symbol_qualified() {
        let source = r#"
impl Server {
    fn start(&self) {}
    fn stop(&self) {}
}
"#;
        let symbols = extract_symbols(source, Language::Rust);
        let found = find_symbol(&symbols, "Server::start").expect("should find Server::start");
        assert_eq!(found.name, "start");
    }

    #[test]
    fn find_symbol_unqualified_searches_children() {
        let source = r#"
impl Server {
    fn start(&self) {}
}
"#;
        let symbols = extract_symbols(source, Language::Rust);
        find_symbol(&symbols, "start").expect("should find 'start' via unqualified search");
    }

    /// find_symbol should recurse into deeply nested children, not just
    /// one level (#1111 item 5).
    #[test]
    fn find_symbol_deeply_nested() {
        // Build a 3-level hierarchy: mod > struct > method
        let inner = SymbolDef {
            name: "deep_method".into(),
            kind: SymbolKind::Method,
            start_line: 3,
            end_line: 4,
            signature: "fn deep_method()".into(),
            children: Vec::new(),
            depth: 2,
        };
        let mid = SymbolDef {
            name: "MidStruct".into(),
            kind: SymbolKind::Struct,
            start_line: 2,
            end_line: 5,
            signature: "struct MidStruct".into(),
            children: vec![inner],
            depth: 1,
        };
        let outer = SymbolDef {
            name: "outer_mod".into(),
            kind: SymbolKind::Module,
            start_line: 1,
            end_line: 6,
            signature: "mod outer_mod".into(),
            children: vec![mid],
            depth: 0,
        };
        let symbols = vec![outer];

        // Unqualified search should find a deeply nested symbol
        find_symbol(&symbols, "deep_method").expect("should find deeply nested symbol");

        // Qualified search with multiple :: should work recursively
        find_symbol(&symbols, "outer_mod::MidStruct").expect("should find qualified nested symbol");
    }

    /// Regression: when a struct and its impl block share the same name,
    /// find_symbol must try all parents with a matching name, not just the
    /// first. The struct has no children, so `Point::new` should resolve
    /// via the impl block's children.
    #[test]
    fn find_symbol_qualified_skips_struct_to_impl() {
        let source = r#"
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
"#;
        let symbols = extract_symbols(source, Language::Rust);
        let found = find_symbol(&symbols, "Point::new").expect("should find Point::new via impl");
        assert_eq!(found.name, "new");
    }

    #[test]
    fn symbol_kind_from_str() {
        assert_eq!(
            SymbolKind::from_str_loose("function"),
            Some(SymbolKind::Function)
        );
        assert_eq!(SymbolKind::from_str_loose("fn"), Some(SymbolKind::Function));
        assert_eq!(
            SymbolKind::from_str_loose("struct"),
            Some(SymbolKind::Struct)
        );
        assert_eq!(SymbolKind::from_str_loose("CONST"), Some(SymbolKind::Const));
        assert_eq!(SymbolKind::from_str_loose("unknown"), None);
    }

    #[test]
    fn unknown_language_returns_empty() {
        assert!(extract_symbols("anything", Language::Unknown).is_empty());
    }

    #[test]
    fn signature_truncates_at_brace() {
        let source = "fn hello(x: i32) {\n    x + 1\n}\n";
        let symbols = extract_symbols(source, Language::Rust);
        assert_eq!(symbols[0].signature, "fn hello(x: i32)");
    }

    #[test]
    fn replace_function_signature_basic() {
        let src = "fn old(a: i32) -> i32 { a }\nfn other() {}";
        let res = replace_function_signature(src, "old", "pub fn new(b: u32) -> u32");
        let out = res.expect("replace_function_signature should succeed for matching name");
        assert!(out.contains("pub fn new(b: u32) -> u32"));
        assert!(out.contains("fn other"));
        assert!(!out.contains("fn old"));
        // Regression: body must be preserved (was deleted when using "body"
        // node kind instead of "block").
        assert!(
            out.contains("{ a }"),
            "function body should be preserved: {out}"
        );
    }

    #[test]
    fn extract_typescript_symbols() {
        let source = r#"
class Foo {
    greet(name: string): string {
        return `Hello, ${name}`;
    }

    farewell(): void {
        console.log("bye");
    }
}

function bar(x: number): number {
    return x * 2;
}

interface Baz {
    id: number;
    name: string;
}

enum Status {
    Active,
    Inactive,
    Pending,
}

const MAX_RETRIES = 5;
"#;
        let symbols = extract_symbols(source, Language::TypeScript);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "should find class Foo");
        assert!(names.contains(&"bar"), "should find function bar");
        assert!(names.contains(&"Baz"), "should find interface Baz");
        assert!(names.contains(&"Status"), "should find enum Status");
        assert!(
            names.contains(&"MAX_RETRIES"),
            "should find const MAX_RETRIES"
        );

        // Class Foo should have methods as children
        let class_foo = symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(class_foo.kind, SymbolKind::Class);
        let child_names: Vec<&str> = class_foo.children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            child_names.contains(&"greet"),
            "Foo should contain method greet"
        );
        assert!(
            child_names.contains(&"farewell"),
            "Foo should contain method farewell"
        );

        // Interface
        let iface = symbols.iter().find(|s| s.name == "Baz").unwrap();
        assert_eq!(iface.kind, SymbolKind::Interface);

        // Enum
        let status = symbols.iter().find(|s| s.name == "Status").unwrap();
        assert_eq!(status.kind, SymbolKind::Enum);
    }

    /// Multi-declarator `const a = 1, b = 2, c = 3;` should emit one symbol
    /// per declarator, not just the first one (#1104).
    #[test]
    fn extract_typescript_multi_declarator_const() {
        let source = "const a = 1, b = 2, c = 3;\n";
        let symbols = extract_symbols(source, Language::TypeScript);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"), "should find const a: {names:?}");
        assert!(names.contains(&"b"), "should find const b: {names:?}");
        assert!(names.contains(&"c"), "should find const c: {names:?}");
        assert_eq!(
            symbols.len(),
            3,
            "exactly 3 symbols for 3 declarators: {names:?}"
        );
    }

    /// `let` and `var` declarators should NOT be extracted (only `const`).
    #[test]
    fn extract_typescript_let_var_not_extracted() {
        let source = "let x = 1, y = 2;\nvar z = 3;\n";
        let symbols = extract_symbols(source, Language::TypeScript);
        assert!(
            symbols.is_empty(),
            "let/var should not produce symbols: {:?}",
            symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn extract_java_symbols() {
        let source = r#"
public class Foo {
    private int count;

    public void bar() {
        System.out.println("hello");
    }

    public int getCount() {
        return count;
    }
}

interface Baz {
    void process();
    String getName();
}

enum Status {
    ACTIVE,
    INACTIVE,
    PENDING
}
"#;
        let symbols = extract_symbols(source, Language::Java);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "should find class Foo");
        assert!(names.contains(&"Baz"), "should find interface Baz");
        assert!(names.contains(&"Status"), "should find enum Status");

        // Class Foo should have method children
        let class_foo = symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(class_foo.kind, SymbolKind::Class);
        let child_names: Vec<&str> = class_foo.children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            child_names.contains(&"bar"),
            "Foo should contain method bar"
        );
        assert!(
            child_names.contains(&"getCount"),
            "Foo should contain method getCount"
        );

        // Interface
        let iface = symbols.iter().find(|s| s.name == "Baz").unwrap();
        assert_eq!(iface.kind, SymbolKind::Interface);

        // Enum
        let status = symbols.iter().find(|s| s.name == "Status").unwrap();
        assert_eq!(status.kind, SymbolKind::Enum);
    }

    #[test]
    fn extract_c_symbols() {
        let source = r#"
#include <stdio.h>

void foo(int x) {
    printf("%d\n", x);
}

int calculate(int a, int b) {
    return a + b;
}

struct Bar {
    int x;
    int y;
    char name[64];
};

enum Color {
    RED,
    GREEN,
    BLUE
};
"#;
        let symbols = extract_symbols(source, Language::C);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"), "should find function foo");
        assert!(
            names.contains(&"calculate"),
            "should find function calculate"
        );
        assert!(names.contains(&"Bar"), "should find struct Bar");
        assert!(names.contains(&"Color"), "should find enum Color");

        // Check kinds
        let foo_sym = symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(foo_sym.kind, SymbolKind::Function);

        let bar_sym = symbols.iter().find(|s| s.name == "Bar").unwrap();
        assert_eq!(bar_sym.kind, SymbolKind::Struct);

        let color_sym = symbols.iter().find(|s| s.name == "Color").unwrap();
        assert_eq!(color_sym.kind, SymbolKind::Enum);
    }

    /// Regression: C functions returning pointer types (e.g. `int *func()`,
    /// `void *func()`, `Foo *func()`) were either missing from `ast list`
    /// or mislabeled with the return type name. The root cause was that
    /// `extract_generic` checked direct `type_identifier` children (the
    /// return type) before checking declarator children (the function name),
    /// and the declarator recursion was only one level deep while pointer
    /// declarators nest two levels: `pointer_declarator -> function_declarator
    /// -> identifier`.
    #[test]
    fn extract_c_pointer_returning_functions() {
        let source = r#"
int *pointer_func(void) { return 0; }
void *void_ptr_func(void) { return 0; }
char *string_func(void) { return "hello"; }

typedef struct Foo { int x; } Foo;
Foo *foo_create(void) { return 0; }
"#;
        let symbols = extract_symbols(source, Language::C);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"pointer_func"),
            "should find int* function: {names:?}"
        );
        assert!(
            names.contains(&"void_ptr_func"),
            "should find void* function: {names:?}"
        );
        assert!(
            names.contains(&"string_func"),
            "should find char* function: {names:?}"
        );
        assert!(
            names.contains(&"foo_create"),
            "should find Foo* function (not mislabeled as 'Foo'): {names:?}"
        );

        // Verify they are classified as functions, not structs/types
        for fname in &["pointer_func", "void_ptr_func", "string_func", "foo_create"] {
            let sym = symbols.iter().find(|s| s.name == *fname).unwrap();
            assert_eq!(
                sym.kind,
                SymbolKind::Function,
                "{fname} should be Function, got {:?}",
                sym.kind
            );
        }
    }

    #[test]
    fn extract_cpp_symbols() {
        let source = r#"
#include <iostream>
#include <string>

class Engine {
public:
    void start() {
        std::cout << "started" << std::endl;
    }

    int getSpeed() const {
        return speed;
    }

private:
    int speed;
};

namespace utils {
    int helper(int x) {
        return x + 1;
    }
}

struct Point {
    double x;
    double y;
};
"#;
        let symbols = extract_symbols(source, Language::Cpp);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Engine"), "should find class Engine");
        assert!(names.contains(&"Point"), "should find struct Point");
        // Namespace may or may not be detected depending on grammar; check class details
        let engine = symbols.iter().find(|s| s.name == "Engine").unwrap();
        assert_eq!(engine.kind, SymbolKind::Class);
    }

    #[test]
    fn extract_cpp_qualified_method() {
        let source = "void MyClass::process(int x) {\n    // body\n}\n";
        let symbols = extract_symbols(source, Language::Cpp);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"process"),
            "should find qualified method process, got: {names:?}"
        );
    }

    // ── full_symbol_span tests ────────────────────────────────────

    #[test]
    fn full_span_no_attributes() {
        let source = "fn foo() {\n    42\n}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = &symbols[0];
        let (start, end) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(start, sym.start_line);
        assert_eq!(end, sym.end_line);
    }

    #[test]
    fn full_span_single_attribute() {
        let source = "#[test]\nfn foo() {\n    42\n}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = &symbols[0];
        assert_eq!(sym.start_line, 2); // tree-sitter sees fn on line 2
        let (start, end) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(start, 1); // includes #[test]
        assert_eq!(end, sym.end_line);
    }

    #[test]
    fn full_span_stacked_attributes() {
        let source = "#[test]\n#[cfg(unix)]\nfn foo() {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = &symbols[0];
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(start, 1); // includes both attributes
    }

    #[test]
    fn full_span_doc_comment() {
        let source = "/// This is a doc comment.\n/// Second line.\nfn foo() {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = &symbols[0];
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(start, 1);
    }

    #[test]
    fn full_span_mixed_attrs_and_docs() {
        let source = "/// A doc comment.\n#[test]\n#[cfg(unix)]\nfn foo() {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = &symbols[0];
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(start, 1);
    }

    #[test]
    fn full_span_python_decorator() {
        let source = "@staticmethod\ndef foo():\n    pass\n";
        let symbols = extract_symbols(source, Language::Python);
        let sym = &symbols[0];
        let (start, _) = full_symbol_span(source, sym, Language::Python);
        assert_eq!(start, 1);
    }

    // Regression: multi-line Python decorators with continuation lines
    // were truncated because only lines starting with '@' matched (#1103).
    #[test]
    fn full_span_python_multiline_decorator() {
        let source = "\
@decorator(
    arg1,
    arg2
)
def foo():
    pass
";
        let symbols = extract_symbols(source, Language::Python);
        let sym = symbols.iter().find(|s| s.name == "foo").unwrap();
        let (start, _) = full_symbol_span(source, sym, Language::Python);
        assert_eq!(
            start, 1,
            "multiline @decorator(...) should be included in foo's span"
        );
    }

    #[test]
    fn full_span_python_stacked_multiline_decorator() {
        let source = "\
@first_decorator
@second_decorator(
    option=True
)
def bar():
    pass
";
        let symbols = extract_symbols(source, Language::Python);
        let sym = symbols.iter().find(|s| s.name == "bar").unwrap();
        let (start, _) = full_symbol_span(source, sym, Language::Python);
        assert_eq!(
            start, 1,
            "both decorators (including multiline) should be included"
        );
    }

    // Regression: //! inner doc comments belong to the module, not the
    // following symbol. full_symbol_span must not absorb them.
    #[test]
    fn full_span_excludes_inner_doc_comments() {
        let source = "//! Module-level doc.\nstruct Config {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = symbols.iter().find(|s| s.name == "Config").unwrap();
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(
            start, 2,
            "inner doc comment //! should not be included in Config's span"
        );
    }

    #[test]
    fn full_span_stops_at_unrelated_code() {
        let source = "fn bar() {}\n\n#[test]\nfn foo() {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let foo = symbols.iter().find(|s| s.name == "foo").unwrap();
        let (start, _) = full_symbol_span(source, foo, Language::Rust);
        assert_eq!(start, 3); // #[test] on line 3, not line 1
    }

    // Regression: multiline Rust attributes like #[cfg_attr(\n  ...\n)]
    // were not recognized by the backward walk because is_annotation_line
    // only matches lines starting with #[.  The fix uses bracket depth
    // tracking to find the opening #[ from interior/closing lines.
    #[test]
    fn full_span_multiline_rust_attribute() {
        let source = "\
#[cfg_attr(
    feature = \"serde\",
    derive(Serialize, Deserialize)
)]
struct Config {
    name: String,
}
";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = symbols.iter().find(|s| s.name == "Config").unwrap();
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(
            start, 1,
            "multiline #[cfg_attr(...)] should be included in Config's span"
        );
    }

    #[test]
    fn full_span_multiline_attribute_with_stacked_single() {
        let source = "\
#[derive(Debug)]
#[cfg_attr(
    feature = \"serde\",
    derive(Serialize)
)]
pub struct Foo {}
";
        let symbols = extract_symbols(source, Language::Rust);
        let sym = symbols.iter().find(|s| s.name == "Foo").unwrap();
        let (start, _) = full_symbol_span(source, sym, Language::Rust);
        assert_eq!(
            start, 1,
            "both #[derive(Debug)] and the multiline #[cfg_attr] should be included"
        );
    }

    #[test]
    fn extract_symbol_text_basic() {
        let source = "#[test]\nfn foo() {\n    42\n}\n\nfn bar() {}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let foo = symbols.iter().find(|s| s.name == "foo").unwrap();
        let text = extract_symbol_text(source, foo, Language::Rust);
        assert!(text.contains("#[test]"));
        assert!(text.contains("fn foo()"));
        assert!(!text.contains("fn bar"));
    }

    // Regression: extract_symbol_text used a global line-ending heuristic
    // that produced wrong byte offsets on files with mixed \r\n and \n endings.
    #[test]
    fn extract_symbol_text_mixed_line_endings() {
        // First line uses \r\n, rest use \n.
        let source = "fn first() {}\r\nfn second() {\n    42\n}\n";
        let symbols = extract_symbols(source, Language::Rust);
        let second = symbols.iter().find(|s| s.name == "second").unwrap();
        let text = extract_symbol_text(source, second, Language::Rust);
        assert!(
            text.contains("fn second()"),
            "should contain fn second(): {text:?}"
        );
        assert!(
            !text.contains("fn first()"),
            "should not contain fn first(): {text:?}"
        );
    }

    #[test]
    fn extract_symbol_text_crlf() {
        // CRLF line endings: str::lines() strips \r\n but each line's
        // byte offset must account for 2-byte endings, not 1.
        let source = "fn first() {}\r\nfn second() {\r\n    42\r\n}\r\n";
        let symbols = extract_symbols(source, Language::Rust);
        let second = symbols.iter().find(|s| s.name == "second").unwrap();
        let text = extract_symbol_text(source, second, Language::Rust);
        assert!(
            text.contains("fn second()"),
            "extracted text should contain 'fn second()', got: {:?}",
            text
        );
        assert!(
            !text.contains("fn first()"),
            "extracted text should not contain 'fn first()', got: {:?}",
            text
        );
    }

    #[test]
    fn go_receiver_methods_grouped_under_struct() {
        let source = "\
package main

type Server struct {
\tHost string
}

func NewServer() *Server { return nil }

func (s *Server) Start() error { return nil }

func (s *Server) Stop() {}
";
        let syms = extract_symbols(source, Language::Go);
        let server = syms.iter().find(|s| s.name == "Server").unwrap();
        assert_eq!(server.kind, SymbolKind::Struct);
        assert_eq!(
            server.children.len(),
            2,
            "Server should have 2 method children, got: {:?}",
            server.children.iter().map(|c| &c.name).collect::<Vec<_>>()
        );
        assert!(server.children.iter().any(|c| c.name == "Start"));
        assert!(server.children.iter().any(|c| c.name == "Stop"));
        // NewServer is a free function, not a method
        assert!(
            syms.iter()
                .any(|s| s.name == "NewServer" && s.kind == SymbolKind::Function)
        );
    }

    #[test]
    fn go_qualified_name_lookup_works() {
        let source = "\
package main

type Dog struct{}
func (d *Dog) Speak() string { return \"woof\" }

type Cat struct{}
func (c *Cat) Speak() string { return \"meow\" }
";
        let syms = extract_symbols(source, Language::Go);
        // Qualified lookup should disambiguate
        let dog_speak = find_symbol(&syms, "Dog::Speak").expect("Dog::Speak should be found");
        assert!(dog_speak.signature.contains("Dog"));

        let cat_speak = find_symbol(&syms, "Cat::Speak").expect("Cat::Speak should be found");
        assert!(cat_speak.signature.contains("Cat"));
    }

    #[test]
    fn go_value_receiver_grouped() {
        let source = "\
package main

type Counter struct{ n int }

func (c Counter) Count() int { return c.n }
";
        let syms = extract_symbols(source, Language::Go);
        let counter = syms.iter().find(|s| s.name == "Counter").unwrap();
        assert_eq!(counter.children.len(), 1);
        assert_eq!(counter.children[0].name, "Count");
    }

    #[test]
    fn go_method_without_matching_struct_stays_top_level() {
        // Receiver type not defined in this file
        let source = "\
package main

func (e *External) DoWork() {}
";
        let syms = extract_symbols(source, Language::Go);
        assert!(
            syms.iter()
                .any(|s| s.name == "DoWork" && s.kind == SymbolKind::Method),
            "orphan method should stay top-level"
        );
    }
}
