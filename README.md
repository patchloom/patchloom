# Patchloom

Agent-grade repo operations in one binary.

## Status

V1 with 8 commands and 166 passing tests.

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

Search for a pattern across all files:

```
patchloom search 'TODO' src/
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

## Global flags

| Flag                         | Description                                       |
|------------------------------|---------------------------------------------------|
| `--json`                     | Emit machine-readable JSON output                 |
| `--jsonl`                    | Emit one JSON object per result line               |
| `--diff`                     | Print unified diff for any write operation         |
| `--apply`                    | Actually mutate files                              |
| `--check`                    | Compute and report changes without writing         |
| `--cwd <dir>`               | Set working directory                              |
| `--glob <pattern>`          | Restrict target files by glob pattern              |
| `--files-from <path>`        | Read file list from a file or stdin (`-`)          |
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
