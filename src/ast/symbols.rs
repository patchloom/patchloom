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

    // Go-specific: group receiver methods under their receiver type.
    // Go methods with receivers (e.g. `func (s *Server) Start()`) are
    // syntactically top-level in the AST, unlike Rust impl blocks or
    // Python/TS class methods. Group them so qualified lookups like
    // `Server::Start` work.
    if lang == Language::Go {
        group_go_receiver_methods(&mut symbols);
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
fn find_function_node<'a>(
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
    let vis = edit.visibility.as_deref().unwrap_or_else(|| {
        child_text_by_kind(fn_node, "visibility_modifier", source).unwrap_or("")
    });
    let params = edit
        .parameters
        .as_deref()
        .unwrap_or_else(|| child_text_by_kind(fn_node, "parameters", source).unwrap_or("()"));
    // tree-sitter-rust has no "return_type" wrapper node. The return type is
    // represented as a "->" child followed by a type child (e.g. "generic_type").
    // Extract it from source: everything between the parameters close and the
    // body (or node end), trimmed.
    let ret = edit
        .return_type
        .as_deref()
        .unwrap_or_else(|| extract_return_type(fn_node, source).unwrap_or(""));

    // Preserve function qualifiers (async, unsafe, const, extern) from the
    // original source. These appear between the visibility/start and "fn".
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

    // Use the same byte-range logic as replace to swap only the sig.
    let sig_end = find_body_start(fn_node).unwrap_or(fn_node.end_byte());

    let start = fn_node.start_byte();
    let before = &source[..start];
    let after = &source[sig_end..];
    Some(format!("{}{}{}", before, new_sig, after))
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
fn group_go_receiver_methods(symbols: &mut Vec<SymbolDef>) {
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
fn innermost_qualified_name<'a>(
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
        let res = rewrite_function_signature(src, "process", &edit);
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
        let res = rewrite_function_signature(src, "dangerous", &edit);
        let out = res.expect("rewrite should succeed");
        assert!(
            out.contains("pub unsafe fn dangerous(ptr: *mut u8)"),
            "unsafe qualifier should be preserved: {out}"
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
}
