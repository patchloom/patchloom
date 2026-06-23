# Execution Plan for Code-Review Items (Great Design Polish for Patchloom)

Created from the strict code-review feedback.

## Overall Principles (from review)
- Ambitious structural simplification ("code judo").
- No file bloat >1k LOC without strong reason.
- Prefer deleting complexity over rearranging.
- Preserve behavior and public/MCP contracts.
- Use explicit git add, make check before commit -s, full hygiene.
- Verification at each phase.

## Phased Plan

### Phase 1: Decompose the giants (api.rs first)
- 1.1 Create src/api/ directory module.
- 1.2 Split into:
  - mod.rs : docs, types (EditResult, ApplyMode*, Options), shared helpers (make_*, apply_mutation, write_if_apply, ensure, build_edit_result)
  - doc.rs : LoadedDoc, load/finish, all doc_* fns
  - file.rs : all file_* fns
  - md.rs : all md_* + LintIssue reexport
  - replace.rs : replace_text
  - patch.rs : apply_patch*
  - tidy.rs : tidy
  - search.rs : search*
  - read.rs : read
  - plan.rs : parse_plan, execute_plan
- 1.3 Reexports in mod.rs to keep `patchloom::api::foo` identical.
- 1.4 Update internal calls, tests, doctests, lib.rs docs if needed.
- 1.5 Run cargo check --all-features after each submodule.
- 1.6 Full make check before commit.
- Then repeat for cmd/tx.rs (thin to surface) and ops/doc.rs .

### Phase 2: Canonical write story
- Audit all call sites of apply_mutation, write_if_apply, backup_write_files, atomic, fs ops in apply paths.
- Make file_rename use a generalized cross file apply_mutation helper.
- Update md_move_section cross file to use shared.
- Make remaining MCP shims and ast apply paths use the helpers (or tx for atomic).
- Add tests for the unified paths.
- Update docs in api and lib.

### Phase 3: MCP repetition
- Introduce a declarative macro `mcp_tool!` or `register_mcp_tools!` in cmd/mcp.rs.
- Table of (name, description, ParamsType, op_constructor, extra_validates, strict).
- Generate the #[tool] async fn that does check, validate, run_ops.
- Preserve all 32 tool schemas and names.
- Update the expected tools test.
- Remove boilerplate from handlers.

### Phase 4: tx/cmd split polish
- Identify duplicated fns in cmd/tx.rs (build_*, emit_*, run_lifecycle, describe_*, validate_* that are not CLI specific).
- Move pure ones to pub(crate) in src/tx.rs .
- Update cmd/tx.rs to call them or thin wrappers.
- Remove or cfg_attr the dead_code allows.
- Ensure CLI specific (colored output, confirm, args) stay in cmd/tx.

### Phase 5: dead_code / feature hygiene
- Global grep for #[allow(dead_code)] and broad allows.
- Replace with #[cfg_attr(not(feature = "cli"), allow(dead_code))] on specific items.
- Or move code behind cfg(feature = "cli").
- Run make test-library-hygiene and the matrix after changes.
- Aim for 0 broad allows in main paths.
- Use the existing target.

### Phase 6: Code judo audit
- Grep for repeated if/ match on ApplyMode, Operation variants, preview/apply.
- Look for opportunities to collapse using Plan for single ops where possible, or a dispatcher.
- Examples: make preview/apply more uniform in helpers.
- Use more of the Operation model for single file ops.
- Remove unnecessary branches.
- Implement 2-3 high value simplifications.

## Execution Rules
- After each logical change or phase: cargo check, make check-fast + hygiene matrix.
- Explicit: git add src/api/mod.rs src/api/doc.rs ... (only changed).
- make check (full) before git commit -s .
- Update todos.
- At end, full make check, git status clean, push, update PR #832 or new.
- Preserve all tests, behavior, public API, MCP tool contracts.

## Success Criteria
- api.rs < 1500 LOC main, submodules <800.
- All writes visibly use shared helpers.
- MCP handlers dramatically shorter (generated).
- cmd/tx.rs obviously thin.
- 0 or minimal dead_code allows, hygiene target passes without allows.
- Some complexity deleted (fewer branches).
- All gates green.

This plan directly addresses the review to make Patchloom look like a great designed project.
