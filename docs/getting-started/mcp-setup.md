# MCP Setup Guide

Patchloom includes an MCP (Model Context Protocol) server for structured tool calls. MCP-capable AI agents can call Patchloom tools directly with JSON parameters, with no shell command construction, no quoting issues, and no `--apply` flag needed.

## Verify MCP support

The MCP server is included by default in all pre-built binaries, Homebrew, and crates.io installs. Verify it works:

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
| `doc_query` | Query a structured file: has, keys, len, select, or flatten (read-only) |
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
| `md_move_section` | Move a heading section (same file reorder or cross-file) |
| `md_lint` | Lint an AGENTS.md file for common issues |
| `fix_whitespace` | Fix whitespace and line endings in a text file. Binary and invalid UTF-8 files are skipped |
| `create_file` | Create a new file with content |
| `delete_file` | Delete a file |
| `move_file` | Move or rename a file (binary-safe) |
| `apply_patch` | Apply a unified diff |
| `batch_replace` | Replace the same text across multiple files atomically |
| `batch_tidy` | Fix whitespace in multiple files atomically |

## How MCP mode differs from CLI mode

| Aspect | CLI mode | MCP mode |
|--------|----------|----------|
| Apply behavior | Requires `--apply` flag | Auto-applies (writes are the default) |
| Input format | Shell arguments | Structured JSON parameters |
| Path security | No restriction | Paths must stay within working directory |
| Error format | stderr text | MCP error response with structured content |
| Discovery | Agent reads AGENTS.md | Agent discovers tools via MCP protocol |

## Debugging and logging

The MCP server can log every tool call to a JSONL file for debugging and performance analysis. Each line records the tool name, duration, and success/failure status.

Enable logging with the `--log` flag:

```bash
patchloom mcp-server --log /tmp/patchloom-mcp.log
```

Or set the `PATCHLOOM_MCP_LOG` environment variable (the `--log` flag takes precedence):

```bash
export PATCHLOOM_MCP_LOG=/tmp/patchloom-mcp.log
patchloom mcp-server
```

Each line is a JSON object:

```json
{"ts":1749123456789,"tool":"replace_text","duration_ms":3,"ok":true}
{"ts":1749123456800,"tool":"doc_set","duration_ms":5,"ok":false,"error":"file not found"}
```

| Field | Type | Description |
|-------|------|-------------|
| `ts` | number | Unix timestamp in milliseconds |
| `tool` | string | Tool name that was called |
| `duration_ms` | number | Execution time in milliseconds |
| `ok` | boolean | Whether the call succeeded |
| `error` | string | Error message (only present on failure) |

### Configuring logging in your MCP client

**Grok (config.toml)** -- pass the env var to the MCP server subprocess:

```toml
[mcp_servers.patchloom]
command = "/path/to/patchloom"
args = ["mcp-server"]
env = { PATCHLOOM_MCP_LOG = "/tmp/patchloom-mcp.log" }
```

Or use `--log` in the args:

```toml
[mcp_servers.patchloom]
command = "/path/to/patchloom"
args = ["mcp-server", "--log", "/tmp/patchloom-mcp.log"]
```

**Claude Desktop / VS Code / Cursor (JSON)** -- use `--log` in the args:

```json
{
  "mcpServers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server", "--log", "/tmp/patchloom-mcp.log"]
    }
  }
}
```

Or pass the env var (Claude Desktop supports `env` in server config):

```json
{
  "mcpServers": {
    "patchloom": {
      "command": "/path/to/patchloom",
      "args": ["mcp-server"],
      "env": { "PATCHLOOM_MCP_LOG": "/tmp/patchloom-mcp.log" }
    }
  }
}
```

## Security model

The MCP server enforces path containment: all file paths must resolve within the working directory where `patchloom mcp-server` was started. Absolute paths, `../` traversal, and symlinks escaping the working directory are rejected. This prevents an agent from accidentally (or maliciously) editing files outside the project.

Each individual tool validates every path before execution.

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
