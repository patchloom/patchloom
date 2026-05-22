# MCP Setup Guide

When built with `--features mcp`, Patchloom can run an MCP (Model Context Protocol) server for structured tool calls. MCP-capable AI agents can call Patchloom tools directly with JSON parameters, with no shell command construction, no quoting issues, and no `--apply` flag needed.

## Install or build with MCP support

MCP is behind a feature flag. Install or build with:

```bash
cargo install --path . --features mcp
# or
cargo build --features mcp --release
```

Verify it works:

```bash
patchloom mcp-server --help
```

## Configure your agent

### Grok (config.toml)

Add to `~/.grok/config.toml`:

```toml
[mcp_servers.patchloom]
command = "/path/to/patchloom"
args = ["mcp-server"]
```

### Claude Desktop (JSON)

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server"]
    }
  }
}
```

### Cursor / VS Code (settings.json)

```json
{
  "mcp.servers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server"]
    }
  }
}
```

### Generic stdio MCP

Any MCP client that supports stdio transport can connect by spawning `patchloom mcp-server` as a subprocess. The server communicates via JSON-RPC over stdin/stdout.

## Available tools

| Tool | Description |
|------|-------------|
| `patchloom_doc_set` | Set a value by selector in a JSON, YAML, or TOML file |
| `patchloom_doc_delete` | Delete a value by selector from a structured file |
| `patchloom_doc_merge` | Deep-merge an object into a document |
| `patchloom_doc_append` | Append a value to an array |
| `patchloom_doc_prepend` | Prepend a value to an array |
| `patchloom_doc_ensure` | Set a value only if the selector path does not exist |
| `patchloom_doc_delete_where` | Delete array elements matching a predicate |
| `patchloom_doc_update` | Update array elements matching a predicate |
| `patchloom_doc_move` | Move a value from one selector path to another |
| `patchloom_doc_get` | Read a value by selector (read-only) |
| `patchloom_doc_has` | Check whether a selector path exists (read-only) |
| `patchloom_doc_keys` | List object keys at a selector path (read-only) |
| `patchloom_doc_len` | Count items in an array or object (read-only) |
| `patchloom_doc_select` | Filter array items by selector (read-only) |
| `patchloom_doc_flatten` | List all leaf selector paths and values (read-only) |
| `patchloom_doc_diff` | Compare two structured files (read-only) |
| `patchloom_search` | Search files for a pattern with context (read-only) |
| `patchloom_status` | Show uncommitted file changes vs git HEAD (read-only) |
| `patchloom_read` | Read file contents with optional line range |
| `patchloom_replace` | Replace text in a file (literal or regex) |
| `patchloom_md_upsert_bullet` | Add a bullet under a markdown heading |
| `patchloom_md_table_append` | Append a row to a markdown table |
| `patchloom_md_replace_section` | Replace a markdown section by heading |
| `patchloom_md_insert_after_heading` | Insert content after a markdown heading |
| `patchloom_md_insert_before_heading` | Insert content before a markdown heading |
| `patchloom_md_lint_agents` | Lint an AGENTS.md file for common issues |
| `patchloom_tidy` | Fix whitespace and line endings |
| `patchloom_create` | Create a new file with content |
| `patchloom_delete` | Delete a file |
| `patchloom_rename` | Move or rename a file (binary-safe) |
| `patchloom_patch` | Apply a unified diff |
| `patchloom_batch` | Run multiple operations in one call |
| `patchloom_tx` | Execute a full transaction plan with format/validate lifecycle |

## How MCP mode differs from CLI mode

| Aspect | CLI mode | MCP mode |
|--------|----------|----------|
| Apply behavior | Requires `--apply` flag | Auto-applies (writes are the default) |
| Input format | Shell arguments | Structured JSON parameters |
| Path security | No restriction | Paths must stay within working directory |
| Error format | stderr text | MCP error response with structured content |
| Discovery | Agent reads AGENTS.md | Agent discovers tools via MCP protocol |

## Security model

The MCP server enforces path containment: all file paths must resolve within the working directory where `patchloom mcp-server` was started. Absolute paths, `../` traversal, and symlinks escaping the working directory are rejected. This prevents an agent from accidentally (or maliciously) editing files outside the project.

The `patchloom_batch` tool parses its operations line by line and validates every path before execution. The `patchloom_tx` tool accepts full transaction plans including `format` and `validate` lifecycle steps, which execute shell commands in the working directory. All operation paths and plan-level `cwd` fields are validated for containment before execution.

## Example tool call

An MCP-capable agent sends:

```json
{
  "method": "tools/call",
  "params": {
    "name": "patchloom_doc_set",
    "arguments": {
      "path": "config.yaml",
      "selector": "database.port",
      "value": 5432
    }
  }
}
```

Patchloom parses the YAML, changes `database.port` to `5432`, preserves all comments and formatting, and writes the file. The agent receives a success response with no further action needed.

For multi-file edits with post-write formatting and validation, use `patchloom_tx`:

```json
{
  "method": "tools/call",
  "params": {
    "name": "patchloom_tx",
    "arguments": {
      "plan": "{\"version\":\"1\",\"operations\":[{\"op\":\"replace\",\"path\":\"src/main.rs\",\"from\":\"v1\",\"to\":\"v2\"},{\"op\":\"doc.set\",\"path\":\"Cargo.toml\",\"selector\":\"package.version\",\"value\":\"2.0.0\"}],\"format\":[{\"cmd\":\"cargo fmt\"}],\"validate\":[{\"cmd\":\"cargo test\",\"required\":true}]}"
    }
  }
}
```

This replaces text in `src/main.rs`, updates the version in `Cargo.toml`, runs `cargo fmt`, and verifies with `cargo test`. If any step fails, the transaction reports the error. With `"strict": true`, writes are rolled back on format or validate failure.
