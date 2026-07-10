# Patchloom 0.12.0

Library surface for agent hosts (Bline-class embedders): fail-closed text edits, AST file mutators, shell command-position replace, batch rename, honest multi-edit diffs, and signature rewrite spacing that matches how agents write signatures. Built on 0.11 containment and plan reliability.

## Highlights

Embedders can set `ReplaceOptions.require_change` so zero matches become structured `EditErrorKind::NoMatch` instead of soft `Ok(changed=false)`. High-level AST writers cover rename, replace-in-symbol, and signature rewrite (including full-string `new_signature` and `FunctionSigEdit::parse_rust`). Multi-file `ast_rename_batch` serializes same-path work and returns per-file results. Opt-in `command_position` rewrites invocable shell tokens without touching `uv pip` / `pipenv`. Post-Apply validate/revert gets `backup::restore_path_from_latest_backup`. Multi-op content edits put the real path in unified-diff headers. Signature rewrite preserves the space (or newline) before `{` when agents omit trailing whitespace on the new signature.

## New features

- **Fail-closed content replace.** `ReplaceOptions.require_change` (default false). Zero matches → `EditErrorKind::NoMatch` with similar-target suggestions when available. `if_exists` still wins when both are set. Re-exports `EditError` / `EditErrorKind` / `edit_error_kind` for kind matching without scraping English. (#1492, #1497)
- **AST file mutators.** `api::ast_rename`, `api::ast_replace_in_symbol`, `api::ast_rewrite_signature_in_content`, plus on-disk `ast_rewrite_signature`. Zero identifier/symbol matches → `NoMatch`. (#1493, #1497)
- **`FunctionSigEdit::parse_rust`.** Rust-first parse of pub / pub(crate) / params-only / nested-paren signatures. (#1493, #1497)
- **Shell command-position matching.** Opt-in `ReplaceOptions.command_position` (not `word_boundary`). Must-pass grammar: bare `pip`, not `pipenv` / `uv pip` / `python -m pip`; wrappers `sudo` / `doas` / `env KEY=val` / `timeout 30` / `nice -n 10` / `stdbuf` / `ionice` / `xargs` / `watch` / `strace` / `eval` / `source`; option flags like `-E` / `-p` and arg-taking flags like `-u USER`; separators `&&` `|` `;`. (#1494, #1497)
- **Validate/revert helper.** `backup::restore_path_from_latest_backup(project_root, path)` restores from the newest session that contains that exact path (exact match only). (#1494, #1497)
- **Batch AST rename.** `api::ast_rename_batch` with path dedupe, same-file serialization, `continue_on_no_match` (default true), `fail_fast` for hard errors, per-file `Result`. (#1495, #1497)

## Bug fixes

- **`apply_content_edits_to_file` diff headers.** File path appears in `--- a/` / `+++ b/` instead of `<buffer>`. Absolute paths still avoid `a//`. (#1500, #1502)
- **Signature rewrite body gap.** Full-string and structured rewrites no longer produce `-> i32{` when the new signature has no trailing space. Original space or newline before `{` is preserved; glued originals get a conventional space; `;` decls do not. (#1503, #1504)
- **`continue_on_no_match: false`.** Batch rename actually stops after the first `NoMatch` (was a no-op). (#1499)
- **`if_exists` vs `require_change` on file replace.** File path honors the same "if_exists wins" rule as content replace. (#1499)
- **Restore path matching.** Basename-only match removed so a different path with the same file name cannot restore the wrong session. (#1499)
- **`command_position` multi-line wrappers.** Prefix peeling no longer strips newlines, so `timeout` / `nice` / `sudo` on a later line are not confused with tokens from the previous line. Also peels `timeout 30`, `nice -n 10`, `stdbuf`, and `ionice` wrappers.
- **`command_position` flag honesty.** Combining with `case_insensitive`, `word_boundary`, `fuzzy`, or context anchors is `InvalidInput` (was silently ignored, which looked like a soft no-match).

## Agent and library notes

- Prefer `require_change: true` in agent hosts that treat zero matches as tool errors.
- Use `command_position` only when rewriting shell invocable names; keep ordinary replace or word_boundary for identifiers.
- Plan `replace` and MCP replace accept the same `require_change` / `command_position` fields as the library API (default false).
- CLI: `patchloom replace … --command-position --require-change` (and MCP `batch_replace`) expose the same flags.
- Full-string signature rewrites may omit trailing space before `{`; the library normalizes the body gap.
- Multi-op content edits expose rolled-up `match_count` on `ContentEditsResult` / file `EditResult`. Crate-root re-exports include `ContentEdit`, `ContentEditsResult`, and `apply_content_edits`.

## Numbers

Rounded test counts and MCP inventory track the library expansions in this cycle. See the README badge and `make check-readme` for the live floor.

## Upgrading

```bash
cargo install patchloom --locked
# or
patchloom = "0.12"
```

**Library:** New `ReplaceOptions` fields default to false (non-breaking). New AST batch / rename APIs need `features = ["ast", "files"]` (or default features). Match structured errors with `edit_error_kind(&err)`.

**Release consumers:** No CLI flag changes required for the new library-only options. Plan/MCP `ast.rewrite_signature` inherits body-gap fix automatically.
