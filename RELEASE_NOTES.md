# Patchloom 0.5.0

The 0.5.0 release brings network-accessible MCP, a richer library API for embedders, and a major internal quality push: 55 PRs, +10,900 lines across 67 files, and 1,678 tests (up from ~1,350 in 0.4.0).

## Highlights

### Streamable HTTP/HTTPS transport for the MCP server

The MCP server is no longer limited to stdio. The new `--http` flag starts a Streamable HTTP server that any remote MCP client can connect to, bringing patchloom in line with the MCP specification's network transports.

```bash
# HTTP on localhost
patchloom mcp-server --http --port 3000

# HTTPS with TLS termination (rustls)
patchloom mcp-server --http --tls-cert cert.pem --tls-key key.pem

# Ephemeral port (OS-assigned, printed in the startup banner)
patchloom mcp-server --http --port 0
```

Supports graceful shutdown (Ctrl+C), configurable bind address (`--host`), and automatic host header restriction for loopback-only binds. Gated behind the `mcp-http` Cargo feature, enabled by default.

### `execute_plan` MCP tool

Agents can now submit a full multi-operation transaction plan in a single MCP tool call instead of chaining individual calls. This is the MCP equivalent of `patchloom tx` and supports all 25 operation types with atomic rollback.

```json
{
  "tool": "execute_plan",
  "arguments": {
    "plan": "op: doc.set\npath: config.yaml\nselector: version\nvalue: \"2.0.0\"\n---\nop: replace\npath: README.md\nold: \"1.0.0\"\nnew: \"2.0.0\""
  }
}
```

### Library API for Rust embedders

Building on 0.4.0's PathGuard foundation, this release fills the remaining gaps for downstream Rust projects embedding patchloom as a library:

- **`files` feature**: Pure file helpers (binary detection, text reading, parallel file processing) available without pulling the full CLI.
- **`api::search_directory`**: Recursive grep-like search with full `SearchOptions` (glob, ignore files, context lines, multiline).
- **`PlanReport` return type**: `execute_plan` now returns structured results, not just exit codes.
- **Search ignore customization**: `custom_ignore_filenames`, `exclude_patterns`, and `max_results` across CLI, MCP, and library surfaces.
- **AST rewrite helpers**: `ast::rename_symbol` and `ast::validate_syntax` available under the `ast` feature without `cli`.

### Internal quality overhaul

16 refactoring PRs restructured the MCP server internals:

- A `mcp_tool!` macro eliminated ~800 lines of per-handler boilerplate, with all 35 MCP tools now using a consistent pattern.
- `src/api.rs` was decomposed into focused submodules.
- Transaction execution was unified between CLI (`cmd/tx`) and library (`src/tx`) paths, sharing `TxState`, `CachedDoc`, and `TxExecResult` types.
- GlobalFlags construction boilerplate was reduced across the codebase.

20 test-focused PRs added 300+ new tests and strengthened existing ones: bare `.success()` assertions were upgraded to `.code(0)`, weak `contains("a")` substring checks were replaced with precise JSON field matches, and edge cases for MCP error paths, guard rejection, and TLS validation were added.

## Bug fixes

- HTTPS server banner now shows the actual bound port when using `--port 0` (previously showed `:0`).
- File MCP tools (`create_file`, `delete_file`, `move_file`, `append_file`) now route through the tx engine for structured JSON responses, consistent with all other write tools.
- YAML nested key creation and markdown move-section body attachment fixed.
- `PathGuard::allow_temp_directory()` now handles macOS `/private/tmp` symlink resolution.
- Crates.io publish workflow made resilient to lock/index drift.

## Numbers

| Metric | v0.5.0 |
|--------|--------|
| CLI commands | 22 |
| MCP tools | 35 (was 31) |
| Unit tests | 948 |
| Integration tests | 720 |
| PTY tests | 10 |
| **Total tests** | **1,678** |
| PRs in this release | 55 |
| Lines changed | +10,954 / -3,864 |

## Install

```bash
# Homebrew
brew install patchloom/tap/patchloom

# Cargo
cargo install patchloom

# Shell installer (macOS/Linux)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/patchloom/patchloom/releases/latest/download/patchloom-installer.sh | sh
```

All install methods ship with every feature enabled: CLI, stdio MCP, HTTP/HTTPS MCP, and AST operations.
