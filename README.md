# Patchloom

[![CI](https://github.com/patchloom/patchloom/actions/workflows/ci.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/ci.yml)
[![Security](https://github.com/patchloom/patchloom/actions/workflows/security.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/security.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](./LICENSE-MIT)
[![Tests](https://img.shields.io/badge/tests-661%20passing-brightgreen)](#)

**One binary. Every platform. Structured file edits for AI agents.**

Patchloom is a single-binary CLI that gives AI coding agents safe, structured file editing on any operating system. It edits JSON, YAML, and TOML by key (not regex), preserves comments, batches multiple file edits into one tool call, and works identically on Linux, macOS, and Windows.

```bash
# Edit a YAML key without breaking comments or formatting
patchloom doc set config.yaml database.port 5432 --apply

# Batch 6 file edits into a single tool call
patchloom batch --apply <<'EOF'
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
doc.set config.toml project.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
replace CHANGELOG.md "1.0.0" "2.0.0"
file.create VERSION "2.0.0"
EOF
```

---

## Why Patchloom?

### The problem

AI agents edit files through tool calls. Each call is a round-trip back to the LLM. When a task touches config files, that process has three failure modes:

1. **Syntax corruption.** The agent uses text replacement on JSON, YAML, or TOML and produces invalid output (mismatched braces, broken indentation, lost comments).
2. **Round-trip tax.** Editing 6 files means 6 separate tool calls. Each one waits for the LLM to generate, execute, read the result, and plan the next call.
3. **Platform fragmentation.** On Linux the agent uses `sed`, `jq`, `grep`. On Windows, none of those exist. The agent falls back to verbose PowerShell or makes errors with unfamiliar syntax.

### How patchloom solves each one

| Problem | How patchloom solves it |
|---|---|
| **Syntax corruption** | `doc` commands parse the file, change the value by key path, and write valid output. Comments and formatting are preserved. No regex needed. |
| **Round-trip tax** | `batch` and `tx` combine N operations into 1 tool call. Six file edits become one command with atomic rollback on failure. |
| **Platform fragmentation** | Single static binary with zero dependencies. Same commands, same flags, same behavior on Linux, macOS, and Windows. |

### What changes with patchloom

<table>
<tr>
<td width="50%">

**Without patchloom** (6 tool calls)

```
Agent: edit file 1  ─── tool call ───▶  15s
Agent: edit file 2  ─── tool call ───▶  15s
Agent: edit file 3  ─── tool call ───▶  15s
Agent: edit file 4  ─── tool call ───▶  15s
Agent: edit file 5  ─── tool call ───▶  15s
Agent: edit file 6  ─── tool call ───▶  15s
                                    Total: ~90s
```

</td>
<td width="50%">

**With patchloom batch** (1 tool call)

```
Agent: batch with
  all 6 edits     ─── tool call ───▶  25s



                  5 round-trips saved
                                    Total: ~25s
```

</td>
</tr>
</table>

### Key capabilities

| Capability | What it does | Example |
|---|---|---|
| **Parser-backed edits** | Edit JSON/YAML/TOML by key, preserving comments and formatting | `doc set config.yaml db.port 5432 --apply` |
| **Batch N files in 1 call** | `batch` and `tx` combine operations into one tool call with rollback | `batch --apply < ops.txt` |
| **Comment preservation** | YAML/TOML comments survive all edits, including array resizing | `doc append config.yaml tags '"v2"' --apply` |
| **Heading-aware markdown** | Edit sections, tables, and bullets by heading, not line number | `md table-append --file README.md --heading "API" --row "\| new \| row \|" --apply` |
| **Atomic rollback** | `strict: true` reverts every file if format or validate steps fail | `tx --plan plan.json --apply` |
| **MCP server** | Expose all operations as structured MCP tool calls (no shell syntax needed) | `patchloom mcp-server` |
| **Cross-platform** | Identical behavior on Linux, macOS, Windows. No `sed`, `jq`, `grep` required. | Same binary everywhere |

### When to use patchloom vs native tools

Patchloom is not a replacement for all file operations. Its instructions tell agents exactly when to use it and when native tools are faster:

| Task | Use patchloom? | Why |
|---|---|---|
| Edit a JSON/YAML/TOML value by key | **Yes** | Parser guarantees valid output, preserves comments |
| Edit 3+ files in one task | **Yes** | `batch`/`tx` eliminates round-trips |
| Append a row to a markdown table | **Yes** | Heading-aware, no line number guessing |
| Read a single file | No | Native `read_file` is faster |
| Simple text search | No | Native `grep` is faster |
| Single-file text replacement | No | Native `search_replace` is faster |

### Benchmark: CLI vs MCP vs native

7-task sessions on Claude Opus 4.6 via Grok 4.3, measuring wall-clock time per task:

```
Task               PL-CLI    MCP    Native
─────────────────  ──────  ──────  ──────
search              26.5s   15.9s   11.6s  ◀ native fastest (expected)
replace             31.4s   20.9s   27.2s  ◀ MCP fastest
doc_set             19.5s   14.7s   14.8s  ◀ MCP ≈ native
md_table            13.3s   15.4s   13.4s  ◀ tied
tx_multi_file       21.5s   35.1s   19.3s  ◀ native fastest
batch_6_files       49.9s   37.8s   28.3s  ◀ native fastest
batch_mixed_ops     25.8s   16.7s   18.3s  ◀ MCP fastest
─────────────────  ──────  ──────  ──────
TOTAL              187.9s  156.4s  132.9s
```

**MCP mode is 17% faster than CLI mode** because the agent discovers tools via protocol instead of reading AGENTS.md, and sends structured calls instead of constructing shell commands.

**What the benchmarks don't capture**: correctness. `doc set` parses JSON/YAML/TOML and guarantees valid output with comments preserved. Native text replacement can corrupt syntax, lose comments, or produce invalid files. Speed matters, but a fast edit that breaks your config is worse than a slower one that's correct.

---

## Install

```bash
# From source
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .

# Coming soon: cargo install patchloom
```

## Quick start

### 1. Generate agent instructions for your project

```bash
patchloom agent-rules >> AGENTS.md
```

Your AI agent reads `AGENTS.md` and learns when to use patchloom vs native tools.

### 2. Edit a config file safely

```bash
# Parser-backed: changes the value, preserves comments and formatting
patchloom doc set config.yaml database.port 5432 --apply
```

### 3. Batch multiple edits into one call

```bash
patchloom batch --apply <<'EOF'
doc.set config.json version "2.0"
md.upsert_bullet AGENTS.md "Rules" "- Always test"
replace src/main.rs "v1" "v2"
EOF
```

Or use a JSON plan with format and validate lifecycle:

```json
{
  "operations": [
    { "op": "doc.set", "path": "config.json", "key": "version", "value": "2.0" },
    { "op": "md.upsert_bullet", "path": "AGENTS.md", "heading": "Rules", "bullet": "- Always test" },
    { "op": "replace", "path": "src/main.rs", "from": "v1", "to": "v2" }
  ],
  "format": [{ "cmd": "cargo fmt --all" }],
  "validate": [{ "cmd": "cargo test", "required": true }]
}
```

```bash
patchloom tx --plan plan.json --apply
```

### 4. Or use MCP for structured tool calls (no shell syntax)

```bash
# Build with MCP support
cargo build --features mcp

# Add to your agent's MCP config
patchloom mcp-server
```

MCP-capable agents call patchloom tools directly as structured JSON, with no shell quoting or command construction. The agent sends `{"file": "config.json", "key": "version", "value": "2.0"}` instead of building `patchloom doc set config.json version '"2.0"' --apply`.

## Getting started

| Resource | What you'll learn |
|---|---|
| [Installation](./docs/getting-started/installation.md) | Install options and shell completions |
| [Core concepts](./docs/getting-started/concepts.md) | Write modes, transaction plans, exit codes |
| [Quickstart](./docs/getting-started/quickstart.md) | 5-minute walkthrough |
| [Reference](./docs/reference/README.md) | Every command, operation, and mode |
| [Examples](./examples/README.md) | Transaction plan templates |

## Commands

### Agent-optimized (these are faster or safer than native tools)

| Command | What it does | When to use |
|---|---|---|
| `batch` | Line-oriented multi-file edits in 1 call | Editing 3+ files with simple syntax |
| `tx` | JSON plan with format/validate lifecycle | Complex multi-file edits with rollback |
| `doc` | Parser-backed JSON/YAML/TOML edits | Changing config values without breaking syntax |
| `md` | Heading-aware markdown edits | Updating tables, sections, bullets in docs |
| `patch` | Apply unified diffs with stale detection | Replaying patches safely |
| `hygiene` | Whitespace and newline normalization | CI checks for text hygiene |
| `mcp-server` | MCP protocol server (structured tool calls) | MCP-capable agents (no shell syntax) |

### General-purpose (also useful in scripts and CI)

| Command | Description |
|---|---|
| `search` | Fast literal or regex search across a repo |
| `replace` | Mechanical string replacement with diff preview |
| `create` | Create a new file with content |
| `delete` | Delete a file |
| `read` | Read file contents with optional line range |
| `status` | Show which files have uncommitted changes |
| `completions` | Generate shell completions (bash, zsh, fish, elvish) |
| `agent-rules` | Generate agent instructions for your project |

## Usage

### search

Search for a pattern across all files:

```
patchloom search 'TODO' src/
```

Regex search with context lines:

```
patchloom search 'fn\s+\w+' src/ -C 2
```

List only file paths with matches:

```
patchloom search 'TODO' --files-with-matches src/
```

Count matches per file:

```
patchloom search 'error' --count src/
```

Literal string search (no regex):

```
patchloom search --literal 'foo(bar)' src/
```

Show lines that do NOT match a pattern:

```
patchloom search -v 'TODO' src/
```

Multiline search (dot matches newlines, pattern spans lines):

```
patchloom search --multiline 'fn main\(\).*\}' src/
```

Case-insensitive search:

```
patchloom search -i 'todo' src/
```

Show 3 lines after each match (like grep -A):

```
patchloom search -A 3 'fn main' src/
```

Show 1 line before and 5 after:

```
patchloom search -B 1 -A 5 'TODO' src/
```

### replace

Replace text across files (preview diff by default, write with `--apply`):

```
patchloom replace --from 'old_name' --to 'new_name' src/ --apply
```

Multiline regex replace (dot matches newlines, pattern spans lines):

```
patchloom replace --regex --multiline --from 'fn main\(\).*\}' --to 'fn main() {}' src/ --apply
```

Regex replace with capture groups:

```
patchloom replace --regex --from 'version = "(\d+)\.(\d+)\.(\d+)"' --to 'version = "$1.$2.99"' Cargo.toml --apply
```

Idempotent replace (succeeds even if text not found):

```
patchloom replace --from 'legacy_name' --to 'new_name' --if-exists --apply
```

Replace only the Nth occurrence (1-based):

```
patchloom replace --from 'TODO' --to 'DONE' --nth 2 src/main.rs --apply
```

Case-insensitive replace:

```
patchloom replace --from 'error' --to 'warning' -i src/ --apply
```

### doc

Read a JSON value:

```
patchloom doc get package.json name
```

Check if a key exists (prints `true` or `false`, always exit 0):

```
patchloom doc has config.yaml database.host
```

List keys of an object:

```
patchloom doc keys package.json .
```

Get the length of an array or object:

```
patchloom doc len package.json dependencies
```

Set a YAML key:

```
patchloom doc set config.yaml server.port 8080 --apply
```

Delete a key:

```
patchloom doc delete config.json deprecated_field --apply
```

Merge an object into a document:

```
patchloom doc merge config.json --value '{"settings": {"debug": true}}' --apply
```

Append to an array:

```
patchloom doc append config.json tags '"new-tag"' --apply
```

Prepend to an array:

```
patchloom doc prepend config.json tags '"first-tag"' --apply
```

Ensure a key exists (idempotent set, only writes if missing):

```
patchloom doc ensure config.json defaults.timeout 30 --apply
```

Move or rename a key:

```
patchloom doc move config.json old_name new_name --apply
```

Filter array items by selector:

```
patchloom doc select config.json "users[active=true]"
```

Update all matching nodes:

```
patchloom doc update config.json "servers[*].enabled" true --apply
```

List all leaf key paths and values in a file:

```
patchloom doc flatten config.json
```

Compare two structured files:

```
patchloom doc diff old.json new.json
```

Delete items from a YAML array by predicate:

```
patchloom doc delete-where config.yml contact_links --predicate 'name=Old Entry' --apply
```

### md

Replace a section in a Markdown file:

```
patchloom md replace-section --file AGENTS.md --heading "Rules" --content "New rules here" --apply
```

Insert content after a heading (without replacing the existing section):

```
patchloom md insert-after-heading --file CHANGELOG.md --heading "## Unreleased" --content "- Added new feature" --apply
```

Insert content before a heading:

```
patchloom md insert-before-heading --file AGENTS.md --heading "## Safety rules" --content "New section content" --apply
```

Add a bullet under a heading if not already present (idempotent):

```
patchloom md upsert-bullet --file AGENTS.md --heading "## Rules" --bullet "- Always run tests before committing" --apply
```

Append a row to a markdown table:

```
patchloom md table-append --file README.md --heading "## Features" --row "| new | feature |" --apply
```

Lint an AGENTS.md file for common issues:

```
patchloom md lint-agents --file AGENTS.md
```

Remove duplicate headings:

```
patchloom md dedupe-headings --file AGENTS.md --apply
```

### create

Create a new file:

```
patchloom create --file AGENTS.md --content "# Project Rules" --apply
```

Create from stdin:

```
echo "generated content" | patchloom create --file output.txt --stdin --apply
```

Overwrite an existing file:

```
patchloom create --file config.json --content '{}' --force --apply
```

### delete

Delete a file:

```
patchloom delete --file obsolete.txt --apply
```

### read

Read a file:

```
patchloom read src/main.rs
```

Read a specific line range:

```
patchloom read src/main.rs --lines 10:25
```

Read multiple files at once:

```
patchloom read src/main.rs src/lib.rs Cargo.toml
```

Get structured JSON output for multiple files:

```
patchloom --json read src/main.rs src/lib.rs
```

### status

Show which files have uncommitted changes:

```
patchloom status
```

Get structured JSON output:

```
patchloom --json status
```

### patch

Apply a unified diff:

```
patchloom patch apply --file changes.patch --apply
```

Check whether a patch applies cleanly (without writing):

```
patchloom patch check --file changes.patch
```

### hygiene

Check files for trailing whitespace, mixed line endings, and missing final newlines:

```
patchloom hygiene check src/
```

Fix issues across a directory:

```
patchloom hygiene fix . --ensure-final-newline --apply
```

### tx

Run a multi-operation plan atomically:

```
patchloom tx --plan plan.json --apply
```

Read the plan from stdin:

```
echo '{"operations": [...]}' | patchloom tx --plan - --apply
```

Get structured JSON output for CI pipelines:

```
patchloom --json tx --plan plan.json --apply
```

### batch

Batch multiple operations in a simple line-oriented format:

```
patchloom batch --apply <<'EOF'
doc.set config.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
file.create VERSION "2.0.0"
md.upsert_bullet CHANGELOG.md "## Changes" "- Bumped to 2.0.0"
hygiene.fix src/main.rs
EOF
```

Supported operations: `doc.set`, `doc.delete`, `doc.merge`, `doc.ensure`, `doc.append`, `doc.prepend`, `replace`, `file.create`, `file.delete`, `md.upsert_bullet`, `md.table_append`, `hygiene.fix`.

For operations needing format/validate lifecycle, regex, or `--nth`, use `tx` with a JSON plan instead.

### mcp-server

Start an MCP server for structured tool calls (requires `--features mcp` build):

```bash
# Build with MCP support
cargo build --features mcp

# Start the server (agents connect via stdio)
patchloom mcp-server
```

MCP-capable agents discover patchloom's 13 tools via the MCP protocol and call them with structured JSON, with no shell quoting or command syntax. Add to your agent's MCP config:

```toml
[mcp_servers.patchloom]
command = "patchloom"
args = ["mcp-server"]
```

## Shell completions

Generate shell completions for your shell:

```bash
# bash
patchloom completions bash > /etc/bash_completion.d/patchloom

# zsh
patchloom completions zsh > ~/.zfunc/_patchloom

# fish
patchloom completions fish > ~/.config/fish/completions/patchloom.fish

# elvish
patchloom completions elvish > ~/.config/elvish/rc.elv
```

### agent-rules

Generate instructions that teach AI agents when and how to use patchloom:

```
patchloom agent-rules >> AGENTS.md
```

The generated instructions tell agents to use native tools for simple operations and patchloom for batching, structured edits, and safety-critical operations.

## Transaction plan format

The `tx` command accepts a JSON plan with an array of operations:

```json
{
  "strict": true,
  "write_policy": { "ensure_final_newline": true },
  "operations": [
    { "op": "replace", "path": "src/main.rs", "from": "old", "to": "new" },
    { "op": "replace", "path": "src/main.rs", "from": "old", "to": "new", "nth": 2 },
    { "op": "replace", "glob": "*.rs", "mode": "regex", "from": "v\\d+", "to": "v2" },
    { "op": "replace", "path": "src/lib.rs", "from": "old_api", "to": "new_api", "case_insensitive": true },
    { "op": "replace", "path": "src/lib.rs", "from": "legacy_call", "to": "modern_call", "if_exists": true },
    { "op": "doc.set", "path": "config.json", "key": "version", "value": "2.0" },
    { "op": "doc.delete", "path": "config.json", "key": "deprecated" },
    { "op": "doc.merge", "path": "config.json", "value": {"new_key": true} },
    { "op": "doc.append", "path": "config.json", "key": "items", "value": "new_item" },
    { "op": "doc.prepend", "path": "config.json", "key": "items", "value": "first_item" },
    { "op": "doc.update", "path": "config.json", "key": "servers[*].enabled", "value": true },
    { "op": "doc.move", "path": "config.json", "from": "old_key", "to": "new_key" },
    { "op": "doc.ensure", "path": "config.json", "key": "defaults.timeout", "value": 30 },
    { "op": "doc.delete_where", "path": "config.yaml", "key": "items", "predicate": "name=old" },
    { "op": "md.replace_section", "path": "README.md", "heading": "Notes", "content": "Updated." },
    { "op": "md.insert_after_heading", "path": "README.md", "heading": "Notes", "content": "After." },
    { "op": "md.insert_before_heading", "path": "README.md", "heading": "Notes", "content": "Before." },
    { "op": "md.upsert_bullet", "path": "AGENTS.md", "heading": "Rules", "bullet": "- New rule" },
    { "op": "md.table_append", "path": "README.md", "heading": "Features", "row": "| new | feat |" },
    { "op": "md.dedupe_headings", "path": "AGENTS.md" },
    { "op": "hygiene.fix", "path": "src/main.rs" },
    { "op": "file.create", "path": "new.txt", "content": "hello" },
    { "op": "file.create", "path": "existing.txt", "content": "overwrite", "force": true },
    { "op": "file.delete", "path": "obsolete.txt" },
    { "op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-old\n+new" },
    { "op": "read", "path": "src/main.rs", "lines": "1:10" },
    { "op": "search", "path": "src/main.rs", "pattern": "TODO", "context": 2 }
  ],
  "format": [
    { "cmd": "cargo fmt --all", "timeout": 30 }
  ],
  "validate": [
    { "cmd": "make check", "required": true, "timeout": 120 }
  ]
}
```

All operations run in order. If any operation fails, all changes are rolled back and no files are written (exit code 7). Pass `--apply` to write to disk.

Plans support three lifecycle arrays and an optional write policy:

- **operations**: The mutations to apply.
- **format**: Shell commands that run after all operations are written to disk but before validation. Use for code formatters (`cargo fmt`, `prettier`, `black`). Each step accepts an optional `timeout` in seconds (default: 60). Note: files are already on disk when format runs; a format failure exits with code 6. In JSON output, the legacy `error` string still starts with `validation_failed` for backward compatibility, while the additive `error_kind` field is `format_failed`. In strict mode, the command exits with code 7, the legacy `error` prefix becomes `rollback`, and `error_kind` still stays `format_failed` so machine readers keep the root cause.
- **validate**: Shell commands that run after format steps. If a required step fails, the transaction exits with code 6. In JSON output, both the legacy `error` prefix and the additive `error_kind` field are `validation_failed`. In strict mode, the command exits with code 7, the legacy `error` prefix becomes `rollback`, and `error_kind` remains `validation_failed` so machine readers still see the original failure type. Each step accepts an optional `timeout` in seconds (default: 60). Like format, validation runs after writes are committed.
- **write_policy**: Optional object with `ensure_final_newline` (bool), `normalize_eol` (`"lf"` or `"crlf"`), and `trim_trailing_whitespace` (bool). Applied to all pending content (including `file.create`) before writing to disk. CLI write flags such as `--ensure-final-newline`, `--normalize-eol`, `--trim-trailing-whitespace`, and `--respect-editorconfig` also apply to `tx`; plan-level `write_policy` entries override conflicting CLI settings.
- **strict**: Optional boolean (default: `false`). When `true`, a format or validation failure reverts all file writes and exits with code 7 (ROLLBACK) instead of code 6. Created files are removed; modified files are restored to their original content.

All shell commands in `format` and `validate` execute via the host platform shell (`sh -c` on Unix, `cmd /C` on Windows); only use plans from trusted sources.

### Operation ordering

Operations execute in array order. When multiple operations target the same file, each sees the result of the previous one. Key rules:

- **Last write wins**: If operations 1 and 3 both modify `config.json`, operation 3 sees the content left by operation 1.
- **Delete then create**: A `file.delete` followed by `file.create` (with `force: true`) on the same path recreates the file with the new content. The deletion is unset by the subsequent write.
- **Delete then replace**: A `file.delete` sets the pending content to empty. A subsequent `replace` on the same path sees empty content, so the `from` pattern will not match unless it matches the empty string.

## Symlink behavior

`atomic_write` follows symlinks: it writes to the target of the symlink, not the symlink itself. This is because the write creates a temp file in the parent directory and renames it over the target path, which `rename(2)` resolves through symlinks. If you need to replace a symlink itself, delete and recreate it.

## Global flags

Read-only flags (available on all commands):

| Flag                  | Description                                       |
|-----------------------|---------------------------------------------------|
| `--json`              | Emit machine-readable JSON output                 |
| `--jsonl`             | Emit one JSON object per result line              |
| `--cwd <dir>`         | Set working directory                             |
| `--glob <pattern>`    | Restrict target files by glob (repeatable)        |
| `--files-from <path>` | Read file list from a file or stdin (`-`)         |
| `-q`, `--quiet`       | Suppress non-JSON human-readable output            |

Write flags (available on write commands: replace, patch, md, doc, hygiene, create, delete, tx):

| Flag                         | Description                                       |
|------------------------------|---------------------------------------------------|
| `--diff`                     | Print unified diff for any write operation         |
| `--apply`                    | Actually mutate files                              |
| `--check`                    | Compute and report changes without writing         |
| `--ensure-final-newline`     | Ensure non-empty written files end with a newline  |
| `--normalize-eol <mode>`    | Normalize line endings after write (keep, lf, crlf)|
| `--trim-trailing-whitespace` | Remove trailing whitespace on touched lines        |
| `--respect-editorconfig`     | Read write policy from `.editorconfig` when present |

## Exit codes

| Code | Name                | Meaning                                  |
|------|---------------------|------------------------------------------|
| 0    | `SUCCESS`           | Operation completed successfully         |
| 1    | `FAILURE`           | General error                            |
| 2    | `CHANGES_DETECTED`  | `--check` found pending changes          |
| 3    | `NO_MATCHES`        | Search or selector matched nothing       |
| 4    | `PARSE_ERROR`       | Input could not be parsed                |
| 5    | `AMBIGUOUS`         | Patch context is stale or ambiguous      |
| 6    | `VALIDATION_FAILED` | A required validation step failed        |
| 7    | `ROLLBACK`          | Transaction aborted, no files written    |

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](./LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE))

at your option.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

All commits must be signed off with `git commit -s`.

### Agent integration tests

`make agent-test` runs 19 pytest scenarios that verify AI agents correctly use patchloom when given instructions. `make bench-agent` runs 3-way benchmarks (CLI vs MCP vs native) across 7 tasks. Use `MODEL=X` to switch models and `RUNS=N` for variance reduction. Requires an LLM API key. Not part of `make check`. See [tests/agent/README.md](./tests/agent/README.md) for details.

## How it works with your AI agent

Two integration modes, same capabilities:

```
┌─────────────────────────────────────────────────────────┐
│  CLI mode (any agent)                                   │
│                                                         │
│  AGENTS.md  ◄── patchloom agent-rules >> ...            │
│  (tells the agent when to use patchloom)                │
│                                                         │
│  Agent reads AGENTS.md at session start                 │
│  ├── Simple edit?     → native tool (faster)            │
│  ├── Config edit?     → patchloom doc (safer)           │
│  ├── Markdown edit?   → patchloom md (smarter)          │
│  └── Multi-file edit? → patchloom batch (batched)       │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  MCP mode (MCP-capable agents)                          │
│                                                         │
│  Agent discovers patchloom tools via MCP protocol       │
│  No shell syntax needed, no quoting errors              │
│  Same operations, structured tool calls                 │
│                                                         │
│  Start: patchloom mcp-server                            │
│  Build: cargo build --features mcp                      │
└─────────────────────────────────────────────────────────┘
```

## Status

661 passing tests across 14 commands. Tested with Grok 4.3, GPT-5.4, and Claude Opus 4.6.

## Security

For current security reporting guidance, see [SECURITY.md](./SECURITY.md).

GitHub private vulnerability reporting will be enabled after the repository becomes public.
