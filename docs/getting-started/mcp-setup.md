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
# Add "--allow-shell" to args to enable format/validate lifecycle steps:
# args = ["mcp-server", "--allow-shell"]
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

### VS Code (.vscode/mcp.json)

Create `.vscode/mcp.json` in your workspace root:

```json
{
  "servers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server"]
    }
  }
}
```

### Cursor (.cursor/mcp.json)

Create `.cursor/mcp.json` in your workspace root:

```json
{
  "servers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server"]
    }
  }
}
```

Or use the [Patchloom VS Code extension](https://github.com/patchloom/patchloom-vscode) to configure MCP automatically via the `Patchloom: Configure MCP` command.

### Generic stdio MCP

Any MCP client that supports stdio transport can connect by spawning `patchloom mcp-server` as a subprocess. The server communicates via JSON-RPC over stdin/stdout.

## Available tools

| Tool | Description |
|------|-------------|
| `doc_set` | Set a value by selector in a JSON, YAML, or TOML file |
| `doc_delete` | Delete a value by selector from a structured file |
| `doc_merge` | Deep-merge an object into a document |
| `doc_append` | Append a value to an array |
| `doc_prepend` | Prepend a value to an array |
| `doc_ensure` | Set a value only if the selector path does not exist |
| `doc_delete_where` | Delete array elements matching a predicate |
| `doc_update` | Update array elements matching a predicate |
| `doc_move` | Move a value from one selector path to another |
| `doc_get` | Read a value by selector (read-only) |
| `doc_has` | Check whether a selector path exists (read-only) |
| `doc_keys` | List object keys at a selector path (read-only) |
| `doc_len` | Count items in an array or object (read-only) |
| `doc_select` | Filter array items by selector (read-only) |
| `doc_flatten` | List all leaf selector paths and values (read-only) |
| `doc_diff` | Compare two structured files (read-only) |
| `search_files` | Search text files for a pattern, including literal, case-insensitive, count, file-only, multiline, invert-match, and assert-count modes. Binary and invalid UTF-8 files are skipped (read-only) |
| `git_status` | Show uncommitted file changes vs git HEAD (read-only) |
| `read_file` | Read file contents with optional line range |
| `replace_text` | Replace text in a text file (literal or regex). Binary and invalid UTF-8 files are skipped |
| `md_upsert_bullet` | Add a bullet under a markdown heading |
| `md_table_append` | Append a row to a markdown table |
| `md_replace_section` | Replace a markdown section by heading |
| `md_insert_after_heading` | Insert content after a markdown heading |
| `md_insert_before_heading` | Insert content before a markdown heading |
| `md_lint` | Lint an AGENTS.md file for common issues |
| `fix_whitespace` | Fix whitespace and line endings in a text file. Binary and invalid UTF-8 files are skipped |
| `create_file` | Create a new file with content |
| `delete_file` | Delete a file |
| `move_file` | Move or rename a file (binary-safe) |
| `apply_patch` | Apply a unified diff |
| `batch` | Run multiple operations in one call (structured objects or line-format strings) |
| `transaction` | Execute atomic multi-file edits with optional format/validate lifecycle |

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

The `batch` tool accepts operations as structured JSON objects (with an `op` field) or as line-format strings, and validates every path before execution. The `transaction` tool accepts either an `operations` array directly or a full plan string. All operation paths and plan-level `cwd` fields are validated for containment before execution. A relative transaction `cwd` still resolves from the server invocation root, then must remain inside that root.

### Shell execution gate

Transaction plans can include `format` and `validate` lifecycle steps that run shell commands (e.g., `cargo fmt`, `cargo test`). Because these execute arbitrary commands, they are **disabled by default** in MCP mode. To enable them, start the server with `--allow-shell`:

```bash
patchloom mcp-server --allow-shell
```

Without `--allow-shell`, any plan containing `format` or `validate` steps is rejected with an error. Plans that only contain file operations work without the flag. Enable `--allow-shell` only when the MCP client is trusted.

## Example tool call

An MCP-capable agent sends:

```json
{
  "method": "tools/call",
  "params": {
    "name": "doc_set",
    "arguments": {
      "path": "config.yaml",
      "selector": "database.port",
      "value": 5432
    }
  }
}
```

Patchloom parses the YAML, changes `database.port` to `5432`, preserves all comments and formatting, and writes the file. The agent receives a success response with no further action needed.

For multi-file atomic edits, use `transaction` with a structured `operations` array:

```json
{
  "method": "tools/call",
  "params": {
    "name": "transaction",
    "arguments": {
      "operations": [
        {"op": "replace", "path": "src/main.rs", "from": "v1", "to": "v2"},
        {"op": "doc.set", "path": "Cargo.toml", "selector": "package.version", "value": "2.0.0"}
      ]
    }
  }
}
```

All operations succeed together or roll back. For advanced use with post-write formatting and validation, use the `plan` string parameter instead:

```json
{
  "method": "tools/call",
  "params": {
    "name": "transaction",
    "arguments": {
      "plan": "{\"version\":\"1\",\"operations\":[{\"op\":\"replace\",\"path\":\"src/main.rs\",\"from\":\"v1\",\"to\":\"v2\"}],\"format\":[{\"cmd\":\"cargo fmt\"}],\"validate\":[{\"cmd\":\"cargo test\",\"required\":true}]}"
    }
  }
}
```

The `plan` string supports `format` and `validate` lifecycle steps (requires `--allow-shell`). With `"strict": true`, writes are rolled back on format or validate failure.
