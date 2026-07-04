//! MCP tool surface map: registry default vs justified custom tools.
//!
//! # Policy (MCP surface honesty)
//!
//! - **Default:** add tools via [`super::registry::MCP_TOOL_REGISTRY`] when the
//!   tool is a 1:1 mapping to a plan [`crate::plan::Operation`] with no special
//!   multi-file preflight, multi-op batching, or non-plan read UX.
//! - **Custom (hand-written `#[tool]`):** only when the tool is *not* a simple
//!   Operation write. Every custom tool must appear in
//!   [`CUSTOM_MCP_TOOLS`] with a one-line reason.
//! - **Metric:** "no unjustified custom tools," not "fewer custom tools."
//!   Forcing search/batch/AST-read into the registry would fight agent UX.
//!
//! Counts (with default features, including `ast`):
//! registry + custom = total tools exposed by `list_tools`.
//!
//! Inventory is consumed by unit tests and documentation; it is intentionally
//! not wired into every production call path.

// Inventory for tests/docs; not every item is referenced outside `#[cfg(test)]`.
#![allow(dead_code)]

/// Why a tool is hand-written instead of registry-generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CustomMcpTool {
    pub name: &'static str,
    /// One-line justification (stable; tested for presence, not exact wording drift).
    pub why: &'static str,
    pub kind: CustomKind,
}

/// Coarse category for custom tools (documentation + tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CustomKind {
    /// Read-only structured doc queries (not write Operations).
    DocReadonly,
    /// Multi-file / parallel / CLI-shaped search or replace.
    MultiFileOrScan,
    /// Multi-op batch or full plan execution.
    MultiOp,
    /// Markdown helpers that need custom output or non-registry shapes.
    MdCustom,
    /// Patch apply with conflict/stale UX.
    Patch,
    /// AST analyze/mutate via tree-sitter (mostly non-plan or custom resolve).
    Ast,
    /// Server metadata / workspace discovery.
    Meta,
}

/// Authoritative list of hand-written MCP tools and why they are not registry tools.
///
/// When adding a custom `#[tool]` handler, add a row here in the same PR.
/// When moving a tool to the registry, remove its row here.
pub(super) const CUSTOM_MCP_TOOLS: &[CustomMcpTool] = &[
    // --- Doc readonly ---
    CustomMcpTool {
        name: "doc_get",
        why: "readonly doc get; not a write Operation",
        kind: CustomKind::DocReadonly,
    },
    CustomMcpTool {
        name: "doc_query",
        why: "readonly multi-action query (has/keys/len/select/flatten)",
        kind: CustomKind::DocReadonly,
    },
    CustomMcpTool {
        name: "doc_diff",
        why: "readonly structured file compare",
        kind: CustomKind::DocReadonly,
    },
    // --- Multi-file / scan ---
    CustomMcpTool {
        name: "search_files",
        why: "multi-path search with layered ignores and report modes",
        kind: CustomKind::MultiFileOrScan,
    },
    CustomMcpTool {
        name: "replace_text",
        why: "parallel multi-file scan + precomputed engine handoff",
        kind: CustomKind::MultiFileOrScan,
    },
    // --- Multi-op ---
    CustomMcpTool {
        name: "batch_replace",
        why: "builds multi-file replace batch, not one Operation",
        kind: CustomKind::MultiOp,
    },
    CustomMcpTool {
        name: "batch_tidy",
        why: "builds multi-file tidy batch, not one Operation",
        kind: CustomKind::MultiOp,
    },
    CustomMcpTool {
        name: "execute_plan",
        why: "full transaction plan (inline or path), not one Operation",
        kind: CustomKind::MultiOp,
    },
    // --- Md custom ---
    CustomMcpTool {
        name: "md_move_section",
        why: "cross-file section move + custom result shape",
        kind: CustomKind::MdCustom,
    },
    CustomMcpTool {
        name: "md_lint",
        why: "readonly AGENTS.md lint; not a write Operation",
        kind: CustomKind::MdCustom,
    },
    // --- Patch ---
    CustomMcpTool {
        name: "apply_patch",
        why: "unified-diff apply with stale/conflict exit mapping",
        kind: CustomKind::Patch,
    },
    // --- Meta ---
    CustomMcpTool {
        name: "git_status",
        why: "readonly git status vs HEAD",
        kind: CustomKind::Meta,
    },
    CustomMcpTool {
        name: "server_info",
        why: "server/workspace metadata for agents",
        kind: CustomKind::Meta,
    },
    // --- AST (feature-gated at registration; always listed here) ---
    CustomMcpTool {
        name: "ast_list",
        why: "AST symbol listing (analyze, not plan write)",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_read",
        why: "AST symbol body read",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_rename",
        why: "AST multi-file rename with scan/filter then stage",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_validate",
        why: "AST syntax validation report",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_search",
        why: "structural AST search",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_refs",
        why: "cross-file symbol references",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_deps",
        why: "import/dependency extraction",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_map",
        why: "repo map / PageRank over symbols",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_diff",
        why: "structural symbol diff across git refs",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_impact",
        why: "transitive impact analysis",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_replace",
        why: "symbol-scoped replace with custom resolve/output",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_insert",
        why: "AST insert with position/container resolve",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_wrap",
        why: "AST wrap with container resolve",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_imports",
        why: "import list/add/remove/dedupe actions",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_reorder",
        why: "symbol reorder strategies",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_group",
        why: "group symbols into module blocks",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_move",
        why: "move symbols across files",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_extract_to_file",
        why: "extract symbol to new file",
        kind: CustomKind::Ast,
    },
    CustomMcpTool {
        name: "ast_split",
        why: "split file across targets by symbols",
        kind: CustomKind::Ast,
    },
];

/// Names of custom tools (for set algebra in tests).
pub(super) fn custom_tool_names() -> impl Iterator<Item = &'static str> {
    CUSTOM_MCP_TOOLS.iter().map(|t| t.name)
}

#[cfg(test)]
mod tests {
    use super::super::registry::MCP_TOOL_REGISTRY;
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn custom_tool_names_are_unique() {
        let mut seen = BTreeSet::new();
        for t in CUSTOM_MCP_TOOLS {
            assert!(
                seen.insert(t.name),
                "duplicate custom tool name: {}",
                t.name
            );
            assert!(!t.why.is_empty(), "{} missing why", t.name);
        }
    }

    #[test]
    fn registry_and_custom_are_disjoint() {
        let registry: BTreeSet<_> = MCP_TOOL_REGISTRY.iter().map(|t| t.tool_name).collect();
        let custom: BTreeSet<_> = custom_tool_names().collect();
        let overlap: Vec<_> = registry.intersection(&custom).copied().collect();
        assert!(
            overlap.is_empty(),
            "tool(s) listed as both registry and custom: {overlap:?}"
        );
    }

    #[test]
    fn registry_plus_custom_count_matches_list_tools_expectation() {
        // With default features, ast tools are registered (see mcp_lists_expected_tools).
        let registry_n = MCP_TOOL_REGISTRY.len();
        let custom_n = CUSTOM_MCP_TOOLS.len();
        assert_eq!(
            registry_n + custom_n,
            54,
            "registry ({registry_n}) + custom ({custom_n}) must equal total MCP tools (54)"
        );
    }

    #[test]
    fn fix_whitespace_is_registry_not_custom() {
        // Regression: fix_whitespace was promoted to registry (#1391 honesty target).
        assert!(
            MCP_TOOL_REGISTRY
                .iter()
                .any(|t| t.tool_name == "fix_whitespace")
        );
        assert!(!custom_tool_names().any(|n| n == "fix_whitespace"));
    }
}
