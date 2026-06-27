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
| `append_file` | Append content to an existing file |
| `delete_file` | Delete a file |
| `move_file` | Move or rename a file (binary-safe) |
| `apply_patch` | Apply a unified diff |
| `batch_replace` | Replace the same text across multiple files atomically |
| `batch_tidy` | Fix whitespace in multiple files atomically |
| `execute_plan` | Execute a full multi-op transaction plan atomically (recommended for complex/multi-file edits; equivalent to CLI `tx`). Supports inline plan or plan_path. |
| `ast_list` | List symbol definitions (functions, classes, structs, enums, methods) in a file or directory (20 languages). Filter by kind. |
| `ast_read` | Read a specific symbol's source code by name from a file. |
| `ast_rename` | Rename identifiers across files using AST-aware renaming (skips strings and comments). |
| `ast_validate` | Validate syntax of source files. Returns parse errors with line numbers. |
| `ast_search` | Structural search using AST queries. Supports S-expression syntax and code patterns with meta-variables. |
| `ast_refs` | Find all references to a symbol across files. Distinguishes definitions from references. |
| `ast_deps` | Extract import/dependency statements from source files (Rust, Python, JS/TS, Go, Java, C/C++, Ruby, PHP). |
| `ast_map` | Generate a ranked repository map using PageRank over the symbol reference graph. Token-budget-aware output. |
| `ast_diff` | Structural diff between two versions of a file. Shows added, removed, and modified symbols. |
| `ast_impact` | Transitive impact analysis: trace the reference graph to find all dependents of a symbol. |
| `ast_replace` | Replace text only within a specific symbol's body using AST scoping. |
| `ast_insert` | Insert code before/after a symbol or inside a container (module, class, impl block). |
| `ast_wrap` | Wrap a symbol in a container (module, class, namespace, impl block, or custom wrapper). |
| `ast_imports` | List, add, remove, or deduplicate import statements in source files. |
| `ast_reorder` | Reorder symbols by strategy: alphabetical, reverse, kind-first, or custom order. |
| `ast_group` | Move symbols into a new or existing module block within the same file. |
| `ast_move` | Move symbols between files with configurable insertion position. |
| `ast_extract` | Extract a symbol to a new file, optionally unwrapping module blocks. |
| `ast_split` | Split a file by distributing symbols across multiple target files. |

## How MCP mode differs from CLI mode

| Aspect | CLI mode | MCP mode |
|--------|----------|----------|
| Apply behavior | Requires `--apply` flag | Auto-applies (writes are the default) |
| Input format | Shell arguments | Structured JSON parameters |
| Path security | No restriction | Paths must stay within working directory |
| Error format | stderr text | MCP error response with structured content |
| Discovery | Agent reads AGENTS.md | Agent discovers tools via MCP protocol |

## Multi-step plans and concurrency guidance (important for agents)

For any work involving more than one edit (especially on the same file or related files), **prefer the `execute_plan` tool** over issuing many individual tools:

- One `execute_plan` call = atomic execution of a mixed plan (doc.set + md.replace_section + create + replace + ...).
- Plans support `strict: true` (default) for full rollback on format/validate failures.
- Plans can include `write_policy`, `format` steps, `validate` steps — same as CLI `tx`.

Example inline plan (JSON):

```json
{
  "version": "1",
  "strict": true,
  "operations": [
    { "op": "doc.set", "path": "package.json", "selector": "version", "value": "2.0.0" },
    { "op": "md.replace_section", "path": "AGENTS.md", "heading": "## Commands", "content": "Run make check.\n" },
    { "op": "file.create", "path": "REPORT.md", "content": "# Summary\n" }
  ]
}
```

**Critical rules for agents (to avoid lost updates and races):**

- Do **not** issue concurrent write tools against the same path(s) unless using `execute_plan`.
- Serialize writes per path. Parallelize only across completely disjoint paths.
- Per-call "ok" does **not** mean the combined result is coherent if you interleave writers yourself.
- Use one `execute_plan` for any logical multi-edit task.

These semantics are also documented in the tool instructions returned by the MCP server and in `patchloom agent-rules --mode mcp`.

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

## Streamable HTTP transport

By default, the MCP server uses stdio transport (ideal for local IDE/agent integration). With `--http`, the server switches to Streamable HTTP transport, allowing remote MCP clients to connect over the network.

### Basic HTTP

```bash
# Default: listen on 127.0.0.1:8080
patchloom mcp-server --http

# Custom port
patchloom mcp-server --http --port 3000

# Listen on all interfaces
patchloom mcp-server --http --host 0.0.0.0
```

The MCP endpoint is served at `/mcp` (e.g., `http://127.0.0.1:8080/mcp`).

### HTTPS with native TLS

```bash
patchloom mcp-server --http --host 0.0.0.0 --port 443 \
  --tls-cert cert.pem --tls-key key.pem
```

Both `--tls-cert` and `--tls-key` must be provided together. The server uses rustls (no OpenSSL dependency).

### HTTP transport flags

| Flag | Default | Description |
|------|---------|-------------|
| `--http` | off | Use Streamable HTTP transport instead of stdio |
| `--host` | `127.0.0.1` | Bind address (requires `--http`) |
| `--port` | `8080` | Bind port (requires `--http`). Use `0` for an OS-assigned ephemeral port (printed in the startup banner) |
| `--tls-cert` | none | TLS certificate PEM file; enables HTTPS (requires `--http` and `--tls-key`) |
| `--tls-key` | none | TLS private key PEM file (requires `--http` and `--tls-cert`) |

### Connecting a remote MCP client

Use any MCP client that supports Streamable HTTP transport. Example with the `rmcp` Rust client:

```rust
use rmcp::transport::StreamableHttpClientTransport;

let transport = StreamableHttpClientTransport::from_uri("http://localhost:8080/mcp");
let client = ().serve(transport).await?;
```

### Logging with HTTP transport

The `--log` flag works identically with HTTP transport:

```bash
patchloom mcp-server --http --log /tmp/mcp.log
```

### Graceful shutdown

The HTTP server shuts down gracefully on Ctrl+C (SIGINT): active SSE streams are terminated, in-flight requests complete, and the server exits cleanly.

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
