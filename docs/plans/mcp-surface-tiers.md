# Design: MCP progressive disclosure / minimal tool pack (#1994)

## Problem

Full default inventory (~56 tools with AST) can overwhelm small agents (context tax on tool schemas). Competitors often ship tiny FS MCP servers.

## Decision (v1: document first, no default break)

1. **Default remains full surface** (backward compatible).
2. **Progressive disclosure via existing schema tiers** (`patchloom schema --tier weak|medium|strong`) for plan/prompt generation.
3. **Agent-rules** lead with a decision table (which tool family for which task), not the full inventory first.
4. **Future (not blocking):** optional env `PATCHLOOM_MCP_SURFACE=core|full` to register a subset at MCP handshake. Core candidate set (illustrative):
   - `read_file`, `search_files`, `replace_text`, `batch_replace`
   - `doc_get`, `doc_set`, `doc_query`
   - `md_replace_section`, `execute_plan`, `server_info`
   - AST off unless surface=full or feature-gated already

## Non-goals

- Removing tools without a flag
- Cloud apply models
- Changing registry MCP package name

## Implementation status

- Decision recorded here (#1994)
- Agent-rules decision tree ships with competitive-docs batch
- Code for `PATCHLOOM_MCP_SURFACE` deferred until a host requests it
