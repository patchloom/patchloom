# Implementation Plan: Bline Library Follow-ups (#811–#815)

**Date:** 2026-06-23
**Related:** #792 (library embedding), #794–#802, #805 (prior Bline polish), PRs #808–#810 (landed)
**Goal:** Complete the library surface so Bline (and similar agent hosts) can use `default-features=false, features=["ast","files"]` with high-quality, typed, ergonomic APIs for plans, search, and ignores — without duplicating logic or suffering awkward (code, json) or private helpers.

## Issues Summary & Success Criteria

### #811: Library API: make execute_plan return typed PlanReport (not (u8, String))
- **Current:** `api::execute_plan(...) -> Result<(u8, String)>`; users must `serde_json::from_str` to get `PlanReport` (alias TxOutput).
- **Success:**
  - `pub fn execute_plan(plan, cwd, guard) -> Result<PlanReport>`
  - `PlanReport` (TxOutput) is the primary return for library.
  - CLI/MCP paths continue to work (they can serialize the report or use a thin adapter).
  - All existing api tests, integration plan tests, docs examples updated.
  - Library matrix (`--no-default --features "ast,files"`) + doc-tests pass.
  - Docs show direct `let report = execute_plan(...) ?;` usage + note on `report.ok`, `status`, `changes`, `searches` etc.
  - Backward compat note in changelog + lib.rs (or small `execute_plan_with_code` if needed; prefer clean break for 0.5 era).

### #812: Search: expose formatting helper and per-file search primitive for library / agent hosts
- **Current:** `api::search` (per-file, basic `Vec<SearchMatch>`), `api::search_directory` (rich `Vec<SearchResult>` with context/column). Formatting (`format_results`) is private in `cmd/search.rs`.
- **Success:**
  - Public `api::format_search_results(results: &[SearchResult], color: bool, json: bool /* or better options */) -> String` (or two fns: human + structured).
  - Per-file rich primitive exposed or `search` enhanced to support context (or new `search_file` that returns `SearchResult` for single file).
  - Agents can get CLI-like output or JSON without internal knowledge.
  - Tests for formatter (human, jsonl, files-with-matches, count, context).
  - No behavior change for CLI `search` command.
  - Reused by any future MCP or other.

### #813: Search / files: first-class helper for multi-source ignore precedence (global + .blineignore + runtime patterns)
- **Current:** `SearchOptions { exclude_patterns, custom_ignore_filenames }` + internal WalkBuilder in `search_directory`. `files::collect_file_paths` is basic (only .gitignore + hidden).
- **Success:**
  - Public helper e.g. `files::collect_file_paths_ignoring(root: &Path, custom_ignore_files: &[&str], exclude_patterns: &[&str], include_hidden: bool) -> Result<Vec<PathBuf>>` or one that takes `&SearchOptions`.
  - Or `api::build_search_file_list(root, &SearchOptions)`.
  - `search_directory` refactored to use the shared helper.
  - Library users (Bline) can do custom processing + `par_process_files` with the same ignore rules as `search_directory`.
  - Tests cover precedence: .gitignore + .blineignore + runtime exclude + globs.
  - Docs example for "layered ignores".

### #814: Docs + ergonomics: keep version strings in sync + improve search error messages for library users
- **Current:** Cargo.toml=0.4.0, but lib.rs/README examples say "0.5". Search errors: "must not be empty", "requires the 'files' feature..." (a bit terse for lib consumers).
- **Success:**
  - Version references in docs/README/lib.rs examples are consistent (use a single source or match current release-please intent; at minimum no 0.3-era drift).
  - Search errors improved: clearer ("search pattern must not be empty"), better feature-gate message ("search_directory requires the 'files' feature (enable it for pure-library directory search)"), include path in some errors.
  - No new hard-coded versions that will drift (consider `env!("CARGO_PKG_VERSION")` where runtime strings are produced; docs can note "latest" or pin to release manifest).
  - `make check` (incl check-readme etc) passes.

### #815: Search: expose richer SearchMatch types and reusable context builder for consistent output across CLI and library
- **Current:** Duplicated types (`api::SearchMatch` simple, `api::SearchResult` rich, `TxSearchMatch`/`TxSearchResult`, CLI internal `SearchMatch`, `ast/search.rs` own). `build_context_lines` private in api.rs.
- **Success:**
  - Rich types are public/canonical (promote `SearchResult` + a `SearchMatch` with `column`, `context_*` as the main; keep simple `SearchMatch` or alias for backward in `api::search`).
  - `pub fn build_context_lines(all_lines: &[&str], match_idx: usize, ctx: usize) -> (Vec<String>, Vec<String>)` (or in `files`).
  - Tx* types for plans stay consistent or documented as "plan-shaped".
  - CLI `cmd/search` can eventually delegate to shared formatter using the public rich types.
  - Tests assert context/column in library search results.
  - No duplication of context logic.

## Overall Success Criteria (cross-cutting)
- Bline can drop custom search/edit/ignore/plan code and use patchloom primitives directly.
- All under `default-features = false, features = ["ast", "files"]` (and with mcp).
- `cargo tree -i clap --no-default-features --features "ast,files"` → no clap.
-  `make check-fast` (includes `test-library-hygiene`, fmt-check, clippy, tests, pty, audit-test-hygiene).
- Full `cargo test --no-default-features --features "ast,files" --lib && cargo test --doc --no-default-features --features "ast,files"`.
- Integration + existing plan/search tests continue to pass under full features.
- Updated embedding docs in `src/lib.rs`, `src/api.rs`, `docs/reference/README.md`, README.md.
- New tests for every new public surface (typed plan, formatter, ignore helper, context builder, better errors).
- `make audit-test-hygiene` clean.
- No new dead_code under library build (or properly `#[cfg_attr(...)]`).
- PR title is single conventional type (e.g. `feat: ...` or `fix: ...` or `test: ...`); body links all 5 issues.
- Auto-merge enabled on the PR.
- Reviewer subagent run at end (fed the 5 issues) reports high confidence all ACs met.

## Current State (post #810)
- `execute_plan` gated on cli|files, returns tuple, PlanReport reexported + used in docs/tests.
- Search: `SearchOptions` has exclude + custom_ignore (from #796). `search_directory` + `search` (basic) public. `SearchResult` reexported. `build_context_lines` private. Formatter private.
- Ignore: logic inside api + basic `files::collect_file_paths`.
- Types: several Search* variants.
- Versions: drift between Cargo (0.4) and docs ("0.5").
- Guard/WritePolicy/append/PlanReport already good from prior.
- Lots of tests in `src/api.rs` (cfg any(cli,files)) and integration for tx plans.

## Design Decisions
- **#811 return type:** Change only the high-level `api::execute_plan` (and its direct) to return `PlanReport`. Core execution builds the struct; serialization to JSON + code extraction happens only at CLI/MCP boundaries or via a small `fn plan_report_to_exit_json(report: &PlanReport) -> (u8, String)`.
  - Add helper if needed: `impl PlanReport { pub fn exit_code(&self) -> u8 { ... } }` (map status/error_kind).
  - Update `execute_plan_direct` to return `Result<PlanReport>` (pub(crate) ok).
  - CLI tx command: call core, serialize for output, use report status or existing logic for exit.
  - This eliminates the deserialize dance for pure lib users.
- **Search types (#815):** Make `SearchResult` the rich canonical for directory. Keep/enhance `SearchMatch` (add column if missing in simple). Export `build_context_lines`. Document that plan search results use `TxSearch*` (which can be made consistent or users deserialize to common shape).
- **Formatting (#812):** Add `pub fn format_search_results(results: &[SearchResult], opts: &SearchFormatOptions) -> String` in api (or files). `SearchFormatOptions { color: bool, json: bool, files_with_matches: bool, count_only: bool, ... }` or simpler overloads. Make CLI use it.
- **Ignore helper (#813):** Add in `src/files.rs` (behind files|cli cfg):
  ```rust
  pub fn collect_file_paths_with_ignores(
      root: &Path,
      custom_ignore_filenames: &[String],
      exclude_patterns: &[String],
      include_hidden: bool,
  ) -> anyhow::Result<Vec<PathBuf>>
  ```
  Or accept `&SearchOptions` (simpler). Refactor `search_directory` to use it + par_process_files. Expose `relative_display` already is. Document precedence: .gitignore (via WalkBuilder) + custom files + exclude globs (post filter).
- **Per-file primitive (#812):** Enhance or add `pub fn search_file(path: &Path, pattern: &str, opts: &SearchOptions) -> Result<Vec<SearchResult>>` (reusing the one-file logic). The existing `search` stays for the absolute simplest case (no context). Make `search` call into richer when context=0.
- **Versions & errors (#814):** 
  - In docs: change examples to use `version = "0.5"` only if that's the release target; or add a CI step later. For now, make sure no older versions (0.3) linger. Use `env!("CARGO_PKG_VERSION")` for any runtime "version" strings (e.g. in agent-rules output).
  - Errors: update `bail!` in search_directory and search to be more helpful for lib users (mention feature flag where relevant).
- **Feature gating & hygiene:** Everything new behind `#[cfg(any(feature = "cli", feature = "files"))]`. Use the `test-library-hygiene` target. Run `cargo clippy --no-default-features --features "ast,files" -- -D warnings` explicitly.
- **No behavior change for CLI** (output, exit codes, jsonl etc must be identical).
- **Docs & examples:** Update lib.rs "Embedding" section with new typed plan example, search formatter, ignore helper example, richer search types. Update reference docs if markers exist.
- **Breaking:** These are additive or improvements in the library API era (post 0.4 refactor). Note in CHANGELOG + lib.rs "Migration from tuple returns".

## Phased Implementation (commit per logical unit, test after each)

**Phase 0: Setup (no code change)**
- This plan doc.
- Update todos.
- Confirm matrix baseline (run the commands, note counts).
- Branch: `fix/bline-811-815-library-ergonomics-YYYYMMDD` (unique).

**Phase 1: #811 Typed execute_plan (core)**
1. In `src/tx.rs`: Change `execute_plan_direct` (and helpers that return tuple) to primarily build/return `TxOutput` / `PlanReport`. Keep serialization in a small helper `fn tx_output_to_cli_result(output: TxOutput) -> (u8, String)` used by error/success paths.
   - Update `build_full_tx_output`, error builders to produce struct.
   - Determine exit code from struct or keep mapping fn.
2. In `src/api.rs`: Change `execute_plan` sig to `-> Result<PlanReport>`. Delegate and return the struct.
3. Update all call sites:
   - api tests (the  execute_plan_* tests).
   - lib.rs docs examples.
   - cmd/tx.rs and places that called direct for CLI (adapt to serialize if still needed).
   - mcp if it calls the tuple version (update to use struct where possible, or keep compat adapter).
4. Add `PlanReport::exit_code(&self) -> u8` or compute in wrapper.
5. Test immediately: `cargo test --no-default-features --features "ast,files" --lib` (the plan tests), full matrix, existing integration that exercises tx plans.
6. Update docs in api.rs + lib.rs showing `let report: PlanReport = execute_plan(...) ?;`

**Phase 2: Search types & context builder (#815) + per-file primitive (#812 part)**
1. Make `build_context_lines` public in `src/api.rs` (or move to `src/files.rs` as `pub fn build_search_context(...)`).
2. Enhance `SearchMatch` or ensure `SearchResult` is rich and public (it already has column, contexts).
3. Add/expose `pub fn search_file(path: &Path, pattern: &str, opts: &SearchOptions) -> Result<Vec<SearchResult>>` that uses the internal one-file logic + context.
4. Re-export richer types if needed from lib.rs.
5. Update tests to cover column/context in single file case.
6. Run library tests for search.

**Phase 3: Formatting helper (#812)**
1. Add in `src/api.rs` (or new `src/search.rs` module):
   ```rust
   #[derive(Default)]
   pub struct SearchFormatOptions { pub color: bool, pub as_json: bool, ... /* files_with_matches, count */ }
   pub fn format_search_results(results: &[SearchResult], opts: &SearchFormatOptions) -> String { ... }
   ```
2. Port logic from `cmd/search.rs::format_results` (human text with colors, json, jsonl, count, files-with-matches, context -- lines).
3. Make CLI `format_results` delegate to the new public one (adapt internal types or convert).
4. Add tests in api tests for the formatter (various modes).
5. Ensure output byte-for-byte matches old CLI where applicable (use existing pty or assert_cmd tests).

**Phase 4: Ignore precedence helper (#813)**
1. In `src/files.rs` (gated):
   - Extract/refactor the WalkBuilder + custom_ignore + post-exclude logic into reusable:
     `pub fn collect_file_paths_ignoring(root: &Path, opts: &api::SearchOptions, include_hidden: bool) -> Result<Vec<PathBuf>>`
     (or without depending on api by duplicating small struct or using the fields).
   - Make `par_process_files` users able to pass the list.
2. Refactor `api::search_directory` (the cfg block) to use the new helper + existing par + `search_one_file_for_api`.
3. Expose the helper in lib.rs reexports under files.
4. Add/update test for custom layered ignores (already one exists, make sure helper is tested directly).
5. Document precedence clearly.

**Phase 5: Polish #814 + cross updates**
1. Version strings: audit all hard-coded:
   - lib.rs, README.md, docs/..., any generated.
   - Make consistent (align to 0.5 for the upcoming release or use a single const in a build script if possible; simplest: update comments and ensure release process keeps them).
   - If runtime version printed (agent-rules?), use `env!("CARGO_PKG_VERSION")`.
2. Improve search errors:
   - "search pattern must not be empty"
   - Feature gate: "search_directory requires the 'files' feature to be enabled for directory search"
   - Add path context where helpful.
3. Run full `make check-fast`, fix any drift.
4. Update embedding docs to showcase new helpers (typed plan, formatter, ignore helper, search_file, context builder).

**Phase 6: Verification & Hygiene**
- Run after every phase + final:
  1. `cargo test --no-default-features --features "ast,files" --lib`
  2. `cargo clippy --no-default-features --features "ast,files" -- -D warnings`
  3. `cargo test --doc --no-default-features --features "ast,files"`
  4. `cargo tree -i clap --no-default-features --features "ast,files"`
  5. `make check-fast` (full gate)
  6. `make audit-test-hygiene`
  7. Relevant integration/pty if changed (search or tx).
- Grep for old tuple usage in docs/tests/examples.
- Update any agent drivers or reference if they hardcode.
- `make fmt`
- Explicitly verify no new unwraps in lib code, no short contains in new tests, etc.

**Phase 7: Docs, changelog, release notes prep**
- Update lib.rs "Note on results", embedding section with new examples.
- Add to CHANGELOG (or let release-please).
- If needed, `make sync-patchloom-md` (but only if command changes).
- Update this plan with "implemented in PR #N".

## Testing Requirements (non-negotiable)
- Every new public fn has unit test under `#[cfg(any("cli","files"))]` in the module or api tests.
- Library-specific tests exercise the typed path, formatter, ignore helper, search_file, context builder directly (no CLI).
- Cross-check: run same scenario via CLI `search` / `tx` and via api, assert equivalent data (not just string).
- Error paths tested (bad glob, empty pattern, missing files feature in dir search).
- Guard still works with new paths.
- Precedence test for ignores covers .gitignore + custom + exclude + glob.
- Performance not regressed (par_process still used).
- Full matrix + `make check` before any commit.

## Risks & Mitigations
- Breaking API change (#811): Mitigate with clear migration in docs + since it's the point of the library push, acceptable in this phase. Provide example of old vs new.
- Output parity for search formatter: Mit by having CLI delegate and adding golden tests or direct comparison in CI.
- Duplication of ignore logic: Centralize in files.rs.
- Version drift forever: For this PR, fix visible ones; suggest adding a test or make target that greps for version strings.
- Scope: Stick to the 5 issues; no unrelated refactors.

## Post-Implementation
- Run full reviewer subagent, giving it the 5 issue URLs + this plan + diffs.
- Branch hygiene: unique branch, explicit `git add file1 file2`, `git commit -s`, `git push`.
- Create PR with title `feat: Bline library follow-ups (#811-#815)` (single type).
- Body: link all issues, summary of changes, evidence of tests/matrix.
- Immediately `gh pr ready` + `gh pr merge --auto --squash` (or enable auto-merge).
- Monitor CI.
- Update patchloom-contrib skill with summary.
- Clean any temp branches.

## Verification Commands (run and capture)
```bash
cargo test --no-default-features --features "ast,files" --lib -- --quiet
cargo clippy --no-default-features --features "ast,files" -- -D warnings
cargo test --doc --no-default-features --features "ast,files" -- --quiet
cargo tree -i clap --no-default-features --features "ast,files"
make check-fast
make audit-test-hygiene
# specific
cargo test --test integration -- --quiet  # or filtered
```

## References
- Prior plan: docs/plans/792-library-embedding.md
- Landed PRs: #808, #809, #810
- Current issues: #811 to #815
- Key files: src/api.rs, src/tx.rs, src/files.rs, src/cmd/search.rs, src/lib.rs, tests/integration.rs, Makefile

This plan is complete, phased for incremental safe progress with tests after each logical piece.
