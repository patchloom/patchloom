# Examples

Sample transaction plans and batch scripts demonstrating Patchloom's capabilities.

These examples are **illustrative templates**, not guaranteed to run unchanged in
this repo. Read the target paths and any embedded `format` or `validate`
commands in each file first, then adapt them to your project before applying.

```
# Transaction plans (JSON)
patchloom tx examples/01-basic-replace.json --diff     # preview
patchloom tx examples/01-basic-replace.json --apply    # apply after adapting paths
patchloom --json tx examples/01-basic-replace.json --apply  # JSON output

# Batch format (line-oriented)
patchloom batch examples/06-batch-version-bump.txt --diff   # preview
patchloom batch examples/06-batch-version-bump.txt --apply  # apply
```

## Plans

| File | Scenario | Operations used |
|------|----------|-----------------|
| [01-basic-replace.json](01-basic-replace.json) | Single-file text replacement | `replace` |
| [02-multi-file-batch.json](02-multi-file-batch.json) | Atomic version bump across multiple files with format and validate steps | `replace`, `doc.set`, `format`, `validate` |
| [03-markdown-editing.json](03-markdown-editing.json) | Update changelog, add rules, append table rows, deduplicate headings | `md.replace_section`, `md.upsert_bullet`, `md.table_append`, `md.dedupe_headings` |
| [04-doc-mutations.json](04-doc-mutations.json) | Structured config changes: set, ensure, merge, append, delete, delete-where | `doc.set`, `doc.ensure`, `doc.merge`, `doc.append`, `doc.delete`, `doc.delete_where` |
| [05-strict-mode.json](05-strict-mode.json) | Create a module, wire it, update changelog; rolls back everything if build or tests fail | `file.create`, `replace`, `md.insert_after_heading`, `strict`, `format`, `validate` |
| [06-batch-version-bump.txt](06-batch-version-bump.txt) | Version bump across JSON, YAML, and markdown using the batch line format | `doc.set`, `replace`, `md.upsert_bullet` |
| [07-yaml-plan.yaml](07-yaml-plan.yaml) | Same semantics as JSON plans but in YAML for readability; config bump with format and validate | `doc.set`, `doc.ensure`, `md.upsert_bullet`, `replace`, `format`, `validate` |
| [08-mcp-tool-call.json](08-mcp-tool-call.json) | Reference showing MCP tool call arguments for common operations (not a tx plan) | `doc_set`, `search_files`, `move_file`, `md_table_append` |
| [09-patch-apply.json](09-patch-apply.json) | Apply a unified diff then follow up with a glob replace to catch remaining references | `patch.apply`, `replace` |
| [10-inspect-and-edit.json](10-inspect-and-edit.json) | Read a config, search for references, then update both config and source code in one tx | `read`, `search`, `doc.set`, `replace` |

## Write modes

All plans support three modes via CLI flags:

- `--diff` (default) -- print a unified diff of what would change
- `--check` -- exit 0 if no changes, exit 2 if changes detected (for CI)
- `--apply` -- write changes to disk

## Strict mode

Plans with `"strict": true` revert all writes if any `format` or `validate` step fails (exit code 7). In strict mode, the JSON `error` string uses the legacy `rollback` prefix because the transaction was reverted, but the additive `error_kind` field still preserves the underlying `format_failed` or `validation_failed` cause. Without strict mode, the JSON `error` string keeps the legacy `validation_failed` prefix for both failure types; use the additive `error_kind` field to tell `format_failed` from `validation_failed`.
