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

**Use a generic filesystem MCP** only when you need pure FS ops and Patchloom is not installed.

**Use Patchloom alone** for list + search + structured edit: MCP `list_files` covers inventory (ignore-aware, capped), so coding agents should not pair Patchloom with a second filesystem MCP for list/read/edit. Prefer `PATCHLOOM_MCP_SURFACE=core` (11 tools including `list_files`).

Install notes: [MCP setup](mcp-setup.md) (Cursor / Claude / Codex paste configs with core). Registry name: `io.github.patchloom/patchloom`.

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

### yq one-liners → Patchloom (agent cheat sheet)

| Shell habit | Patchloom |
|-------------|-----------|
| `yq '.version' package.json` | `patchloom doc get package.json version` or MCP `doc_get` |
| `yq -i '.version = "2.0.0"' package.json` | `patchloom doc set package.json version '"2.0.0"' --apply` (or MCP `doc_set`) |
| `yq 'select(document_index == 0) \| .a' multi.yaml` | `patchloom doc get multi.yaml 0.a` |
| `yq -i '.[0].image = "x"' multi.yaml` | `patchloom doc set multi.yaml 0.image x --apply` |
| `yq '.items[] \| select(.name == "a")' f.yaml` | `patchloom doc select f.yaml items --predicate name=a` (or MCP `doc_query` / plan) |

Prefer parser-backed `doc` over inventing `yq` in agent shells so peels, dry-run exit 2, and multi-doc honesty stay consistent.

## vs ast-grep (complement)

[ast-grep](https://github.com/ast-grep/ast-grep) owns **structural code search and pattern rewrite** (syntax-tree patterns, codemods). Patchloom owns **host-safe apply** for configs, markdown, multi-file transactions, and agent honesty.

| Task | Prefer |
|------|--------|
| Find every call matching a code shape | ast-grep (or `ast search` / project tools) |
| Rename a symbol with project-aware AST | Patchloom `ast rename` / `ast_rename_project` |
| Set `database.port` in YAML without losing comments | Patchloom `doc set` |
| Atomic multi-file apply with undo | Patchloom `tx` / `batch` |
| After fuzzy text replace, refuse over-wide spans (library host) | Patchloom `fuzzy_span_suspicious` / batch `refuse_batch_if_suspicious_fuzzy` |

### Typical pipeline (ast-grep + Patchloom)

1. **Discover** code shapes with ast-grep (or `patchloom ast search` when a simple structural query is enough).
2. **Apply** with Patchloom so peels and undo stay host-safe:
   - identifier rename: `ast rename PATH --old X --new Y --apply` or plan `ast.rename`
   - multi-file atomic apply: `tx` / MCP `execute_plan`
   - config/docs alongside code: `doc set` / `md replace-section` in the same plan
3. **Do not** re-apply the same edit with raw `sed` after Patchloom already wrote.

Example: ast-grep finds call sites; Patchloom `ast rename` + `tx` with `require_change` applies and fails closed if a path misses.

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
| Whitespace / near-miss | `replace … --fuzzy` | Default **fail-closed** (reports `matched_text`, no write). Hosts: `for_agent` + `fuzzy_span_suspicious` / buffer multi-op `refuse_batch_if_suspicious_fuzzy`; optional `--allow-absent-old` only when you accept a nearby span |
| Large file, known line/symbol | `replace` or `ast replace PATH SYMBOL --old --new` | Scope to a symbol when possible |
| Scattered multi-hunk / multi-file | `batch` / `tx` / library `apply_content_edits*` | One plan, atomic undo |
| Config / comments | `doc set` | Not text replace |
| Markdown section | `md replace-section` (etc.) | Not whole-file rewrite |
| Preview / revert | default dry-run (exit 2); `undo --apply` | Backup session on apply |
| Lazy `// ... existing code ...` **with** known anchor | `apply-fragment --after/--before/--old` (markers stripped) | **PASS** (#2018). No Morph-style model merge |
| Lazy markers with **no** anchors | Not supported | Non-goal: supply after/before/old or use Morph |
| Freeform "put method in the right place" with known anchor | `apply-fragment` or `ast` + insert | **PASS** with anchors; no free placement guess |

**Host routing (Morph MCP says "prefer edit_file"):** prefer `doc` / `md` / `ast` / `batch` when structure is known; use `replace` (+ fuzzy only when needed); use `apply-fragment` only with a known after/before/old anchor; never whole-file rewrite for a one-line change.

**Non-goals (do not expect Patchloom to replace Morph here):**

- Anchor-less lazy snippet merge (`// ... existing code ...` with no placement)
- Competing on cloud apply tok/s or network Fast Apply latency

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
