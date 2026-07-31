//! Hand-written **workflow examples** and AST CLI shapes.
//!
//! Shell recipes for common agent tasks; not schema inventory.

/// CLI workflow examples and (when enabled) AST-aware operation examples.
pub(crate) fn append_workflows(
    out: &mut String,
    show_cli: bool,
    show_linux: bool,
    _show_windows: bool,
) {
    // Workflow examples (CLI only)
    if show_cli {
        out.push_str("## Workflow examples\n\n");

        out.push_str(
            "### Rename a function across a codebase\n\n\
             ```bash\n\
             # Prefer AST-aware rename for code identifiers (skips strings/comments)\n\
             patchloom ast rename src/ --old old_function_name --new new_function_name --apply\n\n\
             # Fallback: text-based workflow when AST mode is unavailable\n\
             patchloom search --count \"old_function_name\" src/\n\
             patchloom replace \"old_function_name\" --new \"new_function_name\" src/ --apply\n\
             ```\n\n\
             Default: `replace --fuzzy` with a misspelled `old` that is **not** in the file \
             refuses the write (#1758). Opt in only with `--allow-absent-old` for deliberate \
             approximate recovery, then check `matched_text` (#1736).\n\n",
        );

        out.push_str(
            "### Insert a line after an anchor\n\n\
             ```bash\n\
             # OLD is the preserved anchor; payload goes on --insert-after (not positional NEW)\n\
             patchloom replace 'use std::io;' --insert-after 'use std::fs;' src/main.rs --apply\n\
             # Same idea before a match:\n\
             patchloom replace 'fn main() {' --insert-before '// entry point' src/main.rs --apply\n\
             ```\n\n\
             Whole-line anchors and line-like payloads get a separating newline by default (#1885).\n\n",
        );

        out.push_str(
            "### Delete lines matching a pattern\n\n\
             ```bash\n\
             # Delete entire lines containing a pattern; collapse consecutive blanks\n\
             patchloom replace 'dbg!' --whole-line --new '' src/ --collapse-blanks --apply\n\n\
             # Restrict to a line range (e.g. implementation only, skip tests)\n\
             patchloom replace 'TODO' --whole-line --range 10:200 --new '' notes.md --apply\n\n\
             # Rewrite shell invocable tokens only (not uv pip / pipenv):\n\
             patchloom replace pip --new uv install.sh --command-position --require-change --apply\n\
             patchloom replace 'fn proccess_data' --new 'fn process_data' src/ --fuzzy --allow-absent-old --apply\n\
             patchloom replace 'fn proccess_data' --new 'fn process_data' src/ --fuzzy --allow-absent-old --min-fuzzy-score 0.80 --apply\n\
             ```\n\n",
        );

        out.push_str(
            "### Edit a CI workflow\n\n\
             ```bash\n\
             # Set a value in a YAML workflow by selector path (preserves comments and formatting)\n\
             patchloom doc set .github/workflows/ci.yml jobs.test.timeout-minutes 30 --apply\n\
             ```\n\n",
        );

        out.push_str(
            "### Stale patch recovery via three-way merge\n\n\
             ```bash\n\
             # Apply a patch using three-way merge when context has drifted\n\
             patchloom patch apply changes.patch --on-stale merge --apply\n\
             \n\
             # Or use the merge subcommand directly\n\
             patchloom patch merge changes.patch --apply\n\
             \n\
             # If merge produces conflicts, allow them to be written as markers\n\
             # WARNING: never commit files containing conflict markers\n\
             patchloom patch merge changes.patch --apply --allow-conflicts\n\
             \n\
             # Diff from stdin (--stdin or a bare '-' path both work)\n\
             cat changes.patch | patchloom patch apply --stdin --apply\n\
             cat changes.patch | patchloom patch apply - --apply\n\
             ```\n\n",
        );

        if show_linux {
            out.push_str(
                "```bash\n\
                 # Same via transaction plan (JSON):\n\
                 patchloom tx - --apply <<'EOF'\n\
                 {\"version\": 1, \"operations\": [\n\
                   {\"op\": \"patch.apply\", \"diff\": \"...\", \"on_stale\": \"merge\", \"allow_conflicts\": true}\n\
                 ]}\n\
                 EOF\n\
                 ```\n\n",
            );
        }

        if show_linux {
            out.push_str(
                "### Bump a version across config files\n\n\
                 ```bash\n\
                 patchloom batch --apply <<'EOF'\n\
                 doc.set package.json version \"2.0.0\"\n\
                 doc.set Cargo.toml package.version \"2.0.0\"\n\
                 replace README.md \"1.0.0\" \"2.0.0\"\n\
                 md.upsert_bullet CHANGELOG.md \"## [2.0.0]\" \"- Initial 2.0 release\"\n\
                 EOF\n\
                 ```\n\n",
            );

            out.push_str("### Multi-file refactoring with a transaction\n\n\
                 ```bash\n\
                 patchloom tx - --apply <<'EOF'\n\
                 {\"version\": 1, \"operations\": [\n\
                   {\"op\": \"replace\", \"path\": \"src/config.rs\", \"old\": \"old_default\", \"new\": \"new_default\"},\n\
                   {\"op\": \"doc.set\", \"path\": \"config.toml\", \"selector\": \"default_value\", \"value\": \"new_default\"},\n\
                   {\"op\": \"md.replace_section\", \"path\": \"docs/config.md\", \"heading\": \"## Defaults\",\n\
                    \"content\": \"The default value is now `new_default`.\\n\"}\n\
                 ]}\n\
                 EOF\n\
                 ```\n\n\
                 All operations succeed atomically or roll back together.\n\n\
                 Plan/MCP `replace` accepts library flags (default false):\n\
                 - `require_change`: fail when the pattern matches zero times (agent fail-closed).\n\
                 - `command_position`: rewrite only shell invocable tokens (`sudo`/`timeout`/`flock`/`runuser`/`setsid`/`run0`/`gosu`/`su-exec`/`tini`/`dumb-init`/`unshare`/`nsenter`/`taskset`/`systemd-run`/`firejail`/`busybox`/`chpst`/`softlimit`/`envdir`/`setlock` wrappers yes; `uv pip` no).\n\
                 - `fuzzy`: similarity fallback when exact match fails (also with before_context/after_context).\n\
                 Example: `{\"op\":\"replace\",\"path\":\"install.sh\",\"old\":\"pip\",\"new\":\"uv\",\
                 \"command_position\":true,\"require_change\":true}`\n\
                 Successful plan/tx and `batch_replace` JSON includes `match_mode` (`exact`/`fuzzy`/`anchored`), optional `matched_text` for fuzzy/anchored spans, and `match_count` on replace-backed changes plus worst-case aggregate mode and sum of counts when any replace matched (#1674, #1736). Partial soft-refuses list `refused[]` with reason (`exact_old_absent` / `below_min_fuzzy_score` / `no_matches`); do not treat ok/success alone as full multi-path coverage. Tx search with `max_results` may set `truncated: true` on search results when the matches array was capped.\n\n");
        }
    }

    // AST-aware operations (always shown when AST feature is enabled)
    #[cfg(feature = "ast")]
    if show_cli {
        out.push_str(
            "## AST-aware operations\n\n\
             Tree-sitter-backed operations that understand code structure (20 languages).\n\n\
             ```bash\n\
             # Rename an identifier across a file, skipping strings and comments\n\
             # (path first, then --old / --new)\n\
             patchloom ast rename src/lib.rs --old OldName --new NewName --apply\n\n\
             # Replace text only within a specific function body\n\
             # (PATH then SYMBOL are positionals — no --symbol flag; #1841)\n\
             patchloom ast replace src/config.rs default_timeout --old 30 --new 60 --apply\n\n\
             # List all symbol definitions\n\
             patchloom ast list src/lib.rs\n\n\
             # Find all references to a symbol\n\
             # (SYMBOL then PATH — order differs from replace/read; #1841)\n\
             patchloom ast refs my_function src/\n\
             # impact is also SYMBOL then PATH: ast impact my_function src/\n\
             ```\n\n\
             **AST CLI arg order:** `rename`/`replace`/`read`/`validate` use path-first; \
`refs`/`impact` use symbol-first. There is no `--symbol` or `--name` flag on these commands.\n\n\
             AST rename, replace, and rewrite_signature can also be used in batch and tx plans:\n\n\
             ```bash\n\
             # In a batch file (path, then old, then new — same names as CLI flags):\n\
             ast.rename src/lib.rs OldStruct NewStruct\n\
             ast.replace src/config.rs default_timeout \"30\" \"60\"\n\
             # path old parameters [return_type]:\n\
             ast.rewrite_signature src/lib.rs process \"(x: u64)\" \"-> u64\"\n\
             ```\n\n",
        );
    }
}
