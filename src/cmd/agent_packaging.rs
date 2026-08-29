//! Shared agent-packaging copy for MCP handshake and agent-rules (#2070).
//!
//! Keep strings in one place so handshake instructions and `agent-rules` cannot
//! drift on the name map, explore rule, or surface recommendations.
//!
//! # Inventory vs policy
//!
//! | Kind | Owner |
//! |------|--------|
//! | Operation list (plan op names + short descriptions) | [`crate::schema`] registry / `agent_operations_catalogue` |
//! | Core MCP tool names, name map, explore rule, YAML honesty, core pack body | **this module** (hand-written packaging) |
//! | Host peels, fuzzy refuse, exit/`error_kind` narrative, workflows | [`crate::cmd::agent_rules`] policy modules (hand-written) |
//!
//! Do not push long honesty essays into schema `OpMeta` just to shrink
//! agent-rules. Packaging here is the bridge between MCP handshake and
//! agent-rules openers; policy lives under `agent_rules::`.

/// Tools registered when `PATCHLOOM_MCP_SURFACE=core` (single source of truth).
///
/// Used by MCP registration and by `agent-rules --surface core` so the prose
/// list cannot drift from the handshake inventory (#2070 / MPI).
pub(crate) const CORE_MCP_TOOL_NAMES: &[&str] = &[
    "read_file",
    "search_files",
    "list_files",
    "replace_text",
    "batch_replace",
    "doc_get",
    "doc_set",
    "doc_query",
    "md_replace_section",
    "execute_plan",
    "server_info",
];

/// Compact CLI / plan / MCP name map (models must not invent tool ids).
///
/// Full surface: MCP column may list registered tools including AST.
pub(crate) fn canonical_name_map_markdown() -> &'static str {
    canonical_name_map_markdown_for_surface(true)
}

/// Core surface: do not list unregistered MCP tool ids (e.g. `ast_rename`).
pub(crate) fn canonical_name_map_markdown_core() -> &'static str {
    canonical_name_map_markdown_for_surface(false)
}

fn canonical_name_map_markdown_for_surface(full_ast_tools: bool) -> &'static str {
    if full_ast_tools {
        "\
## Canonical names (do not invent tool ids)\n\
\n\
| Intent | CLI | Plan op (`execute_plan`) | MCP tool |\n\
|--------|-----|--------------------------|----------|\n\
| multi-op atomic | `tx` | (plan root) | `execute_plan` |\n\
| identifier rename | `ast rename` | `ast.rename` | `ast_rename` or plan via `execute_plan` |\n\
| structured single path | `doc set` | `doc.set` | `doc_set` |\n\
| structured multi-match (predicate/wildcard) | `doc update` | `doc.update` | `doc_update` |\n\
| search | `search` | n/a | `search_files` |\n\
| list files | n/a (MCP) | n/a | `list_files` |\n\
| read | `read` | n/a | `read_file` |\n\
\n\
Host meta-tools (for example a host `search_tool` catalog lookup) are **not** \
patchloom MCP tools. Only list registered MCP tool names when summarizing \
patchloom usage.\n\n"
    } else {
        "\
## Canonical names (do not invent tool ids)\n\
\n\
| Intent | CLI | Plan op (`execute_plan`) | MCP tool (core pack) |\n\
|--------|-----|--------------------------|----------------------|\n\
| multi-op atomic | `tx` | (plan root) | `execute_plan` |\n\
| identifier rename | `ast rename` | `ast.rename` | plan via `execute_plan` only (no standalone `ast_*` on core) |\n\
| structured single path | `doc set` | `doc.set` | `doc_set` |\n\
| structured multi-match (predicate/wildcard) | `doc update` | `doc.update` | plan via `execute_plan` only (no standalone `doc_update` on core) |\n\
| search | `search` | n/a | `search_files` |\n\
| list files | n/a (MCP) | n/a | `list_files` |\n\
| read | `read` | n/a | `read_file` |\n\
\n\
Host meta-tools (for example a host `search_tool` catalog lookup) are **not** \
patchloom MCP tools. Only list **registered** MCP tool names when summarizing.\n\n"
    }
}

/// Dual-host explore rule: MCP for inspect, shell for build/test.
pub(crate) fn explore_guidance_markdown() -> &'static str {
    "\
## Explore vs shell (dual-host)\n\
\n\
When patchloom MCP is connected:\n\
\n\
- Prefer `list_files`, `search_files`, and `read_file` with paths **relative** to \
MCP cwd over shell `cat` / `head` / `find` / `ls` / `sed` / `rg` (and over a \
second generic filesystem MCP) for explore and inventory.\n\
- Use shell only for build/test/run (`make`, `cargo test`, language runners) \
unless the user explicitly overrides.\n\
- MCP path containment: relative paths and absolute paths that resolve **inside** \
the server workspace are allowed (agents often join `server_info.cwd` + relative). \
`../` escapes, absolute paths outside the workspace, and escaping symlinks are \
rejected. Prefer relative paths in tool calls.\n\n"
}

/// YAML / doc presentation honesty contract.
pub(crate) fn yaml_style_honesty_markdown() -> &'static str {
    "\
## Structured doc style honesty\n\
\n\
`doc.*` (CLI, plan, MCP) may re-emit **canonical YAML presentation** while \
keeping values correct (for example collapsing indented block sequences under \
a key, or expanding aliases). When **block-sequence indent** or YAML \
alias/anchor/merge identity (`&name`, `*name`, `<<:`) changes, write JSON \
reports `style_changed: true` on CLI `doc --json` and on plan/MCP `changes[]` \
entries for modified files. Detection covers those two fingerprints, not every \
possible formatting change. Do not claim a pure surgical text edit when \
`style_changed` is true.\n\n"
}

/// Host default for coding agents (product default remains full).
pub(crate) fn core_surface_host_recommendation() -> &'static str {
    "\
## Recommended MCP surface for coding agents\n\
\n\
Product default remains **full** (compat). For coding agents (Grok, Claude, \
Cursor) with dual native tools + MCP, start with:\n\
\n\
```bash\n\
export PATCHLOOM_MCP_SURFACE=core\n\
patchloom mcp-server\n\
```\n\
\n\
Use `full` (or unset) when you need AST tools, create/delete/rename standalone \
tools, or advanced md/doc ops as first-class tools. `execute_plan` is on core \
and can still run full plan ops. See docs/plans/mcp-surface-tiers.md and \
docs/getting-started/mcp-setup.md.\n\n"
}

/// One-line recommendation for `server_info` when surface is full.
pub(crate) fn server_info_full_recommendation(tool_count: usize) -> String {
    format!(
        "Coding agents with tight context: set PATCHLOOM_MCP_SURFACE=core \
(tool_count={tool_count} on full). Product default stays full for compatibility."
    )
}

/// Short agent-rules body for `--surface core` (system-prompt injection).
pub(crate) fn agent_rules_core_body(version: &str, show_cli: bool, show_mcp: bool) -> String {
    let mut out = format!(
        "<!-- Generated by patchloom v{version} (surface=core) — https://github.com/patchloom/patchloom -->\n\
         # Patchloom (core pack)\n\n"
    );
    if show_mcp {
        out.push_str(
            "You have MCP tools for file reads, edits, and searches. Prefer paths \
relative to the MCP workspace; absolute paths are allowed only when they resolve \
inside the workspace (`../` and outside-workspace paths are rejected).\n\n",
        );
    } else if show_cli {
        out.push_str(
            "**Decision rule: if you are about to make 3+ tool calls for file edits, use \
`patchloom batch` instead.** One call replaces N round-trips. For agent sandboxes \
that shell out to the CLI, the **host** must invoke \
`patchloom --cwd <workspace> --contain …` and not let the model supply `--cwd` \
(containment follows effective cwd; #1832).\n\n",
        );
    }
    out.push_str(canonical_name_map_markdown_core());
    out.push_str(explore_guidance_markdown());
    out.push_str(yaml_style_honesty_markdown());
    out.push_str(core_surface_host_recommendation());
    if show_mcp {
        let tools = CORE_MCP_TOOL_NAMES
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "**Core tools only** (PATCHLOOM_MCP_SURFACE=core): {tools}. Prefer \
`execute_plan` for multi-op atomic work.\n\n"
        ));
    }
    if show_cli && !show_mcp {
        out.push_str(
            "Dry-run is the default on CLI. Branch on `applied` / `error_kind`, not `ok` alone.\n",
        );
    } else {
        out.push_str(
            "Dry-run is the default on CLI; MCP writes apply unless the host says otherwise. \
Branch on `applied` / `error_kind`, not `ok` alone.\n",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_map_lists_canonical_mcp_ids() {
        let t = canonical_name_map_markdown();
        for id in [
            "execute_plan",
            "search_files",
            "read_file",
            "doc_set",
            "doc_update",
            "ast_rename",
        ] {
            assert!(t.contains(id), "missing {id}");
        }
        assert!(
            t.contains("invent") || t.contains("not** patchloom") || t.contains("Host meta-tools"),
            "name map must discourage invented ids"
        );
        assert!(
            t.contains("structured multi-match") && t.contains("doc update"),
            "full name map must separate set vs update (#2132): {t}"
        );
        let core = canonical_name_map_markdown_core();
        assert!(
            core.contains("no standalone `doc_update` on core")
                || core.contains("execute_plan") && core.contains("doc.update"),
            "core name map must not invent standalone doc_update tool: {core}"
        );
    }

    #[test]
    fn explore_guidance_prefers_mcp_over_shell_cat() {
        let t = explore_guidance_markdown();
        assert!(t.contains("search_files"));
        assert!(t.contains("read_file"));
        assert!(t.contains("cat"));
        assert!(t.contains("build/test") || t.contains("build"));
        // Honesty: MCP allows abs paths inside workspace (AllowIfContained), not
        // "reject every absolute string" (dogfood 2026-07-30).
        assert!(
            t.contains("inside") || t.contains("AllowIfContained") || t.contains("server_info.cwd"),
            "explore guidance must not claim all absolute paths are rejected: {t}"
        );
        assert!(
            !t.contains("rejects absolute path strings"),
            "stale absolute-path claim: {t}"
        );
    }

    #[test]
    fn core_rules_are_much_shorter_than_50kb() {
        let body = agent_rules_core_body("0.0.0", true, true);
        assert!(
            body.len() < 8_000,
            "core agent-rules should stay small for system prompts, got {}",
            body.len()
        );
        assert!(body.contains("surface=core"));
    }

    #[test]
    fn core_rules_list_matches_core_tool_const() {
        let body = agent_rules_core_body("0.0.0", true, true);
        for name in CORE_MCP_TOOL_NAMES {
            assert!(
                body.contains(&format!("`{name}`")),
                "core body missing tool {name} from CORE_MCP_TOOL_NAMES"
            );
        }
        assert_eq!(CORE_MCP_TOOL_NAMES.len(), 11);
    }

    #[test]
    fn yaml_honesty_mentions_style_changed_on_all_surfaces() {
        let t = yaml_style_honesty_markdown();
        assert!(t.contains("style_changed"));
        assert!(t.contains("changes[]") || t.contains("changes"));
        assert!(t.contains("CLI") || t.contains("doc --json"));
        assert!(t.contains("MCP") || t.contains("plan"));
        assert!(
            t.contains("*name") && t.contains("<<:"),
            "style_changed contract must name alias/merge identity"
        );
    }
}
