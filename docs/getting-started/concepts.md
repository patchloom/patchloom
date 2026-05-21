# Core Concepts

## Commands

Patchloom has 15 core commands. Building with `--features mcp` adds a 16th, `mcp-server`:

- **search** / **replace** -- text-level find and replace across files
- **patch** -- apply unified diffs
- **md** -- markdown-aware editing (sections, bullets, tables, headings)
- **doc** -- parser-backed JSON, YAML, and TOML mutations
- **hygiene** -- whitespace and line-ending normalization
- **create** / **delete** / **rename** -- file lifecycle
- **read** -- file content inspection with optional line range (supports multiple files)
- **status** -- uncommitted change summary from git
- **tx** -- atomic multi-operation transactions
- **batch** -- line-oriented multi-operation format (delegates to tx engine)
- **completions** -- shell completion generation
- **agent-rules** -- print end-user agent documentation for patchloom
- **mcp-server** -- MCP protocol server exposing patchloom tools for AI agents (requires `--features mcp`)

For feature-by-feature `Use when` guidance on commands, operations, and notable modes, see the [reference guide](../reference/README.md).

## Write modes

Every write command supports three modes:

| Flag | Behavior | Use case |
|------|----------|----------|
| `--diff` (default) | Print a unified diff of what would change | Preview before applying |
| `--check` | Exit 0 if clean, exit 2 if changes detected | CI pipelines, dry-run validation |
| `--apply` | Write changes to disk | Actual mutation |

These modes are mutually exclusive. Patchloom is safe by default: nothing is written unless you pass `--apply`.

## Write policy

A write policy controls transformations applied to all content before it reaches disk:

- `--ensure-final-newline` -- non-empty files always end with `\n`
- `--normalize-eol <lf|crlf>` -- standardize line endings
- `--trim-trailing-whitespace` -- remove trailing spaces on every line
- `--respect-editorconfig` -- read policy from `.editorconfig` if present

Standalone write commands use these flags directly. In `tx`, the same flags act as defaults for all writes, and plan-level `write_policy` entries override conflicting CLI flags for self-contained plans.

In tx plans, set these at the plan level:

```json
{
  "write_policy": { "ensure_final_newline": true },
  "operations": [...]
}
```

## Transaction plans

The `tx` command runs multiple operations atomically. If any operation fails, all changes are rolled back and no files are written.

Plans are JSON objects with three lifecycle arrays:

1. **operations** -- the mutations (replace, doc.set, md.replace_section, etc.)
2. **format** -- shell commands that run after writes (e.g., `cargo fmt`)
3. **validate** -- shell commands that verify correctness (e.g., `make check`)

With `"strict": true`, a format or validation failure reverts all writes (exit 7). Without strict mode, writes stay on disk (exit 6).

## Exit codes

Every command returns a specific exit code:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Changes detected (with `--check`) |
| 3 | No matches found |
| 4 | Parse error in input |
| 5 | Ambiguous or stale patch context |
| 6 | Validation failed (writes may remain) |
| 7 | Rollback (strict mode, no writes remain) |

These codes let CI pipelines and agent frameworks branch on outcomes without parsing output.

## Glob filtering

Most commands accept `--glob <pattern>` (repeatable) to restrict which files are processed:

```bash
patchloom replace --from "old" --to "new" --glob "*.rs" --glob "*.toml" --apply
```

Glob patterns match either the basename or the path relative to the input root. For example, if you search `src/`, then `--glob 'sub/*.txt'` matches `src/sub/file.txt`.

In tx plans, individual operations can use `"glob"` instead of `"path"` to target multiple files.

## Security model

Patchloom runs with the privileges of the invoking user and treats all inputs (command-line arguments, plan files, stdin) as trusted. This is the same trust model as `make`, `sh`, or `cargo`.

What this means in practice:

- **Plans can execute arbitrary shell commands.** The `format` and `validate` lifecycle steps pass their `cmd` field to `sh -c` (or `cmd /C` on Windows) with the user's full privileges. Only load plans you trust.
- **File operations are unrestricted.** `create`, `delete`, `read`, `replace`, `patch`, and all `tx` operations accept any path the invoking user can access. There is no sandbox, chroot, or path restriction.
- **Plan `cwd` overrides the working directory.** A plan's `cwd` field changes the process working directory for all subsequent operations and lifecycle steps. This is intentional for self-contained plans, but means a malicious plan can resolve relative paths from any directory.

**For AI agent authors:** Do not construct plans from untrusted conversational input without validation. A plan is equivalent to a shell script. Treat plan files with the same care you would treat a Makefile or a bash script from an unknown source.
