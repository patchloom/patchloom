# Patchloom

Patchloom is a Rust CLI for structured file edits, built for AI coding agents.

AI agents are good at reasoning about code but bad at editing config files. When an agent needs to bump a version in `config.yaml`, it reaches for `sed` or text replacement. That works until the regex strips a YAML comment, breaks indentation, or produces invalid syntax. When the task touches six files, that means six separate tool calls, each a full round-trip back to the LLM. And on Windows, `sed` and `jq` do not exist.

Patchloom fixes all three problems with a single Rust binary.

## What it does

- **Structured editing**: Edit JSON, YAML, and TOML files by selector path, not regex. Comments and formatting are preserved because the file is parsed, not pattern-matched.
- **Batch operations**: Bundle multiple file edits into a single tool call, cutting round-trips from six to one.
- **Cross-platform**: Works identically on Linux, macOS, and Windows with zero dependencies.
- **Safe by default**: All write operations preview changes without mutating files unless `--apply` is passed.
- **MCP server**: Exposes all operations as structured tool calls for MCP-capable agents.

## Quick example

```bash
# Edit a YAML value by selector path; comments and formatting survive
patchloom doc set config.yaml database.port 5432 --apply

# Version bump across 6 files in a single tool call
patchloom batch --apply <<'EOF'
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
EOF
```

## 23 commands

| Category | Command | Description |
|----------|---------|-------------|
| Text | `search` | Literal or regex search across files |
| | `replace` | Mechanical string replacement with diff preview |
| | `patch` | Preview or apply unified diffs |
| Structured | `doc` | Parser-backed JSON, YAML, and TOML operations |
| | `md` | Markdown section-aware operations |
| Code | `ast` | AST-aware symbol operations (20 languages) |
| Files | `append` | Append content to an existing file |
| | `prepend` | Prepend content to the beginning of an existing file |
| | `create` | Create a new file with content |
| | `delete` | Delete a file |
| | `rename` | Rename or move a file |
| | `read` | Read file contents with optional line range |
| | `status` | Show uncommitted file changes |
| Batch | `tx` | Execute a multi-operation plan atomically |
| | `batch` | Line-oriented batch operations |
| Normalize | `tidy` | Whitespace, line ending, and final newline normalization |
| Safety | `undo` | Restore files from backup |
| Agent | `mcp-server` | MCP protocol server for structured tool calls |
| | `agent-rules` | Print agent rules for AGENTS.md |
| | `schema` | Export operation schemas with tier filtering |
| | `explain` | Explain a tx plan in plain English |
| Setup | `init` | Set up patchloom in the current project |
| | `completions` | Generate shell completions |

## As a Rust library

Patchloom is also a Rust library. Add it as a dependency to embed structured file editing in your own tools:

```toml
[dependencies]
patchloom = { default-features = false }
```

The `api` module exposes doc, replace, markdown, file, and patch operations. All API types are `Send + Sync`. Disabling default features omits the MCP server and its async dependencies.

See the [crate documentation](https://docs.rs/patchloom) for the full API surface.

## Get started

Head to [Installation](getting-started/installation.md) to install, then follow the [Quickstart](getting-started/quickstart.md) to make your first edit.
