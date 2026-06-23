# Implementation Plan: Library Embedding Gaps (#792)

**Issue:** https://github.com/patchloom/patchloom/issues/792
**Title:** Library embedding: remaining gaps for bline full adoption of search/edit/plan/file ops (post files + search_directory)
**Goal:** Enable full use of patchloom as a pure library with `default-features = false, features = ["ast", "files"]` (no "cli", no clap bloat) for Bline and other agent embedders. Eliminate duplicate code in consumers.

**Success Criteria (from issue):**
- bline (or equivalent) can `features=["ast","files"]` (drop "cli")
- `patchloom_plan` equivalent (execute_plan) works without pulling clap
- All file mutations (incl. append/prepend) go through patchloom api + guard + backup + WritePolicy
- Full gates + LLM edit tests would pass with delegation (we verify via matrix + tests)
- Line count reduction potential in consumers by dropping custom search/edit/append/plan shims + is_binary etc.
- Docs allow easy adoption.

**Current State (as of 2026-06-23, post #791):**
- "files" feature exists for walker/search (ignore + globset).
- `api::search_directory`, `search`, `is_binary`, `read_text_file`, `par_process_files`, `file_create/delete/rename` public (many gated any(cli,files)).
- `PathGuard::builder()` with `.allow_temp_directory()` for yolo/relax.
- `parse_plan` always available.
- `execute_plan` **only under "cli"** (forces clap).
- No `api::file_append` / `file_prepend` (but Plan has `FileAppend`, doc/md have appends; logic duplicated in cmd/append.rs + cmd/tx.rs).
- `EditResult` single file focused.
- Good guard threading on most mutating api fns.
- lib.rs + api.rs have embedding docs + Guard example, but incomplete for plans/append/files-only.
- tx execution lives in `src/cmd/tx.rs` (behind cli mod).

**Feature Decisions:**
- Use existing "files" as the "pure library file ops + tx execution" feature.
- `cli` will continue to enable "files".
- Gate `execute_plan` on `any("cli", "files")`.
- No new feature flag unless needed (keep simple).
- GlobalFlags will be made usable from library tx path (struct already has cfg_attr; cli/ module is always declared).

**Risks & Mitigations:**
- Refactoring tx execution: risk of breakage in CLI path or atomicity. Mit: move code, keep tests, run matrix, use existing integration/pty for CLI.
- Dup logic for append: centralize helper.
- Dep bloat: verify with `cargo tree` no clap in files+ast build.
- API stability: new fns follow existing patterns (EditResult, ApplyMode, guard).
- Docs: keep doctests compiling/runnable with feature combos (use `rust,ignore` or test in cfg).
- Scope creep (AST sig edits): implement core first, add minimal helper or docs; full AST sig rewrite may be follow-up.

**Testing Strategy (critical, run after every significant change):**
1. Feature matrix builds/tests:
   - `cargo check --no-default-features --features files`
   - `cargo check --no-default-features --features "ast,files"`
   - `cargo check --no-default-features --features "ast,files,mcp"`
   - `cargo test --no-default-features --features "ast,files" --lib`
   - `cargo test --no-default-features --features "files" --lib`
2. Full CLI paths unaffected:
   - `cargo test --lib --all-features`
   - `make test` (or `cargo test --lib --all-features`)
   - `make integration-test`
   - `make pty-test` (if interactive)
3. New/updated unit tests:
   - In `src/api.rs` `#[cfg(test)]`: tests for file_append, file_prepend (preview/apply/check, guard success/fail, empty files, nl handling, WritePolicy via config?).
   - Tests for execute_plan under `#[cfg(any(feature="cli", feature="files"))]`.
   - Guard + append in relaxed mode (temp dir).
   - Plan containing FileAppend using execute_plan.
4. Doc tests:
   - `cargo test --doc --no-default-features --features "ast,files"`
   - Update examples in docs to be testable where possible.
5. Dep verification (no clap bloat):
   - `cargo tree -i clap --no-default-features --features "ast,files" 2>&1 | cat` (expect no user dep or only indirect dev).
   - Same for other heavy.
6. Hygiene + full gate:
   - `make fmt-check`
   - `make clippy` (all targets/features)
   - `make check-fast` (or full `make check` before commits)
   - `make audit-test-hygiene`
7. Cross file / plan scenarios:
   - Test plans with mix: append + doc + rename + search.
   - Verify rollback on error.
8. Edge cases:
   - Append to file without final nl.
   - Prepend.
   - Non-existing for append (error).
   - Binary? (but append assumes text like others).
   - With respect_editorconfig etc if possible.
9. For consumers: the tests + matrix simulate "bline" usage.
10. After changes: run full `make check` , fix any.

**Implementation Phases (detailed steps, commit per logical unit or small groups):**

**Phase 1: Refactor tx/plan execution for "files" (ungate execute_plan)**
- Extract core execution from `src/cmd/tx.rs` into new `src/tx.rs` (or `src/plan/executor.rs` for organization; prefer flat `src/tx.rs` for simplicity like other modules).
  - Move: TxState, execute_and_collect, commit_changes, run_lifecycle, rollback_*, build_*_tx_output, make_error_json*, validate_and_prepare_plan, execute_file_op, execute_op, declared paths handling, etc.
  - Keep in cmd/tx.rs: TxArgs struct, run fn for CLI, any clap/CLI output formatting specific to command.
  - cmd/tx.rs will `use crate::tx;` or reexport the direct fn.
- Make `src/tx.rs` compiled with `#[cfg(any(feature = "cli", feature = "files"))] pub(crate) mod tx;` in lib.rs (or appropriate).
- Update `src/cmd/mod.rs` (still cli only) to continue declaring mod tx; (the CLI one).
- Update api.rs: change `execute_plan` to `#[cfg(any(feature = "cli", feature = "files"))]`, update doc comment (remove "requires cli", mention "files" for library), call `crate::tx::execute_plan_direct(...)`.
- Update all `#[cfg(feature = "cli")]` tests for execute_plan in api.rs and tx tests to `#[cfg(any(feature = "cli", feature = "files"))]`.
- Ensure GlobalFlags simulation works (it uses defaults + config load, which should be fine; GlobalFlags struct must be visible - it will be via cli::global even without cli feature for the data).
- Handle imports: move necessary uses (but avoid clap in the new tx.rs core).
- If verbose! or other need, ok (always).
- Add/update unit tests that execute plans with FileAppend etc under the new cfg.
- Verify atomicity, guard upfront check, rollback, json output same.
- Commit: "refactor: extract tx execution engine to support library 'files' feature (#792)"

**Phase 2: Add api::file_append and api::file_prepend**
- In `src/api.rs`, add:
  ```rust
  pub fn file_append(
      path: &Path,
      content: &str,
      mode: ApplyMode,
      guard: Option<&PathGuard>,
  ) -> anyhow::Result<EditResult> { ... }
  ```
  Similar for `file_prepend`.
- Follow exact pattern of `file_create` / `file_delete`:
  - Compute original vs new (read, concat with nl rule).
  - For Preview/Check: return EditResult with diff? (see how others do; some return diff only on apply?).
  - For Apply: ensure_contained, BackupSession, atomic_write with policy (use make_write_policy or from global sim), finalize backup.
  - Support WritePolicy? The high level api fns currently use defaults or respect? Look at file_create: it builds policy?
  - Use `unified_diff` if needed for result.diff.
  - Handle errors same (not file, etc).
- Extract shared pure helper (to avoid logic dup with plan and append cmd):
  - e.g. in `src/ops/file.rs` (create if needed) or in api or write:
    `pub fn append_content(existing: &str, append: &str) -> String { ... }`
    `pub fn prepend_content(...)`
  - Update tx.rs execute_file_op to use `ops::file::append_content(...)` + update_pending.
  - Update cmd/append.rs apply_append and run logic to use the shared (or keep for CLI specific stdin, but share compute).
- For api version, also support the full WritePolicyOptions? For now, use defaults like other file_* , or expose via existing. Current file_create uses basic. To match plan/CLI, load config? For simplicity, start with default policy + note; later enhance if needed. (See file_create impl for exact.)
- Add the fns to be always (no extra cfg, like other file_*).
- Update Plan handling if needed (it will use shared now).
- Add tests in api tests for both fns: create temp, append/prepend text, check result.changed/applied, on-disk for apply, guard reject, preview no write, nl behavior, empty file cases.
- Update any schema or cmd if needed for consistency.
- Commit: "feat: add api::file_append and api::file_prepend for library use (#792)"

**Phase 3: Docs, examples, re-exports, misc gaps**
- Update `src/lib.rs`:
  - Feature table: note that "files" enables search_directory + plan execution + file append etc for library.
  - Expand embedding section with:
    - Recommended for agents: `features = ["ast", "files"]`
    - Full example: build PathGuard relaxed, parse_plan or manual Plan with FileAppend, execute_plan, check result json or code.
    - file_append example.
    - search_directory example (already partial).
    - Migration notes.
  - Mention no clap pulled.
- Update `src/api.rs` top docs: mention execute_plan, file_append, guard for appends, files feature.
- Add more Guard + yolo + append examples/tests.
- Re-exports: in lib.rs or api, consider `pub use plan::Plan; pub use api::{EditResult, ApplyMode, ...};` for ergonomics (already mostly via api:: ).
- For search gap: update docs to say "for advanced ignore (e.g. blineignore) layer on top of search_directory results or use files::collect_file_paths + custom". Expose if easy (e.g. make build_glob_matcher or walker public more? files already has some).
- For EditResult / cross-file: enhance EditResult with `op: &'static str` or `kind: String` ? Or add `related_paths: Vec<String>` . For now, add `action: String` to EditResult if makes sense, document that for plans use the JSON from execute_plan which has multi. Or leave and doc limitation (issue #757 follow-up).
- For AST: add a starter in `src/ast.rs` or api, e.g. `pub fn replace_function_signature(...)` basic for Rust (using parse + replace child). Or simple doc + example of current structural replace. To keep complete, implement a minimal safe signature update helper using tree-sitter for Rust (visibility, name, params stub). Test it.
- Guard semantics docs: ensure append paths document write-time only.
- Update any other docs (README? PATCHLOOM.md via make later?).
- Commit(s): "docs: expand library embedding for 'ast'+'files' + plans + append (#792)"

**Phase 4: Testing, verification, polish**
- Implement all test strategy items.
- Add cfg-gated tests.
- Run matrix commands, capture output in thinking.
- Fix any compile/test issues under no-cli.
- `cargo tree` verification.
- Audit: grep for places that may assume cli for file ops.
- Ensure all mutations in api/plan go through guard + atomic + policy (for append new, yes).
- Run `make audit-test-hygiene`, address if new weak asserts introduced (avoid bare success in new tests).
- Commit: "test: add library feature matrix tests and coverage for new apis (#792)"

**Phase 5: Final gate + release prep**
- Full `make check`
- `make fmt`
- Update CHANGELOG.md with entry under Unreleased.
- Ensure no new warnings.
- Verify on "files"+"ast" the bline-like flow works in a test.
- Branch hygiene, clean tree.

**Order of work (take time, small steps):**
1. Research complete (done in session).
2. Create branch `feat/792-pure-library-embedding`.
3. Phase 1 refactor + test locally under matrix.
4. make check, commit explicit.
5. Phase 2 append fns + shared helper + tests.
6. commit.
7. Phase 3 docs.
8. Phase 4 tests full.
9. Full make check.
10. Push, PR create with detailed body referencing plan + issue.
11. Enable auto-merge (gh pr merge --auto or equivalent for repo).
12. Spawn reviewer subagent.
13. Monitor/fix if reviewer finds issues (but since "then run", after push).

**Branch name:** `feat/792-library-embedding-20260623` (dated unique).

**PR title/body:** "feat: complete pure-library support for embedders (execute_plan, file_append, docs) (#792)"

**Auto merge:** After create, `gh pr merge <N> --auto --squash` ? Use appropriate for repo (they use release-please etc, but follow "with auto merge").

**Post PR:** Use spawn_subagent for reviewer (prompt: review the PR diff, check plan compliance, tests, docs, feature matrix, no cli dep, etc. Use read tools).

**Rollback plan:** If refactor complex, can gate temporarily or revert.

**Time/Recheck:** After each phase: re-read changed files, run targeted cargo test --no-default... , full make check before commit, git status clean, explicit adds only.

**Files likely touched:**
- Cargo.toml (maybe minor)
- src/lib.rs
- src/api.rs (main)
- src/tx.rs (new)
- src/cmd/tx.rs (thin)
- src/cmd/append.rs (share logic)
- src/ops/ (new file.rs ?)
- src/plan.rs (minor)
- tests/...
- docs/plans/792-... (this plan)
- CHANGELOG.md
- Possibly src/ast.rs , src/files.rs

**Verification commands (to run repeatedly):**
```bash
cargo check --no-default-features --features "ast,files"
cargo test --no-default-features --features "ast,files" --lib -- --quiet
cargo tree -i clap --no-default-features --features "ast,files"
make fmt-check
cargo clippy --all-targets --features "ast,files" -- -D warnings
make check-fast
# after full:
make check
```

This plan is complete and detailed. Follow phases sequentially, recheck at each gate.
