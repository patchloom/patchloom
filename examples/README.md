# Examples

Sample transaction plans demonstrating Patchloom's capabilities.

These plans are **illustrative templates**, not guaranteed to run unchanged in
this repo. Read the target paths in each file first, then adapt them to your
project before applying.

```
patchloom tx --plan examples/01-basic-replace.json --diff     # preview
patchloom tx --plan examples/01-basic-replace.json --apply    # apply after adapting paths
patchloom --json tx --plan examples/01-basic-replace.json --apply  # JSON output
```

## Plans

| File | Scenario | Operations used |
|------|----------|-----------------|
| [01-basic-replace.json](01-basic-replace.json) | Single-file text replacement | `replace` |
| [02-multi-file-batch.json](02-multi-file-batch.json) | Atomic version bump across multiple files with format and validate steps | `replace`, `doc.set`, `format`, `validate` |
| [03-markdown-editing.json](03-markdown-editing.json) | Update changelog, add rules, append table rows, deduplicate headings | `md.replace_section`, `md.upsert_bullet`, `md.table_append`, `md.dedupe_headings` |
| [04-doc-mutations.json](04-doc-mutations.json) | Structured config changes: set, ensure, merge, append, delete, delete-where | `doc.set`, `doc.ensure`, `doc.merge`, `doc.append`, `doc.delete`, `doc.delete_where` |
| [05-strict-mode.json](05-strict-mode.json) | Create a module, wire it, update changelog; rolls back everything if build or tests fail | `file.create`, `replace`, `md.insert_after_heading`, `strict`, `format`, `validate` |

## Write modes

All plans support three modes via CLI flags:

- `--diff` (default) -- print a unified diff of what would change
- `--check` -- exit 0 if no changes, exit 2 if changes detected (for CI)
- `--apply` -- write changes to disk

## Strict mode

Plans with `"strict": true` revert all writes if any `format` or `validate` step fails (exit code 7). Without strict mode, a format failure exits with code 6 and reports `format_failed` in JSON output, while a validation failure exits with code 6 and reports `validation_failed`.
