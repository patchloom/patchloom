<p align="center">
  <img src="assets/logo.jpg" alt="Patchloom logo" width="200">
</p>

# Patchloom

[![CI](https://github.com/patchloom/patchloom/actions/workflows/ci.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/ci.yml)
[![Security](https://github.com/patchloom/patchloom/actions/workflows/security.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/security.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](./LICENSE-MIT)
[![Tests](https://img.shields.io/badge/tests-860%20passing-brightgreen)](#)

**One binary. Every platform. Structured file edits for AI agents.**

Patchloom is a single-binary CLI that gives AI coding agents safe, structured file editing on any operating system. It edits JSON, YAML, and TOML by key (not regex), preserves comments, batches multiple file edits into one tool call, and works identically on Linux, macOS, and Windows.

![Patchloom demo: batch edit 4 files, preview diff, apply, verify YAML comments preserved](demo/demo.gif)

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
| **MCP server** | Expose all operations as structured MCP tool calls (requires `--features mcp`) | `patchloom mcp-server` |
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

### Correctness over speed

Patchloom is not faster than native tools for simple, single-file edits. Use native tools for those. But native text replacement cannot safely edit structured files: a `sed` on YAML can corrupt indentation, strip comments, or produce invalid syntax. `doc set` parses the file, changes the value by key, and writes valid output. That guarantee is the point.

Where patchloom *is* faster is multi-file batching. Six file edits via native tools means six round-trips to the LLM. One `batch` call does the same work in a single round-trip.

<details>
<summary>Benchmark details (Claude Opus 4.6 via Grok 4.3, 7 tasks)</summary>

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

MCP mode is 17% faster than CLI mode because the agent discovers tools via protocol instead of constructing shell commands.

</details>

---

## Install

```bash
# Core CLI install (requires Rust 1.95+)
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .

# Install with MCP support
cargo install --path . --features mcp
```

`cargo install --path .` gives you the core CLI commands. If you also want
`patchloom mcp-server`, install with `--features mcp`.

Other install channels are planned for public launch, including crates.io,
GitHub Releases binaries, and Homebrew. See
[Installation](./docs/getting-started/installation.md) for the current path and
planned post-launch options.

## Quick start

### 1. Generate agent instructions for your project

```bash
patchloom agent-rules >> AGENTS.md

# Or tailor the output:
patchloom agent-rules --mode mcp >> AGENTS.md            # MCP-only (no CLI examples)
patchloom agent-rules --platform windows >> AGENTS.md     # Windows-only syntax
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

`tx` plans are trusted input. `format` and `validate` run their `cmd` fields through the host shell (`sh -c` on Unix, `cmd /C` on Windows), so only run plans you trust.

### 4. Or use MCP for structured tool calls (no shell syntax)

```bash
# Install or build with MCP support
cargo install --path . --features mcp
# or: cargo build --features mcp

# Then add to your agent's MCP config
patchloom mcp-server
```

MCP-capable agents call patchloom tools directly as structured JSON, with no shell quoting or command construction. The agent sends `{"file": "config.json", "key": "version", "value": "2.0"}` instead of building `patchloom doc set config.json version '"2.0"' --apply`.

## Getting started

| Resource | What you'll learn |
|---|---|
| [Installation](./docs/getting-started/installation.md) | Install options and shell completions |
| [Core concepts](./docs/getting-started/concepts.md) | Write modes, transaction plans, exit codes |
| [MCP setup](./docs/getting-started/mcp-setup.md) | Configure patchloom as an MCP server for your agent |
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
| `mcp-server` | MCP protocol server (requires `--features mcp`) | MCP-capable agents (no shell syntax) |

### General-purpose (also useful in scripts and CI)

| Command | Description |
|---|---|
| `search` | Fast literal or regex search across a repo |
| `replace` | Mechanical string replacement with diff preview |
| `create` | Create a new file with content |
| `delete` | Delete a file |
| `rename` | Move (rename) a file |
| `read` | Read file contents with optional line range |
| `status` | Show which files have uncommitted changes |
| `completions` | Generate shell completions (bash, zsh, fish, elvish) |
| `agent-rules` | Generate agent instructions for your project |

## How patchloom compares

| Tool | Strength | Where patchloom differs |
|------|----------|------------------------|
| **jq** | JSON query/transform | patchloom also handles YAML, TOML, markdown; batches across files; preserves comments |
| **yq** | YAML/JSON query/transform | patchloom preserves YAML comments via CST editing; adds markdown, batching, atomic transactions |
| **dasel** | Multi-format get/set | patchloom adds batching (N edits in 1 call), atomic rollback, format/validate lifecycle |
| **sd** | Regex find/replace | patchloom adds parser-backed structured edits; batching; never produces invalid JSON/YAML |
| **comby** | Structural code patterns | patchloom targets config files and agent workflows, not source code pattern matching |

The key difference: patchloom is designed for AI agent workflows. One `batch` or `tx` call replaces N sequential tool calls, cutting round-trips and eliminating partial-failure states.

## Full command reference

Every command, flag, transaction operation, and exit code is documented in the **[Command Reference](docs/reference/README.md)**.

Quick links:
- [Core concepts](docs/getting-started/concepts.md) (write modes, exit codes, transaction behavior)
- [Examples](examples/README.md) (copy-paste transaction plans)
- [Installation](docs/getting-started/installation.md) (all install methods)

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](./LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE))

at your option.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

For local verification before opening a pull request, run `make check`. It matches the main Linux CI gate: formatting, clippy, unit tests, integration tests, and generated-doc freshness checks. While iterating locally, `make check-fast` runs the Rust formatting, lint, and test path without the generated-doc freshness checks.

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

860 passing tests across 15 core commands, plus the optional `mcp-server` command. Tested with Grok 4.3, GPT-5.4, and Claude Opus 4.6.

## Security

For current security reporting guidance, see [SECURITY.md](./SECURITY.md).

GitHub private vulnerability reporting will be enabled after the repository becomes public.
