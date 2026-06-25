# Quickstart

This guide takes you from zero to a working multi-file edit in under 5 minutes.

## Prerequisites

- Patchloom installed (see [installation.md](installation.md))
- A git repo to work in, or a test directory you can initialize with `git init` before Step 6

## Step 0: Set up your project (optional)

Run `init` to generate agent rules, get shell completions, and detect MCP setup:

```bash
patchloom init
```

This creates `AGENTS.md` in a new project or appends the rules to an existing agent instructions file so AI agents know how to use patchloom. Pass `-y` to skip confirmation prompts. If `.vscode/` or `.cursor/` already exists, `init` also prints ready-to-copy `.vscode/mcp.json` or `.cursor/mcp.json` snippets.

## Step 1: Search for something

Find all TODO comments in your project:

```bash
patchloom search 'TODO' src/
```

Count them:

```bash
patchloom search 'TODO' --count src/
```

Limit a search to a nested subtree with `--glob`:

```bash
patchloom search 'TODO' src/ --glob 'sub/*.rs'
```

## Step 2: Replace text across files

Preview a rename (no files changed yet):

```bash
patchloom replace 'old_function' --to 'new_function' src/
```

The output shows a unified diff. When it looks correct, apply:

```bash
patchloom replace 'old_function' --to 'new_function' src/ --apply
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
  "version": "1",
  "write_policy": { "ensure_final_newline": true },
  "operations": [
    { "op": "doc.set", "path": "package.json", "selector": "version", "value": "2.0.0" },
    { "op": "replace", "path": "README.md", "from": "v1.0.0", "to": "v2.0.0" },
    { "op": "md.insert_after_heading", "path": "CHANGELOG.md", "heading": "## Unreleased", "content": "- Bumped to v2.0.0" }
  ]
}
```

Preview:

```bash
patchloom tx bump.json --diff
```

Apply all changes atomically:

```bash
patchloom tx bump.json --apply
```

If an operation fails, nothing is written. Format and validate lifecycle steps run
after writes, so use `"strict": true` in the plan if you want those failures to
roll back all changes too. Lifecycle failure output includes the failing step
number, exit status, and the `cwd` used for that step.

## Step 6: Explore code structure with AST

List all functions and types in a directory:

```bash
patchloom ast list src/
```

Filter by symbol kind:

```bash
patchloom ast list src/ --kind function,struct
```

Read a specific symbol's source code:

```bash
patchloom ast read src/main.rs run
```

Find all references to a symbol across files:

```bash
patchloom ast refs process_data src/
```

Validate syntax of source files:

```bash
patchloom ast validate src/
```

Generate a ranked repository map (PageRank over the symbol graph):

```bash
patchloom ast map src/ --max-tokens 2048
```

AST commands support 20 languages including Rust, Python, TypeScript, JavaScript,
Go, Java, C#, C/C++, Ruby, PHP, Swift, Kotlin, HCL, and more.

## Step 7: Inspect and undo changes

After any `--apply`, you can ask patchloom what changed and restore the latest backup session.

`patchloom status` is git-backed. If you're using a scratch directory, run `git init`, add the files you want tracked, and make an initial commit before this step.

See pending working-tree changes:

```bash
patchloom status
```

Preview what `undo` would restore (exit code `2` means files would be restored):

```bash
patchloom undo
```

Restore the most recent backup session:

```bash
patchloom undo --apply
```

## Step 8: Use in CI

Check whether a plan would produce changes (exit code 2 = changes pending):

```bash
patchloom tx bump.json --check
echo $?  # 0 = clean, 2 = changes detected
```

Get machine-readable output:

```bash
patchloom --json tx bump.json --apply
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

## Troubleshooting

### Config file not loading

Patchloom searches for `.patchloom.toml` starting from the working directory
and walking up to the filesystem root. If your config does not seem to take
effect:

1. **Verify the file location.** Run from the directory containing
   `.patchloom.toml` or a subdirectory beneath it.

2. **Check for TOML syntax errors.** Patchloom prints a warning to stderr
   when it finds a `.patchloom.toml` that cannot be parsed:

   ```
   warning: malformed /path/to/.patchloom.toml: expected `=`, found ...
   ```

   Validate your file with:

   ```bash
   patchloom doc get .patchloom.toml write_policy
   ```

   If this errors, fix the TOML syntax.

3. **CLI flags override config.** Flags like `--ensure-final-newline` and
   `--normalize-eol` always take precedence over `.patchloom.toml` values.

### Backups filling up disk

Backup sessions are stored under `.patchloom/backups/` and are automatically
pruned after 7 days. If you need to free space immediately:

```bash
rm -rf .patchloom/backups/
```

This is safe; the next `--apply` run will create a fresh backup directory.

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General failure |
| 2 | Changes detected (used by `--check`) |
| 3 | No matches found |
| 4 | Parse error |
| 5 | Ambiguous match |
| 6 | Validation failed |
| 7 | Rollback (transaction failed and was rolled back) |
| 8 | Patch merge conflicts detected |

## Next steps

- Browse the [examples](https://github.com/patchloom/patchloom/tree/main/examples) directory for more tx plan patterns
- See the full [reference guide](../reference/README.md) for command, operation, and notable mode guidance
- Read [concepts.md](concepts.md) for write modes, exit codes, and glob filtering
