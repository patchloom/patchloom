# Comparisons and when to use what

Patchloom is **not** a generic filesystem MCP and **not** a drop-in replacement for full coding agents. Use this page to choose tools.

## vs official / generic MCP filesystem

| Capability | Generic filesystem MCP | Patchloom |
|------------|------------------------|-----------|
| Read / write / list files | Yes | Yes (`read`, create, delete, …) |
| Dry-run / preview before write | Rare | Default dry-run; exit **2** when changes would apply |
| JSON / YAML / TOML by selector | No (text edit) | `doc` (parser-backed; multi-doc YAML honesty) |
| Markdown section / table / bullet | No | `md` |
| AST rename / symbol ops | No | `ast` |
| Multi-file atomic plan + undo | No | `tx` / `batch` + `undo` |
| Agent-branchable `error_kind` | Weak | `binary`, `invalid_encoding`, `already_exists`, `format_failed`, … |

**Use a generic filesystem MCP** for simple read/write outside structured agent workflows.

**Use Patchloom** when the agent must mutate configs, markdown structure, or multi-file plans with preview and peels.

Install notes: [MCP setup](mcp-setup.md). Registry name: `io.github.patchloom/patchloom`.

## vs yq / dasel / jq

| Capability | yq / dasel | Patchloom |
|------------|------------|-----------|
| Selector mutate JSON/YAML/TOML | Yes | Yes (`doc`) |
| Same tool for md / AST / replace / tx | No | Yes |
| Agent JSON + exit codes | Shell stderr | Stable `error_kind`, dry-run exit 2 |
| Multi-document YAML stream honesty | Limited | Bare-key type_error; `0.key` / `[0]` |
| MCP + Rust library host contracts | No | MCP + `ReplaceOptions::for_agent` |

**Use yq/dasel** in human scripts and one-off shell.

**Use Patchloom** inside agent loops (CLI, MCP, or embedder library).

## vs ast-grep (complement)

[ast-grep](https://github.com/ast-grep/ast-grep) owns **structural code search and pattern rewrite** (syntax-tree patterns, codemods). Patchloom owns **host-safe apply** for configs, markdown, multi-file transactions, and agent honesty.

| Task | Prefer |
|------|--------|
| Find every call matching a code shape | ast-grep (or `ast search` / project tools) |
| Rename a symbol with project-aware AST | Patchloom `ast rename` / `ast_rename_project` |
| Set `database.port` in YAML without losing comments | Patchloom `doc set` |
| Atomic multi-file apply with undo | Patchloom `tx` / `batch` |
| After fuzzy text replace, refuse over-wide spans (library host) | Patchloom `fuzzy_span_suspicious` |

Typical pipeline: discover with ast-grep → apply policy-safe writes with Patchloom.

## vs Morph Fast Apply (and similar merge APIs)

[Morph](https://www.morphllm.com/) (Fast Apply) is a **cloud apply model**: the agent emits a short snippet (often with `// ... existing code ...` markers); Morph merges into the file at high token rates. That optimizes whole-file rewrites inside full agent loops.

| | Morph Fast Apply | Patchloom |
|--|------------------|-----------|
| Execution | Network API | Local binary / library |
| Determinism | Model merge | Parser / exact / controlled fuzzy |
| Configs / multi-doc YAML / md sections | Not the product focus | Core |
| Dry-run exit codes + peels | N/A | Core |
| Offline / air-gapped | No | Yes |

**Interop (no integration required):** a host may use Morph (or IDE apply) for freeform code and Patchloom for structured configs, plans, and peels. Patchloom does **not** depend on paid apply APIs for its core path.

### Morph job migration (verified)

Use this table when someone asks "can patchloom replace Morph for X?" Full PASS/PARTIAL/GAP matrix with re-run commands: [morph-gap-matrix](../plans/morph-gap-matrix.md).

| Morph job | Prefer Patchloom | Notes |
|-----------|------------------|-------|
| Small exact edit | `replace OLD path --new NEW` | Exact; dry-run then `--apply` |
| Whitespace / near-miss | `replace … --fuzzy` | Default **fail-closed** (reports `matched_text`, no write). Hosts: `for_agent` + `fuzzy_span_suspicious`; optional `--allow-absent-old` only when you accept a nearby span |
| Large file, known line/symbol | `replace` or `ast replace PATH SYMBOL --old --new` | Scope to a symbol when possible |
| Scattered multi-hunk / multi-file | `batch` / `tx` / library `apply_content_edits*` | One plan, atomic undo |
| Config / comments | `doc set` | Not text replace |
| Markdown section | `md replace-section` (etc.) | Not whole-file rewrite |
| Preview / revert | default dry-run (exit 2); `undo --apply` | Backup session on apply |
| Lazy `// ... existing code ...` with **no** anchors | Not first-class | **Gap** ([#2018](https://github.com/patchloom/patchloom/issues/2018)): supply matchable `old`, AST target, or `--insert-after` anchor. No Morph-style model merge |
| Freeform "put method in the right place" with no anchors | Not first-class | **Gap** ([#2018](https://github.com/patchloom/patchloom/issues/2018)): use `ast list`/`read` then insert/replace with a chosen anchor |

**Host routing (Morph MCP says "prefer edit_file"):** prefer `doc` / `md` / `ast` / `batch` when structure is known; use `replace` (+ fuzzy only when needed); never whole-file rewrite for a one-line change.

## vs full coding agents (Claude Code, Codex, Cursor, Aider)

Those products own the **agent loop**. Patchloom is a **tool layer** (CLI / MCP / library) they can call. Do not install Patchloom expecting to replace the agent; install it so the agent edits structure safely.

## Context budget tips

Agents pay for tokens on every tool result. Prefer:

- `read` with a **line range** instead of dumping huge files
- `search` with `--count` / `--files-with-matches` (and limits when available) before full content
- `batch` / `tx` / multi-op content edits instead of N sequential replaces
- `--jsonl` when streaming many results
- Soft-skip binary paths; sole binary targets return `error_kind: binary`

See also [Core concepts](concepts.md) and [agent-rules](../../PATCHLOOM.md) output from `patchloom agent-rules`.
