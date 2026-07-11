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

- **Library `edit_error_kind` peels exit typed errors.** `InvalidInputError`, `NoMatchError`, `AmbiguousError`, `ParseErrorError`, `AlreadyExistsError`, and related kinds from CLI/tx paths now map to `EditErrorKind` (including through `.context()` wrappers). Empty replace/search patterns on `replace_in_content` / `search` / `search_directory` emit typed `InvalidInputError` so hosts can branch without scraping English.
- **`search_directory` / `search_file` invalid regex.** Bad patterns (e.g. unclosed groups) now return `InvalidInputError` / `edit_error_kind` `InvalidInput` instead of `Ok([])`, which looked like a soft no-match. Low-level `search_one_file` still returns empty on compile failure for custom walkers; high-level APIs preflight.
- **Shared replace/search regex compile typing.** `compile_replace_regex` and CLI `build_matcher` map parse failures to `InvalidInputError`, so library `search()` / `replace_in_content`, tx replace, and CLI `search --json` set `error_kind: invalid_input` without scraping the regex crate message.
- **`bounded_regex_build` helper.** Finishes a `bounded_regex_builder` as `InvalidInputError`. Used by replace/search compile, tx plan search, and AST in-symbol regex replace so all paths share one kind.
- **Agent-rules / reference docs.** Document invalid search/replace regex as `error_kind: invalid_input` (exit 1) so agents do not treat compile failures as soft no-match.
- **Replace invalid-regex JSON lock.** CLI `replace --json` with a bad pattern sets `error_kind: invalid_input` (integration coverage next to the existing text-mode regex parse test).
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
- **CLI file ops JSON error_kind.** `create`/`rename` conflicts set `already_exists`; missing targets for `delete`/`append`/`prepend`/`rename` set `not_found`; invalid flag combinations and non-file targets set `invalid_input` (all exit 1).
- **CLI tidy JSON `invalid_input`.** `tidy fix --dedent … --indent …` together exits 1 with `error_kind: "invalid_input"` under `--json`.
- **CLI md JSON `invalid_input`.** `md move-section` without `--before`/`--after`, and missing `--stdin`/`--content` on insert/replace paths, set `error_kind: "invalid_input"` (exit 1).
- **CLI explain JSON kinds.** Missing/unreadable plan files set `not_found` / `invalid_input`; plan parse failures exit **4** with `error_kind: "parse_error"`.
- **CLI patch/rename JSON `invalid_input`.** Unreadable patch targets during merge-check and binary rename with write-policy flags set `error_kind: "invalid_input"` (exit 1).
- **CLI batch JSON `parse_error`.** Line parse failures (unknown op, bad arity, bad quotes) exit **4** with `error_kind: "parse_error"` (was unstructured exit 1). Too-many-ops limit stays exit 1 with `invalid_input`.
- **CLI read/status/ast/doc JSON kinds.** Invalid `--lines` sets `invalid_input`; all-paths-missing read sets `not_found`; status outside a git repo sets `invalid_input`; AST missing path / non-dir map target set `not_found` / `invalid_input`; doc merge flag conflicts set `invalid_input`.
- **CLI replace JSON `invalid_input`.** Validation failures (bad `--nth`, `--range` without `--whole-line`, incompatible flag combos, context without paths) set `error_kind: "invalid_input"` (exit 1).
- **CLI search JSON `invalid_input`.** Empty pattern and `--invert-match` + `--multiline` together exit 1 with `error_kind: "invalid_input"` under `--json`.
- **CLI top-level JSON typed errors.** When a command returns a typed `NoMatchError` / `AmbiguousError` through the global error path under `--json`/`--jsonl`, the envelope includes `error_kind` and exits 3/5 (was generic exit 1 without kind).
- **CLI usage errors exit 1.** Invalid flags, enum values, missing required args, and unknown subcommands exit **1** (`FAILURE`), not clap's default **2**. Exit 2 remains only `CHANGES_DETECTED` (`--check` / write preview). `--help` / `--version` still exit 0.
- **CLI usage + `--json`/`--jsonl` envelope.** When parse fails after a global `--json`/`--jsonl`, stdout gets `ok: false`, `error_kind: "invalid_input"`, and the clap message (agents no longer scrape colored stderr).
- **Replace empty-pattern wording.** Empty replace `old`/`pattern` reports `replace pattern must not be empty` (was `search pattern…`), with `error_kind: "invalid_input"` under `--json`.
- **`--contain` JSON `invalid_input`.** Path rejections and empty path arguments under `--contain` set `error_kind: "invalid_input"` on the global `--json`/`--jsonl` error path (typed `InvalidInputError`, not English scraping).
- **Cleaner clap JSON usage messages.** Usage failures under `--json`/`--jsonl` strip the `error: ` prefix and help footer so agents get a short actionable string.
- **Plan `ops` alias.** Transaction plans accept `"ops"` as a serde alias for `"operations"` so agents that emit the shorter field name parse without a `missing field` error.
- **Missing path roots are `not_found`.** When every explicit path root for `search`, `replace`, or `tidy` is missing (including `--files-from` lists, not stdin), exit **1** with `error_kind: "not_found"` (was soft `no_matches` / vacuous tidy success). Pattern misses and clean trees still use exit 3 / 0.
- **Tx file ops missing targets `not_found`.** Plan `file.append` / `file.prepend` / `file.delete` on missing paths emit `error_kind: "not_found"` (exit 1), not generic `operation_failed`.
- **Tx create/rename conflicts `already_exists`.** Plan `file.create` without force and rename into an existing dest set `error_kind: "already_exists"` (exit 1), matching standalone CLI file ops.
- **Tx missing-file `not_found`.** Plan/tx engine IO `NotFound` (e.g. `md.replace_section` on a missing path) sets `error_kind: "not_found"` and exit **1** instead of generic `operation_failed` (9).
- **Engine IO not_found chain.** Tx/CLI engine read failures preserve `std::io::ErrorKind::NotFound` through context wrappers so global `--json` maps them to `error_kind: "not_found"` (e.g. `md replace-section` on a missing file).
- **Doc navigate type mismatches `type_error`.** Parent-not-array/object and related selector type failures in plan doc ops set `error_kind: "type_error"` (exit 1).
- **Tx doc type mismatches `type_error`.** Plan doc append/prepend/delete_where (and similar type failures) set `error_kind: "type_error"` (exit 1), matching CLI doc keys/len.
- **Tx non-file targets `invalid_input`.** Plan file ops against directories set `error_kind: "invalid_input"` (exit 1). Engine `path_err` keeps IO NotFound through path prefixing so missing docs stay `not_found`.
- **More shell wrappers.** `command_position` peels `eatmydata` and s6-style `s6-setuidgid` / `setuidgid` user wrappers.
- **Tx unique multi-match exit code.** Plan/tx `replace` with `unique: true` and multiple matches now exits **5** (`ambiguous`), matching CLI `replace --unique`, instead of generic exit 9 (`operation_failed`).
- **Tx require_change zero-match exit code.** Plan/tx `replace` with `require_change: true` and zero matches now exits **3** (`no_matches`), matching CLI, instead of generic exit 9.
- **Tx patch.apply merge conflicts.** Plan `patch.apply` with `on_stale: "merge"` without `allow_conflicts` exits **8** with `error_kind: "conflicts"` (was generic `operation_failed` / 9), matching CLI `patch merge`.
- **Tx op flag conflicts `invalid_input`.** Plan-level option conflicts (replace whole_line+multiline, range without whole_line, tidy dedent+indent, md.move_section before/after, search invert+multiline) exit **1** with `error_kind: "invalid_input"` (was `parse_error` / 4), matching standalone CLI flags.
- **Doc unsupported extension `invalid_input`.** `doc` on non-JSON/YAML/TOML paths (and plan doc ops on those paths) set `error_kind: "invalid_input"` (exit 1). CLI write path no longer mislabels them as `type_error`.
- **Tx AST empty directory `no_matches`.** Plan `ast.rename` on a directory with no source files exits **3** with `error_kind: "no_matches"` (was `operation_failed` / 9), matching CLI `ast rename`.
- **Doc parse failures `parse_error`.** Malformed JSON/YAML/TOML content exits **4** with `error_kind: "parse_error"` (was unstructured exit 1), for CLI doc and plan/tx doc ops.
- **Tx AST extract/rewrite kinds.** `ast.extract_to_file` into an existing target without `force` sets `already_exists` (exit 1). `ast.rewrite_signature` without signature fields sets `invalid_input` (exit 1).
- **Tx mid-plan delete then use is `not_found`.** Append/prepend/rename after `file.delete` in the same plan set `error_kind: "not_found"` (exit 1). Workspace-guard escapes set `invalid_input`.
- **Tx search assert_count mismatch.** Plan `search` with `assert_count` when the actual match count differs exits **2** with `error_kind: "changes_detected"` (was `operation_failed` / 9), matching CLI `search --assert-count`.
- **CLI `--cwd` missing/non-dir `invalid_input`.** Bad `--cwd` paths exit **1** with `error_kind: "invalid_input"` under `--json` (typed `InvalidInputError`).
- **Empty path strings `invalid_input`.** `resolve_user_path` rejects empty paths with typed `InvalidInputError` (same as `check_paths_contained`).
- **Doc navigate selector mistakes typed.** Empty selectors, unsupported wildcards on write paths, bad predicates, and out-of-bounds indexes set `invalid_input`; expected-object type mismatches set `type_error`.
- **AST split/search/symbols and plan verify typed.** Duplicate or unaccounted split symbols and overlapping symbol spans set `invalid_input`; invalid AST search patterns set `parse_error`; malformed plan `verify` specs set `invalid_input` (was unstructured exit 1).
- **More agent JSON kinds.** Invalid `normalize_eol` values, md `table-append` row/table failures, and library doc type mismatches set `invalid_input` / `type_error`; missing git-blob paths for AST set `not_found` (was unstructured exit 1 / operation_failed).
- **AST symbol-not-found is `no_matches`.** extract/insert/reorder/wrap/move paths set `error_kind: "no_matches"` (exit 3) when a named symbol is missing; create race `already_exists` and unknown undo session are typed the same way.
- **Patch apply/parse typed for tx.** Plan/engine patch parse failures set `parse_error`; stale context sets `ambiguous`; other apply failures set `invalid_input` (CLI path already emitted kinds via string maps).
- **files-from / batch input / MCP bind typed.** Missing `--files-from` or batch input files set `not_found`; unreadable lists and invalid MCP bind/TLS config set `invalid_input`; AST validate parse failure sets `parse_error`.
- **Agent-rules `error_kind` catalogue expanded.** Exit 3/4 rows and exit-1 kind list cover AST symbol misses, normalize_eol, table-append, patch/plan/files-from kinds so agents match runtime envelopes.
- **Faster multi-file `ast rename` pre-scan.** Match detection across many files uses the same adaptive parallel walker as search/replace/tidy (was sequential full-file reads).
- **Faster reverse `ast deps`.** Project-wide importer scan uses the parallel walker (forward deps already did).
- **Faster MCP multi-file `ast_rename` pre-scan.** Same parallel match probe as CLI `ast rename`.
- **Reference docs for typed extract/split/write_policy failures.** `docs/reference` documents `no_matches` / `already_exists` / `invalid_input` for extract, split, and invalid `normalize_eol`.
- **Reference docs for insert/wrap/reorder/move failure kinds.** Missing symbols and bad wrap/reorder inputs document `no_matches` / `invalid_input`.
- **Reference docs for replace/group/MCP bind failures.** `ast.replace` / `ast.group` missing symbols and MCP bind/TLS `invalid_input` documented.
- **YAML/TOML preserve-path typed errors.** Comment-preserving re-parse failures set `parse_error`; serialization failures set `invalid_input` (was unstructured exit 1).
- **YAML newline-string escape typed.** Failure double-quoting multiline YAML scalars sets `invalid_input`.

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
