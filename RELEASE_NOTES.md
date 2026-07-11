# Patchloom 0.12.0

Agent-host and embedder release: fail-closed library edits, shell-aware replace, richer AST writers, and machine-readable errors across CLI, plans, MCP, and the Rust API. Built for Bline-class hosts and agents that branch on `error_kind` instead of scraping English. 151 commits since 0.11.0, with 201 new tests.

## Highlights

Library embedders can require a real match (`require_change`), rewrite shell command tokens without touching package names (`command_position`), rename and batch-rename symbols on disk, and restore a single path from the latest backup session. Agents get a single typed error table: missing files are `not_found`, missing symbols and selectors are `no_matches`, bad flags and regexes are `invalid_input`, post-write formatter failures are `format_failed`, and structured `--json` never returns empty stdout on serialize failure. Multi-file AST rename and reverse-deps scans are parallel. Tx verify attribute filters no longer steal a neighbor's `#[test]`.

## New features

- **Fail-closed content replace.** `ReplaceOptions.require_change` (default false). Zero matches become structured `EditErrorKind::NoMatch` (with similar-target suggestions when available) instead of soft `Ok(changed=false)`. `if_exists` still wins when both are set. Re-exports `EditError`, `EditErrorKind`, and `edit_error_kind` for kind matching without scraping English. CLI `--require-change` and plan/MCP fields match. (#1492, #1497)
- **Shell command-position replace.** Opt-in `ReplaceOptions.command_position` (and CLI `--command-position`) rewrites invocable shell tokens without treating `uv pip` or `pipenv` as bare `pip`. Peels common wrappers: `sudo` / `doas` / `env` / `timeout` / `nice` / `stdbuf` / `ionice` / `setsid` / `runuser` / `busybox` / `flock` / `chroot` / `xargs` / `watch` / `strace` / `eval` / `source`, plus process supervisors such as runit / daemontools / s6. Incompatible flag combinations are `invalid_input`. (#1494, #1497, #1510-#1519, #1527-#1533, #1545-#1547, #1557, #1646)
- **AST file mutators for embedders.** `api::ast_rename`, `api::ast_replace_in_symbol`, `api::ast_rewrite_signature` / `ast_rewrite_signature_in_content`, plus `FunctionSigEdit::parse_rust` for structured or full-string signatures. Multi-file `api::ast_rename_batch` serializes same-path work and returns per-file results (`continue_on_no_match`, `fail_fast`). Zero symbol matches → `NoMatch`. (#1493, #1495, #1497)
- **Validate/revert helper.** `backup::restore_path_from_latest_backup(project_root, path)` restores from the newest session that contains that exact path (exact match only; no basename shortcuts). (#1494, #1497)
- **Plan `ops` alias.** Transaction plans accept `"ops"` as a co-equal alias for `"operations"` so agents that emit either shape deserialize cleanly. (#1578, #1579)
- **Multi-op content edits use real paths in diffs.** Content-edit results put the real file path in unified-diff headers (not a buffer placeholder). Signature rewrite preserves the space or newline before `{` when agents omit trailing whitespace on a full `new_signature`. (#1502, #1504)

## Agent and scripting reliability

- **One typed error table for hosts.** CLI, tx, MCP plan execute, and library `edit_error_kind` share `classify_typed_error` so kinds cannot drift between surfaces. Agents can branch on `error_kind` for exit 1 / 3 / 4 / 8 / 9 without scraping messages. (#1620, #1635-#1641)
- **Missing paths are `not_found`.** Search, replace, tidy, `--files-from` when every target is missing, file append/prepend/delete, and engine IO map to `error_kind: not_found` with exit behavior agents can trust. (#1580-#1586)
- **No-match and count mismatches stay distinct.** Plan/CLI no-matches stay exit 3 with `no_matches`. Search `assert_count` mismatches and related "would change / count wrong" paths use `changes_detected` (exit 2) instead of looking like success or a soft empty result. Tx preview JSON reports `changes_detected` when the plan would write. (#1598, #1648, #1649)
- **Invalid input is typed.** Empty patterns, bad regexes, clap usage under `--json`, `--contain` escapes, bad `--cwd`, non-file targets, doc/selector mistakes, and many plan flag errors set `invalid_input` (exit 1) instead of bare failure or empty success. Clap usage exits 1 (not 2) so it does not look like "changes detected." (#1574-#1577, #1594-#1609, #1621-#1625)
- **Post-write `--format` failures are `format_failed`.** Explicit formatter or format-timeout failures set `error_kind: format_failed` (exit 1) after the write may already have committed; use `undo` or re-run the formatter. Covers standalone writes, doc, and tx. (#1626-#1634)
- **Structured JSON never goes silent.** If serializing a structured report fails, stdout still gets a minimal `ok: false` / `error_kind: operation_failed` envelope and exit 1 (CLI entry, tx, doc value format, library search format helpers). (#1651, #1652, #1653)
- **Doc parse and type errors.** Malformed JSON/YAML/TOML exit 4 with `parse_error`; selector type mismatches set `type_error` where the document shape is wrong for the operation. (#1590-#1592, #1595, #1618-#1619)
- **Patch plans report conflict kinds.** Stale context and merge conflicts map to the existing conflict/ambiguous exit codes with machine-readable kinds on the plan path. (#1593)
- **Create/rename conflicts are `already_exists`.** (#1587)

## Bug fixes

- **Tx verify `attr=` no longer steals a neighbor's attribute.** A fixed multi-line lookback could treat the next function as `#[test]` when a prior test sat nearby. Only contiguous annotation lines immediately above the symbol count; disk snapshots use a single read for extract and attr filter. (#1653)
- **`require_change` and `if_exists` ordering.** File `replace_text` no longer lets `require_change` override a missing-file `if_exists` policy. (#1499)
- **`continue_on_no_match: false` works.** Batch/plan multi-file paths stop after the first no-match when configured (both true and false used to keep going). (#1499)
- **Backup restore path matching.** `restore_path_from_latest_backup` matches the exact path in the session, not a basename-only collision across sessions. (#1499)
- **Library invalid search regex fails closed.** High-level `search` / `search_directory` return typed `InvalidInput` on bad patterns instead of `Ok([])` (soft no-match). (#1621, #1622)
- **Doc write preserves `format_failed`.** Engine remappers no longer drop post-write format failure kinds on `doc set` and related writes. (#1634)

## Performance

- **Multi-file AST rename pre-scan** (CLI and MCP) uses the adaptive parallel walker instead of sequential full-file reads. (#1612, #1614)
- **Reverse `ast deps` project scan** uses the same parallel walker (forward deps already did). (#1613)

## Numbers

| Metric | v0.11.0 | v0.12.0 | Delta |
|--------|---------|---------|-------|
| Unit tests | 2,201 | 2,339 | +138 |
| Integration tests | 1,008 | 1,071 | +63 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **3,219** | **3,420** | **+201** |
| CLI commands | 23 | 23 | -- |
| MCP tools (with `ast`) | 55 | 55 | -- |
| Commits since v0.11.0 | -- | 151 | -- |

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.12"

# Homebrew (after formula updates)
brew upgrade patchloom
```

**Library hosts:** `require_change` and `command_position` default to false (non-breaking). Prefer `edit_error_kind(&err)` for branching. AST file mutators and `ast_rename_batch` need `features = ["ast", "files"]` (or default features). Use `restore_path_from_latest_backup` for per-path validate/revert.

**CLI / scripts:** New optional flags `--require-change` and `--command-position` on `replace` (defaults off). Under `--json` / `--jsonl`, treat `error_kind` as the branch key: `not_found`, `no_matches`, `invalid_input`, `format_failed`, `parse_error`, `type_error`, `already_exists`, `changes_detected`, `conflicts`. Clap usage failures are exit 1 with `invalid_input`, not exit 2.

**Plans / MCP:** `"ops"` is accepted for `"operations"`. Plan and MCP replace accept the same `require_change` / `command_position` fields as the library. Prefer structured kinds over message text when driving retries.

**Tx verify:** If you use `kind=function,attr=test` (or similar), counts now ignore attributes from nearby previous items.
