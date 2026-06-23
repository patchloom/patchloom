# Implementation Plan: Full CLI / MCP / tx plan parity for Bline library search features (#821)

**Date:** 2026-06-23  
**Related:** #821 (this issue), #792, #811–#815 (Bline library), PRs #816/#817/#818, #796 (search ignore), #801 (guard/policy)  
**Goal:** Make the powerful search ignore layering, `SearchOptions`, `collect_file_paths_with_ignores`, `max_results`, rich results, etc. (added for pure-library Bline use) fully available and parity-correct from the CLI `search` command, `tx` plan `Search` operations, and the MCP `search_files` tool — with no gaps in functionality, tests, schema, or docs. Other secondary items (AST sig, WritePolicy on direct api) get explicit decisions recorded.

Everything must pass the full `make check` (incl. library hygiene under `ast,files`), cross-surface parity, and the acceptance criteria listed verbatim in #821.

## Success Criteria (verbatim from #821 + expanded for zero-gap)

From the issue body:

- [ ] CLI `search` supports expressing the full `SearchOptions` (or the important new parts: custom ignores, excludes, max results) with good UX and help text.
- [ ] `patchloom tx` (JSON/YAML/TOML plans) and the plan `Search` variant support the new ignore/max options; execution uses the shared collection helper.
- [ ] MCP `search_files` tool schema + implementation support the new options (and pass them through).
- [ ] `src/schema.rs` search entry (and any related) is complete and matches.
- [ ] `execute_search_op` and CLI search collection are unified (or clearly share the precedence logic from `collect_file_paths_with_ignores`).
- [ ] Cross-surface parity tests (same inputs → equivalent rich results or counts, including .blineignore + exclude + glob cases).
- [ ] No regression for existing CLI/MCP behavior or exit codes.
- [ ] Docs + examples updated (embedding section, reference, schema output).
- [ ] `make check` (incl. check-fast, library hygiene, integration, pty) + `cargo test --no-default-features --features "ast,files"`.
- [ ] Decision recorded (and implemented or explicitly deferred with issue) for AST signature rewrite exposure.
- [ ] Optional: direct api mutating fns gain WritePolicy control (or documented as "use a 1-op plan or the cmd helpers for full policy").
- [ ] Issue body updated with implementation PR links when landed.

Additional zero-gap requirements:
- GlobalFlags extended so `--exclude` and `--ignore-file` are available (like `--glob`), benefiting search + replace + tidy.
- `literal` supported in plan Search (parity).
- `max_results` interacts reasonably with count/assert modes (documented + tested).
- All new fields appear in `patchloom schema` output and agent-rules prompts for the search op.
- MCP description string and its test assertion updated.
- Help text and after_help examples demonstrate the new flags.
- Existing .blineignore tests in api continue to pass; new parity tests added.
- No new dead_code under library-only build.
- Full round-trip: plan containing new search fields roundtrips via serde, executes, and produces correct TxSearch* in report.

Non-goals (per issue):
- Do not change (code, JSON) contract for CLI/MCP tx execution.
- Do not force full formatter unification (CLI keeps its rich colored/jsonl/etc. output).
- Do not add brand new top-level commands.

## Current State (re-verified 2026-06-23)

**Library (complete, behind `any(cli,files)`):**
- `api::SearchOptions` (literal, regex, case, context, globs, max_results, exclude_patterns, custom_ignore_filenames).
- `api::search_directory`, `search_file` (delegates), `search_one_file`, `format_search_results(results, as_json)`, `build_context_lines`.
- `files::collect_file_paths_with_ignores` (adds custom ignores to WalkBuilder + post-retain excludes).
- `par_process_files` + `build_glob_matcher` used for include globs after collection.
- Tests in `src/api.rs` cover globs, max_results, exclude+custom_ignore (blineignore), errors.
- Docs in api.rs + lib.rs show usage and precedence.

**CLI search:**
- `SearchArgs` supports literal, regex, all context, files_with_matches, count, invert, multiline, case, assert_count.
- `--glob` (via GlobalFlags) + `collect_file_paths_opts` (parallel WalkBuilder + .gitignore + hidden + files-from) + post glob filter in par_process.
- No exclude, no ignore_file, no max_results.
- Internal types + rich `format_results` (color, -- separators, jsonl, special count modes).
- See `src/cmd/search.rs`, `src/files.rs:67` (opts), `src/cli/global.rs:52`.

**tx plan Search + execution:**
- `plan::Operation::Search { path, pattern, regex, case_insensitive, multiline, invert_match, context, before/after, assert_count }`.
- No literal, globs, max, exclude, custom_ignore.
- `tx::execute_search_op` does its own basic `WalkBuilder` (no custom), manual re build (escape iff !regex), populates pending, builds TxSearchMatch (with special multi-file text), applies assert_count, records to report.searches.
- Schema in `src/schema.rs` is minimal (missing before/after, globs etc.).
- Examples in plan.rs and schema tests use basic form.

**MCP search_files:**
- `SearchParams` mirrors most of SearchArgs (incl. literal, before/after, count/files_with, assert).
- Builds fake GlobalFlags (only json+cwd) + SearchArgs, calls collect_matches + format_results (forces json).
- Tool description lists old options only.
- No support for advanced ignore/max/globs yet.
- Hardcoded description asserted in `mcp_lists_expected_tools`.

**Cross cutting:**
- `par_process_files` and glob logic already pub under cfg.
- Tx search and CLI search duplicate some matching logic (acceptable for now).
- No .blineignore or exclude support outside library search helpers.
- GlobalFlags already has the pattern for repeatable globs.

## Design Decisions

1. **GlobalFlags for new ignore controls:** Add `exclude: Vec<String>` and `ignore_file: Vec<String>` to GlobalFlags (repeatable --exclude, --ignore-file). This is consistent with --glob and automatically gives the power to `search`, `replace`, and `tidy` (nice side-effect, no extra work). Library `SearchOptions` keeps its names for json ergonomics.

2. **Collection unification:** Enhance the existing `collect_file_paths_opts` (cli) to:
   - Call `builder.add_custom_ignore_filename` for every entry in global.ignore_file (before parallel build).
   - After collecting paths, apply post-retain exclude globset if global.exclude non-empty.
   This makes the walker precedence identical to `collect_file_paths_with_ignores` + the include glob filter that already happens in callers' par_process.
   Keep `collect_file_paths_with_ignores` as the clean public primitive for library.

3. **max_results:** Add to `SearchArgs` as `#[arg(long, default_value_t = 0)] pub max_results: usize;` (0 = unlimited; required for clap not to make the flag mandatory and to keep all existing invocations working). Apply truncate to the detailed `matches` vec (after count computation and after sort for deterministic "first N", before format) when >0 and not count/files_with mode. For assert_count path use the full collection (assert sees pre-limit counts). Document that it primarily limits detailed match output (matching library `search_directory` behavior). In plan Search it will limit recorded TxSearchMatches; the resulting report's per-search `match_count` reflects the (capped) recorded matches while CLI JSON in count mode can preserve full file_match_counts. Clarify the exact semantics in tests and docs.

4. **Plan Search evolution:** Add to the struct (with serde default):
   - `literal: bool`
   - `globs: Vec<String>`
   - `max_results: usize`
   - `exclude_patterns: Vec<String>`
   - `custom_ignore_filenames: Vec<String>`
   Add validation (literal && regex conflict) in **both** `validate_operation` locations (`src/tx.rs` and the one in `src/cmd/tx.rs`) plus an early bail inside `execute_search_op`.
   In `execute_search_op` use the shared `collect_file_paths_with_ignores` (when files feature), `build_glob_matcher`, glob filter (via `matches_glob_with_roots` or equivalent), and cap. Preserve every line of pending population, binary probe, TxSearchMatch construction (incl. multi-file text), context calc, assert_count, and report recording. Sort full matches before applying max_results cap for determinism. Update `declared_paths` (it already uses `..` pattern; verify).
   Explicitly state that `match_count` in the resulting TxSearchResult for a search op will reflect the (possibly capped) recorded matches.

5. **MCP:** Extend `SearchParams` with the four advanced fields + globs + max_results + literal (for completeness). When building GlobalFlags, populate glob/exclude/ignore_file. When building SearchArgs populate max_results (and literal already there). This makes MCP support the full power (and incidentally adds glob support that was previously missing for the tool). Update the `#[tool(description)]` and the exact string asserted in the test.

6. **Schema:** Expand the "search" `OperationSchema` properties to be complete (add literal, globs, before_context, after_context, max_results, exclude_patterns, custom_ignore_filenames). Improve description. This flows to `patchloom schema`, agent-rules, and system prompts automatically.

7. **AST signature rewrite (`rewrite_function_signature` / `FunctionSigEdit`):** Decision: keep library-only for this release. It is specialized (Rust only today), already advertised in embedding docs. Full promotion (`ast signature` subcommand + plan op + MCP tool + guard/diff/apply support) would be nice but is out of scope for #821 parity focus. Record the decision in the issue and a short note in docs. Create follow-up issue only if user demand appears. (Satisfies the AC "Decision recorded...")

8. **WritePolicy on direct api fns:** No change. Only `tidy` takes `&WritePolicyOptions`. Other high-level fns (append, replace_text, doc_*, md_*) intentionally default for ergonomics and backward compat of the library surface. Users who need full policy + guard can use a 1-element plan via `execute_plan` or the lower-level `write` + `atomic` APIs. Document this clearly in api.rs module docs. (Satisfies the optional AC.)

9. **literal in plan:** Add the field for full parity with CLI. Execution logic updated to escape when literal (consistent with current !regex behavior + CLI rules).

10. **Testing strategy:** 
    - Unit tests for new collection paths and flag parsing.
    - Add a shared test helper that sets up a tree with .gitignore + .blineignore + excluded files + globs.
    - Parity test function that runs the identical query 4 ways (api::search_directory, CLI via collect+format or subprocess?, plan via execute_plan_direct or tx helper, MCP via test client) and asserts equivalent SearchResult / TxSearch* data (or counts).
    - Exercise max_results + assert_count combinations.
    - Existing api ignore tests + cmd/search tests must continue to pass.
    - Run under both full features and `ast,files` library matrix.
    - Update schema tests and mcp tool list test.

11. **Docs & generated:** Update SearchArgs after_help, api.rs examples if needed, lib.rs if any drift, schema examples. After code: `make sync-patchloom-md && make check-patchloom-md`. Reference docs if markers exist for search.

12. **No behavior change:** Default (no new flags) must produce identical output, counts, exit codes, json shapes as before.

## Phased Implementation (commit per phase or logical group; test after each)

**Phase 0: Setup & verification baseline (no functional change)**
1. Read #821 in full + this plan + the 811-815 plan + relevant code.
2. Run baseline:
   ```
   cargo test --no-default-features --features "ast,files" --lib -- --quiet
   cargo test --lib --all-features search -- --quiet
   make check-fast 2>&1 | tail -20
   ```
3. Capture current search help and `patchloom schema | grep -A 30 '"search"'`.
4. Create branch `fix/821-search-parity-YYYYMMDD`.
5. Add any missing small test fixtures if needed.

**Phase 1: GlobalFlags + collection unification (foundation)**
1. In `src/cli/global.rs`:
   - Add `exclude: Vec<String>` and `ignore_file: Vec<String>` with proper `#[cfg_attr(feature = "cli", arg(...))]` after the `glob` field.
   - Update doc comments.
2. In `src/files.rs`:
   - Inside `collect_file_paths_opts` (after hidden handling, before parallel build): loop `for name in &global.ignore_file { builder.add_custom_ignore_filename(name); }`
   - After the `Ok(collected...)` line, capture into mut paths, then if !global.exclude.is_empty() { build GlobSet and retain !match; } (move the ? handling).
   - Add a small helper if it reduces dup with with_ignores (optional for cleanliness).
   - Keep `collect_file_paths_with_ignores` unchanged (it is the lib primitive).
3. Update any direct GlobalFlags { ... } literals in tests that would break (use ..Default() where possible). The test_default helpers already use .. so they are safe.
4. Add unit test in files or cli/global for the new collection behavior with custom ignore + exclude.
5. Verify replace and tidy still compile and basic tests pass (they will now get the feature "for free").
6. Commit: "feat: add --exclude and --ignore-file to GlobalFlags and wire into collection (foundation for #821)".

**Phase 2: CLI search surface**
1. Add to `SearchArgs` in `src/cmd/search.rs`:
   ```rust
   /// Limit number of detailed matches returned (0 = no limit).
   #[arg(long)]
   pub max_results: usize,
   ```
2. Update `build_matcher` / collect if needed (no).
3. In `collect_matches`: after merging all_matches (and file counts), before sort or after:
   ```rust
   if args.max_results > 0 && !(args.count || args.files_with_matches) {
       all_matches.truncate(args.max_results);
   }
   ```
   (Keep counts from before truncate for count mode.)
4. In `run`: the assert_count path runs on full collection (good). Normal path will see truncated matches for format.
5. Update the `#[command(after_help = "...")]` with an example using the new flags.
6. Update all ~15 literal struct constructions in the `#[cfg(test)] mod tests` to include `max_results: 0,`.
7. Add 2-3 new tests: basic max_results truncation, combined with context, and one with --ignore-file simulation via GlobalFlags.
8. Run `cargo test --lib --all-features cmd::search -- --quiet`.
9. Commit.

**Phase 3: Plan Search + tx execution**
1. In `src/plan.rs` extend the Search variant (add after assert_count):
   ```rust
   #[serde(default)]
   literal: bool,
   #[serde(default)]
   globs: Vec<String>,
   #[serde(default)]
   max_results: usize,
   #[serde(default)]
   exclude_patterns: Vec<String>,
   #[serde(default)]
   custom_ignore_filenames: Vec<String>,
   ```
2. Update the validation function for Search (add literal && regex conflict).
3. Update examples in the big string and in comments.
4. In `src/tx.rs` `execute_search_op`:
   - Destructure the new fields.
   - Early bail if literal && regex.
   - Compute file_paths using (under cfg) `collect_file_paths_with_ignores(resolved, &custom..., &exclude..., false)` for dirs, else fallback.
   - `let glob_matcher = build_glob_matcher(&globs)?; let glob_roots = vec![...];`
   - Then either adapt the loop to filter with glob or use par_process_files (collect the filtered paths first).
   - Apply max_results cap to all_matches before pushing to tx_searches.
   - Keep every line of pending population, binary probe, TxSearchMatch construction, context calc, assert_count logic, and multi-file text formatting exactly.
   - For literal: `let pat = if *literal || !*regex { regex::escape(pattern) } else { pattern.clone() };` then build re from pat.
5. Make sure declared_paths still works (it already matches on Search { path, .. }).
6. Add tests in tx or integration that construct a Plan with new search fields (including custom ignore) and verify via `execute_plan_direct` the report.searches and exit.
7. Commit.

**Phase 4: MCP search_files**
1. Add to `SearchParams` (with docs):
   - globs, exclude_patterns, custom_ignore_filenames, max_results (literal already present).
2. In the `search_files` handler:
   - Construct `global` with the values from p (glob, exclude, ignore_file).
   - In search_args construction: add `max_results: p.max_results,`
3. Update the long `#[tool(description = "...")]` to mention the new options (copy style from issue or library docs).
4. Update the `assert_eq!` for descriptions.get("search_files") in `mcp_lists_expected_tools` to the new string.
5. Add or extend an MCP integration test that calls search_files with exclude/custom and asserts results.
6. Verify with the mcp test binary path.
7. Commit.

**Phase 5: Schema completeness**
1. In `src/schema.rs`, expand the search OperationSchema properties to include every field the struct now has (literal, globs, before_context, after_context, max_results, exclude_patterns, custom_ignore_filenames) plus keep existing.
2. Improve the description to mention "layered ignores via custom_ignore_filenames + exclude_patterns, globs, max_results, literal mode".
3. Add at least one example in the schema using a new field (or keep vec![] and rely on plan.rs examples).
4. Update any schema tests that assert keys for "search" (e.g. the tier or prompt tests).
5. Run `cargo run -- schema --tier strong | grep -A 40 '"search"'` and eyeball.
6. Commit.

**Phase 6: Parity tests + regression safety**
1. Create or extend a test helper (in api tests or a common test mod) that builds a temp dir with:
   - .gitignore
   - .blineignore
   - files that should be excluded by patterns
   - .rs and other files
2. Write a test `search_parity_blineignore` that:
   - Builds identical SearchOptions / equivalent CLI GlobalFlags+SearchArgs / plan Operation::Search / MCP params.
   - Executes via:
     - `api::search_directory`
     - `crate::cmd::search::collect_matches` + inspection of results (full collection + sort by path/line, then truncate for max_results cases)
     - `crate::tx::execute_plan_direct` (or the internal) with a 1-op plan containing the search, inspect report.searches (full matches sorted before any cap)
     - the MCP test client (via `spawn_test_client` + direct `search_files` handler call, as already used in mcp.rs tests) calling search_files and inspecting the CallToolResult text (parse or string match on the returned JSON/text)
   - Asserts same number of results, same paths (modulo order), same columns/contexts where requested, correct application of excludes/custom.
3. Add cases for max_results capping, literal, globs + exclude together, assert_count with limits.
4. Run the full matrix + `cargo test --test integration`.
5. Ensure no change to exit codes or json shapes for old invocations.
6. Commit.

**Phase 7: Documentation, help, generated**
1. Update after_help in SearchArgs with at least one new-flag example (e.g. using `--ignore-file .blineignore --exclude 'target/**' --max-results 50`).
2. Unconditionally update `docs/reference/README.md`:
   - Add or extend `<!-- ref:global-flag:exclude -->` and `<!-- ref:global-flag:ignore-file -->` sections (modeled on the existing glob/files-from markers).
   - Expand the `<!-- ref:tx-op:search -->` section's "Optional fields" list to include the newly added fields (literal, globs, max_results, exclude_patterns, custom_ignore_filenames) plus previously under-documented ones (before_context, after_context) for completeness.
3. Review and lightly update api.rs and lib.rs docs if the "See also CLI" story needs a sentence.
4. `make sync-patchloom-md` (will pick up schema changes for agent rules) and `make check-patchloom-md`.
5. `make update-readme` if test count changes (unlikely).
6. Commit docs changes.

**Phase 8: Secondary decisions + wrap-up**
1. For AST signature rewrite: add a short section or comment in the relevant files and in the issue update. Decision recorded: library-only (specialized). No code change.
2. For WritePolicy: add a clarifying paragraph in `api.rs` module docs near the WritePolicyOptions section. No behavior change.
3. Update the body of #821 with "Implemented in PR #XXX" links (after creation).
4. Run full verification commands (see below).
5. `make fmt`, `make clippy`, etc.
6. Final hygiene: `git status --short` clean (except allowed), explicit adds.

**Phase 9: Reviewer + landing prep**
- Spawn reviewer subagent with prompt: "Review docs/plans/821-....md + all changed code + run verification commands. Confirm every single AC bullet from #821 is satisfied with evidence (grep, test output, file reads). Flag any gaps."
- Address reviewer findings.
- Prepare PR with body containing `Closes #821` + links to the plan.

## Detailed File Change List (expected)

- `src/cli/global.rs`: add two fields + docs.
- `src/files.rs`: enhance `collect_file_paths_opts` for custom ignores + excludes.
- `src/cmd/search.rs`: add max_results to Args, truncate logic, update after_help + all test constructions.
- `src/plan.rs`: extend Search variant + validation + examples.
- `src/tx.rs`: extend destructuring + logic in execute_search_op + new tests.
- `src/cmd/mcp.rs`: extend SearchParams + global construction + SearchArgs + description + test assert.
- `src/schema.rs`: complete search OperationSchema + tests.
- `tests/...` or `src/api.rs` / `src/cmd/search.rs`: new parity tests + fixtures.
- `docs/plans/821-....md` (this file itself).
- Possibly minor doc tweaks in `src/lib.rs`, `src/api.rs`.
- Generated: PATCHLOOM.md (via make).

## Verification Commands (run after every phase + at end)

```bash
# Library only
cargo test --no-default-features --features "ast,files" --lib -- --quiet

# Full
cargo test --lib --all-features -- --quiet 2>&1 | tail -5

# Search focused
cargo test --lib --all-features 'search' -- --quiet

# MCP
cargo test --lib --all-features 'mcp' -- --quiet

# Integration / pty as needed
cargo test --test integration -- --quiet

# Schema output sanity
cargo run -- schema | python3 -c 'import sys,json; s=json.load(sys.stdin); print([o for o in s if o["name"]=="search"])'

# Full gates
make check-fast
make audit-test-hygiene

# Specific parity (once written)
cargo test --lib --all-features search_parity -- --nocapture
```

Also manually:
- `patchloom search --help | grep -E 'exclude|ignore-file|max-results'`
- Create a temp tree with .blineignore and run equivalent searches via CLI, tx plan (echo 'plan' | patchloom tx --apply or dry), and confirm same hits.
- `cargo tree -i clap --no-default-features --features "ast,files"` still clean.

## Risks & Mitigations

- Parallel collection + custom ignore: Mitigated by adding to builder before build_parallel (same as single-threaded with_ignores).
- max_results + count/assert interaction: Document + test explicitly; assert always sees full set.
- Schema drift recurring: The expansion makes it more complete; future fields should follow same pattern.
- Test maintenance for literal SearchArgs ctors: Use search-and-replace + compile; consider adding a `SearchArgs { pattern:.., paths:.., ..SearchArgs::default() }` if we give it Default (or a builder test helper).
- MCP description string test is brittle: We update it deliberately as part of the change.

## Post-Implementation

- Update #821 with PR link(s) and mark ACs.
- If any sub-issues were spun (e.g. for AST), link them.
- Run `/workflow-retro` or equivalent if process lessons.
- Ensure branch hygiene before push.
- PR title: e.g. `feat: full CLI/MCP/plan parity for Bline search ignore layering (#821)`

This plan was written by exhaustively reading the issue, all relevant source (api, files, cmd/search, tx, plan, mcp, schema, global, lib, ast/symbols), existing tests, and prior plans. Every AC is addressed with concrete steps.

