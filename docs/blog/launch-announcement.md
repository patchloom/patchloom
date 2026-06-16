# Patchloom: structured file edits for AI agents

AI coding agents are remarkably good at reasoning about code. They are remarkably bad at editing config files.

When an agent needs to bump a version in `config.yaml`, it reaches for `sed` or text replacement. That works until the regex strips a YAML comment, breaks indentation, or produces invalid syntax. When the task touches six files, that means six separate tool calls, each a full round-trip back to the LLM. And on Windows, `sed` and `jq` do not exist, so the agent falls back to verbose PowerShell or makes errors with unfamiliar syntax.

We built Patchloom to fix all three problems with a single Rust binary.

## What it does

Patchloom edits JSON, YAML, and TOML files by selector path, not regex. It preserves comments and formatting because it parses the file instead of pattern-matching it. It batches multiple file edits into one tool call, cutting round-trips from six to one. And it works identically on Linux, macOS, and Windows with zero dependencies.

```bash
# Edit a YAML value by selector; comments and formatting survive
patchloom doc set config.yaml database.port 5432 --apply

# Version bump across 6 files in a single tool call
patchloom batch --apply <<'EOF'
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
doc.set config.toml project.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
replace CHANGELOG.md "1.0.0" "2.0.0"
file.create VERSION "2.0.0"
EOF
```

20 commands cover structured document editing, search and replace, markdown section editing, multi-file batching, atomic transactions with rollback, diff patching, file lifecycle operations, operation schema export, and an MCP server that exposes everything as structured tool calls for MCP-capable agents.

## The honest benchmark

We ran 11 real agent tasks, three times each, comparing Patchloom MCP, Patchloom CLI, and native editor tools. The agent was Claude Opus 4 via Grok Build.

```
Method      Total time (11 tasks)   Wins
──────────  ─────────────────────   ────
MCP mode    228.5s                  5/11
Native      233.8s                  3/11
CLI mode    321.9s                  0/11
```

MCP mode wins overall because structured tool calls skip shell syntax construction entirely. CLI mode is slowest because the agent must construct and quote shell commands for every call.

But Patchloom is not faster than native tools for everything. We are upfront about that. The agent instructions Patchloom generates include this table:

| Task | Use Patchloom? | Why |
|------|---------------|-----|
| Edit JSON/YAML/TOML by selector | **Yes** | Parser-backed, comments preserved |
| Batch edits across multiple files | **Yes** | One tool call instead of N |
| Append a row to a markdown table | **Yes** | Heading-aware, no line number guessing |
| Read a single file | No | Native `read_file` is faster |
| Simple text search | No | Native `grep` is faster |
| Single-file text replacement | No | Native `search_replace` is faster |

Patchloom tells agents when not to use it. The right tool for the right job.

## Why it matters

**Comments survive.** `doc set config.yaml database.port 5432` parses the YAML as a concrete syntax tree, changes the value at the selector path, and writes valid output. Inline comments, section comments, indentation, and key ordering are all preserved. A `sed` command cannot do this.

**Round-trips disappear.** Six file edits via native tools means six round-trips to the LLM. One `batch` call does the same work in a single round-trip. In our benchmarks, multi-file batch operations completed in under half the time of sequential native calls.

**Failures roll back.** `tx` plans group multiple operations with format and validate lifecycle hooks. Set `strict: true` and every file reverts if any step fails. No more partial edits to clean up after a broken CI run.

**One binary everywhere.** Same commands, same flags, same behavior on Linux, macOS, and Windows. No dependency on `sed`, `jq`, `grep`, or any Unix-specific tooling.

**Self-documenting.** Run `patchloom agent-rules` to generate an `AGENTS.md` file that teaches the agent exactly when and how to use each command. The agent reads it, learns the tool surface, and knows which tasks to handle natively and which to hand to Patchloom.

## Try it

```bash
cargo install patchloom                    # crates.io
brew install patchloom/tap/patchloom       # macOS / Linux (Homebrew)
```

Or download a prebuilt binary from [GitHub Releases](https://github.com/patchloom/patchloom/releases).

Set up your project in one command:

```bash
patchloom init
```

This creates `AGENTS.md` in a new project or appends the rules to an existing agent instructions file, offers shell completions, and detects MCP configuration. If `.vscode/` or `.cursor/` exists, it prints ready-to-copy `.vscode/mcp.json` or `.cursor/mcp.json` snippets. Or generate just the agent instructions:

```bash
patchloom agent-rules >> AGENTS.md
```

For MCP mode (structured tool calls, no shell syntax):

```bash
cargo install patchloom
patchloom mcp-server
```

There is also a [VS Code extension](https://github.com/patchloom/patchloom-vscode) that handles binary detection, `AGENTS.md` generation, and MCP configuration from the command palette.

## By the numbers

- **1,361 tests**, zero unsafe code
- **20 commands** including MCP server with 30 structured tool calls
- **Agent-tested** with Grok 4.3, GPT-5.4, and Claude Opus 4.6
- **Cross-platform**: Linux (x64, ARM64), macOS (x64, ARM64), Windows (x64)
- **MIT OR Apache-2.0** licensed
- **Rust 1.95+**, single static binary with no runtime dependencies

## What comes next

Since launch, new capabilities have been added including line-oriented replace flags (`--whole-line`, `--range`, `--collapse-blanks`), project config defaults (`.patchloom.toml`), and expanded schema export. See the [reference guide](https://github.com/patchloom/patchloom/blob/main/docs/reference/README.md) for the full command reference and current feature set.

We would love feedback on:

- Which agent workflows hit friction that Patchloom could smooth
- Missing operations or formats (`.env`? `.ini`? HCL?)
- MCP integration with agents we have not tested yet
- Performance reports from real-world projects

File issues, start discussions, or send PRs on GitHub.

## Links

- **Repository**: [github.com/patchloom/patchloom](https://github.com/patchloom/patchloom)
- **VS Code extension**: [github.com/patchloom/patchloom-vscode](https://github.com/patchloom/patchloom-vscode)
- **Documentation**: [Installation](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/installation.md) | [Quick start](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/quickstart.md) | [MCP setup](https://github.com/patchloom/patchloom/blob/main/docs/getting-started/mcp-setup.md)
