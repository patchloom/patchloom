# Quickstart

This guide takes you from zero to a working multi-file edit in under 5 minutes.

## Prerequisites

- Patchloom installed (see [installation.md](installation.md))
- A git repo to work in (or create a test directory)

## Step 1: Search for something

Find all TODO comments in your project:

```bash
patchloom search 'TODO' src/
```

Count them:

```bash
patchloom search 'TODO' --count src/
```

## Step 2: Replace text across files

Preview a rename (no files changed yet):

```bash
patchloom replace --from 'old_function' --to 'new_function' src/
```

The output shows a unified diff. When it looks correct, apply:

```bash
patchloom replace --from 'old_function' --to 'new_function' src/ --apply
```

## Step 3: Edit structured config

Read a value from a JSON file:

```bash
patchloom doc get package.json version
```

Set a new value:

```bash
patchloom doc set package.json version "2.0.0" --apply
```

## Step 4: Batch a few file edits into one call

When you need several related edits at once, `batch` is the fastest path.
This example assumes `package.json`, `README.md`, and `CHANGELOG.md`
exist, and that `CHANGELOG.md` contains a `## Unreleased` heading:

Preview the grouped edits:

```bash
patchloom batch <<'EOF'
doc.set package.json version "3.0.0"
replace README.md "v1.0.0" "v3.0.0"
md.insert_after_heading CHANGELOG.md "## Unreleased" "- Bumped to v3.0.0"
EOF
```

Apply them once the diff looks right:

```bash
patchloom batch --apply <<'EOF'
doc.set package.json version "3.0.0"
replace README.md "v1.0.0" "v3.0.0"
md.insert_after_heading CHANGELOG.md "## Unreleased" "- Bumped to v3.0.0"
EOF
```

## Step 5: Run an atomic transaction with a saved plan

Use `tx` when the change should live in a reusable plan file, or when you need
format/validate lifecycle steps in the same transaction.

Create a plan file called `bump.json`:

```json
{
  "write_policy": { "ensure_final_newline": true },
  "operations": [
    { "op": "doc.set", "path": "package.json", "key": "version", "value": "2.0.0" },
    { "op": "replace", "path": "README.md", "from": "v1.0.0", "to": "v2.0.0" },
    { "op": "md.insert_after_heading", "path": "CHANGELOG.md", "heading": "## Unreleased", "content": "- Bumped to v2.0.0" }
  ]
}
```

Preview:

```bash
patchloom tx --plan bump.json --diff
```

Apply all changes atomically:

```bash
patchloom tx --plan bump.json --apply
```

If any operation fails, nothing is written.

## Step 6: Use in CI

Check whether a plan would produce changes (exit code 2 = changes pending):

```bash
patchloom tx --plan bump.json --check
echo $?  # 0 = clean, 2 = changes detected
```

Get machine-readable output:

```bash
patchloom --json tx --plan bump.json --apply
```

Returns:

```json
{
  "ok": true,
  "status": "success",
  "files_changed": 3,
  "files_created": 0,
  "files_deleted": 0,
  "changes": [
    { "path": "CHANGELOG.md", "action": "modified" },
    { "path": "README.md", "action": "modified" },
    { "path": "package.json", "action": "modified" }
  ]
}
```

## Next steps

- Browse the [examples/](../../examples/) directory for more tx plan patterns
- See the full [reference guide](../reference/README.md) for command, operation, and notable mode guidance
- Read [concepts.md](concepts.md) for write modes, exit codes, and glob filtering
