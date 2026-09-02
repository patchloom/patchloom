//! Hand-written **reference** tables (selectors, exit codes).
//!
//! Complements schema inventory with CLI exit/`error_kind` contracts.

/// Selector path syntax and CLI exit code / `error_kind` tables.
pub(crate) fn append_reference(out: &mut String, show_cli: bool) {
    // Selector path syntax (always shown — used by all doc.* operations)
    out.push_str(
        "## Selector path syntax\n\n\
         All `doc` operations use selector paths to address values inside JSON, YAML, and TOML files.\n\n\
         | Syntax | Meaning | Example |\n\
         |--------|---------|---------|\n\
         | `name` | Object key | `database.host` |\n\
         | `.` | Document root | `doc keys FILE .`; `doc set FILE . VALUE` replaces the whole document |\n\
         | `[N]` | Array index (zero-based) | `servers[0].port` |\n\
         | `[*]` | Wildcard (all array elements) | `jobs[*].timeout` |\n\
         | `[key=val]` | Equality predicate (filter by field value) | `deps[name=express].version` |\n\
         | `[key!=val]` | Not-equal; missing fields do not match | `items[status!=disabled]` |\n\
         | `[key>N]` `[key>=N]` `[key<N]` `[key<=N]` | Numeric compare (`f64`; JSON number or numeric string) | `servers[port>8000]` |\n\
         | `[!key]` | Key absent, JSON `false`, or `null` | `flags[!deprecated]` |\n\n\
         Equality is unchanged: `items[url=a=b]` and `items[url=a>b]` stay `key=value`. \
         Comparison operands must be numeric (`[port>abc]` is `invalid_input`). A present \
         non-numeric field vs `>` / `>=` / `<` / `<=` is also `invalid_input` (not a string \
         compare). Regex predicates are not supported. \
         Segments are separated by `.` or adjacent brackets. A single leading `/` is treated as \
\"from root\" and stripped (JSON Pointer habit), so `/feature_flag` sets key `feature_flag`, not a key literally named `/feature_flag` (#1794). Only one leading slash is special; `//a` keeps a key named `/a`. Prefer bare keys in prompts (`feature_flag`, `server.port`). A lone `.` (or empty / `/`) is the document root: `doc keys FILE .` lists top-level keys, and `doc set FILE . VALUE` replaces the whole document. Examples:\n\n\
         ```text\n\
         .                               # document root (keys / whole-document set)\n\
         scripts.test                    # simple selector path\n\
         /feature_flag                   # same as feature_flag (leading slash stripped)\n\
         jobs[0].steps[*].name           # index + wildcard\n\
         dependencies[name=react].version # predicate filter\n\
         data[type=server][port>8000]    # chained equality + numeric compare\n\
         flags[!deprecated]              # absent, false, or null\n\
         ```\n\n\
         **Write ops and predicates:** `doc set` / `doc ensure` / `doc delete` / `doc move` are \
         **single-path only** (keys and indexes such as `items.0.val`). Wildcards and predicates \
         (`items[id=b].val`, `items[*].enabled`) belong on `doc update` (multi-match write) or \
         `doc delete-where` (array filter). If you pass a predicate to `doc set`, the error points \
         you at `doc update` or an index path. With `--json` / plan / MCP, that fail-closed path \
         also sets machine-stable `suggested_op` (`\"doc.update\"` for set/ensure multi-match; \
         `\"doc.delete_where\"` for predicate delete). Intermediate parent predicates \
         (`items[id=a].val` on delete) use the same mapping as leaf predicates (#2138). Hosts can \
         branch without scraping English (`api::suggested_op_from_error` on the library path).\n\n\
         **Multi-document YAML:** Files with multiple `---` documents parse as a **top-level array** \
         (one element per document). Address a field with a document index first, e.g. `0.metadata.name` \
         or `[0].metadata.name`, not a bare key at the stream root. A bare root key on multi-doc \
         (or any top-level array) fails with `error_kind: type_error` and an index-form hint on \
         `doc get`/`select`/`has` and `doc set` (not soft `no_matches` / not soft `has: false`). \
         `doc keys .` on multi-doc root is also `type_error` (list keys on `0` / `[0]` first). \
         `doc merge` of any overlay into multi-doc **root** is `type_error` (object *or* array \
         overlay would replace the whole stream). Merge into one document with \
         `--selector 0` / plan `\"selector\": \"0\"` (or `doc.set` under `0.`). Bare-key `doc append`/`prepend`/`delete`/`update`/`move`/`ensure` on multi-doc \
         root are also `type_error` with `0.key` / `[0].key` hints (not soft no-match or \
         `invalid_input`). Address fields under `0.` / `[0].` instead. Writes keep the multi-doc \
         form (still `---` separators, not a single YAML sequence).\n\n\
         **YAML anchors:** `doc set` / `doc.update` keep `&name`, `*name`, and `<<: *name` when \
         the edit is a sibling field or a local override beside an existing merge. An interior \
         edit of a pure mapping alias (`service_a: *shared` or a list item `- *shared`) becomes \
         `<<: *shared` plus the local key. Deleting an inherited key, or replacing the alias \
         with a map that does not keep every inherited key, writes a concrete map for that \
         site only.\n\n\
         **Markdown section bounds:** `md_replace_section` / `md_insert_after_section` / section moves \
         end at the next heading of the **same or higher** level. Nested lower-level headings belong to \
         the parent (replacing `# Intro` also rewrites following `##` children until the next `#`). Prefer \
         peer-level headings when siblings must survive. `md_dedupe_headings` removes later **whole \
         sections** with a duplicate level+text heading (body under the second heading is discarded, not \
         merged).\n\
         **Markdown insert placement:** `md_insert_after_heading` inserts **under the heading line** \
         (before existing body). To add a sibling `##` section after the full section body, use \
         `md_insert_after_section`.\n\n",
    );

    // Exit codes (CLI only — MCP tools return results as JSON, not exit codes)
    if show_cli {
        out.push_str(
            "## Exit codes\n\n\
             | Code | Meaning |\n\
             |------|---------|\n\
             | 0 | Success (operation completed, or no changes needed) |\n\
             | 1 | Failure (error during execution, CLI usage/invalid args/unknown flags/subcommands), or tx `rollback_failed` when mid-commit rollback could not fully restore files |\n\
             | 2 | Changes detected (`--check` / write preview including `undo` without `--apply`, or CLI/MCP/plan/tx `search` `assert_count` mismatch; `error_kind: changes_detected` for assert_count). For undo, re-run with `--apply` to restore. |\n\
             | 3 | No matches (search/replace pattern miss, undo with no sessions, missing AST symbol for extract/insert/reorder/wrap/move, or tx/plan AST/md/doc target not found; `error_kind: no_matches`) |\n\
             | 4 | Parse error (malformed input file or plan, invalid AST search pattern/query, or AST validate parse failure; `error_kind: parse_error`) |\n\
             | 5 | Ambiguous (CLI/tx `unique` multi-match, `doc keys`/`len` / MCP `doc_query` keys/len multi-match, or stale/missing patch context; `error_kind: ambiguous` in CLI/tx JSON) |\n\
             | 6 | Validation failed (tx plan validation step returned non-zero; writes may remain when not strict) |\n\
             | 7 | Rollback (tx mid-commit failure or strict lifecycle failure; changes were rolled back) |\n\
             | 8 | Patch merge conflicts (CLI `patch merge` or plan/tx `patch.apply` with `on_stale: merge` without `allow_conflicts`; `error_kind: conflicts`) |\n\
             | 9 | Tx operation staging failure (`operation_failed`) |\n\n\
             **JSON `error_kind` (exit 1):** Prefer branching on kind, not English text. File ops set \
             `already_exists` (create/rename without force, create race), `not_found` (delete/append/prepend/rename missing source, \
             missing `--files-from`/batch input, missing plan file, missing git blob for AST, \
             `read` when every path fails, or library/CLI/MCP `doc get`/`has`/`keys`/`len` / `doc_query` on a missing file), \
             `invalid_input` (bad flags, non-file target, malformed `read --lines` \
             (0-based or end before start; past-EOF range is `no_matches` exit 3), replace `--nth` past the last \
             match with live match count in the message, \
             `status` outside a git repo, AST map non-dir, doc merge flag conflicts, invalid `normalize_eol`, \
             md table-append row/table failures, bad selector/for_each templates, MCP bind/TLS config, \
             CLI usage errors under `--json`/`--jsonl`, empty path arguments, \
             all-explicit-paths-missing for search/replace/tidy, \
             invalid search/replace regex patterns (unclosed groups, etc.), \
             and plan op option conflicts such as replace whole_line+multiline, tidy dedent+indent, \
             md.move_section before/after, search invert_match+multiline), \
             `guard_rejected` (PathGuard / `--contain` path escape or plan cwd escape; #1935; not empty-path `invalid_input`), \
             `format_failed` (post-write `--format` command non-zero exit or format-timeout; write may already be on disk; JSON includes `backup_session` when a session was created so agents can `patchloom undo --session <id>`, plus `applied: true` (canonical; #1831), `write_applied: true` (deprecated alias), `files_changed`, and `files[].path` for every path already written so agents need not re-scan disk (#1795); re-run the formatter or undo). Empty `--files-from` is `invalid_input` (not `no_matches`; #1796). Doc type mismatches set \
             `type_error` (`doc keys`/`len` on wrong type, library doc mutation type errors). \
             Clap usage failures with `--json`/`--jsonl` emit the same envelope on stdout before any subcommand runs.\n\n\
             **JSON `error_kind` (exit 4):** Batch line parse failures, `explain` plan parse failures, \
             invalid AST search patterns/queries, AST validate parse failures, and \
             malformed JSON/YAML/TOML document content set `parse_error` so agents can distinguish syntax \
             mistakes from runtime failures.\n\n\
             **Doc write JSON tip:** With `--json` (CLI) or MCP write tools / `execute_plan`, \
             doc delete success includes `changed` (bool) and `removed` (usize). Multi-op \
             plans also list per-op rows under `mutations`. Exit 0 / `ok: true` with \
             `removed: 0` means idempotent cleanup (nothing matched); do not assume data \
             was deleted from exit status alone.\n\n",
        );
    }
}
