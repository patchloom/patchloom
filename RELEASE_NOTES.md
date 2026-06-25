# Patchloom 0.6.0

The 0.6.0 release rebuilds the MCP server architecture from the ground up, adds 11 new AST tools for code-aware agent workflows, and delivers measurable performance gains for concurrent MCP workloads. 41 PRs, 112 files changed, and 1,816 tests (up from 1,678 in 0.5.0).

## Highlights

### MCP auto-generation: add a tool in 6 lines

The MCP server's internal architecture was redesigned around a declarative registry. 19 of the 43 tools are now auto-generated from `Operation` enum variants via `MCP_TOOL_REGISTRY`, with input schemas derived directly from Rust types using `schemars`. Adding a new tool that maps 1:1 to an existing operation takes 6 lines of metadata instead of a 40-line handler function.

```rust
McpToolMeta {
    tool_name: "new_tool",
    op_name: "new_op",
    description: "Short description. Example: {\"path\": \"file.txt\"}",
    has_strict: true,
    validations: &[FieldValidation::Path("path")],
},
```

The old `mcp_tool!` macro and per-tool params structs are gone. Unknown fields are rejected at the MCP layer. Schema drift between CLI operations and MCP tools is caught automatically by a new params drift test (#921).

### 11 AST tools for code-aware agents

All AST commands are now available as MCP tools, bringing the total from 32 to 43:

| Tool | What it does |
|------|-------------|
| `ast_list` | List symbol definitions (functions, classes, structs) across 20 languages |
| `ast_read` | Read a specific symbol's source code by name |
| `ast_rename` | Rename identifiers across files (skips strings and comments) |
| `ast_validate` | Check syntax and report parse errors with line numbers |
| `ast_search` | Structural search using tree-sitter queries and code patterns |
| `ast_refs` | Find all references to a symbol, distinguishing definitions from uses |
| `ast_deps` | Extract import/dependency statements from source files |
| `ast_map` | Generate a ranked repository map using PageRank over the symbol graph |
| `ast_diff` | Structural diff showing added, removed, and modified symbols |
| `ast_impact` | Transitive impact analysis: trace dependents through the reference graph |
| `ast_replace` | Replace text only within a specific symbol's body |

AST handlers are extracted into a dedicated `ast_tools.rs` module (633 lines), keeping `mcp/mod.rs` focused on infrastructure.

### Performance: spawn_blocking, tree caching, PageRank convergence

Three changes improve MCP server responsiveness under concurrent load:

- **`spawn_blocking` for all sync I/O.** Every MCP handler that touches the filesystem or runs tree-sitter parsing now executes on Tokio's blocking thread pool. This was harmless for stdio transport (single-threaded) but is critical for HTTP/HTTPS transport under concurrent requests.

- **Tree-sitter parse tree caching.** Repeated AST queries against the same file reuse the cached parse tree instead of re-parsing. This is especially impactful for `ast_refs` and `ast_impact`, which traverse the same files multiple times.

- **PageRank convergence.** `ast_map` now uses L1-norm convergence (threshold 1e-6) instead of a fixed 20 iterations. Most graphs converge in 8-12 iterations; complex ones that need more get them automatically.

### Deep structural refactoring

18 refactoring PRs restructured the codebase for long-term maintainability:

- **Write-command state machine** (#912): The repeated preview/check/apply/confirm branching logic was extracted into a shared state machine used by all write commands, eliminating a class of bugs where new commands forgot a mode.
- **Ops module decomposition** (#911, #917): Pure data operations (`doc`, `search`) were moved from `cmd/` to `ops/`, separating business logic from CLI wiring.
- **WritePolicy unification** (#916): Two parallel `WritePolicy` types and the `EolNormalization` enum were merged into a single source of truth.
- **Schema derivation** (#918, #919, #920): Operation schemas are now derived from Rust types via `schemars` + a central `OPERATION_REGISTRY`, replacing hand-maintained schema definitions.
- **Doc mutation dispatch** (#888, #910): All `doc_*` mutations now route through a single `DocMutation` dispatcher with shared validation.
- **Integration test modularization** (#878): The 20,935-line `integration.rs` monolith was split into 28 focused test modules.

## Bug fixes

- `doc flatten` now includes empty arrays and empty objects in output instead of silently dropping them (#897).
- Transaction engine preserves idempotent delete semantics: deleting a file that was already removed in the same plan no longer fails (#889).
- `exit_code_to_result` uses the fallback error path for all non-zero exit codes, not just known ones (#941).
- `delete_where` predicates are validated before execution, catching malformed selectors early (#888).
- `search --jsonl` with zero matches now returns exit code 3 (`NO_MATCHES`) consistently (#907).
- `ast_rename` with a no-op rename (old name equals new name) returns early instead of rewriting files identically (#926).
- Git argument injection is blocked across all commands that accept paths (#925, #926).
- Concurrent-write warnings added to all MCP tool descriptions so agents avoid lost-update races (#908).
- Quickstart and concepts documentation updated with missing exit code 9 (`OPERATION_FAILED`) and corrected ordering.

## Numbers

| Metric | v0.5.0 | v0.6.0 | Delta |
|--------|--------|--------|-------|
| CLI commands | 22 | 22 | -- |
| MCP tools | 32 | 43 | +11 |
| Unit tests | 948 | 1,056 | +108 |
| Integration tests | 720 | 750 | +30 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **1,678** | **1,816** | **+138** |
| PRs in this release | -- | 41 | -- |
| Files changed | -- | 112 | -- |

## Install

```bash
# Homebrew (macOS/Linux)
brew install patchloom/tap/patchloom

# crates.io
cargo install patchloom

# Pre-built binaries
# https://github.com/patchloom/patchloom/releases/latest
```
