# Patchloom: structured file edits for AI agents

AI coding agents are good at reasoning about code. They are bad at editing config files.

When an agent needs to change a YAML value, it reaches for `sed` or text replacement. That works until the regex strips a comment, breaks indentation, or produces invalid syntax. When the task touches six files, that is six separate tool calls, each a round-trip back to the LLM. And on Windows, `sed` and `jq` do not exist, so the agent falls back to verbose PowerShell or makes errors with unfamiliar syntax.

Patchloom is a single-binary CLI that fixes all three problems.

## What it does

Patchloom edits JSON, YAML, and TOML files by selector, not regex. It preserves comments and formatting. It batches multiple file edits into one tool call. And it works identically on Linux, macOS, and Windows.

```bash
# Edit a YAML value by selector without breaking comments or formatting
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

18 commands cover search, structured editing, batching, transactions, markdown editing, diff patching, and file lifecycle operations. An MCP server mode exposes everything as structured tool calls.

## The honest benchmark

We benchmarked Patchloom against native tools across 11 real agent tasks using Claude Opus 4:

```
Method      Total time (11 tasks)
──────────  ─────────────────────
MCP mode    228.5s
Native      233.8s
CLI mode    321.9s
```

MCP mode wins overall because structured tool calls skip shell syntax construction. MCP wins 5/11 tasks; native wins 3/11; 3 are ties.

But Patchloom is not faster than native tools for everything. We say so in the agent instructions it generates:

| Task | Use Patchloom? | Why |
|------|---------------|-----|
| Edit JSON/YAML/TOML by selector | Yes | Parser-backed, comments preserved |
| Batch edits across multiple files | Yes | One tool call instead of N |
| Append a row to a markdown table | Yes | Heading-aware, no line number guessing |
| Read a single file | No | Native `read_file` is faster |
| Simple text search | No | Native `grep` is faster |
| Single-file text replacement | No | Native `search_replace` is faster |

The right tool for the right job. Patchloom tells agents when not to use it.

## What makes it different

**Parser-backed edits.** `doc set config.yaml database.port 5432` parses the YAML, changes the value by selector, and writes valid output. Comments, formatting, and indentation are preserved. A `sed` command cannot do this.

**Batching.** Six file edits via native tools means six round-trips to the LLM. One `batch` call does the same work in a single round-trip.

**Atomic transactions.** `tx` plans group multiple operations with format/validate lifecycle hooks. `strict: true` reverts every file if any step fails.

**Cross-platform.** One binary, identical behavior on Linux, macOS, and Windows. No dependency on `sed`, `jq`, `grep`, or any other Unix tool.

**Agent instructions.** `patchloom agent-rules` generates an `AGENTS.md` file that teaches the agent exactly when and how to use each command. The agent reads it and knows the tool surface without exploration.

## How to try it

```bash
cargo install patchloom                    # crates.io
brew install patchloom/tap/patchloom       # macOS / Linux (Homebrew)
```

Or download a prebuilt binary from [GitHub Releases](https://github.com/patchloom/patchloom/releases).

Then generate agent instructions for your project:

```bash
patchloom agent-rules >> AGENTS.md
```

For MCP mode:

```bash
cargo install patchloom --features mcp
patchloom mcp-server
```

## The numbers

- 1001 tests (496 unit + 505 integration)
- Verified on Grok 4.3, GPT-5.4, and Claude Opus 4.6
- 18 CLI commands + MCP server with structured tool calls
- MIT OR Apache-2.0 licensed
- Rust 1.95+, zero unsafe code

## Links

- Repository: [github.com/patchloom/patchloom](https://github.com/patchloom/patchloom)
- VS Code extension: [github.com/patchloom/patchloom-vscode](https://github.com/patchloom/patchloom-vscode)
- Documentation: [Installation](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/installation.md) | [Quick start](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/quickstart.md) | [MCP setup](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/mcp-setup.md)
