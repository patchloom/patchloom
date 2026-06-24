//! Symbol extraction from source files using tree-sitter AST parsing.

use std::path::Path;

use serde::Serialize;

use super::{Language, child_text_by_kind, child_text_by_kinds, parse_source};

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
    visit_node(&mut cursor, source, lang, 0, &mut symbols);
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
    if let Some((parent, child)) = name.split_once("::") {
        // Qualified name: find parent, then child
        for sym in symbols {
            if sym.name == parent {
                for c in &sym.children {
                    if c.name == child {
                        return Some(c);
                    }
                }
            }
        }
        None
    } else {
        // Unqualified: search top-level, then children
        for sym in symbols {
            if sym.name == name {
                return Some(sym);
            }
        }
        for sym in symbols {
            for c in &sym.children {
                if c.name == name {
                    return Some(c);
                }
            }
        }
        None
    }
}

/// Tree-sitter based helper for function signature updates (Rust focus for now).
/// Locates the exact `function_item` node by identifier, replaces only the signature
/// portion (up to body or semicolon) with `new_sig`.
/// Preserves the rest of the source exactly. This is much safer than line-scan.
/// For other languages or full attrs/generics/docs, extend with queries.
pub fn replace_function_signature(source: &str, old_name: &str, new_sig: &str) -> Option<String> {
    let (tree, _) = parse_source(source, Language::Rust)?;
    let root = tree.root_node();

    // Find function_item whose identifier matches
    fn find_fn<'a>(
        node: tree_sitter_lib::Node<'a>,
        source: &str,
        old_name: &str,
    ) -> Option<tree_sitter_lib::Node<'a>> {
        if node.kind() == "function_item"
            && let Some(id) = child_text_by_kind(node, "identifier", source)
            && id == old_name
        {
            return Some(node);
        }
        let mut c = node.walk();
        for child in node.children(&mut c) {
            if let Some(found) = find_fn(child, source, old_name) {
                return Some(found);
            }
        }
        None
    }

    let fn_node = find_fn(root, source, old_name)?;

    // Signature is from start of node to start of body (or end if no body, e.g. decl)
    let sig_end = if let Some(_body) = child_text_by_kind(fn_node, "body", source) {
        // body starts after sig; find byte offset of body in source? Use node children.
        // Simpler: find the '{' or ';' position after params/return.
        let mut end_byte = fn_node.end_byte();
        let mut c2 = fn_node.walk();
        for ch in fn_node.children(&mut c2) {
            if ch.kind() == "body" || ch.kind() == ";" {
                end_byte = ch.start_byte();
                break;
            }
        }
        end_byte
    } else {
        fn_node.end_byte()
    };

    let start = fn_node.start_byte();
    let before = &source[..start];
    let after = &source[sig_end..];
    Some(format!("{}{}{}", before, new_sig, after))
}

/// Structured edits for parts of a function signature.
#[derive(Debug, Clone, Default)]
pub struct FunctionSigEdit {
    /// New visibility (e.g. "pub", "pub(crate)", or "" for private).
    pub visibility: Option<String>,
    /// New parameter list including parens (e.g. "(x: i32, y: &str)").
    pub parameters: Option<String>,
    /// New return type including arrow if present (e.g. "-> String").
    pub return_type: Option<String>,
}

/// Rewrite function signature with structured changes for visibility, parameters, return type.
/// Preserves function name, body, and other source exactly. Uses tree-sitter for location.
/// Supports basic Rust signatures (no generics/where for the stub; extend as needed).
/// See #797.
///
/// Per #821: this remains a library-only helper for now (no CLI `ast signature`, no plan op, no MCP tool).
/// Record of decision lives in #821 and src/lib.rs embedding docs.
pub fn rewrite_function_signature(
    source: &str,
    old_name: &str,
    edit: &FunctionSigEdit,
) -> Option<String> {
    let (tree, _) = parse_source(source, Language::Rust)?;
    let root = tree.root_node();

    let fn_node = find_fn_for_rewrite(root, source, old_name)?;

    // Build replacement sig from edit or keep original parts.
    let vis = edit
        .visibility
        .as_deref()
        .unwrap_or_else(|| child_text_by_kind(fn_node, "visibility", source).unwrap_or(""));
    let params = edit
        .parameters
        .as_deref()
        .unwrap_or_else(|| child_text_by_kind(fn_node, "parameters", source).unwrap_or("()"));
    let ret = edit
        .return_type
        .as_deref()
        .unwrap_or_else(|| child_text_by_kind(fn_node, "return_type", source).unwrap_or(""));

    let vis_part = if vis.is_empty() {
        String::new()
    } else {
        format!("{} ", vis)
    };
    let ret_part = if ret.is_empty() {
        String::new()
    } else {
        format!(" {}", ret)
    };
    let new_sig = format!("{}fn {}{}{}", vis_part, old_name, params, ret_part);

    // Use the same byte-range logic as replace to swap only the sig.
    let sig_end = if let Some(_body) = child_text_by_kind(fn_node, "body", source) {
        let mut end_byte = fn_node.end_byte();
        let mut c2 = fn_node.walk();
        for ch in fn_node.children(&mut c2) {
            if ch.kind() == "body" || ch.kind() == ";" {
                end_byte = ch.start_byte();
                break;
            }
        }
        end_byte
    } else {
        fn_node.end_byte()
    };

    let start = fn_node.start_byte();
    let before = &source[..start];
    let after = &source[sig_end..];
    Some(format!("{}{}{}", before, new_sig, after))
}

// Small helper duplicated from internal for rewrite (to avoid exposing private).
fn find_fn_for_rewrite<'a>(
    node: tree_sitter_lib::Node<'a>,
    source: &str,
    old_name: &str,
) -> Option<tree_sitter_lib::Node<'a>> {
    if node.kind() == "function_item"
        && let Some(id) = child_text_by_kind(node, "identifier", source)
        && id == old_name
    {
        return Some(node);
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if let Some(found) = find_fn_for_rewrite(child, source, old_name) {
            return Some(found);
        }
    }
    None
}

fn visit_node(
    cursor: &mut tree_sitter_lib::TreeCursor,
    source: &str,
    language: Language,
    depth: usize,
    symbols: &mut Vec<SymbolDef>,
) {
    let node = cursor.node();
    if let Some(mut sym) = try_extract_symbol(node, source, language, depth) {
        // Collect children inside this symbol's scope
        if cursor.goto_first_child() {
            loop {
                visit_node(cursor, source, language, depth + 1, &mut sym.children);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
        symbols.push(sym);
    } else if cursor.goto_first_child() {
        loop {
            visit_node(cursor, source, language, depth, symbols);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn try_extract_symbol(
    node: tree_sitter_lib::Node,
    source: &str,
    language: Language,
    depth: usize,
) -> Option<SymbolDef> {
    let (kind, name) = match language {
        Language::Rust => extract_rust(node, source)?,
        Language::Python => extract_python(node, source)?,
        Language::TypeScript | Language::JavaScript => extract_ts_js(node, source, language)?,
        Language::Go => extract_go(node, source)?,
        Language::Hcl => extract_hcl(node, source)?,
        Language::Protobuf => extract_proto(node, source)?,
        Language::Shell => extract_bash(node, source)?,
        _ => extract_generic(node, source)?,
    };

    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let signature = node_signature(node, source);

    Some(SymbolDef {
        name,
        kind,
        start_line,
        end_line,
        signature,
        children: Vec::new(),
        depth,
    })
}

fn node_signature(node: tree_sitter_lib::Node, source: &str) -> String {
    let start = node.start_byte();
    let mut end = node.end_byte().min(start + 200);
    while end > start && !source.is_char_boundary(end) {
        end -= 1;
    }
    let raw = &source[start..end];
    let sig = match raw.find('{') {
        Some(brace) => raw[..brace].trim(),
        None => raw.lines().next().unwrap_or(raw).trim(),
    };
    sig.to_string()
}

// ---------------------------------------------------------------------------
// Language-specific extractors (matching bline's patterns)
// ---------------------------------------------------------------------------

fn extract_rust(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "function_item" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Function, name.to_string()))
        }
        "struct_item" => {
            let name = child_text_by_kind(node, "type_identifier", source)?;
            Some((SymbolKind::Struct, name.to_string()))
        }
        "enum_item" => {
            let name = child_text_by_kind(node, "type_identifier", source)?;
            Some((SymbolKind::Enum, name.to_string()))
        }
        "trait_item" => {
            let name = child_text_by_kind(node, "type_identifier", source)?;
            Some((SymbolKind::Trait, name.to_string()))
        }
        "impl_item" => {
            let name = child_text_by_kind(node, "type_identifier", source)?;
            Some((SymbolKind::Impl, name.to_string()))
        }
        "const_item" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Const, name.to_string()))
        }
        "type_item" => {
            let name = child_text_by_kind(node, "type_identifier", source)?;
            Some((SymbolKind::Type, name.to_string()))
        }
        "mod_item" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Module, name.to_string()))
        }
        _ => None,
    }
}

fn extract_python(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "function_definition" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            let kind = if node
                .parent()
                .and_then(|p| p.parent())
                .is_some_and(|gp| gp.kind() == "class_definition")
            {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            Some((kind, name.to_string()))
        }
        "class_definition" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Class, name.to_string()))
        }
        _ => None,
    }
}

fn extract_ts_js(
    node: tree_sitter_lib::Node,
    source: &str,
    language: Language,
) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "function_declaration" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Function, name.to_string()))
        }
        "class_declaration" => {
            let name = child_text_by_kinds(node, &["type_identifier", "identifier"], source)?;
            Some((SymbolKind::Class, name.to_string()))
        }
        "method_definition" => {
            let name = child_text_by_kind(node, "property_identifier", source)?;
            Some((SymbolKind::Method, name.to_string()))
        }
        "interface_declaration" if language == Language::TypeScript => {
            let name = child_text_by_kinds(node, &["type_identifier", "identifier"], source)?;
            Some((SymbolKind::Interface, name.to_string()))
        }
        "lexical_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    if let Some(first) = node.child(0)
                        && first.utf8_text(source.as_bytes()).ok() != Some("const")
                    {
                        return None;
                    }
                    let name = child_text_by_kind(child, "identifier", source)?;
                    return Some((SymbolKind::Const, name.to_string()));
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_go(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "function_declaration" => {
            let name = child_text_by_kind(node, "identifier", source)?;
            Some((SymbolKind::Function, name.to_string()))
        }
        "method_declaration" => {
            let name = child_text_by_kinds(node, &["field_identifier", "identifier"], source)?;
            Some((SymbolKind::Method, name.to_string()))
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    let name =
                        child_text_by_kinds(child, &["type_identifier", "identifier"], source)?;
                    let mut inner = child.walk();
                    for grandchild in child.children(&mut inner) {
                        match grandchild.kind() {
                            "struct_type" => {
                                return Some((SymbolKind::Struct, name.to_string()));
                            }
                            "interface_type" => {
                                return Some((SymbolKind::Interface, name.to_string()));
                            }
                            _ => {}
                        }
                    }
                    return Some((SymbolKind::Type, name.to_string()));
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_hcl(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    if node.kind() != "block" {
        return None;
    }
    let block_type = child_text_by_kind(node, "identifier", source)?;
    let mut labels = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "string_lit"
            && let Ok(text) = child.utf8_text(source.as_bytes())
        {
            labels.push(text.trim_matches('"').to_string());
        }
    }
    let (kind, name) = match block_type {
        "resource" | "data" => {
            let name = if labels.len() >= 2 {
                format!("{}.{}", labels[0], labels[1])
            } else if !labels.is_empty() {
                labels[0].clone()
            } else {
                return None;
            };
            (SymbolKind::Struct, name)
        }
        "variable" | "output" => {
            let name = labels.first()?.clone();
            (SymbolKind::Const, name)
        }
        "module" | "provider" => {
            let name = labels
                .first()
                .cloned()
                .unwrap_or_else(|| block_type.to_string());
            (SymbolKind::Module, name)
        }
        "locals" | "terraform" => (SymbolKind::Module, block_type.to_string()),
        _ => return None,
    };
    Some((kind, name))
}

fn extract_proto(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "message" => {
            let name = child_text_by_kinds(node, &["message_name", "identifier"], source)?;
            Some((SymbolKind::Struct, name.to_string()))
        }
        "enum" => {
            let name = child_text_by_kinds(node, &["enum_name", "identifier"], source)?;
            Some((SymbolKind::Enum, name.to_string()))
        }
        "service" => {
            let name = child_text_by_kinds(node, &["service_name", "identifier"], source)?;
            Some((SymbolKind::Interface, name.to_string()))
        }
        "rpc" => {
            let name = child_text_by_kinds(node, &["rpc_name", "identifier"], source)?;
            Some((SymbolKind::Method, name.to_string()))
        }
        _ => None,
    }
}

fn extract_bash(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    if node.kind() != "function_definition" {
        return None;
    }
    let name = child_text_by_kinds(node, &["word", "name", "identifier"], source)?;
    Some((SymbolKind::Function, name.to_string()))
}

// Generic extractor for languages without a hand-tuned extractor

const GENERIC_NAME_KINDS: &[&str] = &[
    "identifier",
    "name",
    "type_identifier",
    "property_identifier",
    "field_identifier",
    "simple_identifier",
];

fn extract_generic(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    let kind = node.kind();
    let symbol_kind = match kind {
        "function_item" | "function_definition" | "function_declaration" => SymbolKind::Function,
        "method_definition" | "method_declaration" => SymbolKind::Method,
        "class_definition" | "class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "struct_item" | "struct_declaration" | "struct_specifier" => SymbolKind::Struct,
        "enum_item" | "enum_declaration" | "enum_specifier" => SymbolKind::Enum,
        "type_declaration" | "type_item" | "type_alias_declaration" => SymbolKind::Type,
        "module_declaration" | "mod_item" | "namespace_declaration" => SymbolKind::Module,
        "trait_item" | "trait_declaration" | "protocol_declaration" => SymbolKind::Trait,
        _ => return None,
    };

    if let Some(name) = child_text_by_kinds(node, GENERIC_NAME_KINDS, source) {
        return Some((symbol_kind, name.to_string()));
    }

    // C-family: name nested inside a declarator node
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind().contains("declarator") || child.kind() == "name")
            && let Some(name) = child_text_by_kinds(child, GENERIC_NAME_KINDS, source)
        {
            return Some((symbol_kind, name.to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn rewrite_function_signature_structured() {
        let src = "fn old(a: i32) -> i32 { a }\nfn other() {}";
        let edit = FunctionSigEdit {
            visibility: Some("pub(crate)".to_string()),
            parameters: Some("(x: u32, y: &str)".to_string()),
            return_type: Some("-> String".to_string()),
        };
        let res = rewrite_function_signature(src, "old", &edit);
        let out = res.expect("rewrite_function_signature should succeed for matching name");
        assert!(out.contains("pub(crate) fn old(x: u32, y: &str) -> String"));
        assert!(out.contains("fn other"));
        assert!(!out.contains("fn old(a: i32)"));
    }
}
