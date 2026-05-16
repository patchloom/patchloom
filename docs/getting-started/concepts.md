# Core Concepts

## Commands

Patchloom has 10 commands, each targeting a different kind of repo operation:

- **search** / **replace** -- text-level find and replace across files
- **patch** -- apply unified diffs
- **md** -- markdown-aware editing (sections, bullets, tables, headings)
- **doc** -- parser-backed JSON, YAML, and TOML mutations
- **hygiene** -- whitespace and line-ending normalization
- **create** / **delete** -- file lifecycle
- **tx** -- atomic multi-operation transactions
- **completions** -- shell completion generation

For feature-by-feature `Use when` guidance on commands, operations, and notable modes, see the [reference guide](../reference/README.md).

## Write modes

Every write command supports three modes:

| Flag | Behavior | Use case |
|------|----------|----------|
| `--diff` (default) | Print a unified diff of what would change | Preview before applying |
| `--check` | Exit 0 if clean, exit 2 if changes detected | CI pipelines, dry-run validation |
| `--apply` | Write changes to disk | Actual mutation |

This means patchloom is safe by default. Nothing is written unless you pass `--apply`.

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

In tx plans, individual operations can use `"glob"` instead of `"path"` to target multiple files.
