# Core Concepts

## Commands

Patchloom has 23 commands:

- **search** / **replace** -- text-level find and replace across files
- **patch** -- apply unified diffs
- **md** -- markdown-aware editing (sections, bullets, tables, headings)
- **doc** -- parser-backed JSON, YAML, and TOML mutations
- **tidy** -- whitespace and line-ending normalization
- **append** / **prepend** -- append or prepend content to an existing file
- **create** / **delete** / **rename** -- file lifecycle
- **read** -- file content inspection with optional line range (supports multiple files)
- **status** -- uncommitted change summary from git
- **tx** -- atomic multi-operation transactions
- **batch** -- line-oriented multi-operation format (delegates to tx engine)
- **ast** -- AST-aware operations (list, read, rename, validate) across 20 languages
- **completions** -- shell completion generation
- **agent-rules** -- print end-user agent documentation for patchloom
- **schema** -- export operation schemas with tier filtering and system prompts
- **explain** -- summarize a tx plan in plain English before applying
- **undo** -- restore files from a backup created by `--apply`
- **init** -- set up patchloom in a project (agent rules, completions, MCP)
- **mcp-server** -- MCP protocol server exposing patchloom tools for AI agents

For feature-by-feature `Use when` guidance on commands, operations, and notable modes, see the [reference guide](../reference/README.md).

## Write modes

Every write command supports four modes:

| Flag | Behavior | Use case |
|------|----------|----------|
| `--diff` (default) | Print a unified diff of what would change | Preview before applying |
| `--check` | Exit 0 if clean, exit 2 if changes detected | CI pipelines, dry-run validation |
| `--apply` | Write changes to disk | Actual mutation |
| `--confirm` | Show the diff, then prompt before writing | Interactive preview-then-apply |

These modes are mutually exclusive. Patchloom is safe by default: nothing is written unless you pass `--apply` or confirm an interactive prompt.

## Write safety on disk

Apply writers share a single `atomic_write` path:

- **Normal files** (`nlink == 1`): write a same-directory temp file, then rename over the target so readers never see a half-written file.
- **Symlinks** (#1230): resolve and write the target; the symlink directory entry is not replaced by a regular file.
- **Hardlinks** (`nlink > 1` on Unix, #1733): stage full content on a same-dir temp, then rewrite the **existing** inode in place so every hardlink path stays in sync. Temp+rename would break siblings (they would keep the old inode). Windows and other platforms without this check keep rename semantics.
- **New files**: create via temp + exclusive persist (no pre-existing inode to preserve).

Library embedders, CLI `--apply`, MCP tools, and `tx`/`batch` all use this path.

## Write policy

A write policy controls transformations applied to all content before it reaches disk:

- `--ensure-final-newline` -- non-empty files always end with `\n`
- `--normalize-eol <lf|crlf|cr>` -- standardize line endings
- `--trim-trailing-whitespace` -- remove trailing spaces on every line
- `--respect-editorconfig` -- read policy from `.editorconfig` if present

Standalone write commands use these flags directly. In `tx`, the same flags act as defaults for all writes, and plan-level `write_policy` entries override conflicting CLI flags for self-contained plans.

In tx plans, set these at the plan level:

```json
{
  "version": 1,
  "write_policy": { "ensure_final_newline": true },
  "operations": [...]
}
```

## Project configuration

Create a `.patchloom.toml` in your project root to set per-project defaults. CLI flags override config values.

```toml
[write_policy]
ensure_final_newline = true
normalize_eol = "lf"
trim_trailing_whitespace = true
collapse_blanks = true

[tx]
strict = false

[exclude]
globs = ["target/**", "node_modules/**"]

[output]
color = "auto"
```

The config file is searched from the working directory upward, so it works in subdirectories too.

## Undo safety net

Before any `--apply` write, patchloom saves the original content of each affected file to `.patchloom/backups/`. If something goes wrong:

```bash
patchloom undo --list          # see available backups
patchloom undo                 # dry-run: show what would change
patchloom undo --apply         # actually restore files
```

Backups older than 7 days are auto-pruned.

## Color output

Patchloom colorizes diffs and search results when stdout is a terminal. Override with:

- `--color=always` -- force color (useful when piping to a pager like `less -R`)
- `--color=never` -- disable color
- `NO_COLOR=1` -- environment variable that disables color for all tools ([no-color.org](https://no-color.org))

Machine-readable modes (`--json`, `--jsonl`, `--quiet`) never produce color.

## Transaction plans

The `tx` command runs multiple operations atomically. If staging fails because a target was not found (missing symbol, heading, or replace pattern), no files are written and the plan exits 3 (`no_matches`) with the concrete detail in the error message. Other staging failures exit 9 (`operation_failed`). If a write fails mid-commit, patchloom restores already-written files from the backup session (exit 7, `rollback`).

Plans are JSON objects with three lifecycle arrays:

1. **operations** -- the mutations (replace, doc.set, md.replace_section, `patch.apply`, etc.)
2. **format** -- shell commands that run after writes (e.g., `cargo fmt`)
3. **validate** -- shell commands that verify correctness (e.g., `make check`)

`patch.apply` operations accept `on_stale: "merge"` for three-way merge when the on-disk file diverged from the patch base, and `allow_conflicts: true` to write conflict markers instead of failing.

With `--json` / `--jsonl`, plan results include file-level `changes` plus, for `doc.delete` / `doc.delete_where`, a `mutations` array and aggregate `changed` / `removed` counts (including `removed: 0` for idempotent no-ops). The same fields appear on MCP write tools and `execute_plan`.

Strict mode defaults to on. Use `"strict": false` in the plan, `[tx] strict = false` in `.patchloom.toml`, or `patchloom tx --no-strict` to keep writes on disk when format/validate fails (exit 6). With strict mode, a format or validation failure reverts all writes (exit 7). If a write fails mid-commit, patchloom restores already-written files from the backup session (exit 7 `rollback`, or exit 1 `rollback_failed` if restore is incomplete).

## Exit codes

Every command returns a specific exit code:

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error (including CLI usage: invalid flags, enum values, missing args, unknown subcommands), or tx `rollback_failed` when mid-commit rollback could not fully restore files |
| 2 | Changes detected (with `--check` or write preview; not used for CLI usage errors) |
| 3 | No matches found |
| 4 | Parse error in input |
| 5 | Ambiguous (multiple replace matches, or stale patch context) |
| 6 | Validation failed (writes may remain) |
| 7 | Rollback (strict mode, no writes remain) |
| 8 | Patch merge conflicts detected (apply blocked unless `--allow-conflicts`) |
| 9 | Tx operation staging failure (`operation_failed`) |

These codes let CI pipelines and agent frameworks branch on outcomes without parsing output.

When `--json` or `--jsonl` is set, CLI usage failures (invalid flags, enum values, missing required args, unknown subcommands) emit a JSON envelope on stdout with `error_kind: "invalid_input"` and exit 1. Without those flags, clap prints human usage text on stderr. Path rejections under `--contain` (and empty path arguments) also set `error_kind: "invalid_input"` on the structured error path.

When every explicit path root for `search`, `replace`, or `tidy` is missing (including a non-stdin `--files-from` list), exit 1 with `error_kind: "not_found"`. Pattern misses on existing files still use exit 3 (`no_matches`). Empty existing directories remain clean success for tidy.

## Glob filtering

Most commands accept `--glob <pattern>` (repeatable) to restrict which files are processed:

```bash
patchloom replace "old" --new "new" --glob "*.rs" --glob "*.toml" --apply
```

Glob patterns match either the basename or the path relative to the input root. For example, if you search `src/`, then `--glob 'sub/*.txt'` matches `src/sub/file.txt`.

In tx plans, individual operations can use `"glob"` instead of `"path"` to target multiple files.

## Security model

Patchloom runs with the privileges of the invoking user and treats all inputs (command-line arguments, plan files, stdin) as trusted. This is the same trust model as `make`, `sh`, or `cargo`.

What this means in practice:

- **Plans can execute arbitrary shell commands.** The `format` and `validate` lifecycle steps pass their `cmd` field to `sh -c` (or `cmd /C` on Windows) with the user's full privileges. Only load plans you trust.
- **CLI file operations are unrestricted by default.** `create`, `delete`, `read`, `search`, `replace`, `patch`, `rename`, `ast`, `md`, `doc`, `tidy`, `status`, and all `tx` operations accept any path the invoking user can access (including `../` escapes from `--cwd`). `--cwd` only sets the default base for relative paths; it is **not** a containment boundary unless you also pass **`--contain`**, which enables PathGuard for CLI reads, writes, and meta-input files (`tx`/`explain` plans, `batch` ops files, `patch` files, and `--files-from` lists). Under `--contain`, absolute paths that resolve **inside** the workspace are allowed (`AllowIfContained`); `../` escapes and absolute paths outside the workspace are rejected. MCP is stricter: it rejects absolute path *strings* even when they would resolve under the server root.
- **MCP and the library PathGuard are sandboxed.** The MCP server and embedders that pass a `PathGuard` reject paths that escape the workspace root (via `../` or symlinks). Prefer MCP tool calls when an agent must stay inside a workspace, or use `patchloom --cwd <ws> --contain …` for CLI agents.
- **Plan `cwd` overrides the working directory.** A plan's `cwd` field changes the working directory for all subsequent operations and lifecycle steps. Relative values resolve from the invocation root, not from the plan file location. In normal CLI use this still runs with the invoking user's filesystem access. In MCP mode, `cwd` must be a **relative** path under the server workspace (honored for re-rooting; absolute path strings and `../` escapes are rejected). Do not combine `cwd` with `for_each` on MCP.

**For AI agent authors:** Prefer the MCP server for agent-driven edits so path containment is always on. If the agent shells out to the CLI instead, pass `--cwd <workspace> --contain` on every write. Do not construct plans from untrusted conversational input without validation. A plan is equivalent to a shell script. Treat plan files with the same care you would treat a Makefile or a bash script from an unknown source.
