//! Hand-written **policy** for agents (honesty, peels, routing, when-to-use).
//!
//! Not generated from schema. Schema owns operation inventory only; this module
//! owns "how hosts and agents must behave." See parent module docs.

/// Shared Apply safety, decision tree, MCP/CLI tool guide, host peels, fuzzy policy.
pub(crate) fn append_policy(out: &mut String, show_cli: bool, show_mcp: bool) {
    // Shared Apply write semantics (CLI + MCP + library).
    out.push_str(
        "**Apply write safety:** all Apply paths share one writer. Symlinks are resolved so \
         the link entry is not replaced (#1230). On Unix, files with multiple hard links \
         (`nlink > 1`) are updated in place so sibling paths stay in sync (#1733); single-link \
         files use temp+rename. CLI `rename` and plan `file.rename` (including force overwrite) \
         use `fs::rename` so multi-hardlinked sources keep their shared inode (#1739, #1746).\n\n",
    );

    // Decision tree before full inventory (competitive research / agent routing).
    out.push_str(
        "## Which surface to use\n\n\
         Prefer Patchloom over shell `sed`/`jq`/`yq` and over whole-file rewrites when \
the edit is structured or multi-file. Prefer **ast-grep** (or similar) for \
**structural code search / pattern codemods** when you need a syntax-tree pattern \
DSL; use Patchloom for host-safe apply, configs, markdown, batch/tx/undo, and peels.\n\n\
         | If the task is… | Prefer | Avoid |\n\
         |---|---|---|\n\
         | JSON/YAML/TOML single key/index path | `doc set` / plan `doc.set` / MCP `doc_set` (e.g. `server.port`, `items.0.v`) | regex replace on structured files |\n\
         | JSON/YAML/TOML multi-match by predicate or wildcard | `doc update` / plan `doc.update` / MCP `doc_update` (e.g. `items[name=a].v`, `items[*].enabled`) | `doc set` with predicates (fails closed) |\n\
         | Multi-document YAML | selector `0.key` or `[0].key` | bare key on stream root |\n\
         | Markdown section/bullet/table | `md *` | whole-file rewrite |\n\
         | Identifier rename in code | `ast rename` / `ast_rename_project` | fuzzy replace for symbols |\n\
         | Find code by AST shape (pattern DSL) | ast-grep (or `ast search`) | blind text grep only |\n\
         | Freeform code body change (known span or symbol) | `replace` exact/`ast replace` / `apply-fragment --old` | whole-file rewrite |\n\
         | Place new lines next to known text | `replace` + `--insert-after` / `--insert-before` or `apply-fragment --after/--before` | inventing full-file rewrite |\n\
         | Morph-style snippet with markers **and** known anchor | `apply-fragment` / plan `apply.fragment` / MCP `apply_fragment` (markers stripped) | Morph cloud merge |\n\
         | Lazy Morph-style markers only (`// ... existing code ...`, **no** anchors) | **not supported** | Morph merge APIs; supply after/before/old (#2018) |\n\
         | Prose/typo in non-code text | `replace` (fuzzy last resort) | AST |\n\
         | Multi-file atomic apply | `tx` / `batch` / `execute_plan` | sequential shell |\n\
         | Host agent policy (Rust) | `ReplaceOptions::for_agent` + peels + `fuzzy_span_suspicious` | soft defaults |\n\n\
         **Morph Fast Apply jobs:** prefer Patchloom offline for exact/`doc`/`md`/`ast`/`batch`/`undo`. \
For Morph-class snippets, use `apply-fragment` with a **required** after/before/old anchor \
(lazy markers stripped; no model merge). Migration matrix: docs/plans/morph-gap-matrix.md (#2018).\n\n\
         **Context budget:** prefer `read` with a line range, `search --count` / \
`--files-with-matches`, and one `batch`/`tx` over N full-file dumps. Use `--jsonl` \
for large result streams. Binary sole paths peel `error_kind: binary`.\n\n",
    );
    // Core pack list must track CORE_MCP_TOOL_NAMES (single source; #2070/#2076).
    let core_tools = crate::cmd::agent_packaging::CORE_MCP_TOOL_NAMES
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "**MCP tool volume:** the server may expose many tools; start from this table \
(and `schema --tier weak|medium|strong` for plan prompts). Full inventory is the default. \
Small agents / tight context: set env `PATCHLOOM_MCP_SURFACE=core` so handshake registers \
only the core pack ({core_tools}). Prefer core alone for list+edit (no second filesystem MCP). \
`PATCHLOOM_MCP_SURFACE=full` or unset keeps the full inventory. `server_info` reports \
`cwd`, `surface`, `tool_count`, package `version`, and MCP `protocol_version`. \
Handshake instructions are surface-aware (core does not list \
full-only tool names). `execute_plan` on core can still run full plan ops; the env reduces \
tool schema size, not the plan catalog. See docs/plans/mcp-surface-tiers.md.\n\n"
    ));

    // When to use
    if show_mcp {
        out.push_str(
            "## Tool selection guide\n\n\
             | Task pattern | Tool to use |\n\
             |---|---|\n\
             | Read file contents (with optional line range) | `read_file` |\n\
             | See uncommitted changes vs git HEAD (omits `.patchloom/` backups) | `git_status` |\n\
             | Set/get a value in JSON, YAML, or TOML | `doc_set`, `doc_get`, `doc_query` |\n\
             | Delete, merge, or ensure a value exists | `doc_delete`, `doc_merge`, `doc_ensure` |\n\
             | Compare two structured files | `doc_diff` |\n\
             | Edit markdown section, bullet, or table | `md_replace_section`, `md_upsert_bullet`, `md_table_append` |\n\
             | Insert text after/before a heading | `md_insert_after_heading`, `md_insert_before_heading` |\n\
             | Insert a sibling section after a full section body | `md_insert_after_section` |\n\
             | Move a heading section (same file or cross-file) | `md_move_section` |\n\
             | Remove later whole sections with a duplicate heading | `md_dedupe_headings` |\n\
             | Lint markdown for structural issues | `md_lint` |\n\
             | Fix trailing whitespace or missing newlines | `fix_whitespace` (one file) or `batch_tidy` (multiple files) |\n\
             | Create, append, prepend, rename, or delete a file | `create_file`, `append_file`, `prepend_file`, `move_file`, `delete_file` |\n\
             | Find/replace text in a file | `replace_text` (one file) or `batch_replace` (same replacement across multiple files) |\n\
             | Search across files | `search_files` |\n\
             | List/inventory files (ignore-aware; max_depth prunes walk; prefer over FS MCP) | `list_files` |\n\
             | Apply a unified diff patch | `apply_patch` |\n\
             | List/read/rename symbols (AST-aware) | `ast_list`, `ast_read`, `ast_rename`, `ast_replace`, `ast_rewrite_signature` |\n\
             | Insert, wrap, or manage imports | `ast_insert`, `ast_wrap`, `ast_imports` |\n\
             | Reorder, group, or move symbols | `ast_reorder`, `ast_group`, `ast_move` |\n\
             | Extract or split files by symbol | `ast_extract_to_file`, `ast_split` |\n\
             | Validate syntax, find refs, or analyze impact | `ast_validate`, `ast_refs`, `ast_impact`, `ast_search` |\n\
             | Repo map, imports, or structural diff | `ast_map`, `ast_deps`, `ast_diff` |\n\
             | Apply same operation to many files | `execute_plan` with `for_each` glob |\n\
             | Get package version, MCP protocol_version, surface, tool_count, and cwd | `server_info` |\n\n\
             **`replace_text` / plan replace flags (default false):**\n\
             - `require_change`: error when the pattern matches zero times (fail closed).\n\
             - `command_position`: rewrite only shell invocable tokens (`sudo`/`timeout`/`flock`/`runuser`/`setsid`/`run0`/`gosu`/`su-exec`/`tini`/`dumb-init`/`unshare`/`nsenter`/`taskset`/`systemd-run`/`firejail`/`busybox`/`chpst`/`softlimit`/`envdir`/`setlock` wrappers yes; `uv pip` no).\n\
             - `fuzzy`: similarity fallback when exact match fails (also with before_context/after_context).\n\
             - `min_fuzzy_score`: reject fuzzy matches below this floor (0.0..=1.0); exact/anchored unaffected (#1687).\n\
             - `allow_absent_old`: only then apply fuzzy when exact `old` is not in the file (#1758). Default false (fail closed).\n\
             Example: `{\"path\":\"install.sh\",\"old\":\"pip\",\"new\":\"uv\",\
             \"command_position\":true,\"require_change\":true}`\n\n\
             **Library host checklist (#2009):** ordered onboarding for LLM agent hosts / embedders \
(primary + fallback replace, peels, multi-op honesty, pre-write span policy) lives in \
`docs/getting-started/embedder-host.md` (linked from README and crate docs).\n\n\
             **Library `for_each` + lifecycle (#2168 / #2169):** `plan.for_each` expands under the \
`files` feature (not `cli` only). `execute_plan` expands before PathGuard so \
`declared_paths` sees concrete files. Plan `format`/`validate` are raw shell; MCP strips them. \
Hosts with PathGuard get an automatic refuse of redirects/pipelines/substitutions. \
Preflight: `api::lifecycle_cmds(plan)` + `api::refuse_lifecycle_shell_metas(cmd)` \
(`true` / `cargo fmt` / `rustfmt` with no metas still run).\n\n\
             **Library patch dest preflight (#2170–#2176):** hosts that refuse secret dests \
before `apply_patch_file` must use `api::unquote_git_c_string` / `api::parse_diff_file_path` \
/ `api::parse_diff_git_paths` / `api::patch_declared_paths` (or `parse_unified_diff` + \
`path`/`rename_from`/`copy_from`). Do not quote-peel only and do not whitespace-split \
`diff --git`. Git 100% `copy from`/`copy to` creates dest and keeps source. Git-meta \
binary / mode-only dests are listed then apply refuses (no silent skip on mixed patches).\n\n\
             **Library `ReplaceOptions::for_agent` (#1965 / #2005):** Rust hosts with primary + fallback \
replace paths should call `ReplaceOptions::for_agent()` in **both** places (not hand-copy \
`ReplaceOptions { ... }` twice). Preset: `unique=true`, `require_change=true`, `fuzzy=true`, \
`min_fuzzy_score=Some(AGENT_MIN_FUZZY_SCORE)` (`0.90`), **`allow_absent_old=false`** (fail closed), \
**`refuse_suspicious_fuzzy=true`** (auto-refuse over-wide fuzzy as `EditErrorKind::FuzzySpanSuspicious` / \
`error_kind: fuzzy_span_suspicious`; peel with `api::is_fuzzy_span_suspicious`). \
Overrides via struct update: replace-all → `unique: false`; deliberate approximate recovery → \
`allow_absent_old: true`; raw fuzzy without span refuse → `refuse_suspicious_fuzzy: false`; \
word-boundary rename → `fuzzy: false`, `min_fuzzy_score: None`, \
`word_boundary: true`. `command_position` cannot combine with fuzzy/word_boundary/regex/whole_line \
(typed `invalid_input`). This is **not** a host-specific recovery policy; approximate rewrite of \
missing `old` stays an explicit opt-in.\n\n\
             **Fuzzy defaults fail closed when exact old is absent (#1758):**\n\
             - If `old` is not present, fuzzy will **not** rewrite a nearby live span by default, even when \
score ≥ `min_fuzzy_score`. JSON error explains the best candidate; set `allow_absent_old=true` only for deliberate approximate recovery.\n\
             - Prefer `ast_rename` / `ast_rename_project` for code identifiers. Fuzzy is a last \
resort for typos in non-AST text (prose, comments), not a general rename tool.\n\
             - When you opt into `allow_absent_old`, still check JSON `matched_text` before treating the edit as semantic success (#1736).\n\
             - **No second named recovery constructor:** hosts that always want approximate recovery keep the one-line override above (not a full host policy; still set `unique` / `word_boundary` per call). Closed as not planned (#1980).\n\
             - **Over-wide fuzzy refuse (#1981 / #2005 / #2006 / #2064):** `for_agent()` sets \
`refuse_suspicious_fuzzy=true` so `replace_in_content` / disk replace auto-refuse over-wide \
fuzzy (no second host call required). Custom policy or non-agent defaults: still call \
`api::fuzzy_span_suspicious(old, matched_text.as_deref(), match_score)` (or \
`fuzzy_span_suspicious_with_policy` + `FuzzySpanPolicy`) **before treating Apply as trusted**. \
Default policy (Unicode chars): refuse when matched is wider than \
`max(4 * old_chars, old_chars + 40)`, or score is in `[0.90, 0.95)` and ratio `> 2`. \
Do not rely on score alone. Multi-op `apply_content_edits` rolls up the **widest** \
`matched_text` and the **minimum** fuzzy `match_score` independently (they may come from \
different ops); use **`ContentEditsResult.op_honesty`** per replace (`old`, `matched_text`, \
`match_score`) for correct refuse pairing (#2006), or call \
`api::refuse_batch_if_suspicious_fuzzy(&batch, &FuzzySpanPolicy::default())` for a full \
batch gate after buffer multi-op (#2064). Plan/tx multi-path top-level uses the same \
worst-case rollup as content_edits: **widest** `matched_text` + **min** fuzzy score (#2007). \
Per-path details stay on `changes[]`. File multi-op Apply with a final span gate: \
`apply_content_edits_to_file_with_span_policy(..., Some(&FuzzySpanPolicy::default()))` \
refuses before write/backup (#2008). Prefer per-op honesty; rollup fields are worst-case only.\n\n\
             **Library embedder undo / post-write (Rust hosts, not CLI-only):**\n\
             - After `ApplyMode::Apply`, `EditResult.backup_session` is the session id for that write (#1686).\n\
             - On `Err` after write/backup (`format_failed` or fail-restore), `api::backup_session_from_error(&err)` peels the session id without scraping Display (#2127).\n\
             - `backup::restore_path_from_latest_backup(project_root, path)` — latest session that contains the path\n\
             - `backup::restore_path_from_session(project_root, timestamp, path)` — one path from a chosen session (#1660)\n\
             - `backup::list_sessions_under(root, &ListSessionsOptions { descendants: true, .. })` — nested monorepo sessions (#1688)\n\
             - `backup::find_backup_roots(path)` — walk path and parents for roots that own `.patchloom/backups` (nearest first; #1934)\n\
             - File create/delete/rename/append and `ast_rewrite_signature` peel via `edit_error_kind` \
               / `error_kind_str` (#1935 / #1936 / #1947 / #1948):\n\
               | Condition | `EditErrorKind` / CLI string |\n\
               |-----------|-----------------------------|\n\
               | Create/rename dest exists without force | `AlreadyExists` / `already_exists` (not `InvalidInput`; use force/overwrite) |\n\
               | Missing path I/O | `NotFound` / `not_found` |\n\
               | Directory target / empty path / unreadable IO | `InvalidInput` / `invalid_input` |\n\
               | Binary (NUL probe) | `Binary` / `binary` (#1963) |\n\
               | Invalid UTF-8 text | `InvalidEncoding` / `invalid_encoding` (#1963) |\n\
               | Over-wide fuzzy span refuse | `FuzzySpanSuspicious` / `fuzzy_span_suspicious` (#2005; `is_fuzzy_span_suspicious`) |\n\
               | Force create over binary/unreadable prior | succeeds with empty original (#1962) |\n\
               | PathGuard / `--contain` escape | `GuardRejected` / `guard_rejected` |\n\
               | Patch merge conflict markers | `Conflicts` / `conflicts` (not batch `ConflictingEdit`) |\n\
               | Check/assert-count exit-2 soft fail | `ChangesDetected` / `changes_detected` |\n\
               | AST missing symbol | `NoMatch` / `no_matches` |\n\
             - Bool peels (match `edit_error_kind`): `api::is_already_exists`, `is_not_found`, \
               `is_conflicts`, `is_changes_detected`, `is_type_error`, `is_format_failed`, \
               `is_guard_rejected`, `is_invalid_input`, `is_binary`, `is_invalid_encoding`, \
               `is_load_text_strict_fail` (binary|encoding|invalid_input), `is_no_match`, \
               `is_ambiguous`; string peel: `api::error_kind_str` / one-shot `api::peel_error` \
               (kind + message + suggestion) for CLI-stable envelopes \
               (#1947 / #1948 / #1963 / #1964).\n\
             - New `EditErrorKind` variants must be **appended** (after the last variant) so \
               discriminants of published kinds stay stable under cargo-semver-checks (#1955).\n\
             - CLI: `patchloom undo --list` walks nested `.patchloom/backups` under the cwd (#1695). Bare CLI undo is dry-run (exit 2); restore needs the write apply flag (see CLI agent-rules).\n\
             - `api::run_post_write_validation` / `ReplaceOptions.post_write` / `WritePolicyOptions.post_write` (#1663, #1690) maps to `format_failed` / `EditErrorKind::FormatFailed`\n\
             - Multi-doc / wrong-root doc navigation peels to `EditErrorKind::TypeError` via \
               `edit_error_kind` / `classify_error` (CLI JSON `error_kind: type_error`; #1883). Do not \
               collapse with `InvalidInput` (empty patterns, bad options) or `Binary` / \
               `InvalidEncoding` for content SoftSkip (#1963).\n\
             - Multi-doc `api::doc_merge(path, value, mode, guard, selector)`: pass `Some(\"0\")` \
               (or `\"[0]\"`) to merge into document 0; `None` is root-only and refuses a non-array \
               overlay on multi-doc root with `TypeError` (#1909).\n\
             - Path binary preflight: `api::is_binary_file` / `files::is_binary_file` (8 KiB NUL probe; open fail → false; #1884)\n\
             - Sole-path text load for hosts: `api::load_text(path)` or `files::load_text_strict` (#1894, #1910)\n\
             - `EditErrorKind` is `#[non_exhaustive]`; match with a wildcard arm (#1910)\n\
             - Text I/O honesty (#1894):\n\
               | Surface | Binary / invalid UTF-8 | Unreadable (IO) |\n\
               |---------|------------------------|-----------------|\n\
               | Sole explicit path (`load_text_strict` / `sole_explicit_non_text`) | `binary` / `invalid_encoding` / `invalid_input` (also dangling/FIFO `not a file`) | IO / `not_found` |\n\
               | Explicit multi-file list | `refused[]` reason `binary`, `invalid_utf8`, or `not_regular_file` | `refused[]` reason `unreadable` |\n\
               | Directory walk (`try_read_text_file`) | content SoftSkip (silent) | SoftSkip; empty scan must not report pattern `no_matches` if unreadable may have masked it (AST rename) |\n\
               | Tx multi-path probe (`read_and_probe`) | SoftSkip `Ok(false)` | Hard `Err` (plan names paths) |\n\
               Byte rule: `classify_text_bytes`. Patch file + patch targets use Strict (#1896).\n\
             - Project rename: `api::ast_rename_project(root, old, new, &opts, guard)` (#1689)\n\
             - Fuzzy tip: bare-identifier typos use token span matching. Library agent hosts: `ReplaceOptions::for_agent()` uses `AGENT_MIN_FUZZY_SCORE` (0.90). CLI examples may use 0.80 as a looser floor. Always check `matched_text` (#1687, #1694, #1736, #1965)\n\
             **Match reporting in JSON:** CLI `replace --json`, MCP `replace_text` / `batch_replace` / `execute_plan`, and library `EditResult` report `match_mode` (`exact`/`fuzzy`/`anchored`), optional `match_score`, optional `matched_text` (actual span for fuzzy/anchored; may differ from `old`), and replace `match_count` (plan/tx also on each change + sum) so agents can verify fuzzy sites. Multi-file / multi-op aggregates use worst-case rollup (`fuzzy` > `anchored` > `exact`) and the **minimum** fuzzy `match_score` across paths/ops (lowest confidence). Soft no-match / fuzzy fail-closed refuse sets `ok: false` and `error_kind: no_matches` on CLI, plan/tx, and MCP (body and `is_error` agree; #1791). When some paths write and others soft-refuse or soft-miss, overall ok/success may still be true: check `refused[]` (path, match_mode, match_score, matched_text, reason=`exact_old_absent`, `below_min_fuzzy_score`, or `no_matches` for exact soft miss on explicit multi-path lists; #1792) so partial apply is not mistaken for full coverage. Soft no-match CLI JSON may include `similar_targets` (did-you-mean) for literal patterns (#1669, #1674, #1736, #1747).\n\
             **Missing paths CLI vs MCP (#1793):** CLI multi-path `replace` soft-skips missing explicit paths under `skipped[]` and still applies the rest. Plan/batch path ops hard-fail missing files with `not_found` and roll back (atomic) unless `if_exists` / `--if-exists` is set, which soft-skips the missing path like CLI. Soft zero-match on existing paths still applies matches and lists `refused[]` with overall success. Under `--json`/`--jsonl`, CLI does not also print a human stderr \"No such file\" line for paths already in `skipped[]` (#1797).\n\
             **Empty `--files-from`:** An empty list file or empty stdin (`--files-from -`) is `error_kind: invalid_input` (exit 1) for **search**, **replace**, and **tidy** (not pattern `no_matches`, and not tidy `ok:true` with zero issues). Fix the path list; do not widen the pattern or treat the workspace as clean (#1796).\n\
             **`--files-from` comments (#1811):** Lines whose first non-whitespace character is `#` are comments and are ignored (gitignore-style). Blank lines are ignored. Do not emit markdown headers as path lines.\n\
             **Do not branch on `ok` alone (#1804):** Prefer `applied: true` (CLI write JSON) or MCP/tx `files_changed > 0` with `status: success` for \"edit landed.\" Soft refuse / no match uses `error_kind`/`status: no_matches` (and `applied: false` / `files_changed: 0`). Partial multi-path: read `refused[]` / `skipped[]`. Format failure: `error_kind: format_failed` with `applied: true` means the write already landed on disk (also `write_applied` for one release; prefer `applied`; #1831).\n\
             **`applied: false` with no write:** When a write was requested but no bytes change (doc ensure/set identity, empty append/prepend, identity replace), JSON still sets `applied: false` and omits `backup_session`. Do not treat success alone as a write. Pre-write hard failures (`already_exists`, `not_found`, `invalid_input`, `guard_rejected`, `parse_error`, `type_error`, `conflicts`, `ambiguous`, `no_matches`, `changes_detected`) also set `applied: false` so agents can branch on `applied` alone.\n\
             **Batch `replace` order:** Batch lines are `replace PATH OLD NEW` (not CLI `replace OLD --new NEW path`). CLI flag `--new` (and plan-shaped `--from`/`--to`) is rejected with a PATH OLD NEW hint; path-last positionals also fail with a parse hint when the third token is an existing file.\n\
             **`identity: true` (#1801):** Replace JSON when the pattern matched but every replacement was identical (`old == new`); `match_count > 0` but `file_count: 0` / empty `files` and no write.\n\
             **`require_change` + `if_exists` (#1800):** When both are set, `if_exists` wins: zero matches → success (`ok: true`, `match_count: 0`), not fail-closed `no_matches`.\n\
             **`tx`/`execute_plan` `strict: false` (#1803):** Continues past soft replace `no_matches` on existing paths. Does **not** continue past hard errors such as `not_found` (missing path). Optional **files** must be omitted from the plan or ensured to exist; optional **content** can use soft miss.\n\
             **`for_each` (plan-level field, not an op; #1842):** Fan-out is a top-level plan field with required `glob`. Example:\n\
             ```json\n\
             {\n\
               \"for_each\": { \"glob\": \"**/*.txt\" },\n\
               \"operations\": [\n\
                 { \"op\": \"replace\", \"path\": \"{path}\", \"old\": \"x\", \"new\": \"y\" }\n\
               ]\n\
             }\n\
             ```\n\
             Placeholders: `{path}`, `{item}` (alias of `{path}`), `{dir}`, `{stem}`, `{ext}`, `{name}`. \
Unknown lone `{name}` in path/from/to fields fails with `invalid_input` (not opaque `not_found`). \
Do not put `for_each` inside `operations` and do not pass a bare path array.\n\
             **Binary / non-text / unreadable paths (#1813 / SoftTextSkip / #1963):** Sole explicit **binary** path is `error_kind: binary`; sole **invalid UTF-8** is `invalid_encoding`; sole **unreadable** IO is `invalid_input` (not pattern `no_matches` / not vacuous tidy clean / not unsupported-language / not missing-symbol). This includes a **sole path supplied only via file-backed `--files-from`** (not stdin `-`). Multi-file **replace**, **search**, and **tidy** lists (positionals or `--files-from`) put non-text co-paths in `refused[]` with `reason: binary`, `invalid_utf8`, `not_regular_file` (FIFO/socket/device/dir), or `unreadable` (permission/IO; do not treat a partial success as covering every listed path). Multi-file read reports binary paths under `skipped[]` with top-level `error_kind: binary` (or `invalid_encoding` when all failures are encoding). Directory walks soft-skip content SoftSkip (binary/utf8) without failing the whole search. If a walk finds **zero** matches/issues and **any** path was unreadable, the result is `invalid_input` (not pattern `no_matches` / not vacuous tidy clean / not `assert-count` success with actual 0 / not AST “no symbols”), because IO may have masked the scan.\n\
             **Search max_results:** CLI/MCP/tx `search` with `max_results` keeps full `match_count` (and full `file_count`) but may cap the detailed `matches` array **and** the `files` list in `--count` / `--files-with-matches` modes; when capped, JSON sets `truncated: true` (omitted when complete; #1798). Under `--jsonl`, capped content searches append a final `{\"type\":\"summary\",\"match_count\":N,\"match_emitted\":M,\"truncated\":true}` line so agents still see the total. Do not treat `matches.len()` or `files.len()` as total coverage when `truncated` is true.\n\n\
             **Multi-result `--json`:** Commands that emit many items (`ast list` / `ast search` / `ast validate` / \
             `ast deps`, `ast map`, undo list, etc.) print one JSON array under `--json` \
             (single `json.loads` on full stdout) and one object per line under `--jsonl`. Do not expect \
             concatenated pretty multi-documents for `--json`.\n\
             **Diagnostic envelopes:** `md lint-agents --json` and MCP `md_lint` emit a single object \
             `{ok, path, issue_count, issues}` (not a bare array). `tidy check --json` is the multi-file \
             sibling: `{ok, issue_count, issues}` with `path` on each issue (optional `skipped[]`), not a \
             top-level path. When issues exist: `ok: false`, exit 2, and `error_kind`/`status` \
             `changes_detected` (same kind as search `--assert-count` mismatch). Branch on `error_kind` \
             or `issue_count`. MCP `md_lint` keeps isError=false when issues \
             are present (same as clean file success with `ok: true`). CLI JSONL stays one issue object per \
             line. (#1854, #1859)\n\n",
        );
    }
    if show_cli {
        out.push_str(
            "**Preview vs apply (#1808, #1810, #1812):** CLI write JSON always includes `applied` \
(`true` after apply mode, `false` for default preview / check mode). Plan/batch/tx JSON also \
includes `applied` (same meaning; pair with `status`: `success` vs `changes_detected`). \
`changed: true` or `files_changed: N` on preview means **would** change, not that bytes were \
written. Exit 2 = changes detected / dry-run.\n\
             **`backup_session` on success (#1802):** Successful CLI replace/doc/tx apply JSON \
includes `backup_session` when a backup was created (same field as `format_failed` and library \
`EditResult`). Use it for surgical undo; do not guess newest session under parallel agents.\n\
             **CLI vs plan/MCP names (#1809):** CLI replace is positional `OLD` + `--new NEW` \
(not plan aliases `from`/`to` as flags). CLI `doc set` is `doc set PATH SELECTOR VALUE` \
(not a `--key` flag). Plan/MCP accept legacy aliases `from`/`to` and `key` for selector.\n\
             **CLI `md upsert-bullet`:** use `--bullet` (or alias `--content`; #1839) with `--heading`.\n\
             **Doc query `--json` (#1838):** `doc get`/`has`/`keys`/`len`/`select`/`flatten` success is \
`{\"ok\":true,\"value\":...,\"path\":...,\"selector\":...}` (selector omitted for flatten). Text mode stays bare. \
`doc has` prints `true`/`false` and exits **0** for both (missing key is not `no_matches`; #1843).\n\
             **Plan/batch `tidy.fix` defaults (#1840, #1847):** Omitting write-policy fields matches CLI \
`tidy fix` (trim trailing whitespace + ensure final newline). Precedence: defaults → plan \
`write_policy` → op fields. Op fields stick through commit (plan `write_policy` is not re-applied \
to that path); a later non-tidy write clears that. Bare example: `{\"op\":\"tidy.fix\",\"path\":\"f.txt\"}`.\n\
             **Replace jsonl multi-file (#1799):** Streams one object per success path \
(`status: ok`), refused soft-miss (`status: refused`), skipped missing (`status: skipped`), \
then a `type: summary` trailer with counts. Prefer this or MCP `batch_replace` over assuming \
success lines are the full path list.\n\n\
             Use patchloom when:\n\
             - Editing JSON, YAML, or TOML (parser-backed, preserves comments, output is always valid)\n\
             - Editing markdown sections, bullets, or tables by heading\n\
             - Batching edits across multiple files in one call\n\
             - You need atomic rollback if any edit fails\n\n\
             **CLI undo (dry-run by default):** `patchloom undo` only previews the most recent \
             backup and exits 2 with `applied: false` and `status: changes_detected`. Restore with \
             `patchloom undo --apply` (optional `--session <timestamp>`), which sets `applied: true` \
             and `status: restored` (#1830). List sessions with `patchloom undo --list`. There is no \
             `--latest` flag.\n\
             **Replace mode flags (#1829):** CLI replacement text is `--new` (not positional NEW and not \
             `--to`). Missing mode errors name `--new` / `--insert-before` / `--insert-after` first; \
             plan/MCP still accept field aliases `new`/`to`.\n\
             **Insert line placement (#1885):** `--insert-before` / `--insert-after` (and plan/MCP \
             `insert_before` / `insert_after`) are line-oriented by default: payloads that look like a \
             new line (indent, `//`/`#` comment, embedded newline) or a whole-line anchor get a separating \
             newline so agents do not glue text onto the match. Pure mid-line inserts (e.g. `X` after \
             `foo` inside a line) stay byte-exact.\n\
             Example (anchor is positional OLD; insert text is on the flag; path last; never \
             `replace ANCHOR NEW path`):\n\
             `patchloom replace 'use std::io;' --insert-after 'use std::fs;' src/main.rs --apply`\n\n",
        );
        if !show_mcp {
            out.push_str(
                "For single-file read, search, create, delete, or rename, your native agent tools are faster.\n\n",
            );
        }
    }
}
