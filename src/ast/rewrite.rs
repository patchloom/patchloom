//! Function signature rewriting using tree-sitter AST analysis.
//!
//! Provides [`find_function_span`] for locating function signatures,
//! [`replace_function_signature`] for simple Rust-only replacement, and
//! [`rewrite_function_signature`] with [`FunctionSigEdit`] for structured
//! multi-language edits (visibility, parameters, return type).
//!
//! size-waiver: accepted single-domain bulk (policy #1408). Multi-language function signature rewrite; tests co-located; do not split for LOC alone.

use super::symbol_extract::innermost_qualified_name;
use super::{Language, child_text_by_kind, child_text_by_kinds, parse_source};

/// Located function signature with byte offsets into the source.
///
/// Returned by [`find_function_span`] to let callers splice in replacement
/// signatures without understanding language-specific syntax.
#[derive(Debug, Clone)]
pub struct FunctionSpan {
    /// Full byte range of the function node (including body).
    pub full_range: std::ops::Range<usize>,
    /// Byte range of just the signature (up to but not including the body).
    /// May include trailing whitespace; see `signature_text` for the trimmed version.
    pub signature_range: std::ops::Range<usize>,
    /// The signature text.
    pub signature_text: String,
    /// Function name.
    pub name: String,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line of the signature (not the body).
    pub signature_end_line: usize,
}

/// Find a function by name and return its signature span.
///
/// Works for any language with a tree-sitter grammar. Returns `None` if
/// the language has no grammar support, the function is not found, or
/// parsing fails.
///
/// The caller can use `signature_range` to splice in a replacement:
///
/// ```rust,ignore
/// let span = find_function_span(source, "old_name", Language::Rust).unwrap();
/// let result = format!(
///     "{}{}{}",
///     &source[..span.signature_range.start],
///     new_signature,
///     &source[span.signature_range.end..],
/// );
/// ```
pub fn find_function_span(
    source: &str,
    function_name: &str,
    lang: Language,
) -> Option<FunctionSpan> {
    let (tree, _) = parse_source(source, lang)?;
    let root = tree.root_node();

    let fn_node = find_function_node(root, source, function_name)?;

    let start = fn_node.start_byte();
    let end = fn_node.end_byte();
    let sig_end = find_body_start(fn_node).unwrap_or(end);

    let signature_text = source[start..sig_end].trim_end().to_string();
    let start_line = fn_node.start_position().row + 1;
    let sig_end_line = source[..sig_end].matches('\n').count() + 1;

    Some(FunctionSpan {
        full_range: start..end,
        signature_range: start..sig_end,
        signature_text,
        name: function_name.to_string(),
        start_line,
        signature_end_line: sig_end_line,
    })
}

/// Node kinds that represent function/method definitions per language.
const FUNCTION_NODE_KINDS: &[&str] = &[
    "function_item",           // Rust
    "function_definition",     // Python, C, C++
    "function_declaration",    // TypeScript, JavaScript, Go
    "method_declaration",      // Go, Java
    "method_definition",       // TypeScript, JavaScript
    "constructor_declaration", // Java
];

/// Node kinds that represent a function body (block of code).
const BODY_NODE_KINDS: &[&str] = &[
    "block",              // Rust, Python, Go
    "statement_block",    // TypeScript, JavaScript
    "compound_statement", // C, C++
];

/// Recursively find a function/method node by name across any supported language.
pub(crate) fn find_function_node<'a>(
    node: tree_sitter_lib::Node<'a>,
    source: &str,
    name: &str,
) -> Option<tree_sitter_lib::Node<'a>> {
    if FUNCTION_NODE_KINDS.contains(&node.kind()) && function_node_has_name(node, source, name) {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_function_node(child, source, name) {
            return Some(found);
        }
    }
    None
}

/// Check if a function/method node has the given name.
///
/// Handles multiple naming strategies:
/// 1. Direct identifier child (Rust, Python, Go, TypeScript, Java)
/// 2. Name nested inside a declarator child (C, C++)
fn function_node_has_name(node: tree_sitter_lib::Node, source: &str, name: &str) -> bool {
    // Strategy 1: direct identifier child
    let name_kinds = &[
        "identifier",
        "name",
        "property_identifier",
        "field_identifier",
        "word",
    ];
    if child_text_by_kinds(node, name_kinds, source) == Some(name) {
        return true;
    }
    // Strategy 2: C-family declarator nesting (function_declarator -> identifier)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind().contains("declarator") {
            if check_declarator_for_name(child, name_kinds, source, name) {
                return true;
            }
            // One more level: function_declarator -> declarator -> identifier
            let mut inner = child.walk();
            for grandchild in child.children(&mut inner) {
                if grandchild.kind().contains("declarator") || grandchild.kind() == "identifier" {
                    if grandchild.kind() == "identifier" {
                        if grandchild.utf8_text(source.as_bytes()).ok() == Some(name) {
                            return true;
                        }
                    } else if check_declarator_for_name(grandchild, name_kinds, source, name) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check a declarator node for a matching name, including inside
/// `qualified_identifier` / `scoped_identifier` nodes (C++ `MyClass::method`).
fn check_declarator_for_name(
    node: tree_sitter_lib::Node,
    name_kinds: &[&str],
    source: &str,
    name: &str,
) -> bool {
    // Direct identifier child
    if child_text_by_kinds(node, name_kinds, source) == Some(name) {
        return true;
    }
    // Check inside qualified_identifier / scoped_identifier for nested name.
    // Recurse because nested namespaces produce chains:
    //   qualified_identifier -> name: qualified_identifier -> name: identifier
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if (child.kind() == "qualified_identifier" || child.kind() == "scoped_identifier")
            && qualified_identifier_has_name(child, name_kinds, source, name)
        {
            return true;
        }
    }
    false
}

/// Recursively check a `qualified_identifier` / `scoped_identifier` for the
/// innermost identifier matching `name`. Handles arbitrary nesting depth
/// (e.g. `ns::MyClass::method`).
fn qualified_identifier_has_name(
    node: tree_sitter_lib::Node,
    name_kinds: &[&str],
    source: &str,
    name: &str,
) -> bool {
    innermost_qualified_name(node, name_kinds, source) == Some(name)
}

/// Find the byte offset where the body starts within a function node.
///
/// Scans children for known body node kinds, returning the start byte of the
/// body. For Python, the body is the indented `block` after the colon.
fn find_body_start(fn_node: tree_sitter_lib::Node) -> Option<usize> {
    let mut cursor = fn_node.walk();
    for child in fn_node.children(&mut cursor) {
        if BODY_NODE_KINDS.contains(&child.kind()) {
            return Some(child.start_byte());
        }
    }
    None
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
    let sig_end = find_body_start(fn_node).unwrap_or(fn_node.end_byte());

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
///
/// Supports all languages with tree-sitter grammars. For Rust, uses full reconstruction
/// (preserving async/unsafe/const/extern qualifiers). For other languages, uses surgical
/// node replacement: each `FunctionSigEdit` field replaces the corresponding AST node
/// in-place, preserving everything else.
///
/// The `return_type` value should use the target language's native syntax:
/// - Rust: `"-> String"`
/// - Python: `"-> dict"` (the `-> ` prefix is part of the value)
/// - TypeScript: `": Promise<Response>"` (the `: ` prefix is part of the value)
/// - Go: `"error"` or `"(*Response, error)"` (no prefix)
/// - Java: `"Response"` (placed before the method name)
///
/// Per #821: this remains a library-only helper for now (no CLI `ast signature`, no plan op, no MCP tool).
/// Record of decision lives in #821 and src/lib.rs embedding docs.
/// See #797, #1266.
pub fn rewrite_function_signature(
    source: &str,
    old_name: &str,
    edit: &FunctionSigEdit,
    lang: Language,
) -> Option<String> {
    if lang == Language::Rust {
        return rewrite_rust_sig(source, old_name, edit);
    }
    rewrite_sig_generic(source, old_name, edit, lang)
}

/// Rust-specific full reconstruction: extracts visibility, qualifiers, params,
/// return type from tree-sitter nodes and rebuilds the signature.
fn rewrite_rust_sig(source: &str, old_name: &str, edit: &FunctionSigEdit) -> Option<String> {
    let (tree, _) = parse_source(source, Language::Rust)?;
    let root = tree.root_node();

    let fn_node = find_fn_for_rewrite(root, source, old_name)?;

    let vis = edit.visibility.as_deref().unwrap_or_else(|| {
        child_text_by_kind(fn_node, "visibility_modifier", source).unwrap_or("")
    });
    let params = edit
        .parameters
        .as_deref()
        .unwrap_or_else(|| child_text_by_kind(fn_node, "parameters", source).unwrap_or("()"));
    let ret = edit
        .return_type
        .as_deref()
        .unwrap_or_else(|| extract_return_type(fn_node, source).unwrap_or(""));

    let qualifiers = extract_fn_qualifiers(fn_node, source);

    let vis_part = if vis.is_empty() {
        String::new()
    } else {
        format!("{} ", vis)
    };
    let qual_part = if qualifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", qualifiers)
    };
    let ret_part = if ret.is_empty() {
        String::new()
    } else {
        format!(" {}", ret)
    };
    let new_sig = format!(
        "{}{}fn {}{}{}",
        vis_part, qual_part, old_name, params, ret_part
    );

    let sig_end = find_body_start(fn_node).unwrap_or(fn_node.end_byte());
    let start = fn_node.start_byte();
    let before = &source[..start];
    let after = &source[sig_end..];
    Some(format!("{}{}{}", before, new_sig, after))
}

/// Node kinds that represent function parameters across languages.
const PARAM_NODE_KINDS: &[&str] = &[
    "parameters",        // Rust, Python
    "formal_parameters", // TypeScript, JavaScript, Java
    "parameter_list",    // Go, C, C++
];

/// Generic multi-language rewrite: surgical node replacement within the
/// signature span. Finds the function via language-agnostic `find_function_node`,
/// then replaces individual sub-nodes (parameters, return type, visibility)
/// in right-to-left order to avoid offset invalidation.
fn rewrite_sig_generic(
    source: &str,
    old_name: &str,
    edit: &FunctionSigEdit,
    lang: Language,
) -> Option<String> {
    let (tree, _) = parse_source(source, lang)?;
    let root = tree.root_node();
    let fn_node = find_function_node(root, source, old_name)?;
    let sig_end = find_body_start(fn_node).unwrap_or(fn_node.end_byte());
    let sig_start = fn_node.start_byte();

    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    // -- Visibility / modifiers --
    if let Some(new_vis) = &edit.visibility {
        if let Some(node) = find_modifier_node(fn_node, lang) {
            let end = node.end_byte();
            // Consume trailing whitespace so we don't leave a double space
            let ws_end = if source.as_bytes().get(end) == Some(&b' ') {
                end + 1
            } else {
                end
            };
            if new_vis.is_empty() {
                edits.push((node.start_byte()..ws_end, String::new()));
            } else {
                edits.push((node.start_byte()..end, new_vis.clone()));
            }
        } else if !new_vis.is_empty() {
            // Insert at the start of the signature
            edits.push((sig_start..sig_start, format!("{} ", new_vis)));
        }
    }

    // -- Parameters --
    if let Some(new_params) = &edit.parameters {
        // For Go methods, skip the receiver parameter_list (first one) and use
        // the one that comes after the function name.
        let params_node = if fn_node.kind() == "method_declaration" {
            fn_node
                .child_by_field_name("parameters")
                .or_else(|| find_nth_child_of_kinds(fn_node, PARAM_NODE_KINDS, 1))
        } else {
            find_first_child_of_kinds(fn_node, PARAM_NODE_KINDS)
        };
        if let Some(node) = params_node {
            edits.push((node.start_byte()..node.end_byte(), new_params.clone()));
        }
    }

    // -- Return type --
    if let Some(new_ret) = &edit.return_type {
        if let Some(range) = find_return_type_range(fn_node, lang, sig_end) {
            if new_ret.is_empty() {
                // Remove return type and preceding whitespace
                let trimmed_start = source[..range.start].trim_end().len();
                edits.push((trimmed_start..range.end, String::new()));
            } else {
                edits.push((range, new_ret.clone()));
            }
        } else if !new_ret.is_empty() {
            // No existing return type; insert after parameters
            let insert_pos = if fn_node.kind() == "method_declaration" {
                fn_node
                    .child_by_field_name("parameters")
                    .or_else(|| find_nth_child_of_kinds(fn_node, PARAM_NODE_KINDS, 1))
            } else {
                find_first_child_of_kinds(fn_node, PARAM_NODE_KINDS)
            }
            .map(|n| n.end_byte())
            .unwrap_or(sig_end);
            edits.push((insert_pos..insert_pos, format!(" {}", new_ret)));
        }
    }

    // Sort right-to-left by start position so earlier splices don't shift later ones
    edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
    let mut result = source.to_string();
    for (range, text) in edits {
        result.replace_range(range, &text);
    }
    Some(result)
}

/// Find the first child node matching any of the given kinds.
/// Also searches one level inside declarator nodes (C/C++ nesting).
fn find_first_child_of_kinds<'a>(
    node: tree_sitter_lib::Node<'a>,
    kinds: &[&str],
) -> Option<tree_sitter_lib::Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            return Some(child);
        }
        // C/C++: parameters live inside function_declarator, not as direct children
        if child.kind().contains("declarator") {
            let mut inner = child.walk();
            for grandchild in child.children(&mut inner) {
                if kinds.contains(&grandchild.kind()) {
                    return Some(grandchild);
                }
            }
        }
    }
    None
}

/// Find the Nth (0-based) child node matching any of the given kinds.
fn find_nth_child_of_kinds<'a>(
    node: tree_sitter_lib::Node<'a>,
    kinds: &[&str],
    n: usize,
) -> Option<tree_sitter_lib::Node<'a>> {
    let mut cursor = node.walk();
    let mut count = 0;
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            if count == n {
                return Some(child);
            }
            count += 1;
        }
    }
    None
}

/// Find the visibility/modifier node for a function definition.
fn find_modifier_node(
    fn_node: tree_sitter_lib::Node,
    lang: Language,
) -> Option<tree_sitter_lib::Node> {
    match lang {
        Language::Java => find_first_child_of_kinds(fn_node, &["modifiers"]),
        _ => find_first_child_of_kinds(fn_node, &["visibility_modifier", "visibility"]),
    }
}

/// Find the byte range of the return type in a function signature.
/// Returns the range that should be replaced (includes language-specific prefix like `-> ` or `: `).
fn find_return_type_range(
    fn_node: tree_sitter_lib::Node,
    lang: Language,
    sig_end: usize,
) -> Option<std::ops::Range<usize>> {
    match lang {
        Language::Python => {
            // Python: `-> type` after parameters, before the colon.
            // tree-sitter-python: "->" child followed by a "type" child.
            let mut cursor = fn_node.walk();
            let mut arrow_start = None;
            for child in fn_node.children(&mut cursor) {
                if child.kind() == "->" {
                    arrow_start = Some(child.start_byte());
                    continue;
                }
                if let Some(a) = arrow_start
                    && child.kind() == "type"
                {
                    return Some(a..child.end_byte());
                }
            }
            None
        }
        Language::Go => {
            // Go: "result" field after parameters (single type or parenthesized list).
            fn_node
                .child_by_field_name("result")
                .map(|n| n.start_byte()..n.end_byte())
        }
        Language::TypeScript | Language::JavaScript => {
            // TS/JS: "return_type" or "type_annotation" child.
            find_first_child_of_kinds(fn_node, &["return_type", "type_annotation"])
                .map(|n| n.start_byte()..n.end_byte())
        }
        Language::Java => {
            // Java: return type node appears before the name. It's a direct child
            // with a type kind (void_type, type_identifier, generic_type, etc.).
            // We find it by looking for type-like children that come before the
            // function name identifier.
            let name_start = fn_node
                .child_by_field_name("name")
                .map(|n| n.start_byte())
                .unwrap_or(sig_end);
            let type_kinds = [
                "void_type",
                "type_identifier",
                "generic_type",
                "integral_type",
                "boolean_type",
                "floating_point_type",
                "array_type",
                "scoped_type_identifier",
            ];
            let mut cursor = fn_node.walk();
            for child in fn_node.children(&mut cursor) {
                if child.start_byte() < name_start && type_kinds.contains(&child.kind()) {
                    return Some(child.start_byte()..child.end_byte());
                }
            }
            None
        }
        Language::C | Language::Cpp => {
            // C/C++: return type is a type specifier before the declarator.
            let decl_start =
                find_first_child_of_kinds(fn_node, &["declarator", "function_declarator"])
                    .map(|n| n.start_byte())
                    .unwrap_or(sig_end);
            let type_kinds = [
                "primitive_type",
                "type_identifier",
                "sized_type_specifier",
                "struct_specifier",
                "enum_specifier",
                "union_specifier",
                "template_type",
            ];
            let mut cursor = fn_node.walk();
            for child in fn_node.children(&mut cursor) {
                if child.start_byte() < decl_start && type_kinds.contains(&child.kind()) {
                    return Some(child.start_byte()..child.end_byte());
                }
            }
            None
        }
        _ => None,
    }
}

/// Extract the return type from a function_item node. tree-sitter-rust has no
/// "return_type" wrapper; the return type is a `->` child followed by a type
/// child (e.g. `generic_type`). Returns `"-> Type"` including the arrow.
fn extract_return_type<'a>(fn_node: tree_sitter_lib::Node, source: &'a str) -> Option<&'a str> {
    let mut cursor = fn_node.walk();
    let mut arrow_start = None;
    for child in fn_node.children(&mut cursor) {
        if child.kind() == "->" {
            arrow_start = Some(child.start_byte());
            continue;
        }
        if let Some(a) = arrow_start {
            // The child right after "->" is the type node.
            return Some(source[a..child.end_byte()].trim_end());
        }
    }
    None
}

/// Extract function qualifiers (async, unsafe, const, extern "C") from a
/// function_item node by scanning the source text between the visibility
/// (or node start) and the "fn" keyword. This is more robust than matching
/// tree-sitter node kinds, which vary across grammar versions.
fn extract_fn_qualifiers(fn_node: tree_sitter_lib::Node, source: &str) -> String {
    let node_text = &source[fn_node.start_byte()..fn_node.end_byte()];
    // Find "fn " (with trailing space) in the node text.
    // Everything between the visibility and "fn " consists of qualifiers.
    let fn_pos = match node_text.find("fn ") {
        Some(pos) => pos,
        None => return String::new(),
    };
    let prefix = &node_text[..fn_pos];
    // Strip the visibility part (e.g., "pub ", "pub(crate) ") if present.
    let vis = child_text_by_kind(fn_node, "visibility_modifier", source)
        .or_else(|| child_text_by_kind(fn_node, "visibility", source));
    let quals = if let Some(v) = vis {
        // Remove the visibility and any trailing whitespace
        let after_vis = prefix.strip_prefix(v).unwrap_or(prefix);
        after_vis.trim()
    } else {
        prefix.trim()
    };
    quals.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_function_signature_structured() {
        let src = "fn old(a: i32) -> i32 { a }\nfn other() {}";
        let edit = FunctionSigEdit {
            visibility: Some("pub(crate)".to_string()),
            parameters: Some("(x: u32, y: &str)".to_string()),
            return_type: Some("-> String".to_string()),
        };
        let res = rewrite_function_signature(src, "old", &edit, Language::Rust);
        let out = res.expect("rewrite_function_signature should succeed for matching name");
        assert!(out.contains("pub(crate) fn old(x: u32, y: &str) -> String"));
        assert!(out.contains("fn other"));
        assert!(!out.contains("fn old(a: i32)"));
    }

    // Regression: rewrite_function_signature must preserve qualifiers
    // (async, unsafe, const, extern) from the original function.
    #[test]
    fn rewrite_function_signature_preserves_async() {
        let src = "pub async fn process(data: &[u8]) -> Result<()> { Ok(()) }";
        let edit = FunctionSigEdit {
            visibility: None,
            parameters: Some("(input: &str)".to_string()),
            return_type: None,
        };
        let res = rewrite_function_signature(src, "process", &edit, Language::Rust);
        let out = res.expect("rewrite should succeed");
        assert!(
            out.contains("pub async fn process(input: &str) -> Result<()>"),
            "async qualifier should be preserved: {out}"
        );
    }

    #[test]
    fn rewrite_function_signature_preserves_unsafe() {
        let src = "pub unsafe fn dangerous(ptr: *const u8) {}";
        let edit = FunctionSigEdit {
            visibility: None,
            parameters: Some("(ptr: *mut u8)".to_string()),
            return_type: None,
        };
        let res = rewrite_function_signature(src, "dangerous", &edit, Language::Rust);
        let out = res.expect("rewrite should succeed");
        assert!(
            out.contains("pub unsafe fn dangerous(ptr: *mut u8)"),
            "unsafe qualifier should be preserved: {out}"
        );
    }

    // ── multi-language rewrite_function_signature tests (#1266) ──

    #[test]
    fn rewrite_python_params() {
        let src = "def process(data: list) -> dict:\n    return {}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(data: list, validate: bool = True)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::Python).unwrap();
        assert!(
            out.contains("def process(data: list, validate: bool = True) -> dict:"),
            "should replace params: {out}"
        );
        assert!(out.contains("return {}"), "body preserved");
    }

    #[test]
    fn rewrite_python_return_type() {
        let src = "def process(data: list) -> dict:\n    return {}\n";
        let edit = FunctionSigEdit {
            return_type: Some("-> list".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::Python).unwrap();
        assert!(
            out.contains("def process(data: list) -> list:"),
            "should replace return type: {out}"
        );
    }

    #[test]
    fn rewrite_python_add_return_type() {
        let src = "def process(data):\n    pass\n";
        let edit = FunctionSigEdit {
            return_type: Some("-> dict".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::Python).unwrap();
        assert!(out.contains("-> dict"), "should add return type: {out}");
    }

    #[test]
    fn rewrite_python_async_preserved() {
        let src = "async def fetch(url: str) -> bytes:\n    pass\n";
        let edit = FunctionSigEdit {
            parameters: Some("(url: str, timeout: int = 30)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "fetch", &edit, Language::Python).unwrap();
        assert!(
            out.contains("async def fetch(url: str, timeout: int = 30)"),
            "async should be preserved: {out}"
        );
    }

    #[test]
    fn rewrite_typescript_params() {
        let src = "function fetchData(url: string): Promise<Response> {\n  return fetch(url);\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(url: string, options?: RequestInit)".to_string()),
            ..Default::default()
        };
        let out =
            rewrite_function_signature(src, "fetchData", &edit, Language::TypeScript).unwrap();
        assert!(
            out.contains("function fetchData(url: string, options?: RequestInit)"),
            "should replace params: {out}"
        );
        assert!(out.contains("return fetch(url)"), "body preserved");
    }

    #[test]
    fn rewrite_typescript_return_type() {
        let src = "function fetchData(url: string): Promise<Response> {\n  return fetch(url);\n}\n";
        let edit = FunctionSigEdit {
            return_type: Some(": Promise<Result[]>".to_string()),
            ..Default::default()
        };
        let out =
            rewrite_function_signature(src, "fetchData", &edit, Language::TypeScript).unwrap();
        assert!(
            out.contains(": Promise<Result[]>"),
            "should replace return type: {out}"
        );
    }

    #[test]
    fn rewrite_go_function_params() {
        let src =
            "func HandleRequest(w http.ResponseWriter, r *http.Request) error {\n\treturn nil\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some(
                "(ctx context.Context, w http.ResponseWriter, r *http.Request)".to_string(),
            ),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "HandleRequest", &edit, Language::Go).unwrap();
        assert!(
            out.contains(
                "func HandleRequest(ctx context.Context, w http.ResponseWriter, r *http.Request)"
            ),
            "should replace params: {out}"
        );
        assert!(out.contains("return nil"), "body preserved");
    }

    #[test]
    fn rewrite_go_function_return_type() {
        let src =
            "func HandleRequest(w http.ResponseWriter, r *http.Request) error {\n\treturn nil\n}\n";
        let edit = FunctionSigEdit {
            return_type: Some("(*Response, error)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "HandleRequest", &edit, Language::Go).unwrap();
        assert!(
            out.contains("(*Response, error)"),
            "should replace return type: {out}"
        );
    }

    #[test]
    fn rewrite_go_method_params() {
        let src = "func (s *Server) HandleRequest(w http.ResponseWriter, r *http.Request) error {\n\treturn nil\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(ctx context.Context, r *http.Request)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "HandleRequest", &edit, Language::Go).unwrap();
        assert!(
            out.contains("func (s *Server) HandleRequest(ctx context.Context, r *http.Request)"),
            "should replace method params without touching receiver: {out}"
        );
    }

    #[test]
    fn rewrite_java_params() {
        let src = "public class Foo {\n    public void processEvent(Event e) {\n        // body\n    }\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(Event e, Config config)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "processEvent", &edit, Language::Java).unwrap();
        assert!(
            out.contains("processEvent(Event e, Config config)"),
            "should replace params: {out}"
        );
    }

    #[test]
    fn rewrite_java_return_type() {
        let src = "public class Foo {\n    public void processEvent(Event e) {\n        // body\n    }\n}\n";
        let edit = FunctionSigEdit {
            return_type: Some("Response".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "processEvent", &edit, Language::Java).unwrap();
        assert!(
            out.contains("public Response processEvent"),
            "should replace return type: {out}"
        );
    }

    #[test]
    fn rewrite_java_visibility() {
        let src = "public class Foo {\n    public void processEvent(Event e) {\n        // body\n    }\n}\n";
        let edit = FunctionSigEdit {
            visibility: Some("protected".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "processEvent", &edit, Language::Java).unwrap();
        assert!(
            out.contains("protected void processEvent"),
            "should replace visibility: {out}"
        );
    }

    #[test]
    fn rewrite_c_params() {
        let src = "int process(const char *input) {\n    return 0;\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(const char *input, size_t len, int flags)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::C).unwrap();
        assert!(
            out.contains("int process(const char *input, size_t len, int flags)"),
            "should replace params: {out}"
        );
        assert!(out.contains("return 0"), "body preserved");
    }

    #[test]
    fn rewrite_c_return_type() {
        let src = "int process(const char *input) {\n    return 0;\n}\n";
        let edit = FunctionSigEdit {
            return_type: Some("void".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::C).unwrap();
        assert!(
            out.contains("void process(const char *input)"),
            "should replace return type: {out}"
        );
    }

    #[test]
    fn rewrite_cpp_params() {
        let src = "void MyClass::process(int x) {\n    // body\n}\n";
        let edit = FunctionSigEdit {
            parameters: Some("(int x, double y)".to_string()),
            ..Default::default()
        };
        let out = rewrite_function_signature(src, "process", &edit, Language::Cpp).unwrap();
        assert!(
            out.contains("void MyClass::process(int x, double y)"),
            "should replace C++ method params: {out}"
        );
    }

    #[test]
    fn rewrite_unsupported_language_returns_none() {
        let src = "whatever";
        let edit = FunctionSigEdit::default();
        assert!(rewrite_function_signature(src, "foo", &edit, Language::Unknown).is_none());
    }

    #[test]
    fn rewrite_function_not_found_returns_none() {
        let src = "def process(data):\n    pass\n";
        let edit = FunctionSigEdit {
            parameters: Some("(x)".to_string()),
            ..Default::default()
        };
        assert!(rewrite_function_signature(src, "nonexistent", &edit, Language::Python).is_none());
    }

    // ── find_function_span tests ──────────────────────────────────

    #[test]
    fn find_function_span_rust() {
        let source = "fn hello(x: i32) -> String {\n    x.to_string()\n}\n";
        let span = find_function_span(source, "hello", Language::Rust).unwrap();
        assert_eq!(span.name, "hello");
        assert!(span.signature_text.contains("fn hello(x: i32) -> String"));
        assert!(!span.signature_text.contains("x.to_string()"));
        assert_eq!(span.start_line, 1);
    }

    #[test]
    fn find_function_span_python() {
        let source = "class Foo:\n    def process(self, data: list) -> dict:\n        return {}\n";
        let span = find_function_span(source, "process", Language::Python).unwrap();
        assert_eq!(span.name, "process");
        assert!(span.signature_text.contains("def process"));
        assert!(span.signature_text.contains("-> dict"));
    }

    #[test]
    fn find_function_span_typescript() {
        let source = "export async function fetchData(url: string): Promise<Response> {\n  return fetch(url);\n}\n";
        let span = find_function_span(source, "fetchData", Language::TypeScript).unwrap();
        assert_eq!(span.name, "fetchData");
        assert!(span.signature_text.contains("function fetchData"));
    }

    #[test]
    fn find_function_span_go_function() {
        let source =
            "func HandleRequest(w http.ResponseWriter, r *http.Request) error {\n\treturn nil\n}\n";
        let span = find_function_span(source, "HandleRequest", Language::Go).unwrap();
        assert_eq!(span.name, "HandleRequest");
        assert!(span.signature_text.contains("func HandleRequest"));
    }

    #[test]
    fn find_function_span_go_method() {
        let source = "func (s *Server) HandleRequest(w http.ResponseWriter, r *http.Request) error {\n\treturn nil\n}\n";
        let span = find_function_span(source, "HandleRequest", Language::Go).unwrap();
        assert_eq!(span.name, "HandleRequest");
        assert!(
            span.signature_text
                .contains("func (s *Server) HandleRequest")
        );
    }

    #[test]
    fn find_function_span_java() {
        let source = "public class Foo {\n    public void processEvent(Event e) {\n        // ...\n    }\n}\n";
        let span = find_function_span(source, "processEvent", Language::Java).unwrap();
        assert_eq!(span.name, "processEvent");
        assert!(span.signature_text.contains("public void processEvent"));
    }

    #[test]
    fn find_function_span_not_found() {
        let source = "fn hello() {}\n";
        let result = find_function_span(source, "nonexistent", Language::Rust);
        assert!(result.is_none());
    }

    #[test]
    fn find_function_span_no_grammar() {
        let source = "whatever";
        let result = find_function_span(source, "foo", Language::Unknown);
        assert!(result.is_none());
    }

    #[test]
    fn find_function_span_multiline_python() {
        let source = "def long_function(\n    param1: str,\n    param2: int,\n    param3: bool = False,\n) -> dict:\n    return {}\n";
        let span = find_function_span(source, "long_function", Language::Python).unwrap();
        assert_eq!(span.name, "long_function");
        assert!(span.signature_text.contains("param1: str"));
        assert!(span.signature_text.contains("-> dict"));
        assert_eq!(span.start_line, 1);
        assert!(span.signature_end_line >= 5);
    }

    #[test]
    fn find_function_span_can_splice_replacement() {
        let source = "fn old_name(x: i32) -> bool {\n    true\n}\n";
        let span = find_function_span(source, "old_name", Language::Rust).unwrap();
        let new_sig = "fn new_name(x: i32, y: bool) -> bool ";
        let mut result = String::new();
        result.push_str(&source[..span.signature_range.start]);
        result.push_str(new_sig);
        result.push_str(&source[span.signature_range.end..]);
        assert!(result.contains("fn new_name(x: i32, y: bool) -> bool"));
        assert!(result.contains("true")); // body preserved
    }

    #[test]
    fn find_function_span_cpp() {
        let source = "void process(int x, double y) {\n    // body\n}\n";
        let span = find_function_span(source, "process", Language::Cpp).unwrap();
        assert_eq!(span.name, "process");
        assert!(
            span.signature_text
                .contains("void process(int x, double y)")
        );
        assert!(!span.signature_text.contains("// body"));
    }

    #[test]
    fn find_function_span_cpp_qualified_name() {
        let source = "void MyClass::process(int x) {\n    // body\n}\n";
        let span = find_function_span(source, "process", Language::Cpp).unwrap();
        assert_eq!(span.name, "process");
        assert!(span.signature_text.contains("void MyClass::process(int x)"));
    }

    #[test]
    fn find_function_span_cpp_nested_namespace_qualified() {
        let source = "int ns::MyClass::deep() {\n    return 0;\n}\n";
        let span = find_function_span(source, "deep", Language::Cpp).unwrap();
        assert_eq!(span.name, "deep");
        assert!(span.signature_text.contains("ns::MyClass::deep()"));
    }

    #[test]
    fn find_function_span_cpp_triple_nested_namespace() {
        let source = "int outer::inner::MyClass::compute(int x) {\n    return x;\n}\n";
        let span = find_function_span(source, "compute", Language::Cpp).unwrap();
        assert_eq!(span.name, "compute");
        assert!(
            span.signature_text
                .contains("outer::inner::MyClass::compute(int x)")
        );
    }

    #[test]
    fn find_function_span_cpp_qualified_no_false_positive() {
        let source = "void MyClass::alpha(int x) {\n}\nvoid MyClass::beta() {\n}\n";
        let span = find_function_span(source, "beta", Language::Cpp).unwrap();
        assert_eq!(span.name, "beta");
        assert!(span.signature_text.contains("beta"));
        assert!(!span.signature_text.contains("alpha"));
    }

    #[test]
    fn find_function_span_c() {
        let source = "int main(int argc, char *argv[]) {\n    return 0;\n}\n";
        let span = find_function_span(source, "main", Language::C).unwrap();
        assert_eq!(span.name, "main");
        assert!(span.signature_text.contains("int main"));
        assert!(!span.signature_text.contains("return 0"));
    }
}
