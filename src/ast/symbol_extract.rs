//! Per-language **tree-sitter visitors** that map AST nodes to [`SymbolDef`]s.
//!
//! This is *not* "extract a symbol to another file" (see [`crate::ast::extract_to_file`]).
//!
//! Each function maps language-specific tree-sitter node kinds to
//! [`SymbolKind`] + name pairs. The top-level [`visit_node`] walks the
//! tree recursively and delegates to the correct extractor.

use super::symbols::{SymbolDef, SymbolKind};
use super::{Language, child_text_by_kind, child_text_by_kinds};

/// Recursively visit tree-sitter nodes and collect symbol definitions.
pub(crate) fn visit_node(
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
        Language::Ruby => extract_ruby(node, source)?,
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
    // Find the body-opening `{` that is NOT inside parentheses or brackets.
    // This avoids truncating at destructured parameters like `function foo({a, b}) {`.
    let mut depth: i32 = 0;
    let mut body_brace = None;
    for (i, ch) in raw.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            '{' if depth == 0 => {
                body_brace = Some(i);
                break;
            }
            _ => {}
        }
    }
    let sig = match body_brace {
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
            // Use the `type` field to get the impl target, not the first
            // `type_identifier` child.  For `impl Display for Foo`, the
            // first `type_identifier` is the trait (`Display`), but the
            // `type` field always points to the target type (`Foo`).
            let type_node = node.child_by_field_name("type")?;
            let name = type_node.utf8_text(source.as_bytes()).ok()?;
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
            // Walk up ancestors to detect if this is a method inside a class.
            // Non-decorated: function_definition -> block -> class_definition
            // Decorated: function_definition -> decorated_definition -> block -> class_definition
            let kind = {
                let mut ancestor = node.parent();
                let mut is_method = false;
                while let Some(a) = ancestor {
                    if a.kind() == "class_definition" {
                        is_method = true;
                        break;
                    }
                    ancestor = a.parent();
                }
                if is_method {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                }
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
        "enum_declaration" if language == Language::TypeScript => {
            let name = child_text_by_kinds(node, &["type_identifier", "identifier"], source)?;
            Some((SymbolKind::Enum, name.to_string()))
        }
        // Match individual variable_declarator nodes instead of the parent
        // lexical_declaration so that `const a = 1, b = 2` emits one symbol
        // per declarator (#1104). visit_node recurses into lexical_declaration
        // children naturally (same pattern as Go grouped type declarations).
        "variable_declarator" => {
            if let Some(parent) = node.parent()
                && parent.kind() == "lexical_declaration"
                && let Some(first) = parent.child(0)
                && first.utf8_text(source.as_bytes()).ok() == Some("const")
            {
                let name = child_text_by_kind(node, "identifier", source)?;
                Some((SymbolKind::Const, name.to_string()))
            } else {
                None
            }
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
        // Match `type_spec` and `type_alias` instead of `type_declaration` so
        // grouped `type ( A struct{...}; B = int )` blocks emit one symbol
        // per spec.  `visit_node` naturally recurses into the parent
        // `type_declaration`.
        "type_spec" => {
            let name = child_text_by_kinds(node, &["type_identifier", "identifier"], source)?;
            let mut inner = node.walk();
            for grandchild in node.children(&mut inner) {
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
            Some((SymbolKind::Type, name.to_string()))
        }
        "type_alias" => {
            let name = child_text_by_kinds(node, &["type_identifier", "identifier"], source)?;
            Some((SymbolKind::Type, name.to_string()))
        }
        _ => None,
    }
}

/// Group Go receiver methods under their receiver type.
///
/// Go methods with receivers (e.g. `func (s *Server) Start()`) are
/// syntactically top-level in the AST. This groups them as children of the
/// matching struct/type/interface so qualified lookups like `Server::Start`
/// and `ast list` nesting work correctly.
pub(crate) fn group_go_receiver_methods(symbols: &mut Vec<SymbolDef>) {
    let mut to_move: Vec<(usize, String)> = Vec::new();
    for (i, sym) in symbols.iter().enumerate() {
        if sym.kind == SymbolKind::Method
            && let Some(receiver_type) = parse_go_receiver_type(&sym.signature)
        {
            to_move.push((i, receiver_type));
        }
    }

    // Process in reverse index order so removals don't shift earlier indices.
    for (idx, receiver_type) in to_move.into_iter().rev() {
        if let Some(parent_idx) = symbols.iter().position(|s| {
            matches!(
                s.kind,
                SymbolKind::Struct | SymbolKind::Type | SymbolKind::Interface
            ) && s.name == receiver_type
        }) {
            let mut method = symbols.remove(idx);
            let adj = if idx < parent_idx {
                parent_idx - 1
            } else {
                parent_idx
            };
            method.depth = symbols[adj].depth + 1;
            symbols[adj].children.push(method);
        }
    }
}

/// Parse the Go receiver type from a method signature string.
///
/// - `func (s *Server) Start()` -> `Some("Server")`
/// - `func (s Server) Start()`  -> `Some("Server")`
/// - `func (*Server) Method()`  -> `Some("Server")`
/// - `func main()`              -> `None`
fn parse_go_receiver_type(signature: &str) -> Option<String> {
    let after_func = signature.strip_prefix("func ")?.trim_start();
    let receiver_part = after_func.strip_prefix('(')?;
    let end_paren = receiver_part.find(')')?;
    let receiver = receiver_part[..end_paren].trim();
    if receiver.is_empty() {
        return None;
    }
    let type_part = receiver.split_whitespace().next_back()?;
    let type_name = type_part.strip_prefix('*').unwrap_or(type_part);
    // Strip generic parameters: Type[T] -> Type
    let type_name = type_name.split('[').next().unwrap_or(type_name);
    if type_name.is_empty() {
        return None;
    }
    Some(type_name.to_string())
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

fn extract_ruby(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "class" => {
            // Ruby class names use `constant` nodes, not `identifier`.
            let name = child_text_by_kinds(node, &["constant", "scope_resolution"], source)?;
            Some((SymbolKind::Class, name.to_string()))
        }
        "module" => {
            let name = child_text_by_kinds(node, &["constant", "scope_resolution"], source)?;
            Some((SymbolKind::Module, name.to_string()))
        }
        "method" | "singleton_method" => {
            let name = child_text_by_kinds(node, &["identifier", "setter", "operator"], source)?;
            Some((SymbolKind::Method, name.to_string()))
        }
        _ => None,
    }
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

/// Name kinds safe for function names (excludes `type_identifier` which matches
/// C/C++ return types like `Queue` in `Queue *create()`).
const GENERIC_FN_NAME_KINDS: &[&str] = &[
    "identifier",
    "name",
    "property_identifier",
    "field_identifier",
    "simple_identifier",
];

fn extract_generic(node: tree_sitter_lib::Node, source: &str) -> Option<(SymbolKind, String)> {
    let kind = node.kind();
    let is_function_kind = matches!(
        kind,
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
    );
    let symbol_kind = match kind {
        "function_item" | "function_definition" | "function_declaration" => SymbolKind::Function,
        "method_definition" | "method_declaration" => SymbolKind::Method,
        "class_definition" | "class_declaration" | "class_specifier" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "struct_item" | "struct_declaration" | "struct_specifier" => SymbolKind::Struct,
        "enum_item" | "enum_declaration" | "enum_specifier" => SymbolKind::Enum,
        "type_declaration" | "type_item" | "type_alias_declaration" => SymbolKind::Type,
        "module_declaration" | "mod_item" | "namespace_declaration" | "namespace_definition" => {
            SymbolKind::Module
        }
        "trait_item" | "trait_declaration" | "protocol_declaration" => SymbolKind::Trait,
        _ => return None,
    };

    if is_function_kind {
        // For function definitions, check declarators FIRST because in C/C++
        // the function name is inside a declarator node, and `type_identifier`
        // direct children are the return type (e.g. `Queue` in `Queue *create()`).
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if (child.kind().contains("declarator") || child.kind() == "name")
                && let Some(name) = find_name_in_declarator_tree(child, GENERIC_NAME_KINDS, source)
            {
                return Some((symbol_kind, name.to_string()));
            }
        }
        // Fall back to direct identifier children for non-C languages (Swift,
        // Kotlin, etc.) but exclude `type_identifier` to avoid matching return types.
        if let Some(name) = child_text_by_kinds(node, GENERIC_FN_NAME_KINDS, source) {
            return Some((symbol_kind, name.to_string()));
        }
    } else {
        if let Some(name) = child_text_by_kinds(node, GENERIC_NAME_KINDS, source) {
            return Some((symbol_kind, name.to_string()));
        }

        // C-family: name nested inside a declarator node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if (child.kind().contains("declarator") || child.kind() == "name")
                && let Some(name) = find_name_in_declarator_tree(child, GENERIC_NAME_KINDS, source)
            {
                return Some((symbol_kind, name.to_string()));
            }
        }
    }
    None
}

/// Recursively search declarator nodes for an identifier name.
///
/// Handles arbitrary nesting depth such as C pointer-returning functions:
/// `pointer_declarator → function_declarator → identifier`. Also checks
/// `qualified_identifier` / `scoped_identifier` for C++ qualified names.
fn find_name_in_declarator_tree<'a>(
    node: tree_sitter_lib::Node<'a>,
    name_kinds: &[&str],
    source: &'a str,
) -> Option<&'a str> {
    // Check direct identifier children
    if let Some(name) = child_text_by_kinds(node, name_kinds, source) {
        return Some(name);
    }
    // Check qualified_identifier / scoped_identifier (C++ qualified names)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == "qualified_identifier" || child.kind() == "scoped_identifier")
            && let Some(name) = innermost_qualified_name(child, name_kinds, source)
        {
            return Some(name);
        }
        // Recurse into nested declarator nodes
        if child.kind().contains("declarator")
            && let Some(name) = find_name_in_declarator_tree(child, name_kinds, source)
        {
            return Some(name);
        }
    }
    None
}

/// Extract the innermost identifier name from a `qualified_identifier` /
/// `scoped_identifier` node. Handles arbitrary nesting depth
/// (e.g. `ns::MyClass::method` returns `"method"`).
pub(crate) fn innermost_qualified_name<'a>(
    node: tree_sitter_lib::Node<'a>,
    name_kinds: &[&str],
    source: &'a str,
) -> Option<&'a str> {
    // Check direct identifier children first
    if let Some(name) = child_text_by_kinds(node, name_kinds, source) {
        return Some(name);
    }
    // Recurse into nested qualified_identifier / scoped_identifier children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == "qualified_identifier" || child.kind() == "scoped_identifier")
            && let Some(name) = innermost_qualified_name(child, name_kinds, source)
        {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::symbols::extract_symbols;

    #[test]
    fn parse_go_receiver_type_cases() {
        assert_eq!(
            parse_go_receiver_type("func (s *Server) Start()"),
            Some("Server".to_string())
        );
        assert_eq!(
            parse_go_receiver_type("func (s Server) Start()"),
            Some("Server".to_string())
        );
        assert_eq!(
            parse_go_receiver_type("func (*Server) Method()"),
            Some("Server".to_string())
        );
        assert_eq!(parse_go_receiver_type("func main()"), None);
        assert_eq!(parse_go_receiver_type("func NewServer() *Server"), None);
    }

    #[test]
    fn node_signature_skips_destructuring_brace() {
        // Regression: node_signature truncated at the first '{' which is the
        // destructuring brace in JS/TS, not the body brace.
        let source = "function foo({a, b}) {\n  return a + b;\n}\n";
        let syms = extract_symbols(source, Language::JavaScript);
        let foo = syms
            .iter()
            .find(|s| s.name == "foo")
            .expect("foo not found");
        assert!(
            foo.signature.contains("{a, b}"),
            "signature should include destructured params: {:?}",
            foo.signature
        );
    }
}
