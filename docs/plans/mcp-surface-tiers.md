# Design: MCP progressive disclosure / minimal tool pack (#1994)

## Problem

Full default inventory (~58 tools with AST) can overwhelm small agents (context tax on tool schemas). Competitors often ship tiny FS MCP servers.

## Decision

1. **Default remains full surface** (backward compatible).
2. **Progressive disclosure via existing schema tiers** (`patchloom schema --tier weak|medium|strong`) for plan/prompt generation.
3. **Agent-rules** lead with a decision table (which tool family for which task), not the full inventory first.
4. **Env `PATCHLOOM_MCP_SURFACE=core|full`** registers a subset at MCP handshake (implemented).

### Core pack (`PATCHLOOM_MCP_SURFACE=core`)

Exactly these tools (AST off):

- `read_file`, `search_files`, `list_files`, `replace_text`, `batch_replace`
- `doc_get`, `doc_set`, `doc_query`
- `md_replace_section`, `execute_plan`, `server_info`

Defined in `src/cmd/mcp/surface.rs` as `CORE_MCP_TOOL_NAMES` / `McpSurface`.

`server_info` reports `cwd`, `surface`, `tool_count`, package `version`, and MCP `protocol_version`.

Handshake `instructions` (MCP `ServerInfo`) are surface-aware: core mode lists
only the core tools and names `PATCHLOOM_MCP_SURFACE=core`, so agents do not
chase full-inventory tool names that are not registered.

`execute_plan` stays in the core pack so multi-op atomicity still works. Plan
ops are not filtered by surface: a host can still send `file.create` inside a
plan. The env flag is a **tool schema** progressive disclosure, not a plan
capability sandbox.

## Non-goals

- Removing tools without a flag
- Cloud apply models
- Changing registry MCP package name

## Implementation status

- [x] Design decision recorded
- [x] Agent-rules decision table + `PATCHLOOM_MCP_SURFACE` docs
- [x] `McpSurface` parse + filter on registry add / custom disable
- [x] Unit + protocol tests for core list_tools and rejected full-only calls
- [x] Surface-aware handshake `instructions` (core does not advertise full-only tools)
- [x] mcp-setup.md host configuration
- [x] Coding-agent host default docs + `server_info.recommendation` when full (#2070)
- [x] Handshake name map + explore guidance + YAML `style_changed` honesty (#2070)
- [x] `agent-rules --surface core` short pack (#2070)
