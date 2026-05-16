# Patchloom

[![CI](https://github.com/patchloom/patchloom/actions/workflows/ci.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/ci.yml)
[![Security](https://github.com/patchloom/patchloom/actions/workflows/security.yml/badge.svg)](https://github.com/patchloom/patchloom/actions/workflows/security.yml)

Agent-grade repo operations in one binary.

## Status

V2 with 12 commands and 399 passing tests.

## Install

Not yet published to crates.io. Install from source:

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .
```

Once published:

```
cargo install patchloom
```

## Getting Started

- Start with [`docs/getting-started/installation.md`](./docs/getting-started/installation.md)
  for install options and shell completions.
- Read [`docs/getting-started/concepts.md`](./docs/getting-started/concepts.md)
  for write modes, transaction plans, and exit codes.
- Follow [`docs/getting-started/quickstart.md`](./docs/getting-started/quickstart.md)
  for a 5-minute walkthrough.
- Browse [`docs/reference/README.md`](./docs/reference/README.md)
  for feature-by-feature guidance on commands, operations, and notable modes.
- Browse [`examples/README.md`](./examples/README.md) for transaction plan templates.

## Commands

| Command | Description |
|---|---|
| `search` | Fast literal or regex search across a repo |
| `replace` | Mechanical string replacement with diff preview |
| `patch` | Preview or apply unified diffs safely |
| `md` | Markdown section-aware operations |
| `doc` | Parser-backed JSON, YAML, and TOML operations |
| `hygiene` | Final newline, line ending, and whitespace normalization |
| `create` | Create a new file with content |
| `delete` | Delete a file |
| `read` | Read file contents with optional line range |
| `status` | Show which files have uncommitted changes |
| `tx` | Execute a multi-operation plan atomically |
| `completions` | Generate shell completions (bash, zsh, fish, elvish) |

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

Check if a key exists (exit 0 = yes, exit 3 = no):

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
    { "op": "patch.apply", "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-old\n+new" }
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

## Security

For current security reporting guidance, see [SECURITY.md](./SECURITY.md).

GitHub private vulnerability reporting will be enabled after the repository becomes public.
