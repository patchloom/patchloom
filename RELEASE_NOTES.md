# Patchloom 0.13.0

Agent embedder and match reporting release: library hosts get scoped symbol
replace, non-anyhow error kinds, surgical undo, and post-write validation.
Every replace path (CLI, plan/tx, MCP, library) reports how the match landed
(`exact` / `fuzzy` / `anchored`) with optional scores and counts so agents can
verify low-confidence edits without scraping English. 14 commits since 0.12.0,
with about 35 new tests.

## Highlights

Fuzzy and context replace work end-to-end on disk, globs, directories, plans, and
MCP. Multi-file and multi-op batches roll up worst-case confidence
(`fuzzy` > `anchored` > `exact`) so mixed results never under-report fuzzy.
Plan/tx JSON carries per-change and aggregate `match_mode`, `match_score`, and
`match_count` from the engine (not a second content re-derive). Library
embedders get `ast_replace_in_symbol` with regex, `classify_error` for
`dyn Error`, single-path backup restore, and `run_post_write_validation` with
`format_failed`. Product docs talk about LLM agent hosts, not a single
downstream product.

## New features

- **LLM agent embedder surfaces.** `ast_replace_in_symbol` (literal/regex),
  `classify_error` / `classify_error_ref`, `backup::restore_path_from_session`,
  `run_post_write_validation` / `PostWriteHooks`, `MatchMode` + scores on
  content and file results, `apply_content_edits_with_label`,
  `find_files_with_symbol` + batch rename helpers, and shell
  `command_position` on multi-op content edits. Signature rewrite body-gap
  invariant is locked so one Apply is enough. (#1658–#1666, #1667)
- **Plan/tx pure fuzzy replace.** `Operation::Replace.fuzzy` and MCP/CLI
  flags enable similarity fallback without context anchors, matching library
  `ReplaceOptions.fuzzy`. (#1668, #1671)
- **Match reporting on every surface.** CLI `replace --json`, MCP
  `replace_text` / `batch_replace` / `execute_plan`, and library `EditResult`
  report `match_mode` and optional `match_score`. Plan/tx also report
  `match_count` per change and a sum. Aggregates use worst-case rollup.
  (#1669, #1673, #1674, #1676, #1681, #1682)
- **Shared rollup helper.** `api::merge_match_modes` is the single ordering
  for content_edits, CLI multi-file, and tx. (#1681)

## Bug fixes

- **Disk pure fuzzy was a no-op on the default files path.** `replace_text`
  with `fuzzy: true` and no context now routes through the content path with
  accurate mode/score. (#1670)
- **Fuzzy/context parity for globs and directories.** Plan glob replace and CLI
  `--fuzzy` expand directories via the normal file collector; multi-match
  `unique` errors match single-path behavior. (#1672)
- **CLI fuzzy JSON prefers engine meta.** No longer re-derives via
  `replace_in_content` and invents `exact` when re-derive fails. (#1677)
- **MCP replace_text no longer overwrites engine match meta.** Removed a
  pre-apply re-derive that forced `range: None` and could lie about ranged
  or fuzzy applies. TxOutput is the source of truth. (#1682)
- **EditResult match_mode and match_count from tx.** Exact disk replaces
  no longer report `match_count: 0` / `match_mode: None` while `changed:
  true`. (#1679)
- **Post-write revert fails closed** when backup restore cannot complete;
  `max_files: 0` returns empty for symbol discovery; `FormatFailed` is a
  distinct kind. (#1670)
- **Tidy JSON emit and check-fast README accuracy.** (#1656)

## Documentation

- Agent-rules and PATCHLOOM.md document match reporting, library undo helpers,
  and worst-case aggregates.
- Product framing uses LLM agent / embedder language (custom ignore examples
  use `.agentignore`; any filename still works).
- Quickstart tx JSON example includes `match_count`.

## Numbers

| Metric | v0.12.0 | v0.13.0 | Delta |
|--------|---------|---------|-------|
| Unit tests | 2,339 | ~2,402 | +~63 |
| Integration tests | 1,071 | ~1,086 | +~15 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **~3,420** | **~3,455** | **+~35** |
| CLI commands | 23 | 23 | -- |
| MCP tools (with `ast`) | 56 | 56 | -- |
| Commits since v0.12.0 | -- | 14 | -- |

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.13"

# Homebrew (after formula updates)
brew upgrade patchloom
```

**Library hosts:** New APIs are additive. Prefer `classify_error` /
`edit_error_kind` for branching. Check `EditResult.match_mode` /
`match_score` / `match_count` after replace. Post-write hooks map to
`EditErrorKind::FormatFailed`. Use `features = ["ast", "files"]` for pure
library embedders without CLI/MCP.

**CLI / scripts:** New optional `--fuzzy` (and plan/MCP `fuzzy`) defaults
off. Under `--json`, multi-file replace top-level `match_mode` is worst-case
when files disagree (no longer omitted when mixed). Prefer `error_kind` and
`match_mode` over message text.

**Plans / MCP:** `fuzzy` on replace ops; `batch_replace` / `execute_plan` /
`replace_text` JSON include engine `match_mode`, optional `match_score`, and
`match_count`. Do not re-parse English for confidence.
