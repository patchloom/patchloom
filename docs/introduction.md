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
# Edit a YAML value by selector; comments and formatting survive
patchloom doc set config.yaml database.port 5432 --apply

# Version bump across 6 files in a single tool call
patchloom batch --apply <<'EOF'
doc.set package.json version "2.0.0"
doc.set config.yaml app.version "2.0.0"
replace README.md "1.0.0" "2.0.0"
EOF
```

## 20 commands

| Category | Commands |
|----------|----------|
| Text | `search`, `replace`, `patch` |
| Structured | `doc` (JSON/YAML/TOML), `md` (Markdown) |
| Files | `create`, `delete`, `rename`, `read`, `status` |
| Batch | `tx` (atomic transactions), `batch` (line-oriented) |
| Normalize | `tidy` (whitespace, line endings) |
| Safety | `undo` (backup restoration) |
| Agent | `mcp-server`, `agent-rules`, `schema`, `explain` |
| Setup | `init`, `completions` |

## Get started

Head to [Installation](getting-started/installation.md) to install, then follow the [Quickstart](getting-started/quickstart.md) to make your first edit.
