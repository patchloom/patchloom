# Patchloom

Agent-grade repo operations in one binary.

## Status

V1 with 8 commands and 172 passing tests.

## Install

Not yet published to crates.io. Build from source:

```
cargo build --release
```

Once published:

```
cargo install patchloom
```

## Commands

| Command   | Description                                          |
|-----------|------------------------------------------------------|
| `search`  | Fast literal or regex search across a repo           |
| `replace` | Mechanical string replacement with diff preview      |
| `patch`   | Preview or apply unified diffs safely                |
| `md`      | Markdown section-aware operations                    |
| `doc`     | Parser-backed JSON, YAML, and TOML operations        |
| `hygiene` | Final newline, line ending, and whitespace normalization |
| `create`  | Create a new file with content                       |
| `tx`      | Execute a multi-operation plan atomically            |

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

Replace text across files (preview diff by default, write with `--apply`):

```
patchloom replace --from 'old_name' --to 'new_name' src/ --apply
```

Read a JSON value:

```
patchloom doc get package.json name
```

Set a YAML key:

```
patchloom doc set config.yaml server.port 8080 --apply
```

Replace a section in a Markdown file:

```
patchloom md replace-section --file AGENTS.md --heading "Rules" --content "New rules here" --apply
```

Create a new file:

```
patchloom create --file AGENTS.md --content "# Project Rules" --apply
```

Delete items from a YAML array by predicate:

```
patchloom doc delete-where config.yml contact_links --predicate 'name=Old Entry' --apply
```

Idempotent replace (succeeds even if text not found):

```
patchloom replace --from 'legacy_name' --to 'new_name' --if-exists --apply
```

Append a row to a markdown table:

```
patchloom md table-append --file README.md --heading "## Features" --row "| new | feature |" --apply
```

Apply a unified diff:

```
patchloom patch apply --file changes.patch --apply
```

Check whether a patch applies cleanly (without writing):

```
patchloom patch check --file changes.patch
```

Fix missing final newlines across a directory:

```
patchloom hygiene fix . --ensure-final-newline --apply
```

Run a multi-operation plan atomically:

```
patchloom tx --plan plan.json --apply
```

## Transaction plan format

The `tx` command accepts a JSON plan with an array of operations:

```json
{
  "operations": [
    { "op": "replace", "path": "src/main.rs", "from": "old", "to": "new" },
    { "op": "doc.set", "path": "config.json", "key": "version", "value": "2.0" },
    { "op": "doc.delete", "path": "config.json", "key": "deprecated" },
    { "op": "doc.merge", "path": "config.json", "key": ".", "value": {"new_key": true} },
    { "op": "doc.append", "path": "config.json", "key": "items", "value": "new_item" },
    { "op": "md.replace_section", "path": "README.md", "heading": "## Notes", "body": "Updated." },
    { "op": "md.insert_after_heading", "path": "README.md", "heading": "## Notes", "body": "Inserted." },
    { "op": "hygiene.fix", "paths": ["src/"] },
    { "op": "file.create", "path": "new.txt", "content": "hello" },
    { "op": "file.delete", "path": "obsolete.txt" }
  ]
}
```

All operations run in order. If any fails, all changes are rolled back (exit code 7). Pass `--apply` to write to disk.

## Symlink behavior

`atomic_write` follows symlinks: it writes to the target of the symlink, not the symlink itself. This is because the write creates a temp file in the parent directory and renames it over the target path, which `rename(2)` resolves through symlinks. If you need to replace a symlink itself, delete and recreate it.

## Global flags

Read-only flags (available on all commands):

| Flag                  | Description                                       |
|-----------------------|---------------------------------------------------|
| `--json`              | Emit machine-readable JSON output                 |
| `--jsonl`             | Emit one JSON object per result line              |
| `--cwd <dir>`         | Set working directory                             |
| `--glob <pattern>`    | Restrict target files by glob pattern             |
| `--files-from <path>` | Read file list from a file or stdin (`-`)         |

Write flags (available on write commands: replace, patch, md, doc, hygiene, create, tx):

| Flag                         | Description                                       |
|------------------------------|---------------------------------------------------|
| `--diff`                     | Print unified diff for any write operation         |
| `--apply`                    | Actually mutate files                              |
| `--check`                    | Compute and report changes without writing         |
| `--atomic`                   | Require all-or-nothing multi-file apply            |
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
