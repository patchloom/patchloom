# Patchloom 0.4.0

This is a major release focused on making Patchloom an excellent, safe, and ergonomic library for Rust applications and AI coding agents, while continuing to polish the CLI and MCP surfaces.

## Highlights

**PathGuard for safe library embedding**

The biggest addition in 0.4.0 is first-class support for `PathGuard` (and the flexible `AbsolutePathPolicy` builder) across the entire public library API.

- All high-level functions in `patchloom::api` (`replace_text`, `doc_*`, `md_*`, `file_*`, `tidy`, `execute_plan`, etc.) now accept an optional `guard: Option<&PathGuard>` as the final parameter.
- `execute_plan` performs upfront validation using the centralized `declared_paths` helper.
- The builder makes common relaxed policies easy: `PathGuard::builder(root).allow_temp_directory().build()`.
- Strict `Reject` policy, cross-file operation safety (rename, md.move_section, etc.), and symlink-aware checks are all enforced at the right time.

This was driven by real downstream usage (e.g. bline) and closes a long tail of PathGuard-related tech debt (#748–#750, #755–#759, #762).

**Library modularity and semver safety**

- The `cli` feature is now optional. Use `default-features = false` + `features = ["mcp", "ast"]` (or any subset) for a dramatically smaller dependency tree when embedding Patchloom.
- Many public types were moved to more reusable locations (`ops`, `write`, etc.) and re-exported.
- All public structs and enums are now `#[non_exhaustive]` for future-proofing.

## Other notable improvements

- Shared `declared_paths` helper eliminates duplicated path collection logic between `cmd/tx` upfront guard checks and MCP validation (#762).
- Extensive new tests for guard behavior (relaxed policies, destination rejection, cross-file operations, cfg combinations).
- Hygiene wins: removal of now-dead `TxState.guard` code, direct unit coverage for the path declaration helper, stronger test assertions in several areas.
- Numerous fixes for atomicity, confirm flows, Windows path handling, and edge cases from the recent improvement cycles.
- Better documentation and examples for the library API surface.

## Migration notes

If you were already using the library API, the new `guard` parameter is trailing and optional. Update call sites:

```rust
// Before
api::replace_text(path, old, new, &opts, mode)?;

// After (no guard)
api::replace_text(path, old, new, &opts, mode, None)?;

// With guard
let guard = PathGuard::builder(cwd).allow_temp_directory().build()?;
api::replace_text(path, old, new, &opts, mode, Some(&guard))?;
```

See the `patchloom::api` module documentation for full details and examples.

## Thank you

Thanks to everyone who filed issues, reviewed, and used the library in real agent tooling. This release makes the "library first" story much stronger.

Full changelog and commit history are in the release PR.
