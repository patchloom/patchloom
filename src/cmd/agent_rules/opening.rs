//! Document header, packaging blocks, and MCP/CLI decision openers.
//!
//! Packaging strings live in [`crate::cmd::agent_packaging`] (name map, explore,
//! YAML honesty, core surface). This module only composes them into the doc.

/// Header marker, packaging sections, and mode-specific openers.
pub(crate) fn append_opening(out: &mut String, version: &str, show_cli: bool, show_mcp: bool) {
    use super::AGENT_RULES_GENERATED_MARKER;
    // Header
    out.push_str(&format!(
        "{AGENT_RULES_GENERATED_MARKER}{version} — https://github.com/patchloom/patchloom -->\n\
         # Patchloom\n\n"
    ));

    // Packaging first (#2070): name map + explore before long inventory.
    out.push_str(crate::cmd::agent_packaging::canonical_name_map_markdown());
    out.push_str(crate::cmd::agent_packaging::explore_guidance_markdown());
    out.push_str(crate::cmd::agent_packaging::yaml_style_honesty_markdown());
    out.push_str(crate::cmd::agent_packaging::core_surface_host_recommendation());

    if show_mcp && !show_cli {
        out.push_str(
            "You have MCP tools for file reads, edits, and searches. \
             Use them for ALL file operations (including explore: `list_files` / \
             `search_files` / `read_file`, not shell `cat`/`find`/`ls` and not a \
             second generic filesystem MCP). Paths are confined to the \
             server workspace (MCP rejects `../` escapes and outside symlinks).\n\n",
        );
    }
    if show_mcp && show_cli {
        out.push_str(
            "**Decision rule: always use patchloom MCP tools instead of your native agent \
             tools for file edits and for explore/search/read when MCP is connected.** \
             Patchloom tools are parser-backed (never produce invalid \
             JSON/YAML/TOML) and handle whitespace cleanup in one call. MCP always enforces \
             workspace containment: absolute paths that resolve inside the server root are \
             allowed (agents often join `server_info.cwd` + relative); `../` escapes and \
             absolute paths outside the root are rejected. Prefer relative tool paths. CLI is \
             unrestricted by default; use `patchloom --cwd <ws> --contain …` so CLI reads, \
             writes, and meta-input files (batch/tx/explain plans, patch files, \
             `--files-from` lists) must resolve inside the workspace (absolute paths under \
             `--cwd` are allowed).\n\n\
             **Host sandbox contract (#1832):** `--contain` is relative to the **effective** \
             working directory (`--cwd` if set, else process cwd). An agent that can pass \
             `--cwd ..` widens the sandbox to a parent directory. Hosts must pin \
             `--cwd <project>` themselves and **strip or ignore model-supplied `--cwd` / \
             `--contain`** before exec. MCP always enforces its own server root.\n\n",
        );
    }
    if show_cli {
        out.push_str(
            "**Decision rule: if you are about to make 3+ tool calls for file edits, use \
             `patchloom batch` instead.** One call replaces N round-trips. For agent sandboxes \
             that shell out to the CLI, the **host** must invoke \
             `patchloom --cwd <workspace> --contain …` and not let the model supply `--cwd` \
             (containment follows effective cwd; #1832).\n\n",
        );
    }
}
