# Patchloom v0.2.0

Patchloom is now a library. This release exposes the core editing engine as a public Rust API, hardens transaction safety, and adds three-way patch merging.

## Library API

Patchloom's structured editing engine is now available as a Rust library. Add it as a dependency with no async overhead:

```toml
[dependencies]
patchloom = { version = "0.2", default-features = false }
```

The `api` module exposes doc, replace, markdown, file, and patch operations. All types are `Send + Sync`. Four utility modules are also public:

- **`containment`** -- workspace path guard that prevents directory traversal attacks. Two-layer defense: syntactic depth check plus symlink-aware canonicalization.
- **`exec`** -- shell command execution with timeout and process-tree management.
- **`files`** -- file-walking, SIMD-accelerated binary detection, and text reading helpers.
- **`write`** -- atomic file writes via tempfile with write-policy transformations.

All public structs and enums are marked `#[non_exhaustive]` for forward-compatible evolution. Cargo-semver-checks runs in CI on every release PR.

## Three-way patch merge

New `patch merge` subcommand handles stale diffs gracefully:

```bash
# Check if a patch applies cleanly, merges cleanly, or has conflicts
patchloom patch merge stale.patch --check

# Apply with three-way merge
patchloom patch merge stale.patch --apply

# Allow conflict markers in output
patchloom patch merge stale.patch --apply --allow-conflicts
```

Also available in tx plans via `on_stale: "merge"` and `allow_conflicts: true`, and in the MCP server's `apply_patch` tool.

Conflicts produce familiar `<<<<<<< patchloom (ours)` / `>>>>>>> patch (theirs)` markers. Exit code 8 (`CONFLICTS`) signals unresolved conflicts.

## Transaction rollback hardening

- **`strict` now defaults to `true`.** Format or validation failures roll back all writes automatically. Override with `"strict": false` in the plan, `[tx] strict = false` in `.patchloom.toml`, or `--no-strict` on the CLI.
- **Mid-commit recovery.** If a write fails partway through, patchloom restores already-written files from the backup session. Exit 7 for clean rollback, exit 1 if restore is incomplete.
- **Clearer exit semantics.** Staging failures exit 4 (`operation_failed`), distinct from parse errors.

## Other improvements

- **Config validation.** Invalid values in `.patchloom.toml` (e.g., `normalize_eol = "unix"`) now emit stderr warnings instead of silently ignoring.
- **Batch quoting docs.** Expanded guidance for JSON string values in batch format, with Unix and Windows examples.
- **Schema fix.** `md.move_section` examples in `patchloom schema` output now include the required `op` field.
- **Test coverage.** 1,476 tests (807 unit + 669 integration), up from 1,300 in v0.1.7.

## Breaking changes

All public structs and enums are now `#[non_exhaustive]`. Code that constructs these types via struct literals must add `..Default::default()`. Serde deserialization (the primary construction path for plans and MCP params) is unaffected.

## Links

- [Full changelog](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.7...patchloom-v0.2.0)
- [Documentation](https://patchloom.github.io/patchloom/)
- [Library API docs](https://docs.rs/patchloom)
- [MCP setup guide](https://patchloom.github.io/patchloom/getting-started/mcp-setup.html)
