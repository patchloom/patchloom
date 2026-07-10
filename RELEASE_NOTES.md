# Patchloom 0.12.0

Library surface for agent hosts (Bline-class embedders): fail-closed text edits, AST file mutators, shell command-position replace, batch rename, honest multi-edit diffs, and signature rewrite spacing that matches how agents write signatures. Built on 0.11 containment and plan reliability.

## Highlights

Embedders can set `ReplaceOptions.require_change` so zero matches become structured `EditErrorKind::NoMatch` instead of soft `Ok(changed=false)`. High-level AST writers cover rename, replace-in-symbol, and signature rewrite (including full-string `new_signature` and `FunctionSigEdit::parse_rust`). Multi-file `ast_rename_batch` serializes same-path work and returns per-file results. Opt-in `command_position` rewrites invocable shell tokens without touching `uv pip` / `pipenv`. Post-Apply validate/revert gets `backup::restore_path_from_latest_backup`. Multi-op content edits put the real path in unified-diff headers. Signature rewrite preserves the space (or newline) before `{` when agents omit trailing whitespace on the new signature.

## New features

- **Fail-closed content replace.** `ReplaceOptions.require_change` (default false). Zero matches → `EditErrorKind::NoMatch` with similar-target suggestions when available. `if_exists` still wins when both are set. Re-exports `EditError` / `EditErrorKind` / `edit_error_kind` for kind matching without scraping English. (#1492, #1497)
- **AST file mutators.** `api::ast_rename`, `api::ast_replace_in_symbol`, `api::ast_rewrite_signature_in_content`, plus on-disk `ast_rewrite_signature`. Zero identifier/symbol matches → `NoMatch`. (#1493, #1497)
- **`FunctionSigEdit::parse_rust`.** Rust-first parse of pub / pub(crate) / params-only / nested-paren signatures. (#1493, #1497)
- **Shell command-position matching.** Opt-in `ReplaceOptions.command_position` (not `word_boundary`). Must-pass grammar: bare `pip`, not `pipenv` / `uv pip` / `python -m pip`; wrappers `sudo` / `doas` / `env KEY=val` / `timeout 30` / `nice -n 10` / `stdbuf` / `ionice` / `setsid` / `runuser` / `busybox` / `flock` / `chroot` / `xargs` / `watch` / `strace` / `eval` / `source`; option flags like `-E` / `-p` and arg-taking flags like `-u USER`; separators `&&` `|` `;`. (#1494, #1497)
- **Validate/revert helper.** `backup::restore_path_from_latest_backup(project_root, path)` restores from the newest session that contains that exact path (exact match only). (#1494, #1497)
- **Batch AST rename.** `api::ast_rename_batch` with path dedupe, same-file serialization, `continue_on_no_match` (default true), `fail_fast` for hard errors, per-file `Result`. (#1495, #1497)

## Bug fixes

- **`apply_content_edits_to_file` diff headers.** File path appears in `--- a/` / `+++ b/` instead of `<buffer>`. Absolute paths still avoid `a//`. (#1500, #1502)
- **Signature rewrite body gap.** Full-string and structured rewrites no longer produce `-> i32{` when the new signature has no trailing space. Original space or newline before `{` is preserved; glued originals get a conventional space; `;` decls do not. (#1503, #1504)
- **`continue_on_no_match: false`.** Batch rename actually stops after the first `NoMatch` (was a no-op). (#1499)
- **`if_exists` vs `require_change` on file replace.** File path honors the same "if_exists wins" rule as content replace. (#1499)
- **Restore path matching.** Basename-only match removed so a different path with the same file name cannot restore the wrong session. (#1499)
- **`command_position` multi-line wrappers.** Prefix peeling no longer strips newlines, so `timeout` / `nice` / `sudo` on a later line are not confused with tokens from the previous line. Also peels `timeout 30`, `nice -n 10`, `stdbuf`, `ionice`, `setsid`, `runuser -u USER`, `busybox` applets, and path-taking `flock` / `chroot` wrappers.
- **`command_position` flag honesty.** Combining with `case_insensitive`, `word_boundary`, `fuzzy`, or context anchors is `InvalidInput` (was silently ignored, which looked like a soft no-match).
- **CLI identity replace honesty.** When the pattern matches but `new` equals `old`, `replace` no longer reports "no matches" / exit 3. It reports success with the raw match count and an "identical (no file changes)" note so `require_change` stays satisfied. JSON includes `identity: true`.
- **CLI patch JSON `error_kind`.** Stale apply/check failures set `error_kind: "ambiguous"` (exit 5), merge conflicts set `"conflicts"` (exit 8), and parse/input failures set `"parse_error"` (exit 4).
- **CLI search JSON `error_kind`.** Soft no-match (`--json` exit 3) sets `error_kind: "no_matches"`, matching replace/tx agents that branch on kind without scraping stderr.
- **CLI replace JSON `error_kind`.** `--json` failures set `error_kind: "no_matches"` (exit 3) or `error_kind: "ambiguous"` (exit 5 / `--unique`), matching tx plan JSON so agents can branch without scraping stderr.
- **Shared JSON `error_kind: no_matches`.** Exit-3 paths for doc (get/keys/len), md, and AST now set `error_kind` via `GlobalFlags::emit_error_json_kind` / doc `format_no_match`, so agents can branch like tx without scraping stderr.
- **GNU long options with `=`.** `command_position` peels `--name=value` flags (`nice --adjustment=10`, `sudo --user=root`, `timeout --signal=TERM 30`).
- **`env --unset VAR`.** Peels `--unset` as an arg-taking flag (alongside `env -u VAR`) so the following invocable command rewrites.
- **`env --chdir DIR`.** Peels `--chdir` the same way as `-C` so the following invocable command rewrites.
- **`timeout --kill-after` stacked durations.** Peels consecutive duration tokens so `timeout --kill-after 5 30 pip` and `timeout --kill-after=5 30 pip` rewrite the command (bare `5 30 pip` still stays non-command).
- **CI isolation wrappers.** `command_position` peels `unshare`, `nsenter`, `taskset`, `prlimit`, `numactl`, `chrt`, and `setpriv` (plus list values like `taskset -c 0,1`) so sandbox and affinity-wrapped installs rewrite.
- **Unit/sandbox wrappers.** `command_position` peels `systemd-run`, `firejail`, `dbus-run-session`, and `chronic`, plus bare `--` end-of-options markers (`dbus-run-session -- pip`).
- **`run0` privilege wrapper.** `command_position` peels systemd `run0` (modern `sudo` alternative) so `run0 -u root pip` rewrites the command.
- **Container entrypoint wrappers.** `command_position` peels `gosu`, `su-exec`, `tini`, and `dumb-init` so Docker/K8s install scripts rewrite the invocable command.
- **CLI undo JSON `error_kind`.** Soft no-session paths (`undo --list` empty, or no sessions to restore) set `error_kind: "no_matches"` (exit 3), matching other CLI exit-3 JSON envelopes.
- **CLI doc JSON `error_kind: type_error`.** `doc keys` / `doc len` on the wrong value type, and write-path type failures, set `error_kind: "type_error"` (exit 1) so agents can distinguish type mismatches from soft no-matches.
- **CLI top-level JSON typed errors.** When a command returns a typed `NoMatchError` / `AmbiguousError` through the global error path under `--json`/`--jsonl`, the envelope includes `error_kind` and exits 3/5 (was generic exit 1 without kind).
- **More shell wrappers.** `command_position` peels `eatmydata` and s6-style `s6-setuidgid` / `setuidgid` user wrappers.
- **Tx unique multi-match exit code.** Plan/tx `replace` with `unique: true` and multiple matches now exits **5** (`ambiguous`), matching CLI `replace --unique`, instead of generic exit 9 (`operation_failed`).
- **Tx require_change zero-match exit code.** Plan/tx `replace` with `require_change: true` and zero matches now exits **3** (`no_matches`), matching CLI, instead of generic exit 9.

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

**Release consumers:** New optional CLI flags `--require-change` and `--command-position` on `replace` (defaults off; existing scripts unchanged). Library-only AST mutators and `FunctionSigEdit::parse_rust` need no CLI action. Plan/MCP `ast.rewrite_signature` inherits body-gap fix automatically.
