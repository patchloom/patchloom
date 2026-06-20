//! Find references to a symbol across files.

use std::path::Path;

use serde::Serialize;

use super::{Language, parse_source};

/// Kind of reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    /// The definition site.
    Definition,
    /// A usage/call site.
    Reference,
}

/// A single reference to a symbol.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolRef {
    /// File path (relative).
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// The source line containing the reference (trimmed).
    pub context: String,
    /// Whether this is a definition or reference.
    pub kind: RefKind,
}

/// Node kinds that typically indicate a definition context.
const DEFINITION_PARENT_KINDS: &[&str] = &[
    "function_item",
    "function_definition",
    "function_declaration",
    "method_declaration",
    "method_definition",
    "struct_item",
    "struct_declaration",
    "enum_item",
    "enum_declaration",
    "trait_item",
    "trait_declaration",
    "impl_item",
    "class_definition",
    "class_declaration",
    "interface_declaration",
    "type_declaration",
    "type_item",
    "mod_item",
    "const_item",
    "type_spec",
];

/// Identifier node kinds to scan.
const IDENTIFIER_KINDS: &[&str] = &[
    "identifier",
    "type_identifier",
    "field_identifier",
    "property_identifier",
    "simple_identifier",
];

/// Node kinds whose subtrees should be skipped (strings, comments).
const SKIP_KINDS: &[&str] = &[
    "string_literal",
    "raw_string_literal",
    "string",
    "string_content",
    "string_fragment",
    "template_string",
    "line_comment",
    "block_comment",
    "comment",
    "doc_comment",
];

/// Find all references to `symbol_name` in the given source code.
pub fn find_refs_in_source(
    source: &str,
    symbol_name: &str,
    lang: Language,
    file_path: &str,
) -> Vec<SymbolRef> {
    let Some((tree, _)) = parse_source(source, lang) else {
        return Vec::new();
    };

    let lines: Vec<&str> = source.lines().collect();
    let mut refs = Vec::new();
    collect_refs(
        tree.root_node(),
        source,
        symbol_name,
        &lines,
        file_path,
        &mut refs,
    );
    refs
}

/// Find refs across multiple files.
pub fn find_refs_in_file(
    path: &Path,
    symbol_name: &str,
    lang_hint: Option<Language>,
    display_path: &str,
) -> Vec<SymbolRef> {
    let lang = lang_hint.unwrap_or_else(|| Language::from_path(path));
    if !lang.has_grammar() {
        return Vec::new();
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    find_refs_in_source(&source, symbol_name, lang, display_path)
}

fn collect_refs(
    node: tree_sitter_lib::Node,
    source: &str,
    symbol_name: &str,
    lines: &[&str],
    file_path: &str,
    refs: &mut Vec<SymbolRef>,
) {
    if SKIP_KINDS.contains(&node.kind()) {
        return;
    }

    if IDENTIFIER_KINDS.contains(&node.kind())
        && let Ok(text) = node.utf8_text(source.as_bytes())
        && text == symbol_name
    {
        let line = node.start_position().row + 1;
        let context = lines
            .get(node.start_position().row)
            .unwrap_or(&"")
            .trim()
            .to_string();

        let kind = if is_definition_site(node) {
            RefKind::Definition
        } else {
            RefKind::Reference
        };

        refs.push(SymbolRef {
            file: file_path.to_string(),
            line,
            context,
            kind,
        });
    }

    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_refs(cursor.node(), source, symbol_name, lines, file_path, refs);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Check if this identifier node is in a definition position.
fn is_definition_site(node: tree_sitter_lib::Node) -> bool {
    // Walk up parents to see if we're directly inside a definition node
    // where this identifier is the "name" child
    if let Some(parent) = node.parent()
        && DEFINITION_PARENT_KINDS.contains(&parent.kind())
    {
        // Check if this node is the "name" field of the parent
        // by checking the field name
        let mut cursor = parent.walk();
        for child in parent.children(&mut cursor) {
            if child.id() == node.id() {
                // This identifier is a direct child of a definition node.
                // It's a definition if it's the name child (first identifier).
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_refs_rust() {
        let source = r#"
fn setup_file() -> String {
    String::new()
}

fn main() {
    let x = setup_file();
    let y = setup_file();
}
"#;
        let refs = find_refs_in_source(source, "setup_file", Language::Rust, "test.rs");
        assert!(refs.len() >= 3); // 1 def + 2 refs
        let defs: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Definition)
            .collect();
        let uses: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == RefKind::Reference)
            .collect();
        assert!(!defs.is_empty());
        assert!(uses.len() >= 2);
    }

    #[test]
    fn find_refs_skips_strings_and_comments() {
        let source = r#"
fn target() {}

fn main() {
    // target is important
    let s = "target";
    target();
}
"#;
        let refs = find_refs_in_source(source, "target", Language::Rust, "test.rs");
        // Should find def + call, but NOT string or comment
        for r in &refs {
            assert!(!r.context.starts_with("//"));
            assert!(!r.context.contains("\"target\""));
        }
    }

    #[test]
    fn find_refs_python() {
        let source = r#"
def helper():
    pass

def main():
    helper()
    helper()
"#;
        let refs = find_refs_in_source(source, "helper", Language::Python, "test.py");
        assert!(refs.len() >= 3);
    }

    #[test]
    fn unknown_language_returns_empty() {
        let refs = find_refs_in_source("anything", "x", Language::Unknown, "test.txt");
        assert!(refs.is_empty());
    }

    #[test]
    fn ref_kind_serializes() {
        let json = serde_json::to_string(&RefKind::Definition).unwrap();
        assert_eq!(json, "\"definition\"");
    }
}
