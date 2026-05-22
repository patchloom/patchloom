# Changelog

All notable changes to Patchloom are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Commands

18 commands (plus `mcp-server` with `--features mcp`) covering search, structured editing, batching, and file operations:

- **search** / **replace** - Literal and regex search/replace across files, with context lines, `--nth`, `--case-insensitive`, `--insert-before`/`--insert-after`, `--assert-count`, and `--if-exists` for idempotent runs
- **doc** - Parser-backed JSON, YAML, and TOML editing (get, set, delete, merge, append, prepend, update, move, ensure, delete-where, select, flatten, diff). Preserves comments and formatting in YAML and TOML
- **md** - Heading-aware markdown editing (replace-section, insert-after/before-heading, upsert-bullet, table-append, dedupe-headings, lint-agents)
- **tx** - Atomic multi-file transactions with 23 operation types, format/validate lifecycle, strict rollback mode, and YAML/TOML plan format support
- **batch** - Line-oriented multi-operation syntax for quick multi-file edits without JSON
- **patch** - Apply or check unified diffs with fuzz matching
- **create** / **delete** / **rename** - File lifecycle operations with `--apply`/`--check`/`--force` modes. Rename handles binary files natively via `fs::rename`
- **read** / **status** - File inspection and git working-tree status
- **mcp-server** - MCP protocol server exposing all operations as structured tool calls
- **agent-rules** / **completions** - Generate AI agent instructions or shell completions

### Structured file safety

- YAML and TOML edits preserve inline comments, section comments, and formatting (CST-level editing)
- JSON/YAML/TOML mutations are parser-backed; output is always valid
- Sequence-rooted YAML files are handled correctly (falls back to non-preserving serialization when root is not a mapping)
- `doc` operations include depth guard (128 levels) on deep merge to prevent stack overflow
- All file writes go through atomic write (tempfile + rename) for crash safety

### Batching and transactions

- `tx` plans support `format` and `validate` lifecycle arrays with configurable timeouts
- `strict` mode reverts all writes on format/validate failure (exit code 7)
- `read` and `search` operations in tx plans for inspect-then-edit workflows in a single call
- `batch` provides simpler line-oriented syntax covering 20 operation types
- Operation ordering is well-defined: last write wins, delete-then-create works, each op sees prior results

### Correctness fixes

- `file.create` after `file.delete` in the same tx plan no longer silently loses the file
- Empty `--from` in replace/tx is rejected instead of inserting between every character
- tx replace with conflicting fields (`to` + `insert_before`) returns PARSE_ERROR
- tx replace missing all output fields returns PARSE_ERROR instead of silently deleting
- Replace-only tx plans with zero matches return NO_MATCHES (exit 3) instead of SUCCESS
- tx glob replace no longer buffers non-matching files into pending state
- `create --check` verifies parent directory exists (non-force mode)
- Race-free file creation via `File::create_new` for `create --apply` and tx `file.create`

### Output and diagnostics

- `--json` structured output on all commands including tx error paths
- `--jsonl` streaming output for search and read
- Explicit `error_kind` values in tx JSON output (parse_error, rollback, validation_failed, format_failed)
- Stderr diagnostics for silently skipped files in search, replace, and tx glob
- File path context in doc operation error messages

### Testing and benchmarks

- 969 tests (492 unit + 477 integration) verified on Grok 4.3, GPT-5.4, and Claude Opus 4.6
- Agent integration tests: 19 scenarios with invocation-capture shim
- CLI benchmarks vs native tools (grep, sed, jq) using hyperfine
- Agent A/B benchmarks measuring duration, tool calls, and success rate

### Infrastructure

- MSRV: Rust 1.95+
- License: MIT OR Apache-2.0
- CI: fmt, clippy, tests, MSRV check, dependency audit, doc freshness checks, code coverage
- `make check` runs the full gate locally, including generated doc freshness
