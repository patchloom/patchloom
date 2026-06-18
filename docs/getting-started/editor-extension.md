# Editor Extension

Patchloom has a companion editor extension that handles binary discovery,
AGENTS.md generation, MCP server configuration, and structured file
operations from the command palette. It works in VS Code, Cursor,
Windsurf, and VSCodium.

## Install

Install from either registry:

- [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=patchloom.patchloom)
- [Open VSX Registry](https://open-vsx.org/extension/patchloom/patchloom)

Or search for **Patchloom** in the Extensions view (`Ctrl+Shift+X` /
`Cmd+Shift+X`).

## What the extension does

### One-click workspace setup

Run `Patchloom: Setup Workspace` from the command palette. It walks
through binary detection, `AGENTS.md` generation, and MCP server
configuration in one pass. If the CLI is not installed, you can install
it directly from the command palette with `Patchloom: Install Patchloom`.

### MCP server configuration

`Patchloom: Configure MCP` injects the Patchloom MCP server into your
editor's config file. Supports:

- **VS Code** (`.vscode/mcp.json`)
- **Cursor** (`.cursor/mcp.json`)
- **Windsurf** (`~/.codeium/windsurf/mcp_config.json`)

This replaces the manual JSON editing described in the
[MCP Setup guide](mcp-setup.md).

### Agent rules generation

`Patchloom: Initialize Project` generates an `AGENTS.md` file from
`patchloom agent-rules`. If one already exists, the extension opens a
diff so you can merge updates manually.

### Quick actions

`Patchloom: Quick Action` opens an interactive picker with structured
editing operations:

| Action | What it does |
|--------|-------------|
| Replace text | Literal text replacement with diff preview |
| Tidy file | Whitespace and newline cleanup with diff preview |
| Set structured value | Update a JSON, YAML, or TOML key with diff preview |
| Search text | Find pattern matches across workspace files |
| Create file | Scaffold a new file and open it in the editor |
| Read structured value | Read a JSON/YAML/TOML key and copy to clipboard |
| Merge patch (three-way) | Apply a stale patch using three-way merge |

### Batch operations

`Patchloom: Batch Apply` opens a line-oriented plan template where you
compose multiple operations (replace, tidy, doc set). The extension pipes
the plan to `patchloom batch --apply` so all changes land atomically.

### Status bar

The status bar shows MCP and binary readiness at a glance. Click it for
full diagnostics, including per-editor MCP configuration status.

### Verify MCP Server

`Patchloom: Verify MCP Server` spawns the MCP server, sends a JSON-RPC
`initialize` handshake, and confirms the server responds correctly.

## When to use the extension vs the CLI

The extension automates the setup steps described in the
[Quickstart](quickstart.md): installing the binary, running
`patchloom init`, and configuring MCP. If you use VS Code, Cursor,
Windsurf, or VSCodium, the extension is the fastest way to get started.

The CLI remains necessary for CI scripts, non-editor agents, and
environments where a VS Code extension is not available.

## Source and issues

- Repository: [github.com/patchloom/patchloom-vscode](https://github.com/patchloom/patchloom-vscode)
- Issues: [github.com/patchloom/patchloom-vscode/issues](https://github.com/patchloom/patchloom-vscode/issues)
